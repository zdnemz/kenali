//! VM-only installer task for the P0-S1 credential-provider spike.
//!
//! AN AI AGENT ON A NON-WINDOWS HOST MUST NEVER RUN INSTALL OR REGISTRATION.
//! This module only *constructs* the owner-run steps: it never writes registry
//! state, never copies a DLL into System32, and never calls `regsvr32`.
//! `KENALI_ALLOW_INSTALL=1` is the sole execution gate; on this host the guard
//! bails before anything else happens. The owner runs the printed steps by
//! hand, VM first (docs/dev_safety_runbook.md).
//!
//! Recovery map (docs/dev_safety_runbook.md §§3–8): R-E rollback|previous
//! version directory, R-C Safe Mode, R-D WinRE. Keep the previous version dir;
//! never edit a registered DLL in place.

use anyhow::{Context, bail};

/// Fixed, absolute install root. Never relative, never user-writable.
const INSTALL_PREFIX: &str = r"C:\Program Files\Kenali";

/// File name of the cdylib produced by the `kenali-cp` lib target.
const CP_DLL: &str = "kenali-cp.dll";

/// Env var that arms the installer. The guard is the only execution check on
/// a non-Windows host.
const ALLOW_ENV: &str = "KENALI_ALLOW_INSTALL";

/// Pure predicate for the guard: only the exact value `1` arms installation.
fn allow_value_ok(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Reads the guard env var. Missing, or any value other than `1`, disarms it.
fn install_allowed() -> bool {
    allow_value_ok(std::env::var(ALLOW_ENV).ok().as_deref())
}

/// Absolute versioned install directory: `<prefix>\<version>`.
fn versioned_install_dir(version: &str) -> String {
    format!("{INSTALL_PREFIX}\\{version}")
}

/// Absolute path of the CP DLL inside the versioned directory.
fn cp_dll_path(version: &str) -> String {
    format!("{}\\{CP_DLL}", versioned_install_dir(version))
}

/// Pre-flight checklist from `docs/dev_safety_runbook.md` §2.
fn preflight_text() -> &'static str {
    "\
PRE-FLIGHT — docs/dev_safety_runbook.md §2 (record on the target VM/account, elevated PowerShell):
  1.  OS build / feature release ......... winver
  2.  TPM present & ready ................ Get-Tpm
  3.  BitLocker / Device Encryption ...... manage-bde -status
  4.  Secure Boot ........................ Confirm-SecureBootUEFI
  5.  Camera VID/PID + formats ........... Get-PnpDevice -Class Camera,Image
  6.  CPU ................................ Get-CimInstance Win32_Processor
  7.  Smart App Control state ............ Windows Security -> App & browser control
  8.  Account type + has password ........ Settings -> Accounts (local vs Microsoft)
  9.  GPU / GL availability .............. VM display adapter + egui glow smoke test
 10.  Windows Hello face ................. Settings -> Sign-in options
 11.  Restore point / VM checkpoint ...... mandatory before any registration
"
}

/// The VM-first ladder (docs/dev_safety_runbook.md §1). Main account is last.
fn ladder_text() -> &'static str {
    "\
VM-FIRST LADDER — docs/dev_safety_runbook.md §1 (advance one rung at a time):
  1. VM .................. disposable Windows 11 x64, checkpoint taken before install
  2. Secondary account ... local administrator on the physical host; main account untouched
  3. Main account ........ only after VM + secondary soak both pass
"
}

/// Recovery notes surfaced to the owner on every install attempt.
fn recovery_text() -> &'static str {
    "\
ROLLBACK / RECOVERY — docs/dev_safety_runbook.md §§3–8:
  - Never edit a registered DLL in place; keep the previous versioned directory.
  - R-E: `cargo xtask rollback` repoints InprocServer32 to the previous version.
  - Kill switch: set Disabled=1 under the provider CLSID key and HKLM\\SOFTWARE\\Kenali.
  - R-C: Safe Mode loads only the built-in password provider.
  - R-D: WinRE — load offline hives / rename the DLL if Safe Mode is unreachable.
"
}

/// Resolves which rung of the ladder the owner explicitly chose.
fn install_target(vm: bool, host_test_account: bool) -> anyhow::Result<&'static str> {
    match (vm, host_test_account) {
        (true, false) => Ok("rung 1 — VM (disposable Windows 11 x64, checkpoint taken)"),
        (false, true) => Ok("rung 2 — secondary host test account (main account untouched)"),
        (false, false) => bail!(
            "no target: pass --vm or --host-test-account; never register on the main account first \
             (VM -> secondary account -> main)"
        ),
        (true, true) => bail!(
            "--vm and --host-test-account are mutually exclusive; advance the ladder one rung at a time"
        ),
    }
}

/// Constructs the CLSID registration steps for the owner to run by hand in the VM.
fn registration_steps(clsid: &str, dll_path: &str) -> Vec<String> {
    let class = format!(r"HKLM:\SOFTWARE\Classes\CLSID\{{{clsid}}}");
    let inproc = format!(r"{class}\InprocServer32");
    let provider = format!(
        r"HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{{{clsid}}}"
    );
    vec![
        format!("New-Item -Path '{class}' -Force"),
        format!(
            "New-ItemProperty -Path '{class}' -Name '(Default)' -PropertyType String -Value 'Kenali Credential Provider' -Force"
        ),
        format!("New-Item -Path '{inproc}' -Force"),
        format!(
            "New-ItemProperty -Path '{inproc}' -Name '(Default)' -PropertyType String -Value '{dll_path}' -Force"
        ),
        format!(
            "New-ItemProperty -Path '{inproc}' -Name 'ThreadingModel' -PropertyType String -Value 'Apartment' -Force"
        ),
        format!("New-Item -Path '{provider}' -Force"),
    ]
}

fn print_runbook_facts() {
    println!("{}", preflight_text());
    println!("{}", ladder_text());
    println!("{}", recovery_text());
}

/// Install task entry point. Refuses on this host unless explicitly armed.
pub fn install(vm: bool, host_test_account: bool) -> anyhow::Result<()> {
    // Guard FIRST. On a non-Windows host this is the only execution check;
    // no Windows operation, registry write or DLL copy is ever attempted here.
    if !install_allowed() {
        print_runbook_facts();
        bail!(
            "install refused: set {ALLOW_ENV}=1 to arm the VM-only installer. \
             No registration was attempted. See docs/dev_safety_runbook.md."
        );
    }

    let target = install_target(vm, host_test_account)
        .context("selecting the VM-first install rung")?;

    let version = env!("CARGO_PKG_VERSION");
    let version_dir = versioned_install_dir(version);
    let dll_path = cp_dll_path(version);
    let clsid = kenali_winapi::com::KENALI_CP_CLSID;

    print_runbook_facts();
    println!("TARGET: {target}");
    println!("VERSION: {version}");
    println!("Install directory (absolute): {version_dir}");
    println!("CP DLL (absolute): {dll_path}");
    println!("CP CLSID: {clsid}");

    let steps = registration_steps(clsid, &dll_path);
    println!("\nCONSTRUCTED STEPS — owner runs these by hand in the VM; this task executes none of them:");
    println!("  0. Take a VM checkpoint / restore point BEFORE any registration.");
    println!("  1. Copy kenali-cp.dll to: {dll_path}");
    println!("  2. Keep the previous version directory intact (never edit a registered DLL in place).");
    for (i, step) in steps.iter().enumerate() {
        println!("  {}. {}", i + 3, step);
    }
    println!(
        "  {}. Lock / sign out and back in; confirm password/PIN still works.",
        steps.len() + 3
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_dir_is_absolute_and_uses_version() {
        assert_eq!(
            versioned_install_dir("0.1.0"),
            r"C:\Program Files\Kenali\0.1.0"
        );
        assert_eq!(
            versioned_install_dir("2.3.4"),
            r"C:\Program Files\Kenali\2.3.4"
        );
    }

    #[test]
    fn dll_path_sits_inside_versioned_dir() {
        assert_eq!(
            cp_dll_path("0.1.0"),
            r"C:\Program Files\Kenali\0.1.0\kenali-cp.dll"
        );
    }

    #[test]
    fn guard_predicate_accepts_only_exact_one() {
        assert!(allow_value_ok(Some("1")));
        assert!(!allow_value_ok(Some("0")));
        assert!(!allow_value_ok(Some("true")));
        assert!(!allow_value_ok(Some(" 1")));
        assert!(!allow_value_ok(None));
    }

    #[test]
    fn preflight_contains_key_facts() {
        let text = preflight_text();
        for fact in [
            "winver",
            "Get-Tpm",
            "manage-bde -status",
            "Confirm-SecureBootUEFI",
            "Camera",
            "Win32_Processor",
            "Smart App Control",
            "Account",
            "GPU",
            "checkpoint",
        ] {
            assert!(text.contains(fact), "pre-flight is missing: {fact}");
        }
    }

    #[test]
    fn registration_steps_reference_clsid_and_absolute_dll() {
        let dll = cp_dll_path("0.1.0");
        let steps = registration_steps(kenali_winapi::com::KENALI_CP_CLSID, &dll);
        assert!(!steps.is_empty());
        assert!(steps
            .iter()
            .any(|step| step.contains(kenali_winapi::com::KENALI_CP_CLSID)));
        assert!(steps.iter().any(|step| step.contains(&dll)));
        assert!(steps.iter().any(|step| step.contains("InprocServer32")));
    }
}
