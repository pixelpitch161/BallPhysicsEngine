//! The scene settings panel, split into tabs. Every widget edits the [`Scene`] in place; the
//! app notices the change and rebuilds the preview and soundtrack.
//!
//! Each setting has a plain name, a "?" with a one-sentence explanation, and a right-click
//! "Reset to default". Settings most people never touch sit under "More settings" in each tab.

use crate::theme;
use bpe_scene::{BounceRules, ChordStep, Color, Container, Instrument, Look, Physics, Scene, Sound, Spawner, Vec2};
use eframe::egui::{
    self, Color32, DragValue, Popup, PopupCloseBehavior, Response, RichText, Sense, Slider, Ui, emath::Numeric,
};
use std::ops::RangeInclusive;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Balls,
    Arena,
    Bounces,
    Look,
    Sound,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Balls, Tab::Arena, Tab::Bounces, Tab::Look, Tab::Sound];

    fn name(self) -> &'static str {
        match self {
            Tab::Balls => "Balls",
            Tab::Arena => "Arena",
            Tab::Bounces => "Bounces",
            Tab::Look => "Look",
            Tab::Sound => "Sound",
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
            Tab::Sound => {
                painter.circle_filled(c + v(-4.0, 5.0), 3.2, color);
                painter.circle_filled(c + v(5.0, 3.0), 3.2, color);
                painter.line_segment([c + v(-1.2, 5.0), c + v(-1.2, -8.0)], stroke);
                painter.line_segment([c + v(7.8, 3.0), c + v(7.8, -10.0)], stroke);
                painter.line_segment([c + v(-1.2, -8.0), c + v(7.8, -10.0)], egui::Stroke::new(2.4, color));
            }
        }
    }
}

/// Panel state that is not part of the scene.
pub struct Inspector {
    pub tab: Tab,
    /// The ball group last clicked in the preview, highlighted in the Balls tab.
    pub selected: Option<usize>,
}

impl Default for Inspector {
    fn default() -> Self {
        Self { tab: Tab::Balls, selected: None }
    }
}

/// Seconds of arrow length per pixel per second of launch speed, shared with the preview handles.
pub const ARROW_SECONDS: f32 = 0.35;

pub fn show(ui: &mut Ui, inspector: &mut Inspector, scene: &mut Scene) {
    tab_bar(ui, &mut inspector.tab);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| match inspector.tab {
            Tab::Balls => balls(ui, &mut scene.spawners, &mut inspector.selected),
            Tab::Arena => arena(ui, &mut scene.container, &mut scene.physics),
            Tab::Bounces => bounces(ui, &mut scene.rules),
            Tab::Look => look(ui, &mut scene.look),
            Tab::Sound => sound(ui, &mut scene.sound),
        });
    });
}

fn tab_bar(ui: &mut Ui, current: &mut Tab) {
    let width = ui.available_width() / Tab::ALL.len() as f32;
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
                egui::FontId::proportional(14.0),
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

fn label(ui: &mut Ui, text: &str, help: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        hint(ui.label(text), help);
        if !help.is_empty() {
            help_icon(ui, help);
        }
    });
}

fn hint(response: Response, help: &str) -> Response {
    if help.is_empty() { response } else { response.on_hover_text(help) }
}

/// Right-click menu that puts a setting back to its default.
fn resettable<T: PartialEq>(response: Response, value: &mut T, default: T) -> Response {
    response.context_menu(|ui| {
        if ui.add_enabled(*value != default, egui::Button::new("Reset to default")).clicked() {
            *value = default;
            ui.close();
        }
    });
    response
}

/// One labelled slider row in a grid, with a right-click reset.
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

/// One labelled slider row in a grid, for values that reset some other way.
fn slider_row<T: Numeric>(
    ui: &mut Ui,
    text: &str,
    help: &str,
    value: &mut T,
    range: RangeInclusive<T>,
    style: impl FnOnce(Slider<'_>) -> Slider<'_>,
) -> Response {
    label(ui, text, help);
    // Fill the rest of the row, leaving room for the number box beside the slider.
    ui.spacing_mut().slider_width = (ui.available_width() - 120.0).max(60.0);
    let response = hint(ui.add(style(Slider::new(value, range))), help);
    ui.end_row();
    response
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
    label(ui, text, help);
    // The right-click menu goes on a wrapper: on the button itself it would share the colour
    // picker's popup and close it as soon as it opens.
    let response = ui.scope(|ui| hint(color_button(ui, value), help)).response;
    resettable(response, value, default);
    ui.end_row();
}

fn color_button(ui: &mut Ui, value: &mut Color) -> Response {
    let mut c = Color32::from_rgba_unmultiplied(value.r, value.g, value.b, value.a);
    let response = ui.color_edit_button_srgba(&mut c);
    if response.changed() {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        *value = Color { r, g, b, a };
    }
    response
}

fn color_list(ui: &mut Ui, colors: &mut Vec<Color>, add_label: &str) {
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        for (i, c) in colors.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                let response = ui
                    .scope(|ui| color_button(ui, c).on_hover_text("Click to change. Right-click to remove."))
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

/// Position as two plain number boxes.
fn position_row(ui: &mut Ui, text: &str, help: &str, value: &mut Vec2) {
    label(ui, text, help);
    ui.horizontal(|ui| {
        ui.add(DragValue::new(&mut value.x).speed(1.0).prefix("across ").suffix(" px")).on_hover_text(help);
        ui.add(DragValue::new(&mut value.y).speed(1.0).prefix("down ").suffix(" px")).on_hover_text(help);
    });
    ui.end_row();
}

/// Launch speed and direction, edited as two sliders over a velocity vector.
fn launch_rows(ui: &mut Ui, velocity: &mut Vec2, default: Vec2) {
    let mut speed = velocity.length();
    let mut angle = if speed > 0.0 { velocity.y.atan2(velocity.x).to_degrees() } else { 0.0 };
    let response = slider_row(
        ui,
        "Launch speed",
        "How fast the balls are thrown at the start, in pixels per second. You can also drag the arrow tip in the preview.",
        &mut speed,
        0.0..=3000.0,
        units(" px/s", 0),
    );
    let speed_response = response;
    let response = slider_row(
        ui,
        "Direction",
        "Which way the balls are thrown. 0° is right, 90° is straight down, -90° is straight up.",
        &mut angle,
        -180.0..=180.0,
        units("°", 0),
    );
    // Only on a real edit: the sliders round the numbers they show, which would nudge the velocity.
    if speed_response.changed() || response.changed() {
        let radians = angle.to_radians();
        *velocity = Vec2::new(radians.cos(), radians.sin()) * speed;
    }
    resettable(speed_response, velocity, default);
    resettable(response, velocity, default);
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
    }
}

fn new_ball() -> Spawner {
    Spawner::Single {
        position: Vec2::ZERO,
        velocity: Vec2::new(300.0, -200.0),
        radius: 20.0,
        color: Color::rgb(0xff, 0x5c, 0xa8),
    }
}

fn balls(ui: &mut Ui, spawners: &mut Vec<Spawner>, selected: &mut Option<usize>) {
    if spawners.is_empty() {
        ui.label(RichText::new("There are no balls yet. Add some below.").color(theme::MUTED));
    }
    let mut remove = None;
    let mut duplicate = None;
    for (index, spawner) in spawners.iter_mut().enumerate() {
        let title = match spawner {
            Spawner::Grid { columns, rows, .. } => format!("Block of balls · {columns} × {rows}"),
            Spawner::Single { .. } => "Single ball".to_string(),
        };
        let is_selected = *selected == Some(index);
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
                    });
                });
                ui.add_space(4.0);
                match spawner {
                    Spawner::Grid { center, columns, rows, spacing, radius, velocity, gradient } => {
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
                    Spawner::Single { position, velocity, radius, color } => {
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
                }
            });
            // Watch for a click anywhere on the card without claiming it, so the sliders,
            // colour buttons and menus inside still get their clicks.
            if card.response.contains_pointer() && ui.input(|i| i.pointer.primary_clicked()) {
                *selected = Some(index);
            }
        });
        ui.add_space(8.0);
    }
    if let Some(index) = duplicate {
        let copy = spawners[index].clone();
        spawners.insert(index + 1, copy);
        *selected = Some(index + 1);
    }
    if let Some(index) = remove {
        spawners.remove(index);
        *selected = None;
    }
    ui.horizontal(|ui| {
        if ui.button("+ Add a block of balls").clicked() {
            spawners.push(new_grid());
            *selected = Some(spawners.len() - 1);
        }
        if ui.button("+ Add one ball").clicked() {
            spawners.push(new_ball());
            *selected = Some(spawners.len() - 1);
        }
    });
}

fn arena(ui: &mut Ui, container: &mut Container, physics: &mut Physics) {
    let line = Color::rgb(0xd8, 0xdc, 0xd0);
    heading(ui, "Shape");
    ui.horizontal(|ui| {
        let is_circle = matches!(container, Container::Circle { .. });
        let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
        if ui.add_sized([width, 30.0], egui::Button::selectable(is_circle, "Ring")).clicked() && !is_circle {
            let (center, thickness, color) = container_basics(container);
            *container = Container::Circle { center, radius: 508.0, thickness, color };
        }
        if ui.add_sized([width, 30.0], egui::Button::selectable(!is_circle, "Box")).clicked() && is_circle {
            let (center, thickness, color) = container_basics(container);
            *container = Container::Rect { center, size: Vec2::new(1000.0, 1000.0), thickness, color };
        }
    });
    ui.add_space(6.0);
    let center = grid_with(ui, "arena", move |ui| match container {
        Container::Circle { center, radius, thickness, color } => {
            slider(
                ui,
                "Size",
                "Radius of the ring in pixels. You can also drag the ring's edge in the preview.",
                radius,
                508.0,
                20.0..=1200.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Line width",
                "Thickness of the ring's line. 0 hides it.",
                thickness,
                3.0,
                0.0..=40.0,
                units(" px", 1),
            );
            color_row(ui, "Line colour", "", color, line);
            center
        }
        Container::Rect { center, size, thickness, color } => {
            slider(
                ui,
                "Width",
                "Width of the box in pixels. You can also drag the box's edge in the preview.",
                &mut size.x,
                1000.0,
                40.0..=2400.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Height",
                "Height of the box in pixels. You can also drag the box's edge in the preview.",
                &mut size.y,
                1000.0,
                40.0..=2400.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Line width",
                "Thickness of the box's line. 0 hides it.",
                thickness,
                3.0,
                0.0..=40.0,
                units(" px", 1),
            );
            color_row(ui, "Line colour", "", color, line);
            center
        }
    });

    let defaults = Physics::default();
    heading(ui, "World");
    grid(ui, "world", |ui| {
        slider(
            ui,
            "Gravity",
            "How hard balls are pulled down, in pixels per second squared. 0 means they fly in straight lines.",
            &mut physics.gravity.y,
            defaults.gravity.y,
            -3000.0..=3000.0,
            units("", 0),
        );
        slider(
            ui,
            "Bounciness",
            "How much speed a ball keeps when it hits the wall. 100% keeps all of it. Lower values make balls settle, higher values make them gain energy.",
            &mut physics.restitution,
            defaults.restitution,
            0.0..=1.5,
            percent,
        );
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
    ui.checkbox(&mut physics.ball_collisions, "Balls bump into each other")
        .on_hover_text("When off, balls pass through one another and only bounce off the wall.");
    more(ui, "arena-more", |ui| {
        grid(ui, "arena-more-grid", |ui| {
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
            position_row(
                ui,
                "Arena position",
                "Where the centre of the arena is, measured from the centre of the video.",
                center,
            );
        });
    });
}

/// Runs a grid and passes its result out, for a borrow that must outlive the grid.
fn grid_with<R>(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Grid::new(id).num_columns(2).spacing([12.0, 10.0]).min_col_width(130.0).show(ui, add).inner
}

fn container_basics(container: &Container) -> (Vec2, f32, Color) {
    match *container {
        Container::Circle { center, thickness, color, .. } | Container::Rect { center, thickness, color, .. } => {
            (center, thickness, color)
        }
    }
}

fn bounces(ui: &mut Ui, rules: &mut BounceRules) {
    let defaults = BounceRules::default();
    ui.label(RichText::new("What happens to a ball each time it hits the wall.").color(theme::MUTED));
    ui.add_space(6.0);
    grid(ui, "rules", |ui| {
        let mut gain = (rules.speed_gain - 1.0) * 100.0;
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
        resettable(response, &mut rules.speed_gain, 1.0);
        slider(
            ui,
            "Top speed",
            "Balls never go faster than this, so speeding up can't run away.",
            &mut rules.max_speed,
            defaults.max_speed,
            10.0..=20000.0,
            log_units(" px/s"),
        );
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
}

fn look(ui: &mut Ui, look: &mut Look) {
    let d = Look::default();
    heading(ui, "Background");
    grid(ui, "background", |ui| {
        color_row(
            ui,
            "Top colour",
            "The background fades from this colour at the top...",
            &mut look.background_top,
            d.background_top,
        );
        color_row(
            ui,
            "Bottom colour",
            "...to this colour at the bottom.",
            &mut look.background_bottom,
            d.background_bottom,
        );
    });
    heading(ui, "Trails");
    grid(ui, "trails", |ui| {
        slider(
            ui,
            "Length",
            "How far back in time each ball's trail reaches. 0 turns trails off.",
            &mut look.trail_seconds,
            d.trail_seconds,
            0.0..=2.0,
            units(" s", 2),
        );
        slider(
            ui,
            "Width",
            "Trail width compared to the ball.",
            &mut look.trail_width,
            d.trail_width,
            0.0..=2.0,
            percent,
        );
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
    heading(ui, "Glow");
    grid(ui, "glow", |ui| {
        slider(
            ui,
            "Ball glow",
            "The soft halo around each ball.",
            &mut look.ball_glow,
            d.ball_glow,
            0.0..=2.0,
            percent,
        );
        slider(
            ui,
            "Wall flash",
            "A flash of light where a ball hits the wall.",
            &mut look.flash_strength,
            d.flash_strength,
            0.0..=5.0,
            percent,
        );
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
    more(ui, "look-more", |ui| {
        grid(ui, "look-more-grid", |ui| {
            slider(
                ui,
                "Flash length",
                "How long a wall flash lasts.",
                &mut look.flash_seconds,
                d.flash_seconds,
                0.0..=2.0,
                units(" s", 2),
            );
            slider(
                ui,
                "Flash size",
                "How big a wall flash is.",
                &mut look.flash_size,
                d.flash_size,
                1.0..=300.0,
                units(" px", 0),
            );
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

const ROOTS: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const QUALITIES: [(&str, &str); 10] = [
    ("", "Major"),
    ("m", "Minor"),
    ("7", "7"),
    ("m7", "m7"),
    ("maj7", "maj7"),
    ("sus2", "sus2"),
    ("sus4", "sus4"),
    ("add9", "add9"),
    ("dim", "dim"),
    ("aug", "aug"),
];

/// Splits a chord name into its root ("F#") and quality ("m7").
fn split_chord(chord: &str) -> (&str, &str) {
    let len = chord.chars().next().map_or(0, char::len_utf8);
    let len = match chord[len..].chars().next() {
        Some('#') | Some('b') => len + 1,
        _ => len,
    };
    chord.split_at(len.min(chord.len()))
}

fn sound(ui: &mut Ui, sound: &mut Sound) {
    let d = Sound::default();
    ui.checkbox(&mut sound.enabled, "Play a note on every wall hit");
    ui.add_enabled_ui(sound.enabled, |ui| {
        heading(ui, "Chords");
        ui.label(
            RichText::new("Each hit plays a note from the current chord. Click a chord to change it.")
                .color(theme::MUTED),
        );
        chords(ui, &mut sound.progression);
        ui.add_space(6.0);
        grid(ui, "sound", |ui| {
            label(ui, "Sound", "A built-in plucked string, or your own WAV file.");
            ui.horizontal(|ui| {
                let is_pluck = matches!(sound.instrument, Instrument::Pluck { .. });
                if ui.add(egui::Button::selectable(is_pluck, "Pluck")).clicked() && !is_pluck {
                    sound.instrument = Instrument::Pluck { decay: 0.9, brightness: 0.5 };
                }
                if ui.add(egui::Button::selectable(!is_pluck, "My WAV file")).clicked() && is_pluck {
                    sound.instrument = Instrument::Sample { path: String::new(), root_note: 60 };
                }
            });
            ui.end_row();
            match &mut sound.instrument {
                Instrument::Pluck { decay, brightness } => {
                    slider(
                        ui,
                        "Ring time",
                        "How long each note rings before fading out.",
                        decay,
                        0.9,
                        0.05..=6.0,
                        units(" s", 2),
                    );
                    slider(
                        ui,
                        "Brightness",
                        "Higher sounds sharper, lower sounds softer.",
                        brightness,
                        0.5,
                        0.0..=1.0,
                        percent,
                    );
                }
                Instrument::Sample { path, root_note } => {
                    label(ui, "File", "The WAV file to play on each hit. It is pitched up or down to match the chord.");
                    ui.horizontal(|ui| {
                        if ui.button("Choose…").clicked()
                            && let Some(file) = rfd::FileDialog::new().add_filter("WAV audio", &["wav"]).pick_file()
                        {
                            *path = file.display().to_string();
                        }
                        let name = std::path::Path::new(path.as_str())
                            .file_name()
                            .map_or("No file chosen".into(), |n| n.to_string_lossy().into_owned());
                        ui.label(name).on_hover_text(path.as_str());
                    });
                    ui.end_row();
                    slider(
                        ui,
                        "Note in the file",
                        "The note your recording plays, so it can be tuned to the chords. Middle C is C4.",
                        root_note,
                        60,
                        24..=108,
                        note_names,
                    );
                }
            }
            slider(
                ui,
                "Lowest note",
                "The lowest note a hit can play. Middle C is C4.",
                &mut sound.low_note,
                d.low_note,
                24..=108,
                note_names,
            );
            slider(
                ui,
                "Highest note",
                "The highest note a hit can play.",
                &mut sound.high_note,
                d.high_note,
                24..=108,
                note_names,
            );
            if sound.high_note < sound.low_note {
                sound.high_note = sound.low_note;
            }
            slider(
                ui,
                "Loudness",
                "Overall loudness of the soundtrack. -14 is the usual level for TikTok, Reels and Shorts.",
                &mut sound.target_loudness,
                d.target_loudness,
                -40.0..=-6.0,
                units(" dB", 0),
            );
        });
        ui.add_space(4.0);
        if ui
            .button("Shuffle notes")
            .on_hover_text("Pick a different set of notes for the same chords. Keep clicking until you like it.")
            .clicked()
        {
            let nanos =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.subsec_nanos());
            sound.seed = sound.seed.wrapping_mul(6364136223846793005).wrapping_add(nanos as u64 | 1) % 1_000_000;
        }
        more(ui, "sound-more", |ui| {
            grid(ui, "sound-more-grid", |ui| {
                slider(
                    ui,
                    "Quietest hit heard",
                    "Gentle hits slower than this make no sound, which avoids a buzz when balls roll along the wall.",
                    &mut sound.min_impact_speed,
                    d.min_impact_speed,
                    0.0..=2000.0,
                    units(" px/s", 0),
                );
                slider(
                    ui,
                    "Full volume at",
                    "Hits at this speed or faster play at full volume.",
                    &mut sound.full_volume_speed,
                    d.full_volume_speed,
                    1.0..=10000.0,
                    log_units(" px/s"),
                );
                slider(
                    ui,
                    "Stereo width",
                    "Pans hits left and right by where they happen. 0% is mono.",
                    &mut sound.stereo_width,
                    d.stereo_width,
                    0.0..=1.0,
                    percent,
                );
            });
        });
    });
}

fn note_names(slider: Slider<'_>) -> Slider<'_> {
    slider.custom_formatter(|n, _| note_name(n as u8)).custom_parser(parse_note)
}

/// Chord buttons. Clicking one opens a small editor for its root, type and length.
fn chords(ui: &mut Ui, progression: &mut Vec<ChordStep>) {
    let mut remove = None;
    let mut swap = None;
    let count = progression.len();
    ui.horizontal_wrapped(|ui| {
        for (i, step) in progression.iter_mut().enumerate() {
            let text = RichText::new(format!("{}  ", step.chord)).strong();
            let response = ui
                .add(egui::Button::new((
                    text,
                    RichText::new(format!("{:.1}s", step.seconds)).small().color(theme::MUTED),
                )))
                .on_hover_text("Click to change this chord");
            Popup::menu(&response).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(300.0);
                let (root, quality) = split_chord(&step.chord);
                let (root, quality) = (root.to_string(), quality.to_string());
                ui.label(RichText::new("Note").color(theme::MUTED));
                ui.horizontal_wrapped(|ui| {
                    for name in ROOTS {
                        if ui.add(egui::Button::selectable(root == name, name)).clicked() {
                            step.chord = format!("{name}{quality}");
                        }
                    }
                });
                ui.label(RichText::new("Type").color(theme::MUTED));
                ui.horizontal_wrapped(|ui| {
                    for (suffix, name) in QUALITIES {
                        if ui.add(egui::Button::selectable(quality == suffix, name)).clicked() {
                            step.chord = format!("{root}{suffix}");
                        }
                    }
                });
                ui.label(RichText::new("Length").color(theme::MUTED));
                ui.add(units(" s", 1)(Slider::new(&mut step.seconds, 0.25..=16.0)));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("Move earlier")).clicked() {
                        swap = Some((i - 1, i));
                    }
                    if ui.add_enabled(i + 1 < count, egui::Button::new("Move later")).clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.add_enabled(count > 1, egui::Button::new("Remove")).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            });
        }
        if ui.button("+").on_hover_text("Add a chord").clicked() {
            let next = progression.last().cloned().unwrap_or(ChordStep { chord: "C".into(), seconds: 4.0 });
            progression.push(next);
        }
    });
    if let Some((a, b)) = swap {
        progression.swap(a, b);
    }
    if let Some(i) = remove {
        progression.remove(i);
    }
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
    use super::*;

    #[test]
    fn chord_names_split_into_root_and_type() {
        assert_eq!(split_chord("F#m7"), ("F#", "m7"));
        assert_eq!(split_chord("Bb"), ("Bb", ""));
        assert_eq!(split_chord("Dsus4"), ("D", "sus4"));
        for root in ROOTS {
            for (quality, _) in QUALITIES {
                assert!(bpe_audio::chord::pitch_classes(&format!("{root}{quality}")).is_some(), "{root}{quality}");
            }
        }
    }

    #[test]
    fn note_names() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(parse_note("C4"), Some(60.0));
        assert_eq!(parse_note("A4"), Some(69.0));
        assert_eq!(parse_note("72"), Some(72.0));
    }
}
