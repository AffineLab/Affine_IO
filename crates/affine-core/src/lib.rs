pub mod serial;
pub mod shared_memory;
pub mod slider;
pub mod types;
pub mod util;

/// USB Vendor ID shared by every Affine controller (`VID_AFF1`).
pub const AFFINE_VID: u16 = 0xAFF1;

/// Baud rate for the USB-CDC fallback transport. Ignored by the USB-CDC
/// endpoint itself but required by the Windows serial DCB.
pub const SERIAL_BAUD: u32 = 115_200;

/// Build version stamped by `build.rs`: `git describe --tags --always --dirty`,
/// falling back to the Cargo package version when git is unavailable.
pub fn version() -> &'static str {
    env!("AFFINE_BUILD_VERSION")
}
