//! The activation dialog: shown at startup while this installation is not activated.
//!
//! Activation is a registration marker, not a gate: the app stays fully usable without it,
//! "Not now" closes the dialog until the next start, and the machine code stays on this machine
//! until the user sends it to the author themselves (see `pdfcraft-activation`).

use pdfcraft_activation::ActivationError;

use crate::{PdfCraftApp, theme, widgets};

/// The startup activation state of one run.
#[derive(Default)]
pub struct Dialog {
    /// The hardware identifier, read once (it cannot change mid-run). `None` when this machine
    /// has none, in which case asking would be pointless.
    machine_id: Option<String>,
    typed: String,
    error: Option<String>,
    /// "Not now" was pressed (or nothing needs asking): stop showing until the next start.
    pub dismissed: bool,
}

impl Dialog {
    pub fn new() -> Self {
        match pdfcraft_activation::machine_id() {
            Ok(id) => Self { machine_id: Some(id), ..Default::default() },
            // Without a hardware identifier there is nothing to ask about; show() dismisses.
            Err(_) => Self::default(),
        }
    }
}

/// Draws the dialog when there is something to ask.
pub fn show(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if app.activation.dismissed {
        return;
    }
    let Some(id) = app.activation.machine_id.clone() else {
        app.activation.dismissed = true;
        return;
    };
    // Activated at some earlier run (or a moment ago): nothing to ask. A stored code that does
    // not match this machine keeps the dialog open with the reason.
    match pdfcraft_activation::status(&id) {
        Ok(_) => {
            app.activation.dismissed = true;
            return;
        }
        Err(ActivationError::NotActivated) => {}
        Err(e) => app.activation.error = Some(e.to_string()),
    }

    let machine_text = pdfcraft_activation::machine_code(&id).render();
    let t = theme::Tokens::get(ctx);
    let mut close = false;
    let mut activate = false;
    let modal = widgets::show_modal(ctx, "activation", |ui| {
        ui.set_width(480.0);
        ui.horizontal(|ui| {
            ui.add(crate::icons::image("badge-check", 22.0, t.accent));
            ui.label(egui::RichText::new(tl!("Activate PdfCraft")).font(theme::semibold(16.0)));
        });
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(tl!(
                "Activation is free: it only tells the author that this installation exists. The code stays on this machine until you send it yourself."
            ))
            .color(t.text_muted),
        );
        ui.add_space(10.0);
        ui.label(tl!("Your machine code — send it to the author:"));
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(egui::RichText::new(&machine_text).font(theme::semibold(15.0)).color(t.accent_text));
            if widgets::pill_button(ui, tl!("Copy"), false).clicked() {
                ui.ctx().copy_text(machine_text.clone());
            }
        });
        ui.add_space(10.0);
        ui.label(tl!("Paste the activation code you received:"));
        ui.add(
            egui::TextEdit::singleline(&mut app.activation.typed)
                .hint_text(tl!("four to six words joined by -"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        if let Some(error) = &app.activation.error {
            ui.label(egui::RichText::new(error).color(egui::Color32::RED));
        }
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::pill_button(ui, tl!("Activate"), true).clicked() {
                activate = true;
            }
            if widgets::pill_button(ui, tl!("Not now"), false).clicked() {
                close = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if close {
        app.activation.dismissed = true;
    }
    if activate {
        app.activation.error = None;
        match pdfcraft_activation::activate(&id, &app.activation.typed) {
            Ok(code) => {
                app.activation.dismissed = true;
                app.notify(tl!("Activated. Thank you!"));
                log::info!("activation stored ({} words)", code.len());
            }
            Err(ActivationError::Write(path, error)) => {
                // The install directory is root-owned on Linux; the words are safe to quote
                // (they come from the 64-word table), so the exact command is shown.
                let code = pdfcraft_activation::parse(&app.activation.typed)
                    .map(|c| c.render())
                    .unwrap_or_else(|_| app.activation.typed.trim().to_owned());
                app.activation.error = Some(crate::i18n::fmt(
                    &tl!("Couldn't write {path}: {error}"),
                    &[("path", path.as_str()), ("error", error.as_str())],
                ));
                app.notify(tl!("To store the activation, run this in a terminal and restart:"));
                app.notify(format!(
                    "sudo mkdir -p \"$(dirname '{path}')\" && printf '%s\\n' '{code}' | sudo tee '{path}' >/dev/null",
                    path = path,
                    code = code
                ));
            }
            Err(ActivationError::BadWord { word, .. }) => {
                app.activation.error =
                    Some(crate::i18n::fmt(&tl!("That word is not in the table: {word}"), &[("word", &word)]));
            }
            Err(ActivationError::WrongLength { expected, got }) => {
                app.activation.error = Some(crate::i18n::fmt(
                    &tl!("A code has {expected} words, this one has {got}."),
                    &[("expected", &expected.to_string()), ("got", &got.to_string())],
                ));
            }
            Err(ActivationError::Mismatch) => {
                app.activation.error = Some(tl!("The activation code does not match this machine.").to_owned());
            }
            Err(e) => app.activation.error = Some(e.to_string()),
        }
    }
}
