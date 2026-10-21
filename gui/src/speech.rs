//! The vinospeak CLI, run for a file: `--print-text` for the preview, then
//! once more to speak it. One reading at a time.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// How long a reading has to wind down after Ctrl+C before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// `$VINOSPEAK`, else `vinospeak` on PATH, else `~/.local/bin/vinospeak`
/// (where `uv tool install` puts it).
pub fn binary() -> PathBuf {
    if let Some(bin) = std::env::var_os("VINOSPEAK").filter(|b| !b.is_empty()) {
        return bin.into();
    }
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join("vinospeak"))
            .find(|bin| bin.is_file())
    });
    let local =
        || std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/bin/vinospeak"));
    on_path.or_else(local).unwrap_or_else(|| "vinospeak".into())
}

/// What vinospeak reads: a file it opens itself (markdown is cleaned up by
/// its suffix), or a document's text from vinoSearch, piped in.
#[derive(Clone)]
pub enum Source {
    File(PathBuf),
    Text(String),
}

fn command(source: &Source) -> Command {
    let mut cmd = Command::new(binary());
    match source {
        Source::File(path) => cmd.arg("--file").arg(path),
        Source::Text(_) => cmd.args(["--file", "-"]),
    };
    cmd.stdin(match source {
        Source::File(_) => Stdio::null(),
        Source::Text(_) => Stdio::piped(),
    });
    cmd
}

/// Feed a piped source from a thread, so a long document can't stall the caller.
fn feed(child: &mut Child, source: &Source) {
    if let (Source::Text(text), Some(mut stdin)) = (source, child.stdin.take()) {
        let text = text.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
}

/// The text vinospeak would speak, with the model never loaded.
pub fn preview(source: &Source) -> Result<String> {
    let mut child = command(source)
        .arg("--print-text")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running vinospeak (uv tool install vinospeak)")?;
    feed(&mut child, source);
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{}", last_line(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The preview as paragraphs, each on one line: vinospeak keeps the file's
/// line breaks, which mean nothing once spoken.
pub fn paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        for word in line.split_whitespace() {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn last_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("vinospeak failed");
    line.trim_start_matches("vinospeak: error: ").to_owned()
}

pub enum State {
    Speaking,
    Stopping,
    Finished,
    Failed(String),
}

pub struct Reading {
    child: Child,
    pub file: PathBuf,
    pub started: Instant,
    stop_sent: Option<Instant>,
}

impl Reading {
    pub fn start(file: &Path, source: &Source, voice: &str, speed: f32) -> Result<Self> {
        let mut child = command(source)
            .args(["--voice", voice, "--speed", &format!("{speed:.1}")])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("running vinospeak (uv tool install vinospeak)")?;
        feed(&mut child, source);
        Ok(Self {
            child,
            file: file.to_owned(),
            started: Instant::now(),
            stop_sent: None,
        })
    }

    /// Ctrl+C, so vinospeak stops its player too; a kill would leave the
    /// player running out the audio it already has.
    pub fn stop(&mut self) {
        if self.stop_sent.is_none() {
            // SAFETY: kill(2) on our own child's pid; it is reaped only in poll.
            unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGINT) };
            self.stop_sent = Some(Instant::now());
        }
    }

    pub fn poll(&mut self) -> State {
        match self.child.try_wait() {
            Ok(Some(status)) if status.success() || self.stop_sent.is_some() => State::Finished,
            Ok(Some(_)) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = self.child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                State::Failed(last_line(&stderr))
            }
            Ok(None) => match self.stop_sent {
                Some(sent) if sent.elapsed() > STOP_GRACE => {
                    let _ = self.child.kill();
                    State::Stopping
                }
                Some(_) => State::Stopping,
                None => State::Speaking,
            },
            Err(e) => State::Failed(e.to_string()),
        }
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            self.stop();
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline {
                if let Ok(Some(_)) = self.child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Kokoro's voices, from the model vinospeak downloaded.
pub fn voices() -> Vec<String> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache")));
    let mut voices: Vec<String> = cache
        .and_then(|c| std::fs::read_dir(c.join("vinospeak/kokoro-82m-int8-ov/voices")).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".bin").map(str::to_owned)
        })
        .collect();
    voices.sort();
    if voices.is_empty() {
        voices.push("af_heart".to_owned());
    }
    voices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_join_their_lines_and_drop_blank_runs() {
        let text = "\n\nTitle.\n\n\nOne line\nand the next.\n\n";
        assert_eq!(paragraphs(text), ["Title.", "One line and the next."]);
    }

    #[test]
    fn the_error_is_the_last_line_without_the_prefix() {
        let stderr = "usage: vinospeak ...\nvinospeak: error: /a.bin is not a UTF-8 text file.\n";
        assert_eq!(last_line(stderr), "/a.bin is not a UTF-8 text file.");
    }
}
