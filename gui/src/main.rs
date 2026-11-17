//! vinospeak-gui: find a file with vinoSearch and listen to it with vinoSpeak.
//! A proof of concept; the vinospeak CLI does the speaking.

mod app;
mod model_cache;
mod outputs;
mod search;
mod speech;
mod version;
mod voices;

use std::process::ExitCode;

use eframe::egui;

const APP_ID: &str = "dev.vinospeak.Listen";

const DEFAULT_SIZE: [f32; 2] = [1040.0, 680.0];
const MIN_SIZE: [f32; 2] = [560.0, 360.0];

/// `WxH`, as `VINOSPEAK_GUI_SIZE` gives a size.
fn parse_size(size: &str) -> Option<[f32; 2]> {
    let (width, height) = size.split_once('x')?;
    Some([width.parse().ok()?, height.parse().ok()?])
}

/// `VINOSPEAK_GUI_SIZE=WxH` is for checking a layout at another size.
fn window_size() -> [f32; 2] {
    std::env::var("VINOSPEAK_GUI_SIZE")
        .ok()
        .and_then(|size| parse_size(&size))
        .unwrap_or(DEFAULT_SIZE)
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("vinospeak-gui {}", version::VERSION);
        return ExitCode::SUCCESS;
    }
    let app_title_bar = vino_ui::window::app_title_bar();
    let options = eframe::NativeOptions {
        viewport: vino_ui::window::viewport(
            egui::ViewportBuilder::default()
                .with_title("vinoSpeak")
                .with_app_id(APP_ID)
                .with_inner_size(window_size())
                .with_min_inner_size(MIN_SIZE),
            app_title_bar,
        ),
        // With vsync, KWin can stall a hidden window.
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
            let mut app = app::App::new();
            app.app_title_bar = app_title_bar;
            Ok(Box::new(app))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_width_x_height() {
        assert_eq!(parse_size("800x600"), Some([800.0, 600.0]));
        assert_eq!(parse_size("800"), None);
        assert_eq!(parse_size("wide x tall"), None);
    }
}
