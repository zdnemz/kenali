#![forbid(unsafe_code)]

//! Host-checkable verification commands. These run on the authoring host and
//! never install, register, sign, or touch Windows state.

use anyhow::{Context, bail};
use std::process::Command;

/// Run the standard host-side checks: cargo check (MSVC), clippy, cargo-deny
/// and cargo-audit. Each tool is invoked read-only; nothing is installed or
/// registered. Missing optional tools are reported rather than silently
/// skipped.
pub fn check() -> anyhow::Result<()> {
    println!("xtask check — host-checkable verification");

    let target = "x86_64-pc-windows-msvc";

    run(
        "cargo",
        &[
            "check",
            "--target",
            target,
            "--locked",
            "--workspace",
        ],
        "MSVC type-check",
    )?;

    run(
        "cargo",
        &["clippy", "--locked", "--workspace", "--", "-D", "warnings"],
        "clippy (all targets)",
    )?;

    // Supply-chain checks are installed on the owner's host; if absent, fail
    // with an explicit message so the gap is visible.
    run_tool(
        "cargo-deny",
        "cargo",
        &["deny", "check", "-A", "unmaintained"],
        "cargo-deny",
    )?;
    run_tool("cargo-audit", "cargo", &["audit"], "cargo-audit")?;

    println!("xtask check: all host checks passed");
    Ok(())
}

/// Run a command, surfacing a clear error on failure.
fn run(program: &str, args: &[&str], what: &str) -> anyhow::Result<()> {
    println!("  -> {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("spawning {what} ({program})"))?;
    if !status.success() {
        bail!("{what} failed with status {status}");
    }
    Ok(())
}

/// Run a tool by probing for a helper binary first; if the probe fails,
/// explain which cargo subcommand must be installed.
fn run_tool(
    probe: &str,
    program: &str,
    args: &[&str],
    what: &str,
) -> anyhow::Result<()> {
    let found = Command::new(probe)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !found {
        bail!("{what} is not installed; install `{probe}` before running xtask check");
    }
    run(program, args, what)
}
