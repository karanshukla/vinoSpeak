//! Which files are listed and which one is chosen: the search once typing
//! pauses, the recent files, and the chosen file's preview. Each is fetched
//! on a thread and comes back as a `Reply`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use super::{App, Preview, Reply};
use crate::search::{self, Hit, Index, Kind};
use crate::sessions;
use crate::speech::{self, Part, Source};

/// vinoSearch answers nothing under three characters (pinned by
/// `tests::three_characters_are_searched_and_two_are_not`).
const MIN_QUERY_CHARS: usize = 3;
/// Search once typing pauses this long, not on every key.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// How many recent files fill the list before a search (pinned by
/// `app::tests::the_recent_list_stops_at_twenty`).
const RECENT_LISTED: usize = 20;
/// vinoSearch's most (its recent::MAX): asking for more than are listed keeps
/// the list full when the newest files are ones not worth hearing (pinned by
/// `app::tests::spreadsheets_and_code_are_not_listed`).
const RECENT_ASKED: u32 = 50;

/// Whether a query is one vinoSearch would answer.
pub(super) fn long_enough(query: &str) -> bool {
    query.trim().chars().count() >= MIN_QUERY_CHARS
}

/// Up to `most` of the hits that belong in the window's lists, sessions titled.
fn worth_listing(hits: Vec<Hit>, most: usize) -> Vec<Hit> {
    search::title_sessions(
        hits.into_iter()
            .filter(search::worth_hearing)
            .take(most)
            .collect(),
    )
}

/// What vinospeak is given to read a hit: a document's text comes from
/// vinoSearch, a session and any other file are opened by vinospeak itself.
fn source(index: &dyn Index, hit: &Hit, part: Part) -> anyhow::Result<Source> {
    match hit.kind {
        Kind::Document => index.file_text(&hit.path).map(Source::Text),
        Kind::Session { .. } => Ok(Source::Session(hit.path.clone(), part)),
        _ => Ok(Source::File(hit.path.clone())),
    }
}

impl App {
    /// Set the search box as if typed, searching on the next frame.
    pub(super) fn type_query(&mut self, query: &str) {
        self.query = query.to_owned();
        self.search_now();
    }

    pub(super) fn query_edited(&mut self) {
        self.edited = Some(Instant::now());
    }

    /// Skip what is left of the pause, as Enter does.
    pub(super) fn search_now(&mut self) {
        self.edited = Some(Instant::now() - DEBOUNCE);
    }

    /// Search results once there's a query that found something, else the
    /// recent files.
    pub(super) fn showing_hits(&self) -> bool {
        long_enough(&self.query) && !self.hits.is_empty()
    }

    pub(super) fn listed(&self) -> &[Hit] {
        if self.showing_hits() {
            &self.hits
        } else {
            &self.recent
        }
    }

    pub(super) fn search_when_typing_pauses(&mut self, ctx: &egui::Context) {
        let Some(edited) = self.edited else {
            return;
        };
        let paused = edited.elapsed();
        if paused < DEBOUNCE {
            ctx.request_repaint_after(DEBOUNCE - paused);
            return;
        }
        self.edited = None;
        if self.vinosearch_missing.is_some() {
            return;
        }
        if long_enough(&self.query) && self.query.trim() != self.searched {
            self.search(ctx);
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
            let hits = index
                .search(&query)
                .map(|hits| worth_listing(hits, usize::MAX));
            let _ = tx.send(Reply::Results(generation, hits));
            ctx.request_repaint();
        });
    }

    pub(super) fn load_recent(&mut self, ctx: &egui::Context) {
        self.recent_asked = true;
        let (tx, ctx, index) = (self.tx.clone(), ctx.clone(), self.index.clone());
        let dirs = self.session_dirs.clone();
        std::thread::spawn(move || {
            let (indexed, missing) = match index.recent(RECENT_ASKED) {
                Ok(hits) => (hits, None),
                Err(e) => (Vec::new(), Some(format!("{e:#}"))),
            };
            let recent = sessions::merge(indexed, sessions::found(&dirs));
            let _ = tx.send(Reply::Recent(worth_listing(recent, RECENT_LISTED), missing));
            ctx.request_repaint();
        });
    }

    pub(super) fn select(&mut self, ctx: &egui::Context, hit: Hit) {
        self.part = match hit.kind {
            Kind::Session { summarised: false } => Part::Transcript,
            _ => Part::Summary,
        };
        self.selected = Some(hit);
        self.page = super::Page::Reader;
        self.load_preview(ctx);
    }

    /// The selected file's text, as vinospeak will say it.
    pub(super) fn load_preview(&mut self, ctx: &egui::Context) {
        self.preview = None;
        self.previewed += 1;
        let Some(hit) = self.selected.clone().filter(|h| h.kind != Kind::Image) else {
            return;
        };
        let (tx, ctx, previewed, part) = (self.tx.clone(), ctx.clone(), self.previewed, self.part);
        let (index, vinospeak) = (self.index.clone(), self.vinospeak.clone());
        std::thread::spawn(move || {
            let text = source(index.as_ref(), &hit, part)
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

    pub(super) fn on_results(
        &mut self,
        ctx: &egui::Context,
        generation: u64,
        hits: anyhow::Result<Vec<Hit>>,
    ) {
        let superseded = generation != self.generation;
        if superseded {
            return;
        }
        self.searching = false;
        match hits {
            Ok(hits) => {
                self.error = None;
                self.hits = hits;
                if self.showing_hits() {
                    self.select_first(ctx);
                }
            }
            Err(e) => {
                self.hits.clear();
                self.error = Some(format!("{e:#}"));
            }
        }
    }

    /// No file picked, nothing searched, and no About opened before the
    /// recent files came in (pinned by
    /// `app::tests::before_a_search_the_recent_files_show_and_the_first_is_previewed`,
    /// `app::tests::about_opened_before_the_recent_files_arrive_stays_open`).
    fn nothing_chosen_yet(&self) -> bool {
        self.selected.is_none() && self.query.is_empty() && self.page == super::Page::Reader
    }

    pub(super) fn on_recent(
        &mut self,
        ctx: &egui::Context,
        recent: Vec<Hit>,
        missing: Option<String>,
    ) {
        self.recent = recent;
        self.vinosearch_missing = missing;
        if self.nothing_chosen_yet() {
            self.select_first(ctx);
        }
    }

    /// Ask the desktop's file dialog for a file; it answers as `Reply::Opened`.
    pub(super) fn ask_for_a_file(&self, ctx: &egui::Context) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Reply::Opened(crate::open::dialog()));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_opened(
        &mut self,
        ctx: &egui::Context,
        chosen: anyhow::Result<Option<PathBuf>>,
    ) {
        match chosen {
            Ok(Some(path)) => self.open(ctx, path),
            Ok(None) => {}
            Err(e) => self.status = format!("Couldn't open file dialog: {e:#}"),
        }
    }

    /// A file dropped on the window, or named on the command line.
    pub(super) fn open_dropped(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|path| !path.as_os_str().is_empty())
        });
        if let Some(path) = dropped.or_else(|| self.to_open.take()) {
            self.open(ctx, path);
        }
    }

    /// Preview a file that isn't in the list, as if it were.
    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        let hit = Hit {
            name: path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            description: path
                .parent()
                .map_or_else(String::new, |d| d.display().to_string()),
            kind: search::kind_of(&path, ""),
            path,
        };
        if let Some(hit) = search::title_sessions(vec![hit]).pop() {
            self.select(ctx, hit);
        }
    }

    pub(super) fn on_preview(
        &mut self,
        ctx: &egui::Context,
        previewed: u64,
        text: anyhow::Result<String>,
    ) {
        let for_another_file_or_part = previewed != self.previewed;
        if for_another_file_or_part {
            return;
        }
        let preview = text
            .map(|text| Preview {
                paragraphs: speech::paragraphs(&text),
                text,
            })
            .map_err(|e| format!("{e:#}"));
        let readable = preview.is_ok();
        self.preview = Some(preview);
        if readable {
            self.start_worker(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_characters_are_searched_and_two_are_not() {
        assert!(long_enough("npu"));
        assert!(!long_enough("np"));
    }

    #[test]
    fn a_query_is_measured_in_characters_without_the_spaces_round_it() {
        assert!(long_enough("éàü"));
        assert!(!long_enough("éà"), "four bytes, two characters");
        assert!(!long_enough("  ab  "));
    }
}
