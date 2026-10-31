//! The styled look: a shape, a fill pattern, an outline and a halo per ball, with the classic
//! look's flash. Shares the classic look's keys (colours, glow, flash) so the balls' hit flashes
//! and colours work the same whichever look they wear.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{Blend, Choice, Color, Gradient, ModuleKind, SettingDef, Settings, SlotId};
use bpe_gfx::{LookInstance, PaintGpu, blend_alpha};
use glam::Vec2;

use crate::paint::Paint;

pub struct StyledLookKind;

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

/// How a ball is coloured in: one colour, lit like a sphere, or a pattern of two colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BallFill {
    Solid,
    #[default]
    Shiny,
    Radial,
    Linear,
    Stripes,
    Rings,
    Dots,
    Checker,
    Halves,
    Slices,
    Swirl,
    Face,
}

impl BallFill {
    /// The fill's number in ball_look.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Shiny => 1.0,
            Self::Radial => 2.0,
            Self::Linear => 3.0,
            Self::Stripes => 4.0,
            Self::Rings => 5.0,
            Self::Dots => 6.0,
            Self::Checker => 7.0,
            Self::Halves => 8.0,
            Self::Slices => 9.0,
            Self::Swirl => 10.0,
            Self::Face => 11.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct StyledLook {
    /// The ball's colours and what spreads them across the balls, with an optional pattern
    /// painted inside each one.
    #[setting(
        paint,
        label = "Colours",
        prefix = "",
        group = "Colours",
        follows = "place|start|order|speed|hits|step|beat|time|path",
        inside,
        simple
    )]
    pub fill: Paint,
    /// The outline the ball is drawn with; it still bounces as a circle.
    #[setting(label = "Shape", when = "thing=balls", simple, random)]
    pub ball_shape: BallShape,
    /// How the ball is coloured in: one colour, lit like a sphere, or a pattern of two colours.
    #[setting(label = "Fill", when = "thing=balls", simple, random)]
    pub finish: BallFill,
    /// The pattern's other colours.
    #[setting(
        paint,
        label = "Pattern colours",
        prefix = "pattern_",
        group = "Pattern",
        when = "thing=balls and finish!=solid|shiny",
        follows = "place|start|order|speed|hits|step|beat|time|path",
        simple
    )]
    pub pattern: Paint,
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
    /// How many stripes, rings, dots or slices fit across the ball.
    #[setting(
        label = "Pattern size",
        when = "thing=balls and finish=stripes|rings|dots|checker|slices|swirl",
        range(0.5, 24),
        group = "Shape",
        keyframe,
        random
    )]
    pub pattern_size: f32,
    /// The shape's and pattern's starting angle.
    #[setting(label = "Turn", when = "thing=balls", range(-180, 180), unit = "°", group = "Shape", keyframe, random)]
    pub turn: f32,
    /// How fast the shape and pattern turn.
    #[setting(label = "Spin", when = "thing=balls", range(-720, 720), unit = "°/s", group = "Shape", keyframe, random)]
    pub spin: f32,
    /// Turns the shape to point the way the ball is going.
    #[setting(label = "Face motion", when = "thing=balls", group = "Shape", random)]
    pub face_motion: bool,
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

impl Default for StyledLook {
    fn default() -> Self {
        Self {
            fill: Paint::default(),
            ball_shape: BallShape::Circle,
            finish: BallFill::Shiny,
            pattern: Paint { colours: Gradient::solid(Color::rgb(0x20, 0x20, 0x40)), ..Paint::default() },
            outline: 0.0,
            glow: 0.12,
            outline_paint: Paint::default(),
            roundness: 0.15,
            pattern_size: 4.0,
            turn: 0.0,
            spin: 0.0,
            face_motion: false,
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

impl ModuleKind for StyledLookKind {
    fn id(&self) -> &'static str {
        "look.styled"
    }
    fn name(&self) -> &'static str {
        "Styled"
    }
    fn help(&self) -> &'static str {
        "A ball with its own shape, fill pattern, outline and halo, stretched as it speeds up."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        StyledLook::defs()
    }
}

/// The speed at which a ball stretches to twice its length at full Stretch.
pub const STRETCH_SPEED: f32 = 900.0;

/// Turns a linear-light colour `turn` of the way around the colour wheel.
fn shift_linear_hue(color: [f32; 4], turn: f32) -> [f32; 4] {
    let shifted = Color::from_linear([color[0], color[1], color[2], 1.0]).shift_hue(turn).to_linear();
    [shifted[0], shifted[1], shifted[2], color[3]]
}

/// The colours one styled ball is painted with, in linear light: its fill, its pattern's other
/// colour, its outline, and the gradient painted inside it (row -1 for none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallColours {
    pub fill: [f32; 4],
    pub pattern: [f32; 4],
    pub outline: [f32; 4],
    pub paint: PaintGpu,
}

impl BallColours {
    /// `fill`, with the look's pattern and outline in their first colours and nothing painted
    /// inside.
    pub fn plain(look: &StyledLook, fill: [f32; 4]) -> Self {
        Self {
            fill,
            pattern: look.pattern.colours.first().to_linear(),
            outline: look.outline_paint.colours.first().to_linear(),
            paint: PaintGpu::default(),
        }
    }
}

/// One styled ball at `time` seconds in `colours`: `code` is its thing code and `seed` its own
/// number.
#[allow(clippy::too_many_arguments)]
pub fn instance(
    look: &StyledLook,
    colours: BallColours,
    center: Vec2,
    velocity: Vec2,
    radius: f32,
    time: f32,
    code: f32,
    seed: f32,
) -> LookInstance {
    let mut color = colours.fill;
    let mut color2 = colours.pattern;
    if look.rainbow != 0.0 {
        let turn = look.rainbow * time;
        color = shift_linear_hue(color, turn);
        color2 = shift_linear_hue(color2, turn);
    }
    let speed = velocity.length();
    let heading = if speed > 1e-3 { velocity / speed } else { Vec2::X };
    // Fast balls stretch along their path and thin out across it, keeping about the same area.
    let amount = look.stretch.clamp(0.0, 1.0) * (speed / STRETCH_SPEED).min(1.5);
    let along = 1.0 + amount;
    let across = 1.0 / along.sqrt();
    let stretches = amount > 1e-4 || look.face_motion;
    let dir = if stretches { heading } else { Vec2::X };
    let mut angle = (look.turn + look.spin * time).to_radians();
    if look.face_motion {
        // Shapes point up at no turn; a quarter turn points them along the motion.
        angle += FRAC_PI_2;
    } else if stretches {
        // Keep the pattern still on screen while the quad lines up with the motion.
        angle -= heading.to_angle();
    }
    LookInstance {
        center: center.into(),
        radius,
        seed,
        color: blend_alpha(color),
        color2: blend_alpha(color2),
        outline_color: blend_alpha(colours.outline),
        form: [look.ball_shape.code(), look.finish.code(), look.pattern_size.max(0.1), look.roundness.clamp(0.0, 1.0)],
        look: [look.outline.max(0.0), look.glow.max(0.0), look.opacity.clamp(0.0, 1.0), angle],
        stretch: [dir.x, dir.y, along, across],
        extent: [look.glow_size.max(0.05), code, 0.0, 0.0],
        paint: colours.paint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::look::ClassicLook;
    use bpe_core::Depth;

    #[test]
    fn a_styled_look_has_six_simple_settings_and_shares_the_classic_keys() {
        let defs = StyledLook::defs();
        let simple: Vec<&str> = defs.iter().filter(|d| d.depth == Depth::Simple).map(|d| d.key).collect();
        assert_eq!(simple, ["colours", "ball_shape", "finish", "pattern_colours", "outline", "glow"]);
        for classic in ClassicLook::defs() {
            let styled = defs.iter().find(|d| d.key == classic.key).unwrap_or_else(|| panic!("{}", classic.key));
            assert_eq!(styled.ty, classic.ty, "{}", classic.key);
            assert_eq!(styled.default, classic.default, "{}", classic.key);
        }
        // The classic look reads a styled look's flash settings as its own.
        let params = StyledLook { flash: 2.0, glow: 0.5, ..StyledLook::default() }.to_params();
        let classic = ClassicLook::from_params(&params);
        assert_eq!((classic.flash, classic.glow), (2.0, 0.5));
    }

    #[test]
    fn no_saved_settings_read_as_the_default_look() {
        assert_eq!(StyledLook::from_params(&bpe_core::Params::default()), StyledLook::default());
        assert!(StyledLook::default().to_params().0.is_empty(), "the defaults are not saved");
        // The pattern's own navy is its setting's default too.
        let pattern = StyledLook::defs().iter().find(|d| d.key == "pattern_colours").unwrap();
        assert_eq!(pattern.default, bpe_core::Value::Gradient(Gradient::solid(Color::rgb(0x20, 0x20, 0x40))));
    }

    #[test]
    fn a_fast_ball_stretches_along_its_motion() {
        let look = StyledLook { stretch: 1.0, ..StyledLook::default() };
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
        // Facing the motion turns the shape's tip along the way it goes instead.
        let facing = StyledLook { face_motion: true, ..StyledLook::default() };
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
    }

    #[test]
    fn the_rainbow_turns_the_colours_with_time_and_shapes_keep_their_codes() {
        let look =
            StyledLook { rainbow: 0.5, ball_shape: BallShape::Star, finish: BallFill::Dots, ..StyledLook::default() };
        let red = Color::rgb(255, 0, 0).to_linear();
        let at0 = instance(&look, BallColours::plain(&look, red), Vec2::ZERO, Vec2::ZERO, 20.0, 0.0, 3.0, 0.0);
        let at1 = instance(&look, BallColours::plain(&look, red), Vec2::ZERO, Vec2::ZERO, 20.0, 1.0, 3.0, 0.0);
        assert_eq!(at0.color, red);
        // Half a turn around the wheel from red is cyan.
        assert_eq!(Color::from_linear(at1.color), Color::rgb(0, 255, 255));
        assert_eq!(at0.form[..2], [5.0, 6.0]);
        // An additive colour keeps its mark.
        let add = instance(
            &look,
            BallColours::plain(&look, [1.0, 0.0, 0.0, 0.0]),
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
