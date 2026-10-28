//! Flat or 3D: how the scene is shown, and the 3D world's container, camera and lights.
//!
//! In 3D, world coordinates are still pixels of the output video: x to the right, y down and z
//! away from the default camera, with the origin in the middle of the container. Gravity is the
//! scene's `Physics::gravity` (x and y) plus [`Space::gravity_depth`] along z, so gravity sliders
//! and keyframes work the same in both views.

use crate::{Color, Dice, Property, Vec2};
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Whether the scene is a flat 2D picture or a 3D space.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum View {
    /// The usual 2D scene.
    #[default]
    Flat,
    /// Balls bounce inside a 3D container with real lighting.
    Space(Box<Space>),
}

/// Everything about the 3D view. Balls, bounce rules, sounds, effects and keyframes come from the
/// rest of the scene as usual.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Space {
    pub container: Container3d,
    pub camera: Camera,
    pub lights: Lights,
    pub material: Material,
    /// Pull along z in pixels per second squared, added to the scene's gravity.
    pub gravity_depth: f32,
    /// How much balls are spread out in depth when they appear, 0 to 1, so a flat block of balls
    /// fills the space. At 0 they start exactly at their spawner's depth.
    pub scatter: f32,
}

/// The shape balls bounce inside in 3D.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Container3d {
    /// A glass ball.
    Sphere { radius: f32 },
    /// A glass box with sides `size` long.
    Cube { size: f32 },
    /// An endless tube. A solid wall behind the balls pushes them forward at `speed`, into rings
    /// that block the tube except for a gap. A ball that gets through a gap breaks that ring.
    Tunnel {
        radius: f32,
        /// How many rings there are.
        rings: u32,
        /// Distance from one ring to the next.
        spacing: f32,
        /// Width of each ring's gap, in degrees.
        gap: f32,
        /// Degrees per second the rings turn. Every other ring turns the other way.
        spin: f32,
        /// Degrees each ring's gap is turned from the one before it.
        gap_offset: f32,
        /// Pixels per second the back wall moves forward.
        speed: f32,
    },
}

/// How the camera moves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CameraMode {
    /// Stays where it is put.
    Fixed,
    /// Circles around its target, `speed` degrees per second.
    Orbit { speed: f32 },
    /// Looks at one ball from `distance` away, from the camera's direction.
    Follow { ball: u32, distance: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    pub mode: CameraMode,
    /// Vertical field of view in degrees.
    pub fov: f32,
    /// Where the camera is, in world pixels. In a tunnel this is measured from the back wall.
    pub position: Vec3,
    /// The point it looks at. In a tunnel this is measured from the back wall.
    pub target: Vec3,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lights {
    /// Degrees around the up axis the key light comes from. 0 is from the camera's side.
    pub key_turn: f32,
    /// Degrees above the horizon the key light comes from.
    pub key_height: f32,
    pub key_color: Color,
    /// Brightness of the key light.
    pub intensity: f32,
    /// Light from above everything, like the sky.
    pub ambient_top: Color,
    /// Light from below, like the ground.
    pub ambient_bottom: Color,
    /// Brightness of the light from above and below.
    pub ambient: f32,
    /// Bright edge around each ball, from behind.
    pub rim: f32,
    /// Whether balls cast shadows.
    pub shadows: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    /// 0 is a mirror finish, 1 completely matt.
    pub roughness: f32,
    /// 0 is plastic, 1 is metal.
    pub metalness: f32,
    /// How much the container's glass shows, 0 to 1.
    pub glass: f32,
    /// Tint of the glass and the container's edges.
    pub glass_color: Color,
    /// Brightness of the container's edges and the tunnel's rings.
    pub edge_glow: f32,
    /// Brightness of the container's inside walls, 0 for none.
    pub walls: f32,
}

impl Default for Space {
    fn default() -> Self {
        Self {
            container: Container3d::default(),
            camera: Camera::default(),
            lights: Lights::default(),
            material: Material::default(),
            gravity_depth: 0.0,
            scatter: 0.5,
        }
    }
}

impl Default for Camera {
    fn default() -> Self {
        Camera::fit(&Container3d::default(), 9.0 / 16.0)
    }
}

impl Default for Lights {
    fn default() -> Self {
        Self {
            key_turn: 35.0,
            key_height: 55.0,
            key_color: Color::rgb(0xff, 0xf2, 0xe0),
            intensity: 3.0,
            ambient_top: Color::rgb(0x6a, 0x7c, 0xb8),
            ambient_bottom: Color::rgb(0x1a, 0x10, 0x24),
            ambient: 0.6,
            rim: 0.5,
            shadows: true,
        }
    }
}

impl Default for Material {
    fn default() -> Self {
        Self {
            roughness: 0.3,
            metalness: 0.0,
            glass: 0.35,
            glass_color: Color::rgb(0xb8, 0xd8, 0xff),
            edge_glow: 1.0,
            walls: 0.12,
        }
    }
}

impl Default for Container3d {
    fn default() -> Self {
        Container3d::Sphere { radius: 380.0 }
    }
}

impl Container3d {
    pub const NAMES: [&'static str; 3] = ["Sphere", "Cube", "Tunnel"];

    /// "Sphere", "Cube" or "Tunnel".
    pub fn name(&self) -> &'static str {
        match self {
            Container3d::Sphere { .. } => "Sphere",
            Container3d::Cube { .. } => "Cube",
            Container3d::Tunnel { .. } => "Tunnel",
        }
    }

    /// A container of the named kind with its usual settings, keeping roughly the same size as `self`.
    pub fn with_name(&self, name: &str) -> Self {
        let size = self.size();
        match name {
            "Cube" => Container3d::Cube { size: size * 1.6 },
            "Tunnel" => Container3d::tunnel(size * 0.85),
            _ => Container3d::Sphere { radius: size },
        }
    }

    /// A tunnel with the usual ring settings.
    pub fn tunnel(radius: f32) -> Self {
        Container3d::Tunnel { radius, rings: 24, spacing: 520.0, gap: 70.0, spin: 40.0, gap_offset: 75.0, speed: 220.0 }
    }

    /// Distance from the middle to the walls: the radius, or half a cube's side.
    pub fn size(&self) -> f32 {
        match *self {
            Container3d::Sphere { radius } | Container3d::Tunnel { radius, .. } => radius,
            Container3d::Cube { size } => size * 0.5,
        }
    }

    pub fn size_mut(&mut self) -> &mut f32 {
        match self {
            Container3d::Sphere { radius } | Container3d::Tunnel { radius, .. } => radius,
            Container3d::Cube { size } => size,
        }
    }

    /// Whether balls can never leave, so a ball outside is a bug.
    pub fn is_closed(&self) -> bool {
        !matches!(self, Container3d::Tunnel { .. })
    }
}

impl CameraMode {
    pub const NAMES: [&'static str; 3] = ["Fixed", "Orbit", "Follow a ball"];

    pub fn name(&self) -> &'static str {
        match self {
            CameraMode::Fixed => "Fixed",
            CameraMode::Orbit { .. } => "Orbit",
            CameraMode::Follow { .. } => "Follow a ball",
        }
    }

    pub fn with_name(name: &str) -> Self {
        match name {
            "Orbit" => CameraMode::Orbit { speed: 12.0 },
            "Follow a ball" => CameraMode::Follow { ball: 0, distance: 700.0 },
            _ => CameraMode::Fixed,
        }
    }
}

impl Camera {
    /// A fixed camera that shows the whole container from a little above, or looks down the
    /// tunnel from just behind its back wall, for a picture `aspect` wide per unit of height.
    pub fn fit(container: &Container3d, aspect: f32) -> Self {
        match *container {
            Container3d::Tunnel { radius, .. } => Camera {
                mode: CameraMode::Fixed,
                fov: 62.0,
                position: Vec3::new(0.0, -radius * 0.15, -radius * 1.2),
                target: Vec3::new(0.0, 0.0, radius * 3.0),
            },
            _ => {
                let fov: f32 = 36.0;
                // Half the width of what the camera sees, per unit of distance.
                let across = (fov.to_radians() * 0.5).tan() * aspect.clamp(0.2, 5.0).min(1.0);
                let reach = match *container {
                    Container3d::Cube { size } => size * 0.5 * 1.45,
                    _ => container.size(),
                };
                let distance = reach * 1.18 / across;
                let pitch = 12f32.to_radians();
                Camera {
                    mode: CameraMode::Fixed,
                    fov,
                    position: Vec3::new(0.0, -pitch.sin() * distance, -pitch.cos() * distance),
                    target: Vec3::ZERO,
                }
            }
        }
    }

    /// Distance from the camera to its target.
    pub fn distance(&self) -> f32 {
        (self.position - self.target).length()
    }

    /// The camera's turn around its target and height above it, in degrees, and its distance:
    /// (turn, height, distance). A turn of 0 looks along +z.
    pub fn angles(&self) -> (f32, f32, f32) {
        let offset = self.position - self.target;
        let distance = offset.length();
        if distance <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let height = (-offset.y / distance).clamp(-1.0, 1.0).asin().to_degrees();
        let turn = offset.x.atan2(-offset.z).to_degrees();
        (turn, height, distance)
    }

    /// Puts the camera at a turn and height (degrees) and distance from its target.
    pub fn set_angles(&mut self, turn: f32, height: f32, distance: f32) {
        let (turn, height) = (turn.to_radians(), height.clamp(-85.0, 85.0).to_radians());
        let distance = distance.max(1.0);
        let flat = height.cos() * distance;
        self.position = self.target + Vec3::new(turn.sin() * flat, -height.sin() * distance, -turn.cos() * flat);
    }

    /// Turns the camera around its target: `yaw` degrees around the up axis and `pitch` degrees
    /// up or down, never going over the top.
    pub fn orbit(&mut self, yaw: f32, pitch: f32) {
        let offset = self.position - self.target;
        let distance = offset.length().max(1.0);
        // y points down, so looking from above means a negative y offset.
        let current_pitch = (-offset.y / distance).clamp(-1.0, 1.0).asin();
        let current_yaw = offset.x.atan2(-offset.z);
        let pitch = (current_pitch + pitch.to_radians()).clamp(-1.45, 1.45);
        let yaw = current_yaw + yaw.to_radians();
        let flat = pitch.cos() * distance;
        self.position = self.target + Vec3::new(yaw.sin() * flat, -pitch.sin() * distance, -yaw.cos() * flat);
    }

    /// Moves the camera and its target together, sideways and up or down on screen, by `delta` pixels.
    pub fn pan(&mut self, delta: Vec2) {
        let forward = (self.target - self.position).normalize_or(Vec3::Z);
        let right = forward.cross(Vec3::NEG_Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        let shift = right * -delta.x + up * delta.y;
        self.position += shift;
        self.target += shift;
    }

    /// Moves the camera towards its target (`factor` below 1) or away from it (above 1).
    pub fn zoom(&mut self, factor: f32) {
        let offset = self.position - self.target;
        let distance = (offset.length() * factor).clamp(50.0, 20_000.0);
        self.position = self.target + offset.normalize_or(Vec3::NEG_Z) * distance;
    }
}

impl Space {
    /// The value of an animatable 3D setting ([`Property::SPACE`]), or None for any other.
    pub fn number(&self, property: &Property) -> Option<f32> {
        let (turn, height, distance) = self.camera.angles();
        Some(match property {
            Property::CameraTurn => turn,
            Property::CameraHeight => height,
            Property::CameraDistance => distance,
            Property::CameraFov => self.camera.fov,
            Property::LightTurn => self.lights.key_turn,
            Property::LightHeight => self.lights.key_height,
            Property::LightBrightness => self.lights.intensity,
            Property::Glass => self.material.glass,
            Property::EdgeGlow => self.material.edge_glow,
            _ => return None,
        })
    }

    /// Sets an animatable 3D setting. Any other setting is ignored.
    pub fn set_number(&mut self, property: &Property, value: f32) {
        let (turn, height, distance) = self.camera.angles();
        match property {
            Property::CameraTurn => self.camera.set_angles(value, height, distance),
            Property::CameraHeight => self.camera.set_angles(turn, value, distance),
            Property::CameraDistance => self.camera.set_angles(turn, height, value),
            Property::CameraFov => self.camera.fov = value,
            Property::LightTurn => self.lights.key_turn = value,
            Property::LightHeight => self.lights.key_height = value,
            Property::LightBrightness => self.lights.intensity = value,
            Property::Glass => self.material.glass = value,
            Property::EdgeGlow => self.material.edge_glow = value,
            _ => {}
        }
    }
}

impl Space {
    /// The key light's direction, pointing from the scene towards the light.
    pub fn key_direction(&self) -> Vec3 {
        let (turn, height) = (self.lights.key_turn.to_radians(), self.lights.key_height.to_radians());
        Vec3::new(turn.sin() * height.cos(), -height.sin(), -turn.cos() * height.cos()).normalize()
    }
}

impl Space {
    /// The settings that change how balls move, with the camera, lights and looks at their
    /// defaults, to tell a picture-only change from one that needs simulating again.
    pub fn moving_parts(&self) -> Space {
        Space {
            container: self.container.clone(),
            gravity_depth: self.gravity_depth,
            scatter: self.scatter,
            ..Space::default()
        }
    }

    /// Rolls a new camera angle, distance and field of view, keeping the container in view.
    pub fn roll_camera(&mut self, dice: &mut Dice, aspect: f32) {
        let fitted = Camera::fit(&self.container, aspect);
        let camera = &mut self.camera;
        camera.target = fitted.target;
        camera.fov = dice.range(28.0, 60.0);
        match self.container {
            Container3d::Tunnel { radius, .. } => {
                camera.fov = dice.range(50.0, 85.0);
                camera.position =
                    fitted.position + Vec3::new(dice.range(-0.4, 0.4), dice.range(-0.4, 0.4), 0.0) * radius;
                camera.mode = CameraMode::Fixed;
            }
            _ => {
                let (_, _, fitted_distance) = fitted.angles();
                // Keep the same framing at the new field of view.
                let distance = fitted_distance * (18f32.to_radians().tan() / (camera.fov * 0.5).to_radians().tan());
                camera.set_angles(dice.range(-50.0, 50.0), dice.range(-10.0, 45.0), distance * dice.range(0.95, 1.2));
                camera.mode = match dice.int(0, 3) {
                    0 | 1 => CameraMode::Fixed,
                    2 => CameraMode::Orbit { speed: dice.range(6.0, 24.0) * dice.sign() },
                    _ => CameraMode::Orbit { speed: dice.range(2.0, 8.0) * dice.sign() },
                };
            }
        }
    }

    /// Rolls new light directions, colours and brightness.
    pub fn roll_lights(&mut self, dice: &mut Dice) {
        let lights = &mut self.lights;
        lights.key_turn = dice.range(-120.0, 120.0);
        lights.key_height = dice.range(25.0, 80.0);
        let warm = dice.range(0.0, 0.18);
        lights.key_color = Color::from_hsv(0.08 + warm * dice.sign(), dice.range(0.0, 0.3), 1.0);
        lights.intensity = dice.range(2.0, 4.5);
        let hue = dice.unit();
        lights.ambient_top = Color::from_hsv(hue, dice.range(0.2, 0.7), dice.range(0.5, 0.9));
        lights.ambient_bottom =
            Color::from_hsv(hue + dice.range(0.2, 0.6), dice.range(0.3, 0.8), dice.range(0.05, 0.3));
        lights.ambient = dice.range(0.3, 0.9);
        lights.rim = dice.range(0.0, 1.0);
        let material = &mut self.material;
        material.roughness = dice.range(0.1, 0.7);
        material.metalness = if dice.chance(0.3) { dice.range(0.5, 1.0) } else { 0.0 };
        material.glass_color = Color::from_hsv(dice.unit(), dice.range(0.2, 0.9), 1.0);
        material.edge_glow = dice.range(0.5, 2.5);
    }
}

impl crate::Scene {
    /// The 3D settings, if the scene is 3D.
    pub fn space(&self) -> Option<&Space> {
        match &self.view {
            View::Space(space) => Some(space),
            View::Flat => None,
        }
    }

    pub fn space_mut(&mut self) -> Option<&mut Space> {
        match &mut self.view {
            View::Space(space) => Some(space),
            View::Flat => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scene;

    /// Changing one camera angle leaves the others where they were, give or take a rounding
    /// error, which the editor ignores (see `Value::same`).
    #[test]
    fn camera_angles_change_one_at_a_time() {
        let mut changed = 0;
        for i in 0..2000 {
            let mut space = Space::default();
            let t = i as f32;
            space.camera.set_angles(t * 0.37 - 180.0, t * 0.071 - 70.0, 300.0 + t * 7.3);
            let before = Property::SPACE.map(|p| space.number(&p));
            space.set_number(&Property::CameraTurn, t * 0.13);
            let after = Property::SPACE.map(|p| space.number(&p));
            changed += (0..9)
                .filter(|&k| {
                    let (Some(a), Some(b)) = (before[k], after[k]) else { return true };
                    k != 0 && !crate::Value::Number(a).same(crate::Value::Number(b))
                })
                .count();
        }
        assert_eq!(changed, 0, "settings moved by themselves");
    }

    #[test]
    fn flat_is_the_default_and_old_scenes_stay_flat() {
        assert_eq!(Scene::default().view, View::Flat);
        let scene = Scene::from_ron("(name: \"Old\")").unwrap();
        assert!(scene.space().is_none());
    }

    #[test]
    fn space_settings_save_and_load() {
        for name in Container3d::NAMES {
            let space = Space { container: Container3d::default().with_name(name), ..Space::default() };
            let scene = Scene { view: View::Space(Box::new(space)), ..Scene::default() };
            let text = scene.to_ron().unwrap();
            assert_eq!(Scene::from_ron(&text).unwrap(), scene, "{name}");
        }
        // Missing 3D settings fill in with their defaults.
        let scene = Scene::from_ron("(view: Space((container: Cube(size: 300))))").unwrap();
        let space = scene.space().unwrap();
        assert_eq!(space.container, Container3d::Cube { size: 300.0 });
        assert_eq!(space.lights, Lights::default());
    }

    #[test]
    fn container_names_round_trip() {
        for name in Container3d::NAMES {
            assert_eq!(Container3d::default().with_name(name).name(), name);
        }
        for name in CameraMode::NAMES {
            assert_eq!(CameraMode::with_name(name).name(), name);
        }
    }

    #[test]
    fn orbiting_keeps_the_distance_and_stops_at_the_top() {
        let mut camera = Camera::default();
        let distance = camera.distance();
        camera.orbit(90.0, 0.0);
        assert!((camera.distance() - distance).abs() < 0.5);
        assert!(camera.position.x > 1000.0, "{:?}", camera.position);
        camera.orbit(0.0, 500.0);
        assert!((camera.distance() - distance).abs() < 0.5);
        // Never quite overhead, so the picture never flips.
        assert!(camera.position.y < 0.0 && camera.position.y > -distance);
        camera.zoom(0.5);
        assert!((camera.distance() - distance * 0.5).abs() < 0.5);
        let before = camera.target;
        camera.pan(Vec2::new(10.0, 0.0));
        assert!((camera.target - before).length() > 9.0);
        assert!((camera.distance() - distance * 0.5).abs() < 0.5);
    }

    #[test]
    fn camera_angles_round_trip() {
        let mut camera = Camera::default();
        camera.set_angles(30.0, 20.0, 1200.0);
        let (turn, height, distance) = camera.angles();
        assert!((turn - 30.0).abs() < 0.01 && (height - 20.0).abs() < 0.01 && (distance - 1200.0).abs() < 0.1);
    }

    #[test]
    fn fitted_cameras_frame_the_container() {
        for name in Container3d::NAMES {
            let container = Container3d::default().with_name(name);
            let camera = Camera::fit(&container, 9.0 / 16.0);
            assert!(camera.distance() > container.size(), "{name}");
            if name == "Tunnel" {
                // Inside the tube, looking down it.
                assert!(Vec2::new(camera.position.x, camera.position.y).length() < container.size());
                assert!(camera.target.z > camera.position.z);
            }
        }
    }

    #[test]
    fn rolled_cameras_and_lights_stay_sensible() {
        for seed in 0..200 {
            let mut dice = Dice::new(seed);
            for name in Container3d::NAMES {
                let mut space = Space { container: Container3d::default().with_name(name), ..Space::default() };
                space.roll_camera(&mut dice, 9.0 / 16.0);
                space.roll_lights(&mut dice);
                let camera = &space.camera;
                assert!(camera.position.is_finite() && camera.distance() > 10.0, "{name} {seed}");
                assert!((5.0..=150.0).contains(&camera.fov));
                assert!(space.key_direction().y < 0.0, "the key light comes from above");
            }
        }
    }

    #[test]
    fn key_light_comes_from_above() {
        let space = Space::default();
        let direction = space.key_direction();
        assert!(direction.y < 0.0, "{direction}");
        assert!((direction.length() - 1.0).abs() < 1e-5);
    }
}
