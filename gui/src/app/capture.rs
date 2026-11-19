//! Checking the window with nobody at the screen: save a screenshot once the
//! first file's text is in, and exit (gui/README.md lists the variables).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use super::App;

/// How long a preview may take before the screenshot is taken without it.
const PREVIEW_WAIT: Duration = Duration::from_secs(30);
/// How long a reading started for the screenshot plays before it is taken.
/// How long a reading runs before the screenshot, unless `VINOSPEAK_GUI_SPEAK` says.
const SPEAKING_WAIT: Duration = Duration::from_secs(4);
const POLL: Duration = Duration::from_millis(100);

/// `VINOSPEAK_GUI_SCREENSHOT=out.png`: where the screenshot goes.
/// `VINOSPEAK_GUI_PICK=n` picks the nth file instead of the first, and
/// `VINOSPEAK_GUI_SPEAK=1` starts reading it first; a number above 1 is
/// how many seconds it reads before the screenshot.
pub(super) struct Capture {
    out: PathBuf,
    pub(super) pick: usize,
    speak: bool,
    speaking_wait: Duration,
    requested: bool,
    deadline: Instant,
}

impl Capture {
    pub(super) fn from_env() -> Option<Self> {
        let out = std::env::var_os("VINOSPEAK_GUI_SCREENSHOT")?;
        Some(Self {
            out: out.into(),
            pick: std::env::var("VINOSPEAK_GUI_PICK")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0),
            speak: std::env::var_os("VINOSPEAK_GUI_SPEAK").is_some(),
            speaking_wait: std::env::var("VINOSPEAK_GUI_SPEAK")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|&s| s > 1)
                .map_or(SPEAKING_WAIT, Duration::from_secs),
            requested: false,
            deadline: Instant::now() + PREVIEW_WAIT,
        })
    }
}

/// Save the next frame to `out`, then close the window.
fn screenshot_then_close(ctx: &egui::Context, out: PathBuf) {
    let done = ctx.clone();
    let save = egui::ScreenshotCallback::new(move |image| {
        let [w, h] = image.size;
        let saved = image::RgbaImage::from_raw(w as u32, h as u32, image.as_raw().to_vec())
            .map(|img| img.save(&out));
        match saved {
            Some(Ok(())) => eprintln!("saved {}", out.display()),
            _ => eprintln!("couldn't save {}", out.display()),
        }
        done.send_viewport_cmd(egui::ViewportCommand::Close);
        done.request_repaint();
    });
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(save));
}

impl App {
    pub(super) fn capture(&mut self, ctx: &egui::Context) {
        let Some(capture) = &mut self.capture else {
            return;
        };
        let ready = self.preview.is_some() || Instant::now() > capture.deadline;
        if ready && !capture.requested {
            if capture.speak && self.reading.is_none() && self.preview.is_some() {
                capture.speak = false;
                capture.deadline = Instant::now() + capture.speaking_wait;
                self.read_aloud(ctx);
                return;
            }
            if self.reading.is_some() && Instant::now() < capture.deadline {
                ctx.request_repaint_after(POLL);
                return;
            }
            capture.requested = true;
            screenshot_then_close(ctx, capture.out.clone());
        }
        ctx.request_repaint_after(POLL);
    }
}
