//! The Text tab: captions over the video, such as a "Can it escape?" hook, a ball counter or a
//! timer. Each caption is a card; its words can hold live values like {balls}.

use super::{ListEdit, Roll, color_row, grid, heading_dice, label, more, percent, resettable, slider};
use crate::theme;
use bpe_scene::{
    Align, Anim, Caption, CaptionSetting, CaptionStyle, Font, Group, List, Property, React, Scene, TEMPLATES, TOKENS,
};
use eframe::egui::{self, RichText, Ui};

/// Text tab state that is not part of the scene.
#[derive(Default)]
pub struct State {
    /// A caption to open and scroll to, set when its clip is clicked on the timeline.
    pub focus: Option<usize>,
    /// Caption font files that couldn't be read, as (path, reason).
    pub font_errors: Vec<(String, String)>,
}

enum CardAction {
    Up,
    Down,
    Remove,
    Duplicate,
}

pub fn show(ui: &mut Ui, panel: &mut State, scene: &mut Scene, roll: &mut Option<Roll>) {
    ui.label(
        RichText::new(
            "Words over the video: a hook to make people watch, counters and timers. Words in braces, like {balls}, \
             show live numbers. Captions show in the preview and the exported video alike.",
        )
        .color(theme::MUTED),
    );
    ui.add_space(6.0);
    heading_dice(ui, "Captions", Group::Captions, roll);
    add_menu(ui, scene);
    ui.add_space(8.0);
    if scene.captions.is_empty() {
        ui.label(RichText::new("No captions yet. Add one above, or roll the dice for a hook.").color(theme::MUTED));
    }
    let duration = scene.video.duration;
    let count = scene.captions.len();
    let mut action = None;
    let focus = panel.focus.take();
    for (index, caption) in scene.captions.iter_mut().enumerate() {
        ui.push_id(("caption", index), |ui| {
            if let Some(a) = card(ui, caption, index, count, duration, focus == Some(index)) {
                action = Some((index, a));
            }
        });
        ui.add_space(8.0);
    }
    let edit = match action {
        Some((i, CardAction::Up)) if i > 0 => {
            scene.captions.swap(i, i - 1);
            Some(ListEdit::Swapped(i, i - 1))
        }
        Some((i, CardAction::Down)) if i + 1 < count => {
            scene.captions.swap(i, i + 1);
            Some(ListEdit::Swapped(i, i + 1))
        }
        Some((i, CardAction::Remove)) => {
            scene.captions.remove(i);
            Some(ListEdit::Removed(i))
        }
        Some((i, CardAction::Duplicate)) => {
            let mut copy = scene.captions[i].clone();
            copy.keep_clear_of(&scene.captions, scene.video.height as f32 * 0.5);
            if !copy.name.is_empty() {
                copy.name.push_str(" copy");
            }
            scene.captions.insert(i + 1, copy);
            Some(ListEdit::Duplicated(i))
        }
        _ => None,
    };
    if let Some(edit) = edit {
        edit.apply(scene, List::Captions);
    }
    for (path, error) in &panel.font_errors {
        ui.label(
            RichText::new(format!("Couldn't read the font {path}: {error}. Using the bold font instead."))
                .color(theme::WARNING),
        );
    }
}

/// Adds a caption, moved off any caption already in its spot.
fn add(scene: &mut Scene, mut caption: Caption) {
    caption.keep_clear_of(&scene.captions, scene.video.height as f32 * 0.5);
    scene.captions.push(caption);
}

fn add_menu(ui: &mut Ui, scene: &mut Scene) {
    ui.horizontal(|ui| {
        ui.menu_button(RichText::new("+ Add caption").strong(), |ui| {
            ui.set_min_width(260.0);
            for &(text, style) in TEMPLATES {
                let hover = format!("{} caption", style_name(style));
                if ui.button(text).on_hover_text(hover).clicked() {
                    add(scene, Caption::new(text, style));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Blank caption").clicked() {
                add(scene, Caption::new("Your words", CaptionStyle::BigCentre));
                ui.close();
            }
        });
    });
}

fn card(
    ui: &mut Ui,
    caption: &mut Caption,
    index: usize,
    count: usize,
    duration: f32,
    focus: bool,
) -> Option<CardAction> {
    let mut action = None;
    let frame = egui::Frame::new()
        .fill(theme::CARD)
        .stroke(egui::Stroke::new(1.0, if focus { theme::ACCENT } else { theme::LINE }))
        .corner_radius(8.0)
        .inner_margin(10.0);
    let response = frame.show(ui, |ui| {
        let id = ui.make_persistent_id("open");
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
        if focus {
            state.set_open(true);
        }
        let header = ui.horizontal(|ui| {
            state.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
            ui.checkbox(&mut caption.enabled, "").on_hover_text("Show this caption");
            let title =
                if caption.text.trim().is_empty() { "(no words)".to_string() } else { first_line(&caption.text) };
            let color = if caption.enabled { theme::TEXT } else { theme::MUTED };
            ui.label(RichText::new(title).strong().color(color));
            ui.label(RichText::new(style_name(caption.style)).small().color(theme::MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("···", |ui| {
                    if ui.button("Duplicate").clicked() {
                        action = Some(CardAction::Duplicate);
                        ui.close();
                    }
                    if ui.button("Remove").clicked() {
                        action = Some(CardAction::Remove);
                        ui.close();
                    }
                })
                .response
                .on_hover_text("More options");
                if arrow_button(ui, false, index + 1 < count)
                    .on_hover_text("Move down (drawn under the ones above)")
                    .clicked()
                {
                    action = Some(CardAction::Down);
                }
                if arrow_button(ui, true, index > 0).on_hover_text("Move up").clicked() {
                    action = Some(CardAction::Up);
                }
            });
        });
        state.store(ui.ctx());
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_body_unindented(ui, |ui| body(ui, caption, index, duration));
        header.response
    });
    if focus {
        response.response.scroll_to_me(Some(egui::Align::Center));
    }
    action
}

fn body(ui: &mut Ui, caption: &mut Caption, index: usize, duration: f32) {
    let key = |ui: &Ui, setting| crate::keys::mark_next(ui, Property::Caption(index as u32, setting));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::multiline(&mut caption.text)
            .desired_rows(1)
            .desired_width(ui.available_width() - 70.0)
            .hint_text("Your words, with {balls} for live numbers");
        ui.add(edit).on_hover_text("The caption's words. Enter starts a new line. Words in braces show live numbers.");
        ui.menu_button("{ }", |ui| {
            ui.set_min_width(320.0);
            ui.label(RichText::new("Add a live number").small().color(theme::MUTED));
            for (token, help) in TOKENS {
                if ui.button(*token).on_hover_text(*help).clicked() {
                    if !caption.text.is_empty() && !caption.text.ends_with([' ', '\n']) {
                        caption.text.push(' ');
                    }
                    caption.text.push_str(token);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Insert a live number, like the ball count or a timer");
    });
    ui.add_space(6.0);
    grid(ui, "caption", |ui| {
        label(ui, "Style", "A ready-made place, size and font. Picking one moves the caption.");
        let mut style = caption.style;
        let picked = egui::ComboBox::from_id_salt("style").selected_text(style_name(style)).show_ui(ui, |ui| {
            for s in [
                CaptionStyle::Hook,
                CaptionStyle::BigCentre,
                CaptionStyle::Counter,
                CaptionStyle::Timer,
                CaptionStyle::Subtitle,
            ] {
                ui.selectable_value(&mut style, s, style_name(s)).on_hover_text(style_help(s));
            }
        });
        named(picked.response, "Style");
        if style != caption.style {
            caption.apply_style(style);
        }
        ui.end_row();

        font_row(ui, caption);
        key(ui, CaptionSetting::Size);
        slider(
            ui,
            "Size",
            "Height of capital letters, in pixels of the video.",
            &mut caption.size,
            Caption::new("", caption.style).size,
            12.0..=400.0,
            super::units(" px", 0),
        );
        key(ui, CaptionSetting::Color);
        color_row(ui, "Colour", "Colour of the letters.", &mut caption.color, bpe_scene::Color::WHITE);
        key(ui, CaptionSetting::Outline);
        slider(
            ui,
            "Outline",
            "Width of the dark line around the letters. 0 for none.",
            &mut caption.outline,
            Caption::new("", caption.style).outline,
            0.0..=20.0,
            super::units(" px", 1),
        );
        key(ui, CaptionSetting::Glow);
        slider(ui, "Glow", "Soft light around the letters.", &mut caption.glow, 0.25, 0.0..=1.0, percent);

        label(ui, "Appears", "How the caption comes in.");
        anim_combo(ui, "Appears", &mut caption.anim_in);
        ui.end_row();
        label(ui, "Disappears", "How the caption goes out. Captions shown until the end don't go out.");
        anim_combo(ui, "Disappears", &mut caption.anim_out);
        ui.end_row();
        label(ui, "Pulse", "Make the caption grow a little with each bounce, or on each beat of the song.");
        let picked = egui::ComboBox::from_id_salt("react").selected_text(react_name(caption.react)).show_ui(ui, |ui| {
            for r in [React::None, React::PulseOnHit, React::PulseOnBeat] {
                ui.selectable_value(&mut caption.react, r, react_name(r));
            }
        });
        named(picked.response, "Pulse");
        ui.end_row();

        let end = duration.max(0.1);
        slider(
            ui,
            "Show from",
            "Seconds from the start when the caption appears. You can also drag its clip on the timeline.",
            &mut caption.show_from,
            0.0,
            0.0..=end,
            super::units(" s", 1),
        );
        label(ui, "Show until", "When the caption disappears. Untick to choose a time.");
        ui.horizontal(|ui| {
            let mut to_end = caption.show_to <= caption.show_from;
            if ui.checkbox(&mut to_end, "the end").changed() {
                caption.show_to = if to_end { 0.0 } else { (caption.show_from + 3.0).min(end) };
            }
            if !to_end {
                let low = caption.show_from;
                let response =
                    ui.add(egui::DragValue::new(&mut caption.show_to).range(low..=end).speed(0.05).suffix(" s"));
                resettable(response, &mut caption.show_to, 0.0);
            }
        });
        ui.end_row();
    });
    more(ui, "caption more", |ui| {
        grid(ui, "caption more grid", |ui| {
            let help = "Where the middle of the first line sits, from the centre of the video.";
            key(ui, CaptionSetting::X);
            offset_row(ui, "Across", help, &mut caption.position.x);
            key(ui, CaptionSetting::Y);
            offset_row(ui, "Down", help, &mut caption.position.y);
            label(ui, "Line up", "Which side lines of text line up on.");
            ui.horizontal(|ui| {
                for (a, name) in [(Align::Left, "Left"), (Align::Centre, "Centre"), (Align::Right, "Right")] {
                    ui.selectable_value(&mut caption.align, a, name);
                }
            });
            ui.end_row();
            key(ui, CaptionSetting::OutlineColor);
            color_row(
                ui,
                "Outline colour",
                "Colour of the line around the letters.",
                &mut caption.outline_color,
                Caption::default().outline_color,
            );
            slider(
                ui,
                "Animation time",
                "How long appearing and disappearing take.",
                &mut caption.anim_seconds,
                0.4,
                0.05..=3.0,
                super::units(" s", 2),
            );
            label(
                ui,
                "Under the effects",
                "Draw the caption before the shader effects, so they bend and colour it too. Off keeps it sharp on top.",
            );
            ui.checkbox(&mut caption.under_effects, "");
            ui.end_row();
            label(ui, "Name", "A name for rules to refer to this caption by. Empty uses its words.");
            let hint = caption.default_name();
            ui.add(egui::TextEdit::singleline(&mut caption.name).hint_text(hint).desired_width(200.0));
            ui.end_row();
        });
    });
}

fn font_row(ui: &mut Ui, caption: &mut Caption) {
    label(ui, "Font", "The letters' typeface. Pick a .ttf or .otf file for your own.");
    ui.horizontal(|ui| {
        let name = font_name(&caption.font);
        let picked = egui::ComboBox::from_id_salt("font").selected_text(name).show_ui(ui, |ui| {
            ui.selectable_value(&mut caption.font, Font::Display, "Bold").on_hover_text("Tall and bold, for hooks");
            ui.selectable_value(&mut caption.font, Font::Rounded, "Rounded").on_hover_text("Soft and friendly");
            ui.selectable_value(&mut caption.font, Font::Mono, "Digits")
                .on_hover_text("Every digit the same width, so counters don't wobble");
            if ui.button("Font file…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Font", &["ttf", "otf"]).pick_file() {
                    caption.font = Font::File(path.display().to_string());
                }
                ui.close();
            }
        });
        named(picked.response, "Font");
    });
    ui.end_row();
}

fn anim_combo(ui: &mut Ui, id: &str, anim: &mut Anim) {
    let picked = egui::ComboBox::from_id_salt(id).selected_text(anim_name(*anim)).show_ui(ui, |ui| {
        for a in [Anim::None, Anim::Fade, Anim::Pop, Anim::Slide, Anim::Typewriter, Anim::Bounce, Anim::Glitch] {
            ui.selectable_value(anim, a, anim_name(a));
        }
    });
    named(picked.response, id);
}

/// Gives a drop-down the setting's name, for screen readers and tests.
/// A small button with a painted arrow, since the editor's font has no arrow characters.
fn arrow_button(ui: &mut Ui, up: bool, enabled: bool) -> egui::Response {
    let button = egui::Button::new("").min_size(egui::vec2(24.0, 22.0));
    let response = ui.add_enabled(enabled, button);
    let c = response.rect.center();
    let (tip, base) = if up { (-4.0, 3.0) } else { (4.0, -3.0) };
    let color = if enabled { theme::TEXT } else { theme::MUTED };
    ui.painter().add(egui::Shape::convex_polygon(
        vec![c + egui::vec2(0.0, tip), c + egui::vec2(4.5, base), c + egui::vec2(-4.5, base)],
        color,
        egui::Stroke::NONE,
    ));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, enabled, if up { "Move up" } else { "Move down" })
    });
    response
}

pub(crate) fn named(response: egui::Response, name: &str) -> egui::Response {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, name));
    response
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default();
    let mut short: String = line.chars().take(28).collect();
    if line.chars().count() > 28 || text.lines().count() > 1 {
        short.push('…');
    }
    short
}

pub fn style_name(style: CaptionStyle) -> &'static str {
    match style {
        CaptionStyle::Hook => "Hook",
        CaptionStyle::BigCentre => "Big centre",
        CaptionStyle::Counter => "Counter",
        CaptionStyle::Timer => "Timer",
        CaptionStyle::Subtitle => "Subtitle",
    }
}

fn style_help(style: CaptionStyle) -> &'static str {
    match style {
        CaptionStyle::Hook => "Big words near the top that ask a question",
        CaptionStyle::BigCentre => "Huge words in the middle",
        CaptionStyle::Counter => "A number with a word, below the arena",
        CaptionStyle::Timer => "A clock at the top",
        CaptionStyle::Subtitle => "A short line near the bottom",
    }
}

fn font_name(font: &Font) -> String {
    match font {
        Font::Display => "Bold".into(),
        Font::Rounded => "Rounded".into(),
        Font::Mono => "Digits".into(),
        Font::File(path) => {
            std::path::Path::new(path).file_name().map_or(path.clone(), |n| n.to_string_lossy().into_owned())
        }
    }
}

fn anim_name(anim: Anim) -> &'static str {
    match anim {
        Anim::None => "Just appears",
        Anim::Fade => "Fade",
        Anim::Pop => "Pop",
        Anim::Slide => "Slide",
        Anim::Typewriter => "Typewriter",
        Anim::Bounce => "Bounce",
        Anim::Glitch => "Glitch",
    }
}

fn react_name(react: React) -> &'static str {
    match react {
        React::None => "No",
        React::PulseOnHit => "On each bounce",
        React::PulseOnBeat => "On each beat",
    }
}

/// One of a caption's two position numbers, in pixels from the centre of the video.
fn offset_row(ui: &mut Ui, text: &str, help: &str, value: &mut f32) {
    let name = label(ui, text, help);
    let number = ui.add(egui::DragValue::new(value).speed(1.0).suffix(" px")).on_hover_text(help);
    resettable(name, value, 0.0);
    resettable(number, value, 0.0);
    ui.end_row();
}
