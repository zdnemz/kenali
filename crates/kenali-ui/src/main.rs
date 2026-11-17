#![forbid(unsafe_code)]

//! `kenali-ui.exe` entry point. Enforce single-instance, elevation and DPI
//! awareness before handing a prepared `App` to `eframe::run_native`.

use kenali_ui::window;
use kenali_winapi::ui::{
    acquire_ui_instance, focus_existing_instance, relaunch_elevated,
    set_per_monitor_dpi_awareness,
};

fn main() {
    let _instance = match acquire_ui_instance() {
        Ok(instance) => instance,
        Err(_) => {
            let _ = focus_existing_instance();
            return;
        }
    };

    match relaunch_elevated() {
        Ok(true) => return,
        Ok(false) => {}
        Err(_) => return,
    }

    if set_per_monitor_dpi_awareness().is_err() {
        return;
    }

    let (options, app) = match window::run() {
        Ok(prepared) => prepared,
        Err(_) => {
            eprintln!("The management interface could not start.");
            return;
        }
    };

    if eframe::run_native(
        window::APP_TITLE,
        options,
        Box::new(move |_cc| Ok(Box::new(app) as Box<dyn eframe::App>)),
    )
    .is_err()
    {
        eprintln!("The management interface stopped unexpectedly.");
    }
}
