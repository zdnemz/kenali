#![forbid(unsafe_code)]

//! SBOM task. Produces a CycloneDX XML inventory via `cargo cyclonedx` under
//! `target/sbom/kenali-<version>.cdx.xml`. Owner-run; the tool is not present
//! on the authoring host, so absence is reported explicitly.

use anyhow::{Context, bail};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build the SBOM output path: target/sbom/kenali-<version>.cdx.xml.
fn sbom_path() -> PathBuf {
    let version = env!("CARGO_PKG_VERSION");
    Path::new("target")
        .join("sbom")
        .join(format!("kenali-{version}.cdx.xml"))
}

/// Generate the CycloneDX SBOM. Fails clearly when cargo-cyclonedx is absent.
pub fn sbom() -> anyhow::Result<()> {
    let out = sbom_path();
    println!("xtask sbom — output: {}", out.display());

    let probe = Command::new("cargo-cyclonedx")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !probe {
        bail!("cargo-cyclonedx is not installed; install it before running xtask sbom");
    }

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let status = Command::new("cargo")
        .arg("cyclonedx")
        .arg("--output")
        .arg(&out)
        .status()
        .with_context(|| "spawning cargo cyclonedx")?;
    if !status.success() {
        bail!("cargo cyclonedx failed with status {status}");
    }
    if !out.is_file() {
        bail!("cargo cyclonedx did not create {}", out.display());
    }
    println!("xtask sbom: wrote {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sbom_path_is_under_target_sbm() {
        let p = sbom_path();
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.starts_with("target/sbom/kenali-"), "unexpected path: {s}");
        assert!(s.ends_with(".cdx.xml"), "unexpected path: {s}");
    }
}
