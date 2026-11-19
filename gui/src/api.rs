//! `vinospeak --http`: OpenAI's speech API on this computer, run while the
//! window is open and stopped with it. It loads its own copy of the model.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

use anyhow::{Context, Result};

/// vinospeak's own default, `api_server.DEFAULT_PORT`.
pub const PORT: u16 = 8880;

/// What the server says on stderr.
#[derive(Debug, PartialEq)]
pub enum ApiEvent {
    /// It is listening, at this base URL.
    Serving(String),
    /// It is gone; its last words.
    Exited(String),
}

/// The base URL in the line vinospeak prints once it listens (pinned by
/// `api::tests::the_url_is_read_from_the_line_it_prints`).
fn serving_url(line: &str) -> Option<String> {
    if !line.contains("is serving") {
        return None;
    }
    let start = line.find("http://")?;
    let url = line[start..].split_whitespace().next()?;
    Some(url.to_owned())
}

pub struct ApiServer {
    child: Child,
    events: mpsc::Receiver<ApiEvent>,
}

impl ApiServer {
    /// Start it loading; `wake` is called for each event, to repaint.
    /// `VINOSPEAK_GUI_DEVICE` picks the device, as for the worker.
    pub fn start(vinospeak: &Path, wake: impl Fn() + Send + 'static) -> Result<Self> {
        let mut cmd = Command::new(vinospeak);
        cmd.arg("--http").arg(PORT.to_string());
        if let Some(device) = std::env::var_os("VINOSPEAK_GUI_DEVICE").filter(|d| !d.is_empty()) {
            cmd.arg("--device").arg(device);
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context(crate::speech::NOT_RUNNABLE)?;
        let (tx, events) = mpsc::channel();
        let stderr = child.stderr.take();
        std::thread::spawn(move || {
            let mut last = String::new();
            if let Some(stderr) = stderr {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Some(url) = serving_url(&line) {
                        let _ = tx.send(ApiEvent::Serving(url));
                        wake();
                    }
                    if !line.trim().is_empty() {
                        last = line;
                    }
                }
            }
            let _ = tx.send(ApiEvent::Exited(crate::speech::last_line(&last)));
            wake();
        });
        Ok(Self { child, events })
    }

    pub fn events(&self) -> Vec<ApiEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_is_read_from_the_line_it_prints() {
        let line = "vinoSpeak is serving OpenAI's speech API at http://127.0.0.1:8880/v1 \
                    (model loaded in 4.2 s). Ctrl+C stops it.";
        assert_eq!(
            serving_url(line),
            Some("http://127.0.0.1:8880/v1".to_owned())
        );
        assert_eq!(serving_url("Downloading http://huggingface.co/x"), None);
    }
}
