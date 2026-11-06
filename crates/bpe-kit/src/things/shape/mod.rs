//! The shape: an outline the balls bounce off from both sides, hollow or filled, moulded by hand
//! in the video. It replaces the old Arena.

pub mod draw;
pub mod foci;
pub mod sim;

pub use foci::foci;
pub use sim::{ShapeSim, ShapeState, place_on_edge, side_of, side_on};

use bpe_core::{Choice, SettingDef, Settings, ShapePath, SlotId, ThingKind};
use glam::Vec2;

pub struct ShapeKind;

/// The kind id.
pub const KIND: &str = "shape";

/// The heading the outline's settings sit under. The inspector shows them under the Look's Body
/// layer and leaves them off the thing's own settings.
pub const BODY_HEADINGS: [&str; 1] = ["Body"];

/// What a new shape's outline starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeStart {
    /// An even polygon of `sides` corners, which can be moulded.
    Polygon,
    #[default]
    Circle,
    /// An exact ellipse of `width` × `height`, turned by `turn`.
    Ellipse,
}

/// Whether a shape is a solid or a band.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeFill {
    /// Solid: balls bounce off its outside.
    Filled,
    /// A band: balls inside stay inside, balls outside stay outside.
    #[default]
    Hollow,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeSettings {
    /// An even polygon you can mould, a circle, or an ellipse.
    #[setting(label = "Start from", group = "Body", simple)]
    pub start: ShapeStart,
    /// How many corners the even polygon has. Changing it starts the outline afresh.
    #[setting(label = "Sides", group = "Body", when = "start=polygon", range(3, 64), simple)]
    pub sides: u32,
    /// The circle's radius, or how far a polygon's corners are from its middle. A moulded outline
    /// grows and shrinks with it.
    #[setting(
        label = "Size",
        group = "Body",
        when = "start!=ellipse",
        span(20, 2000),
        range(1, 20000),
        unit = "px",
        simple,
        keyframe
    )]
    pub size: f32,
    /// How wide the ellipse is, before it turns.
    #[setting(
        label = "Width",
        group = "Body",
        when = "start=ellipse",
        span(20, 2000),
        range(1, 20000),
        unit = "px",
        simple,
        keyframe
    )]
    pub width: f32,
    /// How tall the ellipse is, before it turns.
    #[setting(
        label = "Height",
        group = "Body",
        when = "start=ellipse",
        span(20, 2000),
        range(1, 20000),
        unit = "px",
        simple,
        keyframe
    )]
    pub height: f32,
    /// How far the ellipse is turned, clockwise.
    #[setting(label = "Turn", group = "Body", when = "start=ellipse", span(-180, 180), unit = "°", keyframe)]
    pub turn: f32,
    /// Solid, or a band balls can be on either side of.
    #[setting(label = "Fill", group = "Body", simple)]
    pub fill: ShapeFill,
    /// How thick the band is.
    #[setting(
        label = "Thickness",
        group = "Body",
        when = "fill=hollow",
        range(0.5, 400),
        unit = "px",
        simple,
        keyframe
    )]
    pub thickness: f32,
    /// The moulded outline, in the shape's own units. Empty: the even polygon.
    #[setting(label = "Corners", group = "Body", when = "start=polygon")]
    pub path: ShapePath,
    /// How fast the shape turns, clockwise.
    #[setting(label = "Spin", range(-360, 360), unit = "deg/s", simple, keyframe)]
    pub spin: f32,
    /// How much of its size the shape loses each second.
    #[setting(label = "Shrink", range(0, 1), unit = "per s", keyframe)]
    pub shrink: f32,
    /// The shape stops shrinking at this size.
    #[setting(label = "Smallest size", when = "shrink", range(10, 20000), unit = "px")]
    pub min_size: f32,
    /// Balls go through it instead of bouncing off: a zone a rule can watch them come into and
    /// leave (Enters / Leaves). Lights, lasers and the rest still see the shape.
    #[setting(label = "Balls pass through")]
    pub passes: bool,
}

impl Default for ShapeSettings {
    fn default() -> Self {
        Self {
            start: ShapeStart::Circle,
            sides: 6,
            size: 400.0,
            width: 900.0,
            height: 1300.0,
            turn: 0.0,
            fill: ShapeFill::Hollow,
            thickness: 8.0,
            path: ShapePath::default(),
            spin: 0.0,
            shrink: 0.0,
            min_size: 100.0,
            passes: false,
        }
    }
}

impl ShapeSettings {
    /// A hollow circle of `radius`.
    pub fn circle(radius: f32, thickness: f32) -> Self {
        Self { start: ShapeStart::Circle, size: radius, thickness, ..Self::default() }
    }

    /// A hollow even polygon whose corners are `radius` from the middle.
    pub fn polygon(sides: u32, radius: f32, thickness: f32) -> Self {
        Self { start: ShapeStart::Polygon, sides, size: radius, thickness, ..Self::default() }
    }

    /// A hollow `width` × `height` ellipse.
    pub fn ellipse(width: f32, height: f32, thickness: f32) -> Self {
        Self { start: ShapeStart::Ellipse, width, height, thickness, ..Self::default() }
    }

    /// World pixels per shape unit at full size: an ellipse's larger half-axis, else its Size.
    /// Smallest size and the shape's own units are measured in it.
    pub fn unit(&self) -> f32 {
        match self.start {
            ShapeStart::Ellipse => (self.width.max(self.height) * 0.5).max(1e-3),
            _ => self.size,
        }
    }

    /// An ellipse's half-axes in shape units (the larger one 1).
    pub fn ellipse_axes(&self) -> Vec2 {
        Vec2::new(self.width, self.height) * 0.5 / self.unit()
    }

    /// A hollow `width` × `height` rectangle: a moulded 4-corner outline. Its Size is a power of
    /// two at least half its diagonal, so the corners scale back to exactly these numbers.
    pub fn rectangle(width: f32, height: f32, thickness: f32) -> Self {
        let size = (width.hypot(height) * 0.5).max(1.0).log2().ceil().exp2();
        let (a, b) = (width * 0.5 / size, height * 0.5 / size);
        let path = ShapePath { points: vec![[-a, -b], [a, -b], [a, b], [-a, b]], bends: vec![None; 4] };
        Self { start: ShapeStart::Polygon, sides: 4, size, thickness, path, ..Self::default() }
    }

    /// The moulded outline when there is a usable one (a polygon with a path of 3 or more corners).
    pub fn moulded(&self) -> Option<&ShapePath> {
        (self.start == ShapeStart::Polygon && self.path.points.len() >= 3 && self.path.is_valid()).then_some(&self.path)
    }

    /// The outline in shape units, moulded or not: a circle is its 8 curved sides, an ellipse
    /// its 16, turned by its Turn.
    pub fn outline_path(&self) -> ShapePath {
        match (self.start, self.moulded()) {
            (ShapeStart::Ellipse, _) => ShapePath::ellipse(self.ellipse_axes(), self.turn.to_radians()),
            (_, Some(p)) => p.clone(),
            (ShapeStart::Polygon, None) => ShapePath::even(self.sides),
            (ShapeStart::Circle, None) => ShapePath::circle(),
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Bounce, SlotId::Sound, SlotId::Health];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Bounce];

impl ThingKind for ShapeKind {
    fn id(&self) -> &'static str {
        KIND
    }
    fn family(&self) -> &'static str {
        "Shapes"
    }
    fn name(&self) -> &'static str {
        "Shape"
    }
    fn icon(&self) -> &'static str {
        "shape"
    }
    fn help(&self) -> &'static str {
        "A shape the balls bounce off: hollow or filled, any outline you mould in the video."
    }
    fn settings(&self) -> &'static [SettingDef] {
        ShapeSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        DEFAULT_SLOTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ellipse_outline_path_is_16_curves_through_its_axes() {
        let mut s = ShapeSettings::ellipse(900.0, 1300.0, 3.0);
        s.turn = 30.0;
        let axes = s.ellipse_axes();
        assert!((axes - Vec2::new(450.0 / 650.0, 1.0)).length() < 1e-6, "{axes}");
        let path = s.outline_path();
        assert_eq!((path.side_count(), path.curve_count()), (16, 16));
        let turn = Vec2::from_angle(30f32.to_radians());
        for (k, want) in [(0, Vec2::new(0.0, -axes.y)), (4, Vec2::new(axes.x, 0.0)), (8, Vec2::new(0.0, axes.y))] {
            assert!((path.point(k) - turn.rotate(want)).length() < 1e-5, "corner {k}: {}", path.point(k));
        }
        for p in path.flatten(bpe_core::shape_path::FLATTEN) {
            let own = Vec2::new(turn.x, -turn.y).rotate(p) / axes;
            assert!((own.length() - 1.0).abs() < 1e-3, "{p} is off the ellipse");
        }
        assert_eq!(s.unit(), 650.0);
        assert_eq!(ShapeSettings::circle(300.0, 1.0).unit(), 300.0);
    }

    #[test]
    fn the_body_settings_sit_under_body_and_the_rest_do_not() {
        for d in ShapeSettings::defs() {
            let body =
                ["start", "sides", "size", "width", "height", "turn", "fill", "thickness", "path"].contains(&d.key);
            assert_eq!(d.group == BODY_HEADINGS[0], body, "{}", d.key);
        }
        let d = ShapeSettings::default();
        assert_eq!((d.start, d.size, d.fill, d.thickness), (ShapeStart::Circle, 400.0, ShapeFill::Hollow, 8.0));
    }

    #[test]
    fn a_rectangle_scales_back_to_its_exact_size() {
        for (w, h) in [(900.0, 1500.0), (1000.0, 1400.0), (700.0, 1400.0), (33.3, 71.9)] {
            let s = ShapeSettings::rectangle(w, h, 3.0);
            let half = s.moulded().unwrap().is_axis_rectangle().unwrap() * s.size;
            assert_eq!(half, Vec2::new(w * 0.5, h * 0.5));
        }
    }

    /// Place and Side read the shape's own corners, as By side does: side k is By side's slot k,
    /// and Place starts at the first corner, the top-most (the left of two at a flat top).
    #[test]
    fn place_and_side_follow_the_shapes_own_corners() {
        use crate::testing::SHAPE;
        use crate::{kit, new_scene};
        use bpe_core::resolve;
        let kit = kit();
        let check = |settings: ShapeSettings| {
            let mut scene = new_scene(&kit.registry);
            scene.things.get_mut(&SHAPE).unwrap().params = settings.to_params();
            scene.things.get_mut(&SHAPE).unwrap().transform.rotation = 0.3;
            let w = bpe_runtime::World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims);
            let path = settings.outline_path();
            let n = path.side_count();
            let mid = scene.things[&SHAPE].transform.position;
            let turn = Vec2::from_angle(0.3);
            let world_at = |q: Vec2| mid + turn.rotate(q * settings.size);
            // Every side's middle is on that side, as side_on has it, and the places go round
            // in order from the first corner.
            let mut last = -1.0;
            for side in 0..n {
                let p = world_at(path.side_point(side, 0.5));
                let (place, s, sides) = place_on_edge(&w, SHAPE, p).unwrap();
                assert_eq!((s, sides), (side, n), "{settings:?} side {side}");
                assert_eq!(s as u32, side_on(&w, SHAPE, &path, p));
                assert!(place > last, "{settings:?}: {place} after {last}");
                last = place;
            }
            let (start, _, _) = place_on_edge(&w, SHAPE, world_at(path.point(0))).unwrap();
            assert!(!(1e-3..=1.0 - 1e-3).contains(&start), "{settings:?}: the first corner is at {start}");
            (path, w, mid, turn)
        };
        // A square: its first corner the top left, a quarter of the way round at the top right.
        let (path, w, mid, turn) = check(ShapeSettings::polygon(4, 300.0, 3.0));
        let (q, _, _) = place_on_edge(&w, SHAPE, mid + turn.rotate(path.side_point(1, 0.01) * 300.0)).unwrap();
        assert!((q - 0.2525).abs() < 1e-3, "{q}");
        check(ShapeSettings::polygon(5, 300.0, 3.0));
        // A circle: eight sides from the top.
        let (path, w, mid, turn) = check(ShapeSettings::circle(300.0, 3.0));
        let _ = path;
        let (q, side, _) = place_on_edge(&w, SHAPE, mid + turn.rotate(Vec2::from_angle(0.05) * 300.0)).unwrap();
        let want = 0.25 + 0.05 / std::f32::consts::TAU;
        assert!((q - want).abs() < 1e-3 && side == 2, "just past 3 o'clock is a quarter round: {q} {side}");
        // A moulded outline with a curved side counts it as one side.
        let mut curved = ShapeSettings::polygon(5, 300.0, 3.0);
        let mut path = ShapePath::even(5);
        path.create_curve(1);
        curved.path = path;
        check(curved);
    }

    #[test]
    fn the_outline_path_follows_start_and_path() {
        assert_eq!(ShapeSettings::circle(10.0, 1.0).outline_path(), ShapePath::circle());
        assert_eq!(ShapeSettings::polygon(5, 10.0, 1.0).outline_path(), ShapePath::even(5));
        let mut moulded = ShapeSettings::polygon(5, 10.0, 1.0);
        moulded.path = ShapePath::even(7);
        assert_eq!(moulded.outline_path().side_count(), 7);
        // A circle ignores a leftover path.
        moulded.start = ShapeStart::Circle;
        assert_eq!(moulded.moulded(), None);
    }
}
