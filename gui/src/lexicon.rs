//! Your pronunciations, as vinospeak reads them from `lexicon.toml` beside
//! the window's settings: a `[words]` table of word = "respelling".

use std::path::{Path, PathBuf};

/// A word and how to say it.
pub type Entry = (String, String);

pub fn path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("lexicon.toml"))
}

/// The entries at `path`, none when it's missing; why it can't be used when
/// it isn't a `[words]` table of strings, as vinospeak would refuse it.
pub fn load(path: &Path) -> Result<Vec<Entry>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let Some(words) = table.get("words") else {
        return Ok(Vec::new());
    };
    let words = words
        .as_table()
        .ok_or("[words] must be a table of word = \"respelling\".")?;
    words
        .iter()
        .map(|(word, said)| match said.as_str() {
            Some(said) => Ok((word.clone(), said.to_owned())),
            None => Err(format!("{word} must have a respelling in quotes.")),
        })
        .collect()
}

/// `text` as a TOML basic string.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The entries as lexicon.toml, a word left blank dropped (pinned by
/// `lexicon::tests::what_is_written_reads_back_the_same`).
pub fn to_toml(entries: &[Entry]) -> String {
    let mut out =
        String::from("# Kept by vinoSpeak's window: Settings, Pronunciations.\n[words]\n");
    for (word, said) in entries {
        let word = word.trim();
        if !word.is_empty() {
            out.push_str(&format!("{} = {}\n", quoted(word), quoted(said.trim())));
        }
    }
    out
}

/// Written whole beside the old file then renamed over it.
pub fn save(path: &Path, entries: &[Entry]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("toml.partial");
    std::fs::write(&partial, to_toml(entries))?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vinospeak-lexicon-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("lexicon.toml")
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let path = scratch("round-trip");
        let entries = vec![
            ("Dr. Who".to_owned(), "Doctor Who".to_owned()),
            ("kubectl".to_owned(), "cube control".to_owned()),
            ("say \"hi\"\\".to_owned(), "tab\there".to_owned()),
            ("  ".to_owned(), "dropped".to_owned()),
        ];
        save(&path, &entries).unwrap();
        assert_eq!(load(&path).unwrap(), entries[..3]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_missing_file_is_no_words_and_one_vinospeak_would_refuse_is_an_error() {
        let path = scratch("broken");
        assert_eq!(load(&path).unwrap(), Vec::<Entry>::new());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[words]\nkubectl = 3\n").unwrap();
        assert!(load(&path).unwrap_err().contains("kubectl"));
        std::fs::write(&path, "not = [toml").unwrap();
        assert!(load(&path).is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
