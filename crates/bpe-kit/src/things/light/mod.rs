//! Light: a lamp that lights the scene when it has a Lighting card (lighting spec, part 1a).
//! A Point, Spot, Sun, Area or Strip light, with soft shadows. Inside a Balls, Emitter or Shape
//! thing it rides on it: on each ball, the middle, one ball, or a point on a shape's edge. A
//! light is drawn only as light: it has no sim and never touches the physics.
//!
//! Part 4 adds the Laser (a beam traced through the world, `laser.rs`), gobos and flicker,
//! ultraviolet light, Lights up (light linking, `links.rs`) and colours picked from a layer. Glass
//! and light part 2 splits beams into colours through glass with Rainbow split and adds the Light
//! beam, a laser with a width drawn as a band of ribbons.

pub mod draw;
pub mod laser;
pub mod links;

use bpe_core::{Choice, Color, KindId, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

pub use crate::riding::RidesOn;

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
    /// A thin beam that bounces off shapes and balls.
    Laser,
    /// A wide beam, like sunlight through a slot: a laser with a width (glass and light part 2).
    Beam,
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
            Self::Laser => 5.0,
            Self::Beam => 6.0,
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
            Self::Laser => "Laser",
            Self::Beam => "Light beam",
        }
    }

    /// Whether its light is a beam traced through the world (a Laser or a Light beam).
    pub fn is_beam(self) -> bool {
        matches!(self, Self::Laser | Self::Beam)
    }

    pub const ALL: [LightType; 7] =
        [Self::Point, Self::Spot, Self::Sun, Self::Area, Self::Strip, Self::Laser, Self::Beam];
}

/// Where the light's colour comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LightColour {
    /// A colour you pick.
    #[default]
    Own,
    /// The colour of a lamp at a temperature, in kelvin: low is warm, high is cold.
    Temperature,
    /// The colour of a layer of a thing, as it is at that moment.
    Picked,
}

/// What a pattern the light shines through looks like (a gobo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Gobo {
    #[default]
    None,
    Stripes,
    Dots,
    Window,
    Leaves,
    WaterCaustics,
    Rings,
    Clouds,
}

impl Gobo {
    /// Its number in lighting.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Stripes => 1.0,
            Self::Dots => 2.0,
            Self::Window => 3.0,
            Self::Leaves => 4.0,
            Self::WaterCaustics => 5.0,
            Self::Rings => 6.0,
            Self::Clouds => 7.0,
        }
    }
}

/// How a light flickers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FlickerStyle {
    /// Soft and restless, like a flame.
    #[default]
    Candle,
    /// Mostly on, with sudden dropouts, like a failing tube.
    Faulty,
    /// A smooth throb.
    Pulse,
}

/// Which things a light lights up (light linking).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LightsUp {
    #[default]
    Everything,
    OnlyThese,
    EverythingButThese,
}

impl LightsUp {
    pub fn code(self) -> f32 {
        match self {
            Self::Everything => 0.0,
            Self::OnlyThese => 1.0,
            Self::EverythingButThese => 2.0,
        }
    }
}

/// What a laser does when it meets a thing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AtThings {
    /// It bounces off like off a mirror.
    #[default]
    Bounce,
    /// It ends there.
    Stop,
    /// It goes straight through.
    PassThrough,
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
    /// Inside a Text: at its Draw on pen (a laser or beam stops there; nothing while there is
    /// no pen).
    AtThePen,
    /// At a thing, or one of its balls by number.
    AtAThing,
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
    /// The thing whose layer gives the colour.
    #[setting(label = "Pick from", when = "colour_from=picked", group = "Light")]
    pub pick_from: Target,
    /// Which of its layers, counted from its base layer (1).
    #[setting(label = "Layer", when = "colour_from=picked", range(1, 1e9), span(1, 8), group = "Light")]
    pub pick_layer: u32,
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
    /// Gives darkness instead of light: it takes away as much as it would have added, never
    /// below black.
    #[setting(label = "Negative", group = "Light")]
    pub negative: bool,
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
    /// What it points at: the first thing this names that is there.
    #[setting(label = "Aims at", when = "kind!=point and points=at_a_thing", group = "Light")]
    pub aim_at: Target,
    /// Which of its balls, by number: 0 aims at the middle of the balls.
    #[setting(
        label = "Aims at ball",
        when = "kind!=point and points=at_a_thing",
        range(0, 1e9),
        span(0, 100),
        group = "Light"
    )]
    pub aim_ball: u32,
    /// Where it is: in the world, or, riding a thing, from the ball or point it rides.
    #[setting(label = "Position", unit = "px", keyframe, group = "Place")]
    pub position: Vec2,
    /// Inside a Balls, Emitter, Shape or Text thing: what it rides on.
    #[setting(label = "Rides on", group = "Place")]
    pub rides_on: RidesOn,
    /// The ball it rides, by its number in the group.
    #[setting(label = "Ball number", when = "rides_on=ball_number", range(1, 1e9), span(1, 100), group = "Place")]
    pub ball_number: u32,
    /// Where round the edge it rides (each ball's rim, the shape's outline or the letters'), from 0
    /// (the top) to 1 (all the way round, clockwise).
    #[setting(label = "Round the edge", when = "rides_on=the_edge", span(0, 1), keyframe, group = "Place")]
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
    #[setting(label = "Hits from", when = "flare", group = "On hits")]
    pub flare_from: Target,
    /// How much its light shows in the scene's haze (the Lighting card's Haze): beams.
    #[setting(label = "Beam in haze", when = "kind!=laser|beam", span(0, 4), keyframe, group = "Air")]
    pub beam: f32,
    /// How wide the laser's beam is.
    #[setting(
        label = "Beam width",
        when = "kind=laser",
        range(0, 1e9),
        span(0.5, 20),
        unit = "px",
        keyframe,
        group = "Beam"
    )]
    pub laser_width: f32,
    /// How wide a light beam is.
    #[setting(label = "Width", when = "kind=beam", range(0, 1e9), span(0, 200), unit = "px", keyframe, group = "Beam")]
    pub beam_width: f32,
    /// How softly a light beam's edges fade: 0 sharp, 1 a soft glow across.
    #[setting(label = "Edge softness", when = "kind=beam", span(0, 1), keyframe, group = "Beam")]
    pub beam_softness: f32,
    /// How many rays a light beam is traced with, edge to edge: more follow curved glass more
    /// closely.
    #[setting(label = "Rays across", when = "kind=beam", range(2, 64), span(2, 64), group = "Beam")]
    pub rays_across: u32,
    /// How many colours the beam splits into where glass with Rainbow split bends it (at most 64): 1
    /// keeps it one colour.
    #[setting(label = "Colours", when = "kind=laser|beam", range(1, 64), span(1, 32), group = "Beam")]
    pub colours: u32,
    /// How brightly the beam itself shows.
    #[setting(label = "Beam glow", when = "kind=laser|beam", span(0, 4), keyframe, group = "Beam")]
    pub laser_glow: f32,
    /// How brightly it shines where it bounces or ends.
    #[setting(label = "Hit glow", when = "kind=laser|beam", span(0, 4), keyframe, group = "Beam")]
    pub hit_glow: f32,
    /// How many times it may bounce (at most 256).
    #[setting(label = "Bounces", when = "kind=laser|beam", range(0, 256), span(0, 32), group = "Beam")]
    pub bounces: u32,
    /// How far it reaches in all, bounces and all.
    #[setting(
        label = "Beam length",
        when = "kind=laser|beam",
        range(0, 1e9),
        span(0, 20000),
        unit = "px",
        keyframe,
        group = "Beam"
    )]
    pub beam_length: f32,
    /// What it does at a shape.
    #[setting(label = "At shapes", when = "kind=laser|beam", group = "Beam")]
    pub at_shapes: AtThings,
    /// What it does at a ball.
    #[setting(label = "At balls", when = "kind=laser|beam", group = "Beam")]
    pub at_balls: AtThings,
    /// A pattern the light shines through.
    #[setting(label = "Gobo", group = "Gobo and flicker")]
    pub gobo: Gobo,
    /// How big the pattern is.
    #[setting(
        label = "Gobo size",
        when = "gobo!=none",
        range(0.001, 1e9),
        span(10, 1000),
        unit = "px",
        keyframe,
        group = "Gobo and flicker"
    )]
    pub gobo_size: f32,
    /// How the pattern is turned.
    #[setting(label = "Gobo turn", when = "gobo!=none", span(-180, 180), unit = "°", keyframe, group = "Gobo and flicker")]
    pub gobo_turn: f32,
    /// How fast the pattern turns.
    #[setting(label = "Gobo spin", when = "gobo!=none", span(-360, 360), unit = "°/s", keyframe, group = "Gobo and flicker")]
    pub gobo_spin: f32,
    /// How fast the pattern slides, across and along.
    #[setting(label = "Gobo drift", when = "gobo!=none", unit = "px/s", keyframe, group = "Gobo and flicker")]
    pub gobo_drift: Vec2,
    /// How crisp the pattern's edges are.
    #[setting(label = "Sharpness", when = "gobo!=none", span(0, 1), keyframe, group = "Gobo and flicker")]
    pub gobo_sharpness: f32,
    /// How dark the pattern's shade is: 0 none, 1 black.
    #[setting(label = "Gobo depth", when = "gobo!=none", span(0, 1), keyframe, group = "Gobo and flicker")]
    pub gobo_depth: f32,
    /// How much it flickers: 0 steady.
    #[setting(label = "Flicker", span(0, 1), keyframe, group = "Gobo and flicker")]
    pub flicker: f32,
    /// How fast it flickers.
    #[setting(
        label = "Flicker speed",
        when = "flicker",
        span(0.1, 30),
        unit = "Hz",
        keyframe,
        group = "Gobo and flicker"
    )]
    pub flicker_speed: f32,
    /// How it flickers.
    #[setting(label = "Flicker style", when = "flicker", group = "Gobo and flicker")]
    pub flicker_style: FlickerStyle,
    /// Picks another flicker.
    #[setting(label = "Flicker seed", when = "flicker", span(0, 100), group = "Gobo and flicker")]
    pub flicker_seed: u32,
    /// Ultraviolet: it lights only what glows under UV (a layer's Under UV), but for its visible
    /// share.
    #[setting(label = "Ultraviolet", group = "Special")]
    pub ultraviolet: bool,
    /// How much ordinary light an ultraviolet lamp also gives.
    #[setting(label = "Visible light", when = "ultraviolet", span(0, 1), keyframe, group = "Special")]
    pub uv_visible: f32,
    /// Which things it lights up. Its light in the air shows whatever it lights.
    #[setting(label = "Lights up", group = "Lights up")]
    pub lights_up: LightsUp,
    /// The things it lights up, or leaves out: a thing, a tag or a kind.
    #[setting(label = "Things", when = "lights_up!=everything", group = "Lights up")]
    pub link_things: Target,
    /// Whether it lights the background too.
    #[setting(label = "The background", when = "lights_up!=everything", group = "Lights up")]
    pub link_background: bool,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self {
            kind: LightType::Point,
            colour_from: LightColour::Own,
            colour: Color::rgb(0xff, 0xf2, 0xde),
            kelvin: 4500.0,
            strength: 1.5,
            negative: false,
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
            aim_at: Target::Kind(KindId::from("balls")),
            aim_ball: 1,
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
            beam: 1.0,
            pick_from: Target::Kind(KindId::from("balls")),
            pick_layer: 1,
            laser_width: 3.0,
            beam_width: 60.0,
            beam_softness: 0.35,
            rays_across: 16,
            colours: 12,
            laser_glow: 1.0,
            hit_glow: 1.0,
            bounces: 8,
            beam_length: 8000.0,
            at_shapes: AtThings::Bounce,
            at_balls: AtThings::Bounce,
            gobo: Gobo::None,
            gobo_size: 120.0,
            gobo_turn: 0.0,
            gobo_spin: 0.0,
            gobo_drift: Vec2::ZERO,
            gobo_sharpness: 0.6,
            gobo_depth: 1.0,
            flicker: 0.0,
            flicker_speed: 8.0,
            flicker_style: FlickerStyle::Candle,
            flicker_seed: 1,
            ultraviolet: false,
            uv_visible: 0.15,
            lights_up: LightsUp::Everything,
            link_things: Target::Kind(KindId::from("balls")),
            link_background: true,
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
            LightType::Laser => {
                s.colour = Color::rgb(0xff, 0x2f, 0x4e);
                s.strength = 3.0;
                s.reach = 160.0;
                s.size = 2.0;
                s.height = 20.0;
            }
            LightType::Beam => {
                s.colour = Color::rgb(0xff, 0xf8, 0xf0);
                s.strength = 2.0;
                s.reach = 240.0;
                s.size = 2.0;
                s.height = 20.0;
                s.laser_glow = 0.6;
                s.hit_glow = 0.6;
            }
            _ => {}
        }
        s
    }

    /// The light's colour, linear, before its strength (a picked colour is worked out when it is
    /// drawn: here, its own colour).
    pub fn linear_colour(&self) -> [f32; 3] {
        match self.colour_from {
            LightColour::Own | LightColour::Picked => {
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
            LightColour::Own | LightColour::Picked => [self.colour.r, self.colour.g, self.colour.b],
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
/// Emitter, Shape or Text thing it rides on it, right on what it rides; elsewhere it hangs above the
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
    crate::riding::fit_choice(scene, id);
    Ok(id)
}

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
    fn offers(&self, scene: &bpe_core::Scene, thing: bpe_core::ThingId, key: &str, choice: &str) -> bool {
        key != "rides_on" || crate::riding::offers(scene, thing, choice)
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
                    matches!(
                        d.key,
                        "reach"
                            | "size"
                            | "width"
                            | "length"
                            | "cone"
                            | "ball_number"
                            | "aim_ball"
                            | "flare_length"
                            | "pick_layer"
                            | "laser_width"
                            | "bounces"
                            | "beam_length"
                            | "gobo_size"
                            | "beam_width"
                            | "rays_across"
                            | "colours"
                    ),
                    "{} has a hard range {lo}..{hi}",
                    d.key
                );
            }
        }
        assert_eq!(LightSettings::of_kind(LightType::Strip).width, 8.0);
    }

    #[test]
    fn a_negative_light_pushes_darkness() {
        use bpe_core::{Settings, ThingId, Value, resolve};
        let kit = crate::kit();
        let reg = &kit.registry;
        let mut scene = crate::new_studio_scene(reg);
        let id = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).unwrap();
        let strength = |scene: &bpe_core::Scene| {
            let resolved = resolve(scene, reg);
            let world = bpe_runtime::World::build(scene, &resolved, &kit.sims);
            let mut frame = bpe_gfx::Frame::default();
            bpe_gfx::build_frame(scene, &resolved, &world, &bpe_runtime::Log::default(), 0.0, &kit.draws, &mut frame);
            frame.light.expect("lit").lights[0].p1
        };
        let bright = strength(&scene);
        assert!(bright[0] > 0.0);
        scene.things.get_mut(&id).unwrap().params.set("negative", Value::Bool(true));
        let dark = strength(&scene);
        assert_eq!([dark[0], dark[1], dark[2]], [-bright[0], -bright[1], -bright[2]]);
        assert!(!LightSettings::default().negative);
        assert!(LightSettings::defs().iter().any(|d| d.key == "negative"));
    }
}
