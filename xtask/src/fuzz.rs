#![forbid(unsafe_code)]

//! Fuzz task. Accepts exactly `ipc_decode` and `frame_decode` and invokes the
//! matching `fuzz_<target>` binary via `cargo fuzz run` with a bounded
//! `-max_total_time=<secs>`. Owner-run on Windows; no campaign is run here.

use anyhow::{Context, bail};
use std::process::Command;

const TARGETS: [&str; 2] = ["ipc_decode", "frame_decode"];

/// Run one fuzz target for at most `secs` seconds.
pub fn fuzz(target: &str, secs: u64) -> anyhow::Result<()> {
    if !TARGETS.contains(&target) {
        bail!(
            "fuzz target '{target}' is not recognised; choose one of: {}",
            TARGETS.join(", ")
        );
    }
    if secs == 0 {
        bail!("fuzz: secs must be greater than zero");
    }
    let bin = format!("fuzz_{target}");
    let time_arg = format!("-max_total_time={secs}");
    println!("xtask fuzz — target={bin}, {time_arg}");

    // cargo-fuzz is owner-installed; fail clearly if absent.
    let probe = Command::new("cargo-fuzz")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !probe {
        bail!("cargo-fuzz is not installed; install it before running xtask fuzz");
    }

    let status = Command::new("cargo")
        .arg("fuzz")
        .arg("run")
        .arg(&bin)
        .arg("--")
        .arg(&time_arg)
        .status()
        .with_context(|| format!("spawning cargo fuzz run {bin}"))?;
    if !status.success() {
        bail!("cargo fuzz run {bin} failed with status {status}");
    }
    println!("xtask fuzz: {bin} completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_allowlist_is_exact() {
        assert_eq!(TARGETS, ["ipc_decode", "frame_decode"]);
    }
}
