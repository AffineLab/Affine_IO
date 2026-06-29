use crate::serial::find_com_port;
use crate::util::should_log;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_slider_frame_builds_framed_checksum() {
        let mut out = Vec::new();
        let ok = send_slider_frame(
            &mut |frame| {
                out.extend_from_slice(frame);
                true
            },
            SLIDER_CMD_AUTO_SCAN_START,
            &[],
        );
        assert!(ok);
        assert_eq!(out.len(), 4);
        assert_eq!(&out[..3], &[0xFF, SLIDER_CMD_AUTO_SCAN_START, 0x00]);
        let checksum = out[..3].iter().fold(0u8, |sum, &b| sum.wrapping_sub(b));
        assert_eq!(out[3], checksum);
    }

    #[test]
    fn slider_parser_decodes_a_frame() {
        let mut parser = SliderParser::default();
        // FF 01 02 AA BB <checksum> -> cmd 0x01, payload [AA, BB]
        for byte in [0xFFu8, 0x01, 0x02, 0xAA, 0xBB] {
            assert!(parser.push(byte).is_none());
        }
        let packet = parser.push(0x00).expect("frame should be complete");
        assert_eq!(packet.cmd, 0x01);
        assert_eq!(packet.payload, vec![0xAA, 0xBB]);
    }

    #[test]
    fn slider_parser_unescapes_0xfd() {
        let mut parser = SliderParser::default();
        // size=1; payload byte 0xFF is transmitted escaped as 0xFD 0xFE.
        for byte in [0xFFu8, 0x01, 0x01] {
            assert!(parser.push(byte).is_none());
        }
        assert!(parser.push(0xFD).is_none()); // escape marker
        assert!(parser.push(0xFE).is_none()); // -> 0xFF payload byte
        let packet = parser.push(0x00).expect("frame should be complete");
        assert_eq!(packet.cmd, 0x01);
        assert_eq!(packet.payload, vec![0xFF]);
    }

    #[test]
    fn slider_parser_resets_on_new_start_byte() {
        let mut parser = SliderParser::default();
        assert!(parser.push(0x01).is_none()); // ignored: no active frame yet
        assert!(parser.push(0xFF).is_none()); // start
        assert!(parser.push(0xFF).is_none()); // restart, discards partial
        assert!(parser.push(0x05).is_none());
        assert!(parser.push(0x00).is_none()); // size 0
        let packet = parser.push(0x00).expect("zero-payload frame");
        assert_eq!(packet.cmd, 0x05);
        assert!(packet.payload.is_empty());
    }
}
