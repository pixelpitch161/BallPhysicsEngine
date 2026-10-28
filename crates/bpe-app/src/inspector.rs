//! The scene settings panel, split into tabs. Every widget edits the [`Scene`] in place; the
//! app notices the change and rebuilds the preview and soundtrack.
//!
//! Each setting has a plain name and a "?" with a one-sentence explanation. Double-clicking a
//! setting's name or slider, Alt+clicking it, or its right-click menu puts it back to its
//! default, and most tabs end with a "Reset this tab" button. Settings most people never touch
//! sit under "More settings" in each tab. A dice beside each group heading rolls random values
//! for that group.

pub mod ball_look;
pub mod effects;
pub mod gpu;
pub mod music;
pub mod particles;
pub mod rules;
pub mod shapes;
pub mod soft;
pub mod text;
pub mod view3d;

pub use effects::EffectsPanel;
pub(crate) use effects::pretty_name;

use crate::keys::mark_next as key;
use crate::theme;
use bpe_scene::{
    ArenaMotion, BallStyle, BounceRules, Color, Container, Dice, Group, List, Look, Obstacle, Physics, Property, Scene,
    Spawner, Vec2,
};
use eframe::egui::{
    self, Color32, DragValue, Popup, Response, RichText, Sense, Slider, Ui, color_picker::Alpha, emath::Numeric,
};
use std::ops::RangeInclusive;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Balls,
    Arena,
    Bounces,
    Look,
    Effects,
    Sound,
    Rules,
    Text,
}

impl Tab {
    pub const ALL: [Tab; 8] =
        [Tab::Balls, Tab::Arena, Tab::Bounces, Tab::Look, Tab::Effects, Tab::Sound, Tab::Rules, Tab::Text];

    pub fn name(self) -> &'static str {
        match self {
            Tab::Balls => "Balls",
            Tab::Arena => "Arena",
            Tab::Bounces => "Bounces",
            Tab::Look => "Look",
            Tab::Effects => "Effects",
            Tab::Sound => "Sound",
            Tab::Rules => "Rules",
            Tab::Text => "Text",
        }
    }

    /// Draws the tab's icon, so it looks the same whatever fonts are installed.
    fn paint_icon(self, painter: &egui::Painter, c: egui::Pos2, color: Color32) {
        let stroke = egui::Stroke::new(1.6, color);
        let v = egui::vec2;
        match self {
            Tab::Balls => {
                for offset in [v(-5.0, 3.0), v(5.0, 3.0), v(0.0, -5.0)] {
                    painter.circle_filled(c + offset, 3.2, color);
                }
            }
            Tab::Arena => {
                painter.circle_stroke(c, 8.0, stroke);
                painter.circle_filled(c + v(2.0, 3.0), 2.2, color);
            }
            Tab::Bounces => {
                let points = vec![c + v(-9.0, -6.0), c + v(-3.0, 7.0), c + v(3.0, -4.0), c + v(9.0, 6.0)];
                painter.add(egui::Shape::line(points, stroke));
            }
            Tab::Look => {
                painter.circle_filled(c, 3.5, color);
                for i in 0..8 {
                    let angle = i as f32 * std::f32::consts::TAU / 8.0;
                    let dir = v(angle.cos(), angle.sin());
                    painter.line_segment([c + dir * 6.0, c + dir * 9.0], stroke);
                }
            }
            Tab::Effects => {
                // A four-point sparkle, drawn as two thin diamonds.
                let (long, short) = (9.0, 2.4);
                for (x, y) in [(short, long), (long, short)] {
                    let points = vec![c + v(0.0, -y), c + v(x, 0.0), c + v(0.0, y), c + v(-x, 0.0)];
                    painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
                }
            }
            Tab::Sound => {
                painter.circle_filled(c + v(-4.0, 5.0), 3.2, color);
                painter.circle_filled(c + v(5.0, 3.0), 3.2, color);
                painter.line_segment([c + v(-1.2, 5.0), c + v(-1.2, -8.0)], stroke);
                painter.line_segment([c + v(7.8, 3.0), c + v(7.8, -10.0)], stroke);
                painter.line_segment([c + v(-1.2, -8.0), c + v(7.8, -10.0)], egui::Stroke::new(2.4, color));
            }
            Tab::Rules => {
                // Two blocks joined by a wire.
                painter.rect_stroke(
                    egui::Rect::from_center_size(c + v(-5.5, -4.0), v(7.0, 6.0)),
                    1.5,
                    stroke,
                    egui::StrokeKind::Middle,
                );
                painter.rect_stroke(
                    egui::Rect::from_center_size(c + v(5.5, 4.0), v(7.0, 6.0)),
                    1.5,
                    stroke,
                    egui::StrokeKind::Middle,
                );
                let wire = vec![c + v(-2.0, -4.0), c + v(0.5, -4.0), c + v(-0.5, 4.0), c + v(2.0, 4.0)];
                painter.add(egui::Shape::line(wire, stroke));
            }
            Tab::Text => {
                // A capital T.
                painter.line_segment([c + v(-7.0, -7.0), c + v(7.0, -7.0)], egui::Stroke::new(2.2, color));
                painter.line_segment([c + v(0.0, -7.0), c + v(0.0, 8.0)], egui::Stroke::new(2.2, color));
            }
        }
    }
}

/// Panel state that is not part of the scene.
pub struct Inspector {
    pub tab: Tab,
    /// The ball group last clicked in the preview, highlighted in the Balls tab.
    pub selected: Option<usize>,
    pub effects: EffectsPanel,
    /// Set when a dice button rolled new settings this frame, with a line for the status bar.
    /// The app keeps each roll as its own undo step.
    pub rolled: Option<String>,
    /// How many times the dice have been rolled, mixed into each roll's seed.
    pub(crate) rolls: u64,
    /// Each feature section's own panel state.
    pub rules: rules::State,
    pub text: text::State,
    pub soft: soft::State,
    pub shapes: shapes::State,
    pub particles: particles::State,
    pub gpu: gpu::State,
    pub view3d: view3d::State,
    pub music: music::State,
    /// Ball groups' own looks: which groups are picked together, and a copied look.
    pub looks: ball_look::State,
}

impl Default for Inspector {
    fn default() -> Self {
        Self {
            tab: Tab::Balls,
            selected: None,
            effects: EffectsPanel::default(),
            rolled: None,
            rolls: 0,
            rules: Default::default(),
            text: Default::default(),
            soft: Default::default(),
            shapes: Default::default(),
            particles: Default::default(),
            gpu: Default::default(),
            view3d: Default::default(),
            music: Default::default(),
            looks: Default::default(),
        }
    }
}

/// A dice button clicked this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Roll {
    Group(Group),
    /// One effect in the Effects tab, by its place in the list.
    Effect(usize),
}

impl Inspector {
    /// A fresh seed for a dice button, different on every click.
    fn next_seed(&mut self) -> u64 {
        self.rolls += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        (nanos as u64) ^ self.rolls.wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }

    /// Rolls new values for what a dice button stands for.
    pub fn apply_roll(&mut self, roll: Roll, scene: &mut Scene) {
        let mut dice = Dice::new(self.next_seed());
        let what = match roll {
            Roll::Group(group) => {
                scene.roll(group, &mut dice, 1.0);
                format!("Rolled new {} settings.", group.name())
            }
            Roll::Effect(index) => {
                let Some(effect) = scene.effects.get_mut(index) else { return };
                effects::roll_effect(effect, &self.effects.library, &mut dice, 1.0);
                "Rolled new effect settings.".into()
            }
        };
        self.rolled = Some(format!("{what} Ctrl+Z puts them back."));
    }
}

/// Seconds of arrow length per pixel per second of launch speed, shared with the preview handles.
pub const ARROW_SECONDS: f32 = 0.35;

pub fn show(ui: &mut Ui, inspector: &mut Inspector, scene: &mut Scene) {
    tab_bar(ui, &mut inspector.tab);
    crate::command_palette::drawing_tab(ui.ctx(), inspector.tab);
    let mut roll = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
            match inspector.tab {
                Tab::Balls => {
                    let space = scene.space().is_some(); // 3D (WS4)
                    let edit = balls(
                        ui,
                        &mut scene.spawners,
                        &mut inspector.selected,
                        &mut inspector.looks,
                        &scene.look,
                        &mut roll,
                        space,
                    );
                    if let Some(edit) = edit {
                        edit.apply(scene, List::BallGroups);
                    }
                    soft::section(ui, scene, &mut inspector.soft);
                    gpu::section(ui, scene, &mut inspector.gpu);
                }
                Tab::Arena => {
                    // 3D (WS4): a 3D scene has its own container, and no flat shapes or pegs.
                    let flat = !view3d::section(ui, scene, &mut inspector.view3d);
                    arena(
                        ui,
                        &mut scene.container,
                        &mut scene.arena_motion,
                        &mut scene.physics,
                        &mut scene.shapes,
                        &mut roll,
                        flat,
                    );
                    if flat {
                        shapes::section(ui, scene, &mut inspector.shapes);
                        obstacles(ui, &mut scene.obstacles, &mut scene.shapes);
                    }
                    // Pegs and walls are things you added, like balls, so a reset keeps them. A
                    // custom arena keeps its outline.
                    let fresh = match &scene.container {
                        Container::Custom { shape, .. } => Container::Custom {
                            shape: shape.clone(),
                            center: Vec2::ZERO,
                            radius: shapes::ARENA_RADIUS,
                            rotation: 0.0,
                            thickness: LINE_WIDTH,
                            color: LINE_COLOR,
                        },
                        other => default_container(Shape::of(other)),
                    };
                    let motion = ArenaMotion::default();
                    // Ball bumping lives on the Bounces tab, so this reset leaves it alone.
                    let physics = Physics {
                        ball_collisions: scene.physics.ball_collisions,
                        contact: scene.physics.contact.clone(),
                        ..Physics::default()
                    };
                    // 3D (WS4): in 3D the reset puts back the 3D settings instead of the hidden flat arena.
                    let fresh_space = view3d::reset_target(scene);
                    let changed = (flat && scene.container != fresh)
                        || scene.arena_motion != motion
                        || scene.physics != physics
                        || scene.space() != fresh_space.as_ref();
                    if reset_tab(ui, changed) {
                        if flat {
                            scene.container = fresh;
                        }
                        if let Some(space) = fresh_space {
                            scene.view = bpe_scene::View::Space(Box::new(space));
                        }
                        scene.arena_motion = motion;
                        scene.physics = physics;
                    }
                }
                Tab::Bounces => {
                    bounces(ui, &mut scene.rules, &mut roll);
                    soft::bounces_section(ui, scene, &mut inspector.soft);
                    let fresh = Physics::default();
                    let changed = scene.rules != BounceRules::default()
                        || scene.physics.ball_collisions != fresh.ball_collisions
                        || scene.physics.contact != fresh.contact;
                    if reset_tab(ui, changed) {
                        scene.rules = BounceRules::default();
                        scene.physics.ball_collisions = fresh.ball_collisions;
                        scene.physics.contact = fresh.contact;
                    }
                }
                Tab::Look => {
                    look(ui, &mut scene.look, &mut roll);
                    particles::section(ui, scene, &mut inspector.particles);
                    reset_tab_to(ui, &mut scene.look, Look::default());
                }
                Tab::Effects => {
                    effects::show(ui, &mut inspector.effects, scene, &mut roll);
                    if reset_tab(ui, !scene.effects.is_empty()) {
                        scene.effects.clear();
                        scene.remap_effect_tracks(|_| None);
                    }
                }
                Tab::Sound => {
                    // Music workstream: the whole Sound tab, its reset button included, lives in music.rs.
                    music::section(ui, scene, &mut inspector.music, &mut roll);
                }
                Tab::Rules => rules::section(ui, scene, &mut inspector.rules),
                Tab::Text => {
                    text::show(ui, &mut inspector.text, scene, &mut roll);
                    if reset_tab(ui, !scene.captions.is_empty()) {
                        scene.captions.clear();
                        scene.remap_tracks(List::Captions, |_| None);
                    }
                }
            }
            tip(ui);
        });
    });
    if let Some(roll) = roll {
        inspector.apply_roll(roll, scene);
    }
}

/// A button at the end of a tab that puts everything in it back to its default. Returns whether
/// it was clicked.
fn reset_tab(ui: &mut Ui, changed: bool) -> bool {
    ui.add_space(14.0);
    ui.add_enabled(changed, egui::Button::new("Reset this tab"))
        .on_hover_text("Put every setting in this tab back to its default. Ctrl+Z brings them back.")
        .clicked()
}

fn reset_tab_to<T: PartialEq>(ui: &mut Ui, value: &mut T, default: T) {
    if reset_tab(ui, *value != default) {
        *value = default;
    }
}

fn tip(ui: &mut Ui) {
    ui.add_space(16.0);
    ui.label(
        RichText::new(
            "Tip: double-click a setting's name or slider, or Alt+click it, to reset it. \
             Hold Shift while dragging a slider for fine changes. A dice rolls random settings for its group.",
        )
        .small()
        .color(theme::MUTED),
    );
}

fn tab_bar(ui: &mut Ui, current: &mut Tab) {
    let width = ui.available_width() / Tab::ALL.len() as f32;
    // Eight tabs share the panel, so names shrink a little on a narrow panel to stay readable.
    let font = (width / 4.6).clamp(10.5, 14.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for tab in Tab::ALL {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 52.0), Sense::click());
            let selected = *current == tab;
            let name = tab.name();
            let color = if selected {
                theme::TEXT
            } else if response.hovered() {
                theme::TEXT.gamma_multiply(0.85)
            } else {
                theme::MUTED
            };
            let painter = ui.painter();
            tab.paint_icon(painter, rect.center_top() + egui::vec2(0.0, 17.0), color);
            painter.text(
                rect.center_top() + egui::vec2(0.0, 37.0),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::proportional(font),
                color,
            );
            if selected {
                painter.rect_filled(
                    egui::Rect::from_min_max(rect.left_bottom() - egui::vec2(0.0, 3.0), rect.right_bottom()),
                    0.0,
                    theme::ACCENT,
                );
            }
            if response.clicked() {
                *current = tab;
            }
        }
    });
    let rect = ui.max_rect();
    ui.painter().hline(rect.x_range(), ui.cursor().top(), egui::Stroke::new(1.0, theme::LINE));
}

// ---------- building blocks ----------

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text.to_uppercase()).size(12.0).color(theme::MUTED));
}

/// A group heading with a dice button that rolls new values for the group.
fn heading_dice(ui: &mut Ui, text: &str, group: Group, roll: &mut Option<Roll>) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(text.to_uppercase()).size(12.0).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if dice_button(ui, &format!("Roll random {} settings", group.name())).clicked() {
                *roll = Some(Roll::Group(group));
            }
        });
    });
}

/// A small button with a drawn dice, so it looks the same whatever fonts are installed.
pub fn dice_button(ui: &mut Ui, what: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 22.0), Sense::click());
    let response = response.on_hover_text(format!("{what}. Ctrl+Z puts them back."));
    paint_dice(ui.painter(), rect, response.hovered());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, what));
    response
}

pub fn paint_dice(painter: &egui::Painter, rect: egui::Rect, hovered: bool) {
    if hovered {
        painter.rect_filled(rect, 5.0, theme::HOVER);
    }
    let color = if hovered { theme::TEXT } else { theme::ACCENT_TEXT };
    let face = egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
    painter.rect_stroke(face, 3.0, egui::Stroke::new(1.4, color), egui::StrokeKind::Middle);
    let c = face.center();
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (0.0, 0.0), (-1.0, 1.0), (1.0, 1.0)] {
        painter.circle_filled(c + egui::vec2(x, y) * 3.6, 1.35, color);
    }
}

fn grid(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([12.0, 10.0]).min_col_width(130.0).show(ui, add);
}

fn more(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    egui::CollapsingHeader::new(RichText::new("More settings").color(theme::ACCENT_TEXT))
        .id_salt(id)
        .default_open(false)
        .show(ui, add);
}

/// A small circled question mark that explains a setting when hovered.
fn help_icon(ui: &mut Ui, help: &str) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
    let color = if response.hovered() { theme::TEXT } else { theme::MUTED };
    ui.painter().circle_stroke(rect.center(), 7.0, egui::Stroke::new(1.0, color));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(11.0), color);
    response.on_hover_text(help);
}

/// A setting's name. It senses clicks so that double-clicking it resets the setting. A setting
/// marked with [`key`] just before gets a keyframe diamond.
fn label(ui: &mut Ui, text: &str, help: &str) -> Response {
    let animatable = crate::keys::take_next(ui);
    crate::setting_menu::set_row_property(ui.ctx(), animatable.clone());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let response = hint(ui.add(egui::Label::new(text).selectable(false).sense(Sense::click())), help);
        crate::command_palette::setting_row(&response, text, animatable.as_ref());
        if !help.is_empty() {
            help_icon(ui, help);
        }
        if let Some(property) = animatable {
            // WS5: a wave beside settings that pulse with the music.
            crate::setting_menu::pulse::wave_icon(ui, &property);
            crate::keys::diamond(ui, property);
        }
        response
    })
    .inner
}

fn hint(response: Response, help: &str) -> Response {
    if help.is_empty() { response } else { response.on_hover_text(help) }
}

/// Whether this click asks to reset a setting: a double-click, or a click with Alt held.
/// A double-click in a number box being typed into selects text instead.
fn reset_clicked(response: &Response) -> bool {
    let primary = egui::PointerButton::Primary;
    // egui counts a double-click that comes soon after a click elsewhere as a triple-click.
    let (alt, clicked, many_clicks) = response.ctx.input(|i| {
        let many = i.pointer.button_double_clicked(primary) || i.pointer.button_triple_clicked(primary);
        (i.modifiers.alt, i.pointer.button_clicked(primary), many)
    });
    // Sliders only sense drags, so a click on one shows up as a drag that stops where it started.
    let clicked = response.clicked() || (response.drag_stopped() && clicked);
    (clicked && alt) || (clicked && many_clicks && !response.ctx.text_edit_focused())
}

/// Puts a setting back to its default on a double-click, an Alt+click or the right-click menu.
fn resettable<T: PartialEq>(response: Response, value: &mut T, default: T) -> Response {
    let mut reset = reset_clicked(&response);
    let target = crate::setting_menu::Target {
        property: crate::setting_menu::take_row_property(&response.ctx),
        label: String::new(),
    };
    response.context_menu(|ui| {
        reset |= crate::setting_menu::items(ui, &target, *value != default);
    });
    if reset {
        // Alt+click on a number box would otherwise start typing into it.
        response.ctx.memory_mut(|m| m.stop_text_input());
        *value = default;
    }
    response
}

/// One labelled slider row in a grid, with the reset shortcuts.
fn slider<T: Numeric>(
    ui: &mut Ui,
    text: &str,
    help: &str,
    value: &mut T,
    default: T,
    range: RangeInclusive<T>,
    style: impl FnOnce(Slider<'_>) -> Slider<'_>,
) -> Response {
    let response = slider_row(ui, text, help, value, range, style);
    resettable(response, value, default)
}

/// One labelled slider row in a grid, for values that reset some other way. The response
/// covers the name as well as the slider, so both can be double-clicked.
fn slider_row<T: Numeric>(
    ui: &mut Ui,
    text: &str,
    help: &str,
    value: &mut T,
    range: RangeInclusive<T>,
    style: impl FnOnce(Slider<'_>) -> Slider<'_>,
) -> Response {
    let name = label(ui, text, help);
    // Fill the rest of the row, leaving room for the number box beside the slider.
    ui.spacing_mut().slider_width = (ui.available_width() - 120.0).max(60.0);
    let (min, max) = (range.start().to_f64(), range.end().to_f64());
    let before = value.to_f64();
    let response = hint(ui.add(style(Slider::new(value, range))), help);
    fine_drag(ui, &response, value, before, min, max);
    ui.end_row();
    response.union(name)
}

/// While Shift is held, a slider drag moves the value a tenth as far, for small changes.
fn fine_drag<T: Numeric>(ui: &Ui, response: &Response, value: &mut T, before: f64, min: f64, max: f64) {
    const FINE: f64 = 0.1;
    let id = response.id.with("fine drag");
    // Remember where the slider itself would put the value and where the fine value is, so each
    // frame adds a tenth of the slider's own movement. Only the slider itself: its number box
    // already drags slowly with Shift.
    let last = ui.data(|d| d.get_temp::<(f64, f64)>(id));
    let dragging = ui.ctx().is_being_dragged(response.id);
    // The slider moves to the pointer once more on the frame the button is let go.
    let letting_go = last.is_some() && response.drag_stopped();
    if !((dragging || letting_go) && ui.input(|i| i.modifiers.shift)) {
        ui.data_mut(|d| d.remove::<(f64, f64)>(id));
        return;
    }
    let raw = value.to_f64();
    let fine = match last {
        Some((last_raw, fine)) => (fine + (raw - last_raw) * FINE).clamp(min.min(max), max.max(min)),
        None => before,
    };
    if dragging {
        ui.data_mut(|d| d.insert_temp(id, (raw, fine)));
    } else {
        ui.data_mut(|d| d.remove::<(f64, f64)>(id));
    }
    *value = T::from_f64(if T::INTEGRAL { fine.round() } else { fine });
}

/// A 0..1 value shown as a percentage.
fn percent(slider: Slider<'_>) -> Slider<'_> {
    slider
        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
        .custom_parser(|s| s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0))
}

/// Shows a value with a unit. This only formats the number: the slider's own decimal settings
/// would round the stored value, which would count as an edit as soon as a scene is opened.
fn units(suffix: &'static str, decimals: usize) -> impl FnOnce(Slider<'_>) -> Slider<'_> {
    move |slider| {
        slider
            .custom_formatter(move |v, _| format!("{v:.decimals$}{suffix}"))
            .custom_parser(move |s| s.trim().trim_end_matches(suffix.trim()).trim().parse().ok())
    }
}

fn log_units(suffix: &'static str) -> impl FnOnce(Slider<'_>) -> Slider<'_> {
    move |slider| units(suffix, 0)(slider.logarithmic(true))
}

fn color_row(ui: &mut Ui, text: &str, help: &str, value: &mut Color, default: Color) {
    color_row_with(ui, text, help, value, default, Alpha::BlendOrAdditive);
}

/// A colour row without the picker's "Additive" blending, for colours that only tint or light
/// things (backgrounds, lights, GPU swarms) where adding light has no meaning.
fn plain_color_row(ui: &mut Ui, text: &str, help: &str, value: &mut Color, default: Color) {
    color_row_with(ui, text, help, value, default, Alpha::OnlyBlend);
}

fn color_row_with(ui: &mut Ui, text: &str, help: &str, value: &mut Color, default: Color, alpha: Alpha) {
    let name = label(ui, text, help);
    // The right-click menu goes on a wrapper: on the button itself it would share the colour
    // picker's popup and close it as soon as it opens.
    let swatch = ui.scope(|ui| hint(color_button(ui, value, alpha), help));
    if reset_clicked(&swatch.inner) {
        // The click also opened or closed the colour picker. A reset should leave it shut.
        Popup::close_all(ui.ctx());
        *value = default;
    }
    resettable(swatch.response, value, default);
    resettable(name, value, default);
    ui.end_row();
}

fn color_button(ui: &mut Ui, value: &mut Color, alpha: Alpha) -> Response {
    let mut c = picker_color(*value);
    let response = egui::color_picker::color_edit_button_srgba(ui, &mut c, alpha);
    if response.changed() {
        *value = scene_color(c);
    }
    response
}

/// egui keeps an additive colour as premultiplied with no alpha. Built unmultiplied, no alpha
/// turns it into plain transparent black, so "Additive" would undo itself on the next frame.
fn picker_color(c: Color) -> Color32 {
    if c.is_additive() {
        Color32::from_rgba_premultiplied(c.r, c.g, c.b, 0)
    } else {
        Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
    }
}

/// The picker's colour as a scene colour. An additive one comes back as its colour with no
/// alpha, which is how `Color` keeps it.
fn scene_color(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r, g, b, a }
}

fn color_list(ui: &mut Ui, colors: &mut Vec<Color>, add_label: &str) {
    color_list_with(ui, colors, add_label, Alpha::BlendOrAdditive);
}

/// A colour list without the picker's "Additive" blending (see [`plain_color_row`]).
fn plain_color_list(ui: &mut Ui, colors: &mut Vec<Color>, add_label: &str) {
    color_list_with(ui, colors, add_label, Alpha::OnlyBlend);
}

fn color_list_with(ui: &mut Ui, colors: &mut Vec<Color>, add_label: &str, alpha: Alpha) {
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        for (i, c) in colors.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                let response = ui
                    .scope(|ui| color_button(ui, c, alpha).on_hover_text("Click to change. Right-click to remove."))
                    .response;
                response.context_menu(|ui| {
                    if ui.button("Remove colour").clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            });
        }
        if let Some(i) = remove {
            colors.remove(i);
        }
        if ui.button(add_label).on_hover_text("Add a colour. Right-click a colour to remove it.").clicked() {
            colors.push(colors.last().copied().unwrap_or(Color::WHITE));
        }
    });
}

/// Position as two plain number boxes. Resetting puts it back in the middle.
fn position_row(ui: &mut Ui, text: &str, help: &str, value: &mut Vec2) {
    let name = label(ui, text, help);
    let boxes = ui.horizontal(|ui| {
        let x = ui.add(DragValue::new(&mut value.x).speed(1.0).prefix("across ").suffix(" px")).on_hover_text(help);
        let y = ui.add(DragValue::new(&mut value.y).speed(1.0).prefix("down ").suffix(" px")).on_hover_text(help);
        (x, y)
    });
    let (x, y) = boxes.inner;
    resettable(name, value, Vec2::ZERO);
    resettable(x, value, Vec2::ZERO);
    resettable(y, value, Vec2::ZERO);
    ui.end_row();
}

/// Launch speed and direction, edited as two sliders over a velocity vector. Each resets on
/// its own and keeps the other.
fn launch_rows(ui: &mut Ui, velocity: &mut Vec2, default: Vec2) {
    let default_speed = default.length();
    let default_angle = default.y.atan2(default.x).to_degrees();
    let mut speed = velocity.length();
    // A ball at rest has no direction, so show the default one.
    let mut angle = if speed > 0.0 { velocity.y.atan2(velocity.x).to_degrees() } else { default_angle };
    let before = (speed, angle);
    let speed_response = slider_row(
        ui,
        "Launch speed",
        "How fast the balls are thrown at the start, in pixels per second. You can also drag the arrow tip in the preview.",
        &mut speed,
        0.0..=3000.0,
        units(" px/s", 0),
    );
    let angle_response = slider_row(
        ui,
        "Direction",
        "Which way the balls are thrown. 0° is right, 90° is straight down, -90° is straight up.",
        &mut angle,
        -180.0..=180.0,
        units("°", 0),
    );
    let speed_response = resettable(speed_response, &mut speed, default_speed);
    let angle_response = resettable(angle_response, &mut angle, default_angle);
    // Only on a real edit: the sliders round the numbers they show, which would nudge the velocity.
    if speed_response.changed() || angle_response.changed() || (speed, angle) != before {
        let radians = angle.to_radians();
        *velocity = Vec2::new(radians.cos(), radians.sin()) * speed;
    }
}

// ---------- tabs ----------

fn new_grid() -> Spawner {
    Spawner::Grid {
        center: Vec2::ZERO,
        columns: 10,
        rows: 10,
        spacing: 20.0,
        radius: 5.0,
        velocity: Vec2::new(300.0, 0.0),
        gradient: vec![Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0xb8, 0x2e)],
        depth: 0.0,
        depth_velocity: 0.0,
        style: Some(BallStyle::default()),
    }
}

fn new_ball() -> Spawner {
    Spawner::Single {
        position: Vec2::ZERO,
        velocity: Vec2::new(300.0, -200.0),
        radius: 20.0,
        color: Color::rgb(0xff, 0x5c, 0xa8),
        depth: 0.0,
        depth_velocity: 0.0,
        style: Some(BallStyle::default()),
    }
}

fn new_stream() -> Spawner {
    Spawner::Stream {
        position: Vec2::new(0.0, -400.0),
        velocity: Vec2::ZERO,
        radius: 10.0,
        count: 50,
        interval: 0.2,
        jitter: 20.0,
        gradient: vec![Color::rgb(0x2e, 0xd8, 0xe6), Color::rgb(0xff, 0x3e, 0xc8)],
        depth: 0.0,
        depth_velocity: 0.0,
        style: Some(BallStyle::default()),
    }
}

fn balls(
    ui: &mut Ui,
    spawners: &mut Vec<Spawner>,
    selected: &mut Option<usize>,
    looks: &mut ball_look::State,
    look: &Look,
    roll: &mut Option<Roll>,
    space: bool,
) -> Option<ListEdit> {
    if spawners.is_empty() {
        ui.label(RichText::new("There are no balls yet. Add some below.").color(theme::MUTED));
    }
    looks.sync(*selected, spawners.len());
    if looks.picked.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} groups picked: a change to one's look is made to all of them.",
                    looks.picked.len()
                ))
                .color(theme::ACCENT_TEXT),
            );
            if ui.small_button("Unpick").clicked() {
                looks.picked.clear();
                *selected = None;
            }
        });
        ui.add_space(6.0);
    } else if spawners.len() > 1 {
        ui.label(
            RichText::new("Tip: Ctrl+click or Shift+click cards to change several balls' looks at once.")
                .small()
                .color(theme::MUTED),
        );
        ui.add_space(6.0);
    }
    let styles_before: Vec<Option<BallStyle>> = spawners.iter().map(|s| s.style().cloned()).collect();
    let mut remove = None;
    let mut duplicate = None;
    for (index, spawner) in spawners.iter_mut().enumerate() {
        let title = match spawner {
            Spawner::Grid { columns, rows, .. } => format!("Block of balls · {columns} × {rows}"),
            Spawner::Single { .. } => "Single ball".to_string(),
            Spawner::Stream { count, .. } => format!("Stream of balls · {count}"),
        };
        let is_selected = *selected == Some(index) || looks.picked.contains(&index);
        ui.push_id(index, |ui| {
            let frame = egui::Frame::new()
                .fill(theme::CARD)
                .stroke(egui::Stroke::new(if is_selected { 1.5 } else { 1.0 }, if is_selected { theme::ACCENT } else { theme::LINE }))
                .corner_radius(8.0)
                .inner_margin(10.0);
            let card = frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).strong().color(theme::TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").on_hover_text("Delete these balls").clicked() {
                            remove = Some(index);
                        }
                        if ui.small_button("Duplicate").on_hover_text("Add a copy of these balls").clicked() {
                            duplicate = Some(index);
                        }
                        if dice_button(ui, "Roll random settings for these balls").clicked() {
                            *roll = Some(Roll::Group(Group::Balls(index)));
                        }
                    });
                });
                ui.add_space(4.0);
                match spawner {
                    Spawner::Grid { center, columns, rows, spacing, radius, velocity, gradient, .. } => {
                        grid(ui, "grid", |ui| {
                            slider(ui, "Balls across", "How many columns of balls the block has.", columns, 10, 1..=100, |s| s);
                            slider(ui, "Balls down", "How many rows of balls the block has.", rows, 10, 1..=100, |s| s);
                            slider(
                                ui,
                                "Ball size",
                                "Radius of each ball in pixels. The video is 1080 pixels wide.",
                                radius,
                                5.0,
                                0.5..=60.0,
                                units(" px", 1),
                            );
                            slider(
                                ui,
                                "Gap",
                                "Distance from one ball's centre to the next.",
                                spacing,
                                20.0,
                                1.0..=200.0,
                                units(" px", 1),
                            );
                            launch_rows(ui, velocity, Vec2::new(300.0, 0.0));
                            label(ui, "Colours", "Colours from the top row of the block to the bottom row, blended evenly.");
                            color_list(ui, gradient, "+");
                            ui.end_row();
                        });
                        more(ui, "grid-more", |ui| {
                            grid(ui, "grid-more-grid", |ui| {
                                position_row(
                                    ui,
                                    "Start position",
                                    "Where the middle of the block starts, measured from the centre of the video. You can also drag the block in the preview.",
                                    center,
                                );
                            });
                        });
                    }
                    Spawner::Single { position, velocity, radius, color, .. } => {
                        grid(ui, "single", |ui| {
                            slider(
                                ui,
                                "Ball size",
                                "Radius of the ball in pixels. The video is 1080 pixels wide.",
                                radius,
                                20.0,
                                0.5..=400.0,
                                units(" px", 1),
                            );
                            launch_rows(ui, velocity, Vec2::new(300.0, -200.0));
                            color_row(ui, "Colour", "", color, Color::rgb(0xff, 0x5c, 0xa8));
                        });
                        more(ui, "single-more", |ui| {
                            grid(ui, "single-more-grid", |ui| {
                                position_row(
                                    ui,
                                    "Start position",
                                    "Where the ball starts, measured from the centre of the video. You can also drag the ball in the preview.",
                                    position,
                                );
                            });
                        });
                    }
                    Spawner::Stream { position, velocity, radius, count, interval, jitter, gradient, .. } => {
                        grid(ui, "stream", |ui| {
                            slider(ui, "How many", "How many balls are dropped in total.", count, 50, 1..=2000, log_units(""));
                            slider(
                                ui,
                                "Time between",
                                "Seconds between one ball and the next.",
                                interval,
                                0.2,
                                0.0..=5.0,
                                units(" s", 2),
                            );
                            slider(
                                ui,
                                "Ball size",
                                "Radius of each ball in pixels. The video is 1080 pixels wide.",
                                radius,
                                10.0,
                                0.5..=200.0,
                                units(" px", 1),
                            );
                            slider(
                                ui,
                                "Wobble",
                                "Each ball starts up to this far left or right of the drop point, so they don't all fall the same way.",
                                jitter,
                                20.0,
                                0.0..=500.0,
                                units(" px", 0),
                            );
                            launch_rows(ui, velocity, Vec2::ZERO);
                            label(ui, "Colours", "Colours from the first ball to the last, blended evenly.");
                            color_list(ui, gradient, "+");
                            ui.end_row();
                        });
                        more(ui, "stream-more", |ui| {
                            grid(ui, "stream-more-grid", |ui| {
                                position_row(
                                    ui,
                                    "Drop point",
                                    "Where the balls appear, measured from the centre of the video. You can also drag it in the preview.",
                                    position,
                                );
                            });
                        });
                    }
                }
                if space {
                    view3d::spawner_depth(ui, spawner); // 3D (WS4)
                }
                ball_look::section(ui, index, spawner.style_mut(), look, looks);
            });
            // Watch for a click anywhere on the card without claiming it, so the sliders,
            // colour buttons and menus inside still get their clicks.
            if card.response.contains_pointer() && ui.input(|i| i.pointer.primary_clicked()) {
                looks.click(ui, index, selected);
            }
        });
        ui.add_space(8.0);
    }
    looks.share_changes(&styles_before, spawners);
    let mut edit = None;
    if let Some(index) = duplicate {
        let copy = spawners[index].clone();
        spawners.insert(index + 1, copy);
        *selected = Some(index + 1);
        looks.picked.clear();
        edit = Some(ListEdit::Duplicated(index));
    }
    if let Some(index) = remove {
        spawners.remove(index);
        *selected = None;
        looks.picked.clear();
        edit = Some(ListEdit::Removed(index));
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Add a block of balls").clicked() {
            spawners.push(new_grid());
            *selected = Some(spawners.len() - 1);
        }
        if ui.button("+ Add one ball").clicked() {
            spawners.push(new_ball());
            *selected = Some(spawners.len() - 1);
        }
        if ui.button("+ Add a stream").on_hover_text("Balls dropped one after another, like a Plinko board").clicked() {
            spawners.push(new_stream());
            *selected = Some(spawners.len() - 1);
        }
    });
    edit
}

/// A change to the order of a list whose items can have animated settings, so their
/// keyframes can follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListEdit {
    Removed(usize),
    /// A copy was put just after this item.
    Duplicated(usize),
    Swapped(usize, usize),
}

impl ListEdit {
    pub fn apply(self, scene: &mut Scene, list: List) {
        match self {
            ListEdit::Removed(i) => scene.removed_item(list, i),
            ListEdit::Duplicated(i) => scene.duplicated_item(list, i),
            ListEdit::Swapped(a, b) => scene.swapped_items(list, a, b),
        }
    }
}

const RING_RADIUS: f32 = 508.0;
const BOX_SIZE: Vec2 = Vec2::new(1000.0, 1000.0);
const POLYGON_RADIUS: f32 = 480.0;
const LINE_WIDTH: f32 = 3.0;
pub(super) const LINE_COLOR: Color = Color::rgb(0xd8, 0xdc, 0xd0);

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Ring,
    Box,
    Polygon,
    Rings,
    Custom,
}

impl Shape {
    fn of(container: &Container) -> Self {
        match container {
            Container::Circle { .. } => Shape::Ring,
            Container::Rect { .. } => Shape::Box,
            Container::Polygon { .. } => Shape::Polygon,
            Container::Rings { .. } => Shape::Rings,
            Container::Custom { .. } => Shape::Custom,
        }
    }

    /// A fresh arena of this shape that keeps the old one's position, line width and colour.
    fn make(self, old: &Container) -> Container {
        let (center, thickness, color) = (old.center(), old.thickness(), old.color());
        match self {
            Shape::Ring => Container::Circle { center, radius: RING_RADIUS, thickness, color },
            Shape::Box => Container::Rect { center, size: BOX_SIZE, thickness, color },
            Shape::Polygon => {
                Container::Polygon { center, radius: POLYGON_RADIUS, sides: 6, rotation: 0.0, thickness, color }
            }
            Shape::Rings => match Scene::escape_rings().container {
                Container::Rings {
                    count,
                    inner_radius,
                    spacing,
                    sides,
                    gap,
                    spin,
                    spin_step,
                    gap_offset,
                    gradient,
                    ..
                } => Container::Rings {
                    center,
                    count,
                    inner_radius,
                    spacing,
                    sides,
                    gap,
                    spin,
                    spin_step,
                    gap_offset,
                    thickness: thickness.max(4.0),
                    gradient,
                },
                other => other,
            },
            // Needs the scene's shapes: see `shapes::make_custom`.
            Shape::Custom => old.clone(),
        }
    }
}

/// An arena of this shape in the middle of the video at its usual size.
fn default_container(shape: Shape) -> Container {
    let ring = Container::Circle { center: Vec2::ZERO, radius: RING_RADIUS, thickness: LINE_WIDTH, color: LINE_COLOR };
    match shape {
        Shape::Ring => ring,
        Shape::Rings => Scene::escape_rings().container,
        _ => shape.make(&ring),
    }
}

fn arena(
    ui: &mut Ui,
    container: &mut Container,
    motion: &mut ArenaMotion,
    physics: &mut Physics,
    custom_shapes: &mut Vec<bpe_scene::Shape>,
    roll: &mut Option<Roll>,
    flat: bool,
) {
    // 3D (WS4): the flat shape only applies to a flat scene.
    if flat {
        arena_shape(ui, container, custom_shapes, roll);
    }
    arena_motion_and_world(ui, container, motion, physics, roll, flat);
}

fn arena_shape(
    ui: &mut Ui,
    container: &mut Container,
    custom_shapes: &mut Vec<bpe_scene::Shape>,
    roll: &mut Option<Roll>,
) {
    heading_dice(ui, "Shape", Group::Shape, roll);
    let current = Shape::of(container);
    let choices = [
        (Shape::Ring, "Ring", "A round arena."),
        (Shape::Box, "Box", "A rectangle."),
        (Shape::Polygon, "Polygon", "A triangle, square, hexagon or any shape with equal sides."),
        (
            Shape::Rings,
            "Escape rings",
            "Rings inside each other, each with a gap. A ring shatters when a ball gets out through its gap.",
        ),
        (Shape::Custom, "Custom", "Any outline: draw it on the preview, import an SVG drawing or trace a picture."),
    ];
    ui.columns(choices.len(), |columns| {
        for (ui, (shape, name, help)) in columns.iter_mut().zip(choices) {
            let button = egui::Button::selectable(current == shape, name);
            if ui.add_sized([ui.available_width(), 30.0], button).on_hover_text(help).clicked() && current != shape {
                *container = match shape {
                    Shape::Custom => shapes::make_custom(container, custom_shapes),
                    _ => shape.make(container),
                };
            }
        }
    });
    ui.add_space(6.0);
    shapes::custom_arena(ui, container, custom_shapes);
    grid(ui, "arena", |ui| match container {
        // Shown above, with its Draw and Import buttons.
        Container::Custom { .. } => {}
        Container::Circle { radius, thickness, color, .. } => {
            slider(
                ui,
                "Size",
                "Radius of the ring in pixels. You can also drag the ring's edge in the preview.",
                radius,
                RING_RADIUS,
                20.0..=1200.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Line width",
                "Thickness of the ring's line. 0 hides it.",
                thickness,
                LINE_WIDTH,
                0.0..=40.0,
                units(" px", 1),
            );
            key(ui, Property::ArenaColor);
            color_row(ui, "Line colour", "", color, LINE_COLOR);
        }
        Container::Rect { size, thickness, color, .. } => {
            slider(
                ui,
                "Width",
                "Width of the box in pixels. You can also drag the box's edge in the preview.",
                &mut size.x,
                BOX_SIZE.x,
                40.0..=2400.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Height",
                "Height of the box in pixels. You can also drag the box's edge in the preview.",
                &mut size.y,
                BOX_SIZE.y,
                40.0..=2400.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Line width",
                "Thickness of the box's line. 0 hides it.",
                thickness,
                LINE_WIDTH,
                0.0..=40.0,
                units(" px", 1),
            );
            key(ui, Property::ArenaColor);
            color_row(ui, "Line colour", "", color, LINE_COLOR);
        }
        Container::Polygon { radius, sides, rotation, thickness, color, .. } => {
            slider(ui, "Sides", "3 is a triangle, 4 a square, 6 a hexagon.", sides, 6, 3..=12, |s| s);
            slider(
                ui,
                "Size",
                "Distance from the centre to each corner, in pixels. You can also drag a corner in the preview.",
                radius,
                POLYGON_RADIUS,
                20.0..=1200.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Turn",
                "Turns the shape. At 0° it has a flat side at the bottom.",
                rotation,
                0.0,
                -180.0..=180.0,
                units("°", 0),
            );
            slider(ui, "Line width", "Thickness of the line. 0 hides it.", thickness, 3.0, 0.0..=40.0, units(" px", 1));
            key(ui, Property::ArenaColor);
            color_row(ui, "Line colour", "", color, LINE_COLOR);
        }
        Container::Rings { count, inner_radius, spacing, gap, spin, spin_step, thickness, gradient, .. } => {
            slider(ui, "Rings", "How many rings there are.", count, 10, 1..=60, |s| s);
            slider(
                ui,
                "Smallest ring",
                "Radius of the smallest ring, where the balls start. You can also drag it in the preview.",
                inner_radius,
                110.0,
                10.0..=1000.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Ring spacing",
                "Distance from one ring to the next.",
                spacing,
                38.0,
                4.0..=300.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Gap size",
                "How wide each ring's gap is. Bigger gaps are easier to escape through.",
                gap,
                50.0,
                0.0..=180.0,
                units("°", 0),
            );
            slider(
                ui,
                "Spin",
                "How fast the smallest ring turns, in degrees per second. Negative turns the other way.",
                spin,
                70.0,
                -360.0..=360.0,
                units("°/s", 0),
            );
            slider(
                ui,
                "Spin change",
                "Added to the spin of each ring further out, so the gaps drift apart.",
                spin_step,
                9.0,
                -90.0..=90.0,
                units("°/s", 1),
            );
            slider(ui, "Line width", "Thickness of each ring.", thickness, 6.0, 0.0..=40.0, units(" px", 1));
            label(ui, "Colours", "Colours from the smallest ring to the biggest, blended evenly.");
            color_list(ui, gradient, "+");
            ui.end_row();
        }
    });
    if let Container::Rings { sides, gap_offset, .. } = container {
        more(ui, "rings-more", |ui| {
            grid(ui, "rings-more-grid", |ui| {
                let mut round = *sides < 3;
                label(ui, "Ring shape", "Round rings, or polygons with this many sides.");
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::selectable(round, "Round")).clicked() {
                        round = true;
                        *sides = 0;
                    }
                    if ui.add(egui::Button::selectable(!round, "Polygon")).clicked() && round {
                        round = false;
                        *sides = 6;
                    }
                });
                ui.end_row();
                if !round {
                    slider(ui, "Sides", "3 is a triangle, 4 a square, 6 a hexagon.", sides, 6, 3..=12, |s| s);
                }
                slider(
                    ui,
                    "Gap offset",
                    "How far each ring's gap is turned from the one inside it at the start.",
                    gap_offset,
                    20.0,
                    -180.0..=180.0,
                    units("°", 0),
                );
            });
        });
    }
}

fn arena_motion_and_world(
    ui: &mut Ui,
    container: &mut Container,
    motion: &mut ArenaMotion,
    physics: &mut Physics,
    roll: &mut Option<Roll>,
    flat: bool,
) {
    let d = ArenaMotion::default();
    heading_dice(ui, "Movement", Group::Movement, roll);
    grid(ui, "motion", |ui| {
        key(ui, Property::ArenaSize);
        slider(
            ui,
            "Size over time",
            "Grows or shrinks the whole arena, 100% being the size set above. Click the diamond to animate it, so the arena pulses or closes in.",
            &mut motion.size,
            d.size,
            0.05..=3.0,
            percent,
        );
        key(ui, Property::ArenaSpin);
        slider(
            ui,
            "Arena spin",
            "Turns the whole arena, in degrees per second. Negative turns it the other way.",
            &mut motion.spin,
            d.spin,
            -360.0..=360.0,
            units("°/s", 0),
        );
        key(ui, Property::ShrinkSpeed);
        slider(
            ui,
            "Shrink",
            "Pixels per second the arena closes in. Negative values make it grow.",
            &mut motion.shrink_speed,
            d.shrink_speed,
            -100.0..=100.0,
            units(" px/s", 1),
        );
        slider(
            ui,
            "Shrink per hit",
            "Pixels the arena closes in every time a ball hits it.",
            &mut motion.shrink_per_hit,
            d.shrink_per_hit,
            0.0..=50.0,
            units(" px", 1),
        );
        if motion.shrink_speed > 0.0 || motion.shrink_per_hit > 0.0 {
            slider(
                ui,
                "Smallest size",
                "The arena stops shrinking at this radius.",
                &mut motion.min_size,
                d.min_size,
                1.0..=1000.0,
                log_units(" px"),
            );
        }
    });
    let center = container.center_mut();

    let defaults = Physics::default();
    heading_dice(ui, "World", Group::World, roll);
    grid(ui, "world", |ui| {
        key(ui, Property::Gravity);
        slider(
            ui,
            "Gravity",
            "How hard balls are pulled down, in pixels per second squared. 0 means they fly in straight lines.",
            &mut physics.gravity.y,
            defaults.gravity.y,
            -3000.0..=3000.0,
            units("", 0),
        );
        key(ui, Property::Bounciness);
        slider(
            ui,
            "Bounciness",
            "How much speed a ball keeps when it hits the wall. 100% keeps all of it. Lower values make balls settle, higher values make them gain energy.",
            &mut physics.restitution,
            defaults.restitution,
            0.0..=1.5,
            percent,
        );
        key(ui, Property::AirDrag);
        slider(
            ui,
            "Air drag",
            "Slows balls down over time, as if moving through air. 0 means no slowing.",
            &mut physics.air_drag,
            defaults.air_drag,
            0.0..=3.0,
            units("", 2),
        );
    });
    // "Balls bump into each other" is in the Bounces tab (see `soft::bounces_section`).
    more(ui, "arena-more", |ui| {
        grid(ui, "arena-more-grid", |ui| {
            key(ui, Property::SidewaysGravity);
            slider(
                ui,
                "Sideways gravity",
                "Pulls balls left (negative) or right (positive).",
                &mut physics.gravity.x,
                defaults.gravity.x,
                -3000.0..=3000.0,
                units("", 0),
            );
            slider(
                ui,
                "Simulation precision",
                "Physics steps per second. Higher is more accurate for very fast balls but slower. 240 is plenty for most scenes.",
                &mut physics.steps_per_second,
                defaults.steps_per_second,
                60..=2000,
                log_units(" steps/s"),
            );
            // 3D (WS4): the 3D container is always in the middle.
            if flat {
                position_row(
                    ui,
                    "Arena position",
                    "Where the centre of the arena is, measured from the centre of the video.",
                    center,
                );
            }
        });
    });
}

fn obstacles(ui: &mut Ui, obstacles: &mut Vec<Obstacle>, custom_shapes: &mut Vec<bpe_scene::Shape>) {
    let peg = Color::rgb(0xc8, 0xd8, 0xff);
    let line = Color::rgb(0xd8, 0xdc, 0xd0);
    heading(ui, "Pegs and walls");
    if obstacles.is_empty() {
        ui.label(RichText::new("Pegs and walls inside the arena that balls bounce off.").color(theme::MUTED));
    }
    let mut remove = None;
    for (index, obstacle) in obstacles.iter_mut().enumerate() {
        let title = match obstacle {
            Obstacle::Peg { .. } => "Peg".to_string(),
            Obstacle::PegGrid { columns, rows, .. } => format!("Pegs · {columns} × {rows}"),
            Obstacle::Line { .. } => "Wall".to_string(),
            Obstacle::Shape { shape, .. } => format!("Shape · {shape}"),
        };
        ui.push_id(("obstacle", index), |ui| {
            egui::Frame::new()
                .fill(theme::CARD)
                .stroke(egui::Stroke::new(1.0, theme::LINE))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong().color(theme::TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                    grid(ui, "obstacle", |ui| match obstacle {
                        Obstacle::Peg { position, radius, color } => {
                            slider(
                                ui,
                                "Size",
                                "Radius of the peg in pixels.",
                                radius,
                                12.0,
                                1.0..=300.0,
                                units(" px", 1),
                            );
                            position_row(
                                ui,
                                "Position",
                                "Where the peg is, measured from the centre of the video.",
                                position,
                            );
                            color_row(ui, "Colour", "", color, peg);
                        }
                        Obstacle::PegGrid { center, columns, rows, spacing, radius, stagger, color } => {
                            slider(ui, "Pegs across", "How many pegs in each row.", columns, 11, 1..=60, |s| s);
                            slider(ui, "Rows", "How many rows of pegs.", rows, 12, 1..=60, |s| s);
                            slider(
                                ui,
                                "Peg size",
                                "Radius of each peg in pixels.",
                                radius,
                                7.0,
                                1.0..=100.0,
                                units(" px", 1),
                            );
                            slider(
                                ui,
                                "Gap across",
                                "Distance between pegs in a row.",
                                &mut spacing.x,
                                84.0,
                                4.0..=400.0,
                                units(" px", 0),
                            );
                            slider(
                                ui,
                                "Gap down",
                                "Distance between rows.",
                                &mut spacing.y,
                                76.0,
                                4.0..=400.0,
                                units(" px", 0),
                            );
                            label(ui, "Zigzag", "Shift every other row by half a gap, like a Plinko board.");
                            ui.checkbox(stagger, "Shift every other row");
                            ui.end_row();
                            position_row(
                                ui,
                                "Position",
                                "Where the middle of the pegs is, measured from the centre of the video.",
                                center,
                            );
                            color_row(ui, "Colour", "", color, peg);
                        }
                        Obstacle::Line { from, to, thickness, color } => {
                            position_row(
                                ui,
                                "From",
                                "One end of the wall, measured from the centre of the video.",
                                from,
                            );
                            position_row(ui, "To", "The other end of the wall.", to);
                            slider(ui, "Width", "Thickness of the wall.", thickness, 4.0, 0.5..=60.0, units(" px", 1));
                            color_row(ui, "Colour", "", color, line);
                        }
                        Obstacle::Shape { shape, position, rotation, scale, thickness, color } => {
                            shapes::obstacle_rows(
                                ui,
                                shape,
                                custom_shapes,
                                position,
                                rotation,
                                scale,
                                thickness,
                                color,
                            );
                        }
                    });
                });
        });
        ui.add_space(8.0);
    }
    if let Some(index) = remove {
        obstacles.remove(index);
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Add pegs").on_hover_text("A board of pegs, like Plinko").clicked() {
            obstacles.push(Obstacle::PegGrid {
                center: Vec2::ZERO,
                columns: 11,
                rows: 12,
                spacing: Vec2::new(84.0, 76.0),
                radius: 7.0,
                stagger: true,
                color: peg,
            });
        }
        if ui.button("+ Add one peg").clicked() {
            obstacles.push(Obstacle::Peg { position: Vec2::ZERO, radius: 12.0, color: peg });
        }
        if ui.button("+ Add a wall").on_hover_text("A straight wall between two points").clicked() {
            obstacles.push(Obstacle::Line {
                from: Vec2::new(-150.0, 0.0),
                to: Vec2::new(150.0, 0.0),
                thickness: 4.0,
                color: line,
            });
        }
        shapes::add_obstacle_buttons(ui, obstacles, custom_shapes);
    });
}

fn bounces(ui: &mut Ui, rules: &mut BounceRules, roll: &mut Option<Roll>) {
    let defaults = BounceRules::default();
    heading_dice(ui, "Each bounce", Group::Bounces, roll);
    ui.label(RichText::new("What happens to a ball each time it hits the wall.").color(theme::MUTED));
    ui.add_space(6.0);
    grid(ui, "rules", |ui| {
        let mut gain = (rules.speed_gain - 1.0) * 100.0;
        key(ui, Property::SpeedGain);
        let response = slider_row(
            ui,
            "Speed up",
            "Each bounce makes the ball this much faster. 0% keeps its speed the same, negative values slow it down.",
            &mut gain,
            -50.0..=100.0,
            |s| {
                s.custom_formatter(|v, _| format!("{v:+.1}%"))
                    .custom_parser(|s| s.trim().trim_end_matches('%').parse().ok())
            },
        );
        if response.changed() {
            rules.speed_gain = 1.0 + gain / 100.0;
        }
        resettable(response, &mut rules.speed_gain, defaults.speed_gain);
        label(
            ui,
            "Speed limit",
            "Turn this off to let balls keep speeding up with no top speed. Very fast balls still bounce off every wall and peg.",
        );
        ui.checkbox(&mut rules.limit_speed, "Cap the top speed");
        ui.end_row();
        if rules.limit_speed {
            slider(
                ui,
                "Top speed",
                "Balls never go faster than this, so speeding up can't run away.",
                &mut rules.max_speed,
                defaults.max_speed,
                10.0..=20000.0,
                log_units(" px/s"),
            );
        }
        slider(
            ui,
            "Grow",
            "Pixels added to the ball's size on every bounce. Negative values shrink it. Try the Growing ball preset.",
            &mut rules.grow,
            defaults.grow,
            -20.0..=20.0,
            |s| {
                s.custom_formatter(|v, _| format!("{v:+.1} px"))
                    .custom_parser(|s| s.trim().trim_end_matches("px").trim().parse().ok())
            },
        );
        slider(
            ui,
            "Biggest size",
            "Balls stop growing at this radius.",
            &mut rules.max_radius,
            defaults.max_radius,
            1.0..=1000.0,
            log_units(" px"),
        );
        label(
            ui,
            "Change colour",
            "Balls step through these colours, one per bounce. Leave it empty to keep each ball's own colour.",
        );
        let add_label = if rules.recolor.is_empty() { "+ Pick colours" } else { "+" };
        color_list(ui, &mut rules.recolor, add_label);
        ui.end_row();
    });
    heading_dice(ui, "New balls", Group::NewBalls, roll);
    grid(ui, "spawn", |ui| {
        slider(
            ui,
            "Per hit",
            "New balls that split off a ball every time it hits the wall or a peg. Try the Multiplying balls preset.",
            &mut rules.spawn_per_hit,
            defaults.spawn_per_hit,
            0..=5,
            |s| s,
        );
        slider(
            ui,
            "Per escape",
            "New balls that appear in the middle every time a ball escapes a ring. Only for escape rings.",
            &mut rules.spawn_per_escape,
            defaults.spawn_per_escape,
            0..=10,
            |s| s,
        );
        if rules.spawn_per_hit > 0 || rules.spawn_per_escape > 0 {
            slider(
                ui,
                "Most balls",
                "No new balls are added once there are this many.",
                &mut rules.max_balls,
                defaults.max_balls,
                1..=5000,
                log_units(""),
            );
            slider(
                ui,
                "Spread",
                "New balls fly off up to this many degrees either side of the ball they came from.",
                &mut rules.spawn_spread,
                defaults.spawn_spread,
                0.0..=180.0,
                units("°", 0),
            );
        }
    });
}

fn look(ui: &mut Ui, look: &mut Look, roll: &mut Option<Roll>) {
    let d = Look::default();
    heading_dice(ui, "Background", Group::Background, roll);
    grid(ui, "background", |ui| {
        key(ui, Property::BackgroundTop);
        plain_color_row(
            ui,
            "Top colour",
            "The background fades from this colour at the top...",
            &mut look.background_top,
            d.background_top,
        );
        key(ui, Property::BackgroundBottom);
        plain_color_row(
            ui,
            "Bottom colour",
            "...to this colour at the bottom.",
            &mut look.background_bottom,
            d.background_bottom,
        );
    });
    heading_dice(ui, "Trails", Group::Trails, roll);
    grid(ui, "trails", |ui| {
        slider(
            ui,
            "Length",
            "How far back in time each ball's trail reaches. 0 turns trails off. Only for balls with the classic look: balls with their own look set their trail in the Balls tab.",
            &mut look.trail_seconds,
            d.trail_seconds,
            0.0..=2.0,
            units(" s", 2),
        );
        key(ui, Property::TrailWidth);
        slider(
            ui,
            "Width",
            "Trail width compared to the ball.",
            &mut look.trail_width,
            d.trail_width,
            0.0..=2.0,
            percent,
        );
        key(ui, Property::TrailOpacity);
        slider(
            ui,
            "Brightness",
            "How bright the trail is.",
            &mut look.trail_opacity,
            d.trail_opacity,
            0.0..=1.0,
            percent,
        );
    });
    heading_dice(ui, "Glow", Group::Glow, roll);
    grid(ui, "glow", |ui| {
        key(ui, Property::BallGlow);
        slider(
            ui,
            "Ball glow",
            "The soft halo around each ball with the classic look. Balls with their own look set their glow in the Balls tab.",
            &mut look.ball_glow,
            d.ball_glow,
            0.0..=2.0,
            percent,
        );
        key(ui, Property::FlashStrength);
        slider(
            ui,
            "Wall flash",
            "A flash of light where a ball hits the wall.",
            &mut look.flash_strength,
            d.flash_strength,
            0.0..=5.0,
            percent,
        );
        key(ui, Property::Bloom);
        slider(
            ui,
            "Overall glow",
            "How much light bleeds around everything bright.",
            &mut look.bloom_strength,
            d.bloom_strength,
            0.0..=3.0,
            percent,
        );
    });
    heading_dice(ui, "String art", Group::Strings, roll);
    grid(ui, "strings", |ui| {
        slider(
            ui,
            "Lines per ball",
            "Draws a line from each ball to the places it hit, up to this many of its latest hits. 0 turns it off. Try the String art preset.",
            &mut look.string_lines,
            d.string_lines,
            0..=2000,
            |s| s,
        );
        if look.string_lines > 0 {
            key(ui, Property::StringOpacity);
            slider(
                ui,
                "Line brightness",
                "How bright each line is.",
                &mut look.string_opacity,
                d.string_opacity,
                0.0..=1.0,
                percent,
            );
            key(ui, Property::StringWidth);
            slider(
                ui,
                "Line width",
                "Width of each line in pixels.",
                &mut look.string_width,
                d.string_width,
                0.2..=10.0,
                units(" px", 1),
            );
        }
    });
    more(ui, "look-more", |ui| {
        grid(ui, "look-more-grid", |ui| {
            slider(
                ui,
                "Shatter pieces",
                "How many pieces an escape ring breaks into. 0 makes rings vanish without pieces.",
                &mut look.shatter_pieces,
                d.shatter_pieces,
                0..=200,
                |s| s,
            );
            slider(
                ui,
                "Flash length",
                "How long a wall flash lasts.",
                &mut look.flash_seconds,
                d.flash_seconds,
                0.0..=2.0,
                units(" s", 2),
            );
            key(ui, Property::FlashSize);
            slider(
                ui,
                "Flash size",
                "How big a wall flash is.",
                &mut look.flash_size,
                d.flash_size,
                1.0..=300.0,
                units(" px", 0),
            );
            key(ui, Property::Exposure);
            slider(
                ui,
                "Brightness",
                "Overall brightness of the picture.",
                &mut look.exposure,
                d.exposure,
                0.1..=4.0,
                percent,
            );
        });
    });
}

pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[note as usize % 12], note as i32 / 12 - 1)
}

/// Reads a typed note such as "C4", "F#3" or a plain MIDI number.
fn parse_note(text: &str) -> Option<f64> {
    let text = text.trim();
    if let Ok(number) = text.parse::<f64>() {
        return Some(number);
    }
    let (name, octave) = text.split_at(text.find(|c: char| c.is_ascii_digit() || c == '-')?);
    let (letter, accidental) = name.split_at(name.chars().next()?.len_utf8());
    let class = ["C", "D", "E", "F", "G", "A", "B"].iter().position(|l| l.eq_ignore_ascii_case(letter))?;
    let class = [0, 2, 4, 5, 7, 9, 11][class]
        + match accidental {
            "" => 0,
            "#" => 1,
            "b" => -1,
            _ => return None,
        };
    let octave: i32 = octave.parse().ok()?;
    Some(((octave + 1) * 12 + class) as f64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn additive_colours_survive_the_colour_picker() {
        use super::{picker_color, scene_color};
        use bpe_scene::Color;
        use eframe::egui::{Color32, ecolor::Hsva};
        let glow = Color::additive(0x20, 0xff, 0x40);
        // The picker works in HSVA: an additive colour has a negative alpha there.
        let hsva = Hsva::from(picker_color(glow));
        assert!(hsva.a < 0.0, "the picker sees the colour as additive");
        assert_eq!(scene_color(Color32::from(hsva)), glow);
        // Picking "Additive" on a normal colour keeps its colour.
        let normal = Color::rgb(0x20, 0xff, 0x40);
        let picked = Hsva { a: -Hsva::from(picker_color(normal)).a, ..Hsva::from(picker_color(normal)) };
        assert_eq!(scene_color(Color32::from(picked)), glow);
        for c in [normal, Color::WHITE, Color { r: 0, g: 0, b: 0, a: 0 }] {
            assert_eq!(scene_color(picker_color(c)), c);
        }
    }

    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(parse_note("C4"), Some(60.0));
        assert_eq!(parse_note("A4"), Some(69.0));
        assert_eq!(parse_note("72"), Some(72.0));
    }
}
