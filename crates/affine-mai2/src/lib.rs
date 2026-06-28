use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use affine_core::serial::{SerialPort, find_com_port};
use affine_core::shared_memory::SharedPage;
use affine_core::types::{Hresult, Mai2TouchCallback, S_OK};
use affine_core::util::{
    current_exe_name, ini_get_bool, ini_get_u32, log_diag, log_line, log_ok, log_warn,
    segatools_config_path, sleep_ms, tick_ms,
};
use hidapi::{HidApi, HidDevice};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F1, VK_F2, VK_F3};

const AFFINE_VID: u16 = 0xAFF1;
const MAI2_PID_1P: u16 = 0x52A5;
const MAI2_PID_2P: u16 = 0x52A6;
const AFFINE_CMD_HEARTBEAT: u8 = 0x11;
const AFFINE_CMD_GET_BOARD_INFO: u8 = 0xF0;
const MAI2_BUTTON_HID_USAGE_PAGE: u16 = 0xFFCA;
const MAI2_BUTTON_HID_USAGE: u16 = 0x0001;
const MAI2_BUTTON_HID_REPORT_LEN: usize = 24;
const MAI2_HID_READ_TIMEOUT_MS: i32 = 1000;
const MAI2_VENDOR_HID_USAGE_PAGE: u16 = 0xFFCA;
const MAI2_VENDOR_HID_USAGE: u16 = 0x0002;
const MAI2_VENDOR_HID_REPORT_LEN: usize = 65;
const MAI2_VENDOR_HID_FRAME_LEN: usize = 64;
const MAI2_VENDOR_HID_READ_TIMEOUT_MS: i32 = 1;
const MAI2_TOUCH_HID_USAGE_PAGE: u16 = 0xFF00;
const MAI2_TOUCH_HID_USAGE: u16 = 0x0031;
const MAI2_TOUCH_HID_REPORT_ID: u8 = 0x31;
const MAI2_TOUCH_HID_REPORT_LEN: usize = 64;
const MAI2_TOUCH_HID_READ_BUF_LEN: usize = 65;
const MAI2_TOUCH_HID_READ_TIMEOUT_MS: i32 = 1000;
const MAI2_TOUCH_HID_PART_COUNT: u8 = 2;

const AFFINE_HEARTBEAT_INTERVAL_MS: u64 = 100;
const AFFINE_RESCAN_INTERVAL_MS: u64 = 500;
const AFFINE_BOARD_INFO_DELAY_MS: u64 = 500;
const AFFINE_BOARD_INFO_TIMEOUT_MS: u64 = 1_000;
const AFFINE_DEVICE_LOOP_SLEEP_MS: u64 = 1;
const AFFINE_SERIAL_READ_BUF_LEN: usize = 256;
const AFFINE_SERIAL_RX_BUF_LEN: usize = 512;
const AFFINE_TOUCH_DIAG_INTERVAL_MS: u64 = 2_000;
const AFFINE_LED_PROBE_ENABLED: bool = false;
const AFFINE_TOUCH_STALE_LED_PAUSE_MS: u64 = 300;
const AFFINE_TOUCH_STALE_RECONNECT_MS: u64 = 1_000;
const AFFINE_TOUCH_STALE_GRACE_MS: u64 = 3_000;

const MAI2_IO_OPBTN_TEST: u8 = 0x01;
const MAI2_IO_OPBTN_SERVICE: u8 = 0x02;
const MAI2_IO_OPBTN_COIN: u8 = 0x04;

const MAI2_IO_GAMEBTN_1: u16 = 0x01;
const MAI2_IO_GAMEBTN_2: u16 = 0x02;
const MAI2_IO_GAMEBTN_3: u16 = 0x04;
const MAI2_IO_GAMEBTN_4: u16 = 0x08;
const MAI2_IO_GAMEBTN_5: u16 = 0x10;
const MAI2_IO_GAMEBTN_6: u16 = 0x20;
const MAI2_IO_GAMEBTN_7: u16 = 0x40;
const MAI2_IO_GAMEBTN_8: u16 = 0x80;
const MAI2_IO_GAMEBTN_SELECT: u16 = 0x100;

const MAI2_AFFINE_EXT_SELECT_BIT: u8 = 0;
const MAI2_AFFINE_EXT_TEST_BIT: u8 = 1;
const MAI2_AFFINE_EXT_SERVICE_BIT: u8 = 2;
const MAI2_AFFINE_EXT_COIN_BIT: u8 = 3;

const MAI2_INPUT_MAPPING_NAMES: [&str; 2] = ["mai_io_shm_1", "mai_io_shm_2"];
const MAI2_INPUT_MUTEX_NAMES: [&str; 2] = ["mai_io_shm_1_mutex", "mai_io_shm_2_mutex"];
const MAI2_OUTPUT_MAPPING_NAMES: [&str; 2] = ["mai_io_ctrl_1", "mai_io_ctrl_2"];
const MAI2_OUTPUT_MUTEX_NAMES: [&str; 2] = ["mai_io_ctrl_1_mutex", "mai_io_ctrl_2_mutex"];
const MAI2_POLL_MAPPING_NAME: &str = "mai_io_poll";
const MAI2_POLL_MUTEX_NAME: &str = "mai_io_poll_mutex";

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Mai2InputPage {
    buttons0: u8,
    io_status: u8,
    connected: u8,
    _reserved0: [u8; 5],
    touch: [u8; 7],
    _reserved1: u8,
    sequence: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Mai2OutputPage {
    touch_enabled: u8,
    _reserved0: [u8; 7],
    buttons_sequence: u64,
    buttons: [u8; 56],
    billboard_sequence: u64,
    billboard: [u8; 24],
    pwm_sequence: u64,
    pwm: [u8; 3],
    _reserved1: [u8; 5],
}

impl Default for Mai2OutputPage {
    fn default() -> Self {
        Self {
            touch_enabled: 0,
            _reserved0: [0; 7],
            buttons_sequence: 0,
            buttons: [0; 56],
            billboard_sequence: 0,
            billboard: [0; 24],
            pwm_sequence: 0,
            pwm: [0; 3],
            _reserved1: [0; 5],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Mai2PollPage {
    opbtn: u8,
    _reserved0: u8,
    player1_btn: u16,
    player2_btn: u16,
    affine_coin: u8,
    _reserved1: [u8; 7],
    sequence: u64,
}

#[derive(Clone, Copy)]
struct KeyboardConfig {
    vk_test: u16,
    vk_service: u16,
    vk_coin: u16,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            vk_test: VK_F1,
            vk_service: VK_F2,
            vk_coin: VK_F3,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct LedProbeBoardState {
    has_last: bool,
    last_ms: u64,
    last_payload: [u8; 32],
    has_last_zero: bool,
    last_zero_ms: u64,
    last_zero_payload: [u8; 32],
    sequence: u64,
}

#[derive(Default)]
struct LedProbeState {
    boards: [LedProbeBoardState; 2],
}

struct SharedState {
    callback: Mutex<Mai2TouchCallback>,
    led_probe: Mutex<LedProbeState>,
    led_start_latch: Mutex<[[u8; 24]; 2]>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            callback: Mutex::new(None),
            led_probe: Mutex::new(LedProbeState::default()),
            led_start_latch: Mutex::new([[0; 24]; 2]),
        }
    }
}

struct DeviceHandle {
    player: u8,
    pid: u16,
    // [touch] pxDebugInput from segatools.ini: gates the verbose per-player TouchDiag
    // log line. With a custom mai2 IO the segatools hook/built-in ignore this key, so
    // Affine reuses it as a convenient "debug" toggle. It only controls logging, never
    // input — touch flows via mai2_io_touch_update -> the callback regardless of it.
    diag: bool,
    enabled: AtomicBool,
    hid_connected: AtomicBool,
    vendor_connected: AtomicBool,
    touch_hid_connected: AtomicBool,
    touch_frames: AtomicU64,
    touch_serial_frames: AtomicU64,
    touch_hid_frames: AtomicU64,
    touch_callback_frames: AtomicU64,
    touch_last_update_ms: AtomicU64,
    heartbeat_writes: AtomicU64,
    heartbeat_failures: AtomicU64,
    led_writes: AtomicU64,
    led_button_writes: AtomicU64,
    led_billboard_writes: AtomicU64,
    led_pwm_writes: AtomicU64,
    led_failures: AtomicU64,
    led_suspended: AtomicBool,
    serial_reconnects: AtomicU64,
    vendor_reconnects: AtomicU64,
    input_page: SharedPage<Mai2InputPage>,
    output_page: SharedPage<Mai2OutputPage>,
}

impl DeviceHandle {
    fn new(player: u8, pid: u16, enabled: bool, diag: bool, index: usize) -> Arc<Self> {
        let input_page: SharedPage<Mai2InputPage> = SharedPage::create(
            MAI2_INPUT_MAPPING_NAMES[index],
            MAI2_INPUT_MUTEX_NAMES[index],
        )
        .expect("mai2 input shared memory");
        let output_page: SharedPage<Mai2OutputPage> = SharedPage::create(
            MAI2_OUTPUT_MAPPING_NAMES[index],
            MAI2_OUTPUT_MUTEX_NAMES[index],
        )
        .expect("mai2 output shared memory");

        output_page.update(|page| {
            page.touch_enabled = enabled as u8;
        });

        Arc::new(Self {
            player,
            pid,
            diag,
            enabled: AtomicBool::new(enabled),
            hid_connected: AtomicBool::new(false),
            vendor_connected: AtomicBool::new(false),
            touch_hid_connected: AtomicBool::new(false),
            touch_frames: AtomicU64::new(0),
            touch_serial_frames: AtomicU64::new(0),
            touch_hid_frames: AtomicU64::new(0),
            touch_callback_frames: AtomicU64::new(0),
            touch_last_update_ms: AtomicU64::new(0),
            heartbeat_writes: AtomicU64::new(0),
            heartbeat_failures: AtomicU64::new(0),
            led_writes: AtomicU64::new(0),
            led_button_writes: AtomicU64::new(0),
            led_billboard_writes: AtomicU64::new(0),
            led_pwm_writes: AtomicU64::new(0),
            led_failures: AtomicU64::new(0),
            led_suspended: AtomicBool::new(false),
            serial_reconnects: AtomicU64::new(0),
            vendor_reconnects: AtomicU64::new(0),
            input_page,
            output_page,
        })
    }
}

struct SerialSession {
    port: SerialPort,
    active: AtomicBool,
    reader_failed: AtomicBool,
    board_info_pending: AtomicBool,
    board_info_logged: AtomicBool,
}

impl SerialSession {
    fn new(port: SerialPort) -> Arc<Self> {
        Arc::new(Self {
            port,
            active: AtomicBool::new(true),
            reader_failed: AtomicBool::new(false),
            board_info_pending: AtomicBool::new(true),
            board_info_logged: AtomicBool::new(false),
        })
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst) && !self.reader_failed.load(Ordering::SeqCst)
    }

    fn read(&self, buf: &mut [u8]) -> Option<usize> {
        if !self.active.load(Ordering::SeqCst) {
            return Some(0);
        }

        self.port.read(buf)
    }

    fn write_frame(&self, cmd: u8, payload: &[u8]) -> bool {
        if !self.active.load(Ordering::SeqCst) {
            return false;
        }

        send_frame(&self.port, cmd, payload)
    }

    fn close(&self) {
        self.active.store(false, Ordering::SeqCst);
        self.port.close();
    }
}

pub struct Mai2Runtime {
    shared: Arc<SharedState>,
    devices: [Arc<DeviceHandle>; 2],
    poll_page: SharedPage<Mai2PollPage>,
    keyboard: KeyboardConfig,
    initialized: AtomicBool,
}

static MAI2_RUNTIME: OnceLock<Arc<Mai2Runtime>> = OnceLock::new();

pub fn runtime() -> &'static Arc<Mai2Runtime> {
    MAI2_RUNTIME.get_or_init(Mai2Runtime::new)
}

impl Mai2Runtime {
    fn new() -> Arc<Self> {
        let config_path = segatools_config_path();
        let p1_enabled = ini_get_bool(&config_path, "touch", "p1Enable", true);
        let p2_enabled = ini_get_bool(&config_path, "touch", "p2Enable", true);
        let p1_diag = ini_get_bool(&config_path, "touch", "p1DebugInput", false);
        let p2_diag = ini_get_bool(&config_path, "touch", "p2DebugInput", false);
        let keyboard = KeyboardConfig {
            vk_test: ini_get_u32(&config_path, "io4", "test", VK_F1 as u32) as u16,
            vk_service: ini_get_u32(&config_path, "io4", "service", VK_F2 as u32) as u16 as u16,
            vk_coin: ini_get_u32(&config_path, "io4", "coin", VK_F3 as u32) as u16,
        };
        log_line(&format!(
            "Config: p1Enable={} p2Enable={} p1DebugInput={} p2DebugInput={}",
            p1_enabled as u8, p2_enabled as u8, p1_diag as u8, p2_diag as u8
        ));
        log_line(&format!(
            "Keyboard opbtn fallback: test=0x{:X} service=0x{:X} coin=0x{:X}",
            keyboard.vk_test, keyboard.vk_service, keyboard.vk_coin
        ));
        if AFFINE_LED_PROBE_ENABLED {
            log_line("Mai2IO LED probe enabled: logging changed led_gs_update RGBS frames");
        }

        let shared = Arc::new(SharedState::new());
        let p1 = DeviceHandle::new(1, MAI2_PID_1P, p1_enabled, p1_diag, 0);
        let p2 = DeviceHandle::new(2, MAI2_PID_2P, p2_enabled, p2_diag, 1);
        let poll_page: SharedPage<Mai2PollPage> =
            SharedPage::create(MAI2_POLL_MAPPING_NAME, MAI2_POLL_MUTEX_NAME)
                .expect("mai2 poll shared memory");

        Arc::new(Self {
            shared,
            devices: [p1, p2],
            poll_page,
            keyboard,
            initialized: AtomicBool::new(false),
        })
    }

    pub fn init(&self) -> Hresult {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return S_OK;
        }

        log_line("Initializing...");
        log_line(&format!("Affine IO Version: {}", affine_core::version()));
        log_line("Mai2IO API Version: 1.02");

        if let Some(exe_name) = current_exe_name() {
            log_line(&format!("Running in {exe_name}"));
            if !exe_name.eq_ignore_ascii_case("Sinmai.exe") {
                log_line(&format!("Skipping device init for {exe_name}"));
                log_line("Initialization complete.");
                return S_OK;
            }
        }

        for device in &self.devices {
            let device = device.clone();
            let shared = self.shared.clone();
            let serial_device = device.clone();
            let serial_shared = shared.clone();
            let vendor_device = device.clone();
            let vendor_shared = shared.clone();
            let touch_hid_device = device.clone();
            let touch_hid_shared = shared.clone();
            let touch_device = device.clone();
            let touch_shared = shared.clone();
            thread::spawn(move || vendor_command_thread(vendor_device, vendor_shared));
            thread::spawn(move || touch_hid_thread(touch_hid_device, touch_hid_shared));
            thread::spawn(move || device_thread(serial_device, serial_shared));
            thread::spawn(move || hid_thread(device, shared));
            thread::spawn(move || touch_callback_thread(touch_device, touch_shared));
        }

        log_line("Initialization complete.");
        S_OK
    }

    pub fn poll(&self) -> Hresult {
        let p1 = self.devices[0].input_page.read();
        let p2 = self.devices[1].input_page.read();

        self.poll_page.update(|poll_state| {
            let mut opbtn = 0;
            let mut player1_btn = 0;
            let mut player2_btn = 0;
            let mut coin_pressed = false;

            if p1.connected != 0 {
                player1_btn = map_buttons(p1.buttons0, p1.io_status);

                if p1.io_status & (1 << MAI2_AFFINE_EXT_TEST_BIT) != 0 {
                    opbtn |= MAI2_IO_OPBTN_TEST;
                }
                if p1.io_status & (1 << MAI2_AFFINE_EXT_SERVICE_BIT) != 0 {
                    opbtn |= MAI2_IO_OPBTN_SERVICE;
                }
                if p1.io_status & (1 << MAI2_AFFINE_EXT_COIN_BIT) != 0 {
                    coin_pressed = true;
                }
            }

            if p2.connected != 0 {
                player2_btn = map_buttons(p2.buttons0, p2.io_status);

                if p2.io_status & (1 << MAI2_AFFINE_EXT_TEST_BIT) != 0 {
                    opbtn |= MAI2_IO_OPBTN_TEST;
                }
                if p2.io_status & (1 << MAI2_AFFINE_EXT_SERVICE_BIT) != 0 {
                    opbtn |= MAI2_IO_OPBTN_SERVICE;
                }
            }

            if key_down(self.keyboard.vk_test) {
                opbtn |= MAI2_IO_OPBTN_TEST;
            }
            if key_down(self.keyboard.vk_service) {
                opbtn |= MAI2_IO_OPBTN_SERVICE;
            }
            if key_down(self.keyboard.vk_coin) {
                coin_pressed = true;
            }

            // Coin is an edge-triggered pulse: assert OPBTN_COIN for exactly one
            // poll on the rising edge so a held coin source (hardware OR keyboard)
            // registers a single credit instead of repeating every poll.
            if coin_pressed {
                if poll_state.affine_coin == 0 {
                    poll_state.affine_coin = 1;
                    opbtn |= MAI2_IO_OPBTN_COIN;
                }
            } else {
                poll_state.affine_coin = 0;
            }

            poll_state.opbtn = opbtn;
            poll_state.player1_btn = player1_btn;
            poll_state.player2_btn = player2_btn;
            poll_state.sequence = poll_state.sequence.wrapping_add(1);
        });

        S_OK
    }

    pub fn get_opbtns(&self) -> u8 {
        self.poll_page.read().opbtn
    }

    pub fn get_gamebtns(&self) -> (u16, u16) {
        let page = self.poll_page.read();
        (page.player1_btn, page.player2_btn)
    }

    pub fn set_touch_callback(&self, callback: Mai2TouchCallback) {
        *self.shared.callback.lock().unwrap() = callback;
    }

    pub fn set_touch_enabled(&self, player1: bool, player2: bool) {
        self.devices[0]
            .output_page
            .update(|page| page.touch_enabled = player1 as u8);
        self.devices[1]
            .output_page
            .update(|page| page.touch_enabled = player2 as u8);
    }

    pub fn led_init(&self) -> Hresult {
        // LED rendering is driven directly by led_gs_update writing the output page;
        // there is no separate fade thread. led_init only ensures the runtime is up.
        self.init()
    }

    pub fn led_set_fet_output(&self, board: u8, rgb: [u8; 3]) {
        if let Some(device) = self.devices.get(board as usize) {
            device.output_page.update(|page| {
                if page.pwm != rgb {
                    page.pwm = rgb;
                    page.pwm_sequence = page.pwm_sequence.wrapping_add(1);
                }
            });
        }
    }

    pub fn led_gs_update(&self, board: u8, rgb: &[u8]) {
        if board > 1 || rgb.len() < 32 {
            return;
        }

        let board = board as usize;
        let mut rgbs = [0u8; 32];

        for i in 0..8 {
            rgbs[i * 4] = rgb[i * 4];
            rgbs[i * 4 + 1] = rgb[i * 4 + 1];
            rgbs[i * 4 + 2] = rgb[i * 4 + 2];
            rgbs[i * 4 + 3] = rgb[i * 4 + 3];
        }

        log_led_probe(&self.shared, board, &rgbs);

        let payload = {
            let mut latch = self.shared.led_start_latch.lock().unwrap();
            let mut payload = [0u8; 56];

            for i in 0..8 {
                let in_offset = i * 4;
                let latch_offset = i * 3;
                let out_offset = i * 7;
                let target = [rgbs[in_offset], rgbs[in_offset + 1], rgbs[in_offset + 2]];
                let speed = rgbs[in_offset + 3];
                let start = if speed == 0 {
                    target
                } else {
                    [
                        latch[board][latch_offset],
                        latch[board][latch_offset + 1],
                        latch[board][latch_offset + 2],
                    ]
                };

                payload[out_offset..out_offset + 3].copy_from_slice(&start);
                payload[out_offset + 3..out_offset + 6].copy_from_slice(&target);
                payload[out_offset + 6] = speed;
                latch[board][latch_offset..latch_offset + 3].copy_from_slice(&target);
            }

            payload
        };

        self.devices[board].output_page.update(|page| {
            if page.buttons != payload {
                page.buttons = payload;
                page.buttons_sequence = page.buttons_sequence.wrapping_add(1);
            }
        });
    }

    pub fn led_billboard_set(&self, board: u8, rgb: &[u8]) {
        if board > 1 || rgb.len() < 3 {
            return;
        }

        let mut payload = [0u8; 24];
        for chunk in payload.chunks_exact_mut(3) {
            chunk[0] = rgb[0];
            chunk[1] = rgb[1];
            chunk[2] = rgb[2];
        }

        self.devices[board as usize].output_page.update(|page| {
            if page.billboard != payload {
                page.billboard = payload;
                page.billboard_sequence = page.billboard_sequence.wrapping_add(1);
            }
        });
    }

    #[cfg(feature = "latency-bench")]
    pub fn bench_inject_input(&self, player: u8, buttons0: u8, buttons1: u8, touch: [u8; 7]) {
        let Some(device) = self.devices.get(player.saturating_sub(1) as usize) else {
            return;
        };

        apply_device_frame(
            device,
            &self.shared,
            Some(buttons0),
            Some(buttons1),
            Some(touch),
            TouchSource::Synthetic,
        );
    }
}

fn log_led_probe(shared: &SharedState, board: usize, payload: &[u8; 32]) {
    if !AFFINE_LED_PROBE_ENABLED {
        return;
    }

    let now = tick_ms();
    let mut state = shared.led_probe.lock().unwrap();
    let board_state = &mut state.boards[board];

    if board_state.has_last && board_state.last_payload == *payload {
        return;
    }

    let mut zero_count = 0usize;
    let mut fade_count = 0usize;
    for index in 0..8 {
        if payload[index * 4 + 3] == 0 {
            zero_count += 1;
        } else {
            fade_count += 1;
        }
    }

    let dt = if board_state.has_last {
        format!("{}ms", now.saturating_sub(board_state.last_ms))
    } else {
        String::from("first")
    };

    board_state.sequence = board_state.sequence.wrapping_add(1);

    let mut detail = String::new();
    format_led_probe_rgbs(&mut detail, payload);

    let zero_hint = if fade_count > 0 {
        if board_state.has_last_zero {
            let mut zero_detail = String::new();
            format_led_probe_rgb(&mut zero_detail, &board_state.last_zero_payload);
            format!(
                " last_zero_dt={}ms last_zero=[{}]",
                now.saturating_sub(board_state.last_zero_ms),
                zero_detail
            )
        } else {
            String::from(" last_zero=none")
        }
    } else {
        String::new()
    };

    log_diag(&format!(
        "P{} LedProbe#{:06} dt={} zero={} fade={} rgbs=[{}]{}",
        board + 1,
        board_state.sequence,
        dt,
        zero_count,
        fade_count,
        detail,
        zero_hint
    ));

    if zero_count == 8 {
        board_state.has_last_zero = true;
        board_state.last_zero_ms = now;
        board_state.last_zero_payload = *payload;
    }

    board_state.has_last = true;
    board_state.last_ms = now;
    board_state.last_payload = *payload;
}

fn format_led_probe_rgbs(out: &mut String, payload: &[u8; 32]) {
    for index in 0..8 {
        if index != 0 {
            out.push(' ');
        }
        let offset = index * 4;
        let _ = write!(
            out,
            "{}:{:02X}{:02X}{:02X}/s{:02X}",
            index,
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3]
        );
    }
}

fn format_led_probe_rgb(out: &mut String, payload: &[u8; 32]) {
    for index in 0..8 {
        if index != 0 {
            out.push(' ');
        }
        let offset = index * 4;
        let _ = write!(
            out,
            "{}:{:02X}{:02X}{:02X}",
            index,
            payload[offset],
            payload[offset + 1],
            payload[offset + 2]
        );
    }
}

fn button_rgb_payload(buttons: &[u8; 56]) -> [u8; 24] {
    let mut payload = [0u8; 24];
    for index in 0..8 {
        payload[index * 3] = buttons[index * 7 + 3];
        payload[index * 3 + 1] = buttons[index * 7 + 4];
        payload[index * 3 + 2] = buttons[index * 7 + 5];
    }
    payload
}

fn device_thread(device: Arc<DeviceHandle>, shared: Arc<SharedState>) {
    let mut session: Option<Arc<SerialSession>> = None;
    let mut reader_handle: Option<JoinHandle<()>> = None;
    let mut last_heartbeat = tick_ms();
    let mut board_info_start_ms = 0u64;
    let mut board_info_request_ms = 0u64;
    let mut last_buttons_sequence = 0u64;
    let mut last_billboard_sequence = 0u64;
    let mut last_pwm_sequence = 0u64;
    let mut connected_since_ms = 0u64;
    let mut last_touch_frames = device.touch_frames.load(Ordering::SeqCst);
    let mut led_output_suspended = false;
    let mut device_missing_logged = false;
    let mut port_open_failed_logged = false;
    let mut touch_stale_logged = false;

    loop {
        if !device.enabled.load(Ordering::SeqCst) {
            disconnect_session(&mut session, &mut reader_handle, &device);
            device_missing_logged = false;
            port_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        if device.vendor_connected.load(Ordering::SeqCst)
            || hid_interface_present(
                device.pid,
                MAI2_VENDOR_HID_USAGE_PAGE,
                MAI2_VENDOR_HID_USAGE,
            )
        {
            disconnect_session(&mut session, &mut reader_handle, &device);
            device_missing_logged = false;
            port_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        if session.is_none() {
            let Some(port_path) = find_com_port(AFFINE_VID, device.pid) else {
                if !device_missing_logged {
                    log_warn(&format!("P{}: Device not found", device.player));
                    device_missing_logged = true;
                }
                port_open_failed_logged = false;
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            };

            let mut port = SerialPort::default();
            if !port.open(&port_path, 115_200) {
                if !port_open_failed_logged {
                    log_warn(&format!(
                        "P{}: Failed to open port {port_path}",
                        device.player
                    ));
                    port_open_failed_logged = true;
                }
                device_missing_logged = false;
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }

            log_ok(&format!(
                "Connected P{}: {}",
                device.player,
                port_path.trim_start_matches("\\\\.\\")
            ));
            device.serial_reconnects.fetch_add(1, Ordering::SeqCst);
            let _ = port.set_timeouts(1, 1, 0, 50, 5);

            let opened_session = SerialSession::new(port);
            let reader_device = device.clone();
            let reader_shared = shared.clone();
            let reader_session = opened_session.clone();
            reader_handle = Some(thread::spawn(move || {
                serial_reader_thread(reader_device, reader_shared, reader_session);
            }));
            session = Some(opened_session);

            device_missing_logged = false;
            port_open_failed_logged = false;
            device.input_page.update(|page| {
                page.connected = 1;
                page.sequence = page.sequence.wrapping_add(1);
            });
            last_heartbeat = tick_ms();
            board_info_start_ms = tick_ms();
            board_info_request_ms = 0;
            last_buttons_sequence = 0;
            last_billboard_sequence = 0;
            last_pwm_sequence = 0;
            connected_since_ms = tick_ms();
        }

        let Some(active_session) = session.as_ref().cloned() else {
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };

        if !active_session.is_active() {
            disconnect_session(&mut session, &mut reader_handle, &device);
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        let now = tick_ms();

        if now.saturating_sub(last_heartbeat) >= AFFINE_HEARTBEAT_INTERVAL_MS {
            if !active_session.write_frame(AFFINE_CMD_HEARTBEAT, &[]) {
                device.heartbeat_failures.fetch_add(1, Ordering::SeqCst);
                disconnect_session(&mut session, &mut reader_handle, &device);
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            device.heartbeat_writes.fetch_add(1, Ordering::SeqCst);
            last_heartbeat = now;
        }

        if active_session.board_info_pending.load(Ordering::SeqCst) {
            if board_info_request_ms == 0
                && now.saturating_sub(board_info_start_ms) >= AFFINE_BOARD_INFO_DELAY_MS
            {
                if !active_session.write_frame(AFFINE_CMD_GET_BOARD_INFO, &[]) {
                    disconnect_session(&mut session, &mut reader_handle, &device);
                    sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                    continue;
                }
                board_info_request_ms = now;
            } else if board_info_request_ms != 0
                && now.saturating_sub(board_info_request_ms) >= AFFINE_BOARD_INFO_TIMEOUT_MS
            {
                if !active_session.write_frame(AFFINE_CMD_GET_BOARD_INFO, &[]) {
                    disconnect_session(&mut session, &mut reader_handle, &device);
                    sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                    continue;
                }
                board_info_request_ms = now;
                if now.saturating_sub(board_info_start_ms) > 3_000
                    && !active_session.board_info_logged.load(Ordering::SeqCst)
                {
                    active_session
                        .board_info_pending
                        .store(false, Ordering::SeqCst);
                    active_session
                        .board_info_logged
                        .store(true, Ordering::SeqCst);
                    log_warn(&format!("P{} Firmware: unknown", device.player));
                }
            }
        }

        let output = device.output_page.read();
        let touch_frames = device.touch_frames.load(Ordering::SeqCst);
        if touch_frames != last_touch_frames {
            last_touch_frames = touch_frames;
            if led_output_suspended {
                log_ok(&format!(
                    "P{}: Touch input recovered; resuming LED writes",
                    device.player
                ));
            }
            led_output_suspended = false;
            device.led_suspended.store(false, Ordering::SeqCst);
            touch_stale_logged = false;
        }

        let touch_age_ms = touch_serial_age_ms(&device, now, connected_since_ms);
        let touch_protect_active = output.touch_enabled != 0
            && connected_since_ms != 0
            && now.saturating_sub(connected_since_ms) >= AFFINE_TOUCH_STALE_GRACE_MS;

        let touch_stale = touch_protect_active
            && touch_age_ms
                .map(|age| age >= AFFINE_TOUCH_STALE_LED_PAUSE_MS)
                .unwrap_or(false);

        if touch_stale {
            led_output_suspended = true;
            device.led_suspended.store(true, Ordering::SeqCst);
            if !touch_stale_logged {
                let age = touch_age_ms.unwrap_or_default();
                log_warn(&format!(
                    "P{}: Touch input stale for {age}ms; pausing LED writes",
                    device.player
                ));
                touch_stale_logged = true;
            }
        }

        if touch_protect_active
            && touch_age_ms
                .map(|age| age >= AFFINE_TOUCH_STALE_RECONNECT_MS)
                .unwrap_or(false)
        {
            let age = touch_age_ms.unwrap_or_default();
            log_warn(&format!(
                "P{}: Touch input stalled for {age}ms; reconnecting serial",
                device.player
            ));
            disconnect_session(&mut session, &mut reader_handle, &device);
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        if led_output_suspended {
            sleep_ms(AFFINE_DEVICE_LOOP_SLEEP_MS);
            continue;
        }

        touch_stale_logged = false;

        if output.buttons_sequence != last_buttons_sequence {
            let buttons_payload = button_rgb_payload(&output.buttons);
            if !active_session.write_frame(0x14, &buttons_payload) {
                device.led_failures.fetch_add(1, Ordering::SeqCst);
                disconnect_session(&mut session, &mut reader_handle, &device);
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            device.led_writes.fetch_add(1, Ordering::SeqCst);
            device.led_button_writes.fetch_add(1, Ordering::SeqCst);
            last_buttons_sequence = output.buttons_sequence;
        }

        if output.billboard_sequence != last_billboard_sequence {
            if !active_session.write_frame(0x15, &output.billboard) {
                device.led_failures.fetch_add(1, Ordering::SeqCst);
                disconnect_session(&mut session, &mut reader_handle, &device);
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            device.led_writes.fetch_add(1, Ordering::SeqCst);
            device.led_billboard_writes.fetch_add(1, Ordering::SeqCst);
            last_billboard_sequence = output.billboard_sequence;
        }

        if output.pwm_sequence != last_pwm_sequence {
            if !active_session.write_frame(0x16, &output.pwm) {
                device.led_failures.fetch_add(1, Ordering::SeqCst);
                disconnect_session(&mut session, &mut reader_handle, &device);
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            device.led_writes.fetch_add(1, Ordering::SeqCst);
            device.led_pwm_writes.fetch_add(1, Ordering::SeqCst);
            last_pwm_sequence = output.pwm_sequence;
        }

        sleep_ms(AFFINE_DEVICE_LOOP_SLEEP_MS);
    }
}

fn serial_reader_thread(
    device: Arc<DeviceHandle>,
    shared: Arc<SharedState>,
    session: Arc<SerialSession>,
) {
    let mut rx_buf = [0u8; AFFINE_SERIAL_RX_BUF_LEN];
    let mut rx_len = 0usize;
    let mut read_buf = [0u8; AFFINE_SERIAL_READ_BUF_LEN];

    while session.active.load(Ordering::SeqCst) {
        let Some(read) = session.read(&mut read_buf) else {
            session.reader_failed.store(true, Ordering::SeqCst);
            session.active.store(false, Ordering::SeqCst);
            break;
        };

        if read == 0 {
            continue;
        }

        for &byte in &read_buf[..read] {
            if rx_len >= rx_buf.len() {
                rx_len = 0;
            }
            rx_buf[rx_len] = byte;
            rx_len += 1;

            while let Some(frame) = try_parse_frame(&mut rx_buf, &mut rx_len) {
                match frame {
                    ParsedFrame::Touch {
                        buttons0,
                        io_status,
                        touch,
                    } => {
                        let use_serial_buttons = !device.hid_connected.load(Ordering::SeqCst);
                        apply_device_frame(
                            &device,
                            &shared,
                            if use_serial_buttons { buttons0 } else { None },
                            if use_serial_buttons { io_status } else { None },
                            touch,
                            TouchSource::Serial,
                        );
                    }
                    ParsedFrame::BoardInfo(version) => {
                        session.board_info_pending.store(false, Ordering::SeqCst);
                        session.board_info_logged.store(true, Ordering::SeqCst);
                        log_line(&format!("P{} Firmware: {version}", device.player));
                    }
                }
            }
        }
    }
}

fn touch_serial_age_ms(device: &DeviceHandle, now: u64, connected_since_ms: u64) -> Option<u64> {
    let last_update_ms = device.touch_last_update_ms.load(Ordering::SeqCst);
    if last_update_ms != 0 {
        return Some(now.saturating_sub(last_update_ms));
    }
    if connected_since_ms != 0 {
        return Some(now.saturating_sub(connected_since_ms));
    }
    None
}

fn disconnect_session(
    session: &mut Option<Arc<SerialSession>>,
    reader_handle: &mut Option<JoinHandle<()>>,
    device: &DeviceHandle,
) {
    if let Some(active_session) = session.take() {
        active_session.close();
    }

    if let Some(handle) = reader_handle.take() {
        let _ = handle.join();
    }

    if device.vendor_connected.load(Ordering::SeqCst)
        || device.touch_hid_connected.load(Ordering::SeqCst)
        || device.hid_connected.load(Ordering::SeqCst)
    {
        device.input_page.update(|page| {
            page.connected = 1;
            page.sequence = page.sequence.wrapping_add(1);
        });
    } else {
        device.input_page.write(Mai2InputPage::default());
    }
}

/// Zero the input fields a just-dropped source was feeding, but only when no other
/// live source still provides them. Prevents a disconnected HID source from leaving
/// the game reading held buttons/touch across a reconnect; the next live source
/// overwrites within one frame. Buttons are served by the button-HID thread or the
/// vendor command stream; touch by the touch-HID thread or the vendor command stream.
/// Call AFTER the dropped source has cleared its own `*_connected` flag.
fn clear_inputs_on_source_drop(device: &DeviceHandle) {
    let buttons_live = device.hid_connected.load(Ordering::SeqCst)
        || device.vendor_connected.load(Ordering::SeqCst);
    let touch_live = device.touch_hid_connected.load(Ordering::SeqCst)
        || device.vendor_connected.load(Ordering::SeqCst);
    if buttons_live && touch_live {
        return;
    }
    device.input_page.update(|page| {
        let mut changed = false;
        if !buttons_live && (page.buttons0 != 0 || page.io_status != 0) {
            page.buttons0 = 0;
            page.io_status = 0;
            changed = true;
        }
        if !touch_live && page.touch != [0u8; 7] {
            page.touch = [0u8; 7];
            changed = true;
        }
        if changed {
            page.sequence = page.sequence.wrapping_add(1);
        }
    });
}

fn vendor_command_thread(device: Arc<DeviceHandle>, shared: Arc<SharedState>) {
    let mut hid_unavailable_logged = false;
    let mut hid_missing_logged = false;
    let mut hid_open_failed_logged = false;

    loop {
        if !device.enabled.load(Ordering::SeqCst) {
            device.vendor_connected.store(false, Ordering::SeqCst);
            hid_unavailable_logged = false;
            hid_missing_logged = false;
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        let Ok(api) = HidApi::new() else {
            if !hid_unavailable_logged {
                log_warn(&format!("P{}: HID subsystem unavailable", device.player));
                hid_unavailable_logged = true;
            }
            hid_missing_logged = false;
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_unavailable_logged = false;

        let Some(path) = find_hid_path(
            &api,
            AFFINE_VID,
            device.pid,
            MAI2_VENDOR_HID_USAGE_PAGE,
            MAI2_VENDOR_HID_USAGE,
        ) else {
            if !hid_missing_logged {
                log_warn(&format!(
                    "P{}: Vendor HID command interface not found",
                    device.player
                ));
                hid_missing_logged = true;
            }
            device.vendor_connected.store(false, Ordering::SeqCst);
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_missing_logged = false;

        let Ok(hid) = api.open_path(&path) else {
            if !hid_open_failed_logged {
                log_warn(&format!("P{}: Failed to open Vendor HID", device.player));
                hid_open_failed_logged = true;
            }
            device.vendor_connected.store(false, Ordering::SeqCst);
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_open_failed_logged = false;

        log_ok(&format!("Connected P{} Vendor HID command", device.player));
        device.vendor_connected.store(true, Ordering::SeqCst);
        device.vendor_reconnects.fetch_add(1, Ordering::SeqCst);
        device.input_page.update(|page| {
            page.connected = 1;
            page.sequence = page.sequence.wrapping_add(1);
        });

        let mut last_heartbeat = tick_ms();
        let board_info_start_ms = tick_ms();
        let mut board_info_request_ms = 0u64;
        let mut board_info_pending = true;
        let mut board_info_logged = false;
        let mut last_buttons_sequence = 0u64;
        let mut last_billboard_sequence = 0u64;
        let mut last_pwm_sequence = 0u64;

        loop {
            if !device.enabled.load(Ordering::SeqCst) {
                break;
            }

            if !vendor_command_read_once(
                &hid,
                &device,
                &shared,
                &mut board_info_pending,
                &mut board_info_logged,
            ) {
                break;
            }

            let now = tick_ms();

            if now.saturating_sub(last_heartbeat) >= AFFINE_HEARTBEAT_INTERVAL_MS {
                if !send_hid_frame(&hid, AFFINE_CMD_HEARTBEAT, &[]) {
                    device.heartbeat_failures.fetch_add(1, Ordering::SeqCst);
                    break;
                }
                device.heartbeat_writes.fetch_add(1, Ordering::SeqCst);
                last_heartbeat = now;
            }

            if board_info_pending {
                if board_info_request_ms == 0
                    && now.saturating_sub(board_info_start_ms) >= AFFINE_BOARD_INFO_DELAY_MS
                {
                    if !send_hid_frame(&hid, AFFINE_CMD_GET_BOARD_INFO, &[]) {
                        break;
                    }
                    board_info_request_ms = now;
                } else if board_info_request_ms != 0
                    && now.saturating_sub(board_info_request_ms) >= AFFINE_BOARD_INFO_TIMEOUT_MS
                {
                    if !send_hid_frame(&hid, AFFINE_CMD_GET_BOARD_INFO, &[]) {
                        break;
                    }
                    board_info_request_ms = now;
                    if now.saturating_sub(board_info_start_ms) > 3_000 && !board_info_logged {
                        board_info_pending = false;
                        board_info_logged = true;
                        log_warn(&format!("P{} Firmware: unknown", device.player));
                    }
                }
            }

            let output = device.output_page.read();

            if output.buttons_sequence != last_buttons_sequence {
                if !send_hid_frame(&hid, 0x14, &output.buttons) {
                    device.led_failures.fetch_add(1, Ordering::SeqCst);
                    break;
                }
                device.led_writes.fetch_add(1, Ordering::SeqCst);
                device.led_button_writes.fetch_add(1, Ordering::SeqCst);
                last_buttons_sequence = output.buttons_sequence;
            }

            if output.billboard_sequence != last_billboard_sequence {
                if !send_hid_frame(&hid, 0x15, &output.billboard) {
                    device.led_failures.fetch_add(1, Ordering::SeqCst);
                    break;
                }
                device.led_writes.fetch_add(1, Ordering::SeqCst);
                device.led_billboard_writes.fetch_add(1, Ordering::SeqCst);
                last_billboard_sequence = output.billboard_sequence;
            }

            if output.pwm_sequence != last_pwm_sequence {
                if !send_hid_frame(&hid, 0x16, &output.pwm) {
                    device.led_failures.fetch_add(1, Ordering::SeqCst);
                    break;
                }
                device.led_writes.fetch_add(1, Ordering::SeqCst);
                device.led_pwm_writes.fetch_add(1, Ordering::SeqCst);
                last_pwm_sequence = output.pwm_sequence;
            }

            sleep_ms(AFFINE_DEVICE_LOOP_SLEEP_MS);
        }

        device.vendor_connected.store(false, Ordering::SeqCst);
        clear_inputs_on_source_drop(&device);
        sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
    }
}

fn vendor_command_read_once(
    hid: &HidDevice,
    device: &DeviceHandle,
    shared: &SharedState,
    board_info_pending: &mut bool,
    board_info_logged: &mut bool,
) -> bool {
    let mut report = [0u8; MAI2_VENDOR_HID_REPORT_LEN];

    match hid.read_timeout(&mut report, MAI2_VENDOR_HID_READ_TIMEOUT_MS) {
        Ok(0) => true,
        Ok(read) => {
            let mut rx_buf = [0u8; AFFINE_SERIAL_RX_BUF_LEN];
            let mut rx_len = read.min(rx_buf.len());
            rx_buf[..rx_len].copy_from_slice(&report[..rx_len]);

            while let Some(frame) = try_parse_frame(&mut rx_buf, &mut rx_len) {
                match frame {
                    ParsedFrame::Touch {
                        buttons0,
                        io_status,
                        touch,
                    } => {
                        let use_hid_buttons = !device.hid_connected.load(Ordering::SeqCst);
                        apply_device_frame(
                            device,
                            shared,
                            if use_hid_buttons { buttons0 } else { None },
                            if use_hid_buttons { io_status } else { None },
                            touch,
                            TouchSource::Synthetic,
                        );
                    }
                    ParsedFrame::BoardInfo(version) => {
                        *board_info_pending = false;
                        *board_info_logged = true;
                        log_line(&format!("P{} Firmware: {version}", device.player));
                    }
                }
            }
            true
        }
        Err(_) => false,
    }
}

fn touch_hid_thread(device: Arc<DeviceHandle>, shared: Arc<SharedState>) {
    let mut hid_unavailable_logged = false;
    let mut hid_missing_logged = false;
    let mut hid_open_failed_logged = false;

    loop {
        if !device.enabled.load(Ordering::SeqCst) {
            device.touch_hid_connected.store(false, Ordering::SeqCst);
            hid_unavailable_logged = false;
            hid_missing_logged = false;
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        let Ok(api) = HidApi::new() else {
            if !hid_unavailable_logged {
                log_warn(&format!("P{}: HID subsystem unavailable", device.player));
                hid_unavailable_logged = true;
            }
            hid_missing_logged = false;
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_unavailable_logged = false;

        let Some(path) = find_hid_path(
            &api,
            AFFINE_VID,
            device.pid,
            MAI2_TOUCH_HID_USAGE_PAGE,
            MAI2_TOUCH_HID_USAGE,
        ) else {
            if !hid_missing_logged {
                log_warn(&format!(
                    "P{}: Touch HID stream interface not found",
                    device.player
                ));
                hid_missing_logged = true;
            }
            device.touch_hid_connected.store(false, Ordering::SeqCst);
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_missing_logged = false;

        let Ok(hid) = api.open_path(&path) else {
            if !hid_open_failed_logged {
                log_warn(&format!("P{}: Failed to open Touch HID", device.player));
                hid_open_failed_logged = true;
            }
            device.touch_hid_connected.store(false, Ordering::SeqCst);
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_open_failed_logged = false;

        log_ok(&format!("Connected P{} Touch HID stream", device.player));
        device.touch_hid_connected.store(true, Ordering::SeqCst);
        device.input_page.update(|page| {
            page.connected = 1;
            page.sequence = page.sequence.wrapping_add(1);
        });

        let mut assembler = TouchHidAssembler::default();

        loop {
            if !device.enabled.load(Ordering::SeqCst) {
                break;
            }

            let mut report = [0u8; MAI2_TOUCH_HID_READ_BUF_LEN];
            match hid.read_timeout(&mut report, MAI2_TOUCH_HID_READ_TIMEOUT_MS) {
                Ok(0) => continue,
                Ok(read) => {
                    if let Some(part) = parse_touch_hid_part(&report[..read])
                        && let Some(touch) = assembler.push(part)
                    {
                        apply_device_frame(
                            &device,
                            &shared,
                            None,
                            None,
                            Some(touch),
                            TouchSource::TouchHid,
                        );
                    }
                }
                Err(_) => break,
            }
        }

        device.touch_hid_connected.store(false, Ordering::SeqCst);
        clear_inputs_on_source_drop(&device);
        sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
    }
}

fn hid_thread(device: Arc<DeviceHandle>, shared: Arc<SharedState>) {
    let mut hid_unavailable_logged = false;
    let mut hid_missing_logged = false;
    let mut hid_open_failed_logged = false;

    loop {
        if !device.enabled.load(Ordering::SeqCst) {
            device.hid_connected.store(false, Ordering::SeqCst);
            hid_unavailable_logged = false;
            hid_missing_logged = false;
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        let Ok(api) = HidApi::new() else {
            if !hid_unavailable_logged {
                log_warn(&format!("P{}: HID subsystem unavailable", device.player));
                hid_unavailable_logged = true;
            }
            hid_missing_logged = false;
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_unavailable_logged = false;

        let Some(path) = find_hid_path(
            &api,
            AFFINE_VID,
            device.pid,
            MAI2_BUTTON_HID_USAGE_PAGE,
            MAI2_BUTTON_HID_USAGE,
        ) else {
            if !hid_missing_logged {
                log_warn(&format!(
                    "P{}: HID button interface not found",
                    device.player
                ));
                hid_missing_logged = true;
            }
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_missing_logged = false;

        let Ok(hid) = api.open_path(&path) else {
            if !hid_open_failed_logged {
                log_warn(&format!("P{}: Failed to open HID buttons", device.player));
                hid_open_failed_logged = true;
            }
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_open_failed_logged = false;

        log_ok(&format!("Connected P{} HID buttons", device.player));
        device.hid_connected.store(true, Ordering::SeqCst);

        loop {
            let mut report = [0u8; MAI2_BUTTON_HID_REPORT_LEN];
            match hid.read_timeout(&mut report, MAI2_HID_READ_TIMEOUT_MS) {
                Ok(0) => continue,
                Ok(read) if read >= 2 => {
                    // The custom-HID IN endpoint multiplexes button frames with two
                    // other report types on the same buffer: benchmark events
                    // (report[1] == 0xFF) and a raw-debug stream (report[0] == 0xA5 &&
                    // report[1] == 0x5A). Identify button frames positively: io_status
                    // (report[1]) carries at most 6 valid bits, so anything above 0x3F
                    // is not a button frame and must not be decoded as input.
                    if report[1] > 0x3F || (report[0] == 0xA5 && report[1] == 0x5A) {
                        continue;
                    }
                    apply_device_frame(
                        &device,
                        &shared,
                        Some(report[0]),
                        Some(report[1]),
                        None,
                        TouchSource::Synthetic,
                    );
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }

        device.hid_connected.store(false, Ordering::SeqCst);
        clear_inputs_on_source_drop(&device);
        sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
    }
}

fn touch_callback_thread(device: Arc<DeviceHandle>, shared: Arc<SharedState>) {
    let mut last_diag_ms = tick_ms();
    let mut last_touch_frames = 0u64;
    let mut last_serial_frames = 0u64;
    let mut last_touch_hid_frames = 0u64;
    let mut last_callback_frames = 0u64;
    let mut last_heartbeat_writes = 0u64;
    let mut last_led_writes = 0u64;
    let mut last_led_button_writes = 0u64;
    let mut last_led_billboard_writes = 0u64;
    let mut last_led_pwm_writes = 0u64;

    loop {
        if device.enabled.load(Ordering::SeqCst) && device.output_page.read().touch_enabled != 0 {
            let touch = device.input_page.read().touch;
            // Copy the (Copy) callback pointer out and release the lock BEFORE the
            // unsafe FFI call: a slow/hanging game callback must not block
            // set_touch_callback, and an unwind across the FFI boundary must not leave
            // the mutex poisoned while still held here.
            let callback = *shared.callback.lock().unwrap();
            if let Some(callback) = callback {
                unsafe {
                    callback(device.player, touch.as_ptr());
                }
                device.touch_callback_frames.fetch_add(1, Ordering::SeqCst);
            }

            let now = tick_ms();
            if device.diag && now.saturating_sub(last_diag_ms) >= AFFINE_TOUCH_DIAG_INTERVAL_MS {
                let touch_frames = device.touch_frames.load(Ordering::SeqCst);
                let serial_frames = device.touch_serial_frames.load(Ordering::SeqCst);
                let touch_hid_frames = device.touch_hid_frames.load(Ordering::SeqCst);
                let callback_frames = device.touch_callback_frames.load(Ordering::SeqCst);
                let last_update_ms = device.touch_last_update_ms.load(Ordering::SeqCst);
                let heartbeat_writes = device.heartbeat_writes.load(Ordering::SeqCst);
                let heartbeat_failures = device.heartbeat_failures.load(Ordering::SeqCst);
                let led_writes = device.led_writes.load(Ordering::SeqCst);
                let led_button_writes = device.led_button_writes.load(Ordering::SeqCst);
                let led_billboard_writes = device.led_billboard_writes.load(Ordering::SeqCst);
                let led_pwm_writes = device.led_pwm_writes.load(Ordering::SeqCst);
                let led_failures = device.led_failures.load(Ordering::SeqCst);
                let led_suspended = device.led_suspended.load(Ordering::SeqCst) as u8;
                let reconnects = device.serial_reconnects.load(Ordering::SeqCst);
                let vendor_reconnects = device.vendor_reconnects.load(Ordering::SeqCst);
                let last_update_age = if last_update_ms == 0 {
                    u64::MAX
                } else {
                    now.saturating_sub(last_update_ms)
                };
                let last_update_display = if last_update_age == u64::MAX {
                    String::from("never")
                } else {
                    format!("{last_update_age}ms")
                };

                log_diag(&format!(
                    "P{} diag link  buttons_hid={} vendor_hid={} touch_hid={} suspend={} reconn=s{}/v{} last_update={}",
                    device.player,
                    device.hid_connected.load(Ordering::SeqCst) as u8,
                    device.vendor_connected.load(Ordering::SeqCst) as u8,
                    device.touch_hid_connected.load(Ordering::SeqCst) as u8,
                    led_suspended,
                    reconnects,
                    vendor_reconnects,
                    last_update_display,
                ));
                log_diag(&format!(
                    "P{} diag rate  touch={} (+{}) cdc={} (+{}) touch_hid={} (+{}) cb={} (+{}) hb={} (+{}) hb_fail={}",
                    device.player,
                    touch_frames,
                    touch_frames.saturating_sub(last_touch_frames),
                    serial_frames,
                    serial_frames.saturating_sub(last_serial_frames),
                    touch_hid_frames,
                    touch_hid_frames.saturating_sub(last_touch_hid_frames),
                    callback_frames,
                    callback_frames.saturating_sub(last_callback_frames),
                    heartbeat_writes,
                    heartbeat_writes.saturating_sub(last_heartbeat_writes),
                    heartbeat_failures,
                ));
                log_diag(&format!(
                    "P{} diag led   set={} (+{}) btn={} (+{}) bb={} (+{}) pwm={} (+{}) fail={} | touch={:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    device.player,
                    led_writes,
                    led_writes.saturating_sub(last_led_writes),
                    led_button_writes,
                    led_button_writes.saturating_sub(last_led_button_writes),
                    led_billboard_writes,
                    led_billboard_writes.saturating_sub(last_led_billboard_writes),
                    led_pwm_writes,
                    led_pwm_writes.saturating_sub(last_led_pwm_writes),
                    led_failures,
                    touch[0],
                    touch[1],
                    touch[2],
                    touch[3],
                    touch[4],
                    touch[5],
                    touch[6],
                ));

                last_diag_ms = now;
                last_touch_frames = touch_frames;
                last_serial_frames = serial_frames;
                last_touch_hid_frames = touch_hid_frames;
                last_callback_frames = callback_frames;
                last_heartbeat_writes = heartbeat_writes;
                last_led_writes = led_writes;
                last_led_button_writes = led_button_writes;
                last_led_billboard_writes = led_billboard_writes;
                last_led_pwm_writes = led_pwm_writes;
            }
        }

        sleep_ms(1);
    }
}

enum ParsedFrame {
    Touch {
        buttons0: Option<u8>,
        io_status: Option<u8>,
        touch: Option<[u8; 7]>,
    },
    BoardInfo(String),
}

#[derive(Clone, Copy)]
enum TouchSource {
    Serial,
    TouchHid,
    Synthetic,
}

#[derive(Clone, Copy)]
struct TouchHidPart {
    stream_seq: u8,
    part_index: u8,
    part_count: u8,
    touch_bits: [u8; 5],
}

#[derive(Default)]
struct TouchHidAssembler {
    stream_seq: u8,
    part_mask: u8,
    touch_bits: [u8; 5],
}

impl TouchHidAssembler {
    fn push(&mut self, part: TouchHidPart) -> Option<[u8; 7]> {
        if part.part_count != MAI2_TOUCH_HID_PART_COUNT || part.part_index >= part.part_count {
            return None;
        }

        if self.part_mask == 0 || self.stream_seq != part.stream_seq {
            self.stream_seq = part.stream_seq;
            self.part_mask = 0;
            self.touch_bits = [0; 5];
        }

        self.touch_bits = part.touch_bits;
        self.part_mask |= 1u8 << part.part_index;

        if self.part_mask == ((1u8 << part.part_count) - 1) {
            self.part_mask = 0;
            Some(pack_legacy_touch_bits(&self.touch_bits))
        } else {
            None
        }
    }
}

fn try_parse_frame(rx_buf: &mut [u8], rx_len: &mut usize) -> Option<ParsedFrame> {
    loop {
        if *rx_len == 0 {
            return None;
        }

        if rx_buf[0] == 0xFF {
            if *rx_len < 3 {
                return None;
            }

            if rx_buf[1] == 0x01 && (rx_buf[2] == 0x0A || rx_buf[2] == 0x00) {
                if *rx_len < 14 {
                    return None;
                }
                let checksum = rx_buf[..13]
                    .iter()
                    .fold(0u8, |sum, &byte| sum.wrapping_add(byte));
                if rx_buf[13] == checksum {
                    let buttons0 = (rx_buf[3] & 0x0F) | (rx_buf[4] & 0xF0);
                    let io_status = rx_buf[5] & 0x3F;
                    let mut touch = [0u8; 7];
                    touch.copy_from_slice(&rx_buf[6..13]);
                    consume(rx_buf, rx_len, 14);
                    return Some(ParsedFrame::Touch {
                        buttons0: Some(buttons0),
                        io_status: Some(io_status),
                        touch: Some(touch),
                    });
                }
            } else if rx_buf[1] == AFFINE_CMD_GET_BOARD_INFO {
                let total = 3 + rx_buf[2] as usize + 1;
                if total > rx_buf.len() {
                    consume(rx_buf, rx_len, 1);
                    continue;
                }
                if *rx_len < total {
                    return None;
                }

                let version = parse_board_info(&rx_buf[3..3 + rx_buf[2] as usize]);
                consume(rx_buf, rx_len, total);
                return Some(ParsedFrame::BoardInfo(version));
            }

            consume(rx_buf, rx_len, 1);
            continue;
        }

        if rx_buf[0] == 0x28 {
            if *rx_len < 9 {
                return None;
            }
            if rx_buf[8] == 0x29 {
                let mut touch = [0u8; 7];
                touch.copy_from_slice(&rx_buf[1..8]);
                consume(rx_buf, rx_len, 9);
                return Some(ParsedFrame::Touch {
                    buttons0: None,
                    io_status: None,
                    touch: Some(touch),
                });
            }

            consume(rx_buf, rx_len, 1);
            continue;
        }

        consume(rx_buf, rx_len, 1);
    }
}

fn consume(rx_buf: &mut [u8], rx_len: &mut usize, count: usize) {
    if count >= *rx_len {
        *rx_len = 0;
        return;
    }

    rx_buf.copy_within(count..*rx_len, 0);
    *rx_len -= count;
}

fn parse_board_info(data: &[u8]) -> String {
    if data.is_empty() {
        return String::from("unknown");
    }

    let length = data[0] as usize;
    if length == 0 || length + 1 > data.len() {
        return String::from("unknown");
    }

    String::from_utf8_lossy(&data[1..1 + length]).into_owned()
}

fn send_hid_frame(hid: &HidDevice, cmd: u8, payload: &[u8]) -> bool {
    if payload.len() + 4 > MAI2_VENDOR_HID_FRAME_LEN {
        return false;
    }

    let mut report = [0u8; MAI2_VENDOR_HID_REPORT_LEN];
    let mut idx = 1usize;
    report[idx] = 0xFF;
    idx += 1;
    report[idx] = cmd;
    idx += 1;
    report[idx] = payload.len() as u8;
    idx += 1;
    report[idx..idx + payload.len()].copy_from_slice(payload);
    idx += payload.len();
    report[idx] = report[1..idx]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_add(byte));

    hid.write(&report)
        .map(|written| written != 0)
        .unwrap_or(false)
}

fn parse_touch_hid_part(report: &[u8]) -> Option<TouchHidPart> {
    let start = if report.first().copied() == Some(MAI2_TOUCH_HID_REPORT_ID) {
        0
    } else if report.len() > 1 && report[1] == MAI2_TOUCH_HID_REPORT_ID {
        1
    } else {
        return None;
    };

    if report.len().saturating_sub(start) < MAI2_TOUCH_HID_REPORT_LEN {
        return None;
    }

    let data = &report[start..start + MAI2_TOUCH_HID_REPORT_LEN];
    let part_index = data[4];
    let part_count = data[5];

    if data[1] != 1
        || data[2] != 1
        || part_count != MAI2_TOUCH_HID_PART_COUNT
        || part_index >= part_count
        || data[7] == 0
        || data[7] > 17
        || (data[12] & 0x04) == 0
    {
        return None;
    }

    let mut touch_bits = [0u8; 5];
    touch_bits.copy_from_slice(&data[50..55]);

    Some(TouchHidPart {
        stream_seq: data[3],
        part_index,
        part_count,
        touch_bits,
    })
}

fn pack_legacy_touch_bits(touch_bits: &[u8; 5]) -> [u8; 7] {
    let mut touch = [0u8; 7];

    for (group, packed) in touch.iter_mut().enumerate() {
        for bit in 0..5 {
            let touch_index = group * 5 + bit;
            if touch_index >= 34 {
                break;
            }
            if (touch_bits[touch_index / 8] & (1u8 << (touch_index % 8))) != 0 {
                *packed |= 1u8 << bit;
            }
        }
    }

    touch
}

fn send_frame(port: &SerialPort, cmd: u8, payload: &[u8]) -> bool {
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.push(0xFF);
    frame.push(cmd);
    frame.push(payload.len() as u8);
    frame.extend_from_slice(payload);
    let checksum = frame.iter().fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    frame.push(checksum);
    port.write(&frame)
}

fn apply_device_frame(
    device: &DeviceHandle,
    _shared: &SharedState,
    buttons0: Option<u8>,
    io_status: Option<u8>,
    touch: Option<[u8; 7]>,
    touch_source: TouchSource,
) {
    let touch_changed = touch.is_some();

    device.input_page.update(|page| {
        page.connected = 1;
        if let Some(buttons0) = buttons0 {
            page.buttons0 = buttons0;
        }
        if let Some(io_status) = io_status {
            page.io_status = io_status;
        }
        if let Some(touch) = touch {
            page.touch = touch;
        }
        page.sequence = page.sequence.wrapping_add(1);
    });

    if touch_changed {
        device.touch_frames.fetch_add(1, Ordering::SeqCst);
        match touch_source {
            TouchSource::Serial => {
                device.touch_serial_frames.fetch_add(1, Ordering::SeqCst);
            }
            TouchSource::TouchHid => {
                device.touch_hid_frames.fetch_add(1, Ordering::SeqCst);
            }
            TouchSource::Synthetic => {}
        }
        device
            .touch_last_update_ms
            .store(tick_ms(), Ordering::SeqCst);
    }
}

fn map_buttons(buttons0: u8, buttons1: u8) -> u16 {
    let mut out = 0u16;

    if buttons0 & 0x01 != 0 {
        out |= MAI2_IO_GAMEBTN_1;
    }
    if buttons0 & 0x02 != 0 {
        out |= MAI2_IO_GAMEBTN_2;
    }
    if buttons0 & 0x04 != 0 {
        out |= MAI2_IO_GAMEBTN_3;
    }
    if buttons0 & 0x08 != 0 {
        out |= MAI2_IO_GAMEBTN_4;
    }
    if buttons0 & 0x10 != 0 {
        out |= MAI2_IO_GAMEBTN_5;
    }
    if buttons0 & 0x20 != 0 {
        out |= MAI2_IO_GAMEBTN_6;
    }
    if buttons0 & 0x40 != 0 {
        out |= MAI2_IO_GAMEBTN_7;
    }
    if buttons0 & 0x80 != 0 {
        out |= MAI2_IO_GAMEBTN_8;
    }
    if buttons1 & (1 << MAI2_AFFINE_EXT_SELECT_BIT) != 0 {
        out |= MAI2_IO_GAMEBTN_SELECT;
    }

    out
}

fn key_down(vk: u16) -> bool {
    if vk == 0 {
        return false;
    }

    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

fn find_hid_path(
    api: &HidApi,
    vid: u16,
    pid: u16,
    usage_page: u16,
    usage: u16,
) -> Option<std::ffi::CString> {
    api.device_list()
        .find(|info| {
            info.vendor_id() == vid
                && info.product_id() == pid
                && info.usage_page() == usage_page
                && info.usage() == usage
        })
        .map(|info| info.path().to_owned())
}

fn hid_interface_present(pid: u16, usage_page: u16, usage: u16) -> bool {
    let Ok(api) = HidApi::new() else {
        return false;
    };

    find_hid_path(&api, AFFINE_VID, pid, usage_page, usage).is_some()
}
