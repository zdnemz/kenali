//! Single-window GUI shell: left navigation, theme, event drain and one worker.

use std::time::Duration;

use kenali_winapi::ui::UI_WINDOW_TITLE;

use crate::admin_client;
use crate::diagnostics;
use crate::pipe_worker::{spawn_worker, PipeCommand, PipeEvent, WorkerHandle};
use crate::preview::PreviewSlot;
use crate::theme::Theme;
use crate::{dashboard, enrol, security, settings, strings as s, templates, test_bench, AppError};

/// Native window title (also used by the single-instance focus helper).
pub const APP_TITLE: &str = s::APP_TITLE;

/// The seven operator screens, switched from the left navigation panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Enrol,
    Templates,
    Settings,
    Security,
    Diagnostics,
    TestBench,
}

impl Screen {
    pub const ALL: [Screen; 7] = [
        Screen::Dashboard,
        Screen::Enrol,
        Screen::Templates,
        Screen::Settings,
        Screen::Security,
        Screen::Diagnostics,
        Screen::TestBench,
    ];

    fn label(self) -> &'static str {
        match self {
            Screen::Dashboard => s::NAV_DASHBOARD,
            Screen::Enrol => s::NAV_ENROL,
            Screen::Templates => s::NAV_TEMPLATES,
            Screen::Settings => s::NAV_SETTINGS,
            Screen::Security => s::NAV_SECURITY,
            Screen::Diagnostics => s::NAV_DIAGNOSTICS,
            Screen::TestBench => s::NAV_TEST_BENCH,
        }
    }
}

/// All GUI state. Screens render from this and send commands through `admin_client`.
pub struct App {
    pub(crate) screen: Screen,
    pub(crate) theme: Theme,
    pub(crate) worker: WorkerHandle,
    events: crossbeam_channel::Receiver<PipeEvent>,
    pub(crate) connected: bool,
    pub(crate) hello_proto: Option<u16>,
    pub(crate) service_pid: Option<u32>,
    pub(crate) status: Option<kenali_core::types::StatusReport>,
    pub(crate) templates: Vec<kenali_core::types::TemplateMeta>,
    pub(crate) config: Option<kenali_core::config::ConfigReport>,
    pub(crate) preview: PreviewSlot,
    pub(crate) notice: Option<String>,
    pub(crate) diag_bytes: Vec<u8>,
    pub(crate) enrol: enrol::EnrolState,
    pub(crate) templates_ui: templates::TemplatesState,
    pub(crate) security: security::SecurityState,
    pub(crate) diagnostics: diagnostics::DiagnosticsState,
    pub(crate) test_bench: test_bench::TestBenchState,
}

/// Prepare the worker and native window options. `main` calls `run_native`.
pub fn run() -> Result<(eframe::NativeOptions, App), AppError> {
    let (worker, events) = spawn_worker().map_err(|_| AppError::WorkerSpawn)?;
    admin_client::install(worker.sender().clone())?;

    let app = App {
        screen: Screen::Dashboard,
        theme: Theme::default(),
        worker,
        events,
        connected: false,
        hello_proto: None,
        service_pid: None,
        status: None,
        templates: Vec::new(),
        config: None,
        preview: PreviewSlot::new(),
        notice: None,
        diag_bytes: Vec::new(),
        enrol: enrol::EnrolState::default(),
        templates_ui: templates::TemplatesState::default(),
        security: security::SecurityState::default(),
        diagnostics: diagnostics::DiagnosticsState::default(),
        test_bench: test_bench::TestBenchState::default(),
    };

    let mut options = eframe::NativeOptions::default();
    options.viewport = egui::ViewportBuilder::default().with_title(UI_WINDOW_TITLE);
    Ok((options, app))
}

impl App {
    /// Absorb all pending worker events into GUI state.
    fn drain_events(&mut self) {
        let pending: Vec<PipeEvent> = self.events.try_iter().collect();
        for event in pending {
            match event {
                PipeEvent::Hello { proto, svc_pid } => {
                    self.hello_proto = Some(proto);
                    self.service_pid = Some(svc_pid);
                    self.connected = true;
                    self.notice = Some(s::MSG_CONNECTED.to_owned());
                }
                PipeEvent::Status(report) => self.status = Some(report),
                PipeEvent::TemplateList(list) => self.templates = list,
                PipeEvent::Progress {
                    stage,
                    pose,
                    kept,
                    needed,
                } => enrol::progress(self, stage, pose, kept, needed),
                PipeEvent::Config(report) => self.config = Some(report),
                PipeEvent::TestResult(report) => test_bench::on_result(self, report.detail),
                PipeEvent::DiagnosticsChunk {
                    index,
                    total,
                    bytes,
                } => diagnostics::on_chunk(self, index, total, bytes),
                PipeEvent::Preview(frame) => {
                    let _ = self.preview.replace(&frame);
                }
                PipeEvent::Error { .. } => {
                    self.enrol.wrong_password = true;
                    self.preview.clear();
                    self.notice = Some(s::MSG_ERROR.to_owned());
                }
                PipeEvent::Disconnected => {
                    self.connected = false;
                    self.preview.clear();
                    self.notice = Some(s::MSG_DISCONNECTED.to_owned());
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.theme.apply(ctx);
        self.drain_events();

        let busy = self.preview.as_rgb().is_some()
            || self.enrol.active
            || self.test_bench.running
            || self.diagnostics.receiving;
        if busy {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        egui::SidePanel::left("kenali-nav").show(ctx, |ui| {
            ui.heading(s::APP_TITLE);
            ui.separator();
            for screen in Screen::ALL {
                let selected = self.screen == screen;
                if ui.selectable_label(selected, screen.label()).clicked() {
                    self.screen = screen;
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.screen {
            Screen::Dashboard => dashboard::show(ui, self),
            Screen::Enrol => enrol::show(ui, self),
            Screen::Templates => templates::show(ui, self),
            Screen::Settings => settings::show(ui, self),
            Screen::Security => security::show(ui, self),
            Screen::Diagnostics => diagnostics::show(ui, self),
            Screen::TestBench => test_bench::show(ui, self),
        });
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.preview.clear();
        let _ = self.worker.send(PipeCommand::EndPreview);
        let _ = self.worker.send(PipeCommand::Shutdown);
    }
}
