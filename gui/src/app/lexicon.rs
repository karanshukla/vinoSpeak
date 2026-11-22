//! Settings' Pronunciations: lexicon.toml's words, edited in place and saved
//! for the worker to read again before the next reading.

use std::path::PathBuf;

use eframe::egui;
use vino_ui::a11y;
use vino_ui::widgets::{self, Button, Row, Tone};

use super::App;
use crate::lexicon::{self, Entry};

/// lexicon.toml as Settings shows it.
#[derive(Default)]
pub(super) struct Pronunciations {
    /// None when there is no home to keep it in.
    pub(super) path: Option<PathBuf>,
    pub(super) entries: Vec<Entry>,
    /// As it is on disk, to tell whether there is anything to save.
    saved: Vec<Entry>,
    /// Why the file on disk can't be edited here.
    error: Option<String>,
    /// How the last save went.
    note: Option<String>,
}

const WORD_WIDTH: f32 = 150.0;
const SAID_WIDTH: f32 = 200.0;

impl Pronunciations {
    pub(super) fn at(path: Option<PathBuf>) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }
}

impl App {
    /// lexicon.toml as it is now, unless there are changes not yet saved.
    pub(super) fn load_lexicon(&mut self) {
        let words = &mut self.lexicon;
        if words.entries != words.saved {
            return;
        }
        let Some(path) = &words.path else {
            return;
        };
        match lexicon::load(path) {
            Ok(entries) => {
                words.saved.clone_from(&entries);
                words.entries = entries;
                words.error = None;
            }
            Err(e) => words.error = Some(e),
        }
    }

    pub(super) fn lexicon_rows(&mut self, ui: &mut egui::Ui) {
        widgets::group_label(ui, "Pronunciations");
        widgets::group_help(
            ui,
            "Custom pronunciation respellings for words Kokoro mispronounces (for example, \
             kubectl → cube control). Lowercase entries match any capitalization.",
        );
        if let Some(error) = self.lexicon.error.clone() {
            let help = format!(
                "Cannot edit in Settings: {error} Fix the file manually, then reopen Settings."
            );
            widgets::group(ui, |ui| {
                Row::new("lexicon.toml")
                    .help(&help)
                    .tone(Tone::Error)
                    .show(ui, |_, _| {});
            });
            return;
        }
        let mut remove = None;
        let mut add = false;
        let count = self.lexicon.entries.len();
        let help = match count {
            0 => "No custom pronunciations yet.".to_owned(),
            1 => "1 word".to_owned(),
            n => format!("{n} words"),
        };
        widgets::group(ui, |ui| {
            Row::new("Words").help(&help).show(ui, |ui, _| {
                add = ui.add(Button::secondary("Add word").small()).clicked();
            });
            if count == 0 {
                return;
            }
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    for (n, (word, said)) in self.lexicon.entries.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let shown = ui.add(widgets::field(word, "Word", WORD_WIDTH));
                            let name = format!("Word {}", n + 1);
                            ui.ctx()
                                .accesskit_node_builder(shown.id, |node| node.set_label(name));
                            let arrow = ui.label("as");
                            a11y::decorative(ui, arrow.id);
                            let shown = ui.add(widgets::field(said, "Spoken as", SAID_WIDTH));
                            let name = format!("Pronounce word {} as", n + 1);
                            ui.ctx()
                                .accesskit_node_builder(shown.id, |node| node.set_label(name));
                            if ui.add(Button::ghost("Remove").small()).clicked() {
                                remove = Some(n);
                            }
                        });
                    }
                });
        });
        if add {
            self.lexicon.entries.push(Entry::default());
        }
        if self.lexicon.entries != self.lexicon.saved {
            ui.add_space(8.0);
            if ui
                .add(Button::primary("Save pronunciations").small())
                .clicked()
            {
                self.save_lexicon();
            }
        }
        if let Some(n) = remove {
            self.lexicon.entries.remove(n);
        }
        if let Some(said) = &self.lexicon.note {
            ui.add_space(8.0);
            a11y::live(&widgets::small(ui, said));
        }
    }

    /// Write lexicon.toml and have the worker read it again (pinned by
    /// `app::tests::saved_pronunciations_are_written_and_the_worker_reads_them_again`).
    pub(super) fn save_lexicon(&mut self) {
        let Some(path) = self.lexicon.path.clone() else {
            return;
        };
        let kept: Vec<Entry> = self
            .lexicon
            .entries
            .iter()
            .filter(|(word, _)| !word.trim().is_empty())
            .map(|(w, s)| (w.trim().to_owned(), s.trim().to_owned()))
            .collect();
        match lexicon::save(&path, &kept) {
            Ok(()) => {
                self.lexicon.note = Some(format!("Saved to {}", path.display()));
                self.lexicon.saved.clone_from(&kept);
                self.lexicon.entries = kept;
                if let Some(worker) = &mut self.worker
                    && worker.reload_lexicon().is_err()
                {
                    self.worker = None;
                }
            }
            Err(e) => self.lexicon.note = Some(format!("Could not save: {e}")),
        }
    }
}
