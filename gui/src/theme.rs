//! vinoSearch's settings theme (its crates/settings/src/theme.rs), so the vino windows
//! match. Colours, type and shapes follow the vinoPilot landing page's site.css:
//! Inter, flat surfaces separated by hairlines, 6px corners, a green that only
//! ever means "this one".

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Theme, Visuals,
};

pub struct Palette {
    pub bg: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub rule: Color32,
    pub code_bg: Color32,
    pub link: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const DARK: Palette = Palette {
    bg: rgb(0x121315),
    text: rgb(0xe4e5e8),
    muted: rgb(0x9a9ea6),
    rule: rgb(0x2a2c30),
    code_bg: rgb(0x1e2023),
    link: rgb(0x7bdc97),
};

pub const LIGHT: Palette = Palette {
    bg: rgb(0xfcfcfc),
    text: rgb(0x1c1d20),
    muted: rgb(0x5d6068),
    rule: rgb(0xe2e3e6),
    code_bg: rgb(0xeef0f2),
    link: rgb(0x1a7040),
};

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

// winit on Wayland only knows the theme of decorations it draws itself, so egui would fall
// back to dark on a light desktop. Ask the portal instead.
pub fn desktop_preference() -> Option<Theme> {
    let output = std::process::Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "ReadOne",
            "ss",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_color_scheme(&String::from_utf8_lossy(&output.stdout))
}

// 1 prefers dark, 2 prefers light, 0 is no preference, which desktops answer for their light default.
fn parse_color_scheme(reply: &str) -> Option<Theme> {
    match reply.split_whitespace().last()?.parse::<u32>().ok()? {
        1 => Some(Theme::Dark),
        _ => Some(Theme::Light),
    }
}

pub fn follow_desktop(ctx: &egui::Context) {
    if let Some(theme) = desktop_preference() {
        ctx.set_theme(theme);
    }
}

pub fn bold() -> FontFamily {
    FontFamily::Name("Inter-Bold".into())
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "Inter-Regular",
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..],
        ),
        (
            "Inter-Bold",
            &include_bytes!("../assets/fonts/Inter-Bold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    // Regular leads the stock families so every widget picks it up; the default fonts stay
    // behind it for the glyphs the latin subset lacks.
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "Inter-Regular".to_owned());
    }
    // Monospace is for commands and paths only, so keep the stock face first there.
    let mono = fonts.families.get_mut(&FontFamily::Monospace).unwrap();
    mono.remove(0);
    mono.push("Inter-Regular".to_owned());
    fonts.families.insert(
        bold(),
        vec!["Inter-Bold".to_owned(), "Inter-Regular".to_owned()],
    );
    fonts
}

fn visuals(p: &Palette, base: Visuals) -> Visuals {
    let corner = CornerRadius::same(6);
    let mut v = base;
    v.panel_fill = p.bg;
    v.window_fill = p.bg;
    v.extreme_bg_color = p.code_bg;
    v.faint_bg_color = p.code_bg;
    v.hyperlink_color = p.link;
    v.warn_fg_color = p.link;
    v.window_stroke = Stroke::new(1.0_f32, p.rule);
    v.selection.bg_fill = p.link.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, p.link);

    v.widgets.noninteractive.bg_fill = p.bg;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.rule);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    for widget in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        widget.corner_radius = corner;
    }
    v.widgets.noninteractive.corner_radius = corner;
    v.widgets.inactive.bg_fill = p.code_bg;
    v.widgets.inactive.weak_bg_fill = p.code_bg;
    // halfway to muted: the unchecked box of a checkbox has to show against the page
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.rule.lerp_to_gamma(p.muted, 0.5));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.hovered.bg_fill = p.code_bg;
    v.widgets.hovered.weak_bg_fill = p.code_bg;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.muted);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.active.bg_fill = p.code_bg;
    v.widgets.active.weak_bg_fill = p.code_bg;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.link);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text);
    v.widgets.open.bg_fill = p.code_bg;
    v.widgets.open.weak_bg_fill = p.code_bg;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.link);
    v
}

/// A solid scroll bar paints its handle in `bg_fill` on an `extreme_bg_color`
/// track, and the visuals above make both `code_bg`, so the handle vanished.
/// Nothing else here uses `bg_fill` (buttons take `weak_bg_fill`), so give it
/// the checkbox outline's grey, lighter while hovered and green while dragged.
pub fn visible_scroll_handles(ctx: &egui::Context) {
    for (theme, p) in [(Theme::Dark, &DARK), (Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| {
            let w = &mut style.visuals.widgets;
            w.inactive.bg_fill = p.rule.lerp_to_gamma(p.muted, 0.5);
            w.hovered.bg_fill = p.muted;
            w.active.bg_fill = p.link;
        });
    }
}

pub fn install(ctx: &egui::Context) {
    style(ctx);
    follow_desktop(ctx);
}

/// Fonts, colours and spacing, without asking the desktop for light or dark.
pub fn style(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, Visuals::dark()));
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, Visuals::light()));
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.5));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(30.0, bold()));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.0));
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = 28.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_portal_reply_picks_the_theme() {
        assert_eq!(parse_color_scheme("v u 1\n"), Some(Theme::Dark));
        assert_eq!(parse_color_scheme("v u 2\n"), Some(Theme::Light));
        assert_eq!(parse_color_scheme("v u 0\n"), Some(Theme::Light));
        assert_eq!(parse_color_scheme(""), None);
        assert_eq!(parse_color_scheme("garbage"), None);
    }
}
