//! The player under a file's name: the round Read aloud / Stop button, what
//! it is doing, and the voice, speed, output and volume choices after it.

use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use vino_ui::a11y;
use vino_ui::icons::{self, Icon};
use vino_ui::theme;
use vino_ui::widgets;

use super::App;
use crate::outputs::Output;
use crate::search::Hit;

/// The speed presets, as the player offers them.
pub(super) const SPEEDS: [(f32, &str); 4] =
    [(0.8, "0.8×"), (1.0, "1.0×"), (1.25, "1.25×"), (1.5, "1.5×")];
const BUTTON_DIAMETER: f32 = 44.0;
/// What each choice takes in the player's row, icon and all: a group that
/// doesn't fit starts the next row, rather than running off the window
/// (pinned by `app::tests::the_player_wraps_rather_than_running_off_the_window`).
const VOICE_ROOM: f32 = 190.0;
const SPEED_ROOM: f32 = 210.0;
const OUTPUT_ROOM: f32 = 220.0;
const VOLUME_ROOM: f32 = 150.0;
/// The play and stop shapes are drawn on the icons' 24-unit grid, 18 px across.
const GLYPH_SCALE: f32 = 18.0 / 24.0;
const GLYPH_CENTRE: f32 = 12.0;

/// Minutes and seconds into a reading: `1:05`.
fn elapsed(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// What the output picker calls a sink: its description, or its name when
/// `pactl` didn't list it.
fn described(outputs: &[Output], name: &str) -> String {
    outputs
        .iter()
        .find(|o| o.name == name)
        .map_or_else(|| name.to_owned(), |o| o.description.clone())
}

fn default_output_label(outputs: &[Output], default: Option<&str>) -> String {
    match default {
        Some(name) => format!("Default ({})", described(outputs, name)),
        None => "Default output".to_owned(),
    }
}

/// In a wrapping row `available_width` is the whole row's, not what is left.
fn room_for(ui: &mut egui::Ui, width: f32) {
    if ui.max_rect().right() - ui.cursor().left() < width {
        ui.end_row();
    }
}

/// A round button with a filled play triangle or stop square, named for a
/// screen reader.
fn round_button(
    ui: &mut egui::Ui,
    icon: Icon,
    name: &str,
    fill: Color32,
    color: Color32,
    enabled: bool,
) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        egui::Vec2::splat(BUTTON_DIAMETER),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    if ui.is_rect_visible(rect) {
        let fade = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
        let fill = if response.hovered() && enabled {
            fill.lerp_to_gamma(p.text, 0.08)
        } else {
            fill
        };
        let painter = ui.painter();
        painter.circle_filled(rect.center(), BUTTON_DIAMETER / 2.0, fade(fill));
        let at = |x: f32, y: f32| {
            rect.center() + egui::vec2(x - GLYPH_CENTRE, y - GLYPH_CENTRE) * GLYPH_SCALE
        };
        let glyph = match icon {
            Icon::Play => egui::Shape::convex_polygon(
                vec![at(9.0, 5.0), at(20.0, 12.0), at(9.0, 19.0)],
                fade(color),
                Stroke::NONE,
            ),
            _ => egui::Shape::rect_filled(
                egui::Rect::from_min_max(at(6.5, 6.5), at(17.5, 17.5)),
                2.0,
                fade(color),
            ),
        };
        painter.add(glyph);
        if response.has_focus() {
            let ring = BUTTON_DIAMETER / 2.0 + 3.0;
            painter.circle_stroke(rect.center(), ring, Stroke::new(2.0_f32, p.accent));
        }
    }
    response.on_hover_text(name)
}

impl App {
    pub(super) fn player(&mut self, ui: &mut egui::Ui, hit: &Hit) {
        let speaking_this = self.reading.as_ref().is_some_and(|r| r.file == hit.path);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 8.0);
            if speaking_this {
                self.stop_button(ui);
            } else {
                self.read_aloud_button(ui);
            }
            ui.add_space(8.0);
            room_for(ui, VOICE_ROOM);
            self.voice_picker(ui);
            room_for(ui, SPEED_ROOM);
            let mut speed = self.speed;
            if widgets::segmented(ui, "Speed", &mut speed, &SPEEDS) {
                self.speed = speed;
            }
            room_for(ui, OUTPUT_ROOM);
            self.output_picker(ui);
            room_for(ui, VOLUME_ROOM);
            self.volume_controls(ui);
        });
    }

    fn stop_button(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        if round_button(ui, Icon::Stop, "Stop", p.text, p.bg, true).clicked() {
            self.stop_reading();
        }
        let secs = self
            .reading
            .as_ref()
            .map_or(0, |r| r.started.elapsed().as_secs());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let shown = ui.label(RichText::new("Speaking").family(theme::semibold()));
            a11y::decorative(ui, shown.id);
            let clock = ui.label(RichText::new(elapsed(secs)).size(12.5).color(p.muted));
            a11y::decorative(ui, clock.id);
        });
    }

    fn read_aloud_button(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let readable = matches!(self.preview, Some(Ok(_)));
        let button = round_button(
            ui,
            Icon::Play,
            "Read aloud",
            p.primary_fill,
            p.primary_text,
            readable,
        );
        if button.clicked() {
            let ctx = ui.ctx().clone();
            self.read_aloud(&ctx);
        }
        let shown = ui.label(RichText::new("Read aloud").family(theme::semibold()));
        a11y::decorative(ui, shown.id);
    }

    /// Voices by name, under a heading per accent.
    fn voice_picker(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let chosen = self.voice.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::icon(ui, Icon::Voice, 14.0, p.muted);
            let label = format!("{} · {}", chosen.name, chosen.gender);
            let shown = widgets::combo(ui, "voice", label, 170.0).show_ui(ui, |ui| {
                ui.set_min_width(220.0);
                let mut accent = "";
                for voice in &self.voices {
                    if voice.accent != accent {
                        accent = voice.accent;
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(accent.to_uppercase())
                                .size(11.0)
                                .family(theme::semibold())
                                .color(p.muted),
                        );
                    }
                    let label = format!("{}  ·  {}", voice.name, voice.gender);
                    ui.selectable_value(&mut self.voice, voice.clone(), label);
                }
            });
            ui.ctx()
                .accesskit_node_builder(shown.response.id, |node| node.set_label("Voice"));
            shown.response.on_hover_text(format!(
                "{}, {} ({})",
                chosen.accent, chosen.gender, chosen.id
            ));
        });
    }

    /// Where speech plays. A change applies from the next Read aloud.
    fn output_picker(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        let default = default_output_label(&self.outputs, self.default_output.as_deref());
        let selected = match &self.output {
            Some(name) => described(&self.outputs, name),
            None => default.clone(),
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            icons::icon(ui, Icon::Speaker, 14.0, p.muted);
            let shown = widgets::combo(ui, "output", selected, 200.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.output, None, default);
                for output in &self.outputs {
                    ui.selectable_value(
                        &mut self.output,
                        Some(output.name.clone()),
                        &output.description,
                    );
                }
            });
            ui.ctx()
                .accesskit_node_builder(shown.response.id, |node| node.set_label("Output"));
            shown
                .response
                .on_hover_text("Where speech plays, from the next Read aloud");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hdmi() -> Vec<Output> {
        vec![Output {
            name: "hdmi_sink".to_owned(),
            description: "HDMI 1".to_owned(),
        }]
    }

    #[test]
    fn the_clock_counts_minutes_and_two_digit_seconds() {
        assert_eq!(elapsed(5), "0:05");
        assert_eq!(elapsed(59), "0:59");
        assert_eq!(elapsed(60), "1:00");
        assert_eq!(elapsed(3725), "62:05");
    }

    #[test]
    fn an_output_is_called_by_its_description_or_else_its_name() {
        assert_eq!(described(&hdmi(), "hdmi_sink"), "HDMI 1");
        assert_eq!(described(&hdmi(), "usb_dac"), "usb_dac");
    }

    #[test]
    fn the_default_output_is_named_when_pactl_says_which_it_is() {
        assert_eq!(
            default_output_label(&hdmi(), Some("hdmi_sink")),
            "Default (HDMI 1)"
        );
        assert_eq!(default_output_label(&hdmi(), None), "Default output");
    }
}
