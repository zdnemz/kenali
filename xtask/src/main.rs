#![forbid(unsafe_code)]

//! `cargo xtask` runner. The `install` subcommand is VM-only and arms only
//! with `KENALI_ALLOW_INSTALL=1`; it constructs owner-run steps and never
//! executes registration on the builder host.

mod install;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Kenali developer task runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Construct the VM-only credential-provider install steps (owner-run).
    Install {
        /// Rung 1: disposable Windows 11 VM with a checkpoint taken.
        #[arg(long)]
        vm: bool,
        /// Rung 2: secondary host local-admin test account.
        #[arg(long)]
        host_test_account: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install {
            vm,
            host_test_account,
        } => install::install(vm, host_test_account),
    }
}
