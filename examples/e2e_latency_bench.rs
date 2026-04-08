#[cfg(all(windows, feature = "latency-bench"))]
mod bench {
    use std::env;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use affine_chuni::runtime as chuni_runtime;
    use affine_core::serial::{SerialPort, find_com_port};
    use affine_core::slider::{SliderParser, find_any, send_slider_frame};
    use affine_mai2::runtime as mai2_runtime;
    use affine_mercury::runtime as mercury_runtime;

    const AFFINE_VID: u16 = 0xAFF1;
    const MAI2_PID_1P: u16 = 0x52A5;
    const MAI2_PID_2P: u16 = 0x52A6;
    const CHUNI_PIDS: [u16; 2] = [0x52A4, 0x52A7];
    const SERIAL_BAUD: u32 = 115_200;
    const BENCHMARK_CMD: u8 = 0x22;
    const DEFAULT_ITERATIONS: usize = 500;
    const MAX_READ_SPINS: usize = 4;
    const ROUNDTRIP_TIMEOUT: Duration = Duration::from_secs(1);

    static MAI2_CALLBACKS: AtomicU64 = AtomicU64::new(0);
    static CHUNI_CALLBACKS: AtomicU64 = AtomicU64::new(0);
    static MERCURY_CALLBACKS: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Copy)]
    struct BenchConfig {
        iterations: usize,
        synthetic: bool,
    }

    struct BenchmarkReply {
        dispatch_cycles: u32,
        tx_cycles: u32,
        core_hz: u32,
    }

    pub fn run() {
        let config = parse_args();
        let frequency = perf_frequency();

        println!(
            "affine hardware e2e benchmark ({} iterations)",
            config.iterations
        );

        let mut ran_hardware = false;
        ran_hardware |= bench_device_by_pid("mai2-p1", MAI2_PID_1P, config.iterations, frequency);
        ran_hardware |= bench_device_by_pid("mai2-p2", MAI2_PID_2P, config.iterations, frequency);
        ran_hardware |=
            bench_device_by_any_pid("chuni-slider", &CHUNI_PIDS, config.iterations, frequency);

        if !ran_hardware {
            println!("no benchmark-capable firmware device detected");
        }

        if config.synthetic {
            println!("synthetic benchmark mode enabled");
            bench_direct_call(config.iterations, frequency);
            bench_mai2_poll_path(config.iterations, frequency);
            bench_mai2_callback_path(config.iterations, frequency);
            bench_chuni_callback_path(config.iterations, frequency);
            bench_mercury_callback_path(config.iterations, frequency);
        }
    }

    unsafe extern "C" fn mai2_callback(_player: u8, _state: *const u8) {
        MAI2_CALLBACKS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn chuni_callback(_state: *const u8) {
        CHUNI_CALLBACKS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn mercury_callback(_state: *const bool) {
        MERCURY_CALLBACKS.fetch_add(1, Ordering::SeqCst);
    }

    fn parse_args() -> BenchConfig {
        let mut config = BenchConfig {
            iterations: DEFAULT_ITERATIONS,
            synthetic: false,
        };

        for arg in env::args().skip(1) {
            if arg == "--synthetic" {
                config.synthetic = true;
                continue;
            }

            if let Some(raw) = arg.strip_prefix("--iterations=") {
                if let Ok(iterations) = raw.parse::<usize>() {
                    if iterations > 0 {
                        config.iterations = iterations;
                    }
                }
            }
        }

        config
    }

    fn bench_device_by_pid(label: &str, pid: u16, iterations: usize, frequency: f64) -> bool {
        let Some(path) = find_com_port(AFFINE_VID, pid) else {
            return false;
        };

        bench_serial_device(label, &path, iterations, frequency);
        true
    }

    fn bench_device_by_any_pid(
        label: &str,
        pids: &[u16],
        iterations: usize,
        frequency: f64,
    ) -> bool {
        let Some((_, path)) = find_any(AFFINE_VID, pids) else {
            return false;
        };

        bench_serial_device(label, &path, iterations, frequency);
        true
    }

    fn bench_serial_device(label: &str, path: &str, iterations: usize, frequency: f64) {
        let mut port = SerialPort::default();
        if !port.open(path, SERIAL_BAUD) {
            println!("{label}: failed to open {path}");
            return;
        }

        drain_port(&mut port);

        let mut host_samples_ticks = Vec::with_capacity(iterations);
        let mut device_samples_us = Vec::with_capacity(iterations);
        let mut transit_samples_us = Vec::with_capacity(iterations);

        for sequence in 0..iterations {
            let payload = make_benchmark_payload(sequence as u64);
            let start = perf_counter();

            if !send_slider_frame(&mut |frame| port.write(frame), BENCHMARK_CMD, &payload) {
                println!("{label}: write failed on iteration {sequence}");
                return;
            }

            let reply = match read_benchmark_reply(&mut port, &payload) {
                Some(reply) => reply,
                None => {
                    println!("{label}: timeout waiting for benchmark reply");
                    return;
                }
            };

            let host_ticks = perf_counter() - start;
            let host_us = ticks_to_us(host_ticks, frequency);
            let device_us = cycles_to_us(
                reply.tx_cycles.wrapping_sub(reply.dispatch_cycles),
                reply.core_hz,
            );

            host_samples_ticks.push(host_ticks);
            device_samples_us.push(device_us);
            transit_samples_us.push((host_us - device_us).max(0.0));
        }

        println!("{label}: port={path}");
        sample_stats_ticks(&format!("{label}-rtt"), host_samples_ticks, frequency);
        sample_stats_us(&format!("{label}-device"), device_samples_us);
        sample_stats_us(&format!("{label}-host-minus-device"), transit_samples_us);
    }

    fn read_benchmark_reply(
        port: &mut SerialPort,
        expected_payload: &[u8],
    ) -> Option<BenchmarkReply> {
        let deadline = Instant::now() + ROUNDTRIP_TIMEOUT;
        let mut parser = SliderParser::default();
        let mut buf = [0u8; 64];
        let expected_len = expected_payload.len() + 12;
        let mut idle_spins = 0usize;

        while Instant::now() < deadline {
            let read = match port.read(&mut buf) {
                Some(read) => read,
                None => return None,
            };

            if read == 0 {
                idle_spins += 1;
                if idle_spins >= MAX_READ_SPINS {
                    std::thread::sleep(Duration::from_millis(1));
                }
                continue;
            }

            idle_spins = 0;
            for &byte in &buf[..read] {
                let Some(packet) = parser.push(byte) else {
                    continue;
                };

                if packet.cmd != BENCHMARK_CMD || packet.payload.len() != expected_len {
                    continue;
                }

                if &packet.payload[..expected_payload.len()] != expected_payload {
                    continue;
                }

                let meta = &packet.payload[expected_payload.len()..];
                return Some(BenchmarkReply {
                    dispatch_cycles: read_u32_le(&meta[0..4]),
                    tx_cycles: read_u32_le(&meta[4..8]),
                    core_hz: read_u32_le(&meta[8..12]),
                });
            }
        }

        None
    }

    fn drain_port(port: &mut SerialPort) {
        let mut buf = [0u8; 64];
        for _ in 0..MAX_READ_SPINS {
            match port.read(&mut buf) {
                Some(0) | None => break,
                Some(_) => {}
            }
        }
    }

    fn make_benchmark_payload(sequence: u64) -> [u8; 16] {
        let mut payload = [0u8; 16];
        payload[..8].copy_from_slice(&sequence.to_le_bytes());
        payload[8..].copy_from_slice(&sequence.rotate_left(13).to_le_bytes());
        payload
    }

    fn read_u32_le(raw: &[u8]) -> u32 {
        u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])
    }

    fn cycles_to_us(cycles: u32, core_hz: u32) -> f64 {
        if core_hz == 0 {
            0.0
        } else {
            cycles as f64 * 1_000_000.0 / core_hz as f64
        }
    }

    fn ticks_to_us(ticks: i64, frequency: f64) -> f64 {
        ticks as f64 * 1_000_000.0 / frequency
    }

    fn bench_direct_call(iterations: usize, frequency: f64) {
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = perf_counter();
            std::hint::black_box(());
            samples.push(perf_counter() - start);
        }
        sample_stats_ticks("direct-call", samples, frequency);
    }

    fn bench_mai2_poll_path(iterations: usize, frequency: f64) {
        let runtime = mai2_runtime();
        runtime.set_touch_enabled(false, false);

        let mut samples = Vec::with_capacity(iterations);
        for sequence in 0..iterations {
            let buttons0 = (sequence as u8).rotate_left(1);
            let buttons1 = ((sequence * 3) as u8) & 0x0f;
            let touch = make_mai2_touch(sequence as u64);

            let start = perf_counter();
            runtime.bench_inject_input(1, buttons0, buttons1, touch);
            let _ = runtime.poll();
            let _ = runtime.get_gamebtns();
            samples.push(perf_counter() - start);
        }

        sample_stats_ticks("mai2-poll-end-to-end", samples, frequency);
    }

    fn bench_mai2_callback_path(iterations: usize, frequency: f64) {
        let runtime = mai2_runtime();
        runtime.set_touch_callback(Some(mai2_callback));
        runtime.set_touch_enabled(true, true);

        let mut samples = Vec::with_capacity(iterations);
        for sequence in 0..iterations {
            let before = MAI2_CALLBACKS.load(Ordering::SeqCst);
            let touch = make_mai2_touch(sequence as u64);

            let start = perf_counter();
            runtime.bench_inject_input(1, 0x5a, 0x03, touch);
            samples.push(perf_counter() - start);

            let after = MAI2_CALLBACKS.load(Ordering::SeqCst);
            assert_eq!(after, before + 1, "mai2 callback not observed");
        }

        sample_stats_ticks("mai2-callback-end-to-end", samples, frequency);
    }

    fn bench_chuni_callback_path(iterations: usize, frequency: f64) {
        let runtime = chuni_runtime();
        runtime.start(Some(chuni_callback));

        let mut samples = Vec::with_capacity(iterations);
        for sequence in 0..iterations {
            let before = CHUNI_CALLBACKS.load(Ordering::SeqCst);
            let pressure = make_chuni_pressure(sequence as u64);

            let start = perf_counter();
            runtime.bench_inject_input(pressure, (sequence & 0x3f) as u8);
            samples.push(perf_counter() - start);

            let after = CHUNI_CALLBACKS.load(Ordering::SeqCst);
            assert_eq!(after, before + 1, "chuni callback not observed");
        }

        sample_stats_ticks("chuni-callback-end-to-end", samples, frequency);
        runtime.stop();
    }

    fn bench_mercury_callback_path(iterations: usize, frequency: f64) {
        let runtime = mercury_runtime();
        runtime.start(Some(mercury_callback));

        let mut samples = Vec::with_capacity(iterations);
        for sequence in 0..iterations {
            let before = MERCURY_CALLBACKS.load(Ordering::SeqCst);
            let cells = make_mercury_cells(sequence as u64);

            let start = perf_counter();
            runtime.bench_inject_input(cells);
            samples.push(perf_counter() - start);

            let after = MERCURY_CALLBACKS.load(Ordering::SeqCst);
            assert_eq!(after, before + 1, "mercury callback not observed");
        }

        sample_stats_ticks("mercury-callback-end-to-end", samples, frequency);
    }

    fn make_mai2_touch(sequence: u64) -> [u8; 7] {
        let mut touch = [0u8; 7];
        for (index, slot) in touch.iter_mut().enumerate() {
            *slot = sequence.wrapping_mul(17).wrapping_add(index as u64 * 11) as u8 & 0x1f;
        }
        touch
    }

    fn make_chuni_pressure(sequence: u64) -> [u8; 32] {
        let mut pressure = [0u8; 32];
        for (index, slot) in pressure.iter_mut().enumerate() {
            *slot = sequence.wrapping_mul(5).wrapping_add(index as u64 * 3) as u8;
        }
        pressure
    }

    fn make_mercury_cells(sequence: u64) -> [bool; 240] {
        let mut cells = [false; 240];
        for (index, slot) in cells.iter_mut().enumerate() {
            *slot = (sequence + index as u64).is_multiple_of(3);
        }
        cells
    }

    fn sample_stats_ticks(label: &str, samples_ticks: Vec<i64>, frequency: f64) {
        let samples_us: Vec<f64> = samples_ticks
            .into_iter()
            .map(|ticks| ticks_to_us(ticks, frequency))
            .collect();
        sample_stats_us(label, samples_us);
    }

    fn sample_stats_us(label: &str, mut samples_us: Vec<f64>) {
        samples_us.sort_by(|left, right| left.partial_cmp(right).unwrap());

        let len = samples_us.len();
        if len == 0 {
            println!("{label}: no samples");
            return;
        }

        let p = |percentile: f64| -> f64 {
            let index = ((len.saturating_sub(1)) as f64 * percentile).round() as usize;
            samples_us[index]
        };
        let average_us = samples_us.iter().copied().sum::<f64>() / len as f64;
        let max_us = *samples_us.last().unwrap_or(&0.0);

        println!(
            "{label}: avg={average_us:.3}us p50={:.3}us p95={:.3}us p99={:.3}us max={max_us:.3}us",
            p(0.50),
            p(0.95),
            p(0.99),
        );
    }

    fn perf_counter() -> i64 {
        use windows_sys::Win32::System::Performance::QueryPerformanceCounter;

        let mut value = 0i64;
        unsafe {
            QueryPerformanceCounter(&mut value);
        }
        value
    }

    fn perf_frequency() -> f64 {
        use windows_sys::Win32::System::Performance::QueryPerformanceFrequency;

        let mut value = 0i64;
        unsafe {
            QueryPerformanceFrequency(&mut value);
        }
        value as f64
    }
}

#[cfg(all(windows, feature = "latency-bench"))]
fn main() {
    bench::run();
}

#[cfg(not(all(windows, feature = "latency-bench")))]
fn main() {
    eprintln!(
        "Run with `cargo run --example e2e_latency_bench --features latency-bench` on Windows."
    );
}
