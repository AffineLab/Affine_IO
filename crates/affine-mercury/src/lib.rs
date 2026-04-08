use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use affine_core::serial::SerialPort;
use affine_core::shared_memory::SharedPage;
use affine_core::slider::{
    SLIDER_CMD_AUTO_SCAN, SLIDER_CMD_AUTO_SCAN_START, SliderParser, find_any, send_slider_frame,
    should_log_scan,
};
use affine_core::types::{Hresult, MercuryLedData, MercuryTouchCallback, S_OK};
use affine_core::util::{log_line, sleep_ms};

const AFFINE_VID: u16 = 0xAFF1;
const MERCURY_PIDS: [u16; 1] = [0x52A5];

const MERCURY_STATE_MAPPING_NAME: &str = "mercury_io_shm";
const MERCURY_STATE_MUTEX_NAME: &str = "mercury_io_shm_mutex";
const MERCURY_CONTROL_MAPPING_NAME: &str = "mercury_io_ctrl";
const MERCURY_CONTROL_MUTEX_NAME: &str = "mercury_io_ctrl_mutex";

#[repr(C)]
#[derive(Clone, Copy)]
struct MercuryStatePage {
    opbtn: u8,
    gamebtn: u8,
    connected: u8,
    _reserved0: [u8; 5],
    cells: [u8; 240],
    sequence: u64,
}

impl Default for MercuryStatePage {
    fn default() -> Self {
        Self {
            opbtn: 0,
            gamebtn: 0,
            connected: 0,
            _reserved0: [0; 5],
            cells: [0; 240],
            sequence: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MercuryControlPage {
    active: u8,
    _reserved0: [u8; 7],
    leds_sequence: u64,
    leds: MercuryLedData,
}

pub struct MercuryRuntime {
    callback: Mutex<MercuryTouchCallback>,
    state_page: SharedPage<MercuryStatePage>,
    control_page: SharedPage<MercuryControlPage>,
    started: AtomicBool,
}

static MERCURY_RUNTIME: OnceLock<Arc<MercuryRuntime>> = OnceLock::new();

pub fn runtime() -> &'static Arc<MercuryRuntime> {
    MERCURY_RUNTIME.get_or_init(|| {
        Arc::new(MercuryRuntime {
            callback: Mutex::new(None),
            state_page: SharedPage::<MercuryStatePage>::create(
                MERCURY_STATE_MAPPING_NAME,
                MERCURY_STATE_MUTEX_NAME,
            )
            .expect("mercury state shared memory"),
            control_page: SharedPage::<MercuryControlPage>::create(
                MERCURY_CONTROL_MAPPING_NAME,
                MERCURY_CONTROL_MUTEX_NAME,
            )
            .expect("mercury control shared memory"),
            started: AtomicBool::new(false),
        })
    })
}

impl MercuryRuntime {
    pub fn init(&self) -> Hresult {
        if !self.started.swap(true, Ordering::SeqCst) {
            let runtime = runtime().clone();
            thread::spawn(move || mercury_thread(runtime));
        }
        S_OK
    }

    pub fn start(&self, callback: MercuryTouchCallback) {
        *self.callback.lock().unwrap() = callback;
        self.control_page.update(|page| {
            page.active = 1;
        });
    }

    pub fn opbtns(&self) -> u8 {
        self.state_page.read().opbtn
    }

    pub fn gamebtns(&self) -> u8 {
        self.state_page.read().gamebtn
    }

    pub fn set_leds(&self, data: MercuryLedData) {
        self.control_page.update(|page| {
            page.leds = data;
            page.leds_sequence = page.leds_sequence.wrapping_add(1);
        });
    }

    #[cfg(feature = "latency-bench")]
    pub fn bench_inject_input(&self, cells: [bool; 240]) {
        apply_cells_frame(self, cells);
    }
}

fn mercury_thread(runtime: Arc<MercuryRuntime>) {
    let mut port = SerialPort::default();
    let mut parser = SliderParser::default();
    let mut last_scan_log = 0u64;
    let mut last_active = false;
    let mut last_leds_sequence = 0u64;

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
            runtime.state_page.update(|page| {
                page.connected = 1;
                page.sequence = page.sequence.wrapping_add(1);
            });
            last_active = false;
        }

        let control = runtime.control_page.read();
        let active = control.active != 0;

        if active && !last_active {
            let _ = send_slider_frame(
                &mut |frame| port.write(frame),
                SLIDER_CMD_AUTO_SCAN_START,
                &[],
            );
        }
        last_active = active;

        if control.leds_sequence != last_leds_sequence {
            last_leds_sequence = control.leds_sequence;
        }

        let mut buf = [0u8; 64];
        let Some(read) = port.read(&mut buf) else {
            port.close();
            runtime.state_page.write(MercuryStatePage::default());
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
                apply_cells_frame(&runtime, cells);
            }
        }
    }
}

fn apply_cells_frame(runtime: &MercuryRuntime, cells: [bool; 240]) {
    let cells = runtime.state_page.update(|page| {
        page.connected = 1;
        for (index, value) in cells.into_iter().enumerate() {
            page.cells[index] = value as u8;
        }
        page.sequence = page.sequence.wrapping_add(1);

        let mut snapshot = [false; 240];
        for (index, value) in page.cells.iter().copied().enumerate() {
            snapshot[index] = value != 0;
        }
        snapshot
    });

    invoke_callback(runtime, &cells);
}

fn invoke_callback(runtime: &MercuryRuntime, cells: &[bool; 240]) {
    if let Some(callback) = *runtime.callback.lock().unwrap() {
        unsafe {
            callback(cells.as_ptr());
        }
    }
}
