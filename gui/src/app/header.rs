//! A narrow window's header, in place of the sidebar: the logo and name,
//! then Files, Reading and About as tabs. With vino-ui's own title bar it is
//! what the window is dragged by, and the window's buttons sit in its top
//! right corner (pinned by
//! `app::tests::the_window_buttons_sit_in_the_top_right_corner`).
//!
//! Also the search row, which the sidebar and the Files tab share.

use eframe::egui::{self, Margin, RichText};
use vino_ui::a11y;
use vino_ui::icons::Icon;
use vino_ui::theme;
use vino_ui::widgets;

use super::App;

/// Between the search field and Open.
const GAP: f32 = 8.0;

/// The narrow page's tabs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Files,
    Reading,
    About,
}

impl App {
    pub(super) fn header_panel(&mut self, root: &mut egui::Ui) {
        let p = theme::palette(root.ctx());
        let header = theme::header(root.ctx(), self.kde_header.as_ref(), self.focused);
        egui::Panel::top("header")
            .show_separator_line(false)
            .frame(widgets::header_frame(header, p).inner_margin(Margin {
                left: 16,
                right: 16,
                top: 8,
                bottom: 0,
            }))
            .show(root, |ui| self.header(ui, header));
    }

    /// The drag area goes in first, under the tabs, or it would take their
    /// clicks (pinned by
    /// `app::tests::with_the_app_title_bar_about_still_takes_a_click`).
    fn header(&mut self, ui: &mut egui::Ui, header: theme::Header) {
        if self.app_title_bar {
            vino_ui::window::drag(ui, ui.clip_rect(), "header");
            vino_ui::window::corner(ui);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            vino_ui::logo::speak(ui, 22.0);
            let name = ui.add(
                egui::Label::new(
                    RichText::new("vinoSpeak")
                        .size(15.0)
                        .family(theme::bold())
                        .color(header.text),
                )
                .selectable(false),
            );
            a11y::heading(&name, "vinoSpeak", 1);
        });
        // Wrapped: at the desktop's largest text the tabs don't fit on one line.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(18.0, 0.0);
            a11y::tab_list(ui, "Pages");
            let shown = self.tab();
            let tabs = [
                (Tab::Files, "Files", true),
                (Tab::Reading, "Reading", self.selected.is_some()),
                (Tab::About, vino_ui::about::TITLE, true),
            ];
            for (tab, name, enabled) in tabs {
                if widgets::tab(ui, name, shown == tab, enabled, header).clicked() {
                    self.show_tab(tab);
                }
            }
        });
    }

    fn tab(&self) -> Tab {
        if self.listing {
            Tab::Files
        } else if self.about {
            Tab::About
        } else {
            Tab::Reading
        }
    }

    fn show_tab(&mut self, tab: Tab) {
        self.listing = tab == Tab::Files;
        match tab {
            Tab::About => self.open_about(),
            _ => self.about = false,
        }
    }

    /// The search field and Open, filling the row.
    pub(super) fn search_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let open = widgets::CONTROL_HEIGHT;
            self.search_field(ui, ui.available_width() - open - GAP);
            self.open_button(ui);
        });
    }

    fn search_field(&mut self, ui: &mut egui::Ui, width: f32) {
        let can_search = self.vinosearch_missing.is_none();
        let hint = if can_search {
            "Search by what's in them…"
        } else {
            "Searching needs vinoSearch"
        };
        let response = ui
            .add_enabled_ui(can_search, |ui| {
                widgets::search_field(ui, &mut self.query, hint, "Search", width)
            })
            .inner;
        if response.changed() {
            self.query_edited();
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.search_now();
        }
    }

    fn open_button(&mut self, ui: &mut egui::Ui) {
        let open = widgets::icon_button(
            ui,
            widgets::ButtonKind::Secondary,
            Icon::Folder,
            "Open a file",
        );
        if open.clicked() {
            self.ask_for_a_file(ui.ctx());
        }
    }
}
