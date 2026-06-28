use std::ffi::CString;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

const AFFINE_VID: u16 = 0xAFF1;
const MAI2_PIDS: [u16; 2] = [0x52A5, 0x52A6];

const USAGE_PAGE_BUTTONS: u16 = 0xFFCA;
const USAGE_BUTTONS: u16 = 0x0001;
const USAGE_PAGE_VENDOR: u16 = 0xFFCA;
const USAGE_VENDOR: u16 = 0x0002;
const USAGE_PAGE_TOUCH: u16 = 0xFF00;
const USAGE_TOUCH: u16 = 0x0031;

const CMD_GET_BOARD_INFO: u8 = 0xF0;
const CMD_GET_TOUCH_HID_STATS: u8 = 0x29;
const VENDOR_REPORT_LEN: usize = 65;
const VENDOR_FRAME_LEN: usize = 64;
const TOUCH_REPORT_ID: u8 = 0x31;
const TOUCH_REPORT_LEN: usize = 64;
const TOUCH_READ_BUF_LEN: usize = 65;

#[derive(Default)]
struct TouchProbeStats {
    packets: u64,
    part0: u64,
    part1: u64,
    complete_frames: u64,
    nonzero_packets: u64,
    dropped_frames: u16,
    last_seq: u8,
    last_touch: [u8; 7],
    seen_mask: u8,
    active_seq: u8,
}

fn main() {
    let duration = parse_duration();
    let api = HidApi::new().expect("hidapi init");

    println!("mai2 hardware probe duration={}ms", duration.as_millis());
    list_affine_devices(&api);

    let mut found = false;

    for &pid in &MAI2_PIDS {
        let label = if pid == 0x52A5 { "P1" } else { "P2" };
        let vendor_path = find_hid_path(&api, pid, USAGE_PAGE_VENDOR, USAGE_VENDOR);
        let touch_path = find_hid_path(&api, pid, USAGE_PAGE_TOUCH, USAGE_TOUCH);
        let button_path = find_hid_path(&api, pid, USAGE_PAGE_BUTTONS, USAGE_BUTTONS);

        if vendor_path.is_none() && touch_path.is_none() && button_path.is_none() {
            continue;
        }

        found = true;
        println!("{label}: pid={pid:04X}");
        println!(
            "  buttons_hid={}",
            button_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "missing".to_string())
        );

        if let Some(path) = vendor_path {
            probe_vendor(label, &api, &path);
        } else {
            println!("  vendor_hid=missing");
        }

        if let Some(path) = touch_path {
            probe_touch(label, &api, &path, duration);
        } else {
            println!("  touch_hid=missing");
        }
    }

    if !found {
        println!("no AFF1 mai2 HID interfaces found");
    }
}

fn parse_duration() -> Duration {
    let mut duration_ms = 3_000u64;

    for arg in std::env::args().skip(1) {
        if let Some(raw) = arg.strip_prefix("--duration-ms=")
            && let Ok(value) = raw.parse::<u64>()
        {
            duration_ms = value.max(100);
        }
    }

    Duration::from_millis(duration_ms)
}

fn list_affine_devices(api: &HidApi) {
    println!("AFF1 HID collections:");
    let mut count = 0u32;

    for info in api
        .device_list()
        .filter(|info| info.vendor_id() == AFFINE_VID)
    {
        count += 1;
        println!(
            "  pid={:04X} usage={:04X}:{:04X} iface={} product={} path={}",
            info.product_id(),
            info.usage_page(),
            info.usage(),
            info.interface_number(),
            info.product_string().unwrap_or(""),
            info.path().to_string_lossy(),
        );
    }

    if count == 0 {
        println!("  none");
    }
}

fn find_hid_path(api: &HidApi, pid: u16, usage_page: u16, usage: u16) -> Option<CString> {
    api.device_list()
        .find(|info| {
            info.vendor_id() == AFFINE_VID
                && info.product_id() == pid
                && info.usage_page() == usage_page
                && info.usage() == usage
        })
        .map(|info| info.path().to_owned())
}

fn probe_vendor(label: &str, api: &HidApi, path: &CString) {
    println!("  vendor_hid={}", path.to_string_lossy());

    let Ok(hid) = api.open_path(path) else {
        println!("  vendor_open=failed");
        return;
    };

    match request_vendor_frame(&hid, CMD_GET_BOARD_INFO, &[], Duration::from_millis(1_000)) {
        Some(frame) => println!("  board_info={}", parse_board_info(&frame)),
        None => println!("  board_info=timeout"),
    }

    match request_vendor_frame(
        &hid,
        CMD_GET_TOUCH_HID_STATS,
        &[],
        Duration::from_millis(1_000),
    ) {
        Some(frame) => print_touch_hid_stats(label, &frame),
        None => println!("  touch_hid_stats=timeout"),
    }
}

fn request_vendor_frame(
    hid: &HidDevice,
    cmd: u8,
    payload: &[u8],
    timeout: Duration,
) -> Option<Vec<u8>> {
    send_vendor_frame(hid, cmd, payload)?;

    let deadline = Instant::now() + timeout;
    let mut report = [0u8; VENDOR_REPORT_LEN];

    while Instant::now() < deadline {
        match hid.read_timeout(&mut report, 20) {
            Ok(0) => continue,
            Ok(read) => {
                if let Some(frame) = parse_vendor_frame(&report[..read])
                    && frame.get(1).copied() == Some(cmd)
                {
                    return Some(frame);
                }
            }
            Err(_) => return None,
        }
    }

    None
}

fn send_vendor_frame(hid: &HidDevice, cmd: u8, payload: &[u8]) -> Option<()> {
    if payload.len() + 4 > VENDOR_FRAME_LEN {
        return None;
    }

    let mut report = [0u8; VENDOR_REPORT_LEN];
    let mut idx = 1usize;
    report[idx] = 0xFF;
    idx += 1;
    report[idx] = cmd;
    idx += 1;
    report[idx] = payload.len() as u8;
    idx += 1;
    report[idx..idx + payload.len()].copy_from_slice(payload);
    idx += payload.len();
    report[idx] = report[1..idx]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_add(byte));

    match hid.write(&report) {
        Ok(written) if written != 0 => Some(()),
        _ => None,
    }
}

fn parse_vendor_frame(report: &[u8]) -> Option<Vec<u8>> {
    let start = report.iter().position(|&byte| byte == 0xFF)?;
    if report.len().saturating_sub(start) < 4 {
        return None;
    }

    let len = report[start + 2] as usize;
    let total = 4 + len;
    if report.len().saturating_sub(start) < total {
        return None;
    }

    let frame = &report[start..start + total];
    let checksum = frame[..total - 1]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    if frame[total - 1] != checksum {
        return None;
    }

    Some(frame.to_vec())
}

fn parse_board_info(frame: &[u8]) -> String {
    if frame.len() < 5 || frame[1] != CMD_GET_BOARD_INFO {
        return "invalid".to_string();
    }

    let payload = &frame[3..frame.len() - 1];
    let version_len = payload.first().copied().unwrap_or(0) as usize;
    if version_len == 0 || 1 + version_len > payload.len() {
        return "unknown".to_string();
    }

    String::from_utf8_lossy(&payload[1..1 + version_len]).into_owned()
}

fn print_touch_hid_stats(label: &str, frame: &[u8]) {
    if frame.len() < 52 || frame[1] != CMD_GET_TOUCH_HID_STATS || frame[2] != 48 {
        println!("  touch_hid_stats=invalid");
        return;
    }

    let payload = &frame[3..51];
    let u32_at = |offset: usize| -> u32 {
        u32::from_le_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ])
    };

    println!(
        "  touch_hid_stats[{label}]: frame_start={} part_ok={} busy_retry={} not_ready={} send_fail={} stale_drop={} interval={}ms max_interval={}ms dropped={} pending={} part_index={} in_ready={}",
        u32_at(0),
        u32_at(4),
        u32_at(8),
        u32_at(12),
        u32_at(16),
        u32_at(20),
        u32_at(24),
        u32_at(28),
        u32_at(40),
        payload[44],
        payload[45],
        payload[46],
    );
}

fn probe_touch(label: &str, api: &HidApi, path: &CString, duration: Duration) {
    println!("  touch_hid={}", path.to_string_lossy());

    let Ok(hid) = api.open_path(path) else {
        println!("  touch_open=failed");
        return;
    };

    let started = Instant::now();
    let deadline = started + duration;
    let mut report = [0u8; TOUCH_READ_BUF_LEN];
    let mut stats = TouchProbeStats::default();

    while Instant::now() < deadline {
        match hid.read_timeout(&mut report, 100) {
            Ok(0) => continue,
            Ok(read) => {
                if let Some(part) = parse_touch_report(&report[..read]) {
                    stats.packets += 1;
                    if part.part_index == 0 {
                        stats.part0 += 1;
                    } else if part.part_index == 1 {
                        stats.part1 += 1;
                    }
                    if part.touch_bits.iter().any(|&byte| byte != 0) {
                        stats.nonzero_packets += 1;
                    }
                    stats.dropped_frames = part.dropped_frames;
                    stats.last_seq = part.stream_seq;
                    stats.last_touch = pack_legacy_touch_bits(&part.touch_bits);

                    if stats.seen_mask == 0 || stats.active_seq != part.stream_seq {
                        stats.active_seq = part.stream_seq;
                        stats.seen_mask = 0;
                    }
                    stats.seen_mask |= 1u8 << part.part_index;
                    if stats.seen_mask == 0x03 {
                        stats.complete_frames += 1;
                        stats.seen_mask = 0;
                    }
                }
            }
            Err(_) => break,
        }
    }

    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    println!(
        "  touch_stream[{label}]: packets={} packet_hz={:.1} complete_frames={} frame_hz={:.1} part0={} part1={} nonzero_packets={} dropped={} last_seq={} last_touch={:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
        stats.packets,
        stats.packets as f64 / elapsed,
        stats.complete_frames,
        stats.complete_frames as f64 / elapsed,
        stats.part0,
        stats.part1,
        stats.nonzero_packets,
        stats.dropped_frames,
        stats.last_seq,
        stats.last_touch[0],
        stats.last_touch[1],
        stats.last_touch[2],
        stats.last_touch[3],
        stats.last_touch[4],
        stats.last_touch[5],
        stats.last_touch[6],
    );
}

struct TouchPart {
    stream_seq: u8,
    part_index: u8,
    touch_bits: [u8; 5],
    dropped_frames: u16,
}

fn parse_touch_report(report: &[u8]) -> Option<TouchPart> {
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

    let data = &report[start..start + TOUCH_REPORT_LEN];
    if data[1] != 1 || data[2] != 1 || data[5] != 2 || data[4] >= 2 || (data[12] & 0x04) == 0 {
        return None;
    }

    let mut touch_bits = [0u8; 5];
    touch_bits.copy_from_slice(&data[50..55]);

    Some(TouchPart {
        stream_seq: data[3],
        part_index: data[4],
        touch_bits,
        dropped_frames: u16::from_le_bytes([data[55], data[56]]),
    })
}

fn pack_legacy_touch_bits(touch_bits: &[u8; 5]) -> [u8; 7] {
    let mut touch = [0u8; 7];

    for (group, packed) in touch.iter_mut().enumerate() {
        for bit in 0..5 {
            let touch_index = group * 5 + bit;
            if touch_index >= 34 {
                break;
            }
            if (touch_bits[touch_index / 8] & (1u8 << (touch_index % 8))) != 0 {
                *packed |= 1u8 << bit;
            }
        }
    }

    touch
}
