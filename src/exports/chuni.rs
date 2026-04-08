#![allow(clippy::missing_safety_doc)]

use crate::slider;
use crate::types::{ChuniSliderCallback, Hresult, S_OK, read_bytes, read_mut_bytes, write_value};

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_get_api_version() -> u16 {
    0x0102
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_jvs_init() -> Hresult {
    slider::chuni().init()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_jvs_poll(opbtn: *mut u8, beams: *mut u8) {
    let (next_opbtn, next_beams) = slider::chuni().jvs_poll();
    unsafe {
        write_value(opbtn, next_opbtn);
        write_value(beams, next_beams);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_jvs_read_coin_counter(total: *mut u16) {
    unsafe { write_value(total, slider::chuni().coin_counter()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_init() -> Hresult {
    slider::chuni().init()
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_start(callback: ChuniSliderCallback) {
    slider::chuni().start(callback);
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_stop() {
    slider::chuni().stop();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_slider_set_leds(rgb: *const u8) {
    if let Some(bytes) = unsafe { read_bytes(rgb, 96) } {
        slider::chuni().set_leds(bytes);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_led_init() -> Hresult {
    S_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_led_set_colors(board: u8, rgb: *mut u8) {
    if let Some(bytes) = unsafe { read_mut_bytes(rgb, 189) } {
        slider::chuni().set_air_leds_from_colors(board, bytes);
    }
}
