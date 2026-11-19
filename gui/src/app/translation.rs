//! Translating before reading: About's row that downloads or deletes the
//! translation model, and the player's Translate picker, which shows only
//! once it is downloaded. Nothing is downloaded or loaded until asked for:
//! the model loads in the worker at the first translated reading.

use std::time::Duration;

use eframe::egui::{self, RichText};
use vino_ui::a11y;
use vino_ui::icons::{self, Icon};
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
    /// Bytes in the translation folder, read when About shows and while downloading.
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
            return format!("Couldn't do it: {e}");
        }
        match self.step {
            Some(Step::ConfirmDownload) => format!(
                "Downloads {size} into your home folder. It loads only when you read with \
                 Translate on, and lets go of its memory after five minutes unused."
            ),
            Some(Step::Downloading) => format!(
                "Downloading… {} of {size}",
                model_cache::size(self.bytes.min(translation::DOWNLOAD_BYTES))
            ),
            Some(Step::ConfirmDelete) => format!(
                "Deletes {}. Translate goes away until you download it again.",
                model_cache::size(self.bytes)
            ),
            Some(Step::Deleting) => "Deleting…".to_owned(),
            None if self.downloaded => format!(
                "{} on disk. Pick a language under Translate to have a file read in it.",
                model_cache::size(self.bytes)
            ),
            None => format!(
                "Off. Download {} ({size}) to translate a file into Spanish, French, \
                 Italian, Portuguese or English before it is read.",
                translation::LABEL
            ),
        }
    }
}

impl App {
    pub(super) fn refresh_translation(&mut self) {
        let dir = self.translation_dir.as_deref();
        self.translation.downloaded = dir.is_some_and(translation::downloaded);
        self.translation.bytes = dir.map_or(0, model_cache::disk_usage);
        if !self.translation.downloaded {
            self.translate_to = None;
        }
    }

    /// About's Translation row, under the Model row.
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
                        .add(Button::destructive("Delete the translation model").small())
                        .clicked()
                    {
                        start = Some(Step::Deleting);
                    }
                    if ui.add(Button::ghost("Cancel").small()).clicked() {
                        self.translation.step = None;
                    }
                }
                None if self.translation.downloaded => {
                    let button = Button::secondary("Delete translation…");
                    if ui.add_enabled(!reading, button).clicked() {
                        self.translation.step = Some(Step::ConfirmDelete);
                        self.translation.error = None;
                    }
                }
                None => {
                    if ui.add(Button::secondary("Download…")).clicked() {
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

    /// The player's Translate picker, once the model is downloaded. Picking
    /// a language picks a voice of it, unless the voice already speaks it
    /// (pinned by `app::tests::picking_a_language_picks_a_voice_that_speaks_it`).
    pub(super) fn translate_picker(&mut self, ui: &mut egui::Ui) {
        if !self.translation.downloaded {
            return;
        }
        let p = theme::palette(ui.ctx());
        let chosen = self.translate_to;
        let label = chosen.map_or(UNTRANSLATED, |t| t.name).to_owned();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::icon(ui, Icon::Captions, 14.0, p.muted);
            let shown = widgets::combo(ui, "translate", label, 190.0).show_ui(ui, |ui| {
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
            shown.response.on_hover_text(format!(
                "Translated by {} on this machine before it is read",
                translation::LABEL
            ));
        });
        if self.translate_to != chosen
            && let Some(target) = self.translate_to
        {
            self.voice_for(target);
        }
    }

    fn voice_for(&mut self, target: Target) {
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
        assert!(t.help().starts_with("Off. Download SmolLM3-3B (1.7 GB)"));
        t.step = Some(Step::ConfirmDownload);
        assert!(
            t.help()
                .starts_with("Downloads 1.7 GB into your home folder.")
        );
    }

    #[test]
    fn a_download_counts_up_to_its_size_and_no_further() {
        let mut t = Translation {
            step: Some(Step::Downloading),
            bytes: 512_000_000,
            ..Translation::default()
        };
        assert_eq!(t.help(), "Downloading… 512 MB of 1.7 GB");
        t.bytes = 2_000_000_000;
        assert_eq!(t.help(), "Downloading… 1.7 GB of 1.7 GB");
    }

    #[test]
    fn downloaded_the_row_says_its_size_and_where_to_turn_it_on() {
        let t = Translation {
            downloaded: true,
            bytes: 1_702_000_000,
            ..Translation::default()
        };
        assert_eq!(
            t.help(),
            "1.7 GB on disk. Pick a language under Translate to have a file read in it."
        );
    }

    #[test]
    fn a_failure_is_said_before_anything_else() {
        let t = Translation {
            step: Some(Step::ConfirmDelete),
            error: Some("no network".to_owned()),
            ..Translation::default()
        };
        assert_eq!(t.help(), "Couldn't do it: no network");
    }
}
