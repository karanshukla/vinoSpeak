//! Saving the chosen file's speech to an audio file, with subtitles beside
//! it if asked for, through the worker's save beside any reading.

use std::path::{Path, PathBuf};

use eframe::egui;
use vino_ui::a11y;
use vino_ui::widgets::{self, Button};

use super::{App, Reply};
use crate::speech::Event;

/// What vinospeak saves, by suffix, as the format picker names them.
pub(super) const FORMATS: [(&str, &str); 5] = [
    ("mp3", "MP3"),
    ("opus", "Opus"),
    ("ogg", "Ogg Vorbis"),
    ("flac", "FLAC"),
    ("wav", "WAV"),
];

/// The suffix of one of `FORMATS`, from a saved choice.
pub(super) fn format(suffix: &str) -> Option<&'static str> {
    FORMATS.iter().find(|(s, _)| *s == suffix).map(|(s, _)| *s)
}

pub(super) fn kind(suffix: &str) -> &'static str {
    FORMATS
        .iter()
        .find(|(s, _)| *s == suffix)
        .map_or("Audio", |(_, k)| *k)
}

/// `path` with `suffix`, added when the dialog's name was typed without it
/// (pinned by `app::save::tests::a_name_typed_without_the_suffix_gets_it`).
pub(super) fn with_suffix(path: PathBuf, suffix: &str) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(suffix));
    if has {
        return path;
    }
    let mut name = path.into_os_string();
    name.push(format!(".{suffix}"));
    PathBuf::from(name)
}

/// The subtitles saved beside `output`.
pub(super) fn subtitles_for(output: &Path) -> PathBuf {
    output.with_extension("srt")
}

/// A save the worker was asked for.
pub(super) struct Saving {
    pub(super) id: u64,
    pub(super) output: PathBuf,
    /// How much of it is synthesized, 0 to 1.
    pub(super) done: f32,
    pub(super) cancel_sent: bool,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Minutes and seconds of speech saved: `1:05`.
fn length(seconds: f32) -> String {
    let secs = seconds.round() as u64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

impl App {
    /// The player's Save as…; while saving, how far it is and Cancel. The
    /// format and subtitles are chosen on Settings.
    pub(super) fn save_cell(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        if let Some(saving) = &self.saving {
            let done = saving.done;
            let cancel_sent = saving.cancel_sent;
            ui.horizontal(|ui| {
                ui.set_min_height(widgets::CONTROL_HEIGHT);
                ui.spacing_mut().item_spacing.x = 8.0;
                let cancel = ui.add_enabled(!cancel_sent, Button::ghost("Cancel").small());
                widgets::progress(ui, done, (ui.available_width() - 4.0).max(40.0));
                if cancel.clicked() {
                    self.cancel_save();
                }
            });
            return;
        }
        let readable = matches!(self.preview, Some(Ok(_)));
        let label = format!("Save as {}…", kind(self.save_format));
        let save = ui
            .add_enabled(readable, Button::secondary(&label).min_width(width))
            .on_hover_text(if self.save_subtitles {
                "Includes .srt subtitles. Change format and subtitle options in Settings."
            } else {
                "Change format and subtitle options in Settings."
            });
        if save.clicked() {
            self.ask_where_to_save(ui.ctx());
        }
    }

    /// Under the player: how far a save is, or how the last one went.
    pub(super) fn save_note(&mut self, ui: &mut egui::Ui) {
        let said = match &self.saving {
            Some(saving) => format!(
                "Saving {} · {:.0}%",
                name_of(&saving.output),
                saving.done * 100.0
            ),
            None => match &self.save_note {
                Some(note) => note.clone(),
                None => return,
            },
        };
        ui.add_space(8.0);
        a11y::live(&widgets::small(ui, &said));
    }

    /// Ask the desktop's save dialog where; it answers as `Reply::SaveTo`.
    fn ask_where_to_save(&self, ctx: &egui::Context) {
        let Some(hit) = &self.selected else {
            return;
        };
        let stem = Path::new(&hit.name)
            .file_stem()
            .map_or_else(|| "speech".to_owned(), |s| s.to_string_lossy().into_owned());
        let name = format!("{stem}.{}", self.save_format);
        let kind = kind(self.save_format);
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Reply::SaveTo(crate::open::save_dialog(&name, kind)));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_save_to(
        &mut self,
        ctx: &egui::Context,
        chosen: anyhow::Result<Option<PathBuf>>,
    ) {
        match chosen {
            Ok(Some(path)) => self.save_to(ctx, with_suffix(path, self.save_format)),
            Ok(None) => {}
            Err(e) => self.save_note = Some(format!("Could not open save dialog: {e:#}")),
        }
    }

    /// Have the worker save the previewed text into `output` with the
    /// settings chosen now (pinned by
    /// `app::tests::save_as_sends_the_file_the_voice_and_subtitles_and_reports_it_saved`).
    pub(super) fn save_to(&mut self, ctx: &egui::Context, output: PathBuf) {
        if self.model.deleting {
            self.save_note = Some("Model deletion in progress. Try again shortly.".to_owned());
            return;
        }
        self.start_worker(ctx);
        let spoken = self.voice_sent();
        let subtitles = self.save_subtitles.then(|| subtitles_for(&output));
        let (Some(Ok(preview)), Some(worker)) = (&self.preview, &mut self.worker) else {
            return;
        };
        let target = self.translate_to.map(|t| t.language);
        match worker.save(
            &preview.text,
            &spoken,
            self.speed,
            target,
            &output,
            subtitles.as_deref(),
        ) {
            Ok(id) => {
                self.save_note = None;
                self.saving = Some(Saving {
                    id,
                    output,
                    done: 0.0,
                    cancel_sent: false,
                });
            }
            Err(e) => {
                self.save_note = Some(format!("{e:#}"));
                self.worker = None;
            }
        }
    }

    fn cancel_save(&mut self) {
        if let (Some(saving), Some(worker)) = (&mut self.saving, &mut self.worker) {
            saving.cancel_sent = true;
            if worker.cancel_save().is_err() {
                self.worker = None;
            }
        }
    }

    /// Apply a worker event about the save under way; true when it was one.
    /// A worker that exits ends the save too, and the reading sees it as well.
    pub(super) fn take_save_event(&mut self, event: &Event) -> bool {
        let Some(saving) = self.saving.as_mut() else {
            return false;
        };
        let name = name_of(&saving.output);
        let note = match event {
            Event::Saving { id, done } if *id == saving.id => {
                saving.done = saving.done.max(*done);
                return true;
            }
            Event::Saved { id, seconds } if *id == saving.id => {
                let beside = if self.save_subtitles {
                    ", with subtitles"
                } else {
                    ""
                };
                format!(
                    "Saved {} of audio to {}{beside}",
                    length(*seconds),
                    saving.output.display()
                )
            }
            Event::SaveCancelled { id } if *id == saving.id => format!("Canceled saving {name}"),
            Event::Error {
                id: Some(id),
                message,
            } if *id == saving.id => {
                format!("Could not save {name}: {message}")
            }
            Event::Exited(message) => {
                self.save_note = Some(format!("Could not save {name}: {message}"));
                self.saving = None;
                return false;
            }
            _ => return false,
        };
        self.save_note = Some(note);
        self.saving = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_typed_without_the_suffix_gets_it() {
        let saved = |name: &str| with_suffix(PathBuf::from(name), "mp3");
        assert_eq!(saved("/a/notes.mp3"), PathBuf::from("/a/notes.mp3"));
        assert_eq!(saved("/a/notes.MP3"), PathBuf::from("/a/notes.MP3"));
        assert_eq!(saved("/a/notes"), PathBuf::from("/a/notes.mp3"));
        assert_eq!(saved("/a/v1.2"), PathBuf::from("/a/v1.2.mp3"));
    }

    #[test]
    fn subtitles_go_beside_the_speech() {
        assert_eq!(
            subtitles_for(Path::new("/a/notes.mp3")),
            PathBuf::from("/a/notes.srt")
        );
    }

    #[test]
    fn a_saved_length_is_minutes_and_seconds() {
        assert_eq!(length(4.6), "0:05");
        assert_eq!(length(125.0), "2:05");
    }
}
