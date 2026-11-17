//! Test / Bench: an explicitly non-unlocking dry-run, a calibration capture
//! with RAM-only preview, and generic result wording.

use crate::admin_client;
use crate::pipe_worker::PipeCommand;
use crate::strings as s;
use crate::window::App;

#[derive(Default)]
pub(crate) struct TestBenchState {
    pub(crate) running: bool,
}

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    ui.heading(s::HDR_TEST_BENCH);
    ui.separator();

    ui.label(s::LBL_NEVER_UNLOCKS);
    ui.separator();

    if let Some(notice) = app.notice.as_deref() {
        ui.label(notice);
    }

    if !app.test_bench.running {
        ui.horizontal(|ui| {
            if ui.button(s::BTN_RUN_DRY_RUN).clicked() {
                app.test_bench.running = true;
                let _ = admin_client::send(PipeCommand::BeginTest);
            }
            if ui.button(s::BTN_RUN_CALIBRATION).clicked() {
                app.test_bench.running = true;
                app.preview.clear();
                let _ = admin_client::send(PipeCommand::BeginCalibration);
            }
        });
    } else {
        ui.label(s::LOOKING_FOR_YOU);
        if let Some((w, h, rgb)) = app.preview.as_rgb() {
            let image = egui::ColorImage::from_rgb([w as usize, h as usize], rgb);
            let texture = ui
                .ctx()
                .load_texture("kenali-test-preview", image, egui::TextureOptions::NEAREST);
            ui.image((texture.id(), texture.size_vec2()));
        }
        if ui.button(s::BTN_CANCEL).clicked() {
            app.test_bench.running = false;
            app.preview.clear();
            let _ = admin_client::send(PipeCommand::EndPreview);
            let _ = admin_client::send(PipeCommand::CancelEnrolment);
        }
    }
}

/// Called by the window when a test result arrives. Only generic wording is
/// rendered; the service's raw detail field is intentionally not displayed.
pub(crate) fn on_result(app: &mut App, _detail: String) {
    app.test_bench.running = false;
    app.preview.clear();
    app.notice = Some(s::MSG_DRY_RUN_DONE.to_owned());
}
