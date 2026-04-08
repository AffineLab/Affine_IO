use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use crate::serial::{SerialPort, find_com_port};
use crate::types::{ChuniSliderCallback, Hresult, MercuryLedData, MercuryTouchCallback, S_OK};
use crate::util::{log_line, sleep_ms};

const AFFINE_VID: u16 = 0xAFF1;
const CHUNI_PIDS: [u16; 2] = [0x52A4, 0x52A7];
const MERCURY_PIDS: [u16; 1] = [0x52A5];

const SLIDER_CMD_AUTO_SCAN: u8 = 0x01;
const SLIDER_CMD_SET_LED: u8 = 0x02;
const SLIDER_CMD_AUTO_SCAN_START: u8 = 0x03;
const SLIDER_CMD_AUTO_SCAN_STOP: u8 = 0x04;
const SLIDER_CMD_AUTO_AIR: u8 = 0x05;
const SLIDER_CMD_AUTO_AIR_START: u8 = 0x06;
const SLIDER_CMD_SET_AIR_LED: u8 = 0x07;

pub(crate) struct ChuniRuntime {
    callback: Mutex<ChuniSliderCallback>,
    pressure: Mutex<[u8; 32]>,
    beams: Mutex<u8>,
    tx: Sender<ChuniCommand>,
    rx: Mutex<Option<Receiver<ChuniCommand>>>,
    started: AtomicBool,
    active: AtomicBool,
}

enum ChuniCommand {
    Start,
    Stop,
    SliderLeds([u8; 96]),
    AirLeds([u8; 3]),
}

pub(crate) struct MercuryRuntime {
    callback: Mutex<MercuryTouchCallback>,
    tx: Sender<MercuryCommand>,
    rx: Mutex<Option<Receiver<MercuryCommand>>>,
    started: AtomicBool,
    active: AtomicBool,
}

enum MercuryCommand {
    Start,
}

static CHUNI_RUNTIME: OnceLock<Arc<ChuniRuntime>> = OnceLock::new();
static MERCURY_RUNTIME: OnceLock<Arc<MercuryRuntime>> = OnceLock::new();

pub fn chuni() -> &'static Arc<ChuniRuntime> {
    CHUNI_RUNTIME.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        Arc::new(ChuniRuntime {
            callback: Mutex::new(None),
            pressure: Mutex::new([0; 32]),
            beams: Mutex::new(0),
            tx,
            rx: Mutex::new(Some(rx)),
            started: AtomicBool::new(false),
            active: AtomicBool::new(false),
        })
    })
}

pub fn mercury() -> &'static Arc<MercuryRuntime> {
    MERCURY_RUNTIME.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        Arc::new(MercuryRuntime {
            callback: Mutex::new(None),
            tx,
            rx: Mutex::new(Some(rx)),
            started: AtomicBool::new(false),
            active: AtomicBool::new(false),
        })
    })
}

impl ChuniRuntime {
    pub fn init(&self) -> Hresult {
        if !self.started.swap(true, Ordering::SeqCst)
            && let Some(rx) = self.rx.lock().unwrap().take()
        {
            let runtime = chuni().clone();
            thread::spawn(move || chuni_thread(runtime, rx));
        }
        S_OK
    }

    pub fn start(&self, callback: ChuniSliderCallback) {
        *self.callback.lock().unwrap() = callback;
        self.active.store(true, Ordering::SeqCst);
        let _ = self.tx.send(ChuniCommand::Start);
    }

    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
        let _ = self.tx.send(ChuniCommand::Stop);
    }

    pub fn set_leds(&self, rgb: &[u8]) {
        if rgb.len() < 96 {
            return;
        }
        let mut payload = [0u8; 96];
        payload.copy_from_slice(&rgb[..96]);
        let _ = self.tx.send(ChuniCommand::SliderLeds(payload));
    }

    pub fn set_air_leds_from_colors(&self, board: u8, rgb: &mut [u8]) {
        if board != 0 || rgb.len() < 153 {
            return;
        }

        let payload = [rgb[152], rgb[150], rgb[151]];
        let _ = self.tx.send(ChuniCommand::AirLeds(payload));
    }

    pub fn jvs_poll(&self) -> (u8, u8) {
        (0, *self.beams.lock().unwrap())
    }

    pub fn coin_counter(&self) -> u16 {
        0
    }
}

impl MercuryRuntime {
    pub fn init(&self) -> Hresult {
        if !self.started.swap(true, Ordering::SeqCst)
            && let Some(rx) = self.rx.lock().unwrap().take()
        {
            let runtime = mercury().clone();
            thread::spawn(move || mercury_thread(runtime, rx));
        }
        S_OK
    }

    pub fn start(&self, callback: MercuryTouchCallback) {
        *self.callback.lock().unwrap() = callback;
        self.active.store(true, Ordering::SeqCst);
        let _ = self.tx.send(MercuryCommand::Start);
    }

    pub fn opbtns(&self) -> u8 {
        0
    }

    pub fn gamebtns(&self) -> u8 {
        0
    }

    pub fn set_leds(&self, _data: MercuryLedData) {}
}

fn chuni_thread(runtime: Arc<ChuniRuntime>, rx: Receiver<ChuniCommand>) {
    let mut port = SerialPort::default();
    let mut parser = SliderParser::default();
    let mut last_scan_log = 0u64;

    loop {
        if !port.is_open() {
            let Some((_, path)) = find_any(AFFINE_VID, &CHUNI_PIDS) else {
                if should_log(&mut last_scan_log) {
                    log_line("[Affine IO] Chunithm slider: device not found");
                }
                sleep_ms(500);
                continue;
            };

            if !port.open(&path, 115_200) {
                if should_log(&mut last_scan_log) {
                    log_line(&format!(
                        "[Affine IO] Chunithm slider: failed to open {path}"
                    ));
                }
                sleep_ms(500);
                continue;
            }

            log_line(&format!(
                "[Affine IO] Chunithm slider connected: {}",
                path.trim_start_matches("\\\\.\\")
            ));

            if runtime.active.load(Ordering::SeqCst) {
                send_slider_command(&mut port, SLIDER_CMD_AUTO_AIR_START, &[]);
                send_slider_command(&mut port, SLIDER_CMD_AUTO_SCAN_START, &[]);
            }
        }

        while let Ok(command) = rx.try_recv() {
            match command {
                ChuniCommand::Start => {
                    send_slider_command(&mut port, SLIDER_CMD_AUTO_AIR_START, &[]);
                    send_slider_command(&mut port, SLIDER_CMD_AUTO_SCAN_START, &[]);
                }
                ChuniCommand::Stop => {
                    send_slider_command(&mut port, SLIDER_CMD_AUTO_SCAN_STOP, &[]);
                }
                ChuniCommand::SliderLeds(payload) => {
                    send_slider_command(&mut port, SLIDER_CMD_SET_LED, &payload);
                }
                ChuniCommand::AirLeds(payload) => {
                    send_slider_command(&mut port, SLIDER_CMD_SET_AIR_LED, &payload);
                }
            }
        }

        let mut buf = [0u8; 64];
        let Some(read) = port.read(&mut buf) else {
            port.close();
            *runtime.pressure.lock().unwrap() = [0; 32];
            *runtime.beams.lock().unwrap() = 0;
            invoke_chuni_callback(&runtime, &[0; 32]);
            sleep_ms(500);
            continue;
        };

        if read == 0 {
            continue;
        }

        for &byte in &buf[..read] {
            if let Some(packet) = parser.push(byte) {
                match packet.cmd {
                    SLIDER_CMD_AUTO_SCAN => {
                        if packet.payload.len() >= 32 {
                            let mut pressure = [0u8; 32];
                            pressure.copy_from_slice(&packet.payload[..32]);
                            *runtime.pressure.lock().unwrap() = pressure;
                            if packet.payload.len() >= 33 {
                                *runtime.beams.lock().unwrap() = packet.payload[32];
                            }
                            invoke_chuni_callback(&runtime, &pressure);
                        }
                    }
                    SLIDER_CMD_AUTO_AIR => {
                        if let Some(&beams) = packet.payload.first() {
                            *runtime.beams.lock().unwrap() = beams;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn mercury_thread(runtime: Arc<MercuryRuntime>, rx: Receiver<MercuryCommand>) {
    let mut port = SerialPort::default();
    let mut parser = SliderParser::default();
    let mut last_scan_log = 0u64;

    loop {
        if !port.is_open() {
            let Some((_, path)) = find_any(AFFINE_VID, &MERCURY_PIDS) else {
                if should_log(&mut last_scan_log) {
                    log_line("[Affine IO] Mercury touch: device not found");
                }
                sleep_ms(500);
                continue;
            };

            if !port.open(&path, 115_200) {
                if should_log(&mut last_scan_log) {
                    log_line(&format!("[Affine IO] Mercury touch: failed to open {path}"));
                }
                sleep_ms(500);
                continue;
            }

            log_line(&format!(
                "[Affine IO] Mercury touch connected: {}",
                path.trim_start_matches("\\\\.\\")
            ));
            if runtime.active.load(Ordering::SeqCst) {
                send_slider_command(&mut port, SLIDER_CMD_AUTO_SCAN_START, &[]);
            }
        }

        while let Ok(command) = rx.try_recv() {
            match command {
                MercuryCommand::Start => {
                    send_slider_command(&mut port, SLIDER_CMD_AUTO_SCAN_START, &[]);
                }
            }
        }

        let mut buf = [0u8; 64];
        let Some(read) = port.read(&mut buf) else {
            port.close();
            invoke_mercury_callback(&runtime, &[false; 240]);
            sleep_ms(500);
            continue;
        };

        if read == 0 {
            continue;
        }

        for &byte in &buf[..read] {
            if let Some(packet) = parser.push(byte)
                && packet.cmd == SLIDER_CMD_AUTO_SCAN
                && packet.payload.len() >= 30
            {
                let mut cells = [false; 240];
                for (index, value) in packet.payload[..30].iter().enumerate() {
                    for bit in 0..8 {
                        cells[index * 8 + bit] = value & (1 << bit) != 0;
                    }
                }
                invoke_mercury_callback(&runtime, &cells);
            }
        }
    }
}

fn invoke_chuni_callback(runtime: &ChuniRuntime, pressure: &[u8; 32]) {
    if let Some(callback) = *runtime.callback.lock().unwrap() {
        unsafe {
            callback(pressure.as_ptr());
        }
    }
}

fn invoke_mercury_callback(runtime: &MercuryRuntime, cells: &[bool; 240]) {
    if let Some(callback) = *runtime.callback.lock().unwrap() {
        unsafe {
            callback(cells.as_ptr());
        }
    }
}

fn should_log(last_scan_log: &mut u64) -> bool {
    let now = crate::util::tick_ms();
    if now.saturating_sub(*last_scan_log) >= 5_000 {
        *last_scan_log = now;
        true
    } else {
        false
    }
}

fn find_any(vid: u16, pids: &[u16]) -> Option<(u16, String)> {
    for &pid in pids {
        if let Some(path) = find_com_port(vid, pid) {
            return Some((pid, path));
        }
    }
    None
}

fn send_slider_command(port: &mut SerialPort, cmd: u8, payload: &[u8]) -> bool {
    if !port.is_open() {
        return false;
    }

    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.push(0xFF);
    frame.push(cmd);
    frame.push(payload.len() as u8);
    frame.extend_from_slice(payload);
    let checksum = frame.iter().fold(0u8, |sum, &byte| sum.wrapping_sub(byte));
    frame.push(checksum);
    port.write(&frame)
}

struct SliderParser {
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

struct SliderPacket {
    cmd: u8,
    payload: Vec<u8>,
}

impl SliderParser {
    fn push(&mut self, byte: u8) -> Option<SliderPacket> {
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
