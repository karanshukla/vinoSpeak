//! The About page: this app, vinopilot.ca and the other vinoPilot apps
//! (vino-ui's), its Updates row, and the Model row with Delete… on it.

use std::path::PathBuf;

use eframe::egui;
use vino_ui::a11y;
use vino_ui::theme;
use vino_ui::update::Updater;
use vino_ui::widgets::{self, Button, ChipKind, Row, Tone};

use super::App;

/// About's Updates row. The release installer leaves itself as
/// `~/.local/bin/vinospeak-upgrade`, which the row's button runs; without it
/// (a build from a checkout) the row links to the release instead.
pub(super) fn updater() -> Updater {
    let updater = Updater::new(vino_ui::App::Speak, crate::version::VERSION);
    match std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/bin/vinospeak-upgrade"))
    {
        Some(upgrade) if upgrade.is_file() => {
            updater.with_command([upgrade.to_string_lossy().into_owned()])
        }
        _ => updater,
    }
}

impl App {
    pub(super) fn about(&mut self, ui: &mut egui::Ui) {
        widgets::page_scroll(ui, vino_ui::about::TITLE, widgets::CONTENT_MAX, |ui| {
            let title = widgets::title(ui, vino_ui::about::TITLE);
            a11y::heading(&title, vino_ui::about::TITLE, 2);
            vino_ui::about::page_with_updates(ui, &mut self.updater);
            self.model_row(ui);
        });
    }

    pub(super) fn open_about(&mut self) {
        self.about = true;
        self.model.confirm = false;
        self.refresh_model();
    }

    pub(super) fn toggle_about(&mut self) {
        if self.about {
            self.about = false;
        } else {
            self.open_about();
        }
    }

    /// The downloaded model, its size, and Delete… with its confirmation in place.
    fn model_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let bytes = self.model.bytes.unwrap_or(0);
        let reading = self.reading.is_some();
        if reading {
            self.model.confirm = false;
        }
        let help = self.model.help(bytes, reading);
        let mut row = Row::new("Kokoro voice model").help(&help);
        if self.model.confirm {
            row = row.tint(p.error_tint).tone(Tone::Error);
        } else if self.model.error.is_some() {
            row = row.tone(Tone::Error);
        }
        let mut delete = false;
        widgets::group_label(ui, "Model");
        widgets::group(ui, |ui| {
            row.show(ui, |ui, _| {
                if self.model.deleting {
                    a11y::live(&widgets::chip(ui, ChipKind::Neutral, "Deleting…"));
                } else if self.model.confirm {
                    if ui
                        .add(Button::destructive("Delete the model").small())
                        .clicked()
                    {
                        delete = true;
                    }
                    if ui.add(Button::ghost("Cancel").small()).clicked() {
                        self.model.confirm = false;
                    }
                } else if ui
                    .add_enabled(bytes > 0 && !reading, Button::secondary("Delete…"))
                    .clicked()
                {
                    self.model.confirm = true;
                    self.model.error = None;
                }
            });
        });
        if delete {
            self.delete_model(ui.ctx());
        }
    }
}
