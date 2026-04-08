#![allow(clippy::missing_safety_doc)]

use crate::aime;
use crate::mai2;
use crate::slider;
use crate::types::{
    AimeIoVfdState, ChuniSliderCallback, Hresult, Mai2TouchCallback, MercuryLedData,
    MercuryTouchCallback, S_FALSE, S_OK, read_bytes, read_mut_bytes, write_value,
};

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

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_get_api_version() -> u16 {
    0x0101
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_init() -> Hresult {
    aime::init()
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_poll(unit_no: u8) -> Hresult {
    aime::poll(unit_no)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_get_aime_id(
    unit_no: u8,
    luid: *mut u8,
    luid_size: usize,
) -> Hresult {
    let Some(buffer) = (unsafe { read_mut_bytes(luid, luid_size) }) else {
        return S_FALSE;
    };
    aime::get_aime_id(unit_no, buffer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_get_felica_id(unit_no: u8, IDm: *mut u64) -> Hresult {
    if IDm.is_null() {
        return S_FALSE;
    }
    aime::get_felica_id(unit_no, unsafe { &mut *IDm })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_get_mifare_uid(
    unit_no: u8,
    uid: *mut u8,
    uid_size: usize,
) -> Hresult {
    let Some(buffer) = (unsafe { read_mut_bytes(uid, uid_size) }) else {
        return S_FALSE;
    };
    aime::get_mifare_uid(unit_no, buffer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_mifare_select(
    unit_no: u8,
    uid: *const u8,
    uid_size: usize,
) -> Hresult {
    let Some(buffer) = (unsafe { read_bytes(uid, uid_size) }) else {
        return S_FALSE;
    };
    aime::mifare_select(unit_no, buffer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_mifare_set_key(
    unit_no: u8,
    key_type: u8,
    key: *const u8,
    key_size: usize,
) -> Hresult {
    let Some(buffer) = (unsafe { read_bytes(key, key_size) }) else {
        return S_FALSE;
    };
    aime::mifare_set_key(unit_no, key_type, buffer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_mifare_authenticate(
    unit_no: u8,
    key_type: u8,
    payload: *const u8,
    payload_size: usize,
) -> Hresult {
    let Some(buffer) = (unsafe { read_bytes(payload, payload_size) }) else {
        return S_FALSE;
    };
    aime::mifare_authenticate(unit_no, key_type, buffer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_mifare_read_block(
    unit_no: u8,
    uid: *const u8,
    uid_size: usize,
    block_no: u8,
    block: *mut u8,
    block_size: usize,
) -> Hresult {
    let Some(uid_buffer) = (unsafe { read_bytes(uid, uid_size) }) else {
        return S_FALSE;
    };
    let Some(block_buffer) = (unsafe { read_mut_bytes(block, block_size) }) else {
        return S_FALSE;
    };
    aime::mifare_read_block(unit_no, uid_buffer, block_no, block_buffer)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_felica_transact(
    unit_no: u8,
    req: *const u8,
    req_size: usize,
    res: *mut u8,
    res_size: usize,
    res_size_written: *mut usize,
) -> Hresult {
    let Some(req_buffer) = (unsafe { read_bytes(req, req_size) }) else {
        return S_FALSE;
    };
    let Some(res_buffer) = (unsafe { read_mut_bytes(res, res_size) }) else {
        return S_FALSE;
    };
    if res_size_written.is_null() {
        return S_FALSE;
    }

    aime::felica_transact(unit_no, req_buffer, res_buffer, unsafe {
        &mut *res_size_written
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_radio_on(unit_no: u8) -> Hresult {
    aime::radio_on(unit_no)
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_radio_off(unit_no: u8) -> Hresult {
    aime::radio_off(unit_no)
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_to_update_mode(unit_no: u8) -> Hresult {
    aime::to_update_mode(unit_no)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aime_io_nfc_send_hex_data(
    unit_no: u8,
    payload: *const u8,
    payload_size: usize,
    status_out: *mut u8,
) -> Hresult {
    let Some(buffer) = (unsafe { read_bytes(payload, payload_size) }) else {
        return S_FALSE;
    };
    let status_out = if status_out.is_null() {
        None
    } else {
        Some(unsafe { &mut *status_out })
    };
    aime::send_hex_data(unit_no, buffer, status_out)
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_led_set_color(unit_no: u8, r: u8, g: u8, b: u8) {
    aime::led_set_color(unit_no, [r, g, b]);
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_vfd_set_text(
    _text: *const u8,
    _text_len: usize,
    _state: *const AimeIoVfdState,
) {
    let _ = _text;
    let _ = _text_len;
    let _ = _state;
    aime::vfd_set_text();
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_vfd_set_state(_state: *const AimeIoVfdState) {
    let _ = _state;
    aime::vfd_set_state();
}
