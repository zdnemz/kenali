#![forbid(unsafe_code)]

//! Build orchestration. The release profile is owned by the root Cargo.toml
//! (W5-01). Service and UI receive `-C target-cpu=x86-64-v3`; the credential
//! provider stays baseline for maximum VM compatibility.

use anyhow::{Context, bail};
use std::process::Command;

const TARGET: &str = "x86_64-pc-windows-msvc";

/// Build all Windows binaries. In release mode, service and UI get v3 tuning
/// via RUSTFLAGS while the CP remains at the baseline x86-64 instruction set.
pub fn build(release: bool) -> anyhow::Result<()> {
    let profile = if release { "release" } else { "dev" };
    println!("xtask build — profile={profile}, target={TARGET}");

    // Baseline build of the whole workspace first.
    let mut base = Command::new("cargo");
    base.arg("build").arg("--target").arg(TARGET).arg("--locked");
    if release {
        base.arg("--release");
    }
    println!("  -> cargo build --target {TARGET} --locked{}", if release { " --release" } else { "" });
    let status = base
        .status()
        .with_context(|| "spawning baseline cargo build")?;
    if !status.success() {
        bail!("baseline build failed with status {status}");
    }

    if release {
        // Rebuild service and UI with the v3 target CPU. The CP must stay
        // baseline; limiting the v3 flag to these two binaries preserves that.
        for package in ["kenali-service", "kenali-ui"] {
            let mut cmd = Command::new("cargo");
            cmd.arg("build")
                .arg("--target")
                .arg(TARGET)
                .arg("--locked")
                .arg("--release")
                .arg("-p")
                .arg(package);
            // Environment-scoped flags: +crt-static/CFG/CETCOMPAT come from
            // .cargo/config.toml; we only add target-cpu here.
            cmd.env(
                "RUSTFLAGS",
                "-C target-feature=+crt-static -C control-flow-guard=yes -C link-arg=/CETCOMPAT -C target-cpu=x86-64-v3",
            );
            println!("  -> cargo build --release -p {package} (target-cpu=x86-64-v3)");
            let status = cmd
                .status()
                .with_context(|| format!("spawning v3 build of {package}"))?;
            if !status.success() {
                bail!("v3 build of {package} failed with status {status}");
            }
        }
    }

    println!("xtask build: complete");
    Ok(())
}
