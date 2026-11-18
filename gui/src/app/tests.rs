//! The window driven headless through egui_kittest, finding widgets by their
//! AccessKit labels as a screen reader would. vinoSearch is a fake `Index`;
//! vinospeak is a shell script that prints `--print-text`, and as `--serve`
//! logs each request and "plays" until it's stopped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::App;
use crate::outputs::Output;
use crate::search::{Hit, Index, Kind};
use crate::voices;
use vino_ui::theme;

const FAKE_VINOSPEAK: &str = r#"#!/bin/sh
# argv: --serve [--device D], --delete-model, or --file PATH|- [--kind document | --part P] --print-text
dir=$(dirname "$0")
echo "$*" >> "$dir/calls"
if [ "$1" = "--delete-model" ]; then
    [ -f "$dir/fail-delete" ] && { echo "vinospeak: error: The model is downloading." >&2; exit 2; }
    rm -rf "$dir/cache/kokoro-82m-int8-ov" "$dir/cache/ov-cache"
    echo "Deleted the model and its compiled cache (2 MB)."
    exit 0
fi
if [ "$1" = "--serve" ]; then
    echo '{"event": "ready"}'
    [ -f "$dir/exit-idle" ] && exit 0
    current=
    while read -r line; do
        printf '%s\n' "$line" >> "$dir/calls"
        if [ -n "$current" ]; then
            echo interrupted >> "$dir/calls"
            echo "{\"event\": \"done\", \"id\": $current, \"stopped\": true}"
            current=
        fi
        case "$line" in *'"op":"speak"'*) ;; *) continue ;; esac
        id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
        [ -f "$dir/crash-read" ] && { echo "vinospeak: error: crashed" >&2; exit 1; }
        if [ -f "$dir/fail-read" ]; then
            echo "{\"event\": \"error\", \"id\": $id, \"message\": \"boom\"}"
            continue
        fi
        echo "{\"event\": \"first-audio\", \"id\": $id, \"ms\": 5}"
        if [ -f "$dir/quick" ]; then
            echo "{\"event\": \"done\", \"id\": $id, \"stopped\": false}"
        else
            current=$id
        fi
    done
    exit 0
fi
[ -f "$dir/fail-preview" ] && { echo "vinospeak: error: not a UTF-8 text file." >&2; exit 2; }
if [ "$3" = "--part" ]; then echo "The $4 of the session."
elif [ "$2" = "-" ]; then cat; else cat "$2"; fi
"#;

/// A scratch directory with the fake vinospeak and the files the hits point
/// at, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vinospeak-gui-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("vinospeak");
        std::fs::write(&script, FAKE_VINOSPEAK).unwrap();
        let executable = std::os::unix::fs::PermissionsExt::from_mode(0o755);
        std::fs::set_permissions(&script, executable).unwrap();
        Self(dir)
    }

    fn file(&self, name: &str, text: &str) -> Hit {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        Hit {
            name: name.to_owned(),
            description: self.changed_just_now(),
            kind: if name.ends_with(".pdf") {
                Kind::Document
            } else {
                Kind::Text
            },
            path,
        }
    }

    /// One of vinoWhisper's session fixtures, as vinoSearch lists it: by file name.
    fn session(&self, name: &str, fixture: &str) -> Hit {
        let path = self.0.join(name);
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures");
        std::fs::copy(fixtures.join(fixture), &path).unwrap();
        Hit {
            name: name.to_owned(),
            description: self.changed_just_now(),
            kind: crate::search::kind_of(&path, ""),
            path,
        }
    }

    /// As RecentFiles describes a file: its text shows only in the preview.
    fn changed_just_now(&self) -> String {
        format!("{}  Changed just now", self.0.display())
    }

    fn image(&self, name: &str) -> Hit {
        Hit {
            path: self.0.join(name),
            name: name.to_owned(),
            description: self.0.display().to_string(),
            kind: Kind::Image,
        }
    }

    /// Where the window looks for vinoWhisper's sessions itself.
    fn sessions(&self) -> PathBuf {
        self.0.join("sessions")
    }

    /// One of vinoWhisper's session fixtures, only on disk: vinoSearch doesn't list it.
    fn session_on_disk(&self, name: &str, fixture: &str) -> PathBuf {
        std::fs::create_dir_all(self.sessions()).unwrap();
        let path = self.sessions().join(name);
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures");
        std::fs::copy(fixtures.join(fixture), &path).unwrap();
        path
    }

    fn mode(&self, flag: &str) {
        std::fs::write(self.0.join(flag), "").unwrap();
    }

    /// A downloaded model with three voices and 2 MB of compiled cache.
    fn model(&self) {
        let voices = self.0.join("cache/kokoro-82m-int8-ov/voices");
        std::fs::create_dir_all(&voices).unwrap();
        for voice in ["af_heart", "bm_george", "ef_dora"] {
            std::fs::write(voices.join(format!("{voice}.bin")), "").unwrap();
        }
        std::fs::create_dir_all(self.0.join("cache/ov-cache")).unwrap();
        std::fs::write(self.0.join("cache/ov-cache/blob"), vec![0; 2_000_000]).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.0.join(name)).unwrap_or_default()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct FakeIndex {
    recent: Vec<Hit>,
    searches: HashMap<String, Vec<Hit>>,
    documents: HashMap<PathBuf, String>,
    down: bool,
}

impl FakeIndex {
    fn recent(recent: Vec<Hit>) -> Self {
        Self {
            recent,
            ..Default::default()
        }
    }
}

impl Index for FakeIndex {
    fn search(&self, query: &str) -> Result<Vec<Hit>> {
        if self.down {
            bail!("vinoSearch isn't running or isn't installed");
        }
        Ok(self.searches.get(query).cloned().unwrap_or_default())
    }

    fn recent(&self, limit: u32) -> Result<Vec<Hit>> {
        if self.down {
            bail!("vinoSearch isn't running or isn't installed");
        }
        Ok(self.recent.iter().take(limit as usize).cloned().collect())
    }

    fn file_text(&self, path: &Path) -> Result<String> {
        match self.documents.get(path) {
            Some(text) => Ok(text.clone()),
            None => bail!("{} is not an indexed file", path.display()),
        }
    }
}

fn window(scratch: &Scratch, index: FakeIndex) -> Harness<'static, App> {
    window_sized(scratch, index, 700.0)
}

fn window_sized(scratch: &Scratch, index: FakeIndex, height: f32) -> Harness<'static, App> {
    window_with(scratch, index, egui::vec2(1100.0, height), false)
}

/// Tall enough that About's Model row is on screen, where a click reaches it.
fn tall_window(scratch: &Scratch, index: FakeIndex) -> Harness<'static, App> {
    window_sized(scratch, index, 1400.0)
}

/// The window at `size`, drawing its own title bar or not.
fn window_with(
    scratch: &Scratch,
    index: FakeIndex,
    size: egui::Vec2,
    app_title_bar: bool,
) -> Harness<'static, App> {
    let voices = ["af_heart", "bm_george"]
        .into_iter()
        .filter_map(voices::voice)
        .collect();
    let outputs = vec![Output {
        name: "hdmi_sink".to_owned(),
        description: "HDMI 1".to_owned(),
    }];
    let mut app = App::with(
        Arc::new(index),
        scratch.0.join("vinospeak"),
        voices,
        (outputs, Some("hdmi_sink".to_owned())),
    );
    app.cache = Some(scratch.0.join("cache"));
    app.session_dirs = vec![scratch.sessions()];
    app.app_title_bar = app_title_bar;
    // Fonts set in a frame apply from the next; the bold face must exist
    // before the window draws.
    let mut styled = false;
    Harness::builder().with_size(size).build_ui_state(
        move |ui, app: &mut App| {
            if !styled {
                theme::apply(ui.ctx());
                styled = true;
                ui.ctx().request_repaint();
                return;
            }
            // The whole window, as eframe hands it over: kittest's root has a margin.
            let window = ui.ctx().content_rect();
            let mut root = ui.new_child(egui::UiBuilder::new().max_rect(window));
            root.set_clip_rect(window);
            app.show(&mut root);
        },
        app,
    )
}

/// The window with one recent file, notes.md, previewed.
fn notes_window(scratch: &Scratch, text: &str) -> Harness<'static, App> {
    let mut h = window(
        scratch,
        FakeIndex::recent(vec![scratch.file("notes.md", text)]),
    );
    wait_for(&mut h, text);
    h
}

fn shows(h: &Harness<'_, App>, text: &str) -> bool {
    h.query_all_by_label_contains(text).next().is_some()
}

/// Step the window until `done`, as the background threads and vinospeak
/// take real time.
fn wait_until(h: &mut Harness<'_, App>, what: &str, done: impl Fn(&Harness<'_, App>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.step();
        if done(h) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for(h: &mut Harness<'_, App>, text: &str) {
    wait_until(h, text, |h| shows(h, text));
}

/// The last speak request the worker got, once it has one.
fn last_speak(h: &mut Harness<'_, App>, scratch: &Scratch) -> String {
    let speaks = |calls: String| {
        calls
            .lines()
            .rfind(|l| l.contains(r#""op":"speak""#))
            .map(str::to_owned)
    };
    wait_until(h, "a speak request", |_| {
        speaks(scratch.read("calls")).is_some()
    });
    speaks(scratch.read("calls")).unwrap()
}

#[test]
fn two_scratch_dirs_made_in_one_test_run_never_share_a_path() {
    let (a, b) = (Scratch::new(), Scratch::new());
    assert_ne!(a.0, b.0);
}

#[test]
fn before_a_search_the_recent_files_show_and_the_first_is_previewed() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![
        scratch.file("notes.md", "Newest notes first."),
        scratch.file("older.md", "Older text."),
    ]);
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Newest notes first.");
    assert!(shows(&h, "Recently changed"));
    assert!(shows(&h, "older.md"));
    assert!(
        !shows(&h, "Older text."),
        "only the first file is previewed"
    );
}

#[test]
fn typing_searches_and_lists_hits_without_images() {
    let scratch = Scratch::new();
    let mut index = FakeIndex::default();
    index.searches.insert(
        "mascot".to_owned(),
        vec![
            scratch.image("mascot.png"),
            scratch.file("mascot.md", "The mascot agreement."),
        ],
    );
    let mut h = window(&scratch, index);
    h.step();
    h.get_by_role(Role::TextInput).focus();
    h.step();
    h.get_by_role(Role::TextInput).type_text("mascot");
    wait_for(&mut h, "The mascot agreement.");
    assert!(!shows(&h, "mascot.png"), "images have nothing to read");
    assert!(!shows(&h, "Recently changed"));
}

#[test]
fn a_search_that_finds_nothing_says_so_and_keeps_the_recent_files() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let mut h = window(&scratch, index);
    h.state_mut().type_query("nothing like this");
    wait_for(&mut h, "Nothing close enough.");
    assert!(shows(&h, "Recently changed"));
    assert!(shows(&h, "notes.md"));
}

#[test]
fn spreadsheets_and_code_are_not_listed() {
    let scratch = Scratch::new();
    let mut recent: Vec<Hit> = (0..25)
        .map(|i| scratch.file(&format!("sheet{i}.csv"), "a,b,c"))
        .collect();
    recent.push(scratch.file("notes.md", "Older than the sheets."));
    let mut index = FakeIndex {
        recent,
        ..Default::default()
    };
    index.searches.insert(
        "budget".to_owned(),
        vec![
            scratch.file("budget.csv", "a,b,c"),
            scratch.file("budget.py", "print()"),
            scratch.file("budget.md", "The budget, in words."),
        ],
    );
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Older than the sheets.");
    assert!(!shows(&h, "sheet0.csv"));
    h.state_mut().type_query("budget");
    wait_for(&mut h, "The budget, in words.");
    assert!(!shows(&h, "budget.csv"));
    assert!(!shows(&h, "budget.py"));
}

#[test]
fn the_recent_list_stops_at_twenty() {
    let scratch = Scratch::new();
    let index = FakeIndex {
        recent: (0..21)
            .map(|i| scratch.file(&format!("note{i}.md"), "Some notes."))
            .collect(),
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Some notes.");
    assert!(shows(&h, "note19.md"));
    assert!(!shows(&h, "note20.md"));
}

#[test]
fn clicking_a_file_previews_it() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![
        scratch.file("first.md", "First file."),
        scratch.file("second.md", "Second file."),
    ]);
    let mut h = window(&scratch, index);
    wait_for(&mut h, "First file.");
    h.get_by_label("second.md").click();
    wait_for(&mut h, "Second file.");
    assert!(!shows(&h, "First file."));
}

#[test]
fn a_document_is_previewed_and_read_from_vinosearchs_text() {
    let scratch = Scratch::new();
    let pdf = scratch.file("contract.pdf", "%PDF-1.7 binary");
    let mut index = FakeIndex::recent(vec![pdf.clone()]);
    index
        .documents
        .insert(pdf.path.clone(), "Extracted page one.".to_owned());
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Extracted page one.");
    assert!(shows(&h, "Document"));
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_until(&mut h, "the text sent to the worker", |_| {
        scratch
            .read("calls")
            .contains(r#""text":"Extracted page one.""#)
    });
    assert!(scratch.read("calls").contains("--file - --kind document"));
}

#[test]
fn read_aloud_passes_the_voice_its_language_the_speed_and_the_output() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "Read me.");
    let state = h.state_mut();
    state.voice = voices::voice("bm_george").unwrap();
    state.speed = 1.5;
    state.output = Some("hdmi_sink".to_owned());
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    let request = last_speak(&mut h, &scratch);
    for field in [
        r#""voice":"bm_george""#,
        r#""language":"en-gb""#,
        r#""speed":1.5"#,
        r#""audio_device":"hdmi_sink""#,
        r#""text":"Read me.""#,
    ] {
        assert!(request.contains(field), "{field} in {request}");
    }
}

#[test]
fn the_default_output_passes_no_audio_device() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "Read me.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    let request = last_speak(&mut h, &scratch);
    assert!(request.contains(r#""voice":"af_heart"}"#), "{request}");
    assert!(request.contains(r#""audio_device":null"#), "{request}");
}

#[test]
fn every_reading_goes_to_the_one_worker() {
    let scratch = Scratch::new();
    scratch.mode("quick");
    let mut h = notes_window(&scratch, "Twice.");
    for _ in 0..2 {
        h.get_by_role_and_label(Role::Button, "Read aloud").click();
        wait_for(&mut h, "Finished notes.md");
        h.state_mut().status.clear();
    }
    let calls = scratch.read("calls");
    assert_eq!(
        calls.lines().filter(|l| *l == "--serve").count(),
        1,
        "{calls}"
    );
    assert_eq!(calls.matches(r#""op":"speak""#).count(), 2, "{calls}");
}

#[test]
fn stop_interrupts_vinospeak_and_brings_read_aloud_back() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "A long read.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.get_by_role_and_label(Role::Button, "Stop").click();
    wait_for(&mut h, "Read aloud");
    assert!(
        scratch.read("calls").contains("interrupted"),
        "a stop request, not a kill"
    );
    assert!(shows(&h, "Finished notes.md"));
}

#[test]
fn a_reading_that_ends_says_so() {
    let scratch = Scratch::new();
    scratch.mode("quick");
    let mut h = notes_window(&scratch, "Short.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Finished notes.md");
}

#[test]
fn a_failing_reading_shows_vinospeaks_error() {
    let scratch = Scratch::new();
    scratch.mode("fail-read");
    let mut h = notes_window(&scratch, "Doomed.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Couldn't read notes.md: boom");
}

#[test]
fn a_file_vinospeak_refuses_says_why_and_cant_be_read_aloud() {
    let scratch = Scratch::new();
    scratch.mode("fail-preview");
    let index = FakeIndex::recent(vec![scratch.file("blob.bin", "binary")]);
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Can't read this file: not a UTF-8 text file.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    h.run_steps(5);
    assert!(
        !scratch.read("calls").contains(r#""op":"speak""#),
        "nothing was started"
    );
}

#[test]
fn a_search_vinosearch_fails_says_so() {
    let scratch = Scratch::new();
    let mut h = window(&scratch, FakeIndex::default());
    wait_for(&mut h, "Nothing indexed yet.");
    h.state_mut().index = Arc::new(FakeIndex {
        down: true,
        ..Default::default()
    });
    h.state_mut().type_query("anything");
    wait_for(&mut h, "vinoSearch didn't answer");
}

fn without_vinosearch() -> FakeIndex {
    FakeIndex {
        down: true,
        ..Default::default()
    }
}

#[test]
fn without_vinosearch_the_sessions_on_disk_are_listed_and_search_is_off() {
    let scratch = Scratch::new();
    scratch.session_on_disk("2026-10-27-140512.vwsession", "session_v1.vwsession");
    let mut h = window(&scratch, without_vinosearch());
    wait_for(&mut h, "The summary of the session.");
    assert!(shows(&h, "Search and PDFs need vinoSearch."));
    assert!(shows(&h, "vinoWhisper sessions"));
    assert!(shows(&h, "Local captioning on the NPU"));
    let search = h.get_by_label("Search");
    assert!(search.accesskit_node().is_disabled(), "search is off");
}

#[test]
fn without_vinosearch_or_sessions_the_list_says_how_to_open_a_file() {
    let scratch = Scratch::new();
    let mut h = window(&scratch, without_vinosearch());
    wait_for(&mut h, "Open a file with the folder button.");
}

#[test]
fn a_session_vinosearch_doesnt_list_joins_the_recent_files() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Notes.")]);
    scratch.session_on_disk("2026-10-27-140512.vwsession", "session_v1.vwsession");
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Local captioning on the NPU");
    assert!(shows(&h, "notes.md"));
    assert!(shows(&h, "Recently changed"));
    assert!(!shows(&h, "need vinoSearch"));
}

#[test]
fn a_session_vinosearch_lists_too_is_listed_once() {
    let scratch = Scratch::new();
    let on_disk = scratch.session_on_disk("2026-10-27-140512.vwsession", "session_v1.vwsession");
    let mut listed = scratch.session("x.vwsession", "session_v1.vwsession");
    listed.path = on_disk;
    let mut h = window(&scratch, FakeIndex::recent(vec![listed]));
    wait_for(&mut h, "Local captioning on the NPU");
    let rows = h
        .query_all_by_role(Role::ListBoxOption)
        .filter(|row| {
            row.accesskit_node()
                .label()
                .is_some_and(|l| l.contains("Local captioning"))
        })
        .count();
    assert_eq!(rows, 1);
}

#[test]
fn a_file_named_on_the_command_line_is_previewed() {
    let scratch = Scratch::new();
    let opened = scratch.file("elsewhere.md", "Opened from elsewhere.");
    let mut h = window(&scratch, without_vinosearch());
    h.state_mut().to_open = Some(opened.path);
    wait_for(&mut h, "Opened from elsewhere.");
    assert!(shows(&h, "elsewhere.md"));
}

#[test]
fn a_session_is_titled_by_its_summary_and_reads_either_part() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![
        scratch.session("2026-10-27-140512.vwsession", "session_v1.vwsession"),
    ]);
    let mut h = window(&scratch, index);
    wait_for(&mut h, "The summary of the session.");
    assert!(shows(&h, "Local captioning on the NPU"));
    assert!(shows(&h, "Session"));
    h.get_by_role_and_label(Role::RadioButton, "Transcript")
        .click();
    wait_for(&mut h, "The transcript of the session.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    let request = last_speak(&mut h, &scratch);
    assert!(
        request.contains(r#""text":"The transcript of the session.\n""#),
        "{request}"
    );
}

#[test]
fn a_session_without_a_summary_reads_its_transcript_and_says_why() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![scratch.session(
        "2026-10-27-140512.vwsession",
        "session_v1_nosummary.vwsession",
    )]);
    let mut h = window(&scratch, index);
    wait_for(&mut h, "The transcript of the session.");
    assert!(shows(&h, "27 October 2026, 14:05"));
    assert!(shows(&h, "Not summarised yet"));
    assert!(!scratch.read("calls").contains("--part summary"));
}

#[test]
fn about_shows_the_other_apps_until_a_file_is_picked() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![
        scratch.file("first.md", "First file."),
        scratch.file("second.md", "Second file."),
    ]);
    let mut h = window(&scratch, index);
    wait_for(&mut h, "First file.");
    h.get_by_role_and_label(Role::Tab, "About").click();
    wait_for(&mut h, "Learn more vinoSearch");
    assert!(shows(&h, "vinopilot.ca"));
    assert!(!shows(&h, "Learn more vinoSpeak"));
    h.get_by_label("second.md").click();
    wait_for(&mut h, "Second file.");
    assert!(!shows(&h, "Learn more vinoSearch"));
}

/// A tall window previewing notes.md, so its worker holds a downloaded
/// model open, with About showing the model's size.
fn about_with_a_model(scratch: &Scratch) -> Harness<'static, App> {
    scratch.model();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let mut h = tall_window(scratch, index);
    wait_for(&mut h, "Some notes.");
    wait_until(&mut h, "the worker", |_| {
        scratch.read("calls").contains("--serve")
    });
    h.get_by_role_and_label(Role::Tab, "About").click();
    wait_for(&mut h, "2 MB on disk");
    h
}

fn confirm_delete(h: &mut Harness<'_, App>) {
    h.get_by_role_and_label(Role::Button, "Delete…").click();
    wait_for(h, "Delete the model");
    h.get_by_role_and_label(Role::Button, "Delete the model")
        .click();
}

#[test]
fn cancelling_the_confirmation_keeps_the_model() {
    let scratch = Scratch::new();
    let mut h = about_with_a_model(&scratch);
    h.get_by_role_and_label(Role::Button, "Delete…").click();
    wait_for(&mut h, "Deletes 2 MB");
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    wait_for(&mut h, "2 MB on disk");
    assert!(!scratch.read("calls").contains("--delete-model"));
}

#[test]
fn about_deletes_the_model_after_confirming_in_place() {
    let scratch = Scratch::new();
    let mut h = about_with_a_model(&scratch);
    confirm_delete(&mut h);
    wait_for(&mut h, "Not downloaded");
    assert!(shows(&h, "Deleted the model and its compiled cache"));
    assert!(h.state().worker.is_none(), "the worker let go of the model");
    let delete = h.get_by_role_and_label(Role::Button, "Delete…");
    assert!(delete.accesskit_node().is_disabled(), "nothing to delete");
    let voices: Vec<_> = h.state().voices.iter().map(|v| v.id.clone()).collect();
    assert_eq!(voices, ["af_heart"]);
}

#[test]
fn after_a_delete_the_next_reading_starts_a_worker_again() {
    let scratch = Scratch::new();
    let mut h = about_with_a_model(&scratch);
    confirm_delete(&mut h);
    wait_for(&mut h, "Not downloaded");
    h.get_by_label("notes.md").click();
    wait_for(&mut h, "Some notes.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    last_speak(&mut h, &scratch);
    assert_eq!(scratch.read("calls").matches("--serve").count(), 2);
}

#[test]
fn the_model_cant_be_deleted_while_reading() {
    let scratch = Scratch::new();
    scratch.model();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "A long read.")]);
    let mut h = tall_window(&scratch, index);
    wait_for(&mut h, "A long read.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.get_by_role_and_label(Role::Tab, "About").click();
    wait_for(&mut h, "Stop the reading to delete it.");
    let delete = h.get_by_role_and_label(Role::Button, "Delete…");
    assert!(delete.accesskit_node().is_disabled());
}

#[test]
fn a_delete_vinospeak_refuses_says_why() {
    let scratch = Scratch::new();
    scratch.model();
    scratch.mode("fail-delete");
    let mut h = tall_window(&scratch, FakeIndex::default());
    h.get_by_role_and_label(Role::Tab, "About").click();
    wait_for(&mut h, "2 MB on disk");
    confirm_delete(&mut h);
    wait_for(&mut h, "Couldn't delete it: The model is downloading.");
    assert!(scratch.0.join("cache/ov-cache/blob").is_file());
}

const WIDE: egui::Vec2 = egui::vec2(1100.0, 700.0);
const NARROW: egui::Vec2 = egui::vec2(600.0, 700.0);

/// The window at `size` with vino-ui's title bar, drawn.
fn title_bar_window(scratch: &Scratch, size: egui::Vec2) -> Harness<'static, App> {
    let mut h = window_with(scratch, FakeIndex::default(), size, true);
    h.run_steps(4);
    h
}

#[test]
fn the_window_buttons_sit_in_the_top_right_corner() {
    for size in [WIDE, NARROW] {
        let scratch = Scratch::new();
        let h = title_bar_window(&scratch, size);
        // kittest's margin round the page doesn't move the window's own edges.
        let gap = vino_ui::window::BAR_GAP;
        let close = h.get_by_role_and_label(Role::Button, "Close").rect();
        assert_eq!(close.top(), gap, "{size:?}");
        assert_eq!(close.right(), size.x - gap, "{size:?}");
        for name in ["Minimise", "Maximise"] {
            let button = h.get_by_role_and_label(Role::Button, name).rect();
            assert_eq!(button.top(), gap, "{name} {size:?}");
        }
    }
}

#[test]
fn the_page_title_and_tabs_stay_clear_of_the_window_buttons() {
    let scratch = Scratch::new();
    let notes = scratch.file(
        "a-long-name-for-a-file-that-runs-on-and-on.md",
        "Some notes.",
    );
    let mut h = window_with(&scratch, FakeIndex::recent(vec![notes]), WIDE, true);
    wait_for(&mut h, "Some notes.");
    let minimise = h.get_by_role_and_label(Role::Button, "Minimise").rect();
    let title = h
        .get_by_role_and_label(
            Role::Heading,
            "a-long-name-for-a-file-that-runs-on-and-on.md",
        )
        .rect();
    assert!(title.right() < minimise.left(), "{title:?}");

    let h = title_bar_window(&scratch, NARROW);
    let minimise = h.get_by_role_and_label(Role::Button, "Minimise").rect();
    let about = h.get_by_role_and_label(Role::Tab, "About").rect();
    assert!(about.top() > minimise.bottom() || about.right() < minimise.left());
}

#[test]
fn with_the_app_title_bar_about_still_takes_a_click() {
    let scratch = Scratch::new();
    let mut h = title_bar_window(&scratch, WIDE);
    h.get_by_role_and_label(Role::Tab, "About").click();
    wait_for(&mut h, "Learn more vinoSearch");
}

#[test]
fn about_opened_before_the_recent_files_arrive_stays_open() {
    let scratch = Scratch::new();
    let notes = scratch.file("notes.md", "Some notes.");
    let mut h = window(&scratch, FakeIndex::recent(vec![]));
    h.state_mut().open_about();
    let ctx = h.ctx.clone();
    h.state_mut().on_recent(&ctx, vec![notes], None);
    h.run_steps(4);
    assert!(shows(&h, "Learn more vinoSearch"));
    assert!(h.state().selected.is_none());
}

#[test]
fn a_worker_that_exits_idle_is_started_again_by_the_next_read_aloud() {
    let scratch = Scratch::new();
    scratch.mode("exit-idle");
    let mut h = notes_window(&scratch, "Some notes.");
    wait_until(&mut h, "the idle worker to exit", |h| {
        scratch.read("calls").contains("--serve") && h.state().worker.is_none()
    });
    assert_eq!(h.state().status, "", "no reading failed");
    std::fs::remove_file(scratch.0.join("exit-idle")).unwrap();
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    last_speak(&mut h, &scratch);
    assert_eq!(scratch.read("calls").matches("--serve").count(), 2);
}

#[test]
fn a_worker_that_exits_mid_reading_fails_the_reading() {
    let scratch = Scratch::new();
    scratch.mode("crash-read");
    let mut h = notes_window(&scratch, "Some notes.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Couldn't read notes.md: crashed");
}

#[test]
fn without_the_app_title_bar_the_desktop_keeps_its_buttons() {
    let scratch = Scratch::new();
    let mut h = window(&scratch, FakeIndex::default());
    h.run_steps(4);
    for name in ["Close", "Maximise", "Minimise"] {
        assert!(
            h.query_by_role_and_label(Role::Button, name).is_none(),
            "{name}"
        );
    }
}

#[test]
fn narrow_the_list_is_a_tab_and_picking_a_file_turns_to_reading() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![
        scratch.file("first.md", "First file."),
        scratch.file("second.md", "Second file."),
    ]);
    let mut h = window_with(&scratch, index, NARROW, false);
    wait_for(&mut h, "second.md");
    assert!(!shows(&h, "First file."), "the reader waits behind its tab");
    h.query_all_by_role(Role::ListBoxOption)
        .find(|row| {
            row.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("second.md"))
        })
        .unwrap()
        .click();
    wait_for(&mut h, "Second file.");
    assert!(!shows(&h, "first.md"), "the list is behind its tab");
    h.get_by_role_and_label(Role::Tab, "Files").click();
    wait_for(&mut h, "first.md");
}

#[test]
fn the_player_wraps_rather_than_running_off_the_window() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let size = egui::vec2(960.0, 680.0);
    let mut h = window_with(&scratch, index, size, false);
    wait_for(&mut h, "Some notes.");
    for name in ["Voice", "1.5×", "Output"] {
        let right = h.get_by_label(name).rect().right();
        assert!(right <= size.x, "{name} ends at {right}");
    }
}

#[test]
fn with_the_bar_about_is_titled_once() {
    let scratch = Scratch::new();
    let mut h = title_bar_window(&scratch, WIDE);
    h.get_by_role_and_label(Role::Tab, "About").click();
    wait_for(&mut h, "Learn more vinoSearch");
    assert_eq!(
        h.query_all_by_role_and_label(Role::Heading, "About")
            .count(),
        1
    );
}
