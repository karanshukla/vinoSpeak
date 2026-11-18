//! The vinospeak CLI: run for a file with `--print-text` for the preview,
//! and kept running as `vinospeak --serve` to speak it. One reading at a time.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::voices::Voice;

/// What a failure to start vinospeak is put down to.
pub const NOT_RUNNABLE: &str = "running vinospeak (uv tool install vinospeak)";

/// How long the worker has to stop a reading, or to exit, before it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(2);

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
/// its suffix), or a document's text from vinoSearch, piped in and read as
/// one heading, paragraph or table cell a line.
#[derive(Clone)]
pub enum Source {
    File(PathBuf),
    Text(String),
    /// A vinoWhisper session, which vinospeak opens itself.
    Session(PathBuf, Part),
}

/// Which part of a vinoWhisper session to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Summary,
    Transcript,
}

impl Part {
    fn arg(self) -> &'static str {
        match self {
            Part::Summary => "summary",
            Part::Transcript => "transcript",
        }
    }
}

fn command(vinospeak: &Path, source: &Source) -> Command {
    let mut cmd = Command::new(vinospeak);
    match source {
        Source::File(path) => cmd.arg("--file").arg(path),
        Source::Text(_) => cmd.args(["--file", "-", "--kind", "document"]),
        Source::Session(path, part) => cmd.arg("--file").arg(path).args(["--part", part.arg()]),
    };
    cmd.stdin(match source {
        Source::Text(_) => Stdio::piped(),
        _ => Stdio::null(),
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
pub fn preview(vinospeak: &Path, source: &Source) -> Result<String> {
    let mut child = command(vinospeak, source)
        .arg("--print-text")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(NOT_RUNNABLE)?;
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

/// The last thing vinospeak said on stderr, without argparse's prefix.
pub fn last_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("vinospeak failed");
    line.trim_start_matches("vinospeak: error: ").to_owned()
}

/// What the worker reports, by the id of the reading it is about.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// The model is loaded and warmed up.
    Ready,
    /// The first samples are with the player, `ms` after the worker read the request.
    FirstAudio {
        id: u64,
        ms: u64,
    },
    Done {
        id: u64,
    },
    /// `id` is None for an error that is about no reading (a load that failed).
    Error {
        id: Option<u64>,
        message: String,
    },
    /// The worker is gone; its last words on stderr.
    Exited(String),
}

fn event(line: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let id = v.get("id").and_then(serde_json::Value::as_u64);
    Some(match v.get("event")?.as_str()? {
        "ready" => Event::Ready,
        "first-audio" => Event::FirstAudio {
            id: id?,
            ms: v.get("ms").and_then(serde_json::Value::as_u64).unwrap_or(0),
        },
        "done" => Event::Done { id: id? },
        "error" => Event::Error {
            id,
            message: v
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("vinospeak failed")
                .to_owned(),
        },
        _ => return None,
    })
}

/// All of stderr, read as it comes so a chatty worker can't fill the pipe
/// and stall.
fn drain(mut stderr: Option<ChildStderr>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(pipe) = &mut stderr {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

/// Send each event the worker prints, then `Exited` with its last words on
/// stderr once stdout closes; `wake` is called after each.
fn forward_events(
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    tx: mpsc::Sender<Event>,
    wake: impl Fn() + Send + 'static,
) {
    let stderr = drain(stderr);
    std::thread::spawn(move || {
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(event) = event(&line) {
                    let _ = tx.send(event);
                    wake();
                }
            }
        }
        let stderr = stderr.join().unwrap_or_default();
        let _ = tx.send(Event::Exited(last_line(&stderr)));
        wake();
    });
}

/// `vinospeak --serve`: one process for the window's lifetime that keeps
/// the model loaded, so only the first reading pays for loading and
/// compiling it.
pub struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    events: mpsc::Receiver<Event>,
    next_id: u64,
}

impl Worker {
    /// Start it loading; `wake` is called for each event, to repaint.
    /// `VINOSPEAK_GUI_DEVICE` picks the OpenVINO device (NPU otherwise).
    pub fn start(vinospeak: &Path, wake: impl Fn() + Send + 'static) -> Result<Self> {
        let mut cmd = Command::new(vinospeak);
        cmd.arg("--serve");
        if let Some(device) = std::env::var_os("VINOSPEAK_GUI_DEVICE").filter(|d| !d.is_empty()) {
            cmd.arg("--device").arg(device);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context(NOT_RUNNABLE)?;
        let (tx, events) = mpsc::channel();
        forward_events(child.stdout.take(), child.stderr.take(), tx, wake);
        Ok(Self {
            stdin: child.stdin.take(),
            child,
            events,
            next_id: 0,
        })
    }

    fn send(&mut self, request: &serde_json::Value) -> Result<()> {
        let stdin = self.stdin.as_mut().context("vinospeak has stopped")?;
        writeln!(stdin, "{request}")
            .and_then(|()| stdin.flush())
            .context("vinospeak has stopped")
    }

    /// Read `text` aloud at `volume` (0 to 1), stopping whatever was
    /// playing; its id for the events.
    pub fn speak(
        &mut self,
        text: &str,
        voice: &Voice,
        speed: f32,
        output: Option<&str>,
        volume: f32,
    ) -> Result<u64> {
        self.next_id += 1;
        self.send(&serde_json::json!({
            "op": "speak",
            "id": self.next_id,
            "text": text,
            "voice": voice.id,
            "language": voice.language,
            "speed": speed,
            "audio_device": output,
            "volume": volume,
        }))?;
        Ok(self.next_id)
    }

    /// The reading under way, if any, plays on at `volume` (0 to 1).
    pub fn set_volume(&mut self, volume: f32) -> Result<()> {
        self.send(&serde_json::json!({"op": "volume", "volume": volume}))
    }

    pub fn stop(&mut self) -> Result<()> {
        self.send(&serde_json::json!({"op": "stop"}))
    }

    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Worker {
    /// Closing stdin is the worker's cue to stop playing and exit.
    fn drop(&mut self) {
        self.stdin = None;
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

/// A reading the worker was asked for.
pub struct Reading {
    pub id: u64,
    pub file: PathBuf,
    pub started: Instant,
    pub stop_sent: Option<Instant>,
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
    fn worker_events_are_read_from_json_lines() {
        assert_eq!(
            event(r#"{"event": "ready", "load_ms": 1}"#),
            Some(Event::Ready)
        );
        assert_eq!(
            event(r#"{"event": "first-audio", "id": 2, "ms": 812}"#),
            Some(Event::FirstAudio { id: 2, ms: 812 })
        );
        assert_eq!(
            event(r#"{"event": "done", "id": 2, "stopped": true}"#),
            Some(Event::Done { id: 2 })
        );
        assert_eq!(
            event(r#"{"event": "error", "id": null, "message": "no NPU"}"#),
            Some(Event::Error {
                id: None,
                message: "no NPU".to_owned()
            })
        );
        assert_eq!(event(r#"{"event": "started", "id": 2}"#), None);
        assert_eq!(event("Traceback (most recent call last):"), None);
    }

    #[test]
    fn the_error_is_the_last_line_without_the_prefix() {
        let stderr = "usage: vinospeak ...\nvinospeak: error: /a.bin is not a UTF-8 text file.\n";
        assert_eq!(last_line(stderr), "/a.bin is not a UTF-8 text file.");
    }
}
