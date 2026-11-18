//! vinoWhisper's sessions found on disk, so the window lists them without
//! vinoSearch: a native install's folder and its Flatpak's, the two
//! vinoSearch's index.rs watches.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::search::{self, Hit};

const FLATPAK: &str = "io.github.karanshukla.vinowhisper";

/// Where vinoWhisper keeps its sessions: under `XDG_DATA_HOME` (else
/// `~/.local/share`) natively, under `~/.var/app` as a Flatpak.
pub fn dirs(home: Option<&Path>, data_home: Option<&Path>) -> Vec<PathBuf> {
    let native = data_home
        .filter(|d| d.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|h| h.join(".local/share")));
    let flatpak = home.map(|h| h.join(".var/app").join(FLATPAK).join("data"));
    native
        .into_iter()
        .chain(flatpak)
        .map(|data| data.join("vinowhisper/transcripts"))
        .collect()
}

/// `dirs` for this user.
pub fn user_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    dirs(home.as_deref(), data_home.as_deref())
}

fn changed(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn seconds(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Every session in `dirs`, described as vinoSearch describes a recent file.
/// A missing folder is just empty.
pub fn found(dirs: &[PathBuf]) -> Vec<Hit> {
    let now = seconds(SystemTime::now());
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| search::extension(path) == "vwsession")
        .map(|path| {
            let mtime = changed(&path).map_or(0, seconds);
            search::recent_hit(path, "text", mtime, now)
        })
        .collect()
}

/// vinoSearch's recent files and the sessions on disk as one list, newest
/// first, each path once.
pub fn merge(indexed: Vec<Hit>, on_disk: Vec<Hit>) -> Vec<Hit> {
    let mut seen = HashSet::new();
    let mut all: Vec<(Option<SystemTime>, Hit)> = indexed
        .into_iter()
        .chain(on_disk)
        .filter(|hit| seen.insert(hit.path.clone()))
        .map(|hit| (changed(&hit.path), hit))
        .collect();
    all.sort_by_key(|(changed, _)| std::cmp::Reverse(*changed));
    all.into_iter().map(|(_, hit)| hit).collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::search::Kind;

    #[test]
    fn sessions_are_looked_for_natively_and_in_the_flatpak() {
        assert_eq!(
            dirs(Some(Path::new("/h")), None),
            [
                PathBuf::from("/h/.local/share/vinowhisper/transcripts"),
                PathBuf::from(
                    "/h/.var/app/io.github.karanshukla.vinowhisper/data/vinowhisper/transcripts"
                ),
            ]
        );
    }

    #[test]
    fn xdg_data_home_moves_the_native_folder_only_when_absolute() {
        let moved = dirs(Some(Path::new("/h")), Some(Path::new("/data")));
        assert_eq!(moved[0], PathBuf::from("/data/vinowhisper/transcripts"));
        let relative = dirs(Some(Path::new("/h")), Some(Path::new("data")));
        assert_eq!(
            relative[0],
            PathBuf::from("/h/.local/share/vinowhisper/transcripts")
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vinospeak-sessions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, age: Duration) {
        let file = std::fs::File::create(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn only_sessions_are_found_and_a_missing_folder_is_empty() {
        let dir = scratch("found");
        touch(&dir.join("2026-11-15-074942.vwsession"), Duration::ZERO);
        touch(&dir.join("notes.md"), Duration::ZERO);
        let hits = found(&[dir.clone(), dir.join("missing")]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, Kind::Session { summarised: false });
        assert!(hits[0].description.ends_with("Changed just now"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn merged_newest_first_with_each_path_once() {
        let dir = scratch("merge");
        let (old, new, both) = (
            dir.join("old.md"),
            dir.join("new.vwsession"),
            dir.join("both.vwsession"),
        );
        touch(&old, Duration::from_secs(300));
        touch(&new, Duration::ZERO);
        touch(&both, Duration::from_secs(60));
        let hit = |path: &Path| search::recent_hit(path.to_path_buf(), "text", 0, 0);
        let merged = merge(vec![hit(&old), hit(&both)], vec![hit(&both), hit(&new)]);
        let paths: Vec<_> = merged.iter().map(|h| h.path.clone()).collect();
        assert_eq!(paths, [new, both, old]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
