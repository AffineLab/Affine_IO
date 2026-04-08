use crate::serial::find_com_port;
use crate::util::{should_log, tick_ms};

pub const SLIDER_CMD_AUTO_SCAN: u8 = 0x01;
pub const SLIDER_CMD_SET_LED: u8 = 0x02;
pub const SLIDER_CMD_AUTO_SCAN_START: u8 = 0x03;
pub const SLIDER_CMD_AUTO_SCAN_STOP: u8 = 0x04;
pub const SLIDER_CMD_AUTO_AIR: u8 = 0x05;
pub const SLIDER_CMD_AUTO_AIR_START: u8 = 0x06;
pub const SLIDER_CMD_SET_AIR_LED: u8 = 0x07;

pub fn find_any(vid: u16, pids: &[u16]) -> Option<(u16, String)> {
    for &pid in pids {
        if let Some(path) = find_com_port(vid, pid) {
            return Some((pid, path));
        }
    }
    None
}

pub fn should_log_scan(last_scan_log: &mut u64) -> bool {
    should_log(last_scan_log)
}

pub fn send_slider_frame<F>(writer: &mut F, cmd: u8, payload: &[u8]) -> bool
where
    F: FnMut(&[u8]) -> bool,
{
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.push(0xFF);
    frame.push(cmd);
    frame.push(payload.len() as u8);
    frame.extend_from_slice(payload);
    let checksum = frame.iter().fold(0u8, |sum, &byte| sum.wrapping_sub(byte));
    frame.push(checksum);
    writer(&frame)
}

pub struct SliderPacket {
    pub cmd: u8,
    pub payload: Vec<u8>,
}

pub struct SliderParser {
    buf: [u8; 128],
    len: usize,
    escaped: bool,
    active: bool,
}

impl Default for SliderParser {
    fn default() -> Self {
        Self {
            buf: [0; 128],
            len: 0,
            escaped: false,
            active: false,
        }
    }
}

impl SliderParser {
    pub fn push(&mut self, byte: u8) -> Option<SliderPacket> {
        if byte == 0xFF {
            self.active = true;
            self.escaped = false;
            self.len = 0;
            self.buf[self.len] = byte;
            self.len += 1;
            return None;
        }

        if !self.active {
            return None;
        }

        if byte == 0xFD {
            self.escaped = true;
            return None;
        }

        let decoded = if self.escaped {
            self.escaped = false;
            byte.wrapping_add(1)
        } else {
            byte
        };

        if self.len >= self.buf.len() {
            self.active = false;
            self.len = 0;
            return None;
        }

        self.buf[self.len] = decoded;
        self.len += 1;

        if self.len < 4 {
            return None;
        }

        let size = self.buf[2] as usize;
        let total = size + 4;
        if self.len < total {
            return None;
        }

        let packet = SliderPacket {
            cmd: self.buf[1],
            payload: self.buf[3..3 + size].to_vec(),
        };

        self.active = false;
        self.len = 0;
        Some(packet)
    }
}

pub fn now_ms() -> u64 {
    tick_ms()
}
