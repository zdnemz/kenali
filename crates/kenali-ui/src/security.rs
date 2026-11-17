//! Security: refresh credentials after a password change, pause/resume face
//! unlock, and show the read-only kill-switch state.

use crate::admin_client;
use crate::password_field::PasswordField;
use crate::pipe_worker::PipeCommand;
use crate::strings as s;
use crate::window::App;

#[derive(Default)]
pub(crate) struct SecurityState {
    refreshing: bool,
    password: PasswordField,
}

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    ui.heading(s::HDR_SECURITY);
    ui.separator();

    let paused = app
        .status
        .as_ref()
        .map(|st| !st.enabled || st.face_paused)
        .unwrap_or(true);
    ui.label(if paused {
        s::FACE_UNLOCK_PAUSED
    } else {
        s::FACE_UNLOCK_READY
    });
    if let Some(st) = app.status.as_ref() {
        if let Some(note) = st.pause_note.as_deref() {
            ui.label(note);
        }
        ui.label(format!("{}: {}", s::LBL_KILL_SWITCH, on_off(st.kill_switch)));
    }
    ui.label(s::LBL_KILL_SWITCH_HINT);
    ui.separator();

    ui.horizontal(|ui| {
        if ui.button(s::BTN_DISABLE).clicked() {
            let _ = admin_client::send(PipeCommand::SetEnabled(false));
        }
        if ui.button(s::BTN_ENABLE).clicked() {
            let _ = admin_client::send(PipeCommand::SetEnabled(true));
        }
    });
    ui.separator();

    if !app.security.refreshing {
        if ui.button(s::BTN_REFRESH).clicked() {
            app.security.refreshing = true;
        }
    } else {
        ui.label(s::USE_YOUR_PASSWORD_OR_PIN);
        app.security.password.ui(ui, s::LBL_PASSWORD);
        ui.horizontal(|ui| {
            let ready = !app.security.password.is_empty();
            if ui.add_enabled(ready, egui::Button::new(s::BTN_CONTINUE)).clicked() {
                let password = app.security.password.submit_password();
                app.security.refreshing = false;
                let _ = admin_client::send(PipeCommand::RefreshCredential(password));
                app.notice = Some(s::MSG_REFRESH_DONE.to_owned());
            }
            if ui.button(s::BTN_CANCEL).clicked() {
                app.security.refreshing = false;
            }
        });
    }
}

fn on_off(value: bool) -> &'static str {
    if value {
        s::LBL_ON
    } else {
        s::LBL_OFF
    }
}
