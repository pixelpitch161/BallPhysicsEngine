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
    /// The style's number in styled.wgsl's `form.y`: 1 lights the shape like a sphere.
    pub fn code(self) -> f32 {
        match self {
            Self::Glow | Self::Flat => 0.0,
            Self::Shiny => 1.0,
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
    /// How the ball is lit: Glow is a lit ball with a soft halo, Flat is one even colour and
    /// Shiny is lit like a sphere.
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
    /// styled shape: a glowing circle.
    pub fn is_classic_ball(&self) -> bool {
        self.style == Style::Glow && self.ball_shape == BallShape::Circle
    }

    /// The angle in degrees the pattern inside a classic ball moving at `velocity` turns by at
    /// `time` seconds, the way a styled ball's pattern turns with its shape: the shape's own
    /// Turn and Spin, and the way the ball goes when the pattern turns with the ball.
    pub fn inside_turn(&self, velocity: Vec2, time: f32) -> f32 {
        let mut angle = self.turn + self.spin * time;
        if self.inside.turns {
            angle += heading(velocity).to_angle().to_degrees() + 90.0;
        }
        angle
    }
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
    let turns = look.inside.turns;
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
        for gone in ["finish", "pattern_colours", "pattern_size", "face_motion", "inside", "inside_angle"] {
            assert!(defs.iter().all(|d| d.key != gone), "{gone} is gone");
        }
    }

    #[test]
    fn no_saved_settings_read_as_the_default_look() {
        assert_eq!(BasicLook::from_params(&bpe_core::Params::default()), BasicLook::default());
        assert!(BasicLook::default().to_params().0.is_empty(), "the defaults are not saved");
        let d = BasicLook::default();
        assert!(d.is_classic_ball(), "a new look is a glowing circle");
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
        // Turning with the ball points the shape's tip along the way it goes instead.
        let mut facing = BasicLook::default();
        facing.inside.turns = true;
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
        // A classic ball's pattern turns the same way on screen: a quarter turn for the heading
        // and a quarter turn more.
        assert!((facing.inside_turn(Vec2::new(0.0, 10.0), 0.0) - 180.0).abs() < 1e-4);
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
        let flat = BasicLook { style: Style::Flat, ..look.clone() };
        let glow = BasicLook { style: Style::Glow, ..look };
        for l in [flat, glow] {
            let i = instance(&l, BallColours::plain(&l, red), Vec2::ZERO, Vec2::ZERO, 20.0, 0.0, 3.0, 0.0);
            assert_eq!(i.form[1], 0.0, "{:?} is unlit", l.style);
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
