use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use affine_core::serial::{SerialPort, find_com_port};
use affine_core::shared_memory::SharedPage;
use affine_core::types::{Hresult, Mai2TouchCallback, S_OK};
use affine_core::util::{
    current_exe_name, ini_get_bool, log_line, segatools_config_path, sleep_ms, tick_ms,
};
use hidapi::HidApi;

const AFFINE_VID: u16 = 0xAFF1;
const MAI2_PID_1P: u16 = 0x52A5;
const MAI2_PID_2P: u16 = 0x52A6;
const AFFINE_CMD_HEARTBEAT: u8 = 0x11;
const AFFINE_CMD_GET_BOARD_INFO: u8 = 0xF0;
const MAI2_HID_USAGE_PAGE: u16 = 0xFFCA;
const MAI2_HID_USAGE: u16 = 0x0001;
const MAI2_HID_REPORT_LEN: usize = 24;
const MAI2_HID_READ_TIMEOUT_MS: i32 = 1000;

const AFFINE_HEARTBEAT_INTERVAL_MS: u64 = 100;
const AFFINE_RESCAN_INTERVAL_MS: u64 = 500;
const AFFINE_BOARD_INFO_DELAY_MS: u64 = 500;
const AFFINE_BOARD_INFO_TIMEOUT_MS: u64 = 1_000;

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
#[derive(Clone, Copy, Default)]
struct Mai2OutputPage {
    touch_enabled: u8,
    _reserved0: [u8; 7],
    buttons_sequence: u64,
    buttons: [u8; 24],
    billboard_sequence: u64,
    billboard: [u8; 24],
    pwm_sequence: u64,
    pwm: [u8; 3],
    _reserved1: [u8; 5],
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

#[derive(Clone, Copy, Default)]
struct LedColor {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Clone, Copy, Default)]
struct LedFade {
    start: LedColor,
    target: LedColor,
    current: LedColor,
    start_time: u64,
    duration: u64,
}

#[derive(Default)]
struct LedState {
    fades: [[LedFade; 8]; 2],
    force_update: [bool; 2],
}

struct SharedState {
    callback: Mutex<Mai2TouchCallback>,
    led_state: Mutex<LedState>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            callback: Mutex::new(None),
            led_state: Mutex::new(LedState::default()),
        }
    }
}

struct DeviceHandle {
    player: u8,
    pid: u16,
    enabled: AtomicBool,
    hid_connected: AtomicBool,
    input_page: SharedPage<Mai2InputPage>,
    output_page: SharedPage<Mai2OutputPage>,
}

impl DeviceHandle {
    fn new(player: u8, pid: u16, enabled: bool, index: usize) -> Arc<Self> {
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
            enabled: AtomicBool::new(enabled),
            hid_connected: AtomicBool::new(false),
            input_page,
            output_page,
        })
    }
}

pub struct Mai2Runtime {
    shared: Arc<SharedState>,
    devices: [Arc<DeviceHandle>; 2],
    poll_page: SharedPage<Mai2PollPage>,
    initialized: AtomicBool,
    led_started: AtomicBool,
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
        log_line(&format!(
            "[Affine IO] Config: p1Enable={} p2Enable={}",
            p1_enabled as u8, p2_enabled as u8
        ));

        let shared = Arc::new(SharedState::new());
        let p1 = DeviceHandle::new(1, MAI2_PID_1P, p1_enabled, 0);
        let p2 = DeviceHandle::new(2, MAI2_PID_2P, p2_enabled, 1);
        let poll_page: SharedPage<Mai2PollPage> =
            SharedPage::create(MAI2_POLL_MAPPING_NAME, MAI2_POLL_MUTEX_NAME)
                .expect("mai2 poll shared memory");

        Arc::new(Self {
            shared,
            devices: [p1, p2],
            poll_page,
            initialized: AtomicBool::new(false),
            led_started: AtomicBool::new(false),
        })
    }

    pub fn init(&self) -> Hresult {
        if self.initialized.swap(true, Ordering::SeqCst) {
            return S_OK;
        }

        log_line("[Affine IO] Initializing...");
        log_line("[Affine IO] Affine IO Version: rust");
        log_line("[Affine IO] Mai2IO API Version: 1.02");

        if let Some(exe_name) = current_exe_name() {
            log_line(&format!("[Affine IO] Running in {exe_name}"));
            if !exe_name.eq_ignore_ascii_case("Sinmai.exe") {
                log_line(&format!("[Affine IO] Skipping device init for {exe_name}"));
                log_line("[Affine IO] Initialization complete.");
                return S_OK;
            }
        }

        for device in &self.devices {
            let device = device.clone();
            let shared = self.shared.clone();
            let serial_device = device.clone();
            let serial_shared = shared.clone();
            thread::spawn(move || device_thread(serial_device, serial_shared));
            thread::spawn(move || hid_thread(device, shared));
        }

        log_line("[Affine IO] Initialization complete.");
        S_OK
    }

    pub fn poll(&self) -> Hresult {
        let p1 = self.devices[0].input_page.read();
        let p2 = self.devices[1].input_page.read();

        self.poll_page.update(|poll_state| {
            let mut opbtn = 0;
            let mut player1_btn = 0;
            let mut player2_btn = 0;

            if p1.connected != 0 {
                player1_btn = map_buttons(p1.buttons0, p1.io_status);

                if p1.io_status & (1 << MAI2_AFFINE_EXT_TEST_BIT) != 0 {
                    opbtn |= MAI2_IO_OPBTN_TEST;
                }
                if p1.io_status & (1 << MAI2_AFFINE_EXT_SERVICE_BIT) != 0 {
                    opbtn |= MAI2_IO_OPBTN_SERVICE;
                }

                let coin_pressed = p1.io_status & (1 << MAI2_AFFINE_EXT_COIN_BIT) != 0;
                if coin_pressed {
                    if poll_state.affine_coin == 0 {
                        poll_state.affine_coin = 1;
                        opbtn |= MAI2_IO_OPBTN_COIN;
                    }
                } else {
                    poll_state.affine_coin = 0;
                }
            } else {
                poll_state.affine_coin = 0;
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
        let hr = self.init();

        if !self.led_started.swap(true, Ordering::SeqCst) {
            let devices = [self.devices[0].clone(), self.devices[1].clone()];
            let shared = self.shared.clone();
            thread::spawn(move || led_thread(devices, shared));
        }

        hr
    }

    pub fn led_set_fet_output(&self, board: u8, rgb: [u8; 3]) {
        if let Some(device) = self.devices.get(board as usize) {
            device.output_page.update(|page| {
                page.pwm = rgb;
                page.pwm_sequence = page.pwm_sequence.wrapping_add(1);
            });
        }
    }

    pub fn led_gs_update(&self, board: u8, rgb: &[u8]) {
        if board > 1 || rgb.len() < 32 {
            return;
        }

        let board = board as usize;
        let now = tick_ms();
        let mut immediate = None;
        let mut led_state = self.shared.led_state.lock().unwrap();

        for i in 0..8 {
            let fade = &mut led_state.fades[board][i];
            let next = LedColor {
                r: rgb[i * 4],
                g: rgb[i * 4 + 1],
                b: rgb[i * 4 + 2],
            };
            let speed = rgb[i * 4 + 3];

            fade.start = fade.current;
            fade.target = next;
            fade.start_time = now;

            if speed == 0 {
                fade.duration = 0;
                fade.current = next;
                let payload = immediate.get_or_insert([0u8; 24]);
                payload[i * 3] = next.r;
                payload[i * 3 + 1] = next.g;
                payload[i * 3 + 2] = next.b;
                led_state.force_update[board] = true;
            } else {
                fade.duration = (4095u64 / speed as u64) * 8;
            }
        }

        if let Some(payload) = immediate {
            self.devices[board].output_page.update(|page| {
                page.buttons = payload;
                page.buttons_sequence = page.buttons_sequence.wrapping_add(1);
            });
        }
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
            page.billboard = payload;
            page.billboard_sequence = page.billboard_sequence.wrapping_add(1);
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
        );
    }
}

fn device_thread(device: Arc<DeviceHandle>, shared: Arc<SharedState>) {
    let mut port = SerialPort::default();
    let mut rx_buf = [0u8; 128];
    let mut rx_len = 0usize;
    let mut last_heartbeat = tick_ms();
    let mut board_info_pending = false;
    let mut board_info_logged = false;
    let mut board_info_start_ms = 0u64;
    let mut board_info_request_ms = 0u64;
    let mut last_buttons_sequence = 0u64;
    let mut last_billboard_sequence = 0u64;
    let mut last_pwm_sequence = 0u64;
    let mut device_missing_logged = false;
    let mut port_open_failed_logged = false;

    loop {
        if !device.enabled.load(Ordering::SeqCst) {
            if port.is_open() {
                port.close();
                device.input_page.write(Mai2InputPage::default());
            }
            device_missing_logged = false;
            port_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        if !port.is_open() {
            let Some(port_path) = find_com_port(AFFINE_VID, device.pid) else {
                if !device_missing_logged {
                    log_line(&format!(
                        "[Affine IO] P{}: Device not found (VID_{AFFINE_VID:04X} PID_{:04X})",
                        device.player, device.pid
                    ));
                    device_missing_logged = true;
                }
                port_open_failed_logged = false;
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            };

            if !port.open(&port_path, 115_200) {
                if !port_open_failed_logged {
                    log_line(&format!(
                        "[Affine IO] P{}: Failed to open port {port_path}",
                        device.player
                    ));
                    port_open_failed_logged = true;
                }
                device_missing_logged = false;
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }

            log_line(&format!(
                "[Affine IO] Connected P{}: {}",
                device.player,
                port_path.trim_start_matches("\\\\.\\")
            ));
            device_missing_logged = false;
            port_open_failed_logged = false;
            device.input_page.update(|page| {
                page.connected = 1;
                page.sequence = page.sequence.wrapping_add(1);
            });
            rx_len = 0;
            last_heartbeat = tick_ms();
            board_info_pending = true;
            board_info_logged = false;
            board_info_start_ms = tick_ms();
            board_info_request_ms = 0;
        }

        let now = tick_ms();

        if board_info_pending {
            if board_info_request_ms == 0
                && now.saturating_sub(board_info_start_ms) >= AFFINE_BOARD_INFO_DELAY_MS
            {
                if !send_frame(&mut port, AFFINE_CMD_GET_BOARD_INFO, &[]) {
                    port.close();
                    device.input_page.write(Mai2InputPage::default());
                    sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                    continue;
                }
                board_info_request_ms = now;
            } else if board_info_request_ms != 0
                && now.saturating_sub(board_info_request_ms) >= AFFINE_BOARD_INFO_TIMEOUT_MS
            {
                if !send_frame(&mut port, AFFINE_CMD_GET_BOARD_INFO, &[]) {
                    port.close();
                    device.input_page.write(Mai2InputPage::default());
                    sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                    continue;
                }
                board_info_request_ms = now;
                if now.saturating_sub(board_info_start_ms) > 3_000 && !board_info_logged {
                    board_info_pending = false;
                    board_info_logged = true;
                    log_line(&format!("[Affine IO] P{} Firmware: unknown", device.player));
                }
            }
        } else if now.saturating_sub(last_heartbeat) >= AFFINE_HEARTBEAT_INTERVAL_MS {
            if !send_frame(&mut port, AFFINE_CMD_HEARTBEAT, &[]) {
                port.close();
                device.input_page.write(Mai2InputPage::default());
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            last_heartbeat = now;
        }

        let output = device.output_page.read();

        if output.buttons_sequence != last_buttons_sequence {
            if !send_frame(&mut port, 0x14, &output.buttons) {
                port.close();
                device.input_page.write(Mai2InputPage::default());
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            last_buttons_sequence = output.buttons_sequence;
        }

        if output.billboard_sequence != last_billboard_sequence {
            if !send_frame(&mut port, 0x15, &output.billboard) {
                port.close();
                device.input_page.write(Mai2InputPage::default());
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            last_billboard_sequence = output.billboard_sequence;
        }

        if output.pwm_sequence != last_pwm_sequence {
            if !send_frame(&mut port, 0x16, &output.pwm) {
                port.close();
                device.input_page.write(Mai2InputPage::default());
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            last_pwm_sequence = output.pwm_sequence;
        }

        let mut read_buf = [0u8; 64];
        let Some(read) = port.read(&mut read_buf) else {
            port.close();
            device.input_page.write(Mai2InputPage::default());
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
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
                        );
                    }
                    ParsedFrame::BoardInfo(version) => {
                        board_info_pending = false;
                        board_info_logged = true;
                        log_line(&format!(
                            "[Affine IO] P{} Firmware: {version}",
                            device.player
                        ));
                    }
                }
            }
        }
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
                log_line(&format!(
                    "[Affine IO] P{}: HID subsystem unavailable",
                    device.player
                ));
                hid_unavailable_logged = true;
            }
            hid_missing_logged = false;
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_unavailable_logged = false;

        let Some(path) = find_hid_path(&api, AFFINE_VID, device.pid) else {
            if !hid_missing_logged {
                log_line(&format!(
                    "[Affine IO] P{}: HID button interface not found (VID_{AFFINE_VID:04X} PID_{:04X})",
                    device.player, device.pid
                ));
                hid_missing_logged = true;
            }
            hid_open_failed_logged = false;
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_missing_logged = false;

        let path_display = path.to_string_lossy().into_owned();
        let Ok(hid) = api.open_path(&path) else {
            if !hid_open_failed_logged {
                log_line(&format!(
                    "[Affine IO] P{}: Failed to open HID path {}",
                    device.player, path_display
                ));
                hid_open_failed_logged = true;
            }
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        };
        hid_open_failed_logged = false;

        log_line(&format!(
            "[Affine IO] Connected P{} HID buttons: {}",
            device.player, path_display
        ));
        device.hid_connected.store(true, Ordering::SeqCst);

        loop {
            let mut report = [0u8; MAI2_HID_REPORT_LEN];
            match hid.read_timeout(&mut report, MAI2_HID_READ_TIMEOUT_MS) {
                Ok(0) => continue,
                Ok(read) if read >= 2 => {
                    if read >= MAI2_HID_REPORT_LEN && report[1] == 0xFF {
                        continue;
                    }
                    apply_device_frame(&device, &shared, Some(report[0]), Some(report[1]), None);
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }

        device.hid_connected.store(false, Ordering::SeqCst);
        sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
    }
}

fn led_thread(devices: [Arc<DeviceHandle>; 2], shared: Arc<SharedState>) {
    loop {
        sleep_ms(8);
        let now = tick_ms();
        let mut led_state = shared.led_state.lock().unwrap();

        for (board, device) in devices.iter().enumerate() {
            let mut payload = [0u8; 24];
            let mut need_update = false;

            for index in 0..8 {
                let fade = &mut led_state.fades[board][index];
                let next =
                    if fade.duration == 0 || now >= fade.start_time.saturating_add(fade.duration) {
                        fade.target
                    } else {
                        let progress = (now - fade.start_time) as f32 / fade.duration as f32;
                        LedColor {
                            r: interpolate(fade.start.r, fade.target.r, progress),
                            g: interpolate(fade.start.g, fade.target.g, progress),
                            b: interpolate(fade.start.b, fade.target.b, progress),
                        }
                    };

                if next.r != fade.current.r || next.g != fade.current.g || next.b != fade.current.b
                {
                    need_update = true;
                }

                fade.current = next;
                payload[index * 3] = next.r;
                payload[index * 3 + 1] = next.g;
                payload[index * 3 + 2] = next.b;
            }

            if need_update || led_state.force_update[board] {
                led_state.force_update[board] = false;
                device.output_page.update(|page| {
                    page.buttons = payload;
                    page.buttons_sequence = page.buttons_sequence.wrapping_add(1);
                });
            }
        }
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

fn try_parse_frame(rx_buf: &mut [u8; 128], rx_len: &mut usize) -> Option<ParsedFrame> {
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
                if rx_buf[13] == 0x0A {
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

fn consume(rx_buf: &mut [u8; 128], rx_len: &mut usize, count: usize) {
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

fn send_frame(port: &mut SerialPort, cmd: u8, payload: &[u8]) -> bool {
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.push(0xFF);
    frame.push(cmd);
    frame.push(payload.len() as u8);
    frame.extend_from_slice(payload);
    let checksum = frame.iter().fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    frame.push(checksum);
    port.write(&frame)
}

fn interpolate(start: u8, end: u8, progress: f32) -> u8 {
    let delta = end as f32 - start as f32;
    (start as f32 + delta * progress).clamp(0.0, 255.0) as u8
}

fn apply_device_frame(
    device: &DeviceHandle,
    shared: &SharedState,
    buttons0: Option<u8>,
    io_status: Option<u8>,
    touch: Option<[u8; 7]>,
) {
    let touch_changed = touch.is_some();
    let (touch_copy, touch_enabled) = device.input_page.update(|page| {
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

        (page.touch, device.output_page.read().touch_enabled != 0)
    });

    if touch_changed
        && touch_enabled
        && let Some(callback) = *shared.callback.lock().unwrap()
    {
        unsafe {
            callback(device.player, touch_copy.as_ptr());
        }
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

fn find_hid_path(api: &HidApi, vid: u16, pid: u16) -> Option<std::ffi::CString> {
    api.device_list()
        .find(|info| {
            info.vendor_id() == vid
                && info.product_id() == pid
                && info.usage_page() == MAI2_HID_USAGE_PAGE
                && info.usage() == MAI2_HID_USAGE
        })
        .map(|info| info.path().to_owned())
}
