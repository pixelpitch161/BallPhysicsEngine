//! Light: a lamp that lights the scene when it has a Lighting card (lighting spec, part 1a).
//! A Point, Spot, Sun, Area or Strip light, with soft shadows. Inside a Balls, Emitter or Shape
//! thing it rides on it: on each ball, the middle, one ball, or a point on a shape's edge. A
//! light is drawn only as light: it has no sim and never touches the physics.

pub mod draw;

use bpe_core::{Choice, Color, KindId, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

/// The thing kind's id.
pub const LIGHT: &str = "light";

pub struct LightKind;

/// What kind of lamp it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LightType {
    /// Light from one point, all round.
    #[default]
    Point,
    /// Light from one point inside a cone.
    Spot,
    /// Parallel light from far away, from one direction.
    Sun,
    /// A lit rectangle or disc: soft light from a panel.
    Area,
    /// A neon tube: light along a line or a shape's outline.
    Strip,
}

impl LightType {
    /// Its number in lighting.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Point => 0.0,
            Self::Spot => 1.0,
            Self::Sun => 2.0,
            Self::Area => 3.0,
            Self::Strip => 4.0,
        }
    }

    /// The light's name in the editor's "Add thing" menu: "Point light", "Sun".
    pub fn menu_name(self) -> &'static str {
        match self {
            Self::Point => "Point light",
            Self::Spot => "Spot light",
            Self::Sun => "Sun",
            Self::Area => "Area light",
            Self::Strip => "Strip light",
        }
    }

    pub const ALL: [LightType; 5] = [Self::Point, Self::Spot, Self::Sun, Self::Area, Self::Strip];
}

/// Where the light's colour comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LightColour {
    /// A colour you pick.
    #[default]
    Own,
    /// The colour of a lamp at a temperature, in kelvin: low is warm, high is cold.
    Temperature,
}

/// How the light fades with distance, out to its Reach.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Falloff {
    /// Bright near the lamp, falling away like real light.
    #[default]
    Real,
    /// A soft, even pool that fades out towards the edge.
    Soft,
    /// Straight down from the lamp to nothing at the edge.
    Linear,
    /// The same everywhere within reach.
    None,
}

impl Falloff {
    pub fn code(self) -> f32 {
        match self {
            Self::Real => 0.0,
            Self::Soft => 1.0,
            Self::Linear => 2.0,
            Self::None => 3.0,
        }
    }
}

/// What a light inside a Balls, Emitter or Shape thing rides on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum RidesOn {
    /// One light on every ball.
    #[default]
    EachBall,
    /// The middle of the group, or of the shape.
    TheMiddle,
    /// One ball, by its number.
    BallNumber,
    /// A point on the shape's edge.
    APointOnTheEdge,
}

/// Which way a Spot, Sun, Area or Strip points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Points {
    /// The Direction you set.
    #[default]
    FixedAngle,
    /// The way the ball it rides is moving (else the Direction).
    WhereItsGoing,
    /// Round and round, from the Direction, at Spin speed.
    Spins,
}

/// The shape of an Area light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AreaShape {
    #[default]
    Rectangle,
    Disc,
}

/// What a Strip light runs along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum StripAlong {
    /// A straight tube of its Length, at its Direction.
    #[default]
    Line,
    /// The outline of a shape.
    AShapesOutline,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LightSettings {
    /// What kind of lamp it is.
    #[setting(label = "Kind", simple, group = "Light")]
    pub kind: LightType,
    /// Where its colour comes from.
    #[setting(label = "Colour from", group = "Light")]
    pub colour_from: LightColour,
    /// The light's colour.
    #[setting(label = "Colour", when = "colour_from=own", simple, keyframe, group = "Light")]
    pub colour: Color,
    /// The lamp's temperature: about 2700 a warm bulb, 5500 daylight, 9000 a blue sky.
    #[setting(
        label = "Temperature",
        when = "colour_from=temperature",
        span(1500, 12000),
        unit = "K",
        keyframe,
        group = "Light"
    )]
    pub kelvin: f32,
    /// How bright it is. Below 0 it takes light away.
    #[setting(label = "Strength", span(0, 4), simple, keyframe, pulse, group = "Light")]
    pub strength: f32,
    /// How far its light reaches.
    #[setting(
        label = "Reach",
        when = "kind!=sun",
        range(0, 1e9),
        span(0, 3000),
        unit = "px",
        simple,
        keyframe,
        group = "Light"
    )]
    pub reach: f32,
    /// How the light fades out to its reach.
    #[setting(label = "Falloff", when = "kind!=sun", group = "Light")]
    pub falloff: Falloff,
    /// The lamp's own size: a bigger lamp casts softer shadows.
    #[setting(label = "Size", when = "kind!=sun", range(0, 1e9), span(0, 200), unit = "px", keyframe, group = "Light")]
    pub size: f32,
    /// How high above the picture the lamp hangs: low lamps light balls from the side.
    #[setting(label = "Height", when = "kind!=sun", span(0, 1000), unit = "px", keyframe, group = "Light")]
    pub height: f32,
    /// How high in the sky the sun is: low suns light balls from the side.
    #[setting(label = "Elevation", when = "kind=sun", span(0, 90), unit = "°", keyframe, group = "Light")]
    pub elevation: f32,
    /// How big the sun looks: a bigger sun casts softer shadows.
    #[setting(label = "Sun size", when = "kind=sun", span(0, 20), unit = "°", keyframe, group = "Light")]
    pub sun_size: f32,
    /// How wide the spot's cone is, edge to edge.
    #[setting(
        label = "Cone width",
        when = "kind=spot",
        range(0, 360),
        span(1, 180),
        unit = "°",
        keyframe,
        group = "Light"
    )]
    pub cone: f32,
    /// How soft the cone's edge is.
    #[setting(label = "Cone edge", when = "kind=spot", span(0, 60), unit = "°", keyframe, group = "Light")]
    pub cone_edge: f32,
    /// What a Strip runs along.
    #[setting(label = "Runs along", when = "kind=strip", group = "Light")]
    pub strip_along: StripAlong,
    /// The shape whose outline the strip follows.
    #[setting(label = "Shape", when = "kind=strip and strip_along=a_shapes_outline", group = "Light")]
    pub strip_shape: Target,
    /// The panel's or tube's width.
    #[setting(
        label = "Width",
        when = "kind=area or kind=strip",
        range(0, 1e9),
        span(0, 1000),
        unit = "px",
        keyframe,
        group = "Light"
    )]
    pub width: f32,
    /// The panel's or tube's length.
    #[setting(
        label = "Length",
        when = "kind=area or kind=strip and strip_along=line",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Light"
    )]
    pub length: f32,
    /// The panel's shape.
    #[setting(label = "Area shape", when = "kind=area", group = "Light")]
    pub area_shape: AreaShape,
    /// Which way it points, in degrees clockwise from the right: 90 is down.
    #[setting(label = "Direction", when = "kind!=point", span(-180, 180), unit = "°", keyframe, group = "Light")]
    pub direction: f32,
    /// What decides which way it points.
    #[setting(label = "Points", when = "kind!=point", group = "Light")]
    pub points: Points,
    /// How fast it turns round when it spins.
    #[setting(label = "Spin", when = "kind!=point and points=spins", span(-360, 360), unit = "°/s", keyframe, group = "Light")]
    pub spin: f32,
    /// Where it is: in the world, or, riding a thing, from the ball or point it rides.
    #[setting(label = "Position", unit = "px", keyframe, group = "Place")]
    pub position: Vec2,
    /// Inside a Balls, Emitter or Shape thing: what it rides on.
    #[setting(label = "Rides on", group = "Place")]
    pub rides_on: RidesOn,
    /// The ball it rides, by its number in the group.
    #[setting(label = "Ball number", when = "rides_on=ball_number", range(1, 1e9), span(1, 100), group = "Place")]
    pub ball_number: u32,
    /// Where round the shape's edge it rides, from 0 (its top) to 1 (all the way round).
    #[setting(label = "Round the edge", when = "rides_on=a_point_on_the_edge", span(0, 1), keyframe, group = "Place")]
    pub edge_at: f32,
    /// Whether things that block light throw shadows from it.
    #[setting(label = "Casts shadows", group = "Shadows")]
    pub casts: bool,
    /// How soft its shadows are, on top of its size.
    #[setting(label = "Softness", when = "casts", span(0, 4), unit = "×", keyframe, group = "Shadows")]
    pub softness: f32,
    /// The colour its shadows are tinted.
    #[setting(label = "Shadow colour", when = "casts", keyframe, group = "Shadows")]
    pub shadow_colour: Color,
    /// How much of its light a shadow takes away.
    #[setting(label = "Shadow strength", when = "casts", span(0, 1), keyframe, group = "Shadows")]
    pub shadow_strength: f32,
    /// How much brighter it flares when the things below hit.
    #[setting(label = "Flare", span(0, 4), unit = "×", keyframe, group = "On hits")]
    pub flare: f32,
    /// How long a flare takes to die away.
    #[setting(label = "Flare length", when = "flare", range(0.001, 1e9), span(0.02, 2), unit = "s", group = "On hits")]
    pub flare_length: f32,
    /// Whose hits make it flare.
    #[setting(label = "Heard from", when = "flare", group = "On hits")]
    pub flare_from: Target,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self {
            kind: LightType::Point,
            colour_from: LightColour::Own,
            colour: Color::rgb(0xff, 0xf2, 0xde),
            kelvin: 4500.0,
            strength: 1.5,
            reach: 900.0,
            falloff: Falloff::Real,
            size: 24.0,
            height: 160.0,
            elevation: 35.0,
            sun_size: 2.0,
            cone: 50.0,
            cone_edge: 15.0,
            strip_along: StripAlong::Line,
            strip_shape: Target::Kind(KindId::from("shape")),
            width: 240.0,
            length: 400.0,
            area_shape: AreaShape::Rectangle,
            direction: 90.0,
            points: Points::FixedAngle,
            spin: 45.0,
            position: Vec2::new(0.0, -300.0),
            rides_on: RidesOn::EachBall,
            ball_number: 1,
            edge_at: 0.0,
            casts: true,
            softness: 1.0,
            shadow_colour: Color::rgb(0, 0, 0),
            shadow_strength: 1.0,
            flare: 0.0,
            flare_length: 0.3,
            flare_from: Target::Kind(KindId::from("balls")),
        }
    }
}

impl LightSettings {
    /// A new light of `kind`, its sizes set for that kind (a strip is a thin tube).
    pub fn of_kind(kind: LightType) -> Self {
        let mut s = Self { kind, ..Self::default() };
        match kind {
            LightType::Strip => s.width = 8.0,
            LightType::Area => s.length = 120.0,
            _ => {}
        }
        s
    }

    /// The light's colour, linear, before its strength.
    pub fn linear_colour(&self) -> [f32; 3] {
        match self.colour_from {
            LightColour::Own => {
                let c = self.colour.to_linear();
                [c[0], c[1], c[2]]
            }
            LightColour::Temperature => kelvin_to_linear(self.kelvin),
        }
    }
}

impl LightSettings {
    /// The light's colour as sRGB bytes, for the editor's colour dot.
    pub fn display_colour(&self) -> [u8; 3] {
        match self.colour_from {
            LightColour::Own => [self.colour.r, self.colour.g, self.colour.b],
            LightColour::Temperature => {
                let enc = |c: f32| {
                    let c = c.clamp(0.0, 1.0);
                    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
                    (s * 255.0).round() as u8
                };
                let l = kelvin_to_linear(self.kelvin);
                [enc(l[0]), enc(l[1]), enc(l[2])]
            }
        }
    }
}

/// A lamp's colour at `kelvin`, linear and with its brightest channel at 1 (Tanner Helland's fit
/// of the black body, clamped to 1000..40000 K).
pub fn kelvin_to_linear(kelvin: f32) -> [f32; 3] {
    let t = kelvin.clamp(1000.0, 40000.0) / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
    let g = if t <= 66.0 { 99.470_8 * t.ln() - 161.119_57 } else { 288.122_16 * (t - 60.0).powf(-0.075_514_85) };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };
    let lin = |v: f32| {
        let c = (v / 255.0).clamp(0.0, 1.0);
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let rgb = [lin(r), lin(g), lin(b)];
    let top = rgb.iter().cloned().fold(1e-6, f32::max);
    [rgb[0] / top, rgb[1] / top, rgb[2] / top]
}

/// Adds a light of `kind` under `parent`, named for its kind ("Spot light 2"). Inside a Balls,
/// Emitter or Shape thing it rides on it, right on what it rides; elsewhere it hangs above the
/// middle. The first light gives the scene its Lighting card.
pub fn add_light(
    scene: &mut bpe_core::Scene,
    reg: &bpe_core::Registry,
    kind: LightType,
    parent: bpe_core::ThingId,
) -> Result<bpe_core::ThingId, bpe_core::EditError> {
    let id = crate::add_thing(scene, reg, LIGHT, parent)?;
    // Inside a rig, it rides what the rig sits in.
    let rides = crate::things::rig::fixtures::rides_anything(scene, id);
    let name = scene.free_name(kind.menu_name());
    let mut settings = LightSettings::of_kind(kind);
    if rides {
        settings.position = Vec2::ZERO;
    }
    if let Some(t) = scene.things.get_mut(&id) {
        t.params = settings.to_params();
        t.name = name;
    }
    Ok(id)
}

/// The thing kinds a light inside rides on.
pub const RIDES: [&str; 3] = ["balls", "emitter", "shape"];

impl ThingKind for LightKind {
    fn id(&self) -> &'static str {
        LIGHT
    }
    fn name(&self) -> &'static str {
        "Light"
    }
    fn icon(&self) -> &'static str {
        "light"
    }
    fn help(&self) -> &'static str {
        "A lamp: lights the scene and throws soft shadows. Inside a group of balls or a shape it rides on it."
    }
    fn settings(&self) -> &'static [SettingDef] {
        LightSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn family(&self) -> &'static str {
        "Lights"
    }
    fn can_stick_to_screen(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperatures_run_from_warm_to_cold() {
        let warm = kelvin_to_linear(2700.0);
        let day = kelvin_to_linear(6600.0);
        let cold = kelvin_to_linear(12000.0);
        assert!(warm[0] > warm[2] * 3.0, "{warm:?}");
        assert!((day[0] - 1.0).abs() < 0.05 && (day[2] - 1.0).abs() < 0.08, "{day:?}");
        assert!(cold[2] > cold[0], "{cold:?}");
        let warm = LightSettings { colour_from: LightColour::Temperature, kelvin: 2700.0, ..LightSettings::default() };
        let dot = warm.display_colour();
        assert!(dot[0] == 255 && dot[2] < dot[1], "{dot:?}");
        assert_eq!(LightSettings::default().display_colour(), [0xff, 0xf2, 0xde]);
    }

    #[test]
    fn new_settings_have_no_hard_limits_but_where_the_maths_breaks() {
        for d in LightSettings::defs() {
            if let Some((lo, hi)) = d.range {
                assert!(
                    matches!(d.key, "reach" | "size" | "width" | "length" | "cone" | "ball_number" | "flare_length"),
                    "{} has a hard range {lo}..{hi}",
                    d.key
                );
            }
        }
        assert_eq!(LightSettings::of_kind(LightType::Strip).width, 8.0);
    }
}
