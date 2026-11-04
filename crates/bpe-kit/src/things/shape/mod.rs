//! The shape: an outline the balls bounce off from both sides, hollow or filled, moulded by hand
//! in the video. It replaces the old Arena.

pub mod draw;
pub mod sim;

pub use sim::{ShapeSim, ShapeState};

use bpe_core::{Choice, SettingDef, Settings, ShapePath, SlotId, ThingKind};

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
    /// An even polygon you can mould, or a circle.
    #[setting(label = "Start from", group = "Body", simple)]
    pub start: ShapeStart,
    /// How many corners the even polygon has. Changing it starts the outline afresh.
    #[setting(label = "Sides", group = "Body", when = "start=polygon", range(3, 64), simple)]
    pub sides: u32,
    /// The circle's radius, or how far a polygon's corners are from its middle. A moulded outline
    /// grows and shrinks with it.
    #[setting(label = "Size", group = "Body", span(20, 2000), range(1, 20000), unit = "px", simple, keyframe)]
    pub size: f32,
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
}

impl Default for ShapeSettings {
    fn default() -> Self {
        Self {
            start: ShapeStart::Circle,
            sides: 6,
            size: 400.0,
            fill: ShapeFill::Hollow,
            thickness: 8.0,
            path: ShapePath::default(),
            spin: 0.0,
            shrink: 0.0,
            min_size: 100.0,
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

    /// The outline in shape units, moulded or not: a circle is its 8 curved sides.
    pub fn outline_path(&self) -> ShapePath {
        match (self.start, self.moulded()) {
            (_, Some(p)) => p.clone(),
            (ShapeStart::Polygon, None) => ShapePath::even(self.sides),
            (ShapeStart::Circle, None) => ShapePath::circle(),
        }
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Bounce, SlotId::Particles, SlotId::Sound];
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
    use glam::Vec2;

    #[test]
    fn the_body_settings_sit_under_body_and_the_rest_do_not() {
        for d in ShapeSettings::defs() {
            let body = ["start", "sides", "size", "fill", "thickness", "path"].contains(&d.key);
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
