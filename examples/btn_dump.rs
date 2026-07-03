// Dumps the mai2 button custom-HID report, isolating the button bits from the
// incrementing sequence field, to verify sim button playback.
//   cargo run --example btn_dump -- --duration-ms=8000

use std::time::{Duration, Instant};

use affine_core::AFFINE_VID;
use hidapi::HidApi;

const MAI2_PIDS: [u16; 2] = [0x52A5, 0x52A6];
const BTN_USAGE_PAGE: u16 = 0xFFCA;
const BTN_USAGE: u16 = 0x0001;

fn main() {
    let dur = std::env::args()
        .skip(1)
        .find_map(|a| {
            a.strip_prefix("--duration-ms=")
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(8000);

    let api = HidApi::new().expect("hidapi");
    let pid = MAI2_PIDS
        .iter()
        .copied()
        .find(|&p| {
            api.device_list().any(|i| {
                i.vendor_id() == AFFINE_VID
                    && i.product_id() == p
                    && i.usage_page() == BTN_USAGE_PAGE
                    && i.usage() == BTN_USAGE
            })
        })
        .expect("no AFF1 mai2 button HID found");
    let path = api
        .device_list()
        .find(|i| {
            i.vendor_id() == AFFINE_VID
                && i.product_id() == pid
                && i.usage_page() == BTN_USAGE_PAGE
                && i.usage() == BTN_USAGE
        })
        .unwrap()
        .path()
        .to_owned();
    let hid = api.open_path(&path).expect("open button hid");
    println!("btn_dump pid={pid:04X} duration={dur}ms");

    let start = Instant::now();
    let deadline = start + Duration::from_millis(dur);
    let mut buf = [0u8; 64];
    let mut reports = 0u64;
    let mut last_btn: Option<(u8, u8, Vec<u8>)> = None;
    let mut transitions = 0u64;
    let mut nonzero_reports = 0u64;
    let mut printed_raw = 0;

    while Instant::now() < deadline {
        match hid.read_timeout(&mut buf, 200) {
            Ok(0) => {}
            Ok(n) => {
                reports += 1;
                if printed_raw < 4 {
                    println!(
                        "  raw[{n}]: {}",
                        buf[..n.min(12)]
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    printed_raw += 1;
                }
                // layout now: [buttons0, buttons1, seq_lo, seq_hi, touch_bits[5], ...]
                let (b0, b1) = (buf[0], buf[1]);
                let touch = if n >= 9 { &buf[4..9] } else { &buf[0..0] };
                if b0 != 0 || b1 != 0 {
                    nonzero_reports += 1;
                }
                let key = (b0, b1, touch.to_vec());
                if last_btn.as_ref() != Some(&key) {
                    transitions += 1;
                    let ms = start.elapsed().as_millis();
                    let touch_hex = touch
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("  t+{ms:>5}ms  b0={b0:02X} b1={b1:02X}  touch_bits=[{touch_hex}]");
                    last_btn = Some(key);
                }
            }
            Err(e) => {
                println!("  read error: {e}");
                break;
            }
        }
    }

    let secs = start.elapsed().as_secs_f64().max(0.001);
    println!(
        "\nreports={reports} ({:.0}/s)  button-bit transitions={transitions}  nonzero_reports={nonzero_reports} ({:.0}%)",
        reports as f64 / secs,
        if reports > 0 {
            100.0 * nonzero_reports as f64 / reports as f64
        } else {
            0.0
        },
    );
    if transitions <= 1 {
        println!("VERDICT: button bits are STATIC (no sim playback / no input)");
    } else {
        println!(
            "VERDICT: button bits are CHANGING ({transitions} states) — sim playback / input active"
        );
    }
}
