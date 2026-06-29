use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE,
    SetConsoleMode,
};
use windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA;

pub fn tick_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// Trailing status tag rendered (and colorized on the console) at the end of a line.
#[derive(Clone, Copy)]
enum LogTag {
    /// Neutral informational line, no tag.
    None,
    /// Success / connection established: green `[OK]`.
    Ok,
    /// Recoverable problem (will retry): yellow `[WARN]`.
    Warn,
    /// Failure: red `[FAIL]`.
    Fail,
    /// High-volume diagnostics: whole line dimmed, no tag.
    Diag,
}

/// Enable ANSI colors only when stderr is a real console (not redirected to a
/// file/pipe) and virtual-terminal processing can be turned on. Detected once.
fn ansi_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| unsafe {
        let handle = GetStdHandle(STD_ERROR_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    })
}

fn emit(tag: LogTag, msg: &str) {
    let body = format!("[Affine IO] {msg}");

    // Plain text (no escape codes) for OutputDebugStringA / non-console stderr.
    let plain = match tag {
        LogTag::Ok => format!("{body} [OK]"),
        LogTag::Warn => format!("{body} [WARN]"),
        LogTag::Fail => format!("{body} [FAIL]"),
        LogTag::None | LogTag::Diag => body.clone(),
    };

    if let Ok(cstr) = CString::new(format!("{plain}\n")) {
        unsafe {
            OutputDebugStringA(cstr.as_ptr().cast());
        }
    }

    if ansi_enabled() {
        let colored = match tag {
            LogTag::None => body,
            LogTag::Ok => format!("{body} \x1b[32;1m[OK]\x1b[0m"),
            LogTag::Warn => format!("{body} \x1b[33;1m[WARN]\x1b[0m"),
            LogTag::Fail => format!("{body} \x1b[31;1m[FAIL]\x1b[0m"),
            LogTag::Diag => format!("\x1b[90m{body}\x1b[0m"),
        };
        eprintln!("{colored}");
    } else {
        eprintln!("{plain}");
    }
}

/// Neutral info line. Tolerates a legacy `"[Affine IO] "` prefix so older call
/// sites keep rendering correctly under the new formatter.
pub fn log_line(line: &str) {
    emit(
        LogTag::None,
        line.strip_prefix("[Affine IO] ").unwrap_or(line),
    );
}

/// Neutral informational line.
pub fn log_info(msg: &str) {
    emit(LogTag::None, msg);
}

/// Success / connection established (green `[OK]`).
pub fn log_ok(msg: &str) {
    emit(LogTag::Ok, msg);
}

/// Recoverable problem that will be retried (yellow `[WARN]`).
pub fn log_warn(msg: &str) {
    emit(LogTag::Warn, msg);
}

/// Failure (red `[FAIL]`).
pub fn log_fail(msg: &str) {
    emit(LogTag::Fail, msg);
}

/// High-volume diagnostics (dimmed line).
pub fn log_diag(msg: &str) {
    emit(LogTag::Diag, msg);
}

pub fn segatools_config_path() -> PathBuf {
    std::env::var_os("SEGATOOLS_CONFIG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".\\segatools.ini"))
}

/// Read the raw trimmed value of `key` under `[section]` from an INI file.
/// Section and key matching is case-insensitive; `;`/`#` lines are comments.
/// Returns `None` if the file is unreadable or the key is absent.
fn ini_get_raw(path: &Path, section: &str, key: &str) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
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

        if raw_key.trim().eq_ignore_ascii_case(key) {
            return Some(raw_value.trim().to_string());
        }
    }

    None
}

pub fn ini_get_bool(path: &Path, section: &str, key: &str, default: bool) -> bool {
    ini_get_raw(path, section, key)
        .and_then(|value| parse_bool(&value))
        .unwrap_or(default)
}

pub fn ini_get_u32(path: &Path, section: &str, key: &str, default: u32) -> u32 {
    ini_get_raw(path, section, key)
        .and_then(|value| parse_u32(&value))
        .unwrap_or(default)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_bool_variants() {
        for v in ["1", "true", "TRUE", "yes", "on"] {
            assert_eq!(parse_bool(v), Some(true), "{v}");
        }
        for v in ["0", "false", "No", "off"] {
            assert_eq!(parse_bool(v), Some(false), "{v}");
        }
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn parse_u32_dec_and_hex() {
        assert_eq!(parse_u32("113"), Some(113));
        assert_eq!(parse_u32("0x70"), Some(0x70));
        assert_eq!(parse_u32("0X1a"), Some(0x1A));
        assert_eq!(parse_u32("nope"), None);
    }

    #[test]
    fn ini_reads_values_case_insensitively() {
        let ini = "; a comment\n[Touch]\np1Enable = 1\np1DebugInput=0\n\n[io4]\ntest = 0x70\n";
        let path = std::env::temp_dir().join("affine_core_ini_get_test.ini");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(ini.as_bytes())
            .unwrap();

        assert!(ini_get_bool(&path, "touch", "p1enable", false));
        assert!(!ini_get_bool(&path, "TOUCH", "p1DebugInput", true));
        assert_eq!(ini_get_u32(&path, "io4", "TEST", 0), 0x70);
        // Missing key / section fall back to the default.
        assert!(ini_get_bool(&path, "touch", "absent", true));
        assert_eq!(ini_get_u32(&path, "absent", "test", 42), 42);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ini_missing_file_returns_default() {
        let path = std::env::temp_dir().join("affine_core_ini_absent_xyz.ini");
        let _ = std::fs::remove_file(&path);
        assert!(ini_get_bool(&path, "s", "k", true));
        assert_eq!(ini_get_u32(&path, "s", "k", 7), 7);
    }
}
