//! The header: the logo and name, the search field, and About. With vino-ui's
//! own title bar it is also what the window is dragged by, and the window's
//! buttons sit in its top right corner
//! (`the_window_buttons_sit_in_the_top_right_corner` in tests.rs).

use eframe::egui::{self, Align, Layout, Margin, RichText};
use vino_ui::a11y;
use vino_ui::icons::Icon;
use vino_ui::theme;
use vino_ui::widgets;

use super::App;

/// The header's content, from the window's left and right edges.
const INSET: f32 = 20.0;
/// The header's content, from the window's top edge.
const TOP: f32 = 10.0;
const BOTTOM: i8 = 12;
/// The room About takes beside the search field.
const ABOUT_ROOM: f32 = 36.0;
const BRAND_SIZE: egui::Vec2 = egui::vec2(290.0, 36.0);
/// Between the brand, the search field and About.
const GAP: f32 = 16.0;
/// Between About and the window's buttons.
const CONTROLS_GAP: f32 = 12.0;
const SEARCH_WIDTH: std::ops::RangeInclusive<f32> = 160.0..=560.0;

/// With the window's buttons up in the corner, the header starts at the top
/// and right edges and the buttons bring their own gap.
fn margin(app_title_bar: bool) -> Margin {
    let bar_gap = vino_ui::window::BAR_GAP as i8;
    Margin {
        left: INSET as i8,
        right: if app_title_bar { bar_gap } else { INSET as i8 },
        top: if app_title_bar { bar_gap } else { TOP as i8 },
        bottom: BOTTOM,
    }
}

/// The logo, the name and the line under it.
fn brand(ui: &mut egui::Ui, header: theme::Header) {
    ui.allocate_ui_with_layout(BRAND_SIZE, Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        vino_ui::logo::speak(ui, 32.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // Not selectable: a press on them drags the window.
            let title = ui.add(
                egui::Label::new(
                    RichText::new("vinoSpeak")
                        .size(17.0)
                        .family(theme::bold())
                        .color(header.text),
                )
                .selectable(false),
            );
            a11y::heading(&title, "vinoSpeak", 1);
            ui.add(
                egui::Label::new(
                    RichText::new("Find it, then listen.")
                        .size(12.5)
                        .color(header.muted),
                )
                .selectable(false),
            );
        });
    });
}

impl App {
    pub(super) fn header_panel(&mut self, root: &mut egui::Ui) {
        let p = theme::palette(root.ctx());
        let header = theme::header(root.ctx(), self.kde_header.as_ref(), self.focused);
        egui::Panel::top("header")
            .show_separator_line(false)
            .frame(widgets::header_frame(header, p).inner_margin(margin(self.app_title_bar)))
            .show(root, |ui| self.header(ui, header));
    }

    fn header(&mut self, ui: &mut egui::Ui, header: theme::Header) {
        if self.app_title_bar {
            // Before what is in it, so the field and About keep their clicks.
            vino_ui::window::drag(ui, ui.clip_rect(), "header");
            vino_ui::window::corner(ui);
            ui.add_space(TOP - vino_ui::window::BAR_GAP);
        }
        let window_buttons = vino_ui::window::controls_width() + CONTROLS_GAP;
        let right_of_the_field = if self.app_title_bar {
            window_buttons + ABOUT_ROOM + GAP
        } else {
            0.0
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            brand(ui, header);
            let room = ui.available_width() - right_of_the_field;
            self.search_field(ui, room);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.app_title_bar {
                    ui.add_space(window_buttons);
                }
                self.about_button(ui);
            });
        });
    }

    fn search_field(&mut self, ui: &mut egui::Ui, room: f32) {
        let response = widgets::search_field(
            ui,
            &mut self.query,
            "Search your files by what's in them…",
            "Search",
            room.clamp(*SEARCH_WIDTH.start(), *SEARCH_WIDTH.end()),
        );
        if response.changed() {
            self.query_edited();
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.search_now();
        }
    }

    fn about_button(&mut self, ui: &mut egui::Ui) {
        let about = widgets::icon_button(
            ui,
            widgets::ButtonKind::Ghost,
            Icon::Info,
            "About vinoSpeak",
        );
        if about.clicked() {
            self.toggle_about();
        }
    }
}
