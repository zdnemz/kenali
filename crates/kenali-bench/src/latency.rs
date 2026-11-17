//! Warm/cold latency sampling and deterministic summary statistics.
//!
//! Warm and cold differ only in the setup phase: [`measure_warm`] prepares the
//! workload once and reuses it across iterations, while [`measure_cold`]
//! re-runs setup inside every timed region so setup cost is included in each
//! sample. Both return raw samples; [`summarize`] reduces them to p50/p95.

use std::time::{Duration, Instant};

/// Reduction of a set of latency samples. All fields are deterministic for a
/// given input because [`summarize`] sorts a copy before picking percentiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub count: usize,
    pub min: Duration,
    pub max: Duration,
    pub mean: Duration,
    pub p50: Duration,
    pub p95: Duration,
}

/// Nearest-rank percentile of an *ascending* slice: rank = ceil(p * n),
/// clamped into 1..=n, returned as `sorted[rank - 1]`.
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let n = sorted.len();
    let rank = ((p * n as f64).ceil() as usize).clamp(1, n);
    sorted[rank - 1]
}

/// Compute `min`/`max`/`mean`/`p50`/`p95` from samples. Returns `None` for an
/// empty sample set rather than panicking or inventing a value.
pub fn summarize(samples: &[Duration]) -> Option<Summary> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    let sum: Duration = sorted.iter().sum();
    Some(Summary {
        count,
        min: sorted[0],
        max: sorted[count - 1],
        mean: sum / count as u32,
        p50: percentile(&sorted, 0.50),
        p95: percentile(&sorted, 0.95),
    })
}

/// Time `iterations` executions of `workload` against a single prepared state.
/// `setup` runs once, *outside* every timed region (warm reuse).
pub fn measure_warm<S, W, T>(setup: S, mut workload: W, iterations: usize) -> Vec<Duration>
where
    S: FnOnce() -> T,
    W: FnMut(&mut T),
{
    let mut state = setup();
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        workload(&mut state);
        samples.push(start.elapsed());
    }
    samples
}

/// Time `iterations` executions where `setup` runs *inside* every timed region
/// immediately before `workload`, so setup cost is part of each sample (cold).
pub fn measure_cold<S, W, T>(mut setup: S, mut workload: W, iterations: usize) -> Vec<Duration>
where
    S: FnMut() -> T,
    W: FnMut(&mut T),
{
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        let mut state = setup();
        workload(&mut state);
        samples.push(start.elapsed());
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn summarize_p50_p95_nearest_rank() {
        let s = summarize(&[ms(10), ms(20), ms(30), ms(40), ms(50)]).expect("non-empty");
        assert_eq!(s.count, 5);
        assert_eq!(s.min, ms(10));
        assert_eq!(s.max, ms(50));
        assert_eq!(s.p50, ms(30)); // ceil(0.5*5)=3 -> index 2
        assert_eq!(s.p95, ms(50)); // ceil(0.95*5)=5 -> index 4
    }

    #[test]
    fn summarize_sorts_and_is_order_independent() {
        let a = summarize(&[ms(50), ms(10), ms(30), ms(20), ms(40)]).expect("a");
        let b = summarize(&[ms(10), ms(20), ms(30), ms(40), ms(50)]).expect("b");
        assert_eq!(a, b);
    }

    #[test]
    fn summarize_single_and_empty() {
        let one = summarize(&[ms(7)]).expect("one");
        assert_eq!(one.p50, ms(7));
        assert_eq!(one.p95, ms(7));
        assert_eq!(one.mean, ms(7));
        assert!(summarize(&[]).is_none());
    }

    #[test]
    fn warm_reuses_prepared_state() {
        let seen = std::cell::RefCell::new(Vec::new());
        let samples = measure_warm(
            || 0usize,
            |state| {
                *state += 1;
                seen.borrow_mut().push(*state);
            },
            3,
        );
        assert_eq!(samples.len(), 3);
        assert_eq!(seen.into_inner(), vec![1, 2, 3]);
    }

    #[test]
    fn cold_runs_setup_each_iteration() {
        let seen = std::cell::RefCell::new(Vec::new());
        let samples = measure_cold(
            || 0usize,
            |state| {
                *state += 1;
                seen.borrow_mut().push(*state);
            },
            3,
        );
        assert_eq!(samples.len(), 3);
        assert_eq!(seen.into_inner(), vec![1, 1, 1]);
    }
}
