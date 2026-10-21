//! The activation gate: the app does not run until this installation is activated.
//!
//! The owner requires each installation to be activated with a 4–6 word code derived from the
//! machine's hardware identifier (see `pdfcraft-activation`). Until a valid code is entered,
//! `fn ui` renders only this screen — no tabs, no documents, no tools.

use pdfcraft_activation::ActivationError;

use crate::{PdfCraftApp, theme, widgets};

/// The activation state of one run. `activated` is the gate: `fn ui` checks it before
/// rendering anything else.
#[derive(Default)]
pub struct Dialog {
    /// The hardware identifier, read once (it cannot change mid-run).
    machine_id: Option<String>,
    typed: String,
    error: Option<String>,
    /// Set to `true` when a valid activation code is verified. `fn ui` gates on this.
    pub activated: bool,
}

impl Dialog {
    pub fn new() -> Self {
        match pdfcraft_activation::machine_id() {
            Ok(id) => Self { machine_id: Some(id), ..Default::default() },
            // Without a hardware identifier we cannot derive a code: the gate stays shut.
            Err(_) => Self::default(),
        }
    }
}

/// Draws the fullscreen activation panel when the installation is not yet activated.
/// Returns `true` when the main UI should render (i.e. the installation is activated).
pub fn gate(app: &mut PdfCraftApp, ui: &mut egui::Ui) -> bool {
    if app.activation.activated {
        return true;
    }
    let Some(id) = app.activation.machine_id.clone() else {
        // No hardware identifier: show an error but keep the gate shut — we cannot derive
        // a machine code, so there is nothing the user could send to the author.
        let t = theme::Tokens::get(ui.ctx());
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.pasteboard)).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
                ui.add_space(120.0);
                ui.label(egui::RichText::new(tl!("Activate PdfCraft")).font(theme::semibold(20.0)));
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(tl!("Cannot read a hardware identifier on this machine; activation is not possible."))
                        .color(egui::Color32::RED),
                );
            });
        });
        return false;
    };
    // Activated at some earlier run (the stored code matches this machine): open the gate.
    if pdfcraft_activation::status(&id).is_ok() {
        app.activation.activated = true;
        return true;
    }

    let machine_text = pdfcraft_activation::machine_code(&id).render();
    let t = theme::Tokens::get(ui.ctx());
    let mut activate = false;

    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.pasteboard)).show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
            ui.add_space(80.0);
            ui.add(crate::icons::image("badge-check", 36.0, t.accent));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(tl!("Activate PdfCraft")).font(theme::semibold(22.0)));
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(tl!(
                    "Activation is free: it only tells the author that this installation exists. The code stays on this machine until you send it yourself."
                ))
                .color(t.text_muted),
            );
            ui.add_space(20.0);

            // Machine code, inside a bordered card.
            egui::Frame::default()
                .fill(t.field)
                .corner_radius(8.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    ui.set_width(420.0);
                    ui.label(tl!("Your machine code — send it to the author:"));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&machine_text)
                                .font(theme::semibold(16.0))
                                .color(t.accent_text),
                        );
                        if widgets::pill_button(ui, tl!("Copy"), false).clicked() {
                            ui.ctx().copy_text(machine_text.clone());
                        }
                    });
                });

            ui.add_space(16.0);

            // Activation code input, inside a bordered card.
            egui::Frame::default()
                .fill(t.field)
                .corner_radius(8.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    ui.set_width(420.0);
                    ui.label(tl!("Paste the activation code you received:"));
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut app.activation.typed)
                            .hint_text(tl!("four to six words joined by -"))
                            .desired_width(f32::INFINITY),
                    );
                });

            ui.add_space(10.0);
            if let Some(error) = &app.activation.error {
                ui.label(egui::RichText::new(error).color(egui::Color32::RED));
                ui.add_space(6.0);
            }
            ui.add_space(4.0);
            if widgets::pill_button(ui, tl!("Activate"), true).clicked() {
                activate = true;
            }
        });
    });

    if activate {
        app.activation.error = None;
        match pdfcraft_activation::activate(&id, &app.activation.typed) {
            Ok(code) => {
                app.activation.activated = true;
                app.notify(tl!("Activated. Thank you!"));
                log::info!("activation stored ({} words)", code.len());
            }
            Err(ActivationError::Write(path, error)) => {
                // The install directory is root-owned on Linux; the words are safe to quote
                // (they come from the 64-word table), so the exact command is shown.
                let code =
                    pdfcraft_activation::parse(&app.activation.typed).map(|c| c.render()).unwrap_or_else(|_| app.activation.typed.trim().to_owned());
                app.activation.error =
                    Some(crate::i18n::fmt(tl!("Couldn't write {path}: {error}"), &[("path", path.as_str()), ("error", error.as_str())]));
                app.notify(tl!("To store the activation, run this in a terminal and restart:"));
                app.notify(format!(
                    "sudo mkdir -p \"$(dirname '{path}')\" && printf '%s\\n' '{code}' | sudo tee '{path}' >/dev/null",
                    path = path,
                    code = code
                ));
            }
            Err(ActivationError::BadWord { word, .. }) => {
                app.activation.error = Some(crate::i18n::fmt(tl!("That word is not in the table: {word}"), &[("word", &word)]));
            }
            Err(ActivationError::WrongLength { expected, got }) => {
                app.activation.error = Some(crate::i18n::fmt(
                    tl!("A code has {expected} words, this one has {got}."),
                    &[("expected", &expected.to_string()), ("got", &got.to_string())],
                ));
            }
            Err(ActivationError::Mismatch) => {
                app.activation.error = Some(tl!("The activation code does not match this machine.").to_owned());
            }
            Err(e) => app.activation.error = Some(e.to_string()),
        }
    }

    app.activation.activated
}
