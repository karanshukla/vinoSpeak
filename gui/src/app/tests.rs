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
use crate::search::{Hit, Index, Kind};
use crate::speech::{self, Output};
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
            // As RecentFiles describes it, so a file's text shows only in the preview.
            description: format!("{}  Changed just now", self.0.display()),
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
            description: format!("{}  Changed just now", self.0.display()),
            kind: crate::search::kind_of(&path, ""),
            path,
        }
    }

    fn image(&self, name: &str) -> Hit {
        Hit {
            path: self.0.join(name),
            name: name.to_owned(),
            description: self.0.display().to_string(),
            kind: Kind::Image,
        }
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

/// Tall enough that About's Model row is on screen, where a click reaches it.
fn tall_window(scratch: &Scratch, index: FakeIndex) -> Harness<'static, App> {
    window_sized(scratch, index, 1400.0)
}

fn window_sized(scratch: &Scratch, index: FakeIndex, height: f32) -> Harness<'static, App> {
    let voices = ["af_heart", "bm_george"]
        .into_iter()
        .filter_map(speech::voice)
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
    // Fonts set in a frame apply from the next, and the window's bold face
    // has to exist before it draws.
    let mut styled = false;
    Harness::builder()
        .with_size(egui::vec2(1100.0, height))
        .build_ui_state(
            move |ui, app: &mut App| {
                if !styled {
                    theme::apply(ui.ctx());
                    styled = true;
                    ui.ctx().request_repaint();
                    return;
                }
                app.show(ui);
            },
            app,
        )
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
fn before_a_search_the_recent_files_show_and_the_first_is_previewed() {
    let scratch = Scratch::new();
    let index = FakeIndex {
        recent: vec![
            scratch.file("notes.md", "Newest notes first."),
            scratch.file("older.md", "Older text."),
        ],
        ..Default::default()
    };
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
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "Some notes.")],
        ..Default::default()
    };
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
    let index = FakeIndex {
        recent: vec![
            scratch.file("first.md", "First file."),
            scratch.file("second.md", "Second file."),
        ],
        ..Default::default()
    };
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
    let mut index = FakeIndex {
        recent: vec![pdf.clone()],
        ..Default::default()
    };
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
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "Read me.")],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Read me.");
    let state = h.state_mut();
    state.voice = speech::voice("bm_george").unwrap();
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
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "Read me.")],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Read me.");
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
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "Twice.")],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Twice.");
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
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "A long read.")],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "A long read.");
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
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "Short.")],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Short.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Finished notes.md");
}

#[test]
fn a_failing_reading_shows_vinospeaks_error() {
    let scratch = Scratch::new();
    scratch.mode("fail-read");
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "Doomed.")],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "Doomed.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Couldn't read notes.md: boom");
}

#[test]
fn a_file_vinospeak_refuses_says_why_and_cant_be_read_aloud() {
    let scratch = Scratch::new();
    scratch.mode("fail-preview");
    let index = FakeIndex {
        recent: vec![scratch.file("blob.bin", "binary")],
        ..Default::default()
    };
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
fn vinosearch_being_down_is_shown() {
    let scratch = Scratch::new();
    let index = FakeIndex {
        down: true,
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    h.state_mut().type_query("anything");
    wait_for(&mut h, "vinoSearch didn't answer");
}

#[test]
fn a_session_is_titled_by_its_summary_and_reads_either_part() {
    let scratch = Scratch::new();
    let index = FakeIndex {
        recent: vec![scratch.session("2026-10-27-140512.vwsession", "session_v1.vwsession")],
        ..Default::default()
    };
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
    let index = FakeIndex {
        recent: vec![scratch.session(
            "2026-10-27-140512.vwsession",
            "session_v1_nosummary.vwsession",
        )],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "The transcript of the session.");
    assert!(shows(&h, "27 October 2026, 14:05"));
    assert!(shows(&h, "Not summarised yet"));
    assert!(!scratch.read("calls").contains("--part summary"));
}

#[test]
fn about_shows_the_other_apps_until_a_file_is_picked() {
    let scratch = Scratch::new();
    let index = FakeIndex {
        recent: vec![
            scratch.file("first.md", "First file."),
            scratch.file("second.md", "Second file."),
        ],
        ..Default::default()
    };
    let mut h = window(&scratch, index);
    wait_for(&mut h, "First file.");
    h.get_by_label("About vinoSpeak").click();
    wait_for(&mut h, "Learn more vinoSearch");
    assert!(shows(&h, "vinopilot.ca"));
    assert!(!shows(&h, "Learn more vinoSpeak"));
    h.get_by_label("second.md").click();
    wait_for(&mut h, "Second file.");
    assert!(!shows(&h, "Learn more vinoSearch"));
}

#[test]
fn about_deletes_the_model_after_confirming_in_place() {
    let scratch = Scratch::new();
    scratch.model();
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "Some notes.")],
        ..Default::default()
    };
    let mut h = tall_window(&scratch, index);
    wait_for(&mut h, "Some notes.");
    // The preview starts the worker, which holds the model open.
    wait_until(&mut h, "the worker", |_| {
        scratch.read("calls").contains("--serve")
    });
    h.get_by_label("About vinoSpeak").click();
    wait_for(&mut h, "2 MB on disk");

    h.get_by_role_and_label(Role::Button, "Delete…").click();
    wait_for(&mut h, "Deletes 2 MB");
    h.get_by_role_and_label(Role::Button, "Cancel").click();
    wait_for(&mut h, "2 MB on disk");
    assert!(!scratch.read("calls").contains("--delete-model"));

    h.get_by_role_and_label(Role::Button, "Delete…").click();
    wait_for(&mut h, "Delete the model");
    h.get_by_role_and_label(Role::Button, "Delete the model")
        .click();
    wait_for(&mut h, "Not downloaded");
    assert!(shows(&h, "Deleted the model and its compiled cache"));
    assert!(h.state().worker.is_none(), "the worker let go of the model");
    let delete = h.get_by_role_and_label(Role::Button, "Delete…");
    assert!(delete.accesskit_node().is_disabled(), "nothing to delete");
    let voices: Vec<_> = h.state().voices.iter().map(|v| v.id.clone()).collect();
    assert_eq!(voices, ["af_heart"]);

    // The next reading starts a worker again, which downloads the model.
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
    let index = FakeIndex {
        recent: vec![scratch.file("notes.md", "A long read.")],
        ..Default::default()
    };
    let mut h = tall_window(&scratch, index);
    wait_for(&mut h, "A long read.");
    h.get_by_role_and_label(Role::Button, "Read aloud").click();
    wait_for(&mut h, "Stop");
    h.get_by_label("About vinoSpeak").click();
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
    h.get_by_label("About vinoSpeak").click();
    wait_for(&mut h, "2 MB on disk");
    h.get_by_role_and_label(Role::Button, "Delete…").click();
    wait_for(&mut h, "Delete the model");
    h.get_by_role_and_label(Role::Button, "Delete the model")
        .click();
    wait_for(&mut h, "Couldn't delete it: The model is downloading.");
    assert!(scratch.0.join("cache/ov-cache/blob").is_file());
}
