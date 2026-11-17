#![forbid(unsafe_code)]

//! Rollback task. Guarded by `KENALI_ALLOW_INSTALL=1` exactly like install.
//! R-E of the dev safety runbook: repoint InprocServer32 to the previous
//! versioned directory. Constructs owner-run steps; never touches the registry.

use anyhow::bail;
use std::env;

const ALLOW_ENV: &str = "KENALI_ALLOW_INSTALL";

fn preflight_text() -> &'static str {
    "\
PRE-FLIGHT — docs/dev_safety_runbook.md §2 (record on the target VM/account):
  1.  A previous version directory exists and is intact.
  2.  Restore point / VM checkpoint taken BEFORE rollback.
  3.  Know which CLSID was registered (see install output).
"
}

fn allow_value_ok(value: Option<&str>) -> bool {
    value == Some("1")
}

fn install_allowed() -> bool {
    allow_value_ok(env::var(ALLOW_ENV).ok().as_deref())
}

/// Construct the owner-run rollback steps. Never executes registry changes.
pub fn rollback() -> anyhow::Result<()> {
    if !install_allowed() {
        println!("{}", preflight_text());
        bail!(
            "rollback refused: set {ALLOW_ENV}=1 to arm the VM-only rollback. \
             No registry change was attempted. See docs/dev_safety_runbook.md."
        );
    }
    println!("{}", preflight_text());
    println!("\nCONSTRUCTED STEPS — owner runs these by hand in the VM; this task executes none:");
    println!("  1. Confirm the previous version directory and its kenali-cp.dll.");
    println!("  2. Repoint the CLSID InprocServer32 default value to the previous DLL path.");
    println!("  3. Keep the failed version directory for diagnosis (do not delete).");
    println!("  4. Lock / sign out and back in; confirm password/PIN still works.");
    println!("  5. If Safe Mode is needed, follow R-C; if unreachable, R-D (WinRE).");
    Ok(())
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
