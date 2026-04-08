use std::sync::{Mutex, OnceLock};

use affine_core::serial::{SerialPort, find_com_port};
use affine_core::types::{E_FAIL, E_INVALIDARG, Hresult, S_FALSE, S_OK};
use affine_core::util::log_line;

const AFFINE_VID: u16 = 0xAFF1;
const MONICA_PID: u16 = 0x5730;
const SG_CMD_GET_FW_VERSION: u8 = 0x30;
const SG_CMD_GET_HW_VERSION: u8 = 0x32;
const SG_CMD_RADIO_ON: u8 = 0x40;
const SG_CMD_RADIO_OFF: u8 = 0x41;
const SG_CMD_POLL: u8 = 0x42;
const SG_CMD_MIFARE_SELECT: u8 = 0x43;
const SG_CMD_MIFARE_SET_KEY_AIME: u8 = 0x50;
const SG_CMD_MIFARE_AUTH_AIME: u8 = 0x51;
const SG_CMD_MIFARE_READ_BLOCK: u8 = 0x52;
const SG_CMD_MIFARE_SET_KEY_BANA: u8 = 0x54;
const SG_CMD_MIFARE_AUTH_BANA: u8 = 0x55;
const SG_CMD_TO_UPDATE_MODE: u8 = 0x60;
const SG_CMD_SEND_HEX_DATA: u8 = 0x61;
const SG_CMD_FELICA_ENCAP: u8 = 0x71;
const SG_CMD_EXT_LED_RGB: u8 = 0x81;
const SG_CMD_EXT_BOARD_INFO: u8 = 0xF0;

#[derive(Clone, Copy, Default)]
enum CachedCard {
    #[default]
    None,
    Mifare {
        uid: [u8; 4],
    },
    Felica {
        idm: [u8; 8],
        _pmm: [u8; 8],
    },
}

struct Reader {
    port: SerialPort,
    seq: u8,
    initialized: bool,
    card: CachedCard,
}

impl Default for Reader {
    fn default() -> Self {
        Self {
            port: SerialPort::default(),
            seq: 0,
            initialized: false,
            card: CachedCard::None,
        }
    }
}

struct ReaderResponse {
    status: u8,
    payload: Vec<u8>,
}

static AIME_READER: OnceLock<Mutex<Reader>> = OnceLock::new();

fn reader() -> &'static Mutex<Reader> {
    AIME_READER.get_or_init(|| Mutex::new(Reader::default()))
}

impl Reader {
    fn ensure_connected(&mut self) -> bool {
        if self.port.is_open() {
            return true;
        }

        let Some(path) = find_com_port(AFFINE_VID, MONICA_PID) else {
            return false;
        };

        if !self.port.open(&path, 115_200) {
            return false;
        }

        log_line(&format!(
            "[Affine IO] Monica reader connected: {}",
            path.trim_start_matches("\\\\.\\")
        ));
        true
    }

    fn init(&mut self) -> Hresult {
        if !self.ensure_connected() {
            return E_FAIL;
        }

        if self.initialized {
            return S_OK;
        }

        if let Ok(res) = self.transact(SG_CMD_GET_FW_VERSION, &[])
            && let Ok(version) = String::from_utf8(res.payload)
        {
            log_line(&format!("[Affine IO] Monica firmware: {version}"));
        }

        if let Ok(res) = self.transact(SG_CMD_GET_HW_VERSION, &[])
            && let Ok(version) = String::from_utf8(res.payload)
        {
            log_line(&format!("[Affine IO] Monica hardware: {version}"));
        }

        let _ = self.transact(SG_CMD_EXT_BOARD_INFO, &[]);
        self.initialized = true;
        S_OK
    }

    fn transact(&mut self, cmd: u8, payload: &[u8]) -> Result<ReaderResponse, Hresult> {
        if !self.ensure_connected() {
            return Err(E_FAIL);
        }

        let frame = encode_frame(self.seq, cmd, payload);
        self.seq = self.seq.wrapping_add(1);

        if !self.port.write(&frame) {
            self.port.close();
            return Err(E_FAIL);
        }

        let response = read_frame(&mut self.port).ok_or_else(|| {
            self.port.close();
            E_FAIL
        })?;

        if response.len() < 6 {
            return Err(E_FAIL);
        }

        let status = response[4];
        let payload_len = response[5] as usize;
        if response.len() < 6 + payload_len {
            return Err(E_FAIL);
        }

        Ok(ReaderResponse {
            status,
            payload: response[6..6 + payload_len].to_vec(),
        })
    }

    fn poll(&mut self) -> Hresult {
        let response = match self.transact(SG_CMD_POLL, &[]) {
            Ok(response) => response,
            Err(err) => return err,
        };

        if response.payload.is_empty() {
            self.card = CachedCard::None;
            return S_FALSE;
        }

        let count = response.payload[0];
        if count == 0 {
            self.card = CachedCard::None;
            return S_FALSE;
        }

        if response.payload.len() < 3 {
            self.card = CachedCard::None;
            return E_FAIL;
        }

        match response.payload[1] {
            0x10 => {
                if response.payload.len() < 7 {
                    self.card = CachedCard::None;
                    return E_FAIL;
                }
                let mut uid = [0u8; 4];
                uid.copy_from_slice(&response.payload[3..7]);
                self.card = CachedCard::Mifare { uid };
                S_OK
            }
            0x20 => {
                if response.payload.len() < 19 {
                    self.card = CachedCard::None;
                    return E_FAIL;
                }
                let mut idm = [0u8; 8];
                let mut pmm = [0u8; 8];
                idm.copy_from_slice(&response.payload[3..11]);
                pmm.copy_from_slice(&response.payload[11..19]);
                self.card = CachedCard::Felica { idm, _pmm: pmm };
                S_OK
            }
            _ => {
                self.card = CachedCard::None;
                S_FALSE
            }
        }
    }

    fn get_aime_id(&mut self, luid: &mut [u8]) -> Hresult {
        if luid.len() != 10 {
            return E_INVALIDARG;
        }

        let uid = match self.card {
            CachedCard::Mifare { uid } => uid,
            _ => return S_FALSE,
        };

        let mut payload = [0u8; 5];
        payload[..4].copy_from_slice(&uid);
        payload[4] = 2;

        let response = match self.transact(SG_CMD_MIFARE_READ_BLOCK, &payload) {
            Ok(response) => response,
            Err(err) => return err,
        };

        if response.status != 0 || response.payload.len() < 16 {
            return S_FALSE;
        }

        luid.copy_from_slice(&response.payload[6..16]);
        S_OK
    }

    fn get_felica_id(&self, idm_out: &mut u64) -> Hresult {
        match self.card {
            CachedCard::Felica { idm, .. } => {
                *idm_out = u64::from_be_bytes(idm);
                S_OK
            }
            _ => S_FALSE,
        }
    }

    fn get_mifare_uid(&self, uid_out: &mut [u8]) -> Hresult {
        if uid_out.len() != 4 {
            return E_INVALIDARG;
        }

        match self.card {
            CachedCard::Mifare { uid } => {
                uid_out.copy_from_slice(&uid);
                S_OK
            }
            _ => S_FALSE,
        }
    }
}

pub fn init() -> Hresult {
    reader().lock().unwrap().init()
}

pub fn poll(unit_no: u8) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }
    reader().lock().unwrap().poll()
}

pub fn get_aime_id(unit_no: u8, luid: &mut [u8]) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }
    reader().lock().unwrap().get_aime_id(luid)
}

pub fn get_felica_id(unit_no: u8, idm_out: &mut u64) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }
    reader().lock().unwrap().get_felica_id(idm_out)
}

pub fn get_mifare_uid(unit_no: u8, uid_out: &mut [u8]) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }
    reader().lock().unwrap().get_mifare_uid(uid_out)
}

pub fn mifare_select(unit_no: u8, uid: &[u8]) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }
    if uid.len() != 4 {
        return E_INVALIDARG;
    }

    match reader().lock().unwrap().transact(SG_CMD_MIFARE_SELECT, uid) {
        Ok(response) if response.status == 0 => S_OK,
        Ok(_) => S_FALSE,
        Err(err) => err,
    }
}

pub fn mifare_set_key(unit_no: u8, key_type: u8, key: &[u8]) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }
    if key.len() != 6 {
        return E_INVALIDARG;
    }

    let cmd = if key_type == 0 {
        SG_CMD_MIFARE_SET_KEY_AIME
    } else {
        SG_CMD_MIFARE_SET_KEY_BANA
    };

    match reader().lock().unwrap().transact(cmd, key) {
        Ok(response) if response.status == 0 => S_OK,
        Ok(_) => S_FALSE,
        Err(err) => err,
    }
}

pub fn mifare_authenticate(unit_no: u8, key_type: u8, payload: &[u8]) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }

    let cmd = if key_type == 0 {
        SG_CMD_MIFARE_AUTH_AIME
    } else {
        SG_CMD_MIFARE_AUTH_BANA
    };

    match reader().lock().unwrap().transact(cmd, payload) {
        Ok(response) if response.status == 0 => S_OK,
        Ok(_) => S_FALSE,
        Err(err) => err,
    }
}

pub fn mifare_read_block(unit_no: u8, uid: &[u8], block_no: u8, block: &mut [u8]) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }
    if uid.len() != 4 || block.len() != 16 {
        return E_INVALIDARG;
    }

    let mut payload = [0u8; 5];
    payload[..4].copy_from_slice(uid);
    payload[4] = block_no;

    match reader()
        .lock()
        .unwrap()
        .transact(SG_CMD_MIFARE_READ_BLOCK, &payload)
    {
        Ok(response) if response.status == 0 && response.payload.len() >= 16 => {
            block.copy_from_slice(&response.payload[..16]);
            S_OK
        }
        Ok(_) => S_FALSE,
        Err(err) => err,
    }
}

pub fn felica_transact(
    unit_no: u8,
    req: &[u8],
    res: &mut [u8],
    res_size_written: &mut usize,
) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }

    match reader().lock().unwrap().transact(SG_CMD_FELICA_ENCAP, req) {
        Ok(response) if response.status == 0 => {
            let copy_len = response.payload.len().min(res.len());
            res[..copy_len].copy_from_slice(&response.payload[..copy_len]);
            *res_size_written = copy_len;
            S_OK
        }
        Ok(_) => S_FALSE,
        Err(err) => err,
    }
}

pub fn radio_on(unit_no: u8) -> Hresult {
    simple(unit_no, SG_CMD_RADIO_ON)
}

pub fn radio_off(unit_no: u8) -> Hresult {
    simple(unit_no, SG_CMD_RADIO_OFF)
}

pub fn to_update_mode(unit_no: u8) -> Hresult {
    simple(unit_no, SG_CMD_TO_UPDATE_MODE)
}

pub fn send_hex_data(unit_no: u8, payload: &[u8], status_out: Option<&mut u8>) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }

    match reader()
        .lock()
        .unwrap()
        .transact(SG_CMD_SEND_HEX_DATA, payload)
    {
        Ok(response) => {
            if let Some(status_out) = status_out {
                *status_out = response.status;
            }
            if response.status == 0 { S_OK } else { S_FALSE }
        }
        Err(err) => err,
    }
}

pub fn led_set_color(unit_no: u8, rgb: [u8; 3]) {
    if unit_no != 0 {
        return;
    }
    let _ = reader().lock().unwrap().transact(SG_CMD_EXT_LED_RGB, &rgb);
}

pub fn vfd_set_text() {}

pub fn vfd_set_state() {}

fn simple(unit_no: u8, cmd: u8) -> Hresult {
    if unit_no != 0 {
        return S_FALSE;
    }

    match reader().lock().unwrap().transact(cmd, &[]) {
        Ok(response) if response.status == 0 => S_OK,
        Ok(_) => S_FALSE,
        Err(err) => err,
    }
}

fn encode_frame(seq: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(payload.len() + 5);
    body.push((5 + payload.len()) as u8);
    body.push(0);
    body.push(seq);
    body.push(cmd);
    body.push(payload.len() as u8);
    body.extend_from_slice(payload);

    let checksum = body.iter().fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    let mut encoded = Vec::with_capacity(body.len() + 4);
    encoded.push(0xE0);

    for &byte in body.iter().chain(std::iter::once(&checksum)) {
        if byte == 0xD0 || byte == 0xE0 {
            encoded.push(0xD0);
            encoded.push(byte.wrapping_sub(1));
        } else {
            encoded.push(byte);
        }
    }

    encoded
}

fn read_frame(port: &mut SerialPort) -> Option<Vec<u8>> {
    let mut started = false;
    let mut escaped = false;
    let mut decoded = Vec::with_capacity(256);

    loop {
        let mut buf = [0u8; 64];
        let read = port.read(&mut buf)?;
        if read == 0 {
            continue;
        }

        for &byte in &buf[..read] {
            if !started {
                if byte == 0xE0 {
                    started = true;
                    decoded.clear();
                }
                continue;
            }

            if byte == 0xE0 {
                decoded.clear();
                escaped = false;
                continue;
            }

            if byte == 0xD0 {
                escaped = true;
                continue;
            }

            let value = if escaped {
                escaped = false;
                byte.wrapping_add(1)
            } else {
                byte
            };

            decoded.push(value);

            if decoded.len() >= 2 {
                let expected = decoded[0] as usize + 1;
                if decoded.len() == expected {
                    let checksum = decoded[..decoded.len() - 1]
                        .iter()
                        .fold(0u8, |sum, &part| sum.wrapping_add(part));
                    if checksum != *decoded.last().unwrap() {
                        return None;
                    }
                    decoded.pop();
                    return Some(decoded);
                }
            }
        }
    }
}
