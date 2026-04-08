use crate::mai2;
use crate::types::{
    AimeIoVfdState, ChuniSliderCallback, E_NOTIMPL, Hresult, Mai2TouchCallback, MercuryLedData,
    MercuryTouchCallback, S_OK, write_value,
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
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_jvs_poll(opbtn: *mut u8, beams: *mut u8) {
    unsafe {
        write_value(opbtn, 0);
        write_value(beams, 0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn chuni_io_jvs_read_coin_counter(total: *mut u16) {
    unsafe { write_value(total, 0) };
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_init() -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_start(_callback: ChuniSliderCallback) {}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_stop() {}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_slider_set_leds(_rgb: *const u8) {}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_led_init() -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn chuni_io_led_set_colors(_board: u8, _rgb: *mut u8) {}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_get_api_version() -> u16 {
    0x0100
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_init() -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_poll() -> Hresult {
    S_OK
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mercury_io_get_opbtns(opbtn: *mut u8) {
    unsafe { write_value(opbtn, 0) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mercury_io_get_gamebtns(gamebtn: *mut u8) {
    unsafe { write_value(gamebtn, 0) };
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_init() -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_start(_callback: MercuryTouchCallback) {}

#[unsafe(no_mangle)]
pub extern "C" fn mercury_io_touch_set_leds(_data: MercuryLedData) {}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_get_api_version() -> u16 {
    0x0101
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_init() -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_poll(_unit_no: u8) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_get_aime_id(
    _unit_no: u8,
    _luid: *mut u8,
    _luid_size: usize,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_get_felica_id(_unit_no: u8, _IDm: *mut u64) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_get_mifare_uid(
    _unit_no: u8,
    _uid: *mut u8,
    _uid_size: usize,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_mifare_select(
    _unit_no: u8,
    _uid: *const u8,
    _uid_size: usize,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_mifare_set_key(
    _unit_no: u8,
    _key_type: u8,
    _key: *const u8,
    _key_size: usize,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_mifare_authenticate(
    _unit_no: u8,
    _key_type: u8,
    _payload: *const u8,
    _payload_size: usize,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_mifare_read_block(
    _unit_no: u8,
    _uid: *const u8,
    _uid_size: usize,
    _block_no: u8,
    _block: *mut u8,
    _block_size: usize,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_felica_transact(
    _unit_no: u8,
    _req: *const u8,
    _req_size: usize,
    _res: *mut u8,
    _res_size: usize,
    _res_size_written: *mut usize,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_radio_on(_unit_no: u8) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_radio_off(_unit_no: u8) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_to_update_mode(_unit_no: u8) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_nfc_send_hex_data(
    _unit_no: u8,
    _payload: *const u8,
    _payload_size: usize,
    _status_out: *mut u8,
) -> Hresult {
    E_NOTIMPL
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_led_set_color(_unit_no: u8, _r: u8, _g: u8, _b: u8) {}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_vfd_set_text(
    _text: *const u8,
    _text_len: usize,
    _state: *const AimeIoVfdState,
) {
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_vfd_set_state(_state: *const AimeIoVfdState) {}
