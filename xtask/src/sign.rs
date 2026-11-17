#![forbid(unsafe_code)]

//! Signing task. Requires `KENALI_SIGN_CERT` naming the owner's dev
//! certificate. Signs exactly the four shipped binaries with signtool /fd
//! SHA256. No certificate or PFX is ever committed. Owner-run on Windows only.

use anyhow::{Context, bail};
use std::env;
use std::path::Path;
use std::process::Command;

const CERT_ENV: &str = "KENALI_SIGN_CERT";

/// The exact set of shipped binaries to sign, in signing order.
const BINARIES: [&str; 4] = [
    "kenali-svc.exe",
    "kenali-cp.dll",
    "kenali-ui.exe",
    "kenali.exe",
];

/// Sign the four release binaries. Refuses when the certificate env var is
/// unset or empty; never embeds or commits a cert.
pub fn sign() -> anyhow::Result<()> {
    let cert = env::var(CERT_ENV).unwrap_or_default();
    if cert.trim().is_empty() {
        print_sign_preflight();
        bail!(
            "sign refused: set {CERT_ENV}=<your dev certificate> (owner-run on Windows). \
             No certificate is committed; no binary was signed."
        );
    }

    print_sign_preflight();
    println!("CERT: {cert} (owner-supplied; never committed)");

    for name in BINARIES {
        let path = Path::new("target")
            .join("x86_64-pc-windows-msvc")
            .join("release")
            .join(name);
        if !path.is_file() {
            bail!("sign: binary not found: {}", path.display());
        }
        println!("  -> signtool sign /fd SHA256 /n \"{cert}\" {}", path.display());
        let status = Command::new("signtool")
            .arg("sign")
            .arg("/fd")
            .arg("SHA256")
            .arg("/n")
            .arg(&cert)
            .arg(&path)
            .status()
            .with_context(|| format!("spawning signtool for {name}"))?;
        if !status.success() {
            bail!("signtool failed for {name}: status {status}");
        }
    }

    println!("xtask sign: all binaries signed");
    Ok(())
}

fn print_sign_preflight() {
    println!(
        "SIGN PRE-FLIGHT — owner-run on Windows, release binaries already built:\n\
         \x20 1. Build release: cargo xtask build --release\n\
         \x20 2. Set {CERT_ENV} to a dev certificate installed in the CurrentUser store.\n\
         \x20 3. Verify signatures with `signtool verify /pa /v <binary>` after signing.\n\
         \x20 4. Never commit a certificate, PFX, or private key."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_exactly_four_named_binaries() {
        assert_eq!(BINARIES.len(), 4);
        assert_eq!(BINARIES[0], "kenali-svc.exe");
        assert_eq!(BINARIES[1], "kenali-cp.dll");
        assert_eq!(BINARIES[2], "kenali-ui.exe");
        assert_eq!(BINARIES[3], "kenali.exe");
    }
}
