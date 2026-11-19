//! The translation model as the window reads it: where it is kept (vinoWhisper's
//! folder, so the two apps share one copy), whether it is downloaded, the languages it translates into, and
//! downloading or deleting it through vinospeak. vino_speak/translate.py
//! holds the same rules; tests/fixtures/shared_with_window.json is the table
//! both are tested against.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::speech;

/// What it is called on Settings.
pub const LABEL: &str = "SmolLM3-3B";
/// The model's folder (translate.py's `model_dir()`).
pub const MODEL_DIR: &str = "SmolLM3-3B-int4-cw-ov";
/// The pinned revision, which vinoWhisper writes beside a copy it checked.
pub const REVISION: &str = "136eac7690714ab0a1e65806eb24f98d993f0438";
/// Written in the model's folder once every file matched its pin (model.py's STAMP).
const VERIFIED: &str = ".vinospeak-verified";
/// What vinoWhisper writes: the revision after checking it, a stamp once it used it.
const VINOWHISPER_REVISION: &str = ".vinowhisper-revision";
const VINOWHISPER_VERIFIED: &str = ".vinowhisper-verified";
/// vinoWhisper's Flatpak, whose copy is read but never deleted from here.
const VINOWHISPER_FLATPAK: &str = "io.github.karanshukla.vinowhisper";
/// What the download adds up to (translate.py's `download_bytes()`).
pub const DOWNLOAD_BYTES: u64 = 1_702_009_152;

/// A language it translates into: vinospeak's name for it, what the window
/// calls it, and the voice that reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub language: &'static str,
    pub name: &'static str,
    pub voice: &'static str,
}

/// translate.py's TARGETS, in the picker's order.
pub const TARGETS: &[Target] = &[
    Target {
        language: "es",
        name: "Spanish",
        voice: "ef_dora",
    },
    Target {
        language: "fr-fr",
        name: "French",
        voice: "ff_siwis",
    },
    Target {
        language: "it",
        name: "Italian",
        voice: "if_sara",
    },
    Target {
        language: "pt-br",
        name: "Brazilian Portuguese",
        voice: "pf_dora",
    },
    Target {
        language: "en-us",
        name: "American English",
        voice: "af_heart",
    },
    Target {
        language: "en-gb",
        name: "British English",
        voice: "bf_emma",
    },
];

pub fn target(language: &str) -> Option<Target> {
    TARGETS.iter().copied().find(|t| t.language == language)
}

/// The first downloaded copy, native before the Flatpak's, else the native
/// folder (translate.py's `model_dir()`).
pub fn dir() -> Option<PathBuf> {
    let found = dirs_from(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"));
    found
        .iter()
        .find(|dir| downloaded(dir))
        .or(found.first())
        .cloned()
}

/// `$XDG_DATA_HOME/vinowhisper/models/SmolLM3-3B-int4-cw-ov` (or under
/// `~/.local/share`), then the same inside vinoWhisper's Flatpak.
fn dirs_from(xdg_data_home: Option<OsString>, home: Option<OsString>) -> Vec<PathBuf> {
    let models = |data: PathBuf| data.join("vinowhisper").join("models").join(MODEL_DIR);
    let home = home.map(PathBuf::from);
    let native = xdg_data_home
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".local/share")))
        .map(models);
    let flatpak =
        home.map(|home| models(home.join(".var/app").join(VINOWHISPER_FLATPAK).join("data")));
    native.into_iter().chain(flatpak).collect()
}

/// Whether vinospeak or vinoWhisper finished downloading and checking it.
pub fn downloaded(model: &Path) -> bool {
    model.join(VERIFIED).is_file()
        || std::fs::read_to_string(model.join(VINOWHISPER_REVISION))
            .is_ok_and(|revision| revision.trim() == REVISION)
}

/// Whether vinoWhisper uses it, so deleting it here would take it from vinoWhisper.
pub fn used_by_vinowhisper(model: &Path) -> bool {
    model.join(VINOWHISPER_VERIFIED).is_file()
        || model
            .components()
            .any(|c| c.as_os_str() == VINOWHISPER_FLATPAK)
}

fn run(vinospeak: &Path, flag: &str) -> Result<String> {
    let out = Command::new(vinospeak)
        .arg(flag)
        .stdin(Stdio::null())
        .output()
        .context(speech::NOT_RUNNABLE)?;
    if !out.status.success() {
        bail!(
            "{}",
            speech::last_line(&String::from_utf8_lossy(&out.stderr))
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// `vinospeak --download-translation-model`: what it said, or why it failed.
pub fn download(vinospeak: &Path) -> Result<String> {
    run(vinospeak, "--download-translation-model")
}

/// `vinospeak --delete-translation-model`.
pub fn delete(vinospeak: &Path) -> Result<String> {
    run(vinospeak, "--delete-translation-model")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> serde_json::Value {
        let table =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/shared_with_window.json");
        serde_json::from_str(&std::fs::read_to_string(table).unwrap()).unwrap()
    }

    #[test]
    fn the_targets_voices_and_size_are_the_ones_vinospeak_has() {
        let table = shared();
        let translation = &table["translation"];
        assert_eq!(translation["model_dir"], MODEL_DIR);
        assert_eq!(translation["revision"], REVISION);
        assert_eq!(translation["download_bytes"], DOWNLOAD_BYTES);
        let mut theirs: Vec<(String, String)> = translation["targets"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(language, voice)| (language.clone(), voice.as_str().unwrap().to_owned()))
            .collect();
        let mut ours: Vec<(String, String)> = TARGETS
            .iter()
            .map(|t| (t.language.to_owned(), t.voice.to_owned()))
            .collect();
        theirs.sort();
        ours.sort();
        assert_eq!(ours, theirs);
    }

    #[test]
    fn the_folders_are_vinowhispers_native_then_its_flatpaks() {
        let home = Some(OsString::from("/home/k"));
        let native = PathBuf::from("/home/k/.local/share/vinowhisper/models/SmolLM3-3B-int4-cw-ov");
        let flatpak = PathBuf::from(
            "/home/k/.var/app/io.github.karanshukla.vinowhisper/data/vinowhisper/models/SmolLM3-3B-int4-cw-ov",
        );
        assert_eq!(
            dirs_from(None, home.clone()),
            [native.clone(), flatpak.clone()]
        );
        assert_eq!(
            dirs_from(Some(OsString::new()), home.clone()),
            [native, flatpak.clone()]
        );
        assert_eq!(
            dirs_from(Some("/x/data".into()), home),
            [
                PathBuf::from("/x/data/vinowhisper/models/SmolLM3-3B-int4-cw-ov"),
                flatpak.clone()
            ]
        );
        assert!(used_by_vinowhisper(&flatpak), "never deleted from here");
    }

    fn scratch(name: &str) -> PathBuf {
        let model = std::env::temp_dir().join(format!(
            "vinospeak-translation-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("openvino_model.bin"), "").unwrap();
        model
    }

    #[test]
    fn only_a_checked_download_counts_as_downloaded() {
        let model = scratch("ours");
        assert!(!downloaded(&model));
        std::fs::write(model.join(VERIFIED), "").unwrap();
        assert!(downloaded(&model));
        let _ = std::fs::remove_dir_all(&model);
    }

    #[test]
    fn vinowhispers_copy_counts_only_at_the_pinned_revision() {
        let model = scratch("theirs");
        std::fs::write(model.join(VINOWHISPER_REVISION), "0000\n").unwrap();
        assert!(!downloaded(&model));
        std::fs::write(model.join(VINOWHISPER_REVISION), format!("{REVISION}\n")).unwrap();
        assert!(downloaded(&model));
        assert!(!used_by_vinowhisper(&model));
        std::fs::write(model.join(VINOWHISPER_VERIFIED), "").unwrap();
        assert!(used_by_vinowhisper(&model));
        let _ = std::fs::remove_dir_all(&model);
    }

    #[test]
    fn every_target_names_its_own_voice() {
        for t in TARGETS {
            let voice = crate::voices::voice(t.voice).unwrap();
            assert_eq!(voice.language, t.language, "{}", t.voice);
        }
        assert_eq!(target("es").unwrap().name, "Spanish");
        assert_eq!(target("hi"), None);
    }
}
