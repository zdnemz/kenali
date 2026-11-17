#![forbid(unsafe_code)]

//! `kenali.exe` — internal recovery CLI for the Kenali service.
//!
//! This is a recovery tool, not an operator surface. Every command goes
//! through the single shared [`kenali_winapi::admin_pipe::AdminPipeClient`],
//! which verifies the service PID before exchanging any bytes. Errors and
//! output stay generic: no secret, SID, path or Win32 detail is exposed.

use std::io::{self, Write};

use clap::Parser;

use kenali_core::secret::Password;
use kenali_ipc::admin::{AdminRequest, AdminResponse};
use kenali_ipc::wire::password_into_wide;
use kenali_winapi::admin_pipe::{AdminPipeClient, ADMIN_CONNECT_TIMEOUT};

#[derive(clap::Parser)]
#[command(name = "kenali", about = "Kenali recovery utility (internal use only)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Show current service status.
    Status,
    /// Write the redacted diagnostics archive to standard output.
    ExportDiagnostics,
    /// Disable face unlock.
    Disable,
    /// Refresh the stored credential using a password from standard input.
    RefreshCredential,
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("cannot contact the administration service")]
    Pipe,
    #[error("service rejected the request")]
    Rejected,
    #[error("unexpected response from the administration service")]
    UnexpectedResponse,
    #[error("failed to read standard input")]
    Input,
    #[error("failed to write standard output")]
    Output,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(cli.command) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(command: Command) -> Result<(), CliError> {
    let mut client = AdminPipeClient::connect(ADMIN_CONNECT_TIMEOUT).map_err(|_| CliError::Pipe)?;
    match command {
        Command::Status => {
            let response = client
                .exchange(&AdminRequest::GetStatus)
                .map_err(|_| CliError::Pipe)?;
            match response {
                AdminResponse::Status(status) => {
                    println!("enabled={}", status.enabled);
                    println!("camera_ok={}", status.camera_ok);
                    println!("enrolled={}", status.enrolled_sid.is_some());
                    println!("kill_switch={}", status.kill_switch);
                    println!("face_paused={}", status.face_paused);
                    println!("service_pid={}", status.service_pid);
                    println!("template_count={}", status.template_count);
                    Ok(())
                }
                AdminResponse::Error { .. } => Err(CliError::Rejected),
                _ => Err(CliError::UnexpectedResponse),
            }
        }
        Command::ExportDiagnostics => {
            let response = client
                .exchange(&AdminRequest::ExportDiagnostics)
                .map_err(|_| CliError::Pipe)?;
            match response {
                AdminResponse::DiagnosticsChunk { bytes, .. } => {
                    // ponytail: the bounded report currently fits one chunk;
                    // AdminPipeClient::exchange reads one framed response. Add a
                    // multi-chunk drain API in kenali-winapi if the report grows.
                    let mut stdout = io::stdout().lock();
                    stdout
                        .write_all(bytes.as_slice())
                        .map_err(|_| CliError::Output)?;
                    stdout.flush().map_err(|_| CliError::Output)?;
                    Ok(())
                }
                AdminResponse::Error { .. } => Err(CliError::Rejected),
                _ => Err(CliError::UnexpectedResponse),
            }
        }
        Command::Disable => {
            let response = client
                .exchange(&AdminRequest::SetEnabled(false))
                .map_err(|_| CliError::Pipe)?;
            match response {
                AdminResponse::Status(_) => Ok(()),
                AdminResponse::Error { .. } => Err(CliError::Rejected),
                _ => Err(CliError::UnexpectedResponse),
            }
        }
        Command::RefreshCredential => {
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .map_err(|_| CliError::Input)?;
            while input.ends_with('\n') || input.ends_with('\r') {
                input.pop();
            }
            let units: Vec<u16> = input.encode_utf16().collect();
            input.clear();
            let password = Password::from_wide(units);
            let response = client
                .exchange(&AdminRequest::RefreshCredential(password_into_wide(password)))
                .map_err(|_| CliError::Pipe)?;
            match response {
                AdminResponse::Error { .. } => Err(CliError::Rejected),
                _ => Err(CliError::UnexpectedResponse),
            }
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn subcommands_use_kebab_case_names() {
        assert!(matches!(
            Cli::try_parse_from(["kenali", "status"]),
            Ok(cli) if matches!(cli.command, Command::Status)
        ));
        assert!(matches!(
            Cli::try_parse_from(["kenali", "export-diagnostics"]),
            Ok(cli) if matches!(cli.command, Command::ExportDiagnostics)
        ));
        assert!(matches!(
            Cli::try_parse_from(["kenali", "disable"]),
            Ok(cli) if matches!(cli.command, Command::Disable)
        ));
        assert!(matches!(
            Cli::try_parse_from(["kenali", "refresh-credential"]),
            Ok(cli) if matches!(cli.command, Command::RefreshCredential)
        ));
    }

    #[test]
    fn refresh_credential_has_no_password_argument() {
        assert!(Cli::try_parse_from(["kenali", "refresh-credential", "hunter2"]).is_err());
    }

    #[test]
    fn error_text_stays_generic() {
        for error in [
            CliError::Pipe,
            CliError::Rejected,
            CliError::UnexpectedResponse,
            CliError::Input,
            CliError::Output,
        ] {
            let text = error.to_string();
            assert!(!text.contains("password"));
            assert!(!text.contains("S-1-"));
            assert!(!text.contains("C:\\"));
        }
    }
}
