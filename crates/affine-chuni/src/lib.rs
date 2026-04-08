use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use affine_core::serial::SerialPort;
use affine_core::shared_memory::SharedPage;
use affine_core::slider::{
    SLIDER_CMD_AUTO_AIR, SLIDER_CMD_AUTO_AIR_START, SLIDER_CMD_AUTO_SCAN,
    SLIDER_CMD_AUTO_SCAN_START, SLIDER_CMD_AUTO_SCAN_STOP, SLIDER_CMD_SET_AIR_LED,
    SLIDER_CMD_SET_LED, SliderParser, find_any, send_slider_frame, should_log_scan,
};
use affine_core::types::{ChuniSliderCallback, Hresult, S_OK};
use affine_core::util::{log_line, sleep_ms};

const AFFINE_VID: u16 = 0xAFF1;
const CHUNI_PIDS: [u16; 2] = [0x52A4, 0x52A7];

const CHUNI_STATE_MAPPING_NAME: &str = "chuni_io_shm";
const CHUNI_STATE_MUTEX_NAME: &str = "chuni_io_shm_mutex";
const CHUNI_CONTROL_MAPPING_NAME: &str = "chuni_io_ctrl";
const CHUNI_CONTROL_MUTEX_NAME: &str = "chuni_io_ctrl_mutex";

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ChuniStatePage {
    opbtn: u8,
    beams: u8,
    connected: u8,
    _reserved0: [u8; 5],
    coin_counter: u16,
    _reserved1: [u8; 6],
    pressure: [u8; 32],
    sequence: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ChuniControlPage {
    active: u8,
    _reserved0: [u8; 7],
    slider_leds_sequence: u64,
    slider_leds: [u8; 96],
    air_leds_sequence: u64,
    air_leds: [u8; 3],
    _reserved1: [u8; 5],
}

impl Default for ChuniControlPage {
    fn default() -> Self {
        Self {
            active: 0,
            _reserved0: [0; 7],
            slider_leds_sequence: 0,
            slider_leds: [0; 96],
            air_leds_sequence: 0,
            air_leds: [0; 3],
            _reserved1: [0; 5],
        }
    }
}

pub struct ChuniRuntime {
    callback: Mutex<ChuniSliderCallback>,
    state_page: SharedPage<ChuniStatePage>,
    control_page: SharedPage<ChuniControlPage>,
    started: AtomicBool,
}

static CHUNI_RUNTIME: OnceLock<Arc<ChuniRuntime>> = OnceLock::new();

pub fn runtime() -> &'static Arc<ChuniRuntime> {
    CHUNI_RUNTIME.get_or_init(|| {
        Arc::new(ChuniRuntime {
            callback: Mutex::new(None),
            state_page: SharedPage::<ChuniStatePage>::create(
                CHUNI_STATE_MAPPING_NAME,
                CHUNI_STATE_MUTEX_NAME,
            )
            .expect("chuni state shared memory"),
            control_page: SharedPage::<ChuniControlPage>::create(
                CHUNI_CONTROL_MAPPING_NAME,
                CHUNI_CONTROL_MUTEX_NAME,
            )
            .expect("chuni control shared memory"),
            started: AtomicBool::new(false),
        })
    })
}

impl ChuniRuntime {
    pub fn init(&self) -> Hresult {
        if !self.started.swap(true, Ordering::SeqCst) {
            let runtime = runtime().clone();
            thread::spawn(move || chuni_thread(runtime));
        }
        S_OK
    }

    pub fn start(&self, callback: ChuniSliderCallback) {
        *self.callback.lock().unwrap() = callback;
        self.control_page.update(|page| {
            page.active = 1;
        });
    }

    pub fn stop(&self) {
        self.control_page.update(|page| {
            page.active = 0;
        });
    }

    pub fn set_leds(&self, rgb: &[u8]) {
        if rgb.len() < 96 {
            return;
        }

        self.control_page.update(|page| {
            page.slider_leds.copy_from_slice(&rgb[..96]);
            page.slider_leds_sequence = page.slider_leds_sequence.wrapping_add(1);
        });
    }

    pub fn set_air_leds_from_colors(&self, board: u8, rgb: &mut [u8]) {
        if board != 0 || rgb.len() < 153 {
            return;
        }

        self.control_page.update(|page| {
            page.air_leds = [rgb[152], rgb[150], rgb[151]];
            page.air_leds_sequence = page.air_leds_sequence.wrapping_add(1);
        });
    }

    pub fn jvs_poll(&self) -> (u8, u8) {
        let page = self.state_page.read();
        (page.opbtn, page.beams)
    }

    pub fn coin_counter(&self) -> u16 {
        self.state_page.read().coin_counter
    }

    #[cfg(feature = "latency-bench")]
    pub fn bench_inject_input(&self, pressure: [u8; 32], beams: u8) {
        apply_scan_frame(self, pressure, Some(beams));
    }
}

fn chuni_thread(runtime: Arc<ChuniRuntime>) {
    let mut port = SerialPort::default();
    let mut parser = SliderParser::default();
    let mut last_scan_log = 0u64;
    let mut last_active = false;
    let mut last_slider_leds_sequence = 0u64;
    let mut last_air_leds_sequence = 0u64;

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

            runtime.state_page.update(|page| {
                page.connected = 1;
                page.sequence = page.sequence.wrapping_add(1);
            });
            last_active = false;
        }

        let control = runtime.control_page.read();
        let active = control.active != 0;

        if active != last_active {
            if active {
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
            } else {
                let _ = send_slider_frame(
                    &mut |frame| port.write(frame),
                    SLIDER_CMD_AUTO_SCAN_STOP,
                    &[],
                );
            }

            last_active = active;
        }

        if control.slider_leds_sequence != last_slider_leds_sequence {
            let _ = send_slider_frame(
                &mut |frame| port.write(frame),
                SLIDER_CMD_SET_LED,
                &control.slider_leds,
            );
            last_slider_leds_sequence = control.slider_leds_sequence;
        }

        if control.air_leds_sequence != last_air_leds_sequence {
            let _ = send_slider_frame(
                &mut |frame| port.write(frame),
                SLIDER_CMD_SET_AIR_LED,
                &control.air_leds,
            );
            last_air_leds_sequence = control.air_leds_sequence;
        }

        let mut buf = [0u8; 64];
        let Some(read) = port.read(&mut buf) else {
            port.close();
            runtime.state_page.write(ChuniStatePage::default());
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
                            let beams = packet.payload.get(32).copied();
                            apply_scan_frame(&runtime, pressure, beams);
                        }
                    }
                    SLIDER_CMD_AUTO_AIR => {
                        if let Some(&beams) = packet.payload.first() {
                            runtime.state_page.update(|page| {
                                page.beams = beams;
                                page.connected = 1;
                                page.sequence = page.sequence.wrapping_add(1);
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn apply_scan_frame(runtime: &ChuniRuntime, pressure: [u8; 32], beams: Option<u8>) {
    let pressure = runtime.state_page.update(|page| {
        page.connected = 1;
        page.pressure = pressure;
        if let Some(beams) = beams {
            page.beams = beams;
        }
        page.sequence = page.sequence.wrapping_add(1);
        page.pressure
    });

    invoke_callback(runtime, &pressure);
}

fn invoke_callback(runtime: &ChuniRuntime, pressure: &[u8; 32]) {
    if let Some(callback) = *runtime.callback.lock().unwrap() {
        unsafe {
            callback(pressure.as_ptr());
        }
    }
}
