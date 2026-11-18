//! The window: a search box, vinoSearch's hits (or the files changed most
//! recently) down the left, and the chosen file as vinoSpeak will say it,
//! with Read aloud and Stop. Its look is vino-ui's, shared with vinoSearch
//! and vinoWhisper.
//!
//! This file is the window's state and one frame of it. Each part of the
//! frame, and the state changes behind it, is a module:
//!
//! - `files`: the search, the recent files, the chosen file's preview
//! - `reading`: the worker and the reading under way
//! - `model`: the downloaded model's size, voices and deletion
//! - `header`, `results`, `reader`, `player`, `about`: what is drawn
//! - `capture`: the screenshot taken with nobody at the screen

mod about;
mod capture;
mod files;
mod header;
mod model;
mod player;
mod reader;
mod reading;
mod results;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use eframe::egui::{self, Margin, RichText, Stroke};
use vino_ui::a11y;
use vino_ui::theme;
use vino_ui::widgets;

use self::capture::Capture;
use self::model::Model;
use crate::model_cache;
use crate::outputs::{self, Output};
use crate::search::{Hit, Index, VinoSearch};
use crate::speech::{self, Part, Reading, Worker};
use crate::voices::{self, Voice};

const DEFAULT_VOICE: &str = "af_heart";

/// What a background thread sends back to the window.
enum Reply {
    /// The hits for the search with this `generation`.
    Results(u64, anyhow::Result<Vec<Hit>>),
    /// The recent files and sessions, and why vinoSearch didn't answer if it didn't.
    Recent(Vec<Hit>, Option<String>),
    /// The file the desktop's file dialog chose, or why there is none.
    Opened(anyhow::Result<Option<PathBuf>>),
    /// The preview for the `previewed` it was asked for.
    Preview(u64, anyhow::Result<String>),
    /// What `vinospeak --delete-model` said.
    Deleted(anyhow::Result<String>),
}

/// The chosen file's text, as vinospeak gives it: what the worker is sent
/// to say, and what the reader shows.
struct Preview {
    text: String,
    paragraphs: Vec<String>,
}

pub struct App {
    index: Arc<dyn Index>,
    vinospeak: PathBuf,
    query: String,
    /// When the query was last typed in; None once that has been searched.
    edited: Option<Instant>,
    searched: String,
    /// Counts the searches asked for, so a late answer to an older one is dropped.
    generation: u64,
    searching: bool,
    hits: Vec<Hit>,
    recent: Vec<Hit>,
    recent_asked: bool,
    /// Why vinoSearch didn't answer for the recent files; the window then
    /// lists the sessions on disk and can't search.
    vinosearch_missing: Option<String>,
    /// Where vinoWhisper's sessions are looked for, with or without vinoSearch.
    session_dirs: Vec<PathBuf>,
    /// A file named on the command line, opened on the first frame.
    to_open: Option<PathBuf>,
    error: Option<String>,
    selected: Option<Hit>,
    preview: Option<Result<Preview, String>>,
    /// Counts the previews asked for, so a late one for another file or part is dropped.
    previewed: u64,
    /// The part of the selected session shown and read.
    part: Part,
    voices: Vec<Voice>,
    voice: Voice,
    speed: f32,
    outputs: Vec<Output>,
    default_output: Option<String>,
    /// None plays on the desktop's default output.
    output: Option<String>,
    /// Started with the first preview, kept for the window's lifetime.
    worker: Option<Worker>,
    reading: Option<Reading>,
    status: String,
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
    capture: Option<Capture>,
    focused: bool,
    kde_header: Option<theme::KdeHeader>,
    /// The window's buttons and edges are drawn here, not by the desktop
    /// (vino-ui's `window`).
    pub app_title_bar: bool,
    text_scale: Option<f32>,
    /// The About page shows in place of the reader until a file is picked.
    about: bool,
    updater: vino_ui::update::Updater,
    /// vinospeak's cache: the model, its voices, and OpenVINO's compiled copy.
    cache: Option<PathBuf>,
    model: Model,
}

impl App {
    /// The window as it runs: the real vinoSearch and vinospeak, and the
    /// `VINOSPEAK_GUI_*` variables gui/README.md lists.
    pub fn new() -> Self {
        let mut app = Self::with(
            Arc::new(VinoSearch),
            speech::binary(),
            voices::voices(),
            outputs::outputs(),
        );
        app.kde_header = theme::kde_header();
        app.session_dirs = crate::sessions::user_dirs();
        app.to_open = std::env::args_os().nth(1).map(PathBuf::from);
        app.capture = Capture::from_env();
        if let Ok(query) = std::env::var("VINOSPEAK_GUI_QUERY") {
            app.type_query(&query);
        }
        if let Ok(about) = std::env::var("VINOSPEAK_GUI_ABOUT") {
            app.open_about();
            app.model.confirm = about == "delete";
        }
        app
    }

    /// The window with its outside world handed in, as the tests build it.
    pub fn with(
        index: Arc<dyn Index>,
        vinospeak: PathBuf,
        voices: Vec<Voice>,
        (outputs, default_output): (Vec<Output>, Option<String>),
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let voice = voices
            .iter()
            .find(|v| v.id == DEFAULT_VOICE)
            .unwrap_or(&voices[0])
            .clone();
        Self {
            index,
            vinospeak,
            query: String::new(),
            edited: None,
            searched: String::new(),
            generation: 0,
            searching: false,
            hits: Vec::new(),
            recent: Vec::new(),
            recent_asked: false,
            vinosearch_missing: None,
            session_dirs: Vec::new(),
            to_open: None,
            error: None,
            selected: None,
            preview: None,
            previewed: 0,
            part: Part::Summary,
            voices,
            voice,
            speed: 1.0,
            outputs,
            default_output,
            output: None,
            worker: None,
            reading: None,
            status: String::new(),
            tx,
            rx,
            capture: None,
            focused: true,
            kde_header: None,
            app_title_bar: false,
            text_scale: None,
            about: false,
            updater: about::updater(),
            cache: model_cache::dir(),
            model: Model::default(),
        }
    }

    /// The desktop's text size as zoom, applied only when it changes so a
    /// Ctrl + since then is kept.
    pub fn follow_text_scale(&mut self, ctx: &egui::Context, scale: Option<f32>) {
        if scale != self.text_scale {
            self.text_scale = scale;
            ctx.set_zoom_factor(scale.unwrap_or(1.0));
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Results(generation, hits) => self.on_results(ctx, generation, hits),
                Reply::Recent(recent, missing) => self.on_recent(ctx, recent, missing),
                Reply::Opened(chosen) => self.on_opened(ctx, chosen),
                Reply::Preview(previewed, text) => self.on_preview(ctx, previewed, text),
                Reply::Deleted(said) => self.on_deleted(said),
            }
        }
    }

    /// One frame of the window; the tests drive this without eframe.
    pub fn show(&mut self, root: &mut egui::Ui) {
        let ctx = &root.ctx().clone();
        if !self.recent_asked {
            self.load_recent(ctx);
        }
        self.receive(ctx);
        self.search_when_typing_pauses(ctx);
        self.open_dropped(ctx);
        self.poll_reading(ctx);
        ctx.accesskit_node_builder(egui::accesskit_root_id(), |node| {
            node.set_label("vinoSpeak")
        });

        self.header_panel(root);
        self.status_bar(root);
        self.results_panel(root);
        self.reader_or_about(root);
        self.capture(ctx);
    }

    /// The line along the bottom: what last happened, or which vinospeak speaks.
    fn status_bar(&self, root: &mut egui::Ui) {
        let p = theme::palette(root.ctx());
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(p.surface)
                    .stroke(Stroke::new(1.0_f32, p.rule))
                    .inner_margin(Margin::symmetric(20, 6)),
            )
            .show(root, |ui| {
                let shown = if self.status.is_empty() {
                    ui.label(
                        RichText::new(format!("Speech by {}", self.vinospeak.display()))
                            .monospace()
                            .size(11.5)
                            .color(p.muted),
                    )
                } else {
                    ui.label(RichText::new(&self.status).size(12.5).color(p.muted))
                };
                a11y::live(&shown);
            });
    }

    fn reader_or_about(&mut self, root: &mut egui::Ui) {
        let p = theme::palette(root.ctx());
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.bg))
            .show(root, |ui| {
                if self.about {
                    egui::Frame::NONE
                        .inner_margin(Margin {
                            left: widgets::PAGE_INSET as i8,
                            right: 0,
                            top: 20,
                            bottom: 0,
                        })
                        .show(ui, |ui| self.about(ui));
                } else {
                    self.reader(ui);
                }
            });
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        vino_ui::input::faster_touchpad(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let focused = ctx.input(|input| input.focused);
        let regained_focus = focused && !self.focused;
        if regained_focus {
            theme::follow_desktop(ctx);
            self.kde_header = theme::kde_header();
            self.follow_text_scale(ctx, theme::desktop_text_scale());
            if self.vinosearch_missing.is_some() {
                self.recent_asked = false;
            }
        }
        self.focused = focused;
        self.show(ui);
        if self.app_title_bar {
            vino_ui::window::edges(ui);
        }
    }
}
