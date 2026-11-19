//! The player under a file's name, in two rows that never reflow: the
//! transport (the round Read aloud / Stop button, what it is doing, the
//! volume) and the settings for the next reading (translate, voice, speed,
//! output), each under its heading, in four equal columns or two by two.

use eframe::egui::{self, Color32, RichText, Sense, Stroke};
use vino_ui::a11y;
use vino_ui::icons::Icon;
use vino_ui::theme;
use vino_ui::widgets;

use super::App;
use crate::outputs::Output;
use crate::search::Hit;

/// The speed presets, as the player offers them.
pub(super) const SPEEDS: [(f32, &str); 4] =
    [(0.8, "0.8×"), (1.0, "1.0×"), (1.25, "1.25×"), (1.5, "1.5×")];
const BUTTON_DIAMETER: f32 = 44.0;
/// Taller than the button, so a state's two lines and spinner never stretch the row.
const TRANSPORT_HEIGHT: f32 = 52.0;
/// Below this a setting's column is too narrow for its picker, and the four
/// go two by two (pinned by `app::tests::the_settings_go_two_by_two_when_narrow`).
const MIN_SETTING_WIDTH: f32 = 160.0;
const SETTING_GAP: f32 = 12.0;

/// A setting's heading and the picker drawn under it.
type Setting = (&'static str, fn(&mut App, &mut egui::Ui));
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

/// A setting's small heading above its picker.
fn setting(ui: &mut egui::Ui, heading: &str, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette(ui.ctx());
    ui.spacing_mut().item_spacing.y = 6.0;
    let shown = ui.label(
        RichText::new(heading)
            .size(11.0)
            .family(theme::semibold())
            .color(p.muted),
    );
    a11y::decorative(ui, shown.id);
    add(ui);
}

/// How many settings go side by side in a row this wide.
pub(super) fn settings_per_row(width: f32) -> usize {
    if width >= 4.0 * MIN_SETTING_WIDTH + 3.0 * SETTING_GAP {
        4
    } else {
        2
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

/// What the player says beside Stop: until the first audio, that it is
/// starting or translating, since loading and translating take seconds
/// (pinned by `app::tests::the_player_says_it_is_starting_until_the_first_audio`).
pub(super) fn reading_state(reading: &crate::speech::Reading) -> &'static str {
    match (reading.heard, reading.translated_into) {
        (true, _) => "Speaking",
        (false, Some(_)) => "Translating…",
        (false, None) => "Starting…",
    }
}

impl App {
    pub(super) fn player(&mut self, ui: &mut egui::Ui, hit: &Hit) {
        let speaking_this = self.reading.as_ref().is_some_and(|r| r.file == hit.path);
        self.transport(ui, speaking_this);
        ui.add_space(14.0);
        self.settings(ui);
    }

    /// The button and what it is doing on the left, the volume on the right,
    /// in a row whose height and places don't change with the state (pinned by
    /// `app::tests::the_choices_stay_put_when_a_reading_starts`).
    fn transport(&mut self, ui: &mut egui::Ui, speaking_this: bool) {
        let width = ui.available_width();
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(egui::vec2(width, TRANSPORT_HEIGHT), layout, |ui| {
            ui.set_min_size(egui::vec2(width, TRANSPORT_HEIGHT));
            ui.spacing_mut().item_spacing.x = 12.0;
            if speaking_this {
                self.stop_button(ui);
            } else {
                self.read_aloud_button(ui);
            }
            let right = egui::Layout::right_to_left(egui::Align::Center);
            ui.with_layout(right, |ui| {
                let size = egui::vec2(super::volume::VOLUME_WIDTH, BUTTON_DIAMETER);
                ui.allocate_ui_with_layout(size, layout, |ui| self.volume_controls(ui));
            });
        });
    }

    /// Translate, voice, speed and output for the next Read aloud.
    fn settings(&mut self, ui: &mut egui::Ui) {
        let per_row = settings_per_row(ui.available_width());
        let mut cells: Vec<Setting> = vec![
            ("TRANSLATE", Self::translate_picker),
            ("VOICE", Self::voice_picker),
            ("SPEED", Self::speed_picker),
            ("OUTPUT", Self::output_picker),
        ];
        while !cells.is_empty() {
            let row: Vec<_> = cells.drain(..per_row.min(cells.len())).collect();
            ui.spacing_mut().item_spacing.x = SETTING_GAP;
            ui.columns(per_row, |columns| {
                for (column, (heading, picker)) in columns.iter_mut().zip(row) {
                    setting(column, heading, |ui| picker(self, ui));
                }
            });
            ui.add_space(8.0);
        }
    }

    fn speed_picker(&mut self, ui: &mut egui::Ui) {
        let label = SPEEDS
            .iter()
            .find(|(speed, _)| *speed == self.speed)
            .map_or_else(
                || format!("{}×", self.speed),
                |(_, name)| (*name).to_owned(),
            );
        let shown = widgets::combo(ui, "speed", label, f32::INFINITY).show_ui(ui, |ui| {
            for (speed, name) in SPEEDS {
                ui.selectable_value(&mut self.speed, speed, name);
            }
        });
        ui.ctx()
            .accesskit_node_builder(shown.response.id, |node| node.set_label("Speed"));
    }

    fn stop_button(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.ctx());
        if round_button(ui, Icon::Stop, "Stop", p.text, p.bg, true).clicked() {
            self.stop_reading();
        }
        let Some(reading) = self.reading.as_ref() else {
            return;
        };
        let secs = reading.started.elapsed().as_secs();
        let state = reading_state(reading);
        let mut under = elapsed(secs);
        if let Some(target) = reading.translated_into {
            under = format!("{under} · into {}", target.name);
        }
        let heard = reading.heard;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if !heard {
                    ui.add(egui::Spinner::new().size(13.0).color(p.muted));
                }
                let shown = ui.label(RichText::new(state).family(theme::semibold()));
                a11y::live(&shown);
            });
            let clock = ui.label(RichText::new(under).size(12.5).color(p.muted));
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

    /// Voices by name, under a heading per accent; with a Translate language,
    /// only the voices that speak it (pinned by
    /// `app::tests::translating_offers_only_voices_of_that_language`).
    fn voice_picker(&mut self, ui: &mut egui::Ui) {
        let language = self.translate_to.map(|t| t.language);
        let p = theme::palette(ui.ctx());
        let chosen = self.voice.clone();
        {
            let label = format!("{} · {}", chosen.name, chosen.gender);
            let shown = widgets::combo(ui, "voice", label, f32::INFINITY).show_ui(ui, |ui| {
                ui.set_min_width(220.0);
                let mut accent = "";
                let speaks = |v: &&crate::voices::Voice| language.is_none_or(|l| v.language == l);
                for voice in self.voices.iter().filter(speaks) {
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
        }
    }

    /// Where speech plays. A change applies from the next Read aloud.
    fn output_picker(&mut self, ui: &mut egui::Ui) {
        let default = default_output_label(&self.outputs, self.default_output.as_deref());
        let selected = match &self.output {
            Some(name) => described(&self.outputs, name),
            None => default.clone(),
        };
        {
            let shown = widgets::combo(ui, "output", selected, f32::INFINITY).show_ui(ui, |ui| {
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
        }
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
