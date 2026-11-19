//! The translation model as the window reads it: where vinospeak keeps it,
//! whether it is downloaded, the languages it translates into, and
//! downloading or deleting it through vinospeak. vino_speak/translate.py
//! holds the same rules; tests/fixtures/shared_with_window.json is the table
//! both are tested against.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::speech;

/// What it is called on About.
pub const LABEL: &str = "SmolLM3-3B";
/// The model's folder under `dir()` (translate.py's `model_dir()`).
pub const MODEL_DIR: &str = "SmolLM3-3B-int4-cw-ov";
/// Written in the model's folder once every file matched its pin (model.py's STAMP).
const VERIFIED: &str = ".vinospeak-verified";
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

/// `$XDG_DATA_HOME/vinospeak/translation`, or under `~/.local/share`.
pub fn dir() -> Option<PathBuf> {
    dir_from(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

fn dir_from(xdg_data_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    xdg_data_home
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|home| Path::new(&home).join(".local/share")))
        .map(|data| data.join("vinospeak").join("translation"))
}

/// Whether vinospeak finished downloading and checking it.
pub fn downloaded(dir: &Path) -> bool {
    dir.join(MODEL_DIR).join(VERIFIED).is_file()
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
    fn the_folder_is_vinospeaks_data_not_its_cache() {
        let home = Some(OsString::from("/home/k"));
        assert_eq!(
            dir_from(None, home.clone()),
            Some(PathBuf::from("/home/k/.local/share/vinospeak/translation"))
        );
        assert_eq!(
            dir_from(Some(OsString::new()), home.clone()),
            Some(PathBuf::from("/home/k/.local/share/vinospeak/translation"))
        );
        assert_eq!(
            dir_from(Some("/x/data".into()), home),
            Some(PathBuf::from("/x/data/vinospeak/translation"))
        );
    }

    #[test]
    fn only_a_checked_download_counts_as_downloaded() {
        let dir =
            std::env::temp_dir().join(format!("vinospeak-translation-{}", std::process::id()));
        let model = dir.join(MODEL_DIR);
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("openvino_model.bin"), "").unwrap();
        assert!(!downloaded(&dir));
        std::fs::write(model.join(VERIFIED), "").unwrap();
        assert!(downloaded(&dir));
        let _ = std::fs::remove_dir_all(&dir);
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
