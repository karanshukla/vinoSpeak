//! Settings' Speech API: `vinospeak --http` on this computer while the switch
//! is on, so other apps can read text with vinoSpeak.

use eframe::egui;
use vino_ui::a11y;
use vino_ui::widgets::{self, ChipKind, Row, Tone};

use super::App;
use crate::api::{ApiEvent, ApiServer, PORT};

/// How the server is doing.
#[derive(Debug, Default, PartialEq)]
pub(super) enum ApiState {
    #[default]
    Off,
    Starting,
    Serving(String),
    Failed(String),
}

impl ApiState {
    fn help(&self) -> String {
        match self {
            ApiState::Off => {
                format!("Serves /v1/audio/speech on localhost:{PORT} for local apps.")
            }
            ApiState::Starting => "Loading model…".to_owned(),
            ApiState::Serving(url) => format!("Listening at {url}"),
            ApiState::Failed(why) => format!("Stopped: {why}"),
        }
    }
}

impl App {
    /// Start or stop the server to match the switch, and follow what it says
    /// (pinned by `app::tests::the_speech_api_runs_while_its_switch_is_on`).
    pub(super) fn follow_api(&mut self, ctx: &egui::Context) {
        match (self.api_on, self.api.is_some()) {
            (true, false) if !matches!(self.api_state, ApiState::Failed(_)) => {
                let ctx = ctx.clone();
                match ApiServer::start(&self.vinospeak, move || ctx.request_repaint()) {
                    Ok(server) => {
                        self.api = Some(server);
                        self.api_state = ApiState::Starting;
                    }
                    Err(e) => self.api_state = ApiState::Failed(format!("{e:#}")),
                }
            }
            (false, true) => {
                self.api = None;
                self.api_state = ApiState::Off;
            }
            _ => {}
        }
        let events = self.api.as_ref().map(ApiServer::events).unwrap_or_default();
        for event in events {
            match event {
                ApiEvent::Serving(url) => self.api_state = ApiState::Serving(url),
                ApiEvent::Exited(why) => {
                    self.api = None;
                    self.api_on = false;
                    self.api_state = ApiState::Failed(why);
                }
            }
        }
    }

    pub(super) fn api_rows(&mut self, ui: &mut egui::Ui) {
        let help = self.api_state.help();
        let mut row = Row::new("OpenAI speech API").help(&help);
        if matches!(self.api_state, ApiState::Failed(_)) {
            row = row.tone(Tone::Error);
        }
        widgets::group_label(ui, "Speech API");
        widgets::group(ui, |ui| {
            row.show(ui, |ui, _| {
                if self.api_state == ApiState::Starting {
                    a11y::live(&widgets::chip(ui, ChipKind::Neutral, "Starting…"));
                }
                let was = self.api_on;
                widgets::switch(ui, &mut self.api_on, "OpenAI speech API");
                if self.api_on && !was {
                    self.api_state = ApiState::Off;
                }
            });
        });
    }
}
