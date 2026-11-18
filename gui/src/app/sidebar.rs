//! A wide window's sidebar, as the other vinoPilot apps have one: the logo
//! and name, the search, the list, and About at its foot. With vino-ui's own
//! title bar its brand is a handle the window is dragged by.

use eframe::egui::{self, Margin};
use vino_ui::a11y;
use vino_ui::icons::Icon;
use vino_ui::theme;
use vino_ui::widgets::{self, Edge};

use super::App;

/// Wider than vino-ui's sidebar of pages: this one lists files by name.
const WIDTH: f32 = 300.0;
const WIDTHS: std::ops::RangeInclusive<f32> = 240.0..=480.0;

impl App {
    pub(super) fn sidebar_panel(&mut self, root: &mut egui::Ui) {
        let p = theme::palette(root.ctx());
        egui::Panel::left("sidebar")
            .resizable(true)
            .show_separator_line(false)
            .default_size(WIDTH)
            .size_range(WIDTHS)
            .frame(widgets::sidebar_frame(p))
            .show(root, |ui| {
                egui::Panel::bottom("sidebar-foot")
                    .show_separator_line(false)
                    .frame(egui::Frame::NONE.inner_margin(Margin {
                        top: 8,
                        ..Margin::ZERO
                    }))
                    .show(ui, |ui| self.about_nav(ui));
                ui.spacing_mut().item_spacing.y = 2.0;
                self.brand(ui);
                ui.add_space(14.0);
                self.search_row(ui);
                self.results(ui);
                widgets::panel_rule(ui, Edge::Left);
            });
    }

    /// Drawn before the drag area, so its name drags rather than selects.
    fn brand(&self, ui: &mut egui::Ui) {
        let brand = ui
            .scope(|ui| {
                widgets::sidebar_brand(
                    ui,
                    Some(vino_ui::logo::speak),
                    "vinoSpeak",
                    "Find it, then listen.",
                );
            })
            .response
            .rect;
        if self.app_title_bar {
            let clip = ui.clip_rect();
            vino_ui::window::drag(
                ui,
                egui::Rect::from_x_y_ranges(clip.x_range(), clip.top()..=brand.bottom()),
                "brand",
            );
        }
    }

    fn about_nav(&mut self, ui: &mut egui::Ui) {
        a11y::tab_list(ui, "Pages");
        let title = vino_ui::about::TITLE;
        if widgets::nav_item(ui, Icon::Info, title, title, self.about, None).clicked() {
            self.toggle_about();
        }
    }
}
