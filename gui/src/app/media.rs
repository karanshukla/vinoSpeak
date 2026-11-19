//! Pause and resume from outside the player: Space, and the desktop's media
//! controls and keys through MPRIS.

use std::sync::mpsc;

use eframe::egui;

use super::App;
use crate::mpris::{Command, Media, Playing, Status};

impl App {
    /// Put the window on the session bus as a media player.
    pub fn show_on_the_bus(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        if let Some(media) = Media::start(tx, move || ctx.request_repaint()) {
            self.media = Some((media, rx));
        }
    }

    /// Space pauses or resumes the reading under way, before any focused
    /// button can take it, unless a text field is being typed in.
    pub(super) fn space_pauses(&mut self, ctx: &egui::Context) {
        if self.reading.is_some()
            && !ctx.text_edit_focused()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
        {
            self.toggle_pause();
        }
    }

    /// What the desktop's media controls show (pinned by
    /// `app::tests::the_media_controls_show_and_drive_the_reading`).
    pub(super) fn now_playing(&self) -> Playing {
        let Some(reading) = &self.reading else {
            return Playing::default();
        };
        Playing {
            status: if reading.paused_at.is_some() {
                Status::Paused
            } else {
                Status::Playing
            },
            id: reading.id,
            title: reading
                .file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            voice: self.voice.name.clone(),
        }
    }

    /// Do what the media controls asked; Play with nothing under way reads
    /// the file shown, as Read aloud would.
    pub(super) fn on_media(&mut self, ctx: &egui::Context, command: Command) {
        let paused = self.reading.as_ref().map(|r| r.paused_at.is_some());
        match (command, paused) {
            (Command::Stop, Some(_)) => self.stop_reading(),
            (Command::Play | Command::PlayPause, None) => {
                if matches!(self.preview, Some(Ok(_))) {
                    self.read_aloud(ctx);
                }
            }
            (Command::Play, Some(true))
            | (Command::Pause, Some(false))
            | (Command::PlayPause, Some(_)) => self.toggle_pause(),
            _ => {}
        }
    }

    pub(super) fn follow_media_controls(&mut self, ctx: &egui::Context) {
        let commands: Vec<Command> = self
            .media
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for command in commands {
            self.on_media(ctx, command);
        }
        let now = self.now_playing();
        if let Some((media, _)) = &mut self.media {
            media.show(now);
        }
    }
}
