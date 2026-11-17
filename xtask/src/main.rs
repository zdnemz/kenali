#![forbid(unsafe_code)]

//! `cargo xtask` runner. All Windows-facing commands construct owner-run
//! steps; none register, install, sign, or execute on the builder host.
//! Install-family commands arm only with `KENALI_ALLOW_INSTALL=1`.

mod bench;
mod build;
mod check;
mod fuzz;
mod install;
mod models;
mod package;
mod rollback;
mod sbom;
mod sign;
mod uninstall;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Kenali developer task runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run host-checkable checks (cargo check, clippy, deny, audit).
    Check,
    /// Build Windows binaries (optionally release with v3 CPU tuning).
    Build {
        /// Build in release mode with target-cpu=x86-64-v3 for service and UI.
        #[arg(long)]
        release: bool,
    },
    /// Fetch or verify model weights against models/models.lock.
    Models {
        #[command(subcommand)]
        action: ModelsCommand,
    },
    /// Sign release binaries (requires KENALI_SIGN_CERT, owner-run on Windows).
    Sign,
    /// Construct the VM-only credential-provider install steps (owner-run).
    Install {
        /// Rung 1: disposable Windows 11 VM with a checkpoint taken.
        #[arg(long)]
        vm: bool,
        /// Rung 2: secondary host local-admin test account.
        #[arg(long)]
        host_test_account: bool,
    },
    /// Construct uninstall steps (owner-run, guarded).
    Uninstall,
    /// Construct rollback steps (owner-run, guarded).
    Rollback,
    /// Run a fuzz target for a bounded time (owner-run on Windows).
    Fuzz {
        /// Fuzz target: ipc_decode or frame_decode.
        target: String,
        /// Maximum seconds to run.
        secs: u64,
    },
    /// Run the benchmark harness.
    Bench,
    /// Stage release binaries, PDBs and SBOM under target/package/.
    Package,
    /// Generate CycloneDX SBOM under target/sbom/.
    Sbom,
}

#[derive(Debug, Subcommand)]
enum ModelsCommand {
    /// Download model assets listed in models/models.lock.
    Fetch,
    /// Recompute SHA-256 of each model and reject mismatches before use.
    Verify,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check => check::check(),
        Command::Build { release } => build::build(release),
        Command::Models { action } => match action {
            ModelsCommand::Fetch => models::models(true, false),
            ModelsCommand::Verify => models::models(false, true),
        },
        Command::Sign => sign::sign(),
        Command::Install {
            vm,
            host_test_account,
        } => install::install(vm, host_test_account),
        Command::Uninstall => uninstall::uninstall(),
        Command::Rollback => rollback::rollback(),
        Command::Fuzz { target, secs } => fuzz::fuzz(&target, secs),
        Command::Bench => bench::bench(),
        Command::Package => package::package(),
        Command::Sbom => sbom::sbom(),
    }
}
