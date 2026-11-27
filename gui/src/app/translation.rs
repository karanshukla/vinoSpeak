//! Translating before reading: Settings' row that downloads or deletes the
//! translation model, and the player's Translate picker, which until then is
//! a button that leads to that row. Nothing is downloaded or loaded until asked for:
//! the model loads in the worker at the first translated reading.

use std::time::Duration;

use eframe::egui::{self, RichText};
use vino_ui::a11y;
use vino_ui::theme;
use vino_ui::widgets::{self, Button, ChipKind, Row, Tone};

use super::{App, Reply};
use crate::model_cache;
use crate::translation::{self, Target};

/// What the row is asking to be confirmed, or doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Step {
    ConfirmDownload,
    Downloading,
    ConfirmDelete,
    Deleting,
}

/// How often a download's progress is read off the disk.
const PROGRESS_EVERY: Duration = Duration::from_millis(500);
/// What the picker calls reading the file as it is.
pub(super) const UNTRANSLATED: &str = "Don't translate";

#[derive(Default)]
pub(super) struct Translation {
    pub(super) downloaded: bool,
    /// vinoWhisper uses the same copy, so it isn't deleted from here.
    pub(super) shared: bool,
    /// Bytes in the translation folder, read when Settings shows and while downloading.
    pub(super) bytes: u64,
    pub(super) step: Option<Step>,
    pub(super) error: Option<String>,
    /// What vinospeak said when it last finished.
    pub(super) said: Option<String>,
}

impl Translation {
    /// The line under the row's title.
    pub(super) fn help(&self) -> String {
        let size = model_cache::size(translation::DOWNLOAD_BYTES);
        if let Some(e) = &self.error {
            return format!("Failed: {e}");
        }
        match self.step {
            Some(Step::ConfirmDownload) => {
                format!("Downloads {size}. Only loads when you translate.")
            }
            Some(Step::Downloading) => format!(
                "{} of {size}",
                model_cache::size(self.bytes.min(translation::DOWNLOAD_BYTES))
            ),
            Some(Step::ConfirmDelete) => format!(
                "Deletes {}. You can download it again later.",
                model_cache::size(self.bytes)
            ),
            Some(Step::Deleting) => String::new(),
            None if self.shared => format!(
                "{} on disk · shared with vinoWhisper (delete it there).",
                model_cache::size(self.bytes)
            ),
            None if self.downloaded => format!("{} on disk.", model_cache::size(self.bytes)),
            None => format!(
                "Not downloaded. {} ({size}) translates files locally before reading.",
                translation::LABEL
            ),
        }
    }
}

impl App {
    pub(super) fn refresh_translation(&mut self) {
        let dir = self.translation_dir.as_deref();
        self.translation.downloaded = dir.is_some_and(translation::downloaded);
        self.translation.shared =
            self.translation.downloaded && dir.is_some_and(translation::used_by_vinowhisper);
        self.translation.bytes = dir.map_or(0, model_cache::disk_usage);
        if !self.translation.downloaded {
            self.translate_to = None;
        }
    }

    /// Settings' Translation row.
    pub(super) fn translation_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        if self.translation.step == Some(Step::Downloading) {
            self.translation.bytes = self
                .translation_dir
                .as_deref()
                .map_or(0, model_cache::disk_usage);
            ui.ctx().request_repaint_after(PROGRESS_EVERY);
        }
        let help = self.translation.help();
        let mut row = Row::new("Translation model").help(&help);
        if self.translation.step == Some(Step::ConfirmDelete) {
            row = row.tint(p.error_tint).tone(Tone::Error);
        } else if self.translation.error.is_some() {
            row = row.tone(Tone::Error);
        }
        let reading = self.reading.is_some();
        let mut start = None;
        widgets::group_label(ui, "Translation");
        widgets::group(ui, |ui| {
            row.show(ui, |ui, _| match self.translation.step {
                Some(Step::Downloading) => {
                    a11y::live(&widgets::chip(ui, ChipKind::Neutral, "Downloading…"));
                }
                Some(Step::Deleting) => {
                    a11y::live(&widgets::chip(ui, ChipKind::Neutral, "Deleting…"));
                }
                Some(Step::ConfirmDownload) => {
                    if ui.add(Button::primary("Download").small()).clicked() {
                        start = Some(Step::Downloading);
                    }
                    if ui.add(Button::ghost("Cancel").small()).clicked() {
                        self.translation.step = None;
                    }
                }
                Some(Step::ConfirmDelete) => {
                    if ui
                        .add(Button::destructive("Delete model").small())
                        .clicked()
                    {
                        start = Some(Step::Deleting);
                    }
                    if ui.add(Button::ghost("Cancel").small()).clicked() {
                        self.translation.step = None;
                    }
                }
                None if self.translation.shared => {}
                None if self.translation.downloaded => {
                    let shown = ui.add_enabled(!reading, Button::secondary("Delete…"));
                    ui.ctx().accesskit_node_builder(shown.id, |node| {
                        node.set_label("Delete translation model");
                    });
                    if shown.clicked() {
                        self.translation.step = Some(Step::ConfirmDelete);
                        self.translation.error = None;
                    }
                }
                None => {
                    if ui.add(Button::tonal("Download…")).clicked() {
                        self.translation.step = Some(Step::ConfirmDownload);
                        self.translation.error = None;
                    }
                }
            });
        });
        if let Some(said) = &self.translation.said {
            ui.add_space(8.0);
            a11y::live(&widgets::small(ui, said));
        }
        if let Some(step) = start {
            self.run_translation_step(ui.ctx(), step);
        }
    }

    /// Download or delete through vinospeak, off the window's thread. A
    /// delete stops the worker first, which may hold the model loaded.
    fn run_translation_step(&mut self, ctx: &egui::Context, step: Step) {
        self.translation.step = Some(step);
        self.translation.said = None;
        let worker = if step == Step::Deleting {
            self.worker.take()
        } else {
            None
        };
        let (tx, ctx, vinospeak) = (self.tx.clone(), ctx.clone(), self.vinospeak.clone());
        std::thread::spawn(move || {
            drop(worker);
            let said = if step == Step::Deleting {
                translation::delete(&vinospeak)
            } else {
                translation::download(&vinospeak)
            };
            let _ = tx.send(Reply::Translation(said));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_translation(&mut self, said: anyhow::Result<String>) {
        self.translation.step = None;
        match said {
            Ok(said) => {
                self.translation.said = Some(said);
                self.translation.error = None;
            }
            Err(e) => self.translation.error = Some(format!("{e:#}")),
        }
        self.refresh_translation();
    }

    /// The player's Translate picker, once the model is downloaded; until then
    /// a button that opens Settings at its download (pinned by
    /// `app::tests::translate_before_its_download_leads_to_the_download`).
    /// Picking a language picks a voice of it, unless the voice already speaks it
    /// (pinned by `app::tests::picking_a_language_picks_a_voice_that_speaks_it`).
    pub(super) fn translate_picker(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        if !self.translation.downloaded {
            let offered = ui
                .add(Button::secondary("Translate…").min_width(ui.available_width()))
                .on_hover_text(format!(
                    "Download translation model ({}) in Settings",
                    model_cache::size(translation::DOWNLOAD_BYTES)
                ));
            if offered.clicked() {
                self.open_settings();
                self.translation.step = Some(Step::ConfirmDownload);
                self.translation.error = None;
            }
            return;
        }
        let chosen = self.translate_to;
        let label = chosen.map_or(UNTRANSLATED, |t| t.name).to_owned();
        {
            let shown = widgets::combo(ui, "translate", label, f32::INFINITY).show_ui(ui, |ui| {
                ui.set_min_width(220.0);
                ui.selectable_value(&mut self.translate_to, None, UNTRANSLATED);
                ui.add_space(4.0);
                ui.label(
                    RichText::new("TRANSLATE INTO")
                        .size(11.0)
                        .family(theme::semibold())
                        .color(p.muted),
                );
                for target in translation::TARGETS {
                    ui.selectable_value(&mut self.translate_to, Some(*target), target.name);
                }
            });
            ui.ctx()
                .accesskit_node_builder(shown.response.id, |node| node.set_label("Translate"));
            shown
                .response
                .on_hover_text(format!("Translate locally with {}", translation::LABEL));
        }
        if self.translate_to != chosen
            && let Some(target) = self.translate_to
        {
            self.voice_for(target);
        }
    }

    pub(super) fn voice_for(&mut self, target: Target) {
        if self
            .blend
            .as_ref()
            .is_some_and(|b| b.language != target.language)
        {
            self.blend = None;
        }
        if self.voice.language == target.language {
            return;
        }
        if let Some(voice) = self
            .voices
            .iter()
            .find(|v| v.id == target.voice)
            .or_else(|| self.voices.iter().find(|v| v.language == target.language))
        {
            self.voice = voice.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_says_what_a_download_costs_before_it_starts() {
        let mut t = Translation::default();
        assert_eq!(
            t.help(),
            "Not downloaded. SmolLM3-3B (1.7 GB) translates files locally before reading."
        );
        t.step = Some(Step::ConfirmDownload);
        assert_eq!(t.help(), "Downloads 1.7 GB. Only loads when you translate.");
    }

    #[test]
    fn a_download_counts_up_to_its_size_and_no_further() {
        let mut t = Translation {
            step: Some(Step::Downloading),
            bytes: 512_000_000,
            ..Translation::default()
        };
        assert_eq!(t.help(), "512 MB of 1.7 GB");
        t.bytes = 2_000_000_000;
        assert_eq!(t.help(), "1.7 GB of 1.7 GB");
    }

    #[test]
    fn downloaded_the_row_says_its_size_and_where_to_turn_it_on() {
        let t = Translation {
            downloaded: true,
            bytes: 1_702_000_000,
            ..Translation::default()
        };
        assert_eq!(t.help(), "1.7 GB on disk.");
    }

    #[test]
    fn a_failure_is_said_before_anything_else() {
        let t = Translation {
            step: Some(Step::ConfirmDelete),
            error: Some("no network".to_owned()),
            ..Translation::default()
        };
        assert_eq!(t.help(), "Failed: no network");
    }
}
