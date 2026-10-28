//! The 3D part of the Arena tab: the Flat / 3D switch, the 3D container, the camera, the lights
//! and the look of the balls and glass.

use super::{color_row, dice_button, grid, heading, label, more, percent, slider, slider_row, units};
use crate::theme;
use bpe_scene::{Camera, CameraMode, Container3d, Dice, Lights, Material, Scene, Space, View};
use eframe::egui::{self, RichText, Ui};

/// What the 3D section remembers between frames.
#[derive(Default)]
pub struct State {
    /// The 3D settings from before switching to Flat, so switching back brings them back.
    last_space: Option<Box<Space>>,
    /// How many times a dice was rolled, mixed into each roll's seed.
    rolls: u64,
}

impl State {
    fn dice(&mut self) -> Dice {
        self.rolls += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        Dice::new((nanos as u64) ^ self.rolls.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }
}

/// The view switch, and in 3D everything about the 3D world. Returns whether the scene is 3D,
/// so the Arena tab can leave out the 2D arena settings.
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State) -> bool {
    let aspect = scene.video.width as f32 / scene.video.height.max(1) as f32;
    view_switch(ui, scene, state, aspect);
    let View::Space(space) = &mut scene.view else { return false };
    ui.add_space(6.0);
    container(ui, space, aspect);
    camera(ui, space, state, aspect);
    lights(ui, space, state);
    finish(ui, space);
    true
}

fn view_switch(ui: &mut Ui, scene: &mut Scene, state: &mut State, aspect: f32) {
    heading(ui, "Space");
    let flat = matches!(scene.view, View::Flat);
    let choices = [
        (true, "Flat", "The usual 2D picture."),
        (
            false,
            "3D",
            "Balls bounce inside a glass sphere, a glass cube or an endless tunnel, with real lighting and shadows. \
             Balls, bounce rules, sounds and effects all carry over.",
        ),
    ];
    ui.columns(2, |columns| {
        for (ui, (is_flat, name, help)) in columns.iter_mut().zip(choices) {
            let button = egui::Button::selectable(flat == is_flat, name);
            if ui.add_sized([ui.available_width(), 30.0], button).on_hover_text(help).clicked() && flat != is_flat {
                if is_flat {
                    if let View::Space(space) = std::mem::take(&mut scene.view) {
                        state.last_space = Some(space);
                    }
                } else {
                    let space = state.last_space.take().unwrap_or_else(|| Box::new(fresh_space(scene, aspect)));
                    scene.view = View::Space(space);
                }
            }
        }
    });
}

/// 3D settings for a scene switched to 3D for the first time: a sphere about the size of its
/// 2D arena, with the camera framing it.
fn fresh_space(scene: &Scene, aspect: f32) -> Space {
    let radius = scene.container.size().clamp(150.0, 900.0);
    let container = Container3d::Sphere { radius };
    Space { camera: Camera::fit(&container, aspect), container, ..Space::default() }
}

fn container(ui: &mut Ui, space: &mut Space, aspect: f32) {
    heading(ui, "3D container");
    let current = space.container.name();
    let help = [
        ("Sphere", "A glass ball."),
        ("Cube", "A glass box. Arena spin turns it."),
        (
            "Tunnel",
            "An endless tube with rings. A wall behind the balls pushes them forward, and a ball that finds a ring's gap breaks it.",
        ),
    ];
    ui.columns(help.len(), |columns| {
        for (ui, (name, help)) in columns.iter_mut().zip(help) {
            let button = egui::Button::selectable(current == name, name);
            if ui.add_sized([ui.available_width(), 30.0], button).on_hover_text(help).clicked() && current != name {
                space.container = space.container.with_name(name);
                // The old camera rarely suits the new shape, a tunnel above all.
                space.camera = Camera { mode: space.camera.mode.clone(), ..Camera::fit(&space.container, aspect) };
            }
        }
    });
    ui.add_space(6.0);
    grid(ui, "container-3d", |ui| match &mut space.container {
        Container3d::Sphere { radius } => {
            slider(ui, "Size", "Radius of the sphere in pixels.", radius, 380.0, 50.0..=1500.0, units(" px", 0));
        }
        Container3d::Cube { size } => {
            slider(ui, "Size", "Length of each side of the cube in pixels.", size, 600.0, 80.0..=3000.0, units(" px", 0));
        }
        Container3d::Tunnel { radius, rings, spacing, gap, spin, speed, .. } => {
            slider(ui, "Size", "Radius of the tunnel in pixels.", radius, 320.0, 50.0..=1500.0, units(" px", 0));
            slider(ui, "Rings", "How many rings block the tunnel.", rings, 24, 0..=500, |s| s);
            slider(
                ui,
                "Ring spacing",
                "Distance from one ring to the next.",
                spacing,
                520.0,
                40.0..=3000.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Gap size",
                "How wide each ring's gap is. Bigger gaps are easier to get through.",
                gap,
                70.0,
                5.0..=300.0,
                units("°", 0),
            );
            slider(
                ui,
                "Ring spin",
                "How fast the rings turn, in degrees per second. Every other ring turns the other way.",
                spin,
                40.0,
                -360.0..=360.0,
                units("°/s", 0),
            );
            slider(
                ui,
                "Push speed",
                "How fast the wall behind the balls moves forward, in pixels per second. It stops short of the next ring.",
                speed,
                220.0,
                0.0..=2000.0,
                units(" px/s", 0),
            );
        }
    });
    if let Container3d::Tunnel { gap_offset, .. } = &mut space.container {
        more(ui, "tunnel-more", |ui| {
            grid(ui, "tunnel-more-grid", |ui| {
                slider(
                    ui,
                    "Gap offset",
                    "How far each ring's gap is turned from the one before it.",
                    gap_offset,
                    75.0,
                    -180.0..=180.0,
                    units("°", 0),
                );
            });
        });
    }
}

fn camera(ui: &mut Ui, space: &mut Space, state: &mut State, aspect: f32) {
    heading_with_dice(ui, "Camera", "Roll a random camera", || {
        let mut dice = state.dice();
        space.roll_camera(&mut dice, aspect);
    });
    ui.label(
        RichText::new("Drag the preview to turn the camera, Shift+drag to move it, scroll to zoom, right-click to reset.")
            .small()
            .color(theme::MUTED),
    );
    ui.add_space(4.0);
    let fitted = Camera::fit(&space.container, aspect);
    let camera = &mut space.camera;
    let modes = [
        ("Fixed", "The camera stays where it is."),
        ("Orbit", "The camera circles slowly around the container."),
        ("Follow a ball", "The camera chases one ball."),
    ];
    let current = camera.mode.name();
    ui.columns(modes.len(), |columns| {
        for (ui, (name, help)) in columns.iter_mut().zip(modes) {
            let button = egui::Button::selectable(current == name, name);
            if ui.add_sized([ui.available_width(), 26.0], button).on_hover_text(help).clicked() && current != name {
                camera.mode = CameraMode::with_name(name);
            }
        }
    });
    ui.add_space(6.0);
    grid(ui, "camera-3d", |ui| {
        match &mut camera.mode {
            CameraMode::Fixed => {}
            CameraMode::Orbit { speed } => {
                slider(
                    ui,
                    "Orbit speed",
                    "Degrees per second the camera circles. Negative goes the other way.",
                    speed,
                    12.0,
                    -120.0..=120.0,
                    units("°/s", 0),
                );
            }
            CameraMode::Follow { ball, distance } => {
                slider(ui, "Ball", "Which ball the camera follows, counting from 0.", ball, 0, 0..=500, |s| s);
                slider(
                    ui,
                    "Distance",
                    "How far behind the ball the camera stays.",
                    distance,
                    700.0,
                    50.0..=5000.0,
                    units(" px", 0),
                );
            }
        }
        if !matches!(camera.mode, CameraMode::Follow { .. }) {
            let (mut turn, mut height, mut distance) = camera.angles();
            let before = (turn, height, distance);
            let (fit_turn, fit_height, fit_distance) = fitted.angles();
            let turn_row = slider_row(
                ui,
                "Turn",
                "Turns the camera around the container.",
                &mut turn,
                -180.0..=180.0,
                units("°", 0),
            );
            let height_row = slider_row(
                ui,
                "Height",
                "How far above (positive) or below the container the camera looks from.",
                &mut height,
                -85.0..=85.0,
                units("°", 0),
            );
            let distance_row = slider_row(
                ui,
                "Distance",
                "How far the camera is from what it looks at.",
                &mut distance,
                50.0..=20_000.0,
                |s| units(" px", 0)(s.logarithmic(true)),
            );
            super::resettable(turn_row, &mut turn, fit_turn);
            super::resettable(height_row, &mut height, fit_height);
            super::resettable(distance_row, &mut distance, fit_distance);
            if (turn, height, distance) != before {
                camera.set_angles(turn, height, distance);
            }
        }
        slider(
            ui,
            "Field of view",
            "How wide the camera sees. Small values look flat like a telephoto lens, big values look like a wide angle.",
            &mut camera.fov,
            fitted.fov,
            5.0..=150.0,
            units("°", 0),
        );
    });
    if ui
        .button("Fit to the container")
        .on_hover_text("Put the camera back where it shows the whole container. Ctrl+Z undoes it.")
        .clicked()
    {
        *camera = Camera { mode: camera.mode.clone(), ..fitted };
    }
}

fn lights(ui: &mut Ui, space: &mut Space, state: &mut State) {
    heading_with_dice(ui, "Lights", "Roll random lights and colours", || {
        let mut dice = state.dice();
        space.roll_lights(&mut dice);
    });
    let lights = &mut space.lights;
    let d = Lights::default();
    grid(ui, "lights-3d", |ui| {
        slider(
            ui,
            "Light direction",
            "Where the main light comes from, turning around the container. 0° is from the camera's side.",
            &mut lights.key_turn,
            d.key_turn,
            -180.0..=180.0,
            units("°", 0),
        );
        slider(
            ui,
            "Light height",
            "How high the main light is. 90° is straight above.",
            &mut lights.key_height,
            d.key_height,
            5.0..=90.0,
            units("°", 0),
        );
        slider(
            ui,
            "Brightness",
            "How bright the main light is.",
            &mut lights.intensity,
            d.intensity,
            0.0..=10.0,
            units("", 1),
        );
        color_row(ui, "Light colour", "Colour of the main light.", &mut lights.key_color, d.key_color);
        slider(
            ui,
            "Sky light",
            "Soft light from all around, coloured from above and below.",
            &mut lights.ambient,
            d.ambient,
            0.0..=3.0,
            units("", 2),
        );
        color_row(ui, "Sky colour", "Light from above.", &mut lights.ambient_top, d.ambient_top);
        color_row(ui, "Ground colour", "Light from below.", &mut lights.ambient_bottom, d.ambient_bottom);
        label(ui, "Shadows", "Whether balls cast shadows on each other and the walls.");
        ui.checkbox(&mut lights.shadows, "");
        ui.end_row();
    });
    more(ui, "lights-more", |ui| {
        grid(ui, "lights-more-grid", |ui| {
            slider(
                ui,
                "Rim light",
                "A bright edge around each ball, which helps it stand out from the background.",
                &mut lights.rim,
                d.rim,
                0.0..=3.0,
                units("", 2),
            );
        });
    });
}

fn finish(ui: &mut Ui, space: &mut Space) {
    heading(ui, "Balls and glass");
    let m = Material::default();
    let material = &mut space.material;
    grid(ui, "material-3d", |ui| {
        slider(
            ui,
            "Shininess",
            "100% is a mirror finish, 0% completely matt.",
            &mut material.roughness,
            m.roughness,
            0.0..=1.0,
            |s| {
                s.custom_formatter(|v, _| format!("{:.0}%", (1.0 - v) * 100.0)).custom_parser(|s| {
                    s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| 1.0 - v / 100.0)
                })
            },
        );
        slider(
            ui,
            "Metal",
            "0% looks like plastic, 100% like polished metal.",
            &mut material.metalness,
            m.metalness,
            0.0..=1.0,
            percent,
        );
        slider(
            ui,
            "Glass",
            "How much the container's glass shows. 0% leaves only its edges.",
            &mut material.glass,
            m.glass,
            0.0..=1.0,
            percent,
        );
        color_row(ui, "Glass colour", "Tint of the glass, its edges and the tunnel's rings.", &mut material.glass_color, m.glass_color);
        slider(
            ui,
            "Edge glow",
            "Brightness of the container's edges and the tunnel's rings.",
            &mut material.edge_glow,
            m.edge_glow,
            0.0..=5.0,
            units("", 2),
        );
    });
    let d = Space::default();
    more(ui, "material-more", |ui| {
        grid(ui, "material-more-grid", |ui| {
            slider(
                ui,
                "Walls",
                "How bright the container's inside walls are. 0 hides them.",
                &mut material.walls,
                m.walls,
                0.0..=1.0,
                percent,
            );
            slider(
                ui,
                "Depth spread",
                "How much balls are spread out in depth when they appear, so a flat block of balls fills the space.",
                &mut space.scatter,
                d.scatter,
                0.0..=1.0,
                percent,
            );
            slider(
                ui,
                "Depth gravity",
                "Pulls balls away from the camera (positive) or towards it (negative).",
                &mut space.gravity_depth,
                d.gravity_depth,
                -3000.0..=3000.0,
                units("", 0),
            );
        });
    });
}

/// A group heading with a dice button on the right that runs `roll` when clicked.
fn heading_with_dice(ui: &mut Ui, text: &str, what: &str, roll: impl FnOnce()) {
    ui.add_space(4.0);
    let clicked = ui
        .horizontal(|ui| {
            ui.label(RichText::new(text.to_uppercase()).size(12.0).color(theme::MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| dice_button(ui, what).clicked())
                .inner
        })
        .inner;
    if clicked {
        roll();
    }
}
