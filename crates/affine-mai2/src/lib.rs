use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use affine_core::serial::{SerialPort, find_com_port};
use affine_core::types::{Hresult, Mai2TouchCallback, S_OK};
use affine_core::util::{
    current_exe_name, ini_get_bool, log_line, segatools_config_path, sleep_ms, tick_ms,
};

const AFFINE_VID: u16 = 0xAFF1;
const MAI2_PID_1P: u16 = 0x52A5;
const MAI2_PID_2P: u16 = 0x52A6;
const AFFINE_CMD_HEARTBEAT: u8 = 0x11;
const AFFINE_CMD_GET_BOARD_INFO: u8 = 0xF0;

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

#[derive(Clone, Copy, Default)]
struct DeviceSnapshot {
    connected: bool,
    buttons0: u8,
    buttons1: u8,
    touch: [u8; 7],
}

#[derive(Default)]
struct PollState {
    opbtn: u8,
    player1_btn: u16,
    player2_btn: u16,
    affine_coin: bool,
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
    poll_state: Mutex<PollState>,
    led_state: Mutex<LedState>,
}

impl SharedState {
    fn new() -> Self {
        Self {
            callback: Mutex::new(None),
            poll_state: Mutex::new(PollState::default()),
            led_state: Mutex::new(LedState::default()),
        }
    }
}

enum DeviceCommand {
    Buttons([u8; 24]),
    Billboard([u8; 24]),
    Pwm([u8; 3]),
}

struct DeviceHandle {
    player: u8,
    pid: u16,
    enabled: AtomicBool,
    touch_enabled: AtomicBool,
    snapshot: Mutex<DeviceSnapshot>,
    tx: Sender<DeviceCommand>,
}

impl DeviceHandle {
    fn new(player: u8, pid: u16, enabled: bool) -> (Arc<Self>, Receiver<DeviceCommand>) {
        let (tx, rx) = mpsc::channel();
        let handle = Arc::new(Self {
            player,
            pid,
            enabled: AtomicBool::new(enabled),
            touch_enabled: AtomicBool::new(false),
            snapshot: Mutex::new(DeviceSnapshot::default()),
            tx,
        });
        (handle, rx)
    }

    fn snapshot(&self) -> DeviceSnapshot {
        *self.snapshot.lock().unwrap()
    }

    fn set_snapshot(&self, snapshot: DeviceSnapshot) {
        *self.snapshot.lock().unwrap() = snapshot;
    }

    fn send(&self, command: DeviceCommand) {
        let _ = self.tx.send(command);
    }
}

pub struct Mai2Runtime {
    shared: Arc<SharedState>,
    devices: [Arc<DeviceHandle>; 2],
    device_rxs: Mutex<Option<[Receiver<DeviceCommand>; 2]>>,
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
        let (p1, rx1) = DeviceHandle::new(1, MAI2_PID_1P, p1_enabled);
        let (p2, rx2) = DeviceHandle::new(2, MAI2_PID_2P, p2_enabled);

        Arc::new(Self {
            shared,
            devices: [p1, p2],
            device_rxs: Mutex::new(Some([rx1, rx2])),
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

        if let Some(receivers) = self.device_rxs.lock().unwrap().take() {
            for (index, receiver) in receivers.into_iter().enumerate() {
                let device = self.devices[index].clone();
                let shared = self.shared.clone();
                thread::spawn(move || device_thread(device, receiver, shared));
            }
        }

        log_line("[Affine IO] Initialization complete.");
        S_OK
    }

    pub fn poll(&self) -> Hresult {
        let p1 = self.devices[0].snapshot();
        let p2 = self.devices[1].snapshot();
        let mut poll_state = self.shared.poll_state.lock().unwrap();

        poll_state.opbtn = 0;
        poll_state.player1_btn = 0;
        poll_state.player2_btn = 0;

        if p1.connected {
            poll_state.player1_btn = map_buttons(p1.buttons0, p1.buttons1);

            if p1.buttons1 & (1 << MAI2_AFFINE_EXT_TEST_BIT) != 0 {
                poll_state.opbtn |= MAI2_IO_OPBTN_TEST;
            }
            if p1.buttons1 & (1 << MAI2_AFFINE_EXT_SERVICE_BIT) != 0 {
                poll_state.opbtn |= MAI2_IO_OPBTN_SERVICE;
            }

            let coin_pressed = p1.buttons1 & (1 << MAI2_AFFINE_EXT_COIN_BIT) != 0;
            if coin_pressed {
                if !poll_state.affine_coin {
                    poll_state.affine_coin = true;
                    poll_state.opbtn |= MAI2_IO_OPBTN_COIN;
                }
            } else {
                poll_state.affine_coin = false;
            }
        } else {
            poll_state.affine_coin = false;
        }

        if p2.connected {
            poll_state.player2_btn = map_buttons(p2.buttons0, p2.buttons1);

            if p2.buttons1 & (1 << MAI2_AFFINE_EXT_TEST_BIT) != 0 {
                poll_state.opbtn |= MAI2_IO_OPBTN_TEST;
            }
            if p2.buttons1 & (1 << MAI2_AFFINE_EXT_SERVICE_BIT) != 0 {
                poll_state.opbtn |= MAI2_IO_OPBTN_SERVICE;
            }
        }

        S_OK
    }

    pub fn get_opbtns(&self) -> u8 {
        self.shared.poll_state.lock().unwrap().opbtn
    }

    pub fn get_gamebtns(&self) -> (u16, u16) {
        let state = self.shared.poll_state.lock().unwrap();
        (state.player1_btn, state.player2_btn)
    }

    pub fn set_touch_callback(&self, callback: Mai2TouchCallback) {
        *self.shared.callback.lock().unwrap() = callback;
    }

    pub fn set_touch_enabled(&self, player1: bool, player2: bool) {
        self.devices[0]
            .touch_enabled
            .store(player1, Ordering::SeqCst);
        self.devices[1]
            .touch_enabled
            .store(player2, Ordering::SeqCst);
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
            device.send(DeviceCommand::Pwm(rgb));
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
            self.devices[board].send(DeviceCommand::Buttons(payload));
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

        self.devices[board as usize].send(DeviceCommand::Billboard(payload));
    }
}

fn device_thread(
    device: Arc<DeviceHandle>,
    receiver: Receiver<DeviceCommand>,
    shared: Arc<SharedState>,
) {
    let mut port = SerialPort::default();
    let mut rx_buf = [0u8; 128];
    let mut rx_len = 0usize;
    let mut last_scan_log = 0u64;
    let mut last_heartbeat = tick_ms();
    let mut board_info_pending = false;
    let mut board_info_logged = false;
    let mut board_info_start_ms = 0u64;
    let mut board_info_request_ms = 0u64;

    loop {
        if !device.enabled.load(Ordering::SeqCst) {
            if port.is_open() {
                port.close();
                device.set_snapshot(DeviceSnapshot::default());
            }
            sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
            continue;
        }

        if !port.is_open() {
            let Some(port_path) = find_com_port(AFFINE_VID, device.pid) else {
                let now = tick_ms();
                if now.saturating_sub(last_scan_log) >= 5_000 {
                    log_line(&format!(
                        "[Affine IO] P{}: Device not found (VID_{AFFINE_VID:04X} PID_{:04X})",
                        device.player, device.pid
                    ));
                    last_scan_log = now;
                }
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            };

            if !port.open(&port_path, 115_200) {
                let now = tick_ms();
                if now.saturating_sub(last_scan_log) >= 5_000 {
                    log_line(&format!(
                        "[Affine IO] P{}: Failed to open port {port_path}",
                        device.player
                    ));
                    last_scan_log = now;
                }
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }

            log_line(&format!(
                "[Affine IO] Connected P{}: {}",
                device.player,
                port_path.trim_start_matches("\\\\.\\")
            ));
            device.set_snapshot(DeviceSnapshot {
                connected: true,
                ..Default::default()
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
                    device.set_snapshot(DeviceSnapshot::default());
                    sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                    continue;
                }
                board_info_request_ms = now;
            } else if board_info_request_ms != 0
                && now.saturating_sub(board_info_request_ms) >= AFFINE_BOARD_INFO_TIMEOUT_MS
            {
                if !send_frame(&mut port, AFFINE_CMD_GET_BOARD_INFO, &[]) {
                    port.close();
                    device.set_snapshot(DeviceSnapshot::default());
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
                device.set_snapshot(DeviceSnapshot::default());
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
            last_heartbeat = now;
        }

        while let Ok(command) = receiver.try_recv() {
            let (cmd, payload): (u8, Vec<u8>) = match command {
                DeviceCommand::Buttons(bytes) => (0x14, bytes.to_vec()),
                DeviceCommand::Billboard(bytes) => (0x15, bytes.to_vec()),
                DeviceCommand::Pwm(bytes) => (0x16, bytes.to_vec()),
            };

            if !send_frame(&mut port, cmd, &payload) {
                port.close();
                device.set_snapshot(DeviceSnapshot::default());
                sleep_ms(AFFINE_RESCAN_INTERVAL_MS);
                continue;
            }
        }

        let mut read_buf = [0u8; 64];
        let Some(read) = port.read(&mut read_buf) else {
            port.close();
            device.set_snapshot(DeviceSnapshot::default());
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
                    ParsedFrame::Touch { buttons, touch } => {
                        let mut snapshot = device.snapshot();
                        snapshot.connected = true;
                        if let Some(buttons) = buttons {
                            snapshot.buttons0 = (buttons[0] & 0x0F) | (buttons[1] & 0xF0);
                            snapshot.buttons1 = buttons[2] & 0x3F;
                        }
                        if let Some(touch) = touch {
                            snapshot.touch = touch;
                        }
                        device.set_snapshot(snapshot);

                        if device.touch_enabled.load(Ordering::SeqCst)
                            && let Some(callback) = *shared.callback.lock().unwrap()
                        {
                            unsafe {
                                callback(device.player, snapshot.touch.as_ptr());
                            }
                        }
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
                device.send(DeviceCommand::Buttons(payload));
            }
        }
    }
}

enum ParsedFrame {
    Touch {
        buttons: Option<[u8; 3]>,
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
                    let mut buttons = [0u8; 3];
                    let mut touch = [0u8; 7];
                    buttons.copy_from_slice(&rx_buf[3..6]);
                    touch.copy_from_slice(&rx_buf[6..13]);
                    consume(rx_buf, rx_len, 14);
                    return Some(ParsedFrame::Touch {
                        buttons: Some(buttons),
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
                    buttons: None,
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
