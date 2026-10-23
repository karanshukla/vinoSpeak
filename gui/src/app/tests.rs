//! The window driven headless through egui_kittest, finding widgets by their
//! AccessKit labels as a screen reader would. vinoSearch is a fake `Index`;
//! vinospeak is a shell script that prints `--print-text`, and otherwise
//! logs its arguments and "plays" until it's interrupted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use eframe::egui::{self, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::App;
use crate::search::{Hit, Index, Kind};
use crate::speech::{self, Output};
use crate::theme;

const FAKE_VINOSPEAK: &str = r#"#!/bin/sh
# argv: --file PATH|- [--print-text | --voice V --language L --speed S [--audio-device D]]
dir=$(dirname "$0")
echo "$*" >> "$dir/calls"
case " $* " in
*" --print-text "*)
    [ -f "$dir/fail-preview" ] && { echo "vinospeak: error: not a UTF-8 text file." >&2; exit 2; }
    if [ "$2" = "-" ]; then cat; else cat "$2"; fi
    exit 0 ;;
esac
[ "$2" = "-" ] && cat > "$dir/stdin"
[ -f "$dir/fail-read" ] && { echo "vinospeak: error: boom" >&2; exit 2; }
[ -f "$dir/quick" ] && exit 0
trap 'echo interrupted >> "$dir/calls"; exit 130' INT
i=0
while [ $i -lt 300 ]; do sleep 0.1; i=$((i + 1)); done
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

    fn recent(&self, _limit: u32) -> Result<Vec<Hit>> {
        if self.down {
            bail!("vinoSearch isn't running or isn't installed");
        }
        Ok(self.recent.clone())
    }

    fn file_text(&self, path: &Path) -> Result<String> {
        match self.documents.get(path) {
            Some(text) => Ok(text.clone()),
            None => bail!("{} is not an indexed file", path.display()),
        }
    }
}

fn window(scratch: &Scratch, index: FakeIndex) -> Harness<'static, App> {
    let voices = ["af_heart", "bm_george"]
        .into_iter()
        .filter_map(speech::voice)
        .collect();
    let outputs = vec![Output {
        name: "hdmi_sink".to_owned(),
        description: "HDMI 1".to_owned(),
    }];
    let app = App::with(
        Arc::new(index),
        scratch.0.join("vinospeak"),
        voices,
        (outputs, Some("hdmi_sink".to_owned())),
    );
    // Fonts set in a frame apply from the next, and the window's bold face
    // has to exist before it draws.
    let mut styled = false;
    Harness::builder()
        .with_size(egui::vec2(1100.0, 700.0))
        .build_state(
            move |ctx, app: &mut App| {
                if !styled {
                    theme::style(ctx);
                    styled = true;
                    return;
                }
                app.ui(ctx);
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
    assert!(shows(&h, "RECENTLY CHANGED"));
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
    assert!(!shows(&h, "RECENTLY CHANGED"));
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
    assert!(shows(&h, "RECENTLY CHANGED"));
    assert!(shows(&h, "notes.md"));
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
    assert!(shows(&h, "DOCUMENT"));
    h.get_by_label_contains("Read aloud").click();
    wait_until(&mut h, "the text on stdin", |_| {
        scratch.read("stdin").contains("Extracted page one.")
    });
    assert!(scratch.read("calls").contains("--file -"));
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
    h.get_by_label_contains("Read aloud").click();
    wait_for(&mut h, "Stop");
    let calls = scratch.read("calls");
    let reading = calls.lines().last().unwrap();
    assert!(
        reading
            .ends_with("--voice bm_george --language en-gb --speed 1.5 --audio-device hdmi_sink"),
        "{reading}"
    );
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
    h.get_by_label_contains("Read aloud").click();
    wait_for(&mut h, "Stop");
    let calls = scratch.read("calls");
    assert!(
        calls
            .lines()
            .last()
            .unwrap()
            .contains("--voice af_heart --language en-us")
    );
    assert!(!calls.contains("--audio-device"));
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
    h.get_by_label_contains("Read aloud").click();
    wait_for(&mut h, "Stop");
    h.get_by_label_contains("Stop").click();
    wait_for(&mut h, "Read aloud");
    assert!(
        scratch.read("calls").contains("interrupted"),
        "Ctrl+C, not a kill"
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
    h.get_by_label_contains("Read aloud").click();
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
    h.get_by_label_contains("Read aloud").click();
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
    h.get_by_label_contains("Read aloud").click();
    h.run_steps(5);
    assert!(
        !scratch.read("calls").contains("--voice"),
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
