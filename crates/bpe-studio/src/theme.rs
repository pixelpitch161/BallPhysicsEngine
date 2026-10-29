//! Colours and sizes for the editor's dark theme.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x14, 0x15, 0x1d);
pub const PANEL: Color32 = Color32::from_rgb(0x1c, 0x1e, 0x29);
pub const RAISED: Color32 = Color32::from_rgb(0x2e, 0x32, 0x45);
pub const CARD: Color32 = Color32::from_rgb(0x22, 0x25, 0x33);
pub const HOVER: Color32 = Color32::from_rgb(0x3a, 0x3f, 0x57);
pub const LINE: Color32 = Color32::from_rgb(0x33, 0x37, 0x48);
pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xe8, 0xf0);
pub const MUTED: Color32 = Color32::from_rgb(0x90, 0x95, 0xab);
pub const ACCENT: Color32 = Color32::from_rgb(0x7c, 0x6c, 0xff);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xa8, 0x9d, 0xff);
pub const WARNING: Color32 = Color32::from_rgb(0xf2, 0xc4, 0x6d);
// Used by the sound job's error line.
#[allow(dead_code)]
pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x7a, 0x7a);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = CARD;
    visuals.window_stroke = Stroke::new(1.0, LINE);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = ACCENT_TEXT;
    visuals.slider_trailing_fill = true;
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    for (state, fill) in [
        (&mut widgets.inactive, RAISED),
        (&mut widgets.hovered, HOVER),
        (&mut widgets.active, HOVER),
        (&mut widgets.open, HOVER),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = CornerRadius::same(6);
    }
    widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 26.0;
    });
}
