//! Masked password entry. The egui backing `String` is converted straight into
//! a typed secret and wiped on submit.
//!
//! R14 (accepted residual risk): the plaintext exists in the egui `String` for
//! the lifetime of the field. We clear that source immediately after submit and
//! never clone, log or persist it; this is the one documented place where a
//! transient plaintext buffer exists in the GUI.

use kenali_core::secret::Password;

/// A masked single-line text field holding a transient password.
#[derive(Default)]
pub struct PasswordField {
    text: String,
}

impl PasswordField {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while the field holds characters (used to enable Continue).
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Render the masked field. `hint` is a generic, score-free label.
    pub fn ui(&mut self, ui: &mut egui::Ui, hint: &str) {
        let _ = ui.add(
            egui::TextEdit::singleline(&mut self.text)
                .password(true)
                .hint_text(hint),
        );
    }

    /// Convert to a typed secret and immediately clear the source buffer.
    pub fn submit_password(&mut self) -> Password {
        submit_password(self)
    }
}

/// Consume the field's characters into a `Password` and wipe the source now.
///
/// R14: this is the accepted exception — the UTF-16 units are moved into the
/// zeroizing secret and the egui `String` is cleared in the same breath. See the
/// module comment; the plaintext is never logged or persisted.
pub fn submit_password(field: &mut PasswordField) -> Password {
    let units: Vec<u16> = field.text.encode_utf16().collect();
    field.text.clear();
    Password::from_wide(units)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn submit_clears_the_source() {
        let mut field = PasswordField::new();
        field.text = "secret".into();
        let password = submit_password(&mut field);
        assert!(field.text.is_empty());
        assert_eq!(password.len(), 6);
    }
}
