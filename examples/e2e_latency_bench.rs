#[cfg(all(windows, feature = "latency-bench"))]
mod bench {
    use std::sync::atomic::{AtomicU64, Ordering};

    use affine_chuni::runtime as chuni_runtime;
    use affine_mai2::runtime as mai2_runtime;
    use affine_mercury::runtime as mercury_runtime;

    static MAI2_CALLBACKS: AtomicU64 = AtomicU64::new(0);
    static CHUNI_CALLBACKS: AtomicU64 = AtomicU64::new(0);
    static MERCURY_CALLBACKS: AtomicU64 = AtomicU64::new(0);

    pub fn run() {
        let iterations = 10_000usize;
        let frequency = perf_frequency();

        println!("affine e2e latency benchmark ({iterations} iterations)");
        bench_direct_call(iterations, frequency);
        bench_mai2_poll_path(iterations, frequency);
        bench_mai2_callback_path(iterations, frequency);
        bench_chuni_callback_path(iterations, frequency);
        bench_mercury_callback_path(iterations, frequency);
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

    fn bench_direct_call(iterations: usize, frequency: f64) {
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = perf_counter();
            std::hint::black_box(());
            samples.push(perf_counter() - start);
        }
        sample_stats("direct-call", samples, frequency);
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

        sample_stats("mai2-poll-end-to-end", samples, frequency);
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

        sample_stats("mai2-callback-end-to-end", samples, frequency);
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

        sample_stats("chuni-callback-end-to-end", samples, frequency);
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

        sample_stats("mercury-callback-end-to-end", samples, frequency);
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

    fn sample_stats(label: &str, mut samples_ticks: Vec<i64>, frequency: f64) {
        samples_ticks.sort_unstable();
        let to_us = |ticks: i64| ticks as f64 * 1_000_000.0 / frequency;
        let len = samples_ticks.len();
        let p = |percentile: f64| -> f64 {
            let index = ((len.saturating_sub(1)) as f64 * percentile).round() as usize;
            to_us(samples_ticks[index])
        };
        let average = samples_ticks.iter().copied().sum::<i64>() as f64 / len as f64;
        let average_us = average * 1_000_000.0 / frequency;
        let max_us = to_us(*samples_ticks.last().unwrap_or(&0));

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
