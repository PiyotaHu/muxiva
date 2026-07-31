use serde::Serialize;
use std::{
    collections::VecDeque,
    env,
    process::ExitCode,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const SCENARIO_VERSION: u32 = 1;

#[derive(Serialize)]
struct Report<'a> {
    scenario_version: u32,
    scenario: &'a str,
    commit: String,
    os: &'static str,
    arch: &'static str,
    toolchain: String,
    iterations: u64,
    warmup_iterations: u64,
    payload_bytes: usize,
    elapsed_ns: u128,
    operations_per_second: f64,
    copied_bytes: u64,
    samples_ns: Vec<u128>,
}

fn main() -> ExitCode {
    let scenario = env::args().nth(1).unwrap_or_default();
    if !matches!(
        scenario.as_str(),
        "queue" | "flow" | "frame-copy" | "managed-stream" | "stop"
    ) {
        eprintln!("usage: voxa-bench <queue|flow|frame-copy|managed-stream|stop>");
        return ExitCode::FAILURE;
    }

    let iterations = env_u64("VOXA_BENCH_ITERATIONS", 10_000).max(1);
    let warmup = env_u64("VOXA_BENCH_WARMUP", 1_000);
    let payload_bytes = env_u64("VOXA_BENCH_PAYLOAD_BYTES", 4096) as usize;
    run_once(&scenario, warmup, payload_bytes);
    let started = Instant::now();
    let (copied_bytes, samples_ns) = run_once(&scenario, iterations, payload_bytes);
    let elapsed = started.elapsed();
    let report = Report {
        scenario_version: SCENARIO_VERSION,
        scenario: &scenario,
        commit: env::var("VOXA_BENCH_COMMIT").unwrap_or_else(|_| "unknown".into()),
        os: env::consts::OS,
        arch: env::consts::ARCH,
        toolchain: env::var("VOXA_BENCH_TOOLCHAIN").unwrap_or_else(|_| "unknown".into()),
        iterations,
        warmup_iterations: warmup,
        payload_bytes,
        elapsed_ns: elapsed.as_nanos(),
        operations_per_second: iterations as f64 / elapsed.as_secs_f64(),
        copied_bytes,
        samples_ns,
    };
    println!(
        "{}",
        serde_json::to_string(&report).expect("report serializes")
    );
    ExitCode::SUCCESS
}

fn run_once(scenario: &str, iterations: u64, payload_bytes: usize) -> (u64, Vec<u128>) {
    match scenario {
        "queue" => bench_queue(iterations),
        "flow" => bench_flow(iterations),
        "frame-copy" => bench_copy(iterations, payload_bytes),
        "managed-stream" => bench_managed(iterations),
        "stop" => bench_stop(iterations.min(100)),
        _ => unreachable!(),
    }
}

fn bench_queue(iterations: u64) -> (u64, Vec<u128>) {
    let mut queue = VecDeque::with_capacity(256);
    let mut samples = Vec::new();
    for value in 0..iterations {
        let started = Instant::now();
        queue.push_back(value);
        let _ = queue.pop_front();
        sample(&mut samples, value, started.elapsed());
    }
    (0, samples)
}

fn bench_flow(iterations: u64) -> (u64, Vec<u128>) {
    let mut pressure = 0_u64;
    for value in 0..iterations {
        pressure = pressure.wrapping_add(value).saturating_mul(7) / 8;
        std::hint::black_box(pressure);
    }
    (0, Vec::new())
}

fn bench_copy(iterations: u64, payload_bytes: usize) -> (u64, Vec<u128>) {
    let payload = vec![0x5a; payload_bytes];
    for _ in 0..iterations {
        std::hint::black_box(payload.clone());
    }
    (iterations.saturating_mul(payload_bytes as u64), Vec::new())
}

fn bench_managed(iterations: u64) -> (u64, Vec<u128>) {
    let mailbox = Arc::new(Mutex::new(VecDeque::new()));
    for value in 0..iterations {
        let mut guard = mailbox.lock().expect("mailbox lock");
        guard.push_back(value);
        let _ = guard.pop_front();
    }
    (0, Vec::new())
}

fn bench_stop(iterations: u64) -> (u64, Vec<u128>) {
    let mut samples = Vec::with_capacity(iterations as usize);
    for value in 0..iterations {
        let started = Instant::now();
        thread::spawn(|| {}).join().expect("worker joins");
        sample(&mut samples, value, started.elapsed());
    }
    (0, samples)
}

fn sample(samples: &mut Vec<u128>, iteration: u64, elapsed: Duration) {
    if iteration < 64 || iteration.is_power_of_two() {
        samples.push(elapsed.as_nanos());
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
