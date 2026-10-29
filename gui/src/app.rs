//! The window: a search box, vinoSearch's hits (or the files changed most
//! recently) down the left, and the chosen file as vinoSpeak will say it,
//! with Read aloud and Stop. Its look is vino-ui's, shared with vinoSearch
//! and vinoWhisper (vinoSpeak#32).

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, Layout, Margin, RichText, Sense, Stroke};
use vino_ui::a11y;
use vino_ui::icons::{self, Icon};
use vino_ui::theme;
use vino_ui::widgets::{self, Tone};

use crate::search::{self, Hit, Index, Kind, VinoSearch};
use crate::speech::{self, Event, Output, Part, Reading, Source, Voice, Worker};

/// vinoSearch answers nothing under three characters, as KRunner does.
const MIN_QUERY_CHARS: usize = 3;
/// Search once typing pauses this long, not on every key.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// The reading column's widest: about 80 characters of body text.
const MEASURE: f32 = 680.0;
/// How many recent files fill the list before a search.
const RECENT: u32 = 20;
/// A session's parts, as the reader's toggle offers them.
const PARTS: [(Part, &str); 2] = [(Part::Summary, "Summary"), (Part::Transcript, "Transcript")];
/// The speed presets, as the player offers them.
const SPEEDS: [(f32, &str); 4] = [(0.8, "0.8×"), (1.0, "1.0×"), (1.25, "1.25×"), (1.5, "1.5×")];

enum Reply {
    Results(u64, anyhow::Result<Vec<Hit>>),
    Recent(anyhow::Result<Vec<Hit>>),
    /// The preview for the `previewed` it was asked for.
    Preview(u64, anyhow::Result<String>),
}

/// `VINOSPEAK_GUI_SCREENSHOT=out.png`, with `VINOSPEAK_GUI_QUERY=...` to
/// search first: save the window once the first file's text is in, and exit.
/// `VINOSPEAK_GUI_PICK=n` picks the nth file instead, and
/// `VINOSPEAK_GUI_SPEAK=1` starts reading it. How the proof of concept is
/// checked without someone at the screen.
struct Capture {
    out: PathBuf,
    pick: usize,
    speak: bool,
    requested: bool,
    deadline: Instant,
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
    edited: Option<Instant>,
    searched: String,
    generation: u64,
    searching: bool,
    hits: Vec<Hit>,
    recent: Vec<Hit>,
    recent_asked: bool,
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
    text_scale: Option<f32>,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self::with(
            Arc::new(VinoSearch),
            speech::binary(),
            speech::voices(),
            speech::outputs(),
        );
        app.kde_header = theme::kde_header();
        app.capture = std::env::var_os("VINOSPEAK_GUI_SCREENSHOT").map(|out| Capture {
            out: out.into(),
            pick: std::env::var("VINOSPEAK_GUI_PICK")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
            speak: std::env::var_os("VINOSPEAK_GUI_SPEAK").is_some(),
            requested: false,
            deadline: Instant::now() + Duration::from_secs(30),
        });
        if let Ok(query) = std::env::var("VINOSPEAK_GUI_QUERY") {
            app.type_query(&query);
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
            .find(|v| v.id == "af_heart")
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
            text_scale: None,
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

    /// Set the search box as if typed, searching once the debounce is over.
    fn type_query(&mut self, query: &str) {
        self.query = query.to_owned();
        self.edited = Some(Instant::now() - DEBOUNCE);
    }

    /// Search results once there's a query that found something, else the
    /// recent files.
    fn showing_hits(&self) -> bool {
        self.query.trim().chars().count() >= MIN_QUERY_CHARS && !self.hits.is_empty()
    }

    fn listed(&self) -> &[Hit] {
        if self.showing_hits() {
            &self.hits
        } else {
            &self.recent
        }
    }

    fn search(&mut self, ctx: &egui::Context) {
        self.generation += 1;
        self.searched = self.query.trim().to_owned();
        self.searching = true;
        let (tx, ctx, generation, query) = (
            self.tx.clone(),
            ctx.clone(),
            self.generation,
            self.searched.clone(),
        );
        let index = self.index.clone();
        std::thread::spawn(move || {
            let hits = index.search(&query).map(search::title_sessions);
            let _ = tx.send(Reply::Results(generation, hits));
            ctx.request_repaint();
        });
    }

    fn load_recent(&mut self, ctx: &egui::Context) {
        self.recent_asked = true;
        let (tx, ctx, index) = (self.tx.clone(), ctx.clone(), self.index.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Reply::Recent(
                index.recent(RECENT).map(search::title_sessions),
            ));
            ctx.request_repaint();
        });
    }

    fn select(&mut self, ctx: &egui::Context, hit: Hit) {
        self.part = match hit.kind {
            Kind::Session { summarised: false } => Part::Transcript,
            _ => Part::Summary,
        };
        self.selected = Some(hit);
        self.load_preview(ctx);
    }

    /// The selected file's text, as vinospeak will say it.
    fn load_preview(&mut self, ctx: &egui::Context) {
        self.preview = None;
        self.previewed += 1;
        let Some(hit) = self.selected.clone().filter(|h| h.kind != Kind::Image) else {
            return;
        };
        let (tx, ctx, previewed, part) = (self.tx.clone(), ctx.clone(), self.previewed, self.part);
        let (index, vinospeak) = (self.index.clone(), self.vinospeak.clone());
        std::thread::spawn(move || {
            let text = match hit.kind {
                Kind::Document => index.file_text(&hit.path).map(Source::Text),
                Kind::Session { .. } => Ok(Source::Session(hit.path.clone(), part)),
                _ => Ok(Source::File(hit.path.clone())),
            }
            .and_then(|source| speech::preview(&vinospeak, &source));
            let _ = tx.send(Reply::Preview(previewed, text));
            ctx.request_repaint();
        });
    }

    /// The first file worth hearing in what's listed (the capture's pick).
    fn select_first(&mut self, ctx: &egui::Context) {
        let pick = self.capture.as_ref().map_or(0, |c| c.pick);
        let first = self
            .listed()
            .iter()
            .filter(|h| h.kind != Kind::Image)
            .nth(pick)
            .cloned();
        if let Some(hit) = first {
            self.select(ctx, hit);
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Results(generation, _) if generation != self.generation => {}
                Reply::Results(_, Ok(hits)) => {
                    self.searching = false;
                    self.error = None;
                    // An image has nothing to read aloud.
                    self.hits = hits.into_iter().filter(|h| h.kind != Kind::Image).collect();
                    if self.showing_hits() {
                        self.select_first(ctx);
                    }
                }
                Reply::Results(_, Err(e)) => {
                    self.searching = false;
                    self.hits.clear();
                    self.error = Some(format!("{e:#}"));
                }
                Reply::Recent(Ok(recent)) => {
                    self.recent = recent;
                    if self.selected.is_none() && self.query.is_empty() {
                        self.select_first(ctx);
                    }
                }
                Reply::Recent(Err(e)) => {
                    self.status = format!("No recent files: {e:#}");
                }
                Reply::Preview(previewed, text) => {
                    if previewed == self.previewed {
                        self.preview = Some(
                            text.map(|text| Preview {
                                paragraphs: speech::paragraphs(&text),
                                text,
                            })
                            .map_err(|e| format!("{e:#}")),
                        );
                        if matches!(self.preview, Some(Ok(_))) {
                            self.start_worker(ctx);
                        }
                    }
                }
            }
        }
    }

    /// The worker, loading the model while the user looks at the preview.
    fn start_worker(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        let ctx = ctx.clone();
        match Worker::start(&self.vinospeak, move || ctx.request_repaint()) {
            Ok(worker) => self.worker = Some(worker),
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn read_aloud(&mut self, ctx: &egui::Context) {
        self.start_worker(ctx);
        let (Some(hit), Some(Ok(preview)), Some(worker)) =
            (&self.selected, &self.preview, &mut self.worker)
        else {
            return;
        };
        // The worker stops the reading before this one itself.
        match worker.speak(
            &preview.text,
            &self.voice,
            self.speed,
            self.output.as_deref(),
        ) {
            Ok(id) => {
                self.status = format!("Speaking {}", hit.name);
                self.reading = Some(Reading {
                    id,
                    file: hit.path.clone(),
                    started: Instant::now(),
                    stop_sent: None,
                });
            }
            Err(e) => {
                self.status = format!("{e:#}");
                self.worker = None;
            }
        }
    }

    fn stop_reading(&mut self) {
        if let (Some(reading), Some(worker)) = (&mut self.reading, &mut self.worker)
            && reading.stop_sent.is_none()
        {
            reading.stop_sent = Some(Instant::now());
            if worker.stop().is_err() {
                self.worker = None;
            }
        }
    }

    fn name(reading: &Reading) -> String {
        reading
            .file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    fn poll_reading(&mut self, ctx: &egui::Context) {
        let events = self.worker.as_ref().map(Worker::events).unwrap_or_default();
        let current = self.reading.as_ref().map(|r| r.id);
        for event in events {
            let ended = match event {
                Event::Ready => None,
                Event::FirstAudio { id, ms } => {
                    if let Some(r) = self.reading.as_ref().filter(|r| r.id == id)
                        && std::env::var_os("VINOSPEAK_GUI_DEBUG").is_some()
                    {
                        let since = r.started.elapsed().as_millis();
                        eprintln!("first audio {since} ms after Read aloud ({ms} ms in vinospeak)");
                    }
                    None
                }
                Event::Done { id } if Some(id) == current => Some(None),
                Event::Error { id, message } if id.is_none() || id == current => {
                    Some(Some(message))
                }
                Event::Exited(message) => {
                    self.worker = None;
                    // A worker that died idle is started again by the next Read aloud.
                    current.map(|_| Some(message))
                }
                Event::Done { .. } | Event::Error { .. } => None,
            };
            if let Some(failed) = ended {
                let name = self.reading.as_ref().map(Self::name);
                self.status = match (name, failed) {
                    (Some(name), None) => format!("Finished {name}"),
                    (Some(name), Some(e)) => format!("Couldn't read {name}: {e}"),
                    (None, Some(e)) => format!("vinospeak: {e}"),
                    (None, None) => String::new(),
                };
                self.reading = None;
            }
        }
        let Some(reading) = &self.reading else {
            return;
        };
        // A worker that doesn't answer a stop is killed, and the next Read
        // aloud starts another.
        if reading
            .stop_sent
            .is_some_and(|sent| sent.elapsed() > speech::STOP_GRACE)
        {
            self.status = format!("Finished {}", Self::name(reading));
            self.reading = None;
            self.worker = None;
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn capture(&mut self, ctx: &egui::Context) {
        let Some(capture) = &mut self.capture else {
            return;
        };
        let ready = self.preview.is_some() || Instant::now() > capture.deadline;
        if ready && !capture.requested {
            if capture.speak && self.reading.is_none() && self.preview.is_some() {
                capture.speak = false;
                capture.deadline = Instant::now() + Duration::from_secs(4);
                self.read_aloud(ctx);
                return;
            }
            if self.reading.is_some() && Instant::now() < capture.deadline {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            capture.requested = true;
            let (out, done) = (capture.out.clone(), ctx.clone());
            let save = egui::ScreenshotCallback::new(move |image| {
                let [w, h] = image.size;
                let saved = image::RgbaImage::from_raw(w as u32, h as u32, image.as_raw().to_vec())
                    .map(|img| img.save(&out));
                match saved {
                    Some(Ok(())) => eprintln!("saved {}", out.display()),
                    _ => eprintln!("couldn't save {}", out.display()),
                }
                // Dropping the window stops any reading.
                done.send_viewport_cmd(egui::ViewportCommand::Close);
                done.request_repaint();
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(save));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn header(&mut self, ui: &mut egui::Ui, header: theme::Header) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.allocate_ui_with_layout(
                egui::vec2(290.0, 36.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    vino_ui::logo::speak(ui, 32.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let title = ui.label(
                            RichText::new("vinoSpeak")
                                .size(17.0)
                                .family(theme::bold())
                                .color(header.text),
                        );
                        a11y::heading(&title, "vinoSpeak", 1);
                        ui.label(
                            RichText::new("Find it, then listen.")
                                .size(12.5)
                                .color(header.muted),
                        );
                    });
                },
            );
            let width = ui.available_width().clamp(160.0, 560.0);
            let response = widgets::search_field(
                ui,
                &mut self.query,
                "Search your files by what's in them…",
                "Search",
                width,
            );
            if response.changed() {
                self.edited = Some(Instant::now());
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.edited = Some(Instant::now() - DEBOUNCE);
            }
        });
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let typed = self.query.trim().chars().count() >= MIN_QUERY_CHARS;
        let note = if let Some(e) = &self.error {
            Some(format!("vinoSearch didn't answer: {e}"))
        } else if typed && self.searching {
            Some("Searching…".to_owned())
        } else if typed && self.hits.is_empty() && self.edited.is_none() {
            Some("Nothing close enough.".to_owned())
        } else {
            None
        };
        if let Some(note) = note {
            ui.add_space(10.0);
            a11y::live(&widgets::muted(ui, note));
        }
        let heading = if self.showing_hits() {
            "Results"
        } else {
            "Recently changed"
        };
        widgets::group_label(ui, heading);
        if !self.showing_hits() && self.recent.is_empty() {
            widgets::muted(ui, "Nothing indexed yet.");
            return;
        }
        let selected = self.selected.as_ref().map(|h| h.path.clone());
        let mut clicked = None;
        widgets::page_scroll(ui, "the files", f32::INFINITY, |ui| {
            a11y::list_box(ui, "Files");
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, hit) in self.listed().iter().enumerate() {
                let chosen = selected.as_ref() == Some(&hit.path);
                let faded = hit.kind == Kind::Image;
                // vinoSearch describes a hit as "dir:line  snippet"; a recent file is
                // "dir  Changed ... ago".
                let sub = match hit.kind {
                    Kind::Image => "Image · nothing to read".to_owned(),
                    _ => hit
                        .description
                        .split_once("  ")
                        .map_or(hit.description.clone(), |(place, detail)| {
                            format!("{place} · {}", detail.trim())
                        }),
                };
                let bg = ui.painter().add(egui::Shape::Noop);
                let shown = egui::Frame::new()
                    .inner_margin(Margin::symmetric(10, 9))
                    .corner_radius(10.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let icon = match hit.kind {
                                Kind::Text => Icon::FileText,
                                Kind::Document => Icon::File,
                                Kind::Image => Icon::Image,
                                Kind::Session { .. } => Icon::Sessions,
                            };
                            widgets::file_tile(ui, icon, 30.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                let name = RichText::new(&hit.name).family(if faded {
                                    theme::medium()
                                } else {
                                    theme::semibold()
                                });
                                ui.add(
                                    egui::Label::new(if faded {
                                        name.color(p.muted)
                                    } else {
                                        name
                                    })
                                    .truncate()
                                    .selectable(false),
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(&sub).size(12.5).color(p.muted))
                                        .truncate()
                                        .selectable(false),
                                );
                            });
                        });
                    });
                let rect = shown.response.rect;
                let response = ui.interact(rect, ui.scope_id().with(i), Sense::click());
                if chosen {
                    ui.painter()
                        .set(bg, egui::Shape::rect_filled(rect, 10.0, p.tint));
                    ui.painter().rect_stroke(
                        rect,
                        10.0,
                        Stroke::new(1.0_f32, p.accent),
                        egui::StrokeKind::Inside,
                    );
                } else if response.hovered() {
                    ui.painter()
                        .set(bg, egui::Shape::rect_filled(rect, 10.0, p.field));
                }
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        10.0,
                        Stroke::new(2.0_f32, p.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                a11y::list_box_option(&response, &format!("{}, {sub}", hit.name), chosen);
                if response.clicked() {
                    clicked = Some(hit.clone());
                }
            }
        });
        if let Some(hit) = clicked {
            self.select(ui.ctx(), hit);
        }
    }

    /// Voices by name, under a heading per accent.
    fn voice_picker(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let v = self.voice.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::icon(ui, Icon::Voice, 14.0, p.muted);
            let shown = widgets::combo(ui, "voice", format!("{} · {}", v.name, v.gender), 170.0)
                .show_ui(ui, |ui| {
                    ui.set_min_width(220.0);
                    let mut accent = "";
                    for voice in &self.voices {
                        if voice.accent != accent {
                            accent = voice.accent;
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(accent.to_uppercase())
                                    .size(11.0)
                                    .family(theme::semibold())
                                    .color(p.muted),
                            );
                        }
                        let label = format!("{}  ·  {}", voice.name, voice.gender);
                        ui.selectable_value(&mut self.voice, voice.clone(), label);
                    }
                });
            ui.ctx()
                .accesskit_node_builder(shown.response.id, |node| node.set_label("Voice"));
            shown
                .response
                .on_hover_text(format!("{}, {} ({})", v.accent, v.gender, v.id));
        });
    }

    /// Where speech plays. A change applies from the next Read aloud.
    fn output_picker(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let describe = |name: &str| {
            self.outputs
                .iter()
                .find(|o| o.name == name)
                .map_or_else(|| name.to_owned(), |o| o.description.clone())
        };
        let default = match &self.default_output {
            Some(name) => format!("Default ({})", describe(name)),
            None => "Default output".to_owned(),
        };
        let selected = self.output.as_deref().map_or(default.clone(), describe);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::icon(ui, Icon::Speaker, 14.0, p.muted);
            let shown = widgets::combo(ui, "output", selected, 200.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.output, None, default);
                for o in &self.outputs {
                    ui.selectable_value(&mut self.output, Some(o.name.clone()), &o.description);
                }
            });
            ui.ctx()
                .accesskit_node_builder(shown.response.id, |node| node.set_label("Output"));
            shown
                .response
                .on_hover_text("Where speech plays, from the next Read aloud");
        });
    }

    /// The player: the round button, what it is doing, and the voice, speed
    /// and output choices after it.
    fn player(&mut self, ui: &mut egui::Ui, hit: &Hit) {
        let p = theme::palette(ui.ctx());
        let readable = matches!(self.preview, Some(Ok(_)));
        let speaking_this = self.reading.as_ref().is_some_and(|r| r.file == hit.path);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 8.0);
            if speaking_this {
                if round_button(ui, Icon::Stop, "Stop", p.text, p.bg, true).clicked() {
                    self.stop_reading();
                }
                let secs = self
                    .reading
                    .as_ref()
                    .map_or(0, |r| r.started.elapsed().as_secs());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let shown = ui.label(RichText::new("Speaking").family(theme::semibold()));
                    a11y::decorative(ui, shown.id);
                    let elapsed = ui.label(
                        RichText::new(format!("{}:{:02}", secs / 60, secs % 60))
                            .size(12.5)
                            .color(p.muted),
                    );
                    a11y::decorative(ui, elapsed.id);
                });
            } else {
                if round_button(
                    ui,
                    Icon::Play,
                    "Read aloud",
                    p.primary_fill,
                    p.primary_text,
                    readable,
                )
                .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.read_aloud(&ctx);
                }
                let shown = ui.label(RichText::new("Read aloud").family(theme::semibold()));
                a11y::decorative(ui, shown.id);
            }
            ui.add_space(8.0);
            self.voice_picker(ui);
            let mut speed = self.speed;
            if widgets::segmented(ui, "Speed", &mut speed, &SPEEDS) {
                self.speed = speed;
            }
            self.output_picker(ui);
        });
    }

    fn reader(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let Some(hit) = self.selected.clone() else {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                widgets::muted(ui, "Pick a file to hear it.");
            });
            return;
        };
        egui::Frame::NONE
            .inner_margin(Margin {
                left: 32,
                right: 32,
                top: 20,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let title = ui.add(
                        egui::Label::new(RichText::new(&hit.name).size(22.0).family(theme::bold()))
                            .truncate(),
                    );
                    a11y::heading(&title, &hit.name, 2);
                    match hit.kind {
                        Kind::Document => drop(widgets::tag(ui, Tone::Neutral, "Document")),
                        Kind::Session { .. } => drop(widgets::tag(ui, Tone::Neutral, "Session")),
                        _ => {}
                    }
                });
                ui.add(
                    egui::Label::new(
                        RichText::new(hit.path.display().to_string())
                            .monospace()
                            .color(p.muted),
                    )
                    .truncate(),
                )
                .on_hover_text(hit.path.display().to_string());
                ui.add_space(12.0);
                self.player(ui, &hit);
            });
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            ui.cursor().top(),
            Stroke::new(1.0_f32, p.rule),
        );

        egui::Frame::NONE
            .inner_margin(Margin {
                left: 32,
                right: 0,
                top: 20,
                bottom: 0,
            })
            .show(ui, |ui| {
                if let Kind::Session { summarised } = hit.kind {
                    self.part_toggle(ui, summarised);
                    ui.add_space(12.0);
                }
                self.text(ui, &hit);
            });
    }

    /// Summary or Transcript, for a session. Without a summary the toggle
    /// stays on Transcript, disabled: vino-ui's segmented can't disable one
    /// option alone.
    fn part_toggle(&mut self, ui: &mut egui::Ui, summarised: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let mut part = self.part;
            let changed = ui
                .add_enabled_ui(summarised, |ui| {
                    widgets::segmented(ui, "Part", &mut part, &PARTS)
                })
                .inner;
            if !summarised {
                widgets::muted(ui, "Not summarised yet");
            }
            if changed {
                self.part = part;
                self.load_preview(ui.ctx());
            }
        });
    }

    fn text(&self, ui: &mut egui::Ui, hit: &Hit) {
        let p = theme::palette(ui.ctx());
        match (&hit.kind, &self.preview) {
            (Kind::Image, _) => {
                widgets::muted(ui, "An image has no text to read.");
            }
            (Kind::Document, None) => {
                a11y::live(&widgets::muted(ui, "Getting the text from vinoSearch…"));
            }
            (_, None) => {
                a11y::live(&widgets::muted(ui, "Preparing the text…"));
            }
            (_, Some(Err(e))) => {
                a11y::live(&widgets::muted(ui, format!("Can't read this file: {e}")));
            }
            (_, Some(Ok(preview))) if preview.paragraphs.is_empty() => {
                widgets::muted(ui, "This file has no text to read (a scan?).");
            }
            (_, Some(Ok(preview))) => {
                widgets::page_scroll(ui, "the text", MEASURE, |ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    for para in &preview.paragraphs {
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = ui.available_width();
                        let (color, italics) = if para == "Code skipped." {
                            (p.muted, true)
                        } else {
                            (p.text, false)
                        };
                        job.append(
                            para,
                            0.0,
                            egui::TextFormat {
                                font_id: egui::FontId::proportional(15.0),
                                color,
                                italics,
                                line_height: Some(25.0),
                                ..Default::default()
                            },
                        );
                        ui.add(egui::Label::new(job).wrap());
                    }
                });
            }
        }
    }
}

/// A round 44 px button with an icon, named for a screen reader.
fn round_button(
    ui: &mut egui::Ui,
    icon: Icon,
    name: &str,
    fill: Color32,
    color: Color32,
    enabled: bool,
) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(44.0, 44.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    if ui.is_rect_visible(rect) {
        let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
        let fill = if response.hovered() && enabled {
            fill.lerp_to_gamma(p.text, 0.08)
        } else {
            fill
        };
        let painter = ui.painter();
        painter.circle_filled(rect.center(), 22.0, fade(fill));
        // Filled, as the board draws them: the play triangle and the stop square.
        let k = 18.0 / 24.0;
        let at = |x: f32, y: f32| rect.center() + egui::vec2((x - 12.0) * k, (y - 12.0) * k);
        let shape = match icon {
            Icon::Play => egui::Shape::convex_polygon(
                vec![at(9.0, 5.0), at(20.0, 12.0), at(9.0, 19.0)],
                fade(color),
                Stroke::NONE,
            ),
            _ => egui::Shape::rect_filled(
                egui::Rect::from_min_max(at(6.5, 6.5), at(17.5, 17.5)),
                2.0,
                fade(color),
            ),
        };
        painter.add(shape);
        if response.has_focus() {
            painter.circle_stroke(rect.center(), 25.0, Stroke::new(2.0_f32, p.accent));
        }
    }
    response.on_hover_text(name)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let focused = ctx.input(|input| input.focused);
        if focused && !self.focused {
            theme::follow_desktop(ctx);
            self.kde_header = theme::kde_header();
            self.follow_text_scale(ctx, theme::desktop_text_scale());
        }
        self.focused = focused;
        self.show(ui);
    }
}

impl App {
    /// One frame of the window; the tests drive this without eframe.
    pub fn show(&mut self, root: &mut egui::Ui) {
        let ctx = &root.ctx().clone();
        if !self.recent_asked {
            self.load_recent(ctx);
        }
        self.receive(ctx);
        if let Some(edited) = self.edited {
            let ready = edited.elapsed() >= DEBOUNCE;
            if ready {
                self.edited = None;
                let query = self.query.trim();
                if query.chars().count() >= MIN_QUERY_CHARS && query != self.searched {
                    self.search(ctx);
                }
            } else {
                ctx.request_repaint_after(DEBOUNCE - edited.elapsed());
            }
        }
        self.poll_reading(ctx);
        ctx.accesskit_node_builder(egui::accesskit_root_id(), |node| {
            node.set_label("vinoSpeak")
        });

        let p = theme::palette(ctx);
        let header = theme::header(ctx, self.kde_header.as_ref(), self.focused);
        egui::Panel::top("header")
            .show_separator_line(false)
            .frame(widgets::header_frame(header, p).inner_margin(Margin {
                left: 20,
                right: 20,
                top: 10,
                bottom: 12,
            }))
            .show(root, |ui| self.header(ui, header));
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
        egui::Panel::left("results")
            .resizable(true)
            .show_separator_line(false)
            .default_size(320.0)
            .size_range(260.0..=560.0)
            .frame(egui::Frame::NONE.fill(p.surface).inner_margin(Margin {
                left: 10,
                right: 0,
                top: 0,
                bottom: 12,
            }))
            .show(root, |ui| {
                self.results(ui);
                widgets::panel_rule(ui, widgets::Edge::Left);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.bg))
            .show(root, |ui| self.reader(ui));
        self.capture(ctx);
    }
}

#[cfg(test)]
mod tests;
