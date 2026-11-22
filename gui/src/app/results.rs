//! The list in the sidebar, or a narrow window's Files tab: vinoSearch's
//! hits, or the files changed most recently, one row each.

use eframe::egui::{self, Margin, RichText, Sense, Stroke};
use vino_ui::a11y;
use vino_ui::family;
use vino_ui::file_tile::{self, Format};
use vino_ui::icons::Icon;
use vino_ui::theme;
use vino_ui::widgets::{self, Button, Tone};

use super::App;
use super::files::long_enough;
use crate::search::{Hit, Kind};

const ROW_RADIUS: f32 = 10.0;
const TILE: f32 = 30.0;

/// The line under a file's name. vinoSearch describes a hit as
/// "dir:line  snippet" and a recent file as "dir  Changed ... ago"; the two
/// spaces become a dot.
fn subtitle(hit: &Hit) -> String {
    if hit.kind == Kind::Image {
        return "Image · No readable text".to_owned();
    }
    match hit.description.split_once("  ") {
        Some((place, detail)) => format!("{place} · {}", detail.trim()),
        None => hit.description.clone(),
    }
}

fn tile(ui: &mut egui::Ui, hit: &Hit) {
    match hit.kind {
        Kind::Text | Kind::Document => file_tile::file(ui, Format::of(&hit.path), TILE),
        Kind::Image => widgets::file_tile(ui, Icon::Image, TILE),
        // Not Icon::Sessions: vino-ui draws that as FileText.
        Kind::Session { .. } => vino_ui::logo::whisper(ui, TILE),
    };
}

/// A file's tile, name and subtitle; an image, which can't be read, is faded.
fn row(ui: &mut egui::Ui, hit: &Hit, subtitle: &str) -> egui::Rect {
    let p = theme::palette(ui.ctx());
    let faded = hit.kind == Kind::Image;
    let name = RichText::new(&hit.name).family(if faded {
        theme::medium()
    } else {
        theme::semibold()
    });
    let shown = egui::Frame::new()
        .inner_margin(Margin::symmetric(10, 9))
        .corner_radius(ROW_RADIUS)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                tile(ui, hit);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(
                        egui::Label::new(if faded { name.color(p.muted) } else { name })
                            .truncate()
                            .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(RichText::new(subtitle).size(12.5).color(p.muted))
                            .truncate()
                            .selectable(false),
                    );
                });
            });
        });
    shown.response.rect
}

/// The row's fill and outlines, painted into `background` under its content.
fn highlight(
    ui: &egui::Ui,
    background: egui::layers::ShapeIdx,
    rect: egui::Rect,
    response: &egui::Response,
    chosen: bool,
) {
    let p = theme::palette(ui.ctx());
    let outline = |width: f32| {
        ui.painter().rect_stroke(
            rect,
            ROW_RADIUS,
            Stroke::new(width, p.accent),
            egui::StrokeKind::Inside,
        );
    };
    let fill = |color| {
        ui.painter().set(
            background,
            egui::Shape::rect_filled(rect, ROW_RADIUS, color),
        );
    };
    if chosen {
        fill(p.tint);
        outline(1.0);
    } else if response.hovered() {
        fill(p.field);
    }
    if response.has_focus() {
        outline(2.0);
    }
}

impl App {
    /// A narrow window's Files tab: the search and the list.
    pub(super) fn files_page(&mut self, ui: &mut egui::Ui) {
        self.search_row(ui);
        self.results(ui);
    }

    /// What the search is doing, when that isn't a list of hits.
    fn search_note(&self) -> Option<String> {
        let typed = long_enough(&self.query);
        let still_typing = self.edited.is_some();
        if let Some(e) = &self.error {
            Some(format!("Could not reach vinoSearch: {e}"))
        } else if typed && self.searching {
            Some("Searching…".to_owned())
        } else if typed && self.hits.is_empty() && !still_typing {
            Some("No matching files found.".to_owned())
        } else {
            None
        }
    }

    pub(super) fn results(&mut self, ui: &mut egui::Ui) {
        if self.vinosearch_missing.is_some() {
            ui.add_space(10.0);
            widgets::banner(
                ui,
                Tone::Info,
                "Search and PDFs require vinoSearch.",
                |ui| {
                    if ui
                        .add(Button::secondary("Get vinoSearch").small())
                        .clicked()
                    {
                        let _ = family::open(family::App::Search.page());
                    }
                },
            );
        } else if let Some(note) = self.search_note() {
            ui.add_space(10.0);
            a11y::live(&widgets::muted(ui, note));
        }
        let (heading, empty) = match (self.showing_hits(), self.vinosearch_missing.is_some()) {
            (true, _) => ("Results", ""),
            (false, false) => ("Recently changed", "No files indexed yet."),
            (false, true) => (
                "vinoWhisper sessions",
                "No sessions found. Use the folder button to open a file.",
            ),
        };
        widgets::group_label(ui, heading);
        if !self.showing_hits() && self.recent.is_empty() {
            widgets::muted(ui, empty);
            return;
        }
        if let Some(hit) = self.file_list(ui) {
            self.select(ui.ctx(), hit);
            self.listing = false;
        }
    }

    /// The listed files as a list box; the one clicked this frame.
    fn file_list(&self, ui: &mut egui::Ui) -> Option<Hit> {
        let selected = self.selected.as_ref().map(|h| &h.path);
        let mut clicked = None;
        widgets::page_scroll(ui, "the files", f32::INFINITY, |ui| {
            a11y::list_box(ui, "Files");
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, hit) in self.listed().iter().enumerate() {
                let chosen = selected == Some(&hit.path);
                let subtitle = subtitle(hit);
                let background = ui.painter().add(egui::Shape::Noop);
                let rect = row(ui, hit, &subtitle);
                let response = ui.interact(rect, ui.scope_id().with(i), Sense::click());
                highlight(ui, background, rect, &response, chosen);
                a11y::list_box_option(&response, &format!("{}, {subtitle}", hit.name), chosen);
                if response.clicked() {
                    clicked = Some(hit.clone());
                }
            }
        });
        clicked
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn hit(description: &str, kind: Kind) -> Hit {
        Hit {
            path: PathBuf::from("/a/b"),
            name: "b".to_owned(),
            description: description.to_owned(),
            kind,
        }
    }

    #[test]
    fn a_hits_place_and_snippet_are_split_by_a_dot() {
        let found = hit("~/notes:12  the NPU compiles once ", Kind::Text);
        assert_eq!(subtitle(&found), "~/notes:12 · the NPU compiles once");
        let recent = hit("~/notes  Changed 2 hours ago", Kind::Document);
        assert_eq!(subtitle(&recent), "~/notes · Changed 2 hours ago");
    }

    #[test]
    fn a_description_without_the_two_spaces_is_shown_as_it_is() {
        assert_eq!(
            subtitle(&hit("~/notes one space", Kind::Text)),
            "~/notes one space"
        );
    }

    #[test]
    fn an_image_says_there_is_nothing_to_read() {
        let image = hit("~/pictures  Changed just now", Kind::Image);
        assert_eq!(subtitle(&image), "Image · No readable text");
    }
}
