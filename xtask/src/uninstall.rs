#![forbid(unsafe_code)]

//! Uninstall task. Guarded by `KENALI_ALLOW_INSTALL=1` exactly like install.
//! Constructs owner-run steps; never writes registry state itself.

use anyhow::bail;
use std::env;

const ALLOW_ENV: &str = "KENALI_ALLOW_INSTALL";

/// Pre-flight text printed before the guard bails, mirroring install.rs.
fn preflight_text() -> &'static str {
    "\
PRE-FLIGHT — docs/dev_safety_runbook.md §2 (record on the target VM/account):
  1.  Restore point / VM checkpoint taken BEFORE uninstall.
  2.  Know the previous version directory (never delete it until verified).
  3.  Confirm password/PIN sign-in still works after deregistration.
"
}

/// Guard: only the exact value `1` arms uninstall.
fn allow_value_ok(value: Option<&str>) -> bool {
    value == Some("1")
}

fn install_allowed() -> bool {
    allow_value_ok(env::var(ALLOW_ENV).ok().as_deref())
}

/// Construct the owner-run uninstall steps. Never executes registry changes.
pub fn uninstall() -> anyhow::Result<()> {
    if !install_allowed() {
        print_runbook_facts();
        bail!(
            "uninstall refused: set {ALLOW_ENV}=1 to arm the VM-only uninstall. \
             No registry change was attempted. See docs/dev_safety_runbook.md."
        );
    }
    print_runbook_facts();
    println!("\nCONSTRUCTED STEPS — owner runs these by hand in the VM; this task executes none:");
    println!("  1. Verify a restore point / VM checkpoint exists.");
    println!("  2. Remove the provider CLSID registration keys (owner-run, elevated).");
    println!("  3. Remove HKLM\\SOFTWARE\\Kenali configuration keys.");
    println!("  4. Leave the versioned install directory in place for rollback.");
    println!("  5. Lock / sign out and back in; confirm password/PIN still works.");
    Ok(())
}

fn print_runbook_facts() {
    println!("{}", preflight_text());
    println!(
        "RECOVERY — docs/dev_safety_runbook.md §§3–8: kill switch Disabled=1, \
         R-C Safe Mode, R-D WinRE, R-E versioned rollback."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_predicate_accepts_only_exact_one() {
        assert!(allow_value_ok(Some("1")));
        assert!(!allow_value_ok(Some("0")));
        assert!(!allow_value_ok(Some("true")));
        assert!(!allow_value_ok(None));
    }
}
