//! Diagnostics: the UI chooses the save path; the service returns redacted
//! bytes over the admin pipe. No path is ever sent to the service.

use crate::admin_client;
use crate::pipe_worker::PipeCommand;
use crate::strings as s;
use crate::window::App;

#[derive(Default)]
pub(crate) struct DiagnosticsState {
    pub(crate) save_path: String,
    pub(crate) receiving: bool,
}

/// Accumulate one chunk; when the batch completes, write it to the path the
/// operator chose. Errors surface as a generic notice, never as a reason code.
pub(crate) fn on_chunk(app: &mut App, index: u32, total: u32, bytes: Vec<u8>) {
    if !app.diagnostics.receiving {
        return;
    }
    app.diag_bytes.extend_from_slice(&bytes);
    if index.saturating_add(1) >= total {
        app.diagnostics.receiving = false;
        let path = app.diagnostics.save_path.clone();
        match std::fs::write(&path, &app.diag_bytes) {
            Ok(()) => app.notice = Some(s::MSG_EXPORT_SAVED.to_owned()),
            Err(_) => app.notice = Some(s::MSG_EXPORT_FAILED.to_owned()),
        }
        app.diag_bytes.clear();
    }
}

/// Abort an in-flight transfer and drop the buffered bytes.
pub(crate) fn cancel(app: &mut App) {
    app.diagnostics.receiving = false;
    app.diag_bytes.clear();
}

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    ui.heading(s::HDR_DIAGNOSTICS);
    ui.separator();

    if let Some(notice) = app.notice.as_deref() {
        ui.label(notice);
    }

    let pid = app
        .service_pid
        .map(|p| p.to_string())
        .unwrap_or_else(|| s::LBL_UNKNOWN.to_owned());
    ui.label(format!("{}: {}", s::LBL_SERVICE, pid));

    ui.horizontal(|ui| {
        ui.label(s::LBL_SAVE_PATH);
        ui.text_edit_singleline(&mut app.diagnostics.save_path);
    });

    if !app.diagnostics.receiving {
        let ready = !app.diagnostics.save_path.trim().is_empty();
        if ui.add_enabled(ready, egui::Button::new(s::BTN_EXPORT)).clicked() {
            app.diag_bytes.clear();
            app.diagnostics.receiving = true;
            let _ = admin_client::send(PipeCommand::ExportDiagnostics);
        }
    } else {
        ui.label(s::LBL_RECEIVING);
        if ui.button(s::BTN_CANCEL).clicked() {
            cancel(app);
        }
    }
}
