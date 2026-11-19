//! The About page: this app, vinopilot.ca and the other vinoPilot apps
//! (vino-ui's) and its Updates row. What can be changed is on Settings.

use std::path::PathBuf;

use eframe::egui;
use vino_ui::a11y;
use vino_ui::update::Updater;
use vino_ui::widgets;

use super::{App, Page};

/// About's Updates row. The release installer leaves itself as
/// `~/.local/bin/vinospeak-upgrade`, which the row's button runs; without it
/// (a build from a checkout) the row links to the release instead.
pub(super) fn updater() -> Updater {
    let updater = Updater::new(vino_ui::App::Speak, crate::version::VERSION);
    match std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/bin/vinospeak-upgrade"))
    {
        Some(upgrade) if upgrade.is_file() => {
            updater.with_command([upgrade.to_string_lossy().into_owned()])
        }
        _ => updater,
    }
}

impl App {
    /// `bar`: the title is up in the window's bar, not here.
    pub(super) fn about(&mut self, ui: &mut egui::Ui, bar: bool) {
        widgets::page_scroll(ui, vino_ui::about::TITLE, widgets::CONTENT_MAX, |ui| {
            if !bar {
                let title = widgets::title(ui, vino_ui::about::TITLE);
                a11y::heading(&title, vino_ui::about::TITLE, 2);
            }
            vino_ui::about::page_with_updates(ui, &mut self.updater);
        });
    }

    pub(super) fn open_about(&mut self) {
        self.page = Page::About;
    }
}
