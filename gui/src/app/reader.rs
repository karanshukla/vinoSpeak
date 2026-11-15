//! The chosen file on the right: its name and path, the player, a session's
//! Summary / Transcript toggle, and the text as vinospeak will say it.

use eframe::egui::{self, Margin, RichText, Stroke};
use vino_ui::a11y;
use vino_ui::theme;
use vino_ui::widgets::{self, Tone};

use super::App;
use crate::search::{Hit, Kind};
use crate::speech::Part;

/// The reading column's widest: about 80 characters of body text.
const MEASURE: f32 = 680.0;
/// A session's parts, as the toggle offers them.
const PARTS: [(Part, &str); 2] = [(Part::Summary, "Summary"), (Part::Transcript, "Transcript")];
/// What vinospeak says in place of a code block (readable.py's CODE_SKIPPED).
const CODE_SKIPPED: &str = "Code skipped.";
const INSET: i8 = 32;

fn kind_tag(ui: &mut egui::Ui, kind: Kind) {
    let label = match kind {
        Kind::Document => "Document",
        Kind::Session { .. } => "Session",
        _ => return,
    };
    widgets::tag(ui, Tone::Neutral, label);
}

/// One paragraph of the preview; the stand-in for a code block is set apart.
fn paragraph(ui: &mut egui::Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    let stand_in = text == CODE_SKIPPED;
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(15.0),
            color: if stand_in { p.muted } else { p.text },
            italics: stand_in,
            line_height: Some(25.0),
            ..Default::default()
        },
    );
    ui.add(egui::Label::new(job).wrap());
}

impl App {
    pub(super) fn reader(&mut self, ui: &mut egui::Ui) {
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
                left: INSET,
                right: INSET,
                top: 20,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                title(ui, &hit);
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
                left: INSET,
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
                    for text in &preview.paragraphs {
                        paragraph(ui, text);
                    }
                });
            }
        }
    }
}

/// The file's name with what kind it is, and its path under it.
fn title(ui: &mut egui::Ui, hit: &Hit) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let title = ui.add(
            egui::Label::new(RichText::new(&hit.name).size(22.0).family(theme::bold())).truncate(),
        );
        a11y::heading(&title, &hit.name, 2);
        kind_tag(ui, hit.kind);
    });
    let path = hit.path.display().to_string();
    ui.add(egui::Label::new(RichText::new(&path).monospace().color(p.muted)).truncate())
        .on_hover_text(path);
}
