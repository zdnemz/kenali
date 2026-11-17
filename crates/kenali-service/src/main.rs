#![forbid(unsafe_code)]

//! `kenali-svc.exe` entry point (SCM service name `KenaliSvc`).
//! The FFI dispatch stays in `kenali_winapi::service_host`; this binary only
//! supplies the workload function. No service is registered or started here.

fn main() {
    let code = kenali_winapi::service_host::run_service(kenali_service::run);
    std::process::exit(code);
}
