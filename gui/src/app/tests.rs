//! The window driven headless through egui_kittest, finding widgets by their
//! AccessKit labels as a screen reader would. vinoSearch is a fake `Index`;
//! vinospeak is a shell script that prints `--print-text`, and as `--serve`
//! logs each request and "plays" until it's stopped, translating a request
//! with `translate` into two pieces and saving empty files; as `--http` it
//! says it is serving and sleeps.

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
# argv: --serve [--device D], --delete-model, --download- or --delete-translation-model,
# or --file PATH|- [--kind document | --part P] --print-text
dir=$(dirname "$0")
echo "$*" >> "$dir/calls"
if [ "$1" = "--download-translation-model" ]; then
    [ -f "$dir/fail-download" ] && { echo "vinospeak: error: The Hub can't be reached." >&2; exit 2; }
    mkdir -p "$dir/translation/SmolLM3-3B-int4-cw-ov"
    head -c 3000000 /dev/zero > "$dir/translation/SmolLM3-3B-int4-cw-ov/openvino_model.bin"
    touch "$dir/translation/SmolLM3-3B-int4-cw-ov/.vinospeak-verified"
    echo "Downloaded."
    exit 0
fi
if [ "$1" = "--delete-translation-model" ]; then
    rm -rf "$dir/translation"
    echo "Deleted the translation model (3 MB)."
    exit 0
fi
if [ "$1" = "--delete-model" ]; then
    [ -f "$dir/fail-delete" ] && { echo "vinospeak: error: The model is downloading." >&2; exit 2; }
    rm -rf "$dir/cache/kokoro-82m-int8-ov" "$dir/cache/ov-cache"
    echo "Deleted the model and its compiled cache (2 MB)."
    exit 0
fi
if [ "$1" = "--http" ]; then
    [ -f "$dir/fail-http" ] && { echo "vinospeak: error: [Errno 98] Address already in use" >&2; exit 2; }
    echo "vinoSpeak is serving OpenAI's speech API at http://127.0.0.1:$2/v1 (model loaded in 0.1 s). Ctrl+C stops it." >&2
    exec sleep 30
fi
if [ "$1" = "--serve" ]; then
    echo '{"event": "ready"}'
    [ -f "$dir/exit-idle" ] && exit 0
    current=
    while read -r line; do
        printf '%s\n' "$line" >> "$dir/calls"
        case "$line" in
        *'"op":"save"'*)
            saving=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
            out=$(printf '%s' "$line" | sed -n 's/.*"output":"\([^"]*\)".*/\1/p')
            sub=$(printf '%s' "$line" | sed -n 's/.*"subtitles":"\([^"]*\)".*/\1/p')
            if [ -f "$dir/fail-save" ]; then
                echo "{\"event\": \"error\", \"id\": $saving, \"message\": \"no MP3 encoder\"}"
                continue
            fi
            echo "{\"event\": \"saving\", \"id\": $saving, \"done\": 0.5}"
            [ -f "$dir/slow-save" ] && continue
            : > "$out"
            [ -n "$sub" ] && : > "$sub"
            echo "{\"event\": \"saved\", \"id\": $saving, \"seconds\": 65.0}"
            continue ;;
        *'"op":"cancel-save"'*)
            echo "{\"event\": \"save-cancelled\", \"id\": $saving}"
            continue ;;
        *'"op":"volume"'*|*'"op":"pause"'*|*'"op":"resume"'*|*'"op":"lexicon"'*) continue ;;
        esac
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
        into=$(printf '%s' "$line" | sed -n 's/.*"translate":"\([a-z-]*\)".*/\1/p')
        [ -n "$into" ] && echo "{\"event\": \"translated\", \"id\": $id, \"text\": \"[$into] The first piece.\", \"start\": 0.0, \"end\": 0.5}"
        echo "{\"event\": \"first-audio\", \"id\": $id, \"ms\": 5}"
        [ -n "$into" ] && echo "{\"event\": \"progress\", \"id\": $id, \"done\": 0.2}"
        [ -n "$into" ] && echo "{\"event\": \"translated\", \"id\": $id, \"text\": \"[$into] The second piece.\", \"start\": 0.5, \"end\": 1.0}"
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

/// Tall enough that Settings' Model row is on screen, where a click reaches it.
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
    app.translation_dir = Some(scratch.0.join("translation/SmolLM3-3B-int4-cw-ov"));
    app.refresh_translation();
    app.session_dirs = vec![scratch.sessions()];
    app.lexicon = super::lexicon::Pronunciations::at(Some(scratch.0.join("config/lexicon.toml")));
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
    wait_for(&mut h, "No matches.");
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
fn read_aloud_passes_the_volume_and_none_while_muted() {
    let scratch = Scratch::new();
    scratch.mode("quick");
    let mut h = notes_window(&scratch, "Read me.");
    h.state_mut().volume = 0.5;
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Finished notes.md");
    assert!(last_speak(&mut h, &scratch).contains(r#""volume":0.5"#));
    h.get_by_role_and_label(Role::Button, "Mute").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_until(&mut h, "a muted speak request", |_| {
        scratch.read("calls").contains(r#""volume":0.0}"#)
    });
}

#[test]
fn mute_quiets_the_reading_under_way_without_stopping_it() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "A long read.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.get_by_role_and_label(Role::Button, "Mute").click();
    wait_until(&mut h, "a volume request", |_| {
        scratch
            .read("calls")
            .contains(r#"{"op":"volume","volume":0.0}"#)
    });
    h.run();
    h.get_by_role_and_label(Role::Button, "Unmute").click();
    wait_until(&mut h, "the volume back", |_| {
        scratch
            .read("calls")
            .contains(r#"{"op":"volume","volume":1.0}"#)
    });
    assert!(shows(&h, "Stop"), "still reading");
    assert!(!scratch.read("calls").contains("interrupted"));
}

#[test]
fn the_volume_with_nothing_playing_waits_for_the_next_reading() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "Read me.");
    h.get_by_role_and_label(Role::Button, "Mute").click();
    h.run();
    assert!(!scratch.read("calls").contains(r#""op":"volume""#));
}

#[test]
fn the_default_output_passes_no_audio_device() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "Read me.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    let request = last_speak(&mut h, &scratch);
    assert!(request.contains(r#""voice":"af_heart","#), "{request}");
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
    wait_for(&mut h, "Couldn't read file: not a UTF-8 text file.");
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
    wait_for(&mut h, "No files indexed yet.");
    h.state_mut().index = Arc::new(FakeIndex {
        down: true,
        ..Default::default()
    });
    h.state_mut().type_query("anything");
    wait_for(&mut h, "Couldn't reach vinoSearch");
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
    assert!(shows(&h, "Search and PDFs require vinoSearch."));
    assert!(shows(&h, "vinoWhisper sessions"));
    assert!(shows(&h, "Local captioning on the NPU"));
    let search = h.get_by_label("Search");
    assert!(search.accesskit_node().is_disabled(), "search is off");
}

#[test]
fn without_vinosearch_or_sessions_the_list_says_how_to_open_a_file() {
    let scratch = Scratch::new();
    let mut h = window(&scratch, without_vinosearch());
    wait_for(&mut h, "No sessions yet.");
    assert!(
        h.query_by_role_and_label(Role::Button, "Open file…")
            .is_some()
    );
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
    assert!(!shows(&h, "require vinoSearch"));
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
    assert!(shows(&h, "No summary yet"));
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
fn settings_with_a_model(scratch: &Scratch) -> Harness<'static, App> {
    scratch.model();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let mut h = tall_window(scratch, index);
    wait_for(&mut h, "Some notes.");
    wait_until(&mut h, "the worker", |_| {
        scratch.read("calls").contains("--serve")
    });
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "2 MB on disk");
    h
}

fn confirm_delete(h: &mut Harness<'_, App>) {
    h.get_by_role_and_label(Role::Button, "Delete…").click();
    wait_for(h, "Delete model");
    h.get_by_role_and_label(Role::Button, "Delete model")
        .click();
}

#[test]
fn cancelling_the_confirmation_keeps_the_model() {
    let scratch = Scratch::new();
    let mut h = settings_with_a_model(&scratch);
    h.get_by_role_and_label(Role::Button, "Delete…").click();
    wait_for(&mut h, "Deletes 2 MB");
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    wait_for(&mut h, "2 MB on disk");
    assert!(!scratch.read("calls").contains("--delete-model"));
}

#[test]
fn settings_deletes_the_model_after_confirming_in_place() {
    let scratch = Scratch::new();
    let mut h = settings_with_a_model(&scratch);
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
    let mut h = settings_with_a_model(&scratch);
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
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "Stop playback before deleting.");
    let delete = h.get_by_role_and_label(Role::Button, "Delete…");
    assert!(delete.accesskit_node().is_disabled());
}

#[test]
fn a_delete_vinospeak_refuses_says_why() {
    let scratch = Scratch::new();
    scratch.model();
    scratch.mode("fail-delete");
    let mut h = tall_window(&scratch, FakeIndex::default());
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "2 MB on disk");
    confirm_delete(&mut h);
    wait_for(&mut h, "Couldn't delete model: The model is downloading.");
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
fn the_player_stays_inside_the_window() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let size = egui::vec2(960.0, 680.0);
    let mut h = window_with(&scratch, index, size, false);
    wait_for(&mut h, "Some notes.");
    for name in ["Translate…", "Voice", "Speed", "Save as MP3…", "Volume"] {
        let right = h.get_by_label(name).rect().right();
        assert!(right <= size.x, "{name} ends at {right}");
    }
}

#[test]
fn the_settings_go_two_by_two_when_narrow() {
    use super::player::settings_per_row;
    assert_eq!(settings_per_row(700.0), 4);
    assert_eq!(settings_per_row(500.0), 2);
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let mut h = window_with(&scratch, index, egui::vec2(1280.0, 800.0), false);
    wait_for(&mut h, "Some notes.");
    let top = |h: &Harness<'_, App>, name: &str| h.get_by_label(name).rect().top();
    assert_eq!(
        top(&h, "Voice"),
        top(&h, "Save as MP3…"),
        "one row when wide"
    );
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let mut h = window_with(&scratch, index, NARROW, false);
    wait_for(&mut h, "notes.md");
    h.query_all_by_role(Role::ListBoxOption)
        .find(|row| {
            row.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("notes.md"))
        })
        .unwrap()
        .click();
    wait_for(&mut h, "Some notes.");
    assert_eq!(top(&h, "Voice"), top(&h, "Translate…"));
    assert!(top(&h, "Speed") > top(&h, "Voice"), "two rows when narrow");
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

#[test]
fn the_voice_speed_and_volume_come_back_in_the_next_window() {
    let scratch = Scratch::new();
    let settings = scratch.0.join("config/vinospeak/gui.json");
    let mut h = notes_window(&scratch, "Read me.");
    h.state_mut().remember(settings.clone());
    h.state_mut().voice = voices::voice("bm_george").unwrap();
    h.state_mut().volume = 0.4;
    h.get_by_label("Speed").click();
    h.run();
    h.get_by_label("1.25×").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Mute").click();
    h.run();

    let mut next = notes_window(&scratch, "Read me.");
    next.state_mut().remember(settings);
    let state = next.state();
    assert_eq!(state.voice.id, "bm_george");
    assert_eq!(state.speed, 1.25);
    assert_eq!(state.volume, 0.4);
    assert!(!state.muted, "a window never opens muted");
}

#[test]
fn a_saved_voice_that_is_not_installed_or_an_unoffered_speed_is_left_at_the_default() {
    let scratch = Scratch::new();
    let settings = scratch.0.join("gui.json");
    std::fs::write(
        &settings,
        r#"{"voice": "zz_nobody", "speed": 3.0, "volume": 0.7}"#,
    )
    .unwrap();
    let mut h = notes_window(&scratch, "Read me.");
    h.state_mut().remember(settings);
    let state = h.state();
    assert_eq!(state.voice.id, "af_heart");
    assert_eq!(state.speed, 1.0);
    assert_eq!(state.volume, 0.7);
}

/// The window with notes.md previewed, the translation model downloaded and
/// Dora's Spanish voice among the voices; tall enough for About's Translation row.
fn translating_window(scratch: &Scratch) -> Harness<'static, App> {
    let model = scratch.0.join("translation/SmolLM3-3B-int4-cw-ov");
    std::fs::create_dir_all(&model).unwrap();
    std::fs::write(model.join(".vinospeak-verified"), "").unwrap();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Read me.")]);
    let mut h = tall_window(scratch, index);
    wait_for(&mut h, "Read me.");
    let state = h.state_mut();
    state.voices.push(voices::voice("ef_dora").unwrap());
    state.refresh_translation();
    h.run();
    h
}

fn pick_language(h: &mut Harness<'_, App>, name: &str) {
    h.get_by_label("Translate").click();
    h.run();
    h.get_by_label(name).click();
    h.run();
}

#[test]
fn translate_picks_a_language_only_once_its_model_is_downloaded() {
    let scratch = Scratch::new();
    let h = notes_window(&scratch, "Read me.");
    assert!(h.query_by_label("Translate").is_none());
    assert!(h.query_by_label("Translate…").is_some());
    let h = translating_window(&scratch);
    assert!(h.query_by_label("Translate").is_some());
    assert!(h.query_by_label("Translate…").is_none());
}

#[test]
fn translate_before_its_download_leads_to_the_download() {
    let scratch = Scratch::new();
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Read me.")]);
    let mut h = tall_window(&scratch, index);
    wait_for(&mut h, "Read me.");
    h.get_by_role_and_label(Role::Button, "Translate…").click();
    wait_for(&mut h, "Downloads 1.7 GB. Only loads when you translate.");
    assert_eq!(h.state().page, super::Page::Settings);
    assert!(
        !scratch
            .read("calls")
            .contains("--download-translation-model"),
        "nothing downloads before Download is pressed"
    );
}

#[test]
fn a_copy_vinowhisper_uses_is_shared_and_not_deleted_from_here() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    let model = scratch.0.join("translation/SmolLM3-3B-int4-cw-ov");
    std::fs::write(model.join(".vinowhisper-verified"), "").unwrap();
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "shared with vinoWhisper (delete it there).");
    assert!(h.query_by_label("Delete translation model").is_none());
}

#[test]
fn picking_a_language_picks_a_voice_that_speaks_it() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    pick_language(&mut h, "Spanish");
    assert_eq!(h.state().voice.id, "ef_dora");
    h.state_mut().voice = voices::voice("af_heart").unwrap();
    pick_language(&mut h, "American English");
    assert_eq!(h.state().voice.id, "af_heart", "Heart already speaks it");
}

#[test]
fn a_translated_reading_asks_for_the_language_and_says_so_until_it_speaks() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    pick_language(&mut h, "Spanish");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    let request = last_speak(&mut h, &scratch);
    for field in [
        r#""translate":"es""#,
        r#""language":null"#,
        r#""voice":"ef_dora""#,
    ] {
        assert!(request.contains(field), "{field} in {request}");
    }
    wait_for(&mut h, "Speaking notes.md in Spanish");
}

#[test]
fn a_status_says_translating_before_the_first_audio() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    pick_language(&mut h, "French");
    let ctx = h.ctx.clone();
    h.state_mut().start_worker(&ctx);
    h.state_mut().read_aloud(&ctx);
    assert_eq!(h.state().status, "Translating notes.md into French");
}

#[test]
fn an_untranslated_reading_sends_no_language_to_translate_into() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    let request = last_speak(&mut h, &scratch);
    assert!(request.contains(r#""translate":null"#), "{request}");
    assert!(request.contains(r#""language":"en-us""#), "{request}");
}

fn settings(scratch: &Scratch) -> Harness<'static, App> {
    let index = FakeIndex::recent(vec![scratch.file("notes.md", "Some notes.")]);
    let mut h = tall_window(scratch, index);
    wait_for(&mut h, "Some notes.");
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "Not downloaded. SmolLM3-3B (1.7 GB)");
    h
}

#[test]
fn settings_downloads_the_translation_model_only_after_confirming() {
    let scratch = Scratch::new();
    let mut h = settings(&scratch);
    h.get_by_role_and_label(Role::Button, "Download…").click();
    wait_for(&mut h, "Downloads 1.7 GB. Only loads when you translate.");
    assert!(
        !scratch
            .read("calls")
            .contains("--download-translation-model")
    );
    h.get_by_role_and_label(Role::Button, "Download").click();
    wait_for(&mut h, "3 MB on disk.");
    assert!(shows(&h, "Downloaded."));
    assert!(h.state().translation.downloaded);
}

#[test]
fn cancelling_the_download_downloads_nothing() {
    let scratch = Scratch::new();
    let mut h = settings(&scratch);
    h.get_by_role_and_label(Role::Button, "Download…").click();
    wait_for(&mut h, "Downloads 1.7 GB");
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    wait_for(&mut h, "Not downloaded. SmolLM3-3B");
    assert!(
        !scratch
            .read("calls")
            .contains("--download-translation-model")
    );
}

#[test]
fn a_download_that_fails_says_why_and_offers_it_again() {
    let scratch = Scratch::new();
    scratch.mode("fail-download");
    let mut h = settings(&scratch);
    h.get_by_role_and_label(Role::Button, "Download…").click();
    wait_for(&mut h, "Downloads 1.7 GB");
    h.get_by_role_and_label(Role::Button, "Download").click();
    wait_for(&mut h, "Failed: The Hub can't be reached.");
    assert!(h.query_by_label("Download…").is_some());
    assert!(!h.state().translation.downloaded);
}

#[test]
fn deleting_the_translation_model_takes_translate_away() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    pick_language(&mut h, "Spanish");
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "on disk.");
    h.get_by_role_and_label(Role::Button, "Delete translation model")
        .click();
    wait_for(&mut h, "You can download it again later.");
    h.get_by_role_and_label(Role::Button, "Delete model")
        .click();
    wait_for(&mut h, "Deleted the translation model (3 MB).");
    let state = h.state();
    assert!(!state.translation.downloaded);
    assert_eq!(state.translate_to, None);
    assert!(state.worker.is_none(), "the worker let go of the model");
}

#[test]
fn the_language_comes_back_in_the_next_window_while_its_model_is_there() {
    let scratch = Scratch::new();
    let settings = scratch.0.join("config/vinospeak/gui.json");
    let mut h = translating_window(&scratch);
    h.state_mut().remember(settings.clone());
    pick_language(&mut h, "Italian");
    h.run();

    let mut next = translating_window(&scratch);
    next.state_mut().remember(settings.clone());
    assert_eq!(next.state().translate_to.map(|t| t.language), Some("it"));

    std::fs::remove_dir_all(scratch.0.join("translation")).unwrap();
    let mut without = notes_window(&scratch, "Read me.");
    without.state_mut().remember(settings);
    assert_eq!(without.state().translate_to, None);
}

#[test]
fn the_player_says_it_is_starting_until_the_first_audio() {
    let state =
        |h: &Harness<'_, App>| super::player::reading_state(h.state().reading.as_ref().unwrap());
    for (language, before) in [(None, "Starting…"), (Some("Spanish"), "Translating…")] {
        let scratch = Scratch::new();
        let mut h = translating_window(&scratch);
        if let Some(language) = language {
            pick_language(&mut h, language);
        }
        let ctx = h.ctx.clone();
        h.state_mut().start_worker(&ctx);
        h.state_mut().read_aloud(&ctx);
        assert_eq!(state(&h), before);
        wait_until(&mut h, "the first audio", |h| {
            h.state().reading.as_ref().is_none_or(|r| r.heard)
        });
    }
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Speaking");
}

#[test]
fn the_choices_stay_put_when_a_reading_starts() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    pick_language(&mut h, "French");
    let places = |h: &Harness<'_, App>| {
        ["Translate", "Voice", "Speed"].map(|label| h.get_by_label(label).rect())
    };
    let before = places(&h);
    for heard in [false, true] {
        h.state_mut().reading = Some(crate::speech::Reading {
            id: 1,
            file: scratch.0.join("notes.md"),
            started: Instant::now(),
            stop_sent: None,
            translated_into: crate::translation::target("fr-fr"),
            heard,
            done: 0.0,
            paused_at: None,
            length: Duration::ZERO,
            chosen: crate::speech::Chosen::default(),
        });
        h.run_steps(3);
        assert!(h.query_by_label("Stop").is_some());
        assert_eq!(places(&h), before, "heard: {heard}");
    }
}

#[test]
fn translating_offers_only_voices_of_that_language() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    h.get_by_label("Voice").click();
    h.run();
    assert!(
        h.query_by_label("Heart  ·  female").is_some(),
        "every voice untranslated"
    );
    h.key_press(egui::Key::Escape);
    h.run();
    pick_language(&mut h, "Spanish");
    h.get_by_label("Voice").click();
    h.run();
    assert!(h.query_by_label("Dora  ·  female").is_some());
    assert!(
        h.query_by_label("Heart  ·  female").is_none(),
        "no American voice for Spanish"
    );
}

#[test]
fn a_restored_language_brings_a_voice_that_speaks_it() {
    let scratch = Scratch::new();
    let settings = scratch.0.join("config/vinospeak/gui.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, r#"{"voice": "af_heart", "translate": "es"}"#).unwrap();
    let mut h = translating_window(&scratch);
    h.state_mut().remember(settings);
    let state = h.state();
    assert_eq!(state.translate_to.map(|t| t.language), Some("es"));
    assert_eq!(state.voice.language, "es");
}

#[test]
fn the_bar_shows_how_far_the_reading_is_and_never_goes_back() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    h.state_mut().reading = Some(crate::speech::Reading {
        id: 7,
        file: scratch.0.join("notes.md"),
        started: Instant::now(),
        stop_sent: None,
        translated_into: None,
        heard: true,
        done: 0.0,
        paused_at: None,
        length: Duration::ZERO,
        chosen: crate::speech::Chosen::default(),
    });
    for (done, expected) in [(0.4, 0.4), (0.2, 0.4), (0.9, 0.9)] {
        h.state_mut().on_progress(7, done);
        assert_eq!(h.state().reading.as_ref().unwrap().done, expected);
    }
    h.state_mut().on_progress(8, 1.0);
    assert_eq!(
        h.state().reading.as_ref().unwrap().done,
        0.9,
        "another reading's"
    );
}

#[test]
fn scrubbing_reads_again_from_where_the_bar_was_let_go() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "One. Two. Three.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.get_by_role_and_label(Role::Slider, "Position").click();
    wait_until(&mut h, "a speak from the middle", |_| {
        scratch.read("calls").contains(r#""from":0.5"#)
    });
    let reading = h.state().reading.as_ref().unwrap();
    assert_eq!((reading.id, reading.done), (2, 0.5));
    h.run_steps(3);
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    let done = h.state().reading.as_ref().unwrap().done;
    assert!((done - 0.55).abs() < 1e-6, "{done}");
    h.state_mut().on_progress(3, 1.0);
    h.run_steps(3);
    h.key_press(egui::Key::ArrowRight);
    wait_for(&mut h, "Finished notes.md");
    assert_eq!(scratch.read("calls").matches(r#""op":"speak""#).count(), 3);
}

#[test]
fn space_pauses_and_resumes_the_reading() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "A long read.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.key_press(egui::Key::Space);
    wait_for(&mut h, "Paused");
    let paused = h.state().reading.as_ref().unwrap().played();
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        h.state().reading.as_ref().unwrap().played(),
        paused,
        "the clock holds"
    );
    h.key_press(egui::Key::Space);
    wait_until(&mut h, "a resume", |_| {
        scratch.read("calls").contains(r#"{"op":"resume"}"#)
    });
    assert!(scratch.read("calls").contains(r#"{"op":"pause"}"#));
    assert!(h.query_by_label("Paused").is_none());
    assert!(h.query_by_label("Stop").is_some(), "still the same reading");
}

#[test]
fn space_in_the_search_box_is_typed_not_a_pause() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "A long read.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.get_by_role(Role::TextInput).focus();
    h.run_steps(3);
    h.key_press(egui::Key::Space);
    h.run_steps(3);
    assert!(h.state().reading.as_ref().unwrap().paused_at.is_none());
}

#[test]
fn the_media_controls_show_and_drive_the_reading() {
    use crate::mpris::{Command, Status};
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "A long read.");
    assert_eq!(h.state().now_playing().status, Status::Stopped);
    let ctx = h.ctx.clone();
    h.state_mut().on_media(&ctx, Command::PlayPause);
    wait_for(&mut h, "Stop");
    let now = h.state().now_playing();
    assert_eq!(
        (now.status, now.title.as_str(), now.voice.as_str()),
        (Status::Playing, "notes.md", "Heart")
    );
    h.state_mut().on_media(&ctx, Command::Play);
    assert_eq!(
        h.state().now_playing().status,
        Status::Playing,
        "Play while playing does nothing"
    );
    h.state_mut().on_media(&ctx, Command::Pause);
    assert_eq!(h.state().now_playing().status, Status::Paused);
    h.state_mut().on_media(&ctx, Command::PlayPause);
    assert_eq!(h.state().now_playing().status, Status::Playing);
    let length = h.state().now_playing().length;
    assert_eq!(length, crate::speech::spoken_length("A long read.", 1.0));
    h.state_mut().on_progress(1, 0.5);
    assert_eq!(h.state().position().as_millis(), (length / 2).as_millis());
    h.state_mut().on_media(&ctx, Command::SeekTo(0.25));
    wait_until(&mut h, "a seek", |_| {
        scratch.read("calls").contains(r#""from":0.25"#)
    });
    h.state_mut().on_media(&ctx, Command::Stop);
    wait_for(&mut h, "Read aloud");
    assert_eq!(h.state().now_playing().status, Status::Stopped);
}

#[test]
fn a_new_setting_takes_over_the_reading_from_where_it_is() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "One. Two. Three. Four.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.state_mut().on_progress(1, 0.5);
    h.run_steps(2);
    assert_eq!(h.state().reading.as_ref().unwrap().id, 1, "nothing changed");
    h.state_mut().speed = 1.5;
    wait_until(&mut h, "a faster speak from the middle", |_| {
        let calls = scratch.read("calls");
        calls.contains(r#""from":0.5"#) && calls.contains(r#""speed":1.5"#)
    });
    let ctx = h.ctx.clone();
    h.state_mut().on_media(&ctx, crate::mpris::Command::Pause);
    h.state_mut().voice = voices::voice("bm_george").unwrap();
    wait_until(&mut h, "a new voice", |_| {
        scratch.read("calls").contains("bm_george")
    });
    h.run_steps(2);
    let reading = h.state().reading.as_ref().unwrap();
    assert_eq!(reading.id, 3);
    assert!(reading.paused_at.is_some(), "still paused");
    assert_eq!(scratch.read("calls").matches(r#""op":"speak""#).count(), 3);
}

fn selected(h: &Harness<'_, App>, text: &str) -> bool {
    h.get_by_label(text).accesskit_node().is_selected() == Some(true)
}

#[test]
fn the_translation_shows_as_it_arrives_with_the_spoken_piece_marked() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    pick_language(&mut h, "Spanish");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "[es] The second piece.");
    assert!(shows(&h, "[es] The first piece."));
    assert!(
        !shows(&h, "Read me."),
        "the original gives way to the translation"
    );
    assert!(shows(&h, "Translation"));
    assert!(shows(&h, "Spanish"));
    assert!(selected(&h, "[es] The first piece."));
    assert!(!selected(&h, "[es] The second piece."));
    let id = h.state().reading.as_ref().unwrap().id;
    h.state_mut().on_progress(id, 0.7);
    h.run();
    assert!(!selected(&h, "[es] The first piece."));
    assert!(selected(&h, "[es] The second piece."));
}

#[test]
fn the_original_is_a_toggle_away_and_an_untranslated_reading_brings_it_back() {
    let scratch = Scratch::new();
    let mut h = translating_window(&scratch);
    pick_language(&mut h, "French");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "[fr-fr] The second piece.");
    h.get_by_label("Original").click();
    h.run();
    assert!(shows(&h, "Read me."));
    assert!(!shows(&h, "[fr-fr] The first piece."));
    h.get_by_label("Translation").click();
    h.run();
    assert!(shows(&h, "[fr-fr] The first piece."));
    h.get_by_role_and_label(Role::Button, "Stop").click();
    wait_for(&mut h, "Read aloud");
    assert!(
        shows(&h, "[fr-fr] The first piece."),
        "kept once the reading ends"
    );
    pick_language(&mut h, "Don't translate");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Read me.");
    assert!(!shows(&h, "[fr-fr] The first piece."));
    assert!(h.query_by_label("Original").is_none());
}

#[test]
fn a_blend_is_sent_as_one_voice_in_tenths_and_comes_back_next_run() {
    let scratch = Scratch::new();
    let settings = scratch.0.join("config/vinospeak/gui.json");
    let mut h = notes_window(&scratch, "Read me.");
    h.state_mut().remember(settings.clone());
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "Blend with");
    assert!(
        h.query_by_label("Mix").is_none(),
        "Mix stays hidden until a second voice is picked"
    );
    let state = h.state_mut();
    state.blend = voices::voice("bm_george");
    state.mix = 3;
    h.run();
    assert!(h.query_by_label("Mix").is_some());
    assert!(shows(&h, "Heart 70% · George 30%"));
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    h.run();
    let voice = h.get_by_label("Voice").accesskit_node().value();
    assert_eq!(
        voice.as_deref(),
        Some("Heart + George"),
        "the player says it is blended"
    );
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    let request = last_speak(&mut h, &scratch);
    assert!(
        request.contains(r#""voice":"af_heart(7)+bm_george(3)""#),
        "{request}"
    );
    assert!(request.contains(r#""language":"en-us""#), "{request}");

    let mut next = notes_window(&scratch, "Read me.");
    next.state_mut().remember(settings);
    let state = next.state();
    assert_eq!(
        state.blend.as_ref().map(|v| v.id.as_str()),
        Some("bm_george")
    );
    assert_eq!(state.mix, 3);
}

#[test]
fn a_new_blend_takes_over_the_reading_from_where_it_is() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "One. Two. Three.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.state_mut().blend = voices::voice("bm_george");
    wait_until(&mut h, "the blended voice", |_| {
        scratch.read("calls").contains("af_heart(5)+bm_george(5)")
    });
}

/// The window, saving notes.md with subtitles to `notes.mp3` in the scratch dir.
fn saving_window(scratch: &Scratch) -> (Harness<'static, App>, PathBuf) {
    let mut h = notes_window(scratch, "Save me.");
    let state = h.state_mut();
    state.voice = voices::voice("bm_george").unwrap();
    state.save_subtitles = true;
    let output = scratch.0.join("out/notes.mp3");
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    let ctx = h.ctx.clone();
    h.state_mut().save_to(&ctx, output.clone());
    (h, output)
}

#[test]
fn save_as_sends_the_file_the_voice_and_subtitles_and_reports_it_saved() {
    let scratch = Scratch::new();
    let (mut h, output) = saving_window(&scratch);
    wait_for(&mut h, &format!("Saved {} (1:05)", output.display()));
    assert!(output.is_file() && output.with_extension("srt").is_file());
    let calls = scratch.read("calls");
    let request = calls
        .lines()
        .find(|l| l.contains(r#""op":"save""#))
        .unwrap();
    for field in [
        format!(r#""output":"{}""#, output.display()),
        format!(
            r#""subtitles":"{}""#,
            output.with_extension("srt").display()
        ),
        r#""voice":"bm_george""#.to_owned(),
        r#""language":"en-gb""#.to_owned(),
        r#""text":"Save me.""#.to_owned(),
    ] {
        assert!(request.contains(&field), "{field} in {request}");
    }
    assert!(shows(&h, "+ .srt"));
    assert!(
        h.query_by_label("Save as MP3…").is_some(),
        "ready for another"
    );
}

#[test]
fn a_save_under_way_shows_how_far_it_is_and_can_be_cancelled() {
    let scratch = Scratch::new();
    scratch.mode("slow-save");
    let (mut h, output) = saving_window(&scratch);
    wait_for(&mut h, "Saving notes.mp3 · 50%");
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    wait_for(&mut h, "Canceled saving notes.mp3");
    assert!(scratch.read("calls").contains(r#""op":"cancel-save""#));
    assert!(!output.exists());
}

#[test]
fn a_save_that_fails_says_why_and_the_reading_is_untouched() {
    let scratch = Scratch::new();
    scratch.mode("fail-save");
    let (mut h, _) = saving_window(&scratch);
    wait_for(&mut h, "Couldn't save notes.mp3: no MP3 encoder");
    assert!(h.state().reading.is_none());
    assert!(!shows(&h, "Couldn't read"));
}

#[test]
fn saved_pronunciations_are_written_and_the_worker_reads_them_again() {
    let scratch = Scratch::new();
    let mut h = settings_with_a_model(&scratch);
    wait_for(&mut h, "Pronunciations");
    h.get_by_role_and_label(Role::Button, "Add word").click();
    h.run();
    h.get_by_label("Word 1").focus();
    h.get_by_label("Word 1").type_text("kubectl");
    h.run();
    h.get_by_label("Say word 1 as").focus();
    h.get_by_label("Say word 1 as").type_text("cube control");
    h.run();
    h.get_by_role_and_label(Role::Button, "Save pronunciations")
        .click();
    wait_for(&mut h, "Saved.");
    let written = scratch.read("config/lexicon.toml");
    assert!(
        written.contains("[words]\n\"kubectl\" = \"cube control\"\n"),
        "{written}"
    );
    wait_until(&mut h, "the worker told", |_| {
        scratch.read("calls").contains(r#""op":"lexicon""#)
    });
}

#[test]
fn a_lexicon_that_cant_be_read_is_not_overwritten() {
    let scratch = Scratch::new();
    std::fs::create_dir_all(scratch.0.join("config")).unwrap();
    std::fs::write(
        scratch.0.join("config/lexicon.toml"),
        "[words]\nkubectl = 3\n",
    )
    .unwrap();
    let mut h = settings_with_a_model(&scratch);
    wait_for(&mut h, "Couldn't read lexicon.toml");
    assert!(h.query_by_label("Add word").is_none());
    assert_eq!(
        scratch.read("config/lexicon.toml"),
        "[words]\nkubectl = 3\n"
    );
}

#[test]
fn the_speech_api_runs_while_its_switch_is_on() {
    let scratch = Scratch::new();
    let mut h = settings_with_a_model(&scratch);
    h.get_by_role_and_label(Role::Switch, "OpenAI speech API")
        .click();
    wait_for(&mut h, "Listening at http://127.0.0.1:8880/v1");
    assert!(scratch.read("calls").contains("--http 8880"));
    h.get_by_role_and_label(Role::Switch, "OpenAI speech API")
        .click();
    h.run();
    assert!(h.state().api.is_none());
    assert!(shows(&h, "Serves /v1/audio/speech on localhost:8880"));
}

#[test]
fn a_speech_api_that_cant_start_says_why_and_turns_off() {
    let scratch = Scratch::new();
    scratch.mode("fail-http");
    let mut h = settings_with_a_model(&scratch);
    h.get_by_role_and_label(Role::Switch, "OpenAI speech API")
        .click();
    wait_for(&mut h, "Stopped: [Errno 98] Address already in use");
    assert!(!h.state().api_on);
}

#[test]
fn what_is_set_once_is_on_settings_above_about() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "Read me.");
    for kept in ["Translate…", "Voice", "Speed", "Save as MP3…"] {
        assert!(
            h.query_by_label(kept).is_some(),
            "{kept} stays in the player"
        );
    }
    for moved in ["Output", "Blend with", "Mix", "Format"] {
        assert!(h.query_by_label(moved).is_none(), "{moved} is on Settings");
    }
    let settings = h.get_by_role_and_label(Role::Tab, "Settings").rect();
    let about = h.get_by_role_and_label(Role::Tab, "About").rect();
    assert!(
        settings.bottom() <= about.top(),
        "Settings sits above About"
    );

    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "Pronunciations");
    for moved in ["Output", "Blend with", "Format", "Subtitles"] {
        assert!(
            h.query_all_by_label(moved).next().is_some(),
            "{moved} on Settings"
        );
    }
    assert!(h.query_by_label("Read aloud").is_none());
    h.get_by_role_and_label(Role::Tab, "Settings").click();
    wait_for(&mut h, "Read aloud");
}

#[test]
fn read_aloud_and_save_are_disabled_while_the_model_is_deleting() {
    let scratch = Scratch::new();
    let mut h = notes_window(&scratch, "Some notes.");
    h.state_mut().model.deleting = true;
    h.run();
    assert!(
        h.get_by_role_and_label(Role::Button, "Read aloud")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        h.get_by_role_and_label(Role::Button, "Save as MP3…")
            .accesskit_node()
            .is_disabled()
    );
}
