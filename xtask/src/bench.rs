#![forbid(unsafe_code)]

//! Benchmark task. Delegates to the `kenali-bench` harness; no camera, model,
//! TPM or hardware is touched. Results are printed for the owner to record.

use anyhow::{Context, bail};
use std::process::Command;

/// Run the benchmark binary (host-safe; no hardware or model required).
pub fn bench() -> anyhow::Result<()> {
    println!("xtask bench — running kenali-bench harness");
    let status = Command::new("cargo")
        .arg("run")
        .arg("--locked")
        .arg("-p")
        .arg("kenali-bench")
        .arg("--")
        .arg("--summary")
        .status()
        .with_context(|| "spawning kenali-bench")?;
    if !status.success() {
        bail!("kenali-bench failed with status {status}");
    }
    println!("xtask bench: complete");
    Ok(())
}
