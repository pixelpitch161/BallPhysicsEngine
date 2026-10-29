//! What to draw for one frame, built on the CPU by each thing's drawing part.

use std::collections::BTreeMap;

use bpe_core::{Resolved, ResolvedThing, Scene, Thing};
use bpe_runtime::{Log, Outline, World};
use bytemuck::{Pod, Zeroable};

/// A ball: a lit sphere with a halo.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BallInstance {
    pub center: [f32; 2],
    pub radius: f32,
    /// How brightly the halo shines.
    pub glow: f32,
    /// Linear light. An additive colour has alpha -1, see [`blend_alpha`].
    pub color: [f32; 4],
}

/// A wall outline, drawn as an instanced quad with a distance function. `c.x` picks the shape:
/// 0 ring or polygon, 1 box.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ShapeInstance {
    /// Centre, angle.
    pub a: [f32; 4],
    /// Ring: radius, sides. Box: half width, half height.
    pub b: [f32; 4],
    /// Kind, half line width, glow strength.
    pub c: [f32; 4],
    /// Linear-light colour. Alpha scales the line, not the glow. Below zero the line is added
    /// light (see [`blend_alpha`]).
    pub color: [f32; 4],
}

impl ShapeInstance {
    pub fn outline(outline: &Outline, color: [f32; 4], glow: f32) -> Self {
        let color = blend_alpha(color);
        match *outline {
            Outline::Ring { center, radius, sides, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [radius, sides as f32, 0.0, 0.0],
                c: [0.0, thickness * 0.5, glow, 0.0],
                color,
            },
            Outline::Box { center, half, angle, thickness } => Self {
                a: [center.x, center.y, angle, 0.0],
                b: [half.x, half.y, 0.0, 0.0],
                c: [1.0, thickness * 0.5, glow, 0.0],
                color,
            },
        }
    }
}

/// A soft additive glow where something hit. The colour carries its strength.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FlashInstance {
    pub center: [f32; 2],
    pub size: f32,
    pub color: [f32; 4],
}

/// A colour with alpha 0 but some light is an additive colour (the colour picker's "Additive"
/// blending) and gets an alpha of -1. The shaders read a negative alpha as "add this much light
/// and cover nothing", so it can still fade like any other alpha.
pub fn blend_alpha(c: [f32; 4]) -> [f32; 4] {
    if c[3] == 0.0 && (c[0] > 0.0 || c[1] > 0.0 || c[2] > 0.0) { [c[0], c[1], c[2], -1.0] } else { c }
}

/// Everything the renderer needs for one frame: the picture's globals and the instance lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The video's size in pixels; world coordinates are these pixels, origin at the centre, y
    /// down.
    pub width: u32,
    pub height: u32,
    pub time: f32,
    pub background_top: [f32; 4],
    pub background_bottom: [f32; 4],
    pub bloom: f32,
    pub exposure: f32,
    pub shapes: Vec<ShapeInstance>,
    pub balls: Vec<BallInstance>,
    pub flashes: Vec<FlashInstance>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1920,
            time: 0.0,
            background_top: [0.0, 0.0, 0.0, 1.0],
            background_bottom: [0.0, 0.0, 0.0, 1.0],
            bloom: 0.0,
            exposure: 1.0,
            shapes: Vec::new(),
            balls: Vec::new(),
            flashes: Vec::new(),
        }
    }
}

impl Frame {
    /// Empties the instance lists and keeps the globals.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.balls.clear();
        self.flashes.clear();
    }
}

/// What a thing's drawing part can read.
pub struct DrawCx<'a> {
    pub thing: &'a Thing,
    pub resolved: &'a ResolvedThing,
    pub world: &'a World,
    pub log: &'a Log,
    /// Seconds into the clip being drawn.
    pub time: f64,
}

/// How a thing kind draws itself.
pub trait ThingDraw: Send + Sync + 'static {
    /// The thing kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    fn draw(&self, cx: &DrawCx, frame: &mut Frame);
}

/// Every registered drawing part, by kind id.
#[derive(Default)]
pub struct Draws {
    things: BTreeMap<&'static str, Box<dyn ThingDraw>>,
}

impl Draws {
    pub fn add(&mut self, d: impl ThingDraw) {
        let id = d.kind();
        assert!(!self.things.contains_key(id), "draw registered twice: {id}");
        self.things.insert(id, Box::new(d));
    }

    pub fn thing(&self, kind: &str) -> Option<&dyn ThingDraw> {
        self.things.get(kind).map(|d| d.as_ref())
    }
}

/// Fills `frame` for `time`: the lists are emptied, then every resolved thing in id order
/// whose kind has a drawing part draws.
pub fn build_frame(
    scene: &Scene,
    resolved: &Resolved,
    world: &World,
    log: &Log,
    time: f64,
    draws: &Draws,
    frame: &mut Frame,
) {
    frame.clear();
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        let Some(d) = draws.thing(&r.kind) else { continue };
        d.draw(&DrawCx { thing, resolved: r, world, log, time }, frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Value, resolve, testkit};
    use bpe_runtime::{Log, Sims, World};

    struct FakeScene;
    impl ThingDraw for FakeScene {
        fn kind(&self) -> &'static str {
            "scene"
        }
        fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
            frame.width = 100;
            frame.height = 200;
            frame.time = cx.time as f32;
            frame.bloom = 0.5;
        }
    }

    struct FakeBalls;
    impl ThingDraw for FakeBalls {
        fn kind(&self) -> &'static str {
            "balls"
        }
        fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
            let radius = match cx.resolved.params.get("radius") {
                Some(Value::Number(r)) => *r as f32,
                _ => 0.0,
            };
            frame.balls.push(BallInstance { center: [1.0, 2.0], radius, glow: 0.0, color: [1.0; 4] });
            frame.flashes.push(FlashInstance { center: [0.0; 2], size: 1.0, color: [1.0; 4] });
        }
    }

    #[test]
    fn build_frame_fills_from_each_thing_and_starts_clean() {
        let reg = testkit::registry();
        let mut scene = bpe_core::Scene::empty(&reg);
        let balls = testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
        scene.things.get_mut(&balls).unwrap().params.set("radius", Value::Number(9.0));
        testkit::push(&mut scene, "folder", bpe_core::ThingId::ROOT);
        let resolved = resolve(&scene, &reg);
        let sims = Sims::default();
        let world = World::build(&scene, &resolved, &sims);
        let mut draws = Draws::default();
        draws.add(FakeScene);
        draws.add(FakeBalls);
        let mut frame = Frame::default();
        for _ in 0..2 {
            build_frame(&scene, &resolved, &world, &Log::default(), 1.5, &draws, &mut frame);
        }
        assert_eq!((frame.width, frame.height, frame.time, frame.bloom), (100, 200, 1.5, 0.5));
        assert_eq!(frame.balls.len(), 1, "lists are cleared between frames");
        assert_eq!(frame.balls[0].radius, 9.0);
        assert_eq!(frame.flashes.len(), 1);
        assert!(frame.shapes.is_empty());
        assert!(draws.thing("folder").is_none());
    }

    #[test]
    fn outlines_and_blending_follow_the_old_renderer() {
        use bpe_runtime::Outline;
        let ring =
            Outline::Ring { center: glam::Vec2::new(1.0, 2.0), radius: 50.0, sides: 6, angle: 0.3, thickness: 4.0 };
        let s = ShapeInstance::outline(&ring, [1.0, 0.5, 0.0, 1.0], 0.1);
        assert_eq!(s.a, [1.0, 2.0, 0.3, 0.0]);
        assert_eq!(s.b, [50.0, 6.0, 0.0, 0.0]);
        assert_eq!(s.c, [0.0, 2.0, 0.1, 0.0]);
        let b = Outline::Box { center: glam::Vec2::ZERO, half: glam::Vec2::new(3.0, 4.0), angle: 0.0, thickness: 2.0 };
        assert_eq!(ShapeInstance::outline(&b, [1.0; 4], 0.0).c, [1.0, 1.0, 0.0, 0.0]);
        assert_eq!(blend_alpha([1.0, 0.0, 0.0, 0.0]), [1.0, 0.0, 0.0, -1.0]);
        assert_eq!(blend_alpha([0.0, 0.0, 0.0, 0.0]), [0.0; 4]);
        assert_eq!(blend_alpha([0.2, 0.0, 0.0, 1.0]), [0.2, 0.0, 0.0, 1.0]);
    }
}
