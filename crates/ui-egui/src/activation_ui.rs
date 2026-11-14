//! The activation gate: the app does not run until this installation is activated.
//!
//! The owner requires each installation to be activated with a 4–6 word code derived from the
//! machine's hardware identifier (see `pdfcraft-activation`). Until a valid code is entered,
//! the shell renders only this screen — no tabs, no documents, no tools.
//!
//! The panel follows the suite's design language (the ZCode design system): a calm, dense,
//! centered column — no decorative fills, brand color only on the primary action, one field
//! per step, errors inline and small.

use egui::Stroke;

use pdfcraft_activation::ActivationError;

use crate::{PdfCraftApp, theme, widgets};

/// The activation state of one run. `activated` is the gate: the shell checks it before
/// rendering anything else.
#[derive(Default)]
pub struct Dialog {
    /// The hardware identifier, read once (it cannot change mid-run).
    machine_id: Option<String>,
    typed: String,
    error: Option<String>,
    /// Set to `true` when a valid activation code is verified. The shell gates on this.
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

/// The two fields' fixed width: a narrow, calm column, not a stretched card.
const FIELD_WIDTH: f32 = 420.0;

/// A centered full-height column painting `fill` behind it, running `content` down a
/// `FIELD_WIDTH`-wide inner column.
fn centered(ui: &mut egui::Ui, fill: egui::Color32, content: impl FnOnce(&mut egui::Ui)) {
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(fill)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - FIELD_WIDTH) / 2.0).max(0.0));
            ui.vertical(|ui| content(ui));
        });
    });
}

/// Draws the fullscreen activation panel when the installation is not yet activated.
/// Returns `true` when the main UI should render (i.e. the installation is activated).
pub fn gate(app: &mut PdfCraftApp, ui: &mut egui::Ui) -> bool {
    if app.activation.activated {
        return true;
    }
    let Some(id) = app.activation.machine_id.clone() else {
        // No hardware identifier: the gate stays shut — there is nothing the user could send.
        let t = theme::Tokens::get(ui.ctx());
        centered(ui, t.pasteboard, |ui| {
            ui.add_space(80.0);
            ui.label(egui::RichText::new(tl!("Activate PdfCraft")).font(theme::semibold(18.0)));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(tl!("Cannot read a hardware identifier on this machine; activation is not possible.")).color(t.text_muted));
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
    let error_ink = egui::Color32::from_rgb(0xC4, 0x3B, 0x38);
    let mut activate = false;

    centered(ui, t.pasteboard, |ui| {
        ui.add_space(48.0);
        ui.add(crate::icons::image("badge-check", 28.0, t.accent));
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tl!("Activate PdfCraft")).font(theme::semibold(18.0)));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tl!(
                "Activation is free: it only tells the author that this installation exists. The code stays on this machine until you send it yourself."
            ))
            .color(t.text_muted)
            .font(theme::regular(13.0)),
        );
        ui.add_space(16.0);

        // Step 1: the machine code to send, in a read-only field with a copy affordance.
        ui.label(egui::RichText::new(tl!("Your machine code — send it to the author:")).font(theme::regular(12.5)));
        ui.add_space(4.0);
        egui::Frame::NONE.fill(t.field).stroke(Stroke::new(1.0, t.border)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 8)).show(
            ui,
            |ui| {
                ui.set_width(FIELD_WIDTH - 20.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&machine_text).font(theme::regular(13.5)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::ghost_button(ui, "copy", tl!("Copy machine code")).clicked() {
                            ui.ctx().copy_text(machine_text.clone());
                        }
                    });
                });
            },
        );
        ui.add_space(14.0);

        // Step 2: the activation code received back.
        ui.label(egui::RichText::new(tl!("Paste the activation code you received:")).font(theme::regular(12.5)));
        ui.add_space(4.0);
        egui::Frame::NONE.fill(t.field).stroke(Stroke::new(1.0, t.border)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 2)).show(
            ui,
            |ui| {
                ui.set_width(FIELD_WIDTH - 20.0);
                ui.add(
                    egui::TextEdit::singleline(&mut app.activation.typed)
                        .hint_text(tl!("four to six words joined by -"))
                        .desired_width(FIELD_WIDTH - 40.0)
                        .frame(egui::Frame::NONE),
                );
            },
        );
        ui.add_space(8.0);
        if let Some(error) = &app.activation.error {
            ui.label(egui::RichText::new(error).color(error_ink).font(theme::regular(12.5)));
            ui.add_space(6.0);
        }
        ui.add_space(4.0);
        if widgets::pill_button(ui, tl!("Activate"), true).clicked() {
            activate = true;
        }
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
