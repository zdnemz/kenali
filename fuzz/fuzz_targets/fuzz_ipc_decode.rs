#![forbid(unsafe_code)]

//! IPC decode fuzz target driver.
//!
//! Host-runnable driver for the shared byte-slice entry points. It reads
//! newline-delimited inputs from stdin, or a seed corpus directory/file given
//! as the first argument, and calls `fuzz::fuzz_ipc_decode` on each candidate.
//! No external crate is used (`libfuzzer-sys` is intentionally absent), so the
//! binary remains host-testable and MSVC-checkable. No input bytes are logged.

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

/// Cap on bytes read per candidate input so a hostile seed cannot drive
/// unbounded memory use in the driver itself.
const MAX_INPUT_BYTES: usize = 1 << 20;

fn main() {
    let mut successes = 0usize;
    let mut args = env::args_os().skip(1);

    match args.next() {
        Some(arg) => {
            let path = Path::new(&arg);
            if path.is_dir() {
                let Ok(entries) = std::fs::read_dir(path) else {
                    return;
                };
                for entry in entries.flatten() {
                    let file = entry.path();
                    if file.is_file() {
                        if let Ok(bytes) = read_bounded(&file) {
                            successes += fuzz::fuzz_ipc_decode(&bytes);
                        }
                    }
                }
            } else if let Ok(bytes) = read_bounded(path) {
                successes += fuzz::fuzz_ipc_decode(&bytes);
            }
        }
        None => {
            let mut bytes = Vec::new();
            let _ = io::stdin().take(MAX_INPUT_BYTES as u64).read_to_end(&mut bytes);
            if bytes.contains(&b'\n') {
                for candidate in bytes.split(|byte| *byte == b'\n') {
                    successes += fuzz::fuzz_ipc_decode(candidate);
                }
            } else {
                successes += fuzz::fuzz_ipc_decode(&bytes);
            }
        }
    }

    // Numeric summary only; no bytes or diagnostics from the input are logged.
    let _ = writeln!(io::stdout(), "ipc_decode successes: {successes}");
}

fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES as u64).read_to_end(&mut bytes)?;
    Ok(bytes)
}
