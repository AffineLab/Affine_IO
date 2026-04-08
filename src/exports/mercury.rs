#![allow(clippy::missing_safety_doc)]

use affine_core::types::{Hresult, MercuryLedData, MercuryTouchCallback, S_OK, write_value};
use affine_mercury as mercury;

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_get_api_version() -> u16 {
    0x0100
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_init() -> Hresult {
    mercury::runtime().init()
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_poll() -> Hresult {
    S_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mercury_io_get_opbtns(opbtn: *mut u8) {
    unsafe { write_value(opbtn, mercury::runtime().opbtns()) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mercury_io_get_gamebtns(gamebtn: *mut u8) {
    unsafe { write_value(gamebtn, mercury::runtime().gamebtns()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_init() -> Hresult {
    mercury::runtime().init()
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_start(callback: MercuryTouchCallback) {
    mercury::runtime().start(callback);
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_set_leds(data: MercuryLedData) {
    mercury::runtime().set_leds(data);
}
