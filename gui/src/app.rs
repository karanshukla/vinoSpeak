//! The window: a search box, vinoSearch's hits down the left, and the chosen
//! file as vinoSpeak will say it, with Read aloud and Stop.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Sense, Stroke};

use crate::search::{self, Hit, Kind};
use crate::speech::{self, Reading, State};
use crate::theme;

/// vinoSearch answers nothing under three characters, as KRunner does.
const MIN_QUERY_CHARS: usize = 3;
/// Search once typing pauses this long, not on every key.
const DEBOUNCE: Duration = Duration::from_millis(300);

enum Reply {
    Results(u64, anyhow::Result<Vec<Hit>>),
    Preview(PathBuf, anyhow::Result<String>),
}

/// `VINOSPEAK_GUI_SCREENSHOT=out.png` with `VINOSPEAK_GUI_QUERY=...`: search,
/// pick the first readable hit, save the window once its preview is in, and
/// exit. How the proof of concept is checked without someone at the screen.
struct Capture {
    out: PathBuf,
    speak: bool,
    requested: bool,
    deadline: Instant,
}

pub struct App {
    query: String,
    edited: Option<Instant>,
    searched: String,
    generation: u64,
    searching: bool,
    hits: Vec<Hit>,
    error: Option<String>,
    selected: Option<usize>,
    preview: Option<Result<Vec<String>, String>>,
    voices: Vec<String>,
    voice: String,
    speed: f32,
    reading: Option<Reading>,
    status: String,
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
    capture: Option<Capture>,
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let voices = speech::voices();
        let voice = if voices.iter().any(|v| v == "af_heart") {
            "af_heart".to_owned()
        } else {
            voices[0].clone()
        };
        let capture = std::env::var_os("VINOSPEAK_GUI_SCREENSHOT").map(|out| Capture {
            out: out.into(),
            speak: std::env::var_os("VINOSPEAK_GUI_SPEAK").is_some(),
            requested: false,
            deadline: Instant::now() + Duration::from_secs(20),
        });
        let query = std::env::var("VINOSPEAK_GUI_QUERY").unwrap_or_default();
        Self {
            edited: (!query.is_empty()).then(|| Instant::now() - DEBOUNCE),
            query,
            searched: String::new(),
            generation: 0,
            searching: false,
            hits: Vec::new(),
            error: None,
            selected: None,
            preview: None,
            voices,
            voice,
            speed: 1.0,
            reading: None,
            status: String::new(),
            tx,
            rx,
            capture,
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
        std::thread::spawn(move || {
            let _ = tx.send(Reply::Results(generation, search::search(&query)));
            ctx.request_repaint();
        });
    }

    fn select(&mut self, ctx: &egui::Context, index: usize) {
        self.selected = Some(index);
        self.preview = None;
        let hit = &self.hits[index];
        if hit.kind != Kind::Text {
            return;
        }
        let (tx, ctx, path) = (self.tx.clone(), ctx.clone(), hit.path.clone());
        std::thread::spawn(move || {
            let text = speech::preview(&path);
            let _ = tx.send(Reply::Preview(path, text));
            ctx.request_repaint();
        });
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Results(generation, _) if generation != self.generation => {}
                Reply::Results(_, Ok(hits)) => {
                    self.searching = false;
                    self.error = None;
                    self.hits = hits;
                    self.selected = None;
                    self.preview = None;
                    if let Some(first) = self.hits.iter().position(|h| h.kind == Kind::Text) {
                        self.select(ctx, first);
                    }
                }
                Reply::Results(_, Err(e)) => {
                    self.searching = false;
                    self.hits.clear();
                    self.error = Some(format!("{e:#}"));
                }
                Reply::Preview(path, text) => {
                    let current = self.selected.map(|i| &self.hits[i].path);
                    if current == Some(&path) {
                        self.preview = Some(
                            text.map(|t| speech::paragraphs(&t))
                                .map_err(|e| format!("{e:#}")),
                        );
                    }
                }
            }
        }
    }

    fn read_aloud(&mut self) {
        let Some(hit) = self.selected.map(|i| &self.hits[i]) else {
            return;
        };
        self.reading = None; // dropping it stops the one before
        match Reading::start(&hit.path, &self.voice, self.speed) {
            Ok(reading) => {
                self.status = format!("Speaking {}", hit.name);
                self.reading = Some(reading);
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn poll_reading(&mut self, ctx: &egui::Context) {
        let Some(reading) = &mut self.reading else {
            return;
        };
        let name = reading
            .file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        match reading.poll() {
            State::Speaking | State::Stopping => {
                ctx.request_repaint_after(Duration::from_millis(250));
                return;
            }
            State::Finished => self.status = format!("Finished {name}"),
            State::Failed(e) => self.status = format!("Couldn't read {name}: {e}"),
        }
        self.reading = None;
    }

    fn capture(&mut self, ctx: &egui::Context) {
        let Some(capture) = &mut self.capture else {
            return;
        };
        for event in ctx.input(|i| i.raw.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let [w, h] = image.size;
                let saved = image::RgbaImage::from_raw(w as u32, h as u32, image.as_raw().to_vec())
                    .map(|img| img.save(&capture.out));
                match saved {
                    Some(Ok(())) => eprintln!("saved {}", capture.out.display()),
                    _ => eprintln!("couldn't save {}", capture.out.display()),
                }
                self.reading = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        let ready = self.preview.is_some()
            || (!self.searching && self.edited.is_none() && self.selected.is_none())
            || Instant::now() > capture.deadline;
        if ready && !capture.requested {
            if capture.speak && self.reading.is_none() && self.preview.is_some() {
                capture.speak = false;
                capture.deadline = Instant::now() + Duration::from_secs(4);
                self.read_aloud();
                return;
            }
            if self.reading.is_some() && Instant::now() < capture.deadline {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            capture.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        ui.horizontal(|ui| {
            ui.label(RichText::new("vinoSpeak").family(theme::bold()).size(22.0));
            ui.label(RichText::new("Find it with vinoSearch, then listen.").color(p.muted));
        });
        ui.add_space(8.0);
        let edit = egui::TextEdit::singleline(&mut self.query)
            .hint_text("Search your files by what's in them…")
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(10, 8));
        let response = ui.add(edit);
        if response.changed() {
            self.edited = Some(Instant::now());
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.edited = Some(Instant::now() - DEBOUNCE);
        }
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let note = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(8.0);
            ui.label(RichText::new(text).color(p.muted));
        };
        if let Some(e) = &self.error {
            note(ui, &format!("vinoSearch didn't answer: {e}"));
            return;
        }
        if self.query.trim().chars().count() < MIN_QUERY_CHARS {
            note(ui, "Type at least three characters.");
            return;
        }
        if self.searching && self.hits.is_empty() {
            note(ui, "Searching…");
            return;
        }
        if self.hits.is_empty() {
            note(ui, "Nothing close enough.");
            return;
        }
        let mut clicked = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, hit) in self.hits.iter().enumerate() {
                let selected = self.selected == Some(i);
                let frame = egui::Frame::new()
                    .inner_margin(Margin::symmetric(10, 8))
                    .corner_radius(CornerRadius::same(6))
                    .fill(if selected {
                        p.code_bg
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::new(
                        1.0_f32,
                        if selected {
                            p.link
                        } else {
                            Color32::TRANSPARENT
                        },
                    ));
                let shown = frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let faded = hit.kind != Kind::Text;
                    let name = RichText::new(&hit.name).family(theme::bold());
                    ui.label(if faded { name.color(p.muted) } else { name });
                    // vinoSearch describes a text hit as "dir:line  snippet".
                    let (place, snippet) = match hit.kind {
                        Kind::Text => hit
                            .description
                            .split_once("  ")
                            .unwrap_or((&hit.description, "")),
                        Kind::Document => ("Document · can't be read aloud yet", ""),
                        Kind::Image => ("Image · nothing to read", ""),
                    };
                    for line in [place, snippet.trim()] {
                        if !line.is_empty() {
                            let text = RichText::new(line).small().color(p.muted);
                            ui.add(egui::Label::new(text).truncate());
                        }
                    }
                });
                let response = ui.interact(shown.response.rect, ui.id().with(i), Sense::click());
                if response.clicked() {
                    clicked = Some(i);
                }
            }
        });
        if let Some(i) = clicked {
            self.select(ui.ctx(), i);
        }
    }

    fn reader(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let Some(hit) = self.selected.map(|i| self.hits[i].clone()) else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Pick a file to hear it.").color(p.muted));
            });
            return;
        };
        ui.label(RichText::new(&hit.name).family(theme::bold()).size(18.0));
        ui.label(
            RichText::new(hit.path.display().to_string())
                .small()
                .color(p.muted),
        );
        ui.add_space(8.0);

        let readable = hit.kind == Kind::Text && matches!(self.preview, Some(Ok(_)));
        let speaking_this = self.reading.as_ref().is_some_and(|r| r.file == hit.path);
        ui.horizontal(|ui| {
            if speaking_this {
                let stop = egui::Button::new(RichText::new("⏹  Stop").color(p.bg)).fill(p.text);
                if ui.add(stop).clicked()
                    && let Some(r) = &mut self.reading
                {
                    r.stop();
                }
                let secs = self
                    .reading
                    .as_ref()
                    .map_or(0, |r| r.started.elapsed().as_secs());
                ui.label(RichText::new(format!("{}:{:02}", secs / 60, secs % 60)).color(p.link));
            } else {
                let play =
                    egui::Button::new(RichText::new("▶  Read aloud").color(p.bg)).fill(p.link);
                if ui.add_enabled(readable, play).clicked() {
                    self.read_aloud();
                }
            }
            ui.separator();
            egui::ComboBox::from_id_salt("voice")
                .selected_text(&self.voice)
                .show_ui(ui, |ui| {
                    for v in &self.voices {
                        ui.selectable_value(&mut self.voice, v.clone(), v);
                    }
                });
            ui.add(
                egui::Slider::new(&mut self.speed, 0.5..=2.0)
                    .step_by(0.1)
                    .fixed_decimals(1)
                    .suffix("×"),
            );
        });
        ui.add_space(6.0);
        ui.separator();

        match (&hit.kind, &self.preview) {
            (Kind::Document, _) => {
                ui.label(RichText::new(
                    "vinoSpeak reads text files for now. Documents need vinoSearch's text extraction, which is next.",
                ).color(p.muted));
            }
            (Kind::Image, _) => {
                ui.label(RichText::new("An image has no text to read.").color(p.muted));
            }
            (_, None) => {
                ui.label(RichText::new("Preparing the text…").color(p.muted));
            }
            (_, Some(Err(e))) => {
                ui.label(RichText::new(format!("Can't read this file: {e}")).color(p.muted));
            }
            (_, Some(Ok(paragraphs))) => {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        // A readable measure, however wide the window.
                        ui.set_max_width(ui.available_width().min(720.0));
                        ui.add_space(4.0);
                        for para in paragraphs {
                            let text = RichText::new(para).size(15.0);
                            if para == "Code skipped." {
                                ui.label(text.italics().color(p.muted));
                            } else {
                                ui.label(text.color(p.text));
                            }
                            ui.add_space(6.0);
                        }
                    });
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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

        let p = theme::palette(ctx);
        let pad = egui::Frame::new().fill(p.bg).inner_margin(Margin::same(16));
        egui::TopBottomPanel::top("header")
            .frame(pad.inner_margin(Margin {
                bottom: 12,
                ..Margin::same(16)
            }))
            .show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("status")
            .frame(pad.inner_margin(Margin::symmetric(16, 6)))
            .show(ctx, |ui| {
                let status = if self.status.is_empty() {
                    format!("Speech by {}", speech::binary().display())
                } else {
                    self.status.clone()
                };
                ui.label(RichText::new(status).small().color(p.muted));
            });
        egui::SidePanel::left("results")
            .resizable(true)
            .default_width(330.0)
            .width_range(260.0..=560.0)
            .frame(pad.inner_margin(Margin {
                top: 4,
                ..Margin::same(12)
            }))
            .show(ctx, |ui| self.results(ui));
        egui::CentralPanel::default()
            .frame(pad.inner_margin(Margin {
                left: 24,
                top: 4,
                ..Margin::same(16)
            }))
            .show(ctx, |ui| self.reader(ui));
        self.capture(ctx);
    }
}
