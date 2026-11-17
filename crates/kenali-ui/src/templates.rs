//! Templates: list stored metadata, add via the capture wizard, delete behind
//! an explicit confirmation. No embedding bytes ever reach this screen.

use kenali_core::types::{EnrolMode, TemplateId};

use crate::admin_client;
use crate::enrol;
use crate::pipe_worker::PipeCommand;
use crate::strings as s;
use crate::window::{App, Screen};

/// Local confirmation state for the destructive action.
#[derive(Default)]
pub(crate) struct TemplatesState {
    confirm: Option<TemplateId>,
}

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    ui.heading(s::HDR_TEMPLATES);
    ui.separator();

    if ui.button(s::BTN_ADD_TEMPLATE).clicked() {
        enrol::begin(app, EnrolMode::AddTemplate);
        app.screen = Screen::Enrol;
    }
    ui.separator();

    let templates = app.templates.clone();
    if templates.is_empty() {
        ui.label(s::MSG_NO_TEMPLATES);
        return;
    }

    for meta in templates {
        ui.group(|ui| {
            ui.label(format!("{}: {}", s::LBL_POSE, s::pose_guidance(meta.pose)));
            ui.label(format!("{}: {}", s::LBL_LIGHTING, meta.lighting));
            ui.label(format!("{}: {}", s::LBL_GLASSES, on_off(meta.glasses)));
            ui.label(format!("{}: {}", s::LBL_CAPTURED, meta.captured_at));
            if ui.button(s::BTN_DELETE).clicked() {
                app.templates_ui.confirm = Some(meta.id);
            }
        });
    }

    if let Some(id) = app.templates_ui.confirm {
        ui.separator();
        ui.label(s::MSG_CONFIRM_DELETE);
        ui.horizontal(|ui| {
            if ui.button(s::BTN_CANCEL).clicked() {
                app.templates_ui.confirm = None;
            }
            if ui.button(s::BTN_DELETE).clicked() {
                let _ = admin_client::send(PipeCommand::DeleteTemplate { id });
                app.templates_ui.confirm = None;
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
