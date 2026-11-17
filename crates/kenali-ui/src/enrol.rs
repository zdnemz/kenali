//! Enrol wizard: prerequisites → password → guided capture → seal → self-test →
//! summary. Preview is rendered inline from RAM and wiped on every exit path.

use kenali_core::types::{EnrolMode, EnrolStage, Pose};

use crate::admin_client;
use crate::password_field::PasswordField;
use crate::pipe_worker::PipeCommand;
use crate::preview::PreviewSlot;
use crate::strings as s;
use crate::window::App;

/// Local state of the enrol/refresh wizard.
pub(crate) struct EnrolState {
    pub(crate) active: bool,
    pub(crate) stage: EnrolStage,
    pub(crate) pose: Pose,
    pub(crate) kept: u8,
    pub(crate) needed: u8,
    pub(crate) wrong_password: bool,
    password: PasswordField,
}

impl Default for EnrolState {
    fn default() -> Self {
        Self {
            active: false,
            stage: EnrolStage::Prerequisites,
            pose: Pose::Straight,
            kept: 0,
            needed: 0,
            wrong_password: false,
            password: PasswordField::new(),
        }
    }
}

/// Begin (or restart) the wizard in the given mode.
pub(crate) fn begin(app: &mut App, mode: EnrolMode) {
    let wizard = &mut app.enrol;
    wizard.active = true;
    wizard.stage = EnrolStage::Prerequisites;
    wizard.pose = Pose::Straight;
    wizard.kept = 0;
    wizard.needed = 0;
    wizard.wrong_password = false;
    app.preview.clear();
    let _ = admin_client::send(PipeCommand::BeginEnrolment { mode });
}

/// Apply a progress event from the service.
pub(crate) fn progress(app: &mut App, stage: EnrolStage, pose: Pose, kept: u8, needed: u8) {
    let wizard = &mut app.enrol;
    wizard.active = true;
    wizard.stage = stage;
    wizard.pose = pose;
    wizard.kept = kept;
    wizard.needed = needed;
}

/// Cancel: wipe the RAM preview and ask the service to abort.
pub(crate) fn cancel(app: &mut App) {
    app.preview.clear();
    app.enrol.active = false;
    app.enrol.wrong_password = false;
    let _ = admin_client::send(PipeCommand::CancelEnrolment);
    let _ = admin_client::send(PipeCommand::EndPreview);
    app.notice = Some(s::MSG_PREVIEW_CLEARED.to_owned());
}

pub(crate) fn show(ui: &mut egui::Ui, app: &mut App) {
    ui.heading(s::HDR_ENROL);
    ui.separator();

    if !app.enrol.active {
        ui.label(s::LBL_PREREQUISITES);
        if ui.button(s::BTN_START).clicked() {
            begin(app, EnrolMode::New);
        }
        return;
    }

    ui.label(stage_caption(app.enrol.stage));
    ui.separator();

    match app.enrol.stage {
        EnrolStage::Prerequisites => {
            ui.label(s::LBL_PREREQUISITES);
            ui.label(if app.connected {
                s::MSG_CONNECTED
            } else {
                s::MSG_DISCONNECTED
            });
            if ui.button(s::BTN_CONTINUE).clicked() {
                app.enrol.stage = EnrolStage::Password;
            }
            cancel_button(ui, app);
        }
        EnrolStage::Password => {
            ui.label(s::USE_YOUR_PASSWORD_OR_PIN);
            app.enrol.password.ui(ui, s::LBL_PASSWORD);
            if app.enrol.wrong_password {
                ui.label(s::MSG_ERROR);
            }
            ui.horizontal(|ui| {
                let ready = !app.enrol.password.is_empty();
                if ui.add_enabled(ready, egui::Button::new(s::BTN_CONTINUE)).clicked() {
                    let password = app.enrol.password.submit_password();
                    app.enrol.wrong_password = false;
                    let _ = admin_client::send(PipeCommand::EnrolPassword(password));
                    let _ = admin_client::send(PipeCommand::BeginPreview);
                    app.enrol.stage = EnrolStage::Capture;
                }
                cancel_button(ui, app);
            });
        }
        EnrolStage::Capture => {
            preview_ui(ui, &app.preview);
            ui.label(s::pose_guidance(app.enrol.pose));
            ui.label(format!(
                "{}: {}/{}",
                s::LBL_SAMPLES, app.enrol.kept, app.enrol.needed
            ));
            ui.horizontal(|ui| {
                if ui.button(s::BTN_SUBMIT_SAMPLE).clicked() {
                    let _ = admin_client::send(PipeCommand::SubmitSample);
                }
                if ui.button(s::BTN_FINISH).clicked() {
                    let _ = admin_client::send(PipeCommand::FinishEnrolment);
                    app.enrol.stage = EnrolStage::Seal;
                    app.preview.clear();
                }
                cancel_button(ui, app);
            });
        }
        EnrolStage::Seal => {
            ui.label(s::BLINK_TWICE);
            cancel_button(ui, app);
        }
        EnrolStage::SelfTest => {
            ui.label(s::LOOKING_FOR_YOU);
            cancel_button(ui, app);
        }
        EnrolStage::Summary => {
            ui.label(s::WELCOME);
            let count = app.status.as_ref().map(|st| st.template_count).unwrap_or(0);
            ui.label(format!("{}: {}", s::LBL_ENROLMENT, count));
            if ui.button(s::BTN_DONE).clicked() {
                app.enrol.active = false;
                app.preview.clear();
            }
        }
        _ => {
            ui.label(s::LOOKING_FOR_YOU);
            cancel_button(ui, app);
        }
    }
}

fn preview_ui(ui: &mut egui::Ui, preview: &PreviewSlot) {
    ui.label(s::LBL_PREVIEW);
    if let Some((w, h, rgb)) = preview.as_rgb() {
        let image = egui::ColorImage::from_rgb([w as usize, h as usize], rgb);
        let texture = ui
            .ctx()
            .load_texture("kenali-preview", image, egui::TextureOptions::NEAREST);
        ui.image((texture.id(), texture.size_vec2()));
    }
}

fn cancel_button(ui: &mut egui::Ui, app: &mut App) {
    if ui.button(s::BTN_CANCEL).clicked() {
        cancel(app);
    }
}

fn stage_caption(stage: EnrolStage) -> &'static str {
    match stage {
        EnrolStage::Prerequisites => s::STEP_PREREQUISITES,
        EnrolStage::Password => s::STEP_PASSWORD,
        EnrolStage::Capture => s::STEP_CAPTURE,
        EnrolStage::Seal => s::STEP_SEAL,
        EnrolStage::SelfTest => s::STEP_SELF_TEST,
        EnrolStage::Summary => s::STEP_SUMMARY,
        _ => s::STEP_CAPTURE,
    }
}
