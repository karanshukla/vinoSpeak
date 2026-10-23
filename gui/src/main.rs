//! vinospeak-gui: find a file with vinoSearch and listen to it with vinoSpeak.
//! A proof of concept; the vinospeak CLI does the speaking.

mod app;
mod logo;
mod search;
mod speech;
mod theme;

use std::process::ExitCode;

use eframe::egui;

const APP_ID: &str = "dev.vinospeak.Listen";

/// 960×640, or `VINOSPEAK_GUI_SIZE=WxH` for checking a layout at another size.
fn size() -> [f32; 2] {
    std::env::var("VINOSPEAK_GUI_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.parse().ok()?, h.parse().ok()?])
        })
        .unwrap_or([960.0, 640.0])
}

fn main() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("vinoSpeak")
            .with_app_id(APP_ID)
            .with_inner_size(size())
            .with_min_inner_size([560.0, 360.0]),
        // As in vinoSearch's settings: with vsync, KWin can stall a hidden window.
        vsync: false,
        ..Default::default()
    };
    let result = eframe::run_native(
        "vinoSpeak",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            theme::visible_scroll_handles(&cc.egui_ctx);
            Ok(Box::new(app::App::new()))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("vinospeak-gui: {err}");
            ExitCode::FAILURE
        }
    }
}
