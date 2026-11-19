//! Saving the chosen file's speech to an audio file, with subtitles beside
//! it if asked for, through the worker's save beside any reading.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use vino_ui::a11y;
use vino_ui::theme;
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

fn kind(suffix: &str) -> &'static str {
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
    /// Format, Subtitles and Save as…; while saving, how far it is and Cancel.
    pub(super) fn save_row(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let heading = ui.label(
            RichText::new("SAVE AS AUDIO")
                .size(11.0)
                .family(theme::semibold())
                .color(p.muted),
        );
        a11y::decorative(ui, heading.id);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if let Some(saving) = &self.saving {
                let (done, name) = (saving.done, name_of(&saving.output));
                widgets::progress(ui, done, 160.0);
                let said = format!("Saving {name} · {:.0}%", done * 100.0);
                a11y::live(&ui.label(said));
                let cancel = ui.add_enabled(!saving.cancel_sent, Button::ghost("Cancel").small());
                if cancel.clicked() {
                    self.cancel_save();
                }
                return;
            }
            let readable = matches!(self.preview, Some(Ok(_)));
            let label = kind(self.save_format).to_owned();
            let shown = widgets::combo(ui, "save-format", label, 130.0).show_ui(ui, |ui| {
                for (suffix, name) in FORMATS {
                    ui.selectable_value(&mut self.save_format, suffix, name);
                }
            });
            ui.ctx()
                .accesskit_node_builder(shown.response.id, |node| node.set_label("Format"));
            widgets::switch(ui, &mut self.save_subtitles, "Subtitles");
            let shown = ui.label("Subtitles");
            a11y::decorative(ui, shown.id);
            let save = ui.add_enabled(readable, Button::secondary("Save as…"));
            if save.clicked() {
                self.ask_where_to_save(ui.ctx());
            }
        });
        if let Some(said) = &self.save_note {
            ui.add_space(4.0);
            a11y::live(&widgets::small(ui, said));
        }
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
            Err(e) => self.save_note = Some(format!("Couldn't open the save dialog: {e:#}")),
        }
    }

    /// Have the worker save the previewed text into `output` with the
    /// settings chosen now (pinned by
    /// `app::tests::save_as_sends_the_file_the_voice_and_subtitles_and_reports_it_saved`).
    pub(super) fn save_to(&mut self, ctx: &egui::Context, output: PathBuf) {
        if self.model.deleting {
            self.save_note = Some("Deleting the model; save once that's done.".to_owned());
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
                    "Saved {} of speech to {}{beside}",
                    length(*seconds),
                    saving.output.display()
                )
            }
            Event::SaveCancelled { id } if *id == saving.id => format!("Didn't save {name}"),
            Event::Error {
                id: Some(id),
                message,
            } if *id == saving.id => {
                format!("Couldn't save {name}: {message}")
            }
            Event::Exited(message) => {
                self.save_note = Some(format!("Couldn't save {name}: {message}"));
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
