//! The downloaded model as the window knows it: how much is on disk, the
//! voices in it, and deleting it through `vinospeak --delete-model`.

use eframe::egui;

use super::{App, Reply};
use crate::{model_cache, voices};

/// The model on Settings' Model row.
#[derive(Default)]
pub(super) struct Model {
    /// Bytes on disk, the model and its compiled copy; None until Settings shows.
    pub(super) bytes: Option<u64>,
    pub(super) confirm: bool,
    pub(super) deleting: bool,
    /// Why the last delete failed.
    pub(super) error: Option<String>,
    /// What vinospeak said when it last deleted the model.
    pub(super) deleted: Option<String>,
}

impl Model {
    /// The line under the Model row's title, for `bytes` on disk.
    pub(super) fn help(&self, bytes: u64, reading: bool) -> String {
        if let Some(e) = &self.error {
            format!("Couldn't delete model: {e}")
        } else if self.confirm {
            format!(
                "Deletes {} (shared with vinoPop). Re-downloads ~160 MB on next read.",
                model_cache::size(bytes)
            )
        } else if bytes == 0 {
            "Not downloaded. First read downloads ~160 MB.".to_owned()
        } else if reading {
            format!(
                "{} on disk. Stop playback before deleting.",
                model_cache::size(bytes)
            )
        } else {
            format!("{} on disk.", model_cache::size(bytes))
        }
    }
}

impl App {
    pub(super) fn refresh_model(&mut self) {
        self.model.bytes = Some(self.cache.as_deref().map_or(0, model_cache::disk_usage));
    }

    /// The voices in the model as it is now: Heart alone once it's deleted,
    /// all of them once it's downloaded again.
    fn refresh_voices(&mut self) {
        self.voices = voices::voices_in(self.cache.as_deref());
        if !self.voices.contains(&self.voice) {
            self.voice = self.voices[0].clone();
        }
        if self
            .blend
            .as_ref()
            .is_some_and(|b| !self.voices.contains(b))
        {
            self.blend = None;
        }
    }

    /// The worker is ready, and its load may have been the first download.
    pub(super) fn model_may_have_been_downloaded(&mut self) {
        self.refresh_model();
        let only_the_fallback_voice = self.voices.len() <= 1;
        if only_the_fallback_voice {
            self.refresh_voices();
        }
    }

    /// Stop the worker, which holds the model open and may be downloading
    /// it, then have vinospeak delete it. The next reading downloads it again.
    pub(super) fn delete_model(&mut self, ctx: &egui::Context) {
        self.model.confirm = false;
        self.model.deleting = true;
        let worker = self.worker.take();
        let (tx, ctx, vinospeak) = (self.tx.clone(), ctx.clone(), self.vinospeak.clone());
        std::thread::spawn(move || {
            drop(worker);
            let _ = tx.send(Reply::Deleted(model_cache::delete(&vinospeak)));
            ctx.request_repaint();
        });
    }

    /// `said` is what `vinospeak --delete-model` printed.
    pub(super) fn on_deleted(&mut self, said: anyhow::Result<String>) {
        self.model.deleting = false;
        match said {
            Ok(said) => {
                self.model.deleted = Some(said);
                self.model.error = None;
            }
            Err(e) => self.model.error = Some(format!("{e:#}")),
        }
        self.refresh_model();
        self.refresh_voices();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON_DISK: u64 = 163_400_000;

    #[test]
    fn the_row_says_what_is_on_disk() {
        let model = Model::default();
        assert_eq!(model.help(ON_DISK, false), "163 MB on disk.");
        assert_eq!(
            model.help(0, false),
            "Not downloaded. First read downloads ~160 MB."
        );
    }

    #[test]
    fn while_reading_the_row_says_to_stop_first() {
        let model = Model::default();
        assert_eq!(
            model.help(ON_DISK, true),
            "163 MB on disk. Stop playback before deleting."
        );
    }

    #[test]
    fn confirming_says_what_a_delete_costs() {
        let model = Model {
            confirm: true,
            ..Model::default()
        };
        assert_eq!(
            model.help(ON_DISK, false),
            "Deletes 163 MB (shared with vinoPop). Re-downloads ~160 MB on next read."
        );
    }

    #[test]
    fn a_failed_delete_is_said_before_anything_else() {
        let model = Model {
            confirm: true,
            error: Some("The model is downloading.".to_owned()),
            ..Model::default()
        };
        assert_eq!(
            model.help(ON_DISK, false),
            "Couldn't delete model: The model is downloading."
        );
    }
}
