use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use affine_core::serial::SerialPort;
use affine_core::slider::{
    SLIDER_CMD_AUTO_AIR, SLIDER_CMD_AUTO_AIR_START, SLIDER_CMD_AUTO_SCAN,
    SLIDER_CMD_AUTO_SCAN_START, SLIDER_CMD_AUTO_SCAN_STOP, SLIDER_CMD_SET_AIR_LED,
    SLIDER_CMD_SET_LED, SliderParser, find_any, send_slider_frame, should_log_scan,
};
use affine_core::types::{ChuniSliderCallback, Hresult, S_OK};
use affine_core::util::{log_line, sleep_ms};

const AFFINE_VID: u16 = 0xAFF1;
const CHUNI_PIDS: [u16; 2] = [0x52A4, 0x52A7];

pub struct ChuniRuntime {
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

static CHUNI_RUNTIME: OnceLock<Arc<ChuniRuntime>> = OnceLock::new();

pub fn runtime() -> &'static Arc<ChuniRuntime> {
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

impl ChuniRuntime {
    pub fn init(&self) -> Hresult {
        if !self.started.swap(true, Ordering::SeqCst)
            && let Some(rx) = self.rx.lock().unwrap().take()
        {
            let runtime = runtime().clone();
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

fn chuni_thread(runtime: Arc<ChuniRuntime>, rx: Receiver<ChuniCommand>) {
    let mut port = SerialPort::default();
    let mut parser = SliderParser::default();
    let mut last_scan_log = 0u64;

    loop {
        if !port.is_open() {
            let Some((_, path)) = find_any(AFFINE_VID, &CHUNI_PIDS) else {
                if should_log_scan(&mut last_scan_log) {
                    log_line("[Affine IO] Chunithm slider: device not found");
                }
                sleep_ms(500);
                continue;
            };

            if !port.open(&path, 115_200) {
                if should_log_scan(&mut last_scan_log) {
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
                let _ = send_slider_frame(
                    &mut |frame| port.write(frame),
                    SLIDER_CMD_AUTO_AIR_START,
                    &[],
                );
                let _ = send_slider_frame(
                    &mut |frame| port.write(frame),
                    SLIDER_CMD_AUTO_SCAN_START,
                    &[],
                );
            }
        }

        while let Ok(command) = rx.try_recv() {
            match command {
                ChuniCommand::Start => {
                    let _ = send_slider_frame(
                        &mut |frame| port.write(frame),
                        SLIDER_CMD_AUTO_AIR_START,
                        &[],
                    );
                    let _ = send_slider_frame(
                        &mut |frame| port.write(frame),
                        SLIDER_CMD_AUTO_SCAN_START,
                        &[],
                    );
                }
                ChuniCommand::Stop => {
                    let _ = send_slider_frame(
                        &mut |frame| port.write(frame),
                        SLIDER_CMD_AUTO_SCAN_STOP,
                        &[],
                    );
                }
                ChuniCommand::SliderLeds(payload) => {
                    let _ = send_slider_frame(
                        &mut |frame| port.write(frame),
                        SLIDER_CMD_SET_LED,
                        &payload,
                    );
                }
                ChuniCommand::AirLeds(payload) => {
                    let _ = send_slider_frame(
                        &mut |frame| port.write(frame),
                        SLIDER_CMD_SET_AIR_LED,
                        &payload,
                    );
                }
            }
        }

        let mut buf = [0u8; 64];
        let Some(read) = port.read(&mut buf) else {
            port.close();
            *runtime.pressure.lock().unwrap() = [0; 32];
            *runtime.beams.lock().unwrap() = 0;
            invoke_callback(&runtime, &[0; 32]);
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
                            invoke_callback(&runtime, &pressure);
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

fn invoke_callback(runtime: &ChuniRuntime, pressure: &[u8; 32]) {
    if let Some(callback) = *runtime.callback.lock().unwrap() {
        unsafe {
            callback(pressure.as_ptr());
        }
    }
}
