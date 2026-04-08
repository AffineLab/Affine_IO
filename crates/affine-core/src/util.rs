use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;

pub fn tick_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

pub fn log_line(line: &str) {
    let mut owned = String::from(line);
    if !owned.ends_with('\n') {
        owned.push('\n');
    }

    if let Ok(cstr) = CString::new(owned.clone()) {
        unsafe {
            OutputDebugStringA(cstr.as_ptr().cast());
        }
    }

    eprint!("{owned}");
}

pub fn segatools_config_path() -> PathBuf {
    std::env::var_os("SEGATOOLS_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".\\segatools.ini"))
}

pub fn ini_get_bool(path: &Path, section: &str, key: &str, default: bool) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return default;
    };

    let mut current_section = String::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            current_section.clear();
            current_section.push_str(line[1..line.len() - 1].trim());
            continue;
        }

        if !current_section.eq_ignore_ascii_case(section) {
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };

        if !raw_key.trim().eq_ignore_ascii_case(key) {
            continue;
        }

        return parse_bool(raw_value.trim()).unwrap_or(default);
    }

    default
}

pub fn ini_get_u32(path: &Path, section: &str, key: &str, default: u32) -> u32 {
    let Ok(contents) = fs::read_to_string(path) else {
        return default;
    };

    let mut current_section = String::new();

    for raw_line in contents.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            current_section.clear();
            current_section.push_str(line[1..line.len() - 1].trim());
            continue;
        }

        if !current_section.eq_ignore_ascii_case(section) {
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };

        if !raw_key.trim().eq_ignore_ascii_case(key) {
            continue;
        }

        return parse_u32(raw_value.trim()).unwrap_or(default);
    }

    default
}

pub fn current_exe_name() -> Option<String> {
    std::env::current_exe().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })
}

pub fn should_log(last_scan_log: &mut u64) -> bool {
    let now = tick_ms();
    if now.saturating_sub(*last_scan_log) >= 5_000 {
        *last_scan_log = now;
        true
    } else {
        false
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    if value == "1"
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("yes")
        || value.eq_ignore_ascii_case("on")
    {
        Some(true)
    } else if value == "0"
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("off")
    {
        Some(false)
    } else {
        None
    }
}

fn parse_u32(value: &str) -> Option<u32> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).ok()
    } else {
        value.parse::<u32>().ok()
    }
}
