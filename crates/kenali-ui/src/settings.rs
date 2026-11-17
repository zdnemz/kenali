//! Settings: typed config values from the service. Only tightening changes are
//! submitted; PAD calibration fields are shown read-only.

use kenali_core::config::{ConfigKey, ConfigValue};

use crate::admin_client;
use crate::pipe_worker::PipeCommand;
use crate::strings as s;
use crate::window::App;

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    ui.heading(s::HDR_SETTINGS);
    ui.separator();

    if ui.button(s::BTN_THEME).clicked() {
        let _ = app.theme.toggle(ui.ctx());
    }
    ui.separator();

    let entries: Vec<(ConfigKey, ConfigValue)> = match app.config.as_ref() {
        Some(config) => config.entries.clone(),
        None => {
            ui.label(s::MSG_NO_CONFIG);
            return;
        }
    };

    for (key, value) in entries {
        let read_only = matches!(key, ConfigKey::PadEnabled | ConfigKey::PadThreshold);
        ui.horizontal(|ui| {
            ui.label(format!("{key:?}"));
            let mut next = value;
            let changed = value_ui(ui, value, &mut next);
            if read_only {
                ui.label(s::LBL_READ_ONLY);
            }
            if !read_only && changed {
                let _ = admin_client::send(PipeCommand::SetConfig { key, value: next });
                if let Some(current) = app.config.as_mut() {
                    let _ = current.set(key, next);
                }
            }
        });
    }
    ui.separator();
    ui.label(s::LBL_PAD_HINT);
}

/// Render one typed value. Returns true when the operator changed it.
fn value_ui(ui: &mut egui::Ui, value: ConfigValue, next: &mut ConfigValue) -> bool {
    match value {
        ConfigValue::Bool(current) => {
            let mut edited = current;
            let response = ui.checkbox(&mut edited, "");
            *next = ConfigValue::Bool(edited);
            response.changed()
        }
        ConfigValue::U8(current) => {
            let mut edited = current;
            let response = ui.add(egui::DragValue::new(&mut edited).range(1..=u8::MAX));
            *next = ConfigValue::U8(edited);
            response.changed()
        }
        ConfigValue::U32(current) => {
            let mut edited = current;
            let response = ui.add(egui::DragValue::new(&mut edited).range(60..=86_400));
            *next = ConfigValue::U32(edited);
            response.changed()
        }
        ConfigValue::F32(current) => {
            let mut edited = current;
            let response = ui.add(
                egui::DragValue::new(&mut edited)
                    .range(0.0..=20.0)
                    .speed(0.5),
            );
            *next = ConfigValue::F32(edited);
            response.changed()
        }
        _ => {
            ui.label("—");
            false
        }
    }
}
