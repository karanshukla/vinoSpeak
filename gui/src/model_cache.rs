//! vinospeak's cache directory as the window reads it: where it is, how much
//! is in it, and how a size is shown. vino_speak/model.py and cli.py hold the
//! same rules in Python; tests/fixtures/shared_with_window.json is the table
//! both are tested against (docs/architecture.md, "Shared with the window").

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use crate::speech;

/// The downloaded model's directory in the cache (model.py's `default_model_dir()`).
pub const MODEL_DIR: &str = "kokoro-82m-int8-ov";

/// Where vinospeak keeps the model it downloaded and OpenVINO's compiled copy.
pub fn dir() -> Option<PathBuf> {
    dir_from(std::env::var_os("XDG_CACHE_HOME"), std::env::var_os("HOME"))
}

/// `$XDG_CACHE_HOME/vinospeak`, or `~/.cache/vinospeak` when that is unset or empty.
fn dir_from(xdg_cache_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    xdg_cache_home
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|home| Path::new(&home).join(".cache")))
        .map(|cache| cache.join("vinospeak"))
}

/// Bytes in the files under `path`, not following links; 0 when it isn't there.
pub fn disk_usage(path: &Path) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|file| file.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

/// A size as people say it: 163 MB, 1.2 GB.
pub fn size(bytes: u64) -> String {
    match bytes {
        1_000_000_000.. => format!("{:.1} GB", bytes as f64 / 1e9),
        1_000_000.. => format!("{} MB", (bytes as f64 / 1e6).round()),
        _ => format!("{} kB", (bytes as f64 / 1e3).round()),
    }
}

/// `vinospeak --delete-model`: what it said it deleted, or why it couldn't.
pub fn delete(vinospeak: &Path) -> Result<String> {
    let out = Command::new(vinospeak)
        .arg("--delete-model")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> serde_json::Value {
        let table =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/shared_with_window.json");
        serde_json::from_str(&std::fs::read_to_string(table).unwrap()).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vinospeak-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sizes_read_as_people_say_them() {
        assert_eq!(size(999), "1 kB");
        assert_eq!(size(163_400_000), "163 MB");
        assert_eq!(size(1_260_000_000), "1.3 GB");
    }

    /// The same table tests/test_cli.py runs `cli.size` through.
    #[test]
    fn sizes_are_shown_as_vinospeak_shows_them() {
        for case in shared()["sizes"].as_array().unwrap() {
            let (bytes, shown) = (case[0].as_u64().unwrap(), case[1].as_str().unwrap());
            assert_eq!(size(bytes), shown, "{bytes} bytes");
        }
    }

    /// The same table tests/test_model.py runs `model.cache_dir` through.
    #[test]
    fn the_cache_is_where_vinospeak_keeps_it() {
        let shared = shared();
        assert_eq!(shared["model_dir"], MODEL_DIR);
        for case in shared["cache_dirs"].as_array().unwrap() {
            let var = |name: &str| case[name].as_str().map(OsString::from);
            assert_eq!(
                dir_from(var("xdg_cache_home"), var("home")),
                Some(PathBuf::from(case["cache_dir"].as_str().unwrap())),
                "{case}"
            );
        }
    }

    #[test]
    fn with_no_home_and_no_xdg_cache_there_is_no_cache() {
        assert_eq!(dir_from(None, None), None);
        assert_eq!(dir_from(Some(OsString::new()), None), None);
    }

    #[test]
    fn disk_usage_adds_up_a_tree_and_an_absent_one_is_empty() {
        let dir = std::env::temp_dir().join(format!("vinospeak-du-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("one"), [0; 10]).unwrap();
        std::fs::write(dir.join("a/b/two"), [0; 5]).unwrap();
        assert_eq!(disk_usage(&dir), 15);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(disk_usage(&dir), 0);
    }

    #[test]
    fn disk_usage_counts_neither_a_link_nor_what_it_points_to() {
        let dir = scratch("du-links");
        std::fs::create_dir_all(dir.join("tree")).unwrap();
        std::fs::create_dir_all(dir.join("outside/deep")).unwrap();
        std::fs::write(dir.join("tree/one"), [0; 10]).unwrap();
        std::fs::write(dir.join("outside/big"), [0; 1000]).unwrap();
        std::fs::write(dir.join("outside/deep/bigger"), [0; 2000]).unwrap();
        std::os::unix::fs::symlink(dir.join("outside/big"), dir.join("tree/file-link")).unwrap();
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("tree/dir-link")).unwrap();
        assert_eq!(disk_usage(&dir.join("tree")), 10);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
