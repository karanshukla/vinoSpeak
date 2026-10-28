//! vinospeak-gui: find a file with vinoSearch and listen to it with vinoSpeak.
//! A proof of concept; the vinospeak CLI does the speaking.

mod app;
mod search;
mod speech;

use std::process::ExitCode;

use eframe::egui;

const APP_ID: &str = "dev.vinospeak.Listen";

/// 1040×736, or `VINOSPEAK_GUI_SIZE=WxH` for checking a layout at another size.
fn size() -> [f32; 2] {
    std::env::var("VINOSPEAK_GUI_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.parse().ok()?, h.parse().ok()?])
        })
        .unwrap_or([1040.0, 736.0])
}

fn main() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("vinoSpeak")
            .with_app_id(APP_ID)
            .with_inner_size(size())
            .with_min_inner_size([560.0, 360.0]),
        // As in vinoSearch's settings: with vsync, KWin can stall a hidden window.
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let result = eframe::run_native(
        "vinoSpeak",
        options,
        Box::new(|cc| {
            vino_ui::theme::install(&cc.egui_ctx);
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
