//! Light/dark theme selection. The initial theme follows the Windows system
//! theme reported by egui/winit; the operator may override it from Settings.

/// The two supported visual themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
}

/// Current visual theme plus whether it still tracks the system setting.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub kind: ThemeKind,
    pub follow_system: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            kind: ThemeKind::Dark,
            follow_system: true,
        }
    }
}

impl Theme {
    /// Read the system preference egui received from winit, defaulting to dark.
    pub fn system_kind(ctx: &egui::Context) -> ThemeKind {
        match ctx.system_theme() {
            Some(egui::Theme::Light) => ThemeKind::Light,
            Some(egui::Theme::Dark) | None => ThemeKind::Dark,
        }
    }

    /// Effective theme: the manual choice when set, otherwise the system one.
    pub fn effective(&self, ctx: &egui::Context) -> ThemeKind {
        if self.follow_system {
            Self::system_kind(ctx)
        } else {
            self.kind
        }
    }

    /// Apply the effective theme to this egui context.
    pub fn apply(&self, ctx: &egui::Context) {
        match self.effective(ctx) {
            ThemeKind::Dark => ctx.set_visuals(egui::Visuals::dark()),
            ThemeKind::Light => ctx.set_visuals(egui::Visuals::light()),
        }
    }

    /// Toggle between dark and light, pinning the choice (no longer following
    /// the system). Returns the new effective kind.
    pub fn toggle(&mut self, ctx: &egui::Context) -> ThemeKind {
        self.kind = match self.effective(ctx) {
            ThemeKind::Dark => ThemeKind::Light,
            ThemeKind::Light => ThemeKind::Dark,
        };
        self.follow_system = false;
        self.kind
    }
}
