//! The voice, speed, volume and translation language, restored at start and
//! saved as they change.
//! Mute is not kept: a window that opens silent looks broken.

use std::path::PathBuf;

use eframe::egui;

use super::App;
use super::player::SPEEDS;
use crate::settings::{self, Settings};

impl App {
    /// Take the choices saved at `path`, and save there from now on. A saved
    /// voice that isn't installed, or a speed the player doesn't offer, is
    /// left at its default.
    pub fn remember(&mut self, path: PathBuf) {
        let saved = settings::load(&path);
        if let Some(voice) = saved
            .voice
            .as_deref()
            .and_then(|id| self.voices.iter().find(|v| v.id == id))
        {
            self.voice = voice.clone();
        }
        if let Some(speed) = saved.speed.filter(|s| SPEEDS.iter().any(|(o, _)| o == s)) {
            self.speed = speed;
        }
        if let Some(volume) = saved.volume {
            self.volume = volume;
        }
        self.translate_to = saved
            .translate
            .as_deref()
            .and_then(crate::translation::target)
            .filter(|_| self.translation.downloaded);
        self.saved = Some(self.choices());
        self.settings = Some(path);
    }

    fn choices(&self) -> Settings {
        Settings {
            voice: Some(self.voice.id.clone()),
            speed: Some(self.speed),
            volume: Some(self.volume),
            translate: self.translate_to.map(|t| t.language.to_owned()),
        }
    }

    /// Written once the pointer is let go, not on every step of a drag.
    pub(super) fn save_choices(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.settings else {
            return;
        };
        let choices = self.choices();
        if self.saved.as_ref() == Some(&choices) || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        if let Err(e) = settings::save(path, &choices) {
            self.status = format!("Couldn't save your choices: {e}");
        }
        self.saved = Some(choices);
    }
}
