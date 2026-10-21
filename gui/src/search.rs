//! vinoSearch over D-Bus: the same search GNOME Shell's overview runs, so
//! any running vinoSearch answers, whichever desktop it was set up for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use zbus::zvariant::OwnedValue;

const SERVICE: &str = "dev.vinosearch.Runner";
const PATH: &str = "/gnome";
const INTERFACE: &str = "org.gnome.Shell.SearchProvider2";
/// vinoSearch's document formats (its src/extract.rs). vinoSpeak reads
/// UTF-8 text only, so these are listed but can't be read aloud yet.
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
}
