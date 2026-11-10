//! The editor's look: the Ball Studio Design System's tokens, fonts and drawn controls. This is the
//! only place in the editor that names a colour, a font or a size.

mod controls;
mod fonts;
mod knob;
mod meter;
mod swatch;
mod tokens;
mod track;
mod video;

pub use controls::*;
pub use fonts::{ensure_text_fonts, family_ready, medium, mono, semibold, text_family};
// The knob's rings, response and plain form are for the sound parts after this one.
#[allow(unused_imports)]
pub use knob::{Knob, KnobResponse, ModRing, RingDrag, knob, wheel_notches};
pub use meter::{level_meter, loudness_graph, loudness_row};
pub use swatch::surface_swatch;
pub use tokens::*;
pub use track::{colour_of, track_color};
pub use video::{scope_hue, video, video_bytes};

use eframe::egui::{self, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};

/// Sets the fonts, text sizes, colours and spacing on `ctx`.
pub fn apply(ctx: &egui::Context) {
    fonts::install(ctx);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = GROUND;
    visuals.text_edit_bg_color = Some(GROUND);
    visuals.faint_bg_color = RAISED;
    visuals.code_bg_color = GROUND;
    visuals.window_stroke = Stroke::new(1.0, LINE);
    visuals.window_corner_radius = CornerRadius::same(RADIUS);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS);
    let shadow = Shadow { offset: [0, 8], blur: 20, spread: 0, color: egui::Color32::from_black_alpha(0xaa) };
    visuals.popup_shadow = shadow;
    visuals.window_shadow = shadow;
    // Text selection and selectable labels: a grey fill, so light text stays readable on it.
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = Stroke::new(1.0, TEXT);
    visuals.hyperlink_color = TEXT;
    visuals.warn_fg_color = TEXT;
    visuals.error_fg_color = TEXT;
    visuals.slider_trailing_fill = false;
    visuals.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.25 };
    visuals.indent_has_left_vline = false;
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE_SOFT);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.bg_fill = LINE;
    widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.hovered.bg_fill = HOVER;
    widgets.hovered.weak_bg_fill = HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, LINE);
    widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    for state in [&mut widgets.active, &mut widgets.open] {
        state.bg_fill = TEXT;
        state.weak_bg_fill = TEXT;
        state.bg_stroke = Stroke::new(1.0, TEXT);
        state.fg_stroke = Stroke::new(1.0, ON_LIGHT);
    }
    for state in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.corner_radius = CornerRadius::same(RADIUS);
        state.expansion = 0.0;
    }
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Heading, semibold(18.0)),
            (TextStyle::Monospace, mono(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size.y = CONTROL_H;
        style.spacing.menu_margin = Margin::same(2);
        style.spacing.slider_rail_height = 2.0;
        style.spacing.indent = 14.0;
    });
}

/// Inside `add`, value fields and number boxes are dark wells, as in a settings row.
pub fn wells<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.weak_bg_fill = GROUND;
        w.hovered.weak_bg_fill = GROUND;
        w.hovered.bg_stroke = Stroke::new(1.0, MUTED);
        add(ui)
    })
    .inner
}

/// Inside `add`, buttons and dropdowns are raised grey, as dropdowns in a settings row.
pub fn raised<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.weak_bg_fill = RAISED;
        add(ui)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::TrackColour;
    use eframe::egui::Color32;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn a_clip_mixes_22_percent_over_the_panel() {
        let mix = |p: u8, c: u8| (p as f32 + (c as f32 - p as f32) * 0.22).round() as u8;
        let pink = track_color(TrackColour::PINK);
        let want = Color32::from_rgb(mix(0x28, pink.r()), mix(0x28, pink.g()), mix(0x28, pink.b()));
        assert_eq!(track::mix_over_panel(pink), want);
        assert_eq!(want, Color32::from_rgb(0x57, 0x38, 0x44));
    }

    #[test]
    fn a_switch_toggles_on_click() {
        let mut h = Harness::builder().with_context_setup(apply).build_ui_state(
            |ui, on: &mut bool| {
                let label = ui.label("Loop");
                switch(ui, on).labelled_by(label.id);
            },
            false,
        );
        h.run();
        h.get_by_label("Loop").click();
        h.run();
        assert!(*h.state());
    }

    #[test]
    fn the_track_picker_picks() {
        let mut h = Harness::builder().with_context_setup(apply).allow_missing_glyphs().build_ui_state(
            |ui, picked: &mut Option<TrackColour>| {
                if let Some(c) = track_picker(ui, Some(TrackColour::PINK)) {
                    *picked = Some(c);
                }
            },
            None,
        );
        h.run();
        h.get_by_label("Colour hex").click();
        h.run();
        h.get_by_label("Colour hex").type_text("#3fd6b5");
        h.run();
        assert_eq!(*h.state(), Some(TrackColour::TEAL));
    }

    /// Panels draw through this module: no other editor file makes up a colour. A line that shows
    /// a real scene colour (a swatch, a picture) says so with a `// scene colour` comment.
    #[test]
    fn no_panel_names_its_own_colour() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let banned = [
            "Color32::from_rgb(",
            "Color32::from_gray(",
            "Color32::WHITE",
            "Color32::BLACK",
            "from_white_alpha(",
            "from_black_alpha(",
            "from_rgba_",
        ];
        let mut found = Vec::new();
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n != "style") {
                        stack.push(path);
                    }
                    continue;
                }
                // Tests may build whatever colours they like: test files (`tests.rs`, `*_tests.rs`) and a
                // file's own test module are skipped.
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "tests.rs" || name.ends_with("_tests.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let code = text.split("#[cfg(test)]\nmod tests {").next().unwrap_or("");
                for (i, line) in code.lines().enumerate() {
                    if banned.iter().any(|b| line.contains(b)) && !line.contains("// scene colour") {
                        found.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                    }
                }
            }
        }
        assert!(found.is_empty(), "colours outside style/:\n{}", found.join("\n"));
    }
}
