#![forbid(unsafe_code)]

//! `kenali-bench` developer CLI. Subcommands are dispatched from
//! `std::env::args` — no CLI/async/network dependency. All workloads are
//! synthetic (no model weights, camera or TPM); measurements use `Instant`.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use kenali_bench::calibration::{choose_threshold, roc_curve};
use kenali_bench::latency::{measure_cold, measure_warm, summarize, Summary};
use kenali_bench::throughput::per_frame_ms;

const NFR02_BUDGET_MS: f64 = 40.0;

fn usage() {
    eprintln!(
        "kenali-bench — developer benchmark/calibration harness (synthetic workloads)\n\
         USAGE:\n\
           kenali-bench latency [iterations]   warm vs cold timing, p50/p95\n\
           kenali-bench throughput [frames]    ms/frame vs NFR-02 budget\n\
           kenali-bench calibration            threshold at target FAR + ROC points\n\
           kenali-bench profile                per-stage synthetic stage timings"
    );
}

fn parse_usize(arg: Option<&String>, default: usize) -> usize {
    arg.and_then(|s| s.parse::<usize>().ok()).unwrap_or(default)
}

fn spin(duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        std::hint::spin_loop();
    }
}

fn print_summary(label: &str, samples: &[Duration]) {
    match summarize(samples) {
        Some(Summary {
            count,
            min,
            max,
            mean,
            p50,
            p95,
        }) => println!(
            "{label}: n={count} min={min:?} p50={p50:?} p95={p95:?} max={max:?} mean={mean:?}"
        ),
        None => println!("{label}: no samples"),
    }
}

fn cmd_latency(args: &[String]) {
    let iterations = parse_usize(args.first(), 100);
    println!("latency: {iterations} iterations (synthetic workload)");
    let warm = measure_warm(|| (), |_| spin(Duration::from_micros(200)), iterations);
    let cold = measure_cold(
        || spin(Duration::from_micros(500)),
        |_| spin(Duration::from_micros(200)),
        iterations,
    );
    print_summary("warm", &warm);
    print_summary("cold", &cold);
}

fn cmd_throughput(args: &[String]) {
    let frames = parse_usize(args.first(), 100);
    // 4 ms of synthetic work per frame; total derives from the frame count.
    let total = Duration::from_micros(4_000 * frames as u64);
    match per_frame_ms(total, frames) {
        Some(ms) => {
            let fps = if ms > 0.0 { 1_000.0 / ms } else { f64::INFINITY };
            let verdict = if ms <= NFR02_BUDGET_MS {
                "within"
            } else {
                "OVER"
            };
            println!(
                "throughput: {frames} frames in {total:?} => {ms:.3} ms/frame \
                 ({fps:.1} fps); {verdict} NFR-02 budget of {NFR02_BUDGET_MS:.0} ms"
            );
        }
        None => println!("throughput: frames=0 => rate undefined (no frames to amortise)"),
    }
}

fn cmd_calibration() {
    let impostor: Vec<f64> = (0..20).map(|i| i as f64 * 0.025).collect();
    let genuine: Vec<f64> = (0..20).map(|i| 0.55 + i as f64 * 0.02).collect();
    let candidates: Vec<f64> = (0..=20).map(|i| i as f64 * 0.05).collect();
    let target_far = 0.05;

    match choose_threshold(&impostor, &genuine, &candidates, target_far) {
        Some(r) => println!(
            "calibration: threshold={:.3} far={:.4} tar={:.4} (target FAR {target_far})",
            r.threshold, r.far, r.tar
        ),
        None => println!("calibration: no candidate meets target FAR {target_far}"),
    }
    let curve = roc_curve(&impostor, &genuine);
    println!(
        "calibration: ROC points={} (fpr {:.3}..{:.3})",
        curve.len(),
        curve.first().map_or(0.0, |p| p.fpr),
        curve.last().map_or(0.0, |p| p.fpr)
    );
}

fn cmd_profile() {
    println!("profile: per-stage synthetic timings (p50/p95)");
    let stages: [(&str, Duration); 4] = [
        ("detect", Duration::from_micros(300)),
        ("align", Duration::from_micros(150)),
        ("embed", Duration::from_micros(500)),
        ("pad", Duration::from_micros(250)),
    ];
    for (name, cost) in stages {
        let samples = measure_warm(|| (), |_| spin(cost), 64);
        print_summary(name, &samples);
    }
}

fn run(args: &[String]) -> Result<(), ()> {
    match args.first().map(String::as_str) {
        Some("latency") => {
            cmd_latency(&args[1..]);
            Ok(())
        }
        Some("throughput") => {
            cmd_throughput(&args[1..]);
            Ok(())
        }
        Some("calibration") => {
            cmd_calibration();
            Ok(())
        }
        Some("profile") => {
            cmd_profile();
            Ok(())
        }
        Some("-h") | Some("--help") | Some("help") => {
            usage();
            Ok(())
        }
        _ => {
            eprintln!("unknown or missing subcommand");
            usage();
            Err(())
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}
