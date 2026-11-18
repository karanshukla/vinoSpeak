//! The player's volume: a mute button and a slider, both heard by the
//! reading under way.

use eframe::egui::{self, Pos2, Rect, Sense, Shape, Stroke};
use vino_ui::theme;

use super::App;

const SLIDER_WIDTH: f32 = 110.0;
const SLIDER_HEIGHT: f32 = 24.0;
const TRACK_HEIGHT: f32 = 4.0;
const KNOB_RADIUS: f32 = 7.0;
const MUTE_SIZE: f32 = 28.0;
/// What one arrow key moves the slider by.
const STEP: f32 = 0.05;
/// The speaker is drawn on the icons' 24-unit grid, inset this far in the button.
const GLYPH_INSET: f32 = 5.0;
const GLYPH_CENTRE: f32 = 12.0;
const GLYPH_STROKE: f32 = 1.8;
/// The second sound wave shows from this volume up.
const LOUD: f32 = 0.5;

/// The volume at `x` along a slider spanning `track`, 0 at its left end.
fn volume_at(x: f32, track: Rect) -> f32 {
    ((x - track.left()) / track.width()).clamp(0.0, 1.0)
}

/// `volume` moved by `steps` arrow presses, kept on the steps and in 0 to 1.
fn stepped(volume: f32, steps: i32) -> f32 {
    let at = (volume / STEP).round() as i32 + steps;
    (at as f32 * STEP).clamp(0.0, 1.0)
}

fn percent(volume: f32) -> String {
    format!("{}%", (volume * 100.0).round())
}

/// The arrow keys pressed on a focused slider: + for Right and Up.
fn arrow_steps(ui: &egui::Ui) -> i32 {
    ui.input(|i| {
        let pressed = |keys: [egui::Key; 2]| keys.iter().filter(|k| i.key_pressed(**k)).count();
        pressed([egui::Key::ArrowRight, egui::Key::ArrowUp]) as i32
            - pressed([egui::Key::ArrowLeft, egui::Key::ArrowDown]) as i32
    })
}

/// A speaker with one or two waves, or with a cross when `silent`.
fn paint_speaker(
    painter: &egui::Painter,
    rect: Rect,
    volume: f32,
    silent: bool,
    color: egui::Color32,
) {
    let scale = rect.width().min(rect.height()) / (2.0 * GLYPH_CENTRE);
    let at =
        |x: f32, y: f32| rect.center() + egui::vec2(x - GLYPH_CENTRE, y - GLYPH_CENTRE) * scale;
    let stroke = Stroke::new(GLYPH_STROKE * scale, color);
    let body = [
        (11.0, 5.0),
        (6.0, 9.0),
        (3.0, 9.0),
        (3.0, 15.0),
        (6.0, 15.0),
        (11.0, 19.0),
    ];
    painter.add(Shape::closed_line(
        body.iter().map(|&(x, y)| at(x, y)).collect(),
        stroke,
    ));
    if silent {
        painter.line_segment([at(16.0, 9.5), at(21.0, 14.5)], stroke);
        painter.line_segment([at(21.0, 9.5), at(16.0, 14.5)], stroke);
        return;
    }
    let wave = |radius: f32| -> Vec<Pos2> {
        (0..=8)
            .map(|i| {
                let angle = (-50.0 + 12.5 * i as f32).to_radians();
                at(12.0 + radius * angle.cos(), 12.0 + radius * angle.sin())
            })
            .collect()
    };
    painter.add(Shape::line(wave(4.5), stroke));
    if volume >= LOUD {
        painter.add(Shape::line(wave(8.5), stroke));
    }
}

impl App {
    pub(super) fn volume_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if self.mute_button(ui).clicked() {
                self.muted = !self.muted;
                self.volume_changed();
            }
            if self.volume_slider(ui) {
                self.muted = false;
                self.volume_changed();
            }
        });
    }

    fn mute_button(&self, ui: &mut egui::Ui) -> egui::Response {
        let p = theme::palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(MUTE_SIZE), Sense::click());
        let name = if self.muted { "Unmute" } else { "Mute" };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            if response.hovered() {
                painter.rect_filled(rect, 6.0, p.pressed);
            }
            let silent = self.muted || self.volume == 0.0;
            let color = if self.muted { p.muted } else { p.text };
            paint_speaker(
                painter,
                rect.shrink(GLYPH_INSET),
                self.volume,
                silent,
                color,
            );
            if response.has_focus() {
                painter.rect_stroke(
                    rect.expand(2.0),
                    8.0,
                    Stroke::new(2.0_f32, p.accent),
                    egui::StrokeKind::Outside,
                );
            }
        }
        response.on_hover_text(name)
    }

    /// True when the volume was moved.
    fn volume_slider(&mut self, ui: &mut egui::Ui) -> bool {
        let p = theme::palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(SLIDER_WIDTH, SLIDER_HEIGHT),
            Sense::click_and_drag(),
        );
        let track = Rect::from_center_size(
            rect.center(),
            egui::vec2(rect.width() - 2.0 * KNOB_RADIUS, TRACK_HEIGHT),
        );
        let before = self.volume;
        if let Some(pointer) = response.interact_pointer_pos() {
            self.volume = volume_at(pointer.x, track);
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
            let steps = arrow_steps(ui);
            if steps != 0 {
                self.volume = stepped(self.volume, steps);
            }
        }
        let heard = self.heard_volume();
        response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(heard), "Volume"));
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let fill = if self.muted { p.muted } else { p.primary_fill };
            painter.rect_filled(track, TRACK_HEIGHT / 2.0, p.pressed);
            let knob_x = track.left() + track.width() * self.volume;
            let done = Rect::from_min_max(track.min, egui::pos2(knob_x, track.max.y));
            painter.rect_filled(done, TRACK_HEIGHT / 2.0, fill);
            let knob = egui::pos2(knob_x, rect.center().y);
            painter.circle(knob, KNOB_RADIUS, p.field, Stroke::new(1.0_f32, p.edge));
            if response.has_focus() {
                painter.circle_stroke(knob, KNOB_RADIUS + 3.0, Stroke::new(2.0_f32, p.accent));
            }
        }
        response.on_hover_text(percent(heard));
        self.volume != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_reads_0_at_its_left_end_and_1_at_its_right() {
        let track = Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(110.0, 4.0));
        assert_eq!(volume_at(10.0, track), 0.0);
        assert_eq!(volume_at(60.0, track), 0.5);
        assert_eq!(volume_at(110.0, track), 1.0);
    }

    #[test]
    fn a_drag_past_either_end_stops_at_it() {
        let track = Rect::from_min_max(egui::pos2(10.0, 0.0), egui::pos2(110.0, 4.0));
        assert_eq!(volume_at(-50.0, track), 0.0);
        assert_eq!(volume_at(500.0, track), 1.0);
    }

    #[test]
    fn an_arrow_moves_to_the_next_step_and_no_further_than_the_ends() {
        assert_eq!(percent(stepped(0.5, 1)), "55%");
        assert_eq!(percent(stepped(0.52, -1)), "45%");
        assert_eq!(stepped(1.0, 1), 1.0);
        assert_eq!(stepped(0.0, -1), 0.0);
    }
}
