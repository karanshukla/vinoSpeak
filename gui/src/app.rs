//! The window: a sidebar with the search, vinoSearch's hits (or the files
//! changed most recently) and About, and the chosen file as vinoSpeak will
//! say it, with Read aloud and Stop. Narrow, the sidebar becomes tabs. Its
//! look is vino-ui's, shared with the other vinoPilot apps.
//!
//! This file is the window's state and one frame of it. Each part of the
//! frame, and the state changes behind it, is a module:
//!
//! - `files`: the search, the recent files, the chosen file's preview
//! - `reading`: the worker and the reading under way
//! - `model`: the downloaded model's size, voices and deletion
//! - `translation`: the translation model's download, and the Translate picker
//! - `sidebar`, `header`, `results`, `reader`, `player`, `volume`, `about`: what is drawn
//! - `choices`: the voice, speed and volume kept between runs
//! - `capture`: the screenshot taken with nobody at the screen
//! - `media`: the desktop's media controls (MPRIS) and Space

mod about;
mod capture;
mod choices;
mod files;
mod header;
mod media;
mod model;
mod player;
mod reader;
mod reading;
mod results;
mod sidebar;
#[cfg(test)]
mod tests;
mod translation;
mod volume;

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use eframe::egui::{self, Margin};
use vino_ui::a11y;
use vino_ui::theme;
use vino_ui::widgets;

use self::capture::Capture;
use self::model::Model;
use self::translation::Translation;
use crate::model_cache;
use crate::outputs::{self, Output};
use crate::search::{Hit, Index, VinoSearch};
use crate::speech::{self, Part, Reading, Worker};
use crate::voices::{self, Voice};

const DEFAULT_VOICE: &str = "af_heart";
/// A narrow window's page, from its left edge: the header's own inset.
const NARROW_INSET: f32 = 16.0;

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
    /// What `vinospeak --download-` or `--delete-translation-model` said.
    Translation(anyhow::Result<String>),
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
    /// 0 to 1, kept while muted so unmuting brings it back.
    volume: f32,
    muted: bool,
    outputs: Vec<Output>,
    default_output: Option<String>,
    /// None plays on the desktop's default output.
    output: Option<String>,
    /// Started with the first preview, kept for the window's lifetime.
    worker: Option<Worker>,
    reading: Option<Reading>,
    /// The last translated reading's text, as far as it has been translated.
    translated: Option<crate::speech::Translated>,
    /// The original text in place of the translation, by the reader's toggle.
    show_original: bool,
    status: String,
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
    capture: Option<Capture>,
    /// Where the voice, speed and volume are kept; None keeps them for this run only.
    settings: Option<PathBuf>,
    saved: Option<crate::settings::Settings>,
    focused: bool,
    kde_header: Option<theme::KdeHeader>,
    /// The window's buttons and edges are drawn here, not by the desktop
    /// (vino-ui's `window`).
    pub app_title_bar: bool,
    text_scale: Option<f32>,
    /// The About page shows in place of the reader until a file is picked.
    about: bool,
    /// A narrow window shows the list in place of the reader, as its Files tab.
    listing: bool,
    updater: vino_ui::update::Updater,
    /// vinospeak's cache: the model, its voices, and OpenVINO's compiled copy.
    cache: Option<PathBuf>,
    model: Model,
    /// Where vinospeak keeps the translation model; None when there is no home.
    translation_dir: Option<PathBuf>,
    translation: Translation,
    /// The language a reading is translated into first; None reads it as it is.
    translate_to: Option<crate::translation::Target>,
    /// The window as a media player on the session bus, and what it is asked.
    media: Option<(crate::mpris::Media, mpsc::Receiver<crate::mpris::Command>)>,
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
        app.refresh_translation();
        if let Some(path) = crate::settings::path() {
            app.remember(path);
        }
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
            volume: 1.0,
            muted: false,
            outputs,
            default_output,
            output: None,
            worker: None,
            reading: None,
            translated: None,
            show_original: false,
            status: String::new(),
            tx,
            rx,
            capture: None,
            settings: None,
            saved: None,
            focused: true,
            kde_header: None,
            app_title_bar: false,
            text_scale: None,
            about: false,
            listing: true,
            updater: about::updater(),
            cache: model_cache::dir(),
            model: Model::default(),
            translation_dir: crate::translation::dir(),
            translation: Translation::default(),
            translate_to: None,
            media: None,
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
                Reply::Translation(said) => self.on_translation(said),
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
        self.space_pauses(ctx);
        self.follow_media_controls(ctx);
        self.follow_new_settings(ctx);
        ctx.accesskit_node_builder(egui::accesskit_root_id(), |node| {
            node.set_label("vinoSpeak")
        });

        let window = ctx.content_rect().width();
        let wide = window >= widgets::SIDEBAR_FROM;
        if wide {
            self.sidebar_panel(root);
        } else {
            self.header_panel(root);
        }
        let bar = wide && self.app_title_bar;
        let inset = if wide {
            widgets::page_inset(window)
        } else {
            NARROW_INSET
        };
        if bar {
            vino_ui::window::bar(root, inset, |ui| self.bar_title(ui), |_| {});
        }
        self.page(root, wide, bar, inset);
        self.save_choices(ctx);
        self.capture(ctx);
    }

    /// The page's title up in the window's bar: the chosen file, or About.
    fn bar_title(&self, ui: &mut egui::Ui) {
        if self.about {
            let title = widgets::title(ui, vino_ui::about::TITLE);
            a11y::heading(&title, vino_ui::about::TITLE, 2);
        } else if let Some(hit) = &self.selected {
            reader::title_line(ui, hit);
        }
    }

    /// Beside the sidebar, or under the narrow header's tabs: the reader,
    /// About, or (narrow) the list.
    fn page(&mut self, root: &mut egui::Ui, wide: bool, bar: bool, inset: f32) {
        let p = theme::palette(root.ctx());
        let top = if bar {
            2
        } else if wide {
            20
        } else {
            12
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.bg))
            .show(root, |ui| {
                let margin = Margin {
                    left: inset as i8,
                    right: 0,
                    top,
                    bottom: 0,
                };
                if !wide && self.listing {
                    egui::Frame::NONE
                        .inner_margin(Margin {
                            right: 16,
                            ..margin
                        })
                        .show(ui, |ui| self.files_page(ui));
                } else if self.about {
                    egui::Frame::NONE
                        .inner_margin(margin)
                        .show(ui, |ui| self.about(ui, bar));
                } else {
                    self.reader(ui, inset, top, bar);
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
