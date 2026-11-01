//! The look: a ball's colours, its shape and style (a glowing ball, one flat colour, or lit like
//! a sphere), a pattern inside each one, an outline, a halo and a flash where it hits. Other
//! things wear the same look and read only its colours, its inside pattern and its glow.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{Blend, Choice, Color, ModuleKind, SettingDef, Settings, SlotId};
use bpe_gfx::{LookInstance, PaintGpu, blend_alpha};
use glam::Vec2;

use crate::paint::{InsidePaint, Paint};

pub struct BasicLookKind;

/// The outline a ball is drawn with. Physics still treats every ball as a circle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BallShape {
    #[default]
    Circle,
    Square,
    Triangle,
    Diamond,
    Hexagon,
    Star,
    Heart,
    Donut,
}

impl BallShape {
    /// The shape's number in ball_look.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Circle => 0.0,
            Self::Square => 1.0,
            Self::Triangle => 2.0,
            Self::Diamond => 3.0,
            Self::Hexagon => 4.0,
            Self::Star => 5.0,
            Self::Heart => 6.0,
            Self::Donut => 7.0,
        }
    }
}

/// How a ball is lit: a glowing ball, one flat colour, or lit like a shiny sphere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Style {
    #[default]
    Glow,
    Flat,
    Shiny,
}

impl Style {
    /// The style's number in styled.wgsl's `form.y`: 0 one even colour, 1 lit like a sphere,
    /// 2 lit like the classic glowing ball.
    pub fn code(self) -> f32 {
        match self {
            Self::Flat => 0.0,
            Self::Shiny => 1.0,
            Self::Glow => 2.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BasicLook {
    /// The colours and what spreads them across the balls (or letters, or points).
    #[setting(
        paint,
        label = "Colours",
        prefix = "",
        group = "Colours",
        follows = "place|start|order|speed|hits|step|beat|time|path",
        simple
    )]
    pub fill: Paint,
    /// How the colour is laid out inside each ball, letter or body.
    #[setting(
        paint,
        label = "Inside each one",
        prefix = "in_",
        group = "Inside each one",
        when = "thing=balls|text|soft"
    )]
    pub inside: InsidePaint,
    /// The outline the ball is drawn with; it still bounces as a circle.
    #[setting(label = "Shape", when = "thing=balls", simple, random)]
    pub ball_shape: BallShape,
    /// How the ball is lit: Glow is the classic ball, lit from the upper left; Shiny is lit like
    /// a sphere; Flat is one even colour. Whatever the style, the halo is as bright as Glow says.
    #[setting(label = "Style", when = "thing=balls", group = "Style", simple, random)]
    pub style: Style,
    /// How wide the outline just inside the edge is; 0 means none.
    #[setting(label = "Outline", when = "thing=balls", range(0, 24), unit = "px", simple, keyframe, random)]
    pub outline: f32,
    /// How brightly the halo shines.
    #[setting(label = "Glow", when = "thing!=liquid", range(0, 2), simple, keyframe, random, pulse)]
    pub glow: f32,
    /// The outline's colours.
    #[setting(
        paint,
        label = "Outline colours",
        prefix = "outline_",
        group = "Shape",
        when = "thing=balls and outline",
        follows = "place|start|order|speed|hits|step|beat|time|path"
    )]
    pub outline_paint: Paint,
    /// How rounded the shape's corners are; circles ignore it.
    #[setting(
        label = "Roundness",
        when = "thing=balls and ball_shape!=circle",
        range(0, 1),
        group = "Shape",
        keyframe,
        random
    )]
    pub roundness: f32,
    /// Turns the shape to point the way the ball is going.
    #[setting(label = "Face motion", when = "thing=balls and ball_shape!=circle", group = "Shape", random)]
    pub face_motion: bool,
    /// The shape's starting angle; the pattern inside turns with it.
    #[setting(label = "Turn", when = "thing=balls", range(-180, 180), unit = "°", group = "Shape", keyframe, random)]
    pub turn: f32,
    /// How fast the shape turns; the pattern inside turns with it.
    #[setting(label = "Spin", when = "thing=balls", range(-720, 720), unit = "°/s", group = "Shape", keyframe, random)]
    pub spin: f32,
    /// How far the halo reaches, in ball radii.
    #[setting(label = "Glow size", when = "thing=balls", range(0.05, 4), group = "Light", keyframe, random)]
    pub glow_size: f32,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", when = "thing=balls", range(0, 1), group = "Light", keyframe, pulse)]
    pub opacity: f32,
    /// Full turns around the colour wheel per second, so the ball cycles through the rainbow.
    #[setting(label = "Rainbow", when = "thing=balls", range(-2, 2), unit = "turns/s", group = "Light", keyframe, random)]
    pub rainbow: f32,
    /// Every hit turns the colour this far round the colour wheel.
    #[setting(label = "Hue shift per hit", when = "thing=balls", range(-180, 180), unit = "deg", group = "Light", keyframe, random)]
    pub hue_per_hit: f32,
    /// How much fast balls stretch along their path and thin out across it.
    #[setting(label = "Stretch", when = "thing=balls", range(0, 1), group = "Motion", keyframe, random)]
    pub stretch: f32,
    /// How bright the flash on each hit is.
    #[setting(label = "Flash", when = "thing=balls", range(0, 3), group = "Flash", keyframe, random)]
    pub flash: f32,
    /// How big the flash is.
    #[setting(
        label = "Flash size",
        when = "thing=balls and flash",
        range(0, 300),
        unit = "px",
        group = "Flash",
        keyframe
    )]
    pub flash_size: f32,
    /// How long the flash lasts.
    #[setting(label = "Flash time", when = "thing=balls and flash", range(0.02, 2), unit = "s", group = "Flash")]
    pub flash_seconds: f32,
    /// How it mixes with what is under it: Normal paints over it, the others mix with it.
    /// Everything with the same Blend and amount is mixed in once, as one picture, so where its
    /// parts overlap they do not stack, unless Blend with each other is on.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if it weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
    /// Where its own parts overlap, they blend with each other too, as they do with what is
    /// under them; off, they cover each other and are mixed in as one picture.
    #[setting(
        label = "Blend with each other",
        when = "blend=add|screen|multiply|lighten|darken|subtract",
        group = "Blending"
    )]
    pub blend_each: bool,
}

impl Default for BasicLook {
    fn default() -> Self {
        Self {
            fill: Paint::default(),
            inside: InsidePaint::default(),
            ball_shape: BallShape::Circle,
            style: Style::Glow,
            outline: 0.0,
            glow: 0.12,
            outline_paint: Paint::default(),
            roundness: 0.15,
            face_motion: false,
            turn: 0.0,
            spin: 0.0,
            glow_size: 0.8,
            opacity: 1.0,
            rainbow: 0.0,
            hue_per_hit: 0.0,
            stretch: 0.0,
            flash: 0.4,
            flash_size: 26.0,
            flash_seconds: 0.25,
            blend: Blend::Normal,
            blend_amount: 1.0,
            blend_each: false,
        }
    }
}

impl BasicLook {
    /// The look's first colour, in linear light: what things that paint one colour show.
    pub fn first_colour(&self) -> [f32; 4] {
        self.fill.colours.first().to_linear()
    }

    /// Whether a ball wearing this look is drawn as the classic glowing ball rather than a
    /// styled shape: a glowing circle with nothing the classic ball can't draw (an outline, its
    /// own glow size, opacity, rainbow or stretch).
    pub fn is_classic_ball(&self) -> bool {
        let d = Self::default();
        self.style == Style::Glow
            && self.ball_shape == BallShape::Circle
            && self.outline <= 0.0
            && self.glow_size == d.glow_size
            && self.opacity >= 1.0
            && self.rainbow == 0.0
            && self.stretch <= 0.0
    }

    /// Whether the shape turns to point the way the ball goes. A circle has no point, so it
    /// never does, whatever the hidden Face motion says.
    pub fn faces_motion(&self) -> bool {
        self.face_motion && self.ball_shape != BallShape::Circle
    }

    /// The angle in degrees the pattern inside a ball moving at `velocity` turns by on screen at
    /// `time` seconds, on top of its own angle and spin: the shape's Turn and Spin, and the way
    /// the ball goes when the pattern turns with the ball. A classic ball adds it to its paint.
    pub fn inside_turn(&self, velocity: Vec2, time: f32) -> f32 {
        self.turn + self.spin * time + self.inside_heading(velocity)
    }

    /// What a styled ball adds to its paint's angle, which is measured on the turned shape: the
    /// way the ball goes when the pattern turns with the ball, less the shape's own turn to face
    /// its motion, so the pattern turns only when Turns with the ball says.
    pub fn inside_turn_on_shape(&self, velocity: Vec2) -> f32 {
        let faced = if self.faces_motion() { facing_degrees(velocity) } else { 0.0 };
        self.inside_heading(velocity) - faced
    }

    /// How far the pattern turns to follow the ball's way, in degrees: none unless it turns with
    /// the ball.
    fn inside_heading(&self, velocity: Vec2) -> f32 {
        if self.inside.turns { facing_degrees(velocity) } else { 0.0 }
    }
}

/// The turn, in degrees, that points something drawn pointing up the way `velocity` goes.
fn facing_degrees(velocity: Vec2) -> f32 {
    heading(velocity).to_angle().to_degrees() + 90.0
}

impl ModuleKind for BasicLookKind {
    fn id(&self) -> &'static str {
        "look.basic"
    }
    fn name(&self) -> &'static str {
        "Look"
    }
    fn help(&self) -> &'static str {
        "Colours, with a shape, a style, a pattern inside each one, an outline and a halo."
    }
    fn fits(&self, thing_kind: &str) -> bool {
        !matches!(thing_kind, "liquid" | "swarm")
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        BasicLook::defs()
    }
}

/// The speed at which a ball stretches to twice its length at full Stretch.
pub const STRETCH_SPEED: f32 = 900.0;

/// The way something moving at `velocity` goes; a still one faces right.
fn heading(velocity: Vec2) -> Vec2 {
    let speed = velocity.length();
    if speed > 1e-3 { velocity / speed } else { Vec2::X }
}

/// Turns a linear-light colour `turn` of the way around the colour wheel.
fn shift_linear_hue(color: [f32; 4], turn: f32) -> [f32; 4] {
    let shifted = Color::from_linear([color[0], color[1], color[2], 1.0]).shift_hue(turn).to_linear();
    [shifted[0], shifted[1], shifted[2], color[3]]
}

/// The colours one styled ball is painted with, in linear light: its fill, its outline, and
/// what is painted inside it (row -1 for nothing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallColours {
    pub fill: [f32; 4],
    pub outline: [f32; 4],
    pub paint: PaintGpu,
}

impl BallColours {
    /// `fill`, with the look's outline in its first colour and nothing painted inside.
    pub fn plain(look: &BasicLook, fill: [f32; 4]) -> Self {
        Self { fill, outline: look.outline_paint.colours.first().to_linear(), paint: PaintGpu::default() }
    }
}

/// One styled ball at `time` seconds in `colours`: `code` is its thing code and `seed` its own
/// number.
#[allow(clippy::too_many_arguments)]
pub fn instance(
    look: &BasicLook,
    colours: BallColours,
    center: Vec2,
    velocity: Vec2,
    radius: f32,
    time: f32,
    code: f32,
    seed: f32,
) -> LookInstance {
    let mut color = colours.fill;
    if look.rainbow != 0.0 {
        color = shift_linear_hue(color, look.rainbow * time);
    }
    let speed = velocity.length();
    let heading = heading(velocity);
    // Fast balls stretch along their path and thin out across it, keeping about the same area.
    let amount = look.stretch.clamp(0.0, 1.0) * (speed / STRETCH_SPEED).min(1.5);
    let along = 1.0 + amount;
    let across = 1.0 / along.sqrt();
    let turns = look.faces_motion();
    let stretches = amount > 1e-4 || turns;
    let dir = if stretches { heading } else { Vec2::X };
    let mut angle = (look.turn + look.spin * time).to_radians();
    if turns {
        // Shapes point up at no turn; a quarter turn points them along the motion.
        angle += FRAC_PI_2;
    } else if stretches {
        // Keep the shape and pattern still on screen while the quad lines up with the motion.
        angle -= heading.to_angle();
    }
    LookInstance {
        center: center.into(),
        radius,
        seed,
        color: blend_alpha(color),
        outline_color: blend_alpha(colours.outline),
        form: [look.ball_shape.code(), look.style.code(), 0.0, look.roundness.clamp(0.0, 1.0)],
        look: [look.outline.max(0.0), look.glow.max(0.0), look.opacity.clamp(0.0, 1.0), angle],
        stretch: [dir.x, dir.y, along, across],
        extent: [look.glow_size.max(0.05), code, 0.0, 0.0],
        paint: colours.paint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Depth, Value};

    #[test]
    fn a_look_has_five_simple_settings() {
        let defs = BasicLook::defs();
        let simple: Vec<&str> = defs.iter().filter(|d| d.depth == Depth::Simple).map(|d| d.key).collect();
        assert_eq!(simple, ["colours", "ball_shape", "style", "outline", "glow"]);
        let style = defs.iter().find(|d| d.key == "style").unwrap();
        assert_eq!((style.label, style.group, style.when), ("Style", "Style", "thing=balls"));
        assert_eq!(style.default, Value::Choice("glow".into()));
        assert_eq!(Style::NAMES, ["glow", "flat", "shiny"]);
        let pattern = defs.iter().find(|d| d.key == "in_pattern").unwrap();
        assert_eq!((pattern.label, pattern.group), ("Pattern", "Inside each one"));
        for gone in ["finish", "pattern_colours", "pattern_size", "inside", "inside_angle"] {
            assert!(defs.iter().all(|d| d.key != gone), "{gone} is gone");
        }
    }

    #[test]
    fn no_saved_settings_read_as_the_default_look() {
        assert_eq!(BasicLook::from_params(&bpe_core::Params::default()), BasicLook::default());
        assert!(BasicLook::default().to_params().0.is_empty(), "the defaults are not saved");
        let d = BasicLook::default();
        assert!(d.is_classic_ball(), "a new look is a glowing circle");
        // Anything the classic ball can't draw sends it down the styled path.
        for l in [
            BasicLook { outline: 3.0, ..d.clone() },
            BasicLook { glow_size: 1.2, ..d.clone() },
            BasicLook { opacity: 0.5, ..d.clone() },
            BasicLook { rainbow: 0.2, ..d.clone() },
            BasicLook { stretch: 0.5, ..d.clone() },
            BasicLook { style: Style::Shiny, ..d.clone() },
            BasicLook { ball_shape: BallShape::Star, ..d.clone() },
        ] {
            assert!(!l.is_classic_ball(), "{l:?}");
        }
        // Turning a circle needs no shape: the classic ball turns its pattern itself.
        assert!(BasicLook { turn: 40.0, spin: 30.0, ..d.clone() }.is_classic_ball());
        assert_eq!(d.inside.pattern, crate::paint::Pattern::None);
    }

    #[test]
    fn a_fast_ball_stretches_along_its_motion() {
        let look = BasicLook { stretch: 1.0, ..BasicLook::default() };
        let still = instance(&look, BallColours::plain(&look, [1.0; 4]), Vec2::ZERO, Vec2::ZERO, 20.0, 0.0, 3.0, 0.5);
        assert_eq!(still.stretch, [1.0, 0.0, 1.0, 1.0]);
        assert_eq!(still.extent, [0.8, 3.0, 0.0, 0.0]);
        assert_eq!(still.seed, 0.5);
        let fast = instance(
            &look,
            BallColours::plain(&look, [1.0; 4]),
            Vec2::ZERO,
            Vec2::new(0.0, STRETCH_SPEED),
            20.0,
            0.0,
            3.0,
            0.5,
        );
        assert_eq!(fast.stretch[..2], [0.0, 1.0]);
        assert!((fast.stretch[2] - 2.0).abs() < 1e-6);
        assert!((fast.stretch[3] - 1.0 / 2f32.sqrt()).abs() < 1e-6);
        // The pattern stays still on screen: the quad turned a quarter turn, the pattern turns
        // back.
        assert!((fast.look[3] + FRAC_PI_2).abs() < 1e-6);
        // Facing the motion points the shape's tip along the way it goes instead.
        let facing = BasicLook { face_motion: true, ball_shape: BallShape::Star, ..BasicLook::default() };
        let faced = instance(
            &facing,
            BallColours::plain(&facing, [1.0; 4]),
            Vec2::ZERO,
            Vec2::new(0.0, 10.0),
            20.0,
            0.0,
            3.0,
            0.5,
        );
        assert_eq!(faced.stretch, [0.0, 1.0, 1.0, 1.0]);
        assert!((faced.look[3] - FRAC_PI_2).abs() < 1e-6);
        // A circle has no tip: a hidden Face motion turns nothing.
        let circle = BasicLook { ball_shape: BallShape::Circle, ..facing.clone() };
        let still = instance(
            &circle,
            BallColours::plain(&circle, [1.0; 4]),
            Vec2::ZERO,
            Vec2::new(0.0, 10.0),
            20.0,
            0.0,
            3.0,
            0.5,
        );
        assert_eq!((still.stretch, still.look[3]), ([1.0, 0.0, 1.0, 1.0], 0.0));
        // Facing the motion turns the shape, not the pattern inside: the paint turns back.
        let down = Vec2::new(0.0, 10.0);
        assert!((facing.inside_turn_on_shape(down) + 180.0).abs() < 1e-4);
        // Turning with the ball turns only the pattern: a quarter turn for the heading and a
        // quarter turn more, on a classic ball as on a styled one.
        let mut turning = BasicLook::default();
        turning.inside.turns = true;
        assert!((turning.inside_turn(down, 0.0) - 180.0).abs() < 1e-4);
        assert!((turning.inside_turn_on_shape(down) - 180.0).abs() < 1e-4);
        let shape = instance(&turning, BallColours::plain(&turning, [1.0; 4]), Vec2::ZERO, down, 20.0, 0.0, 3.0, 0.5);
        assert_eq!((shape.stretch, shape.look[3]), ([1.0, 0.0, 1.0, 1.0], 0.0), "the shape stays put");
        let both = BasicLook { face_motion: true, ball_shape: BallShape::Star, ..turning };
        assert!(both.inside_turn_on_shape(down).abs() < 1e-4, "the pattern turns with the shape");
        assert_eq!(BasicLook { turn: 30.0, spin: 10.0, ..BasicLook::default() }.inside_turn(Vec2::X, 2.0), 50.0);
    }

    #[test]
    fn the_rainbow_turns_the_colours_with_time_and_shapes_keep_their_codes() {
        let look = BasicLook { rainbow: 0.5, ball_shape: BallShape::Star, style: Style::Shiny, ..BasicLook::default() };
        let red = Color::rgb(255, 0, 0).to_linear();
        let at0 = instance(&look, BallColours::plain(&look, red), Vec2::ZERO, Vec2::ZERO, 20.0, 0.0, 3.0, 0.0);
        let at1 = instance(&look, BallColours::plain(&look, red), Vec2::ZERO, Vec2::ZERO, 20.0, 1.0, 3.0, 0.0);
        assert_eq!(at0.color, red);
        // Half a turn around the wheel from red is cyan.
        assert_eq!(Color::from_linear(at1.color), Color::rgb(0, 255, 255));
        assert_eq!(at0.form[..2], [5.0, 1.0]);
        for (style, code) in [(Style::Flat, 0.0), (Style::Glow, 2.0)] {
            let l = BasicLook { style, ..look.clone() };
            let i = instance(&l, BallColours::plain(&l, red), Vec2::ZERO, Vec2::ZERO, 20.0, 0.0, 3.0, 0.0);
            assert_eq!(i.form[1], code, "{style:?}");
        }
        // An additive colour keeps its mark.
        let add = instance(
            &BasicLook::default(),
            BallColours::plain(&BasicLook::default(), [1.0, 0.0, 0.0, 0.0]),
            Vec2::ZERO,
            Vec2::ZERO,
            20.0,
            0.0,
            3.0,
            0.0,
        );
        assert_eq!(add.color[3], -1.0);
    }
}
