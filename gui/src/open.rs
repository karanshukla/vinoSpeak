//! The desktop's own file dialogs, through xdg-desktop-portal: for a file
//! that isn't listed, and for where to save speech.

use std::path::{Path, PathBuf};

use anyhow::Result;
use ashpd::desktop::ResponseError;
use ashpd::desktop::file_chooser::{FileFilter, SelectedFiles};

/// What vinospeak reads itself, plus the documents vinoSearch extracts.
const READABLE: [&str; 6] = ["txt", "md", "markdown", "vwsession", "pdf", "epub"];

fn readable_filter() -> FileFilter {
    READABLE
        .iter()
        .flat_map(|ext| [format!("*.{ext}"), format!("*.{}", ext.to_uppercase())])
        .fold(FileFilter::new("Text, sessions, and documents"), |f, g| {
            f.glob(&g)
        })
}

/// Blocks until the dialog closes; None when it was cancelled.
pub fn dialog() -> Result<Option<PathBuf>> {
    let asked = async_io::block_on(async {
        SelectedFiles::open_file()
            .title("Open file")
            .accept_label("Open")
            .modal(true)
            .multiple(false)
            .filter(readable_filter())
            .filter(FileFilter::new("All files").glob("*"))
            .send()
            .await?
            .response()
    });
    match asked {
        Ok(files) => Ok(files.uris().first().and_then(|u| file_uri_path(u.as_str()))),
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Where to save speech as `name` (`notes.mp3`), offering only files of
/// its suffix, called `kind`; None when it was cancelled.
pub fn save_dialog(name: &str, kind: &str) -> Result<Option<PathBuf>> {
    let suffix = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let only = FileFilter::new(kind).glob(&format!("*.{suffix}"));
    let asked = async_io::block_on(async {
        SelectedFiles::save_file()
            .title("Save audio")
            .accept_label("Save")
            .modal(true)
            .current_name(name)
            .filter(only.clone())
            .current_filter(only)
            .send()
            .await?
            .response()
    });
    match asked {
        Ok(files) => Ok(files.uris().first().and_then(|u| file_uri_path(u.as_str()))),
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// `file:///a%20b` as `/a b`. Anything but a local file URI is None.
fn file_uri_path(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_uri_is_decoded_and_anything_else_is_none() {
        assert_eq!(
            file_uri_path("file:///home/me/take%20two.md"),
            Some(PathBuf::from("/home/me/take two.md"))
        );
        assert_eq!(
            file_uri_path("file://localhost/a"),
            Some(PathBuf::from("/a"))
        );
        assert_eq!(file_uri_path("https://example.com/a"), None);
        assert_eq!(file_uri_path("file:///bad%2"), None);
    }
}
