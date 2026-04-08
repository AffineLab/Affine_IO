#![allow(clippy::missing_safety_doc)]

use crate::aime;
use crate::types::{AimeIoVfdState, Hresult, S_FALSE, read_bytes, read_mut_bytes};

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
    text: *const u8,
    text_len: usize,
    state: *const AimeIoVfdState,
) {
    let _ = text;
    let _ = text_len;
    let _ = state;
    aime::vfd_set_text();
}

#[unsafe(no_mangle)]
pub extern "C" fn aime_io_vfd_set_state(state: *const AimeIoVfdState) {
    let _ = state;
    aime::vfd_set_state();
}
