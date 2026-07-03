// Soak-tests the mai2 touch-stream HID and button HID concurrently for a fixed
// duration, watching for disconnects (reopens), firmware-side frame drops,
// stream-sequence gaps (missed frames), and inter-packet stalls.
//
//   cargo run --example hid_soak -- --duration-ms=60000

use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use affine_core::AFFINE_VID;
use hidapi::HidApi;

const MAI2_PIDS: [u16; 2] = [0x52A5, 0x52A6];
const USAGE_PAGE_TOUCH: u16 = 0xFF00;
const USAGE_TOUCH: u16 = 0x0031;
const TOUCH_REPORT_ID: u8 = 0x31;
const TOUCH_REPORT_LEN: usize = 64;
const USAGE_PAGE_BUTTONS: u16 = 0xFFCA;
const USAGE_BUTTONS: u16 = 0x0001;

fn find_path(api: &HidApi, pid: u16, usage_page: u16, usage: u16) -> Option<CString> {
    api.device_list()
        .find(|i| {
            i.vendor_id() == AFFINE_VID
                && i.product_id() == pid
                && i.usage_page() == usage_page
                && i.usage() == usage
        })
        .map(|i| i.path().to_owned())
}

#[derive(Default)]
struct TouchStats {
    packets: u64,
    complete_frames: u64,
    part0: u64,
    part1: u64,
    nonzero: u64,
    fw_dropped_max: u16,
    seq_gaps: u64,
    seq_missed: u64,
    reopens: u64,
    open_fails: u64,
    max_gap_ms: u64,
    elapsed_s: f64,
}

#[derive(Default)]
struct ButtonStats {
    reports: u64,
    btn_transitions: u64,
    nonzero: u64,
    seq_gaps: u64,
    seq_missed: u64,
    reopens: u64,
    open_fails: u64,
    max_gap_ms: u64,
}

/// Returns (stream_seq, part_index, nonzero, firmware_dropped_frames).
fn parse_touch(report: &[u8]) -> Option<(u8, u8, bool, u16)> {
    let start = if report.first().copied() == Some(TOUCH_REPORT_ID) {
        0
    } else if report.len() > 1 && report[1] == TOUCH_REPORT_ID {
        1
    } else {
        return None;
    };
    if report.len().saturating_sub(start) < TOUCH_REPORT_LEN {
        return None;
    }
    let d = &report[start..start + TOUCH_REPORT_LEN];
    if d[1] != 1 || d[2] != 1 || d[5] != 2 || d[4] >= 2 || (d[12] & 0x04) == 0 {
        return None;
    }
    let nonzero = d[50..55].iter().any(|&b| b != 0);
    let fw_dropped = u16::from_le_bytes([d[55], d[56]]);
    Some((d[3], d[4], nonzero, fw_dropped))
}

fn touch_soak(pid: u16, deadline: Instant, stop: Arc<AtomicBool>) -> TouchStats {
    let start = Instant::now();
    let mut st = TouchStats::default();
    let mut last_packet = Instant::now();
    let mut seen_mask = 0u8;
    let mut active_seq = 0u8;
    let mut have_seq = false;
    let mut prev_complete_seq = 0u8;
    let mut first_open = true;

    'outer: while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
        let Ok(api) = HidApi::new() else {
            st.open_fails += 1;
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        let Some(path) = find_path(&api, pid, USAGE_PAGE_TOUCH, USAGE_TOUCH) else {
            st.open_fails += 1;
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        let Ok(hid) = api.open_path(&path) else {
            st.open_fails += 1;
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        if !first_open {
            st.reopens += 1;
        }
        first_open = false;

        let mut buf = [0u8; TOUCH_REPORT_LEN + 1];
        loop {
            if Instant::now() >= deadline || stop.load(Ordering::SeqCst) {
                break 'outer;
            }
            match hid.read_timeout(&mut buf, 100) {
                Ok(0) => {}
                Ok(n) => {
                    if let Some((seq, part, nonzero, fw_dropped)) = parse_touch(&buf[..n]) {
                        let gap = last_packet.elapsed().as_millis() as u64;
                        if gap > st.max_gap_ms {
                            st.max_gap_ms = gap;
                        }
                        last_packet = Instant::now();
                        st.packets += 1;
                        if part == 0 {
                            st.part0 += 1;
                        } else {
                            st.part1 += 1;
                        }
                        if nonzero {
                            st.nonzero += 1;
                        }
                        if fw_dropped > st.fw_dropped_max {
                            st.fw_dropped_max = fw_dropped;
                        }
                        if !have_seq || active_seq != seq {
                            active_seq = seq;
                            seen_mask = 0;
                            have_seq = true;
                        }
                        seen_mask |= 1u8 << part;
                        if seen_mask == 0x03 {
                            st.complete_frames += 1;
                            if st.complete_frames > 1 {
                                let expected = prev_complete_seq.wrapping_add(1);
                                if seq != expected {
                                    st.seq_gaps += 1;
                                    st.seq_missed += seq.wrapping_sub(expected) as u64;
                                }
                            }
                            prev_complete_seq = seq;
                            seen_mask = 0;
                        }
                    }
                }
                Err(_) => break, // treat as disconnect -> reopen loop
            }
        }
    }
    st.elapsed_s = start.elapsed().as_secs_f64();
    st
}

fn button_soak(pid: u16, deadline: Instant, stop: Arc<AtomicBool>) -> ButtonStats {
    let mut st = ButtonStats::default();
    let mut last_packet = Instant::now();
    let mut first_open = true;

    'outer: while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
        let Ok(api) = HidApi::new() else {
            st.open_fails += 1;
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        let Some(path) = find_path(&api, pid, USAGE_PAGE_BUTTONS, USAGE_BUTTONS) else {
            st.open_fails += 1;
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        let Ok(hid) = api.open_path(&path) else {
            st.open_fails += 1;
            thread::sleep(Duration::from_millis(200));
            continue;
        };
        if !first_open {
            st.reopens += 1;
        }
        first_open = false;

        // report layout: [button_bits0, button_bits1, seq_lo, seq_hi, ...]
        let mut buf = [0u8; 64];
        let mut last_btn: Option<(u8, u8)> = None;
        let mut last_seq: Option<u16> = None;
        loop {
            if Instant::now() >= deadline || stop.load(Ordering::SeqCst) {
                break 'outer;
            }
            match hid.read_timeout(&mut buf, 100) {
                Ok(0) => {}
                Ok(n) if n >= 4 => {
                    let gap = last_packet.elapsed().as_millis() as u64;
                    if gap > st.max_gap_ms {
                        st.max_gap_ms = gap;
                    }
                    last_packet = Instant::now();
                    st.reports += 1;
                    let (b0, b1) = (buf[0], buf[1]);
                    let seq = u16::from_le_bytes([buf[2], buf[3]]);
                    if b0 != 0 || b1 != 0 {
                        st.nonzero += 1;
                    }
                    if last_btn != Some((b0, b1)) {
                        st.btn_transitions += 1;
                        last_btn = Some((b0, b1));
                    }
                    if let Some(prev) = last_seq {
                        let expected = prev.wrapping_add(1);
                        if seq != expected {
                            st.seq_gaps += 1;
                            st.seq_missed += seq.wrapping_sub(expected) as u64;
                        }
                    }
                    last_seq = Some(seq);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
    st
}

fn main() {
    let dur_ms = std::env::args()
        .skip(1)
        .find_map(|a| {
            a.strip_prefix("--duration-ms=")
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(60_000)
        .max(500);

    let api = HidApi::new().expect("hidapi init");
    let pid = MAI2_PIDS
        .iter()
        .copied()
        .find(|&pid| find_path(&api, pid, USAGE_PAGE_TOUCH, USAGE_TOUCH).is_some())
        .expect("no AFF1 mai2 touch-stream HID found");
    let label = if pid == 0x52A5 { "P1" } else { "P2" };
    let has_btn = find_path(&api, pid, USAGE_PAGE_BUTTONS, USAGE_BUTTONS).is_some();
    println!("hid_soak {label} pid={pid:04X} duration={dur_ms}ms buttons_iface={has_btn}");
    drop(api);

    let deadline = Instant::now() + Duration::from_millis(dur_ms);
    let stop = Arc::new(AtomicBool::new(false));

    let ts = stop.clone();
    let th = thread::spawn(move || touch_soak(pid, deadline, ts));
    let bs = stop.clone();
    let bh = thread::spawn(move || button_soak(pid, deadline, bs));

    thread::sleep(Duration::from_millis(dur_ms));
    stop.store(true, Ordering::SeqCst);
    let t = th.join().expect("touch thread");
    let b = bh.join().expect("button thread");

    let secs = t.elapsed_s.max(0.001);
    let expected_frames = 200.0 * secs;
    println!("\n==== TOUCH HID ({:.1}s) ====", secs);
    println!(
        "  packets={} ({:.1}/s)  complete_frames={} ({:.1}/s, ~{:.0}% of 200Hz)",
        t.packets,
        t.packets as f64 / secs,
        t.complete_frames,
        t.complete_frames as f64 / secs,
        100.0 * (t.complete_frames as f64) / expected_frames,
    );
    println!(
        "  part0={} part1={} (balance diff={})  nonzero_packets={} ({:.0}%)",
        t.part0,
        t.part1,
        (t.part0 as i64 - t.part1 as i64).abs(),
        t.nonzero,
        if t.packets > 0 {
            100.0 * t.nonzero as f64 / t.packets as f64
        } else {
            0.0
        },
    );
    println!(
        "  firmware_dropped_max={}  seq_gaps={} (missed_frames~{})  max_inter_packet_gap={}ms",
        t.fw_dropped_max, t.seq_gaps, t.seq_missed, t.max_gap_ms,
    );
    println!(
        "  DISCONNECTS: reopens={} open_fails={}",
        t.reopens, t.open_fails
    );

    println!("\n==== BUTTON HID ====");
    println!(
        "  reports={} ({:.0}/s)  button_bit_transitions={}  nonzero_reports={} ({:.0}%)",
        b.reports,
        b.reports as f64 / secs,
        b.btn_transitions,
        b.nonzero,
        if b.reports > 0 {
            100.0 * b.nonzero as f64 / b.reports as f64
        } else {
            0.0
        },
    );
    println!(
        "  seq_gaps={} (missed_frames~{})  max_gap={}ms",
        b.seq_gaps, b.seq_missed, b.max_gap_ms,
    );
    println!(
        "  DISCONNECTS: reopens={} open_fails={}",
        b.reopens, b.open_fails
    );

    let touch_ok = t.reopens == 0
        && t.open_fails == 0
        && t.fw_dropped_max == 0
        && t.seq_gaps == 0
        && t.max_gap_ms < 100
        && t.complete_frames as f64 >= expected_frames * 0.95;
    let btn_ok = b.reopens == 0 && b.open_fails == 0 && b.seq_gaps == 0;
    println!(
        "\nVERDICT: touch={}  button={}",
        if touch_ok { "CLEAN" } else { "SEE ABOVE" },
        if btn_ok { "CLEAN" } else { "SEE ABOVE" },
    );
}
