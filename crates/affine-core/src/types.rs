pub type Hresult = windows_sys::core::HRESULT;

pub const S_OK: Hresult = 0;
pub const S_FALSE: Hresult = 1;
pub const E_FAIL: Hresult = 0x8000_4005u32 as i32;
pub const E_INVALIDARG: Hresult = 0x8007_0057u32 as i32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AimeIoVfdState {
    pub encoding: u8,
    pub text_speed: u8,
    pub scroll_enabled: u8,
    pub h_scroll: u16,
    pub cursor_x: u16,
    pub cursor_y: u8,
    pub wnd_x0: u16,
    pub wnd_y0: u8,
    pub wnd_x1: u16,
    pub wnd_y1: u8,
    pub rotate: u8,
    pub brightness: u8,
    pub screen_on: u8,
    pub clear_seq: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_snake_case)]
pub struct MercuryLedData {
    pub unitCount: u32,
    pub rgba: [u8; 480 * 4],
}

impl Default for MercuryLedData {
    fn default() -> Self {
        Self {
            unitCount: 0,
            rgba: [0; 480 * 4],
        }
    }
}

pub type Mai2TouchCallback = Option<unsafe extern "C" fn(player: u8, state: *const u8)>;
pub type ChuniSliderCallback = Option<unsafe extern "C" fn(state: *const u8)>;
pub type MercuryTouchCallback = Option<unsafe extern "C" fn(state: *const bool)>;

/// # Safety
///
/// `dst` must be either null or a valid, writable pointer to a `T`.
pub unsafe fn write_value<T: Copy>(dst: *mut T, value: T) {
    if let Some(dst) = unsafe { dst.as_mut() } {
        *dst = value;
    }
}

/// # Safety
///
/// `src` must be either null or valid for reads of `len` bytes for the
/// returned lifetime.
pub unsafe fn read_bytes<'a>(src: *const u8, len: usize) -> Option<&'a [u8]> {
    if src.is_null() {
        return None;
    }

    Some(unsafe { core::slice::from_raw_parts(src, len) })
}

/// # Safety
///
/// `src` must be either null or valid for mutable access to `len` bytes for the
/// returned lifetime, with no aliased mutable references.
pub unsafe fn read_mut_bytes<'a>(src: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    if src.is_null() {
        return None;
    }

    Some(unsafe { core::slice::from_raw_parts_mut(src, len) })
}
