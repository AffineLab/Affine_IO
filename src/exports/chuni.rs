#![allow(clippy::missing_safety_doc)]

use affine_chuni as chuni;
use affine_core::types::{
    ChuniSliderCallback, Hresult, S_OK, read_bytes, read_mut_bytes, write_value,
};

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_get_api_version() -> u16 {
    0x0102
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_jvs_init() -> Hresult {
    chuni::runtime().init()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_jvs_poll(opbtn: *mut u8, beams: *mut u8) {
    let (next_opbtn, next_beams) = chuni::runtime().jvs_poll();
    unsafe {
        write_value(opbtn, next_opbtn);
        write_value(beams, next_beams);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_jvs_read_coin_counter(total: *mut u16) {
    unsafe { write_value(total, chuni::runtime().coin_counter()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_init() -> Hresult {
    chuni::runtime().init()
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_start(callback: ChuniSliderCallback) {
    chuni::runtime().start(callback);
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_stop() {
    chuni::runtime().stop();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_slider_set_leds(rgb: *const u8) {
    if let Some(bytes) = unsafe { read_bytes(rgb, 96) } {
        chuni::runtime().set_leds(bytes);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_led_init() -> Hresult {
    S_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_led_set_colors(board: u8, rgb: *mut u8) {
    if let Some(bytes) = unsafe { read_mut_bytes(rgb, 189) } {
        chuni::runtime().set_air_leds_from_colors(board, bytes);
    }
}
