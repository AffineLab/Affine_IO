#![allow(clippy::missing_safety_doc)]

use affine_core::types::{Hresult, Mai2TouchCallback, S_OK, write_value};
use affine_mai2 as mai2;

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_get_api_version() -> u16 {
    0x0102
}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_init() -> Hresult {
    mai2::runtime().init()
}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_poll() -> Hresult {
    mai2::runtime().poll()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mai2_io_get_opbtns(opbtn: *mut u8) {
    unsafe { write_value(opbtn, mai2::runtime().get_opbtns()) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mai2_io_get_gamebtns(player1: *mut u16, player2: *mut u16) {
    let (left, right) = mai2::runtime().get_gamebtns();
    unsafe {
        write_value(player1, left);
        write_value(player2, right);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_touch_init(callback: Mai2TouchCallback) -> Hresult {
    mai2::runtime().set_touch_callback(callback);
    S_OK
}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_touch_set_sens(_bytes: *mut u8) {}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_touch_update(player1: bool, player2: bool) {
    mai2::runtime().set_touch_enabled(player1, player2);
}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_led_init() -> Hresult {
    mai2::runtime().led_init()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mai2_io_led_set_fet_output(board: u8, rgb: *const u8) {
    if rgb.is_null() {
        return;
    }

    let bytes = unsafe { core::slice::from_raw_parts(rgb, 3) };
    let mut payload = [0u8; 3];
    payload.copy_from_slice(bytes);
    mai2::runtime().led_set_fet_output(board, payload);
}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_led_dc_update(_board: u8, _rgb: *const u8) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mai2_io_led_gs_update(board: u8, rgb: *const u8) {
    if rgb.is_null() {
        return;
    }

    let bytes = unsafe { core::slice::from_raw_parts(rgb, 32) };
    mai2::runtime().led_gs_update(board, bytes);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mai2_io_led_billboard_set(board: u8, rgb: *const u8) {
    if rgb.is_null() {
        return;
    }

    let bytes = unsafe { core::slice::from_raw_parts(rgb, 3) };
    mai2::runtime().led_billboard_set(board, bytes);
}

#[unsafe(no_mangle)]
pub extern "C" fn mai2_io_led_cam_set(_state: u8) {}
