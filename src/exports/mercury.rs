#![allow(clippy::missing_safety_doc)]

use crate::slider;
use crate::types::{Hresult, MercuryLedData, MercuryTouchCallback, S_OK, write_value};

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_get_api_version() -> u16 {
    0x0100
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_init() -> Hresult {
    slider::mercury().init()
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_poll() -> Hresult {
    S_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mercury_io_get_opbtns(opbtn: *mut u8) {
    unsafe { write_value(opbtn, slider::mercury().opbtns()) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mercury_io_get_gamebtns(gamebtn: *mut u8) {
    unsafe { write_value(gamebtn, slider::mercury().gamebtns()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_init() -> Hresult {
    slider::mercury().init()
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_start(callback: MercuryTouchCallback) {
    slider::mercury().start(callback);
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_set_leds(data: MercuryLedData) {
    slider::mercury().set_leds(data);
}
