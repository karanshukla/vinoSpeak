//! Reading aloud: the `vinospeak --serve` worker, the one reading it has
//! under way, and what its events do to the window.

use std::time::{Duration, Instant};

use eframe::egui;

use super::App;
use crate::speech::{self, Event, Reading, Translated, Worker};

/// How a reading ended.
#[derive(Debug, PartialEq)]
enum Ended {
    Finished,
    Failed(String),
}

/// The status line for a reading that ended; `file` is None when the worker
/// reported with no reading under way.
fn ended_status(file: Option<String>, ended: Ended) -> String {
    match (file, ended) {
        (Some(file), Ended::Finished) => format!("Finished {file}"),
        (Some(file), Ended::Failed(why)) => format!("Couldn't read {file}: {why}"),
        (None, Ended::Failed(why)) => format!("vinospeak: {why}"),
        (None, Ended::Finished) => String::new(),
    }
}

fn file_name(reading: &Reading) -> String {
    reading
        .file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn debug_first_audio(reading: &Reading, ms_in_vinospeak: u64) {
    if std::env::var_os("VINOSPEAK_GUI_DEBUG").is_some() {
        let since = reading.started.elapsed().as_millis();
        eprintln!("first audio {since} ms after Read aloud ({ms_in_vinospeak} ms in vinospeak)");
    }
}

impl App {
    /// The worker, loading the model while the user looks at the preview.
    pub(super) fn start_worker(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() || self.model.deleting {
            return;
        }
        let ctx = ctx.clone();
        match Worker::start(&self.vinospeak, move || ctx.request_repaint()) {
            Ok(worker) => self.worker = Some(worker),
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    /// Read the previewed text; the worker stops whatever it was reading first.
    pub(super) fn read_aloud(&mut self, ctx: &egui::Context) {
        self.read_from(ctx, 0.0);
    }

    /// Read the previewed text again from the sentence `at` (0 to 1) into it,
    /// with the settings chosen now (pinned by
    /// `app::tests::scrubbing_reads_again_from_where_the_bar_was_let_go`).
    pub(super) fn read_from(&mut self, ctx: &egui::Context, at: f32) {
        if self.model.deleting {
            self.status = "Deleting model — try again shortly.".to_owned();
            return;
        }
        self.start_worker(ctx);
        let volume = self.heard_volume();
        let chosen = self.chosen();
        let spoken = self.voice_sent();
        let (Some(hit), Some(Ok(preview)), Some(worker)) =
            (&self.selected, &self.preview, &mut self.worker)
        else {
            return;
        };
        let target = self.translate_to;
        match worker.speak(
            &preview.text,
            &spoken,
            self.speed,
            self.output.as_deref(),
            volume,
            target.map(|t| t.language),
            at,
        ) {
            Ok(id) => {
                self.translated = target.map(|into| Translated {
                    id,
                    file: hit.path.clone(),
                    part: self.part,
                    into,
                    pieces: Vec::new(),
                });
                self.show_original = false;
                self.status = match target {
                    Some(t) => format!("Translating {} into {}", hit.name, t.name),
                    None => format!("Speaking {}", hit.name),
                };
                self.reading = Some(Reading {
                    id,
                    file: hit.path.clone(),
                    started: Instant::now(),
                    stop_sent: None,
                    translated_into: target,
                    heard: false,
                    done: at,
                    paused_at: None,
                    length: speech::spoken_length(&preview.text, self.speed),
                    chosen,
                });
            }
            Err(e) => {
                self.status = format!("{e:#}");
                self.worker = None;
            }
        }
    }

    /// The voice, blended if it is, as the worker is sent it.
    pub(super) fn voice_sent(&self) -> speech::Spoken {
        speech::Spoken {
            spec: crate::voices::spec(&self.voice, self.blend.as_ref().map(|b| (b, self.mix))),
            language: self.voice.language,
        }
    }

    /// The settings a reading started now is spoken with.
    fn chosen(&self) -> speech::Chosen {
        speech::Chosen {
            voice: self.voice_sent().spec,
            speed: self.speed,
            output: self.output.clone(),
            translate: self.translate_to.map(|t| t.language),
        }
    }

    /// The volume speech plays at: none while muted.
    pub(super) fn heard_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }

    /// The reading under way plays on at the volume or mute just chosen; the
    /// next one starts at it.
    pub(super) fn volume_changed(&mut self) {
        let volume = self.heard_volume();
        if let (Some(_), Some(worker)) = (&self.reading, &mut self.worker)
            && worker.set_volume(volume).is_err()
        {
            self.worker = None;
        }
    }

    /// Pause the reading under way, or resume it; the clock holds while it
    /// is paused (pinned by `app::tests::space_pauses_and_resumes_the_reading`).
    pub(super) fn toggle_pause(&mut self) {
        let (Some(reading), Some(worker)) = (&mut self.reading, &mut self.worker) else {
            return;
        };
        let paused = reading.paused_at.is_none();
        if worker.pause(paused).is_err() {
            self.worker = None;
            return;
        }
        match reading.paused_at.take() {
            Some(at) => reading.started += at.elapsed(),
            None => reading.paused_at = Some(Instant::now()),
        }
    }

    /// A reading whose voice, blend, speed, output or Translate was changed
    /// goes on with the new one from the sentence it was at, paused if it
    /// was, once the pointer is let go (pinned
    /// by `app::tests::a_new_setting_takes_over_the_reading_from_where_it_is`).
    pub(super) fn follow_new_settings(&mut self, ctx: &egui::Context) {
        let Some(reading) = &self.reading else {
            return;
        };
        let now = self.chosen();
        let shown = self
            .selected
            .as_ref()
            .is_some_and(|hit| hit.path == reading.file);
        let dragging = ctx.input(|i| i.pointer.any_down());
        if reading.chosen == now
            || reading.stop_sent.is_some()
            || !shown
            || reading.done >= 1.0
            || dragging
        {
            return;
        }
        let (at, paused) = (reading.done, reading.paused_at.is_some());
        self.read_from(ctx, at);
        if paused {
            self.toggle_pause();
        }
    }

    /// Read again from `at` (0 to 1) into the text; the end stops it.
    pub(super) fn seek(&mut self, ctx: &egui::Context, at: f32) {
        if at < 1.0 {
            self.read_from(ctx, at);
        } else {
            self.stop_reading();
        }
    }

    pub(super) fn stop_reading(&mut self) {
        if let (Some(reading), Some(worker)) = (&mut self.reading, &mut self.worker)
            && reading.stop_sent.is_none()
        {
            reading.stop_sent = Some(Instant::now());
            if worker.stop().is_err() {
                self.worker = None;
            }
        }
    }

    pub(super) fn poll_reading(&mut self, ctx: &egui::Context) {
        let events = self.worker.as_ref().map(Worker::events).unwrap_or_default();
        let current = self.reading.as_ref().map(|r| r.id);
        for event in events {
            if self.take_save_event(&event) {
                continue;
            }
            if let Some(ended) = self.take_event(event, current) {
                let file = self.reading.take().as_ref().map(file_name);
                self.status = ended_status(file, ended);
            }
        }
        self.drop_a_worker_that_ignores_stop();
        if self.reading.is_some() || self.saving.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// The bar moves on for this reading and never back (pinned by
    /// `app::tests::the_bar_shows_how_far_the_reading_is_and_never_goes_back`).
    pub(super) fn on_progress(&mut self, id: u64, done: f32) {
        if let Some(reading) = self.reading.as_mut().filter(|r| r.id == id) {
            reading.done = reading.done.max(done);
        }
    }

    /// Apply one worker event; how the reading with id `current` ended, if
    /// this event ends it. A worker that exits with no reading under way ends
    /// none, and the next Read aloud starts another (pinned by
    /// `app::tests::a_worker_that_exits_idle_is_started_again_by_the_next_read_aloud`,
    /// `app::tests::a_worker_that_exits_mid_reading_fails_the_reading`).
    fn take_event(&mut self, event: Event, current: Option<u64>) -> Option<Ended> {
        match event {
            Event::Ready => {
                self.model_may_have_been_downloaded();
                None
            }
            Event::FirstAudio { id, ms } => {
                if let Some(reading) = self.reading.as_mut().filter(|r| r.id == id) {
                    reading.heard = true;
                    let reading = &*reading;
                    debug_first_audio(reading, ms);
                    if let Some(target) = reading.translated_into {
                        self.status = format!("Speaking {} in {}", file_name(reading), target.name);
                    }
                }
                None
            }
            Event::Progress { id, done } => {
                self.on_progress(id, done);
                None
            }
            Event::Translated { id, piece } => {
                if let Some(translated) = self.translated.as_mut().filter(|t| t.id == id) {
                    translated.pieces.push(piece);
                }
                None
            }
            Event::Done { id } if Some(id) == current => Some(Ended::Finished),
            Event::Error { id, message } if id.is_none() || id == current => {
                Some(Ended::Failed(message))
            }
            Event::Exited(message) => {
                self.worker = None;
                current.map(|_| Ended::Failed(message))
            }
            Event::Done { .. }
            | Event::Error { .. }
            | Event::Saving { .. }
            | Event::Saved { .. }
            | Event::SaveCancelled { .. } => None,
        }
    }

    /// A stop not answered within `STOP_GRACE` ends the reading here; dropping
    /// the worker kills it, and the next Read aloud starts another.
    fn drop_a_worker_that_ignores_stop(&mut self) {
        let Some(reading) = &self.reading else {
            return;
        };
        if reading
            .stop_sent
            .is_some_and(|sent| sent.elapsed() > speech::STOP_GRACE)
        {
            self.status = format!("Finished {}", file_name(reading));
            self.reading = None;
            self.worker = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_that_ended_is_named_in_the_status() {
        let file = || Some("notes.md".to_owned());
        assert_eq!(ended_status(file(), Ended::Finished), "Finished notes.md");
        assert_eq!(
            ended_status(file(), Ended::Failed("boom".to_owned())),
            "Couldn't read notes.md: boom"
        );
    }

    #[test]
    fn a_failure_with_no_reading_under_way_is_vinospeaks_own() {
        assert_eq!(
            ended_status(None, Ended::Failed("no NPU".to_owned())),
            "vinospeak: no NPU"
        );
        assert_eq!(ended_status(None, Ended::Finished), "");
    }
}
