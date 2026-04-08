use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use affine_core::serial::SerialPort;
use affine_core::slider::{
    SLIDER_CMD_AUTO_SCAN, SLIDER_CMD_AUTO_SCAN_START, SliderParser, find_any, send_slider_frame,
    should_log_scan,
};
use affine_core::types::{Hresult, MercuryLedData, MercuryTouchCallback, S_OK};
use affine_core::util::{log_line, sleep_ms};

const AFFINE_VID: u16 = 0xAFF1;
const MERCURY_PIDS: [u16; 1] = [0x52A5];

pub struct MercuryRuntime {
    callback: Mutex<MercuryTouchCallback>,
    tx: Sender<MercuryCommand>,
    rx: Mutex<Option<Receiver<MercuryCommand>>>,
    started: AtomicBool,
    active: AtomicBool,
}

enum MercuryCommand {
    Start,
}

static MERCURY_RUNTIME: OnceLock<Arc<MercuryRuntime>> = OnceLock::new();

pub fn runtime() -> &'static Arc<MercuryRuntime> {
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

impl MercuryRuntime {
    pub fn init(&self) -> Hresult {
        if !self.started.swap(true, Ordering::SeqCst)
            && let Some(rx) = self.rx.lock().unwrap().take()
        {
            let runtime = runtime().clone();
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

fn mercury_thread(runtime: Arc<MercuryRuntime>, rx: Receiver<MercuryCommand>) {
    let mut port = SerialPort::default();
    let mut parser = SliderParser::default();
    let mut last_scan_log = 0u64;

    loop {
        if !port.is_open() {
            let Some((_, path)) = find_any(AFFINE_VID, &MERCURY_PIDS) else {
                if should_log_scan(&mut last_scan_log) {
                    log_line("[Affine IO] Mercury touch: device not found");
                }
                sleep_ms(500);
                continue;
            };

            if !port.open(&path, 115_200) {
                if should_log_scan(&mut last_scan_log) {
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
                let _ = send_slider_frame(
                    &mut |frame| port.write(frame),
                    SLIDER_CMD_AUTO_SCAN_START,
                    &[],
                );
            }
        }

        while let Ok(command) = rx.try_recv() {
            match command {
                MercuryCommand::Start => {
                    let _ = send_slider_frame(
                        &mut |frame| port.write(frame),
                        SLIDER_CMD_AUTO_SCAN_START,
                        &[],
                    );
                }
            }
        }

        let mut buf = [0u8; 64];
        let Some(read) = port.read(&mut buf) else {
            port.close();
            invoke_callback(&runtime, &[false; 240]);
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
                invoke_callback(&runtime, &cells);
            }
        }
    }
}

fn invoke_callback(runtime: &MercuryRuntime, cells: &[bool; 240]) {
    if let Some(callback) = *runtime.callback.lock().unwrap() {
        unsafe {
            callback(cells.as_ptr());
        }
    }
}
