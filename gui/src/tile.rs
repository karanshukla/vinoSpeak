//! A file's tile in the list, drawn as vino-ui's logos are (an ink tile with a
//! faint edge, a white mark, one accent) so it sits level with the vinoWhisper
//! logo a session shows. The accent says which format it is.

use std::path::Path;

use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, vec2,
};
use vino_ui::{a11y, icons, theme};

use crate::search;

const INK: Color32 = Color32::from_rgb(0x0e, 0x0e, 0x10);
const WHITE: Color32 = Color32::from_rgb(0xfa, 0xfa, 0xfa);
const RED: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
const BLUE: Color32 = Color32::from_rgb(0x6c, 0xa8, 0xff);
const ORANGE: Color32 = Color32::from_rgb(0xf5, 0xa0, 0x4a);
const PURPLE: Color32 = Color32::from_rgb(0xb9, 0x97, 0xff);
const YELLOW: Color32 = Color32::from_rgb(0xf2, 0xd2, 0x4b);
const GREEN: Color32 = Color32::from_rgb(0x56, 0xcc, 0x78);
const GREY: Color32 = Color32::from_rgb(0xa0, 0xa4, 0xac);

// On the logos' grid: a 56-unit tile.
const PAGE: &str = "M34 10H18a4 4 0 0 0-4 4v28a4 4 0 0 0 4 4h20a4 4 0 0 0 4-4V18z M34 10v8h8";
const LINES: &str = "M21 28h14 M21 36h14";
const HASH: &str = "M26 24l-2 15 M34 24l-2 15 M21 29h15 M20 35h15";
const SCREEN: &str = "M14 13h28a3 3 0 0 1 3 3v18a3 3 0 0 1-3 3H14a3 3 0 0 1-3-3V16a3 3 0 0 1 3-3z M28 37v7 M20 44h16";
const BARS: &str = "M21 30v-4 M28 30v-10 M35 30v-7";
const BOOK: &str = "M28 18C24 14 18 13 11 13v28c7 0 13 1 17 5c4-4 10-5 17-5V13c-7 0-13 1-17 5z";
const SPINE: &str = "M28 19v26";
const CAPTION: &str = "M14 14h28a3 3 0 0 1 3 3v22a3 3 0 0 1-3 3H14a3 3 0 0 1-3-3V17a3 3 0 0 1 3-3z";
const CAPTION_LINES: &str = "M19 28h8 M33 28h4 M19 35h18";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Writing,
    Slides,
    Book,
    Subtitles,
    Markdown,
    Plain,
}

impl Format {
    /// By extension: vinoSearch's document formats (search::DOCUMENTS), and
    /// anything else it hands over is read as text.
    pub fn of(path: &Path) -> Self {
        match search::extension(path).as_str() {
            "pdf" => Self::Pdf,
            "docx" | "docm" | "dotx" | "dotm" | "odt" | "ott" | "fodt" | "doc" | "dot" | "rtf" => {
                Self::Writing
            }
            "pptx" | "pptm" | "potx" | "ppsx" | "ppsm" | "odp" | "otp" | "fodp" | "ppt" | "pot"
            | "pps" => Self::Slides,
            "epub" => Self::Book,
            "srt" | "vtt" => Self::Subtitles,
            "md" | "markdown" | "mdx" => Self::Markdown,
            _ => Self::Plain,
        }
    }
}

/// The tile in a `size` square, hidden from screen readers: the file's name is
/// written beside it.
pub fn file(ui: &mut Ui, format: Format, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    a11y::decorative(ui, response.id);
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect, format);
    }
    response
}

fn paint(painter: &Painter, rect: Rect, format: Format) {
    let k = rect.width().min(rect.height()) / 56.0;
    painter.rect_filled(rect, 14.0 * k, INK);
    painter.rect_stroke(
        rect.shrink(k),
        13.0 * k,
        Stroke::new(2.0 * k, Color32::from_white_alpha(46)),
        eframe::egui::StrokeKind::Middle,
    );
    let draw = |path: &str, color: Color32| stroke(painter, rect.min, k, path, color);
    match format {
        Format::Pdf => {
            draw(PAGE, WHITE);
            // Across the page and past its edges, as a PDF's icon has it.
            let at = |x: f32, y: f32| rect.min + vec2(x, y) * k;
            painter.rect_filled(
                Rect::from_min_max(at(8.0, 25.0), at(48.0, 40.0)),
                3.0 * k,
                RED,
            );
            painter.text(
                at(28.0, 32.5),
                Align2::CENTER_CENTER,
                "PDF",
                FontId::new(13.0 * k, theme::bold()),
                WHITE,
            );
        }
        Format::Writing => {
            draw(PAGE, WHITE);
            draw(LINES, BLUE);
        }
        Format::Slides => {
            draw(SCREEN, WHITE);
            draw(BARS, ORANGE);
        }
        Format::Book => {
            draw(BOOK, WHITE);
            draw(SPINE, PURPLE);
        }
        Format::Subtitles => {
            draw(CAPTION, WHITE);
            draw(CAPTION_LINES, YELLOW);
        }
        Format::Markdown => {
            draw(PAGE, WHITE);
            draw(HASH, GREEN);
        }
        Format::Plain => {
            draw(PAGE, WHITE);
            draw(LINES, GREY);
        }
    }
}

fn stroke(painter: &Painter, origin: Pos2, k: f32, path: &str, color: Color32) {
    let stroke = Stroke::new(4.0 * k, color);
    for sub in icons::parse(path) {
        let points: Vec<Pos2> = sub
            .points
            .iter()
            .map(|p| origin + p.to_vec2() * k)
            .collect();
        if sub.closed {
            painter.add(Shape::closed_line(points, stroke));
        } else {
            // egui's strokes end square; the logos' are round.
            if let (Some(first), Some(last)) = (points.first(), points.last()) {
                painter.circle_filled(*first, stroke.width / 2.0, color);
                painter.circle_filled(*last, stroke.width / 2.0, color);
            }
            painter.add(Shape::line(points, stroke));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_document_format_has_its_own_tile() {
        for ext in search::DOCUMENTS {
            assert_ne!(
                Format::of(Path::new(&format!("/a/b.{ext}"))),
                Format::Plain,
                "{ext}"
            );
        }
    }

    #[test]
    fn formats_go_by_extension_in_any_case() {
        let of = |p: &str| Format::of(Path::new(p));
        assert_eq!(of("/a/b.PDF"), Format::Pdf);
        assert_eq!(of("/a/b.docx"), Format::Writing);
        assert_eq!(of("/a/b.odp"), Format::Slides);
        assert_eq!(of("/a/b.epub"), Format::Book);
        assert_eq!(of("/a/b.vtt"), Format::Subtitles);
        assert_eq!(of("/a/b.md"), Format::Markdown);
        assert_eq!(of("/a/b.txt"), Format::Plain);
        assert_eq!(of("/a/README"), Format::Plain);
    }
}
