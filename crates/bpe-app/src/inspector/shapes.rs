//! Custom shapes in the Arena tab: a drawn, imported or traced outline as the arena, and
//! shapes as obstacles inside it.

use super::{LINE_COLOR, color_row, grid, label, position_row, slider, units};
use crate::draw_shape::{self, DrawTarget};
use crate::theme;
use bpe_scene::shape_import::{self, TraceBy, TraceOptions};
use bpe_scene::{Color, Container, Obstacle, Scene, Shape, Vec2};
use eframe::egui::{self, RichText, Ui};

/// Radius a new custom arena gets, in pixels.
pub const ARENA_RADIUS: f32 = 480.0;
const OBSTACLE_SIZE: f32 = 120.0;
const OBSTACLE_COLOR: Color = Color::rgb(0xc8, 0xd8, 0xff);

/// Built-in outlines offered when a scene has none of its own yet.
pub fn built_in() -> Vec<Shape> {
    vec![Shape::heart(), Shape::star(5, 0.5), Shape::wave()]
}

/// Adds `shape` to the scene under a name no other shape has, and returns that name.
pub fn add_shape(shapes: &mut Vec<Shape>, mut shape: Shape) -> String {
    let probe = Scene { shapes: std::mem::take(shapes), ..Scene::default() };
    shape.name = probe.unique_shape_name(&shape.name);
    *shapes = probe.shapes;
    let name = shape.name.clone();
    shapes.push(shape);
    name
}

/// The name of a closed shape to use for a new custom arena: the first closed one in the
/// scene, or a heart added for the purpose.
fn closed_shape(shapes: &mut Vec<Shape>) -> String {
    match shapes.iter().find(|s| s.is_closed()) {
        Some(shape) => shape.name.clone(),
        None => add_shape(shapes, Shape::heart()),
    }
}

/// A custom arena that keeps the old arena's position, line width and colour.
pub fn make_custom(old: &Container, shapes: &mut Vec<Shape>) -> Container {
    Container::Custom {
        shape: closed_shape(shapes),
        center: old.center(),
        radius: ARENA_RADIUS,
        rotation: 0.0,
        thickness: old.thickness(),
        color: old.color(),
    }
}

/// The settings of a custom arena: its outline, size, turn and line.
pub fn custom_arena(ui: &mut Ui, container: &mut Container, shapes: &mut Vec<Shape>) {
    let Container::Custom { shape, radius, rotation, thickness, color, .. } = container else { return };
    grid(ui, "custom-arena", |ui| {
        let name = label(ui, "Outline", "Which shape the arena is. Drawn, imported and traced shapes are listed here.");
        shape_picker(ui, "arena-shape", name.id, shape, shapes, true);
        ui.end_row();
        slider(
            ui,
            "Size",
            "Distance from the centre to the shape's farthest point, in pixels. You can also drag the edge in the preview.",
            radius,
            ARENA_RADIUS,
            20.0..=1200.0,
            units(" px", 0),
        );
        slider(ui, "Turn", "Turns the shape, clockwise.", rotation, 0.0, -180.0..=180.0, units("°", 0));
        slider(ui, "Line width", "Thickness of the outline. 0 hides it.", thickness, 3.0, 0.0..=40.0, units(" px", 1));
        crate::keys::mark_next(ui, bpe_scene::Property::ArenaColor);
        color_row(ui, "Line colour", "", color, LINE_COLOR);
    });
    if !shapes.iter().any(|s| s.name == *shape) {
        ui.label(
            RichText::new(format!("There is no shape called \"{shape}\" any more, so balls aren't held in."))
                .color(theme::WARNING),
        );
    } else if !shapes.iter().any(|s| s.name == *shape && s.is_closed()) {
        ui.label(
            RichText::new("This outline isn't closed, so balls can fall out of it. Close it to hold them in.")
                .color(theme::WARNING),
        );
    }
}

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {
    /// How pictures are traced, kept while the editor is open.
    trace: TraceOptions,
    /// The last import or trace that failed, and why.
    error: Option<String>,
}

/// The Draw, Import SVG and Trace picture buttons for a custom arena, with tracing settings
/// under "More".
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    let Container::Custom { shape, radius, thickness, center, rotation, .. } = &mut scene.container else { return };
    let shapes = &mut scene.shapes;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Draw…")
            .on_hover_text(
                "Draw the outline on the preview: click for corners, drag for curves, click the first point or press Enter to close.",
            )
            .clicked()
        {
            draw_shape::start(ui.ctx(), DrawTarget::Arena);
        }
        if ui.button("Import SVG…").on_hover_text("Use every path in an SVG drawing as the arena's outline.").clicked()
            && let Some(file) = rfd::FileDialog::new().add_filter("SVG drawing", &["svg"]).pick_file()
        {
            let name = file_name(&file);
            match std::fs::read(&file).map_err(anyhow::Error::from).and_then(|d| shape_import::import_svg(&d, &name, 1.0))
            {
                Ok(placed) => {
                    *shape = add_shape(shapes, placed.shape);
                    state.error = None;
                }
                Err(error) => state.error = Some(format!("Couldn't import {name}: {error}")),
            }
        }
        if ui
            .button("Trace picture…")
            .on_hover_text(
                "Trace the outline of a picture's solid part as the arena. Holes and separate parts inside it become obstacles.",
            )
            .clicked()
            && let Some(file) = rfd::FileDialog::new()
                .add_filter("Picture", &["png", "jpg", "jpeg", "bmp", "gif", "webp"])
                .pick_file()
        {
            let name = file_name(&file);
            let traced = std::fs::read(&file)
                .map_err(anyhow::Error::from)
                .and_then(|d| shape_import::trace_image(&d, &name, &state.trace, *radius));
            match traced {
                Ok(placed) => {
                    // Everything traced is placed relative to the picture's middle, which stays
                    // where the arena's centre was, turned as the arena is turned.
                    let (origin, turn) = (*center, Vec2::from_angle(rotation.to_radians()));
                    let mut placed = placed.into_iter();
                    if let Some(outline) = placed.next() {
                        *shape = add_shape(shapes, outline.shape);
                        *radius = outline.size.round();
                        *center = origin + turn.rotate(outline.center);
                    }
                    for part in placed {
                        let name = add_shape(shapes, part.shape);
                        scene.obstacles.push(Obstacle::Shape {
                            shape: name,
                            position: origin + turn.rotate(part.center),
                            rotation: *rotation,
                            scale: part.size,
                            thickness: *thickness,
                            color: OBSTACLE_COLOR,
                        });
                    }
                    state.error = None;
                }
                Err(error) => state.error = Some(format!("Couldn't trace {name}: {error}")),
            }
        }
    });
    if let Some(message) = &state.error {
        ui.label(RichText::new(message).color(theme::WARNING));
    }
    egui::CollapsingHeader::new("More").id_salt("trace-more").show(ui, |ui| {
        grid(ui, "trace-options", |ui| {
            label(ui, "Trace by", "Which part of a picture counts as solid when tracing it.");
            ui.horizontal(|ui| {
                for (by, name, tip) in [
                    (TraceBy::Alpha, "See-through", "Solid where the picture isn't see-through."),
                    (TraceBy::Dark, "Dark", "Solid where the picture is dark."),
                    (TraceBy::Light, "Light", "Solid where the picture is light."),
                ] {
                    ui.selectable_value(&mut state.trace.by, by, name).on_hover_text(tip);
                }
            });
            ui.end_row();
            let defaults = TraceOptions::default();
            slider(
                ui,
                "Threshold",
                "How solid or dark a pixel must be to count. Lower takes in more of the picture.",
                &mut state.trace.threshold,
                defaults.threshold,
                0.05..=0.95,
                units("", 2),
            );
            slider(
                ui,
                "Smoothing",
                "How much the traced outline is simplified, in pixels of the picture.",
                &mut state.trace.smoothing,
                defaults.smoothing,
                0.0..=6.0,
                units(" px", 1),
            );
        });
    });
}

fn file_name(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported".into())
}

/// A drop-down of the scene's shapes plus the built-in ones, which are copied into the scene
/// when picked. `closed_only` hides open lines, which can't hold balls.
fn shape_picker(
    ui: &mut Ui,
    id: &str,
    name: egui::Id,
    current: &mut String,
    shapes: &mut Vec<Shape>,
    closed_only: bool,
) {
    let mut picked: Option<Shape> = None;
    let combo = egui::ComboBox::from_id_salt(id).selected_text(current.as_str()).width(200.0).show_ui(ui, |ui| {
        for shape in shapes.iter().filter(|s| !closed_only || s.is_closed()) {
            if ui.selectable_label(*current == shape.name, &shape.name).clicked() {
                *current = shape.name.clone();
            }
        }
        ui.separator();
        for shape in built_in().into_iter().filter(|s| !closed_only || s.is_closed()) {
            if ui.selectable_label(false, format!("New {}", shape.name.to_lowercase())).clicked() {
                picked = Some(shape);
            }
        }
    });
    combo.response.labelled_by(name);
    if let Some(shape) = picked {
        *current = add_shape(shapes, shape);
    }
}

/// The rows of a shape obstacle's card.
#[allow(clippy::too_many_arguments)]
pub fn obstacle_rows(
    ui: &mut Ui,
    shape: &mut String,
    shapes: &mut Vec<Shape>,
    position: &mut Vec2,
    rotation: &mut f32,
    size: &mut f32,
    thickness: &mut f32,
    color: &mut Color,
) {
    let name = label(ui, "Outline", "Which shape this obstacle is.");
    shape_picker(ui, "obstacle-shape", name.id, shape, shapes, false);
    ui.end_row();
    slider(
        ui,
        "Size",
        "Distance from its middle to its farthest point.",
        size,
        OBSTACLE_SIZE,
        4.0..=1200.0,
        units(" px", 0),
    );
    slider(ui, "Turn", "Turns the shape, clockwise.", rotation, 0.0, -180.0..=180.0, units("°", 0));
    position_row(ui, "Position", "Where the shape's middle is, measured from the centre of the video.", position);
    slider(ui, "Line width", "Thickness of the outline.", thickness, 4.0, 0.5..=60.0, units(" px", 1));
    color_row(ui, "Colour", "", color, OBSTACLE_COLOR);
}

/// Buttons that add a shape obstacle: a built-in one, or one drawn on the preview.
pub fn add_obstacle_buttons(ui: &mut Ui, obstacles: &mut Vec<Obstacle>, shapes: &mut Vec<Shape>) {
    ui.menu_button("+ Add a shape", |ui| {
        let mut add = |name: String| {
            obstacles.push(Obstacle::Shape {
                shape: name,
                position: Vec2::ZERO,
                rotation: 0.0,
                scale: OBSTACLE_SIZE,
                thickness: 4.0,
                color: OBSTACLE_COLOR,
            });
        };
        for shape in shapes.clone() {
            if ui.button(&shape.name).clicked() {
                add(shape.name);
                ui.close();
            }
        }
        if !shapes.is_empty() {
            ui.separator();
        }
        for shape in built_in() {
            if ui.button(format!("New {}", shape.name.to_lowercase())).clicked() {
                let name = add_shape(shapes, shape);
                add(name);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Draw…").clicked() {
            draw_shape::start(ui.ctx(), DrawTarget::Obstacle);
            ui.close();
        }
    })
    .response
    .on_hover_text("An outline that balls bounce off: drawn, imported for the arena, or built in");
}
