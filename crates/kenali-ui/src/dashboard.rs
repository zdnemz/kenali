//! Dashboard: read-only status badges mirroring the shared tile vocabulary.

use crate::strings as s;
use crate::window::App;

/// Render the dashboard from the last status snapshot. No comparison values.
pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    ui.heading(s::HDR_DASHBOARD);
    ui.separator();

    if let Some(notice) = app.notice.as_deref() {
        ui.label(notice);
    }

    let connected = app.connected;
    let status = app.status.as_ref();

    egui::Grid::new("dashboard-badges")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            badge(ui, s::LBL_SERVICE, if connected { s::LBL_ON } else { s::LBL_OFF }, Some(connected));

            let camera_ok = status.map(|st| st.camera_ok).unwrap_or(false);
            badge(ui, s::LBL_CAMERA, on_off(camera_ok), Some(camera_ok && connected));

            let count = status.map(|st| st.template_count).unwrap_or(0);
            let enrolled = count > 0;
            badge(ui, s::LBL_ENROLMENT, if enrolled { s::LBL_ON } else { s::LBL_OFF }, Some(enrolled));

            let face_state = match status {
                None => s::FACE_UNLOCK_UNAVAILABLE,
                Some(st) if !st.enabled || st.face_paused => s::FACE_UNLOCK_PAUSED,
                Some(_) => s::FACE_UNLOCK_READY,
            };
            let face_ok = status.map(|st| st.enabled && !st.face_paused).unwrap_or(false);
            badge(ui, "Face unlock", face_state, Some(face_ok && connected));

            let kill = status.map(|st| st.kill_switch).unwrap_or(false);
            badge(ui, s::LBL_KILL_SWITCH, on_off(kill), None);
            badge(
                ui,
                s::LBL_SID,
                status
                    .and_then(|st| st.enrolled_sid.as_ref())
                    .map(|sid| sid.as_str())
                    .unwrap_or(s::LBL_UNKNOWN),
                None,
            );
        });

    ui.separator();
    ui.label(s::LBL_KILL_SWITCH_HINT);
    if let Some(st) = status {
        if let Some(note) = st.pause_note.as_deref() {
            ui.label(note);
        }
    } else {
        ui.label(s::MSG_NO_STATUS);
    }
}

fn on_off(value: bool) -> &'static str {
    if value {
        s::LBL_ON
    } else {
        s::LBL_OFF
    }
}

fn badge(ui: &mut egui::Ui, label: &str, value: &str, ok: Option<bool>) {
    let text = match ok {
        Some(true) => egui::RichText::new(value).color(egui::Color32::from_rgb(120, 200, 120)),
        Some(false) => egui::RichText::new(value).color(egui::Color32::from_rgb(220, 160, 90)),
        None => egui::RichText::new(value),
    };
    ui.label(label);
    ui.label(text);
}
