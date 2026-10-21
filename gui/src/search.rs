//! vinoSearch over D-Bus: the same search GNOME Shell's overview runs, so
//! any running vinoSearch answers, whichever desktop it was set up for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use zbus::zvariant::OwnedValue;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Document,
    Image,
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
    if gicon.contains(" image-") {
        Kind::Image
    } else if DOCUMENTS.contains(&ext.as_str()) {
        Kind::Document
    } else {
        Kind::Text
    }
}

fn string(meta: &HashMap<String, OwnedValue>, key: &str) -> String {
    meta.get(key)
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default()
}

pub fn search(query: &str) -> Result<Vec<Hit>> {
    let conn = zbus::blocking::Connection::session().context("connecting to the session bus")?;
    let ids: Vec<String> = conn
        .call_method(
            Some(SERVICE),
            PATH,
            Some(INTERFACE),
            "GetInitialResultSet",
            &(vec![query.to_owned()],),
        )
        .context("vinoSearch isn't running or isn't installed")?
        .body()
        .deserialize()?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let metas: Vec<HashMap<String, OwnedValue>> = conn
        .call_method(
            Some(SERVICE),
            PATH,
            Some(INTERFACE),
            "GetResultMetas",
            &(ids,),
        )?
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
}

/// The indexed text files and documents changed most recently, for a window
/// that has nothing searched yet.
pub fn recent(limit: u32) -> Result<Vec<Hit>> {
    let conn = zbus::blocking::Connection::session().context("connecting to the session bus")?;
    let files: Vec<(String, String, i64)> = conn
        .call_method(
            Some(SERVICE),
            CONTROL_PATH,
            Some(CONTROL),
            "RecentFiles",
            &(limit,),
        )
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
                    Kind::Text
                },
                path,
            }
        })
        .collect())
}

/// A document's text, extracted by vinoSearch as it does for indexing.
pub fn file_text(path: &Path) -> Result<String> {
    let conn = zbus::blocking::Connection::session().context("connecting to the session bus")?;
    let text: String = conn
        .call_method(
            Some(SERVICE),
            CONTROL_PATH,
            Some(CONTROL),
            "FileText",
            &(path.to_string_lossy().as_ref(),),
        )
        .context("vinoSearch couldn't extract the text")?
        .body()
        .deserialize()?;
    Ok(text)
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
