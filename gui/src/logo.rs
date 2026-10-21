//! `docs/assets/vinospeak.svg`, painted at `size` points square, so it stays
//! sharp at any scale without an SVG renderer (as vinoSearch's settings do).

use eframe::egui::{self, Color32, Sense, Shape, Stroke, epaint::QuadraticBezierShape};

pub fn logo(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    // The SVG's viewBox is 64 units, and the tile fills 4 to 60 of it: draw
    // those 56 units edge to edge.
    let k = size / 56.0;
    let at = |x: f32, y: f32| rect.min + egui::vec2((x - 4.0) * k, (y - 4.0) * k);
    let box_ =
        |x: f32, y: f32, w: f32, h: f32| egui::Rect::from_min_max(at(x, y), at(x + w, y + h));
    let white = Color32::from_rgb(0xfa, 0xfa, 0xfa);
    let green = Color32::from_rgb(0x56, 0xcc, 0x78);
    let painter = ui.painter();
    painter.rect_filled(
        box_(4.0, 4.0, 56.0, 56.0),
        14.0 * k,
        Color32::from_rgb(0x0e, 0x0e, 0x10),
    );
    painter.rect_stroke(
        box_(5.0, 5.0, 54.0, 54.0),
        13.0 * k,
        Stroke::new(2.0 * k, Color32::from_white_alpha(46)),
        egui::StrokeKind::Middle,
    );
    // The speaker; its two small corner arcs are left square at this size.
    let speaker = [
        (14.0, 27.0),
        (16.0, 25.0),
        (21.0, 25.0),
        (29.0, 18.0),
        (29.0, 46.0),
        (21.0, 39.0),
        (16.0, 39.0),
        (14.0, 37.0),
    ];
    painter.add(Shape::convex_polygon(
        speaker.iter().map(|&(x, y)| at(x, y)).collect(),
        white,
        Stroke::NONE,
    ));
    for [start, control, end] in [
        [(36.0, 26.0), (40.0, 32.0), (36.0, 38.0)],
        [(42.0, 20.0), (51.0, 32.0), (42.0, 44.0)],
    ] {
        let points = [start, control, end].map(|(x, y)| at(x, y));
        painter.add(QuadraticBezierShape::from_points_stroke(
            points,
            false,
            Color32::TRANSPARENT,
            Stroke::new(4.0 * k, green),
        ));
        // Round caps: egui strokes end square.
        painter.circle_filled(points[0], 2.0 * k, green);
        painter.circle_filled(points[2], 2.0 * k, green);
    }
}
