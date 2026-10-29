//! vinoSearch over D-Bus: the same search GNOME Shell's overview runs, so
//! any running vinoSearch answers, whichever desktop it was set up for.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use zbus::OwnedVariant;

const SERVICE: &str = "dev.vinosearch.Runner";
const PATH: &str = "/gnome";
const INTERFACE: &str = "org.gnome.Shell.SearchProvider2";
const CONTROL_PATH: &str = "/control";
const CONTROL: &str = "dev.vinosearch.Control1";
/// vinoSearch's document formats (its src/extract.rs). vinoSpeak reads
/// UTF-8 text only, so their text comes from vinoSearch's extractor.
const DOCUMENTS: &[&str] = &[
    "pdf", "docx", "odt", "pptx", "odp", "xlsx", "xls", "ods", "csv", "tsv",
];

/// What the window asks vinoSearch. A trait so the tests can stand in for
/// a running vinoSearch.
pub trait Index: Send + Sync {
    fn search(&self, query: &str) -> Result<Vec<Hit>>;
    fn recent(&self, limit: u32) -> Result<Vec<Hit>>;
    fn file_text(&self, path: &Path) -> Result<String>;
}

/// The vinoSearch runner on the session bus.
pub struct VinoSearch;

impl Index for VinoSearch {
    fn search(&self, query: &str) -> Result<Vec<Hit>> {
        search(query)
    }

    fn recent(&self, limit: u32) -> Result<Vec<Hit>> {
        recent(limit)
    }

    fn file_text(&self, path: &Path) -> Result<String> {
        file_text(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Document,
    Image,
    /// A vinoWhisper session (`.vwsession`): a transcript, and a summary
    /// once it has been summarised.
    Session {
        summarised: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub path: PathBuf,
    pub name: String,
    /// The directory, line and snippet, as vinoSearch describes the hit.
    pub description: String,
    pub kind: Kind,
}

pub fn kind_of(path: &Path, gicon: &str) -> Kind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "vwsession" {
        Kind::Session { summarised: false }
    } else if gicon.contains(" image-") {
        Kind::Image
    } else if DOCUMENTS.contains(&ext.as_str()) {
        Kind::Document
    } else {
        Kind::Text
    }
}

fn string(meta: &HashMap<String, OwnedVariant>, key: &str) -> String {
    meta.get(key)
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default()
}

fn search(query: &str) -> Result<Vec<Hit>> {
    zbus::block_on(async {
        let conn = zbus::Connection::session()
            .await
            .context("connecting to the session bus")?;
        let ids: Vec<String> = conn
            .call_method(
                Some(SERVICE),
                PATH,
                Some(INTERFACE),
                "GetInitialResultSet",
                &(vec![query.to_owned()],),
            )
            .await
            .context("vinoSearch isn't running or isn't installed")?
            .body()
            .deserialize()?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let metas: Vec<HashMap<String, OwnedVariant>> = conn
            .call_method(
                Some(SERVICE),
                PATH,
                Some(INTERFACE),
                "GetResultMetas",
                &(ids,),
            )
            .await?
            .body()
            .deserialize()?;
        Ok(metas
            .iter()
            .map(|meta| {
                let path = PathBuf::from(string(meta, "id"));
                Hit {
                    kind: kind_of(&path, &string(meta, "gicon")),
                    name: string(meta, "name"),
                    description: string(meta, "description"),
                    path,
                }
            })
            .collect())
    })
}

/// The indexed text files and documents changed most recently, for a window
/// that has nothing searched yet.
fn recent(limit: u32) -> Result<Vec<Hit>> {
    zbus::block_on(async {
        let conn = zbus::Connection::session()
            .await
            .context("connecting to the session bus")?;
        let files: Vec<(String, String, i64)> = conn
            .call_method(
                Some(SERVICE),
                CONTROL_PATH,
                Some(CONTROL),
                "RecentFiles",
                &(limit,),
            )
            .await
            .context("this vinoSearch can't list recent files")?
            .body()
            .deserialize()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        Ok(files
            .into_iter()
            .map(|(path, kind, mtime)| {
                let path = PathBuf::from(path);
                let dir = path
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                Hit {
                    name: path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                    description: format!("{dir}  Changed {}", ago(now - mtime)),
                    kind: if kind == "document" {
                        Kind::Document
                    } else {
                        kind_of(&path, "")
                    },
                    path,
                }
            })
            .collect())
    })
}

/// A document's text, extracted by vinoSearch as it does for indexing.
fn file_text(path: &Path) -> Result<String> {
    zbus::block_on(async {
        let conn = zbus::Connection::session()
            .await
            .context("connecting to the session bus")?;
        let text: String = conn
            .call_method(
                Some(SERVICE),
                CONTROL_PATH,
                Some(CONTROL),
                "FileText",
                &(path.to_string_lossy().as_ref(),),
            )
            .await
            .context("vinoSearch couldn't extract the text")?
            .body()
            .deserialize()?;
        Ok(text)
    })
}

/// The most of a summary read for its title; vinoWhisper's are a few KiB.
const SUMMARY_PEEK: u64 = 64 * 1024;

/// Each session titled by its summary's `# ` line, else by when it was
/// recorded, and marked with whether it has a summary. vinoSearch names a
/// session by its file name, which is only a time stamp.
pub fn title_sessions(hits: Vec<Hit>) -> Vec<Hit> {
    hits.into_iter()
        .map(|mut hit| {
            if let Kind::Session { .. } = hit.kind {
                let summary = summary(&hit.path);
                hit.kind = Kind::Session {
                    summarised: summary.is_some(),
                };
                if let Some(title) = summary
                    .as_deref()
                    .and_then(summary_title)
                    .or_else(|| recorded(&hit.path))
                {
                    hit.name = title;
                }
            }
            hit
        })
        .collect()
}

/// The start of a session's summary.md; None without one, or for a bundle
/// that won't open (vinospeak says why when it is read).
fn summary(path: &Path) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut bytes = Vec::new();
    let entry = zip.by_name("summary.md").ok()?;
    entry.take(SUMMARY_PEEK).read_to_end(&mut bytes).ok()?;
    // A cut can split a character; the title is on the first lines anyway.
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn summary_title(summary: &str) -> Option<String> {
    summary
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty())
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "27 October 2026, 14:05" from vinoWhisper's `2026-10-27-140512.vwsession`.
fn recorded(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let field = |range: std::ops::Range<usize>| stem.get(range)?.parse::<u32>().ok();
    let (year, month, day) = (field(0..4)?, field(5..7)?, field(8..10)?);
    let (hour, minute) = (field(11..13)?, field(13..15)?);
    let month = MONTHS.get(usize::try_from(month).ok()?.checked_sub(1)?)?;
    Some(format!("{day} {month} {year}, {hour:02}:{minute:02}"))
}

fn ago(secs: i64) -> String {
    let plural = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs.max(0) {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => plural(s / 60, "minute"),
        s if s < 86_400 => plural(s / 3600, "hour"),
        s if s < 86_400 * 60 => plural(s / 86_400, "day"),
        s => plural(s / (86_400 * 30), "month"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_come_from_the_icon_and_the_extension() {
        let kind = |p: &str, icon: &str| kind_of(Path::new(p), icon);
        assert_eq!(
            kind("/a/IMG.jpg", ". GThemedIcon image-jpeg image-x-generic"),
            Kind::Image
        );
        assert_eq!(
            kind("/a/b.PDF", ". GThemedIcon application-pdf text-x-generic"),
            Kind::Document
        );
        assert_eq!(
            kind("/a/b.md", ". GThemedIcon text-markdown text-x-generic"),
            Kind::Text
        );
        assert_eq!(kind("/a/Makefile", ""), Kind::Text);
        assert_eq!(
            kind(
                "/t/2026-10-27-140512.vwsession",
                ". GThemedIcon x text-x-generic"
            ),
            Kind::Session { summarised: false }
        );
    }

    #[test]
    fn sessions_are_titled_by_their_summary_else_by_when() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures");
        let hit = |path: PathBuf| Hit {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            description: String::new(),
            kind: kind_of(&path, ""),
            path,
        };
        let dir = std::env::temp_dir().join(format!("vinospeak-titles-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let unsummarised = dir.join("2026-10-27-140512.vwsession");
        std::fs::copy(
            fixtures.join("session_v1_nosummary.vwsession"),
            &unsummarised,
        )
        .unwrap();
        let broken = dir.join("notes.vwsession");
        std::fs::write(&broken, "not a zip").unwrap();
        let hits = title_sessions(vec![
            hit(fixtures.join("session_v1.vwsession")),
            hit(unsummarised),
            hit(broken),
            hit(PathBuf::from("/a/b.md")),
        ]);
        let _ = std::fs::remove_dir_all(&dir);
        let shown: Vec<_> = hits.iter().map(|h| (h.name.as_str(), h.kind)).collect();
        assert_eq!(
            shown,
            [
                (
                    "Local captioning on the NPU",
                    Kind::Session { summarised: true }
                ),
                (
                    "27 October 2026, 14:05",
                    Kind::Session { summarised: false }
                ),
                ("notes.vwsession", Kind::Session { summarised: false }),
                ("b.md", Kind::Text),
            ]
        );
        assert_eq!(recorded(Path::new("/t/2026-13-01-000000.vwsession")), None);
    }

    #[test]
    fn ages_read_like_a_person_would_say_them() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(7200), "2 hours ago");
        assert_eq!(ago(86_400 * 3), "3 days ago");
        assert_eq!(ago(86_400 * 90), "3 months ago");
    }
}
