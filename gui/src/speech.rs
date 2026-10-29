//! The vinospeak CLI: run for a file with `--print-text` for the preview,
//! and kept running as `vinospeak --serve` to speak it. One reading at a time.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

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

/// `vinospeak --serve`: one process for the window's lifetime that keeps
/// the model loaded, so only the first reading pays for loading and
/// compiling it (vinoSpeak#39).
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
            .context("running vinospeak (uv tool install vinospeak)")?;
        let (stdout, mut stderr) = (child.stdout.take(), child.stderr.take());
        let (tx, events) = mpsc::channel();
        // Read whole, so a chatty stderr can't fill its pipe and stall the worker.
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(pipe) = &mut stderr {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        });
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

    /// Read `text` aloud, stopping whatever was playing; its id for the events.
    pub fn speak(
        &mut self,
        text: &str,
        voice: &Voice,
        speed: f32,
        output: Option<&str>,
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
        }))?;
        Ok(self.next_id)
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

/// A sound output, as PipeWire (through pipewire-pulse) or PulseAudio
/// names it.
#[derive(Clone, PartialEq)]
pub struct Output {
    /// The sink name vinospeak's `--audio-device` takes.
    pub name: String,
    pub description: String,
}

/// The desktop's sound outputs, and the default one's name. Empty when
/// `pactl` isn't there, which leaves only the default.
pub fn outputs() -> (Vec<Output>, Option<String>) {
    let run = |args: &[&str]| {
        Command::new("pactl")
            .args(args)
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let sinks = run(&["-f", "json", "list", "sinks"])
        .map(|json| parse_sinks(&json))
        .unwrap_or_default();
    let default = run(&["get-default-sink"]).map(|name| name.trim().to_owned());
    (sinks, default)
}

fn parse_sinks(json: &str) -> Vec<Output> {
    let Ok(serde_json::Value::Array(sinks)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    sinks
        .iter()
        .filter_map(|sink| {
            let name = sink.get("name")?.as_str()?.to_owned();
            let description = sink
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or(&name)
                .to_owned();
            Some(Output { name, description })
        })
        .collect()
}

/// A Kokoro voice. Its id says the rest: `af_heart` is an American English
/// (a) female (f) voice called Heart.
#[derive(Clone, PartialEq)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub accent: &'static str,
    pub gender: &'static str,
    /// What vinospeak's `--language` takes for it.
    pub language: &'static str,
}

/// Kokoro's language letters, in the order the picker groups them. Japanese
/// (j) and Mandarin (z) are left out: vinospeak has no language for them.
const ACCENTS: &[(char, &str, &str)] = &[
    ('a', "American English", "en-us"),
    ('b', "British English", "en-gb"),
    ('e', "Spanish", "es"),
    ('f', "French", "fr-fr"),
    ('h', "Hindi", "hi"),
    ('i', "Italian", "it"),
    ('p', "Brazilian Portuguese", "pt-br"),
];

pub fn voice(id: &str) -> Option<Voice> {
    let mut letters = id.chars();
    let (accent, gender) = (letters.next()?, letters.next()?);
    let name = id.get(3..).filter(|_| id.get(2..3) == Some("_"))?;
    let &(_, accent, language) = ACCENTS.iter().find(|(c, _, _)| *c == accent)?;
    let gender = match gender {
        'f' => "female",
        'm' => "male",
        _ => return None,
    };
    let name = name
        .split('_')
        .map(|word| {
            let mut c = word.chars();
            c.next()
                .map_or_else(String::new, |first| first.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(Voice {
        id: id.to_owned(),
        name,
        accent,
        gender,
        language,
    })
}

/// Kokoro's voices that vinospeak can speak, from the model it downloaded,
/// grouped by accent, women then men, by name.
pub fn voices() -> Vec<Voice> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache")));
    let mut voices: Vec<Voice> = cache
        .and_then(|c| std::fs::read_dir(c.join("vinospeak/kokoro-82m-int8-ov/voices")).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let file = e.ok()?.file_name().into_string().ok()?;
            voice(file.strip_suffix(".bin")?)
        })
        .collect();
    let rank = |v: &Voice| ACCENTS.iter().position(|a| a.1 == v.accent);
    voices.sort_by(|a, b| (rank(a), a.gender, &a.name).cmp(&(rank(b), b.gender, &b.name)));
    if voices.is_empty() {
        voices.extend(voice("af_heart"));
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
    fn voice_ids_become_names() {
        let v = voice("af_heart").unwrap();
        assert_eq!(
            (v.name.as_str(), v.accent, v.gender, v.language),
            ("Heart", "American English", "female", "en-us")
        );
        let v = voice("bm_george").unwrap();
        assert_eq!(
            (v.accent, v.gender, v.language),
            ("British English", "male", "en-gb")
        );
        assert_eq!(voice("pm_alex").unwrap().language, "pt-br");
        assert_eq!(voice("af_two_words").unwrap().name, "Two Words");
        assert!(voice("jf_alpha").is_none(), "no Japanese in vinospeak");
        assert!(voice("zm_yunxi").is_none(), "no Mandarin in vinospeak");
        assert!(voice("ax_odd").is_none());
        assert!(voice("af").is_none());
    }

    #[test]
    fn sinks_come_from_pactl_json() {
        let json =
            r#"[{"index":1,"name":"hdmi_sink","description":"HDMI 1"},{"name":"bare"},{"x":1}]"#;
        let names: Vec<_> = parse_sinks(json)
            .into_iter()
            .map(|o| (o.name, o.description))
            .collect();
        assert_eq!(
            names,
            [
                ("hdmi_sink".to_owned(), "HDMI 1".to_owned()),
                ("bare".to_owned(), "bare".to_owned())
            ]
        );
        assert!(parse_sinks("not json").is_empty());
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
