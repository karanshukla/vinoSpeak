//! The chosen file on the right: its name and path, the player, a session's
//! Summary / Transcript toggle, and the text as vinospeak will say it.

use eframe::egui::{self, Margin, RichText, Stroke};
use vino_ui::a11y;
use vino_ui::theme;
use vino_ui::widgets::{self, Tone};

use super::App;
use crate::search::{Hit, Kind};
use crate::speech::{self, Part, Translated};
use crate::translation;

/// The reading column's widest: about 80 characters of body text.
const MEASURE: f32 = 680.0;
/// A session's parts, as the toggle offers them.
const PARTS: [(Part, &str); 2] = [(Part::Summary, "Summary"), (Part::Transcript, "Transcript")];
/// What vinospeak says in place of a code block (readable.py's CODE_SKIPPED).
const CODE_SKIPPED: &str = "Code skipped.";
/// A translated reading's text, as the toggle offers it: the original or not.
const SHOWN: [(bool, &str); 2] = [(false, "Translation"), (true, "Original")];

fn kind_tag(ui: &mut egui::Ui, kind: Kind) {
    let label = match kind {
        Kind::Document => "Document",
        Kind::Session { .. } => "Session",
        _ => return,
    };
    widgets::tag(ui, Tone::Neutral, label);
}

/// One paragraph of the preview; the stand-in for a code block is set apart.
fn paragraph(ui: &mut egui::Ui, text: &str) -> egui::Response {
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
    ui.add(egui::Label::new(job).wrap())
}

/// A translated piece's paragraphs, tinted while it is the one being spoken
/// and marked selected for a screen reader. Every piece keeps the same margin,
/// so the text doesn't move as the tint does.
fn piece(ui: &mut egui::Ui, text: &str, spoken: bool) {
    let p = theme::palette(ui.ctx());
    egui::Frame::NONE
        .fill(if spoken {
            p.tint
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(6)
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 12.0;
            for text in speech::paragraphs(text) {
                let shown = paragraph(ui, &text);
                if spoken {
                    ui.ctx()
                        .accesskit_node_builder(shown.id, |node| node.set_selected(true));
                }
            }
        });
}

impl App {
    /// `bar`: the file's name is up in the window's bar, not here.
    pub(super) fn reader(&mut self, ui: &mut egui::Ui, inset: f32, top: i8, bar: bool) {
        let p = theme::palette(ui.ctx());
        let inset = inset as i8;
        let Some(hit) = self.selected.clone() else {
            egui::Frame::NONE
                .inner_margin(Margin {
                    left: inset,
                    top: top + 20,
                    ..Margin::ZERO
                })
                .show(ui, |ui| {
                    widgets::muted(ui, "Choose a file from the list to start reading.");
                    self.status_line(ui);
                });
            return;
        };
        egui::Frame::NONE
            .inner_margin(Margin {
                left: inset,
                right: inset,
                top,
                bottom: 16,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                if !bar {
                    ui.horizontal(|ui| title_line(ui, &hit));
                }
                path_line(ui, &hit);
                ui.add_space(12.0);
                self.player(ui, &hit);
                self.status_line(ui);
            });
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            ui.cursor().top(),
            Stroke::new(1.0_f32, p.rule),
        );
        egui::Frame::NONE
            .inner_margin(Margin {
                left: inset,
                right: 0,
                top: 20,
                bottom: 0,
            })
            .show(ui, |ui| {
                if let Kind::Session { summarised } = hit.kind {
                    self.part_toggle(ui, summarised);
                    ui.add_space(12.0);
                }
                if let Some(into) = self.translation_of(&hit).map(|t| t.into) {
                    self.translation_toggle(ui, into);
                    ui.add_space(12.0);
                }
                match self.translation_of(&hit) {
                    Some(translated) if !self.show_original => {
                        self.translated_text(ui, translated);
                    }
                    _ => self.text(ui, &hit),
                }
            });
    }

    /// What the last reading did, or why it failed, under the player.
    fn status_line(&self, ui: &mut egui::Ui) {
        if !self.status.is_empty() {
            ui.add_space(8.0);
            a11y::live(&widgets::small(ui, &self.status));
        }
    }

    /// Summary or Transcript, for a session. Without a summary the toggle
    /// stays on Transcript, disabled: vino-ui's segmented can't disable one
    /// option alone.
    fn part_toggle(&mut self, ui: &mut egui::Ui, summarised: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let mut part = self.part;
            let changed = ui
                .add_enabled_ui(summarised, |ui| {
                    widgets::button_group(ui, "Part", &mut part, &PARTS)
                })
                .inner;
            if !summarised {
                widgets::muted(ui, "No summary yet");
            }
            if changed {
                self.part = part;
                self.load_preview(ui.ctx());
            }
        });
    }

    /// The last translated reading's text, if it was of this file and part.
    fn translation_of(&self, hit: &Hit) -> Option<&Translated> {
        self.translated
            .as_ref()
            .filter(|t| t.file == hit.path && t.part == self.part)
    }

    fn translation_toggle(&mut self, ui: &mut egui::Ui, into: translation::Target) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            widgets::button_group(ui, "Text shown", &mut self.show_original, &SHOWN);
            widgets::muted(
                ui,
                format!(
                    "{} (translated locally by {})",
                    into.name,
                    translation::LABEL
                ),
            );
        });
    }

    /// The piece being spoken: the one whose source the reading's progress
    /// is in, once it is heard (pinned by
    /// `app::tests::the_translation_shows_as_it_arrives_with_the_spoken_piece_marked`).
    fn spoken(&self, translated: &Translated) -> Option<usize> {
        let reading = self
            .reading
            .as_ref()
            .filter(|r| r.id == translated.id && r.heard)?;
        translated
            .pieces
            .iter()
            .rposition(|p| p.start <= reading.done && reading.done < p.end)
    }

    fn translated_text(&self, ui: &mut egui::Ui, translated: &Translated) {
        let under_way = self.reading.as_ref().is_some_and(|r| r.id == translated.id);
        let rest = translated.pieces.last().map_or(0.0, |p| p.end) < 1.0;
        if translated.pieces.is_empty() {
            let says = if under_way {
                format!("Translating into {}…", translated.into.name)
            } else {
                "Stopped before translation started.".to_owned()
            };
            a11y::live(&widgets::muted(ui, says));
            return;
        }
        let spoken = self.spoken(translated);
        widgets::page_scroll(ui, "the translation", MEASURE, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for (n, shown) in translated.pieces.iter().enumerate() {
                piece(ui, &shown.text, spoken == Some(n));
            }
            if rest {
                ui.add_space(8.0);
                widgets::muted(
                    ui,
                    if under_way {
                        "Translating remaining text…"
                    } else {
                        "Stopped before translation finished."
                    },
                );
            }
        });
    }

    fn text(&self, ui: &mut egui::Ui, hit: &Hit) {
        match (&hit.kind, &self.preview) {
            (Kind::Image, _) => {
                widgets::muted(ui, "Images do not contain readable text.");
            }
            (Kind::Document, None) => {
                a11y::live(&widgets::muted(ui, "Extracting text via vinoSearch…"));
            }
            (_, None) => {
                a11y::live(&widgets::muted(ui, "Loading text…"));
            }
            (_, Some(Err(e))) => {
                a11y::live(&widgets::muted(ui, format!("Cannot read file: {e}")));
            }
            (_, Some(Ok(preview))) if preview.paragraphs.is_empty() => {
                widgets::muted(ui, "This file has no readable text (possibly a scan).");
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

/// The file's name with what kind it is, in a horizontal `ui`: the page's
/// title, here or up in the window's bar.
pub(super) fn title_line(ui: &mut egui::Ui, hit: &Hit) {
    ui.spacing_mut().item_spacing.x = 10.0;
    let title = ui.add(
        egui::Label::new(RichText::new(&hit.name).size(22.0).family(theme::bold()))
            .truncate()
            .selectable(false),
    );
    a11y::heading(&title, &hit.name, 2);
    kind_tag(ui, hit.kind);
}

fn path_line(ui: &mut egui::Ui, hit: &Hit) {
    let p = theme::palette(ui.ctx());
    let path = hit.path.display().to_string();
    ui.add(egui::Label::new(RichText::new(&path).monospace().color(p.muted)).truncate())
        .on_hover_text(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same file tests/test_readable.py checks readable.py's CODE_SKIPPED against.
    #[test]
    fn the_code_stand_in_is_what_vinospeak_prints_for_a_code_block() {
        let table = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/shared_with_window.json");
        let shared: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(table).unwrap()).unwrap();
        assert_eq!(shared["code_skipped"], CODE_SKIPPED);
    }
}
