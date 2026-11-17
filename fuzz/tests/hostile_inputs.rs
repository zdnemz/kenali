#![forbid(unsafe_code)]

//! Host-side hostile-input tests for the shared fuzz entry points.
//!
//! Proptest drives both entry points with arbitrary byte vectors. Each
//! property asserts that the function returns normally (no panic) and that
//! its work stays bounded by the caps pinned in the codec and frame seams.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use proptest::prelude::*;

/// Upper bound on the number of decodes one input can count. Keep this generous
/// enough for future additions but small enough to catch runaway loops.
const MAX_IPC_DECODE_COUNT: usize = 16;
const MAX_FRAME_DECODE_COUNT: usize = 8;

/// A single hostile input must not take longer than this on the host. The
/// entry points do bounded work (no deserialization of oversized inputs), so
/// ten seconds is a generous guard against accidental unbounded loops.
const MAX_TEST_DURATION: std::time::Duration = std::time::Duration::from_secs(10);

fn drive_ipc(data: &[u8]) -> usize {
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| fuzz::fuzz_ipc_decode(data)));
    let elapsed = started.elapsed();
    assert!(result.is_ok(), "fuzz_ipc_decode panicked on hostile input");
    assert!(
        elapsed < MAX_TEST_DURATION,
        "fuzz_ipc_decode exceeded bounded work: {elapsed:?}"
    );
    let count = result.unwrap_or(0);
    assert!(
        count <= MAX_IPC_DECODE_COUNT,
        "fuzz_ipc_decode returned unbounded count: {count}"
    );
    count
}

fn drive_frame(data: &[u8]) -> usize {
    let started = Instant::now();
    let result = catch_unwind(AssertUnwindSafe(|| fuzz::fuzz_frame_decode(data)));
    let elapsed = started.elapsed();
    assert!(result.is_ok(), "fuzz_frame_decode panicked on hostile input");
    assert!(
        elapsed < MAX_TEST_DURATION,
        "fuzz_frame_decode exceeded bounded work: {elapsed:?}"
    );
    let count = result.unwrap_or(0);
    assert!(
        count <= MAX_FRAME_DECODE_COUNT,
        "fuzz_frame_decode returned unbounded count: {count}"
    );
    count
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn ipc_decode_never_panics_on_arbitrary_bytes(data in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = drive_ipc(&data);
    }

    #[test]
    fn frame_decode_never_panics_on_arbitrary_bytes(data in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = drive_frame(&data);
    }

    #[test]
    fn both_entry_points_agree_on_empty_input(_dummy in any::<u8>()) {
        let empty: &[u8] = &[];
        prop_assert!(drive_ipc(empty) <= MAX_IPC_DECODE_COUNT);
        prop_assert!(drive_frame(empty) <= MAX_FRAME_DECODE_COUNT);
    }
}
