//! How a Water scene looks (Water mode part 6): a Liquid's **Look** card, the scene's **Light** and
//! **Back** cards and a Lamp's card. Kept apart from the cards the physics reads, so a change here
//! never makes a final again (the kit's `build.rs` leaves this file out of the take's print).

use bpe_core::{Choice, Color, Gradient, Settings};

/// What a Liquid's colour is read from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum ColourFrom {
    /// Its one Colour (and what it has mixed with).
    #[default]
    ItsOwn,
    Speed,
    Depth,
}

/// What makes a Liquid glow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum GlowFrom {
    #[default]
    Always,
    /// The faster, the brighter.
    Speed,
}

/// A Liquid's Look card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WaterLook {
    #[setting(label = "Colour", simple, keyframe, group = "Its colour")]
    pub colour: Color,
    /// Its own colour, or a gradient read from how fast or how deep it is.
    #[setting(label = "Colour from", simple, group = "Its colour")]
    pub colour_from: ColourFrom,
    #[setting(label = "Gradient", when = "colour_from!=its_own", keyframe, group = "Its colour")]
    pub colour_gradient: Gradient,

    /// How far you see into it before its colour takes over.
    #[setting(label = "Clarity", range(0, 1e9), span(0, 600), unit = "px", simple, keyframe, group = "Its body")]
    pub clarity: f32,
    /// Its colour where it is deep.
    #[setting(label = "Deep colour", when = "clarity", keyframe, group = "Its body")]
    pub deep: Color,
    /// 0 is see-through like water, 1 is solid like paint.
    #[setting(label = "Solid", range(0, 1), span(0, 1), simple, keyframe, group = "Its body")]
    pub solid: f32,
    /// How far it bends what is seen through it.
    #[setting(label = "Bend", range(0, 1e9), span(0, 60), unit = "px", keyframe, group = "Its body")]
    pub bend: f32,
    /// Splits the bent picture into colours at its edges.
    #[setting(label = "Rainbow split", when = "bend", range(0, 1), span(0, 1), keyframe, group = "Its body")]
    pub split: f32,

    /// How smooth its outline is: 0 shows every drop.
    #[setting(label = "Smoothing", range(0, 1e9), span(0, 1), keyframe, group = "Its surface")]
    pub smoothing: f32,
    /// How much thin streams hold together as streaks instead of beads.
    #[setting(label = "Streaks", range(0, 1e9), span(0, 4), keyframe, group = "Its surface")]
    pub streaks: f32,
    /// How brightly its surface catches the lamps.
    #[setting(label = "Shine", range(0, 1), span(0, 1), simple, keyframe, group = "Its surface")]
    pub shine: f32,
    /// How wide each highlight is: small is glassy, large is soft.
    #[setting(label = "Shine size", when = "shine", range(0, 1), span(0, 1), group = "Its surface")]
    pub shine_size: f32,
    /// A bright rim where the surface turns away from you.
    #[setting(label = "Edge sheen", range(0, 1), span(0, 1), keyframe, group = "Its surface")]
    pub edge_sheen: f32,
    #[setting(label = "Ripples", range(0, 1e9), span(0, 1), keyframe, group = "Its surface")]
    pub ripples: f32,
    /// Oil-on-water colours on its surface.
    #[setting(label = "Thin film", range(0, 1), span(0, 1), keyframe, group = "Its surface")]
    pub thin_film: f32,

    /// Light it gives off, which lights the course near it.
    #[setting(label = "Gives off light", range(0, 1e9), span(0, 4), simple, keyframe, group = "Glow")]
    pub glow: f32,
    #[setting(label = "Glow colour", when = "glow", keyframe, group = "Glow")]
    pub glow_colour: Color,
    #[setting(label = "Glow from", when = "glow", group = "Glow")]
    pub glow_from: GlowFrom,

    /// A drawn line round the liquid (0: none).
    #[setting(label = "Outline", range(0, 1e9), span(0, 10), unit = "px", keyframe, group = "Drawn")]
    pub outline: f32,
    #[setting(label = "Outline colour", when = "outline", keyframe, group = "Drawn")]
    pub outline_colour: Color,
    /// Shades it in this many flat bands, like a cartoon (0: smooth).
    #[setting(label = "Bands", range(0, 16), group = "Drawn")]
    pub bands: u32,
    #[setting(label = "Band softness", when = "bands", range(0, 1), span(0, 1), group = "Drawn")]
    pub band_softness: f32,

    /// White froth where it churns.
    #[setting(label = "Foam", range(0, 1), span(0, 1), keyframe, group = "Extras")]
    pub foam: f32,
    #[setting(label = "Foam size", when = "foam", range(0.5, 1e9), span(1, 40), unit = "px", group = "Extras")]
    pub foam_size: f32,
    #[setting(label = "Foam colour", when = "foam", keyframe, group = "Extras")]
    pub foam_colour: Color,
    /// Its trapped air drawn as bubbles, and small bubbles rising inside it.
    #[setting(label = "Bubbles", range(0, 1), span(0, 1), keyframe, group = "Extras")]
    pub bubbles: f32,
    #[setting(label = "Bubble size", when = "bubbles", range(0.5, 1e9), span(1, 40), unit = "px", group = "Extras")]
    pub bubbles_size: f32,
    #[setting(label = "Bubble colour", when = "bubbles", keyframe, group = "Extras")]
    pub bubbles_colour: Color,
    /// How long each small bubble rising in it lives, fading in and out.
    #[setting(label = "Bubbles last", when = "bubbles", range(0.05, 60), span(0.1, 5), unit = "s", group = "Extras")]
    pub bubbles_life: f32,
    /// Fine spray thrown up where it lands hard.
    #[setting(label = "Mist", range(0, 1), span(0, 1), keyframe, group = "Extras")]
    pub mist: f32,
    #[setting(label = "Mist size", when = "mist", range(0.5, 1e9), span(1, 40), unit = "px", group = "Extras")]
    pub mist_size: f32,
    #[setting(label = "Mist colour", when = "mist", keyframe, group = "Extras")]
    pub mist_colour: Color,
    /// Bright sparks streaking behind its fastest drops.
    #[setting(label = "Sparks", range(0, 1), span(0, 1), keyframe, group = "Extras")]
    pub sparks: f32,
    #[setting(label = "Spark size", when = "sparks", range(0.5, 1e9), span(1, 40), unit = "px", group = "Extras")]
    pub sparks_size: f32,
    #[setting(label = "Spark colour", when = "sparks", keyframe, group = "Extras")]
    pub sparks_colour: Color,
    /// How long a spark's trail is, in seconds of its flight.
    #[setting(label = "Sparks last", when = "sparks", range(0.05, 60), span(0.1, 5), unit = "s", group = "Extras")]
    pub sparks_life: f32,
}

impl Default for WaterLook {
    fn default() -> Self {
        Self {
            colour: Color::rgb(0x2f, 0x9b, 0xff),
            colour_from: ColourFrom::ItsOwn,
            colour_gradient: Gradient::even(&[Color::rgb(0x2f, 0x9b, 0xff), Color::WHITE]),
            clarity: 200.0,
            deep: Color::rgb(0x0b, 0x2a, 0x4f),
            solid: 0.0,
            bend: 12.0,
            split: 0.0,
            smoothing: 0.5,
            streaks: 1.0,
            shine: 0.5,
            shine_size: 0.3,
            edge_sheen: 0.3,
            ripples: 0.0,
            thin_film: 0.0,
            glow: 0.0,
            glow_colour: Color::WHITE,
            glow_from: GlowFrom::Always,
            outline: 0.0,
            outline_colour: Color::rgb(0, 0, 0),
            bands: 0,
            band_softness: 0.2,
            foam: 0.0,
            foam_size: 6.0,
            foam_colour: Color::WHITE,
            bubbles: 0.0,
            bubbles_size: 6.0,
            bubbles_colour: Color::WHITE,
            bubbles_life: 1.0,
            mist: 0.0,
            mist_size: 6.0,
            mist_colour: Color::WHITE,
            sparks: 0.0,
            sparks_size: 6.0,
            sparks_colour: Color::rgb(0xff, 0xb0, 0x40),
            sparks_life: 1.0,
        }
    }
}

/// How carefully the water's light is worked out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum LightQuality {
    Draft,
    #[default]
    Good,
    Best,
}

/// The Light card: the light the lamps don't give.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WaterLightCard {
    /// Light that is everywhere, lamp or no lamp: 1 shows every colour as it is.
    #[setting(label = "Ambient", range(0, 1e9), span(0, 2), simple, keyframe, group = "Light")]
    pub ambient: f32,
    /// How much light bounces off the rock and the water onto what is near.
    #[setting(label = "Bounce", range(0, 1e9), span(0, 1), keyframe, group = "Light")]
    pub bounce: f32,
    /// The bright rippling patterns water throws on what is under it.
    #[setting(label = "Caustics", range(0, 1e9), span(0, 2), simple, keyframe, group = "Light")]
    pub caustics: f32,
    /// Beams of light showing in the water and the air.
    #[setting(label = "Shafts", range(0, 1e9), span(0, 1), keyframe, group = "Light")]
    pub shafts: f32,
    /// How carefully the light is worked out: Draft is quickest, Best is for the finished video.
    #[setting(label = "Quality", group = "Quality")]
    pub quality: LightQuality,
}

impl Default for WaterLightCard {
    fn default() -> Self {
        Self { ambient: 1.0, bounce: 0.3, caustics: 1.0, shafts: 0.0, quality: LightQuality::Good }
    }
}

/// What shows where the course is carved away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum BackShows {
    /// The course's own rock, darker, as if further back.
    #[default]
    DarkerRock,
    Colour,
    Gradient,
    Picture,
}

/// The Back card: what is behind the water.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BackCard {
    #[setting(label = "Shows", simple, group = "Back")]
    pub shows: BackShows,
    /// How much darker than the rock in front: 0 the same, 1 black.
    #[setting(label = "Darker", when = "shows=darker_rock", range(0, 1), span(0, 1), keyframe, group = "Back")]
    pub darker: f32,
    #[setting(label = "Colour", when = "shows=colour", keyframe, group = "Back")]
    pub colour: Color,
    #[setting(label = "Gradient", when = "shows=gradient", keyframe, group = "Back")]
    pub gradient: Gradient,
    /// Which way the gradient runs.
    #[setting(label = "Direction", when = "shows=gradient", span(-180, 180), unit = "°", group = "Back")]
    pub angle: f32,
    /// A PNG picture shown behind the course.
    #[setting(label = "Picture", when = "shows=picture", unit = "picture", group = "Back")]
    pub picture: String,
}

impl Default for BackCard {
    fn default() -> Self {
        Self {
            shows: BackShows::DarkerRock,
            darker: 0.5,
            colour: Color::rgb(0x14, 0x18, 0x1e),
            gradient: Gradient::even(&[Color::rgb(0x0e, 0x1c, 0x33), Color::rgb(0x05, 0x07, 0x0b)]),
            angle: 90.0,
            picture: String::new(),
        }
    }
}

/// The kinds of lamp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum LampKind {
    /// A cone of light from one point.
    #[default]
    Spot,
    /// A line of light.
    Strip,
    /// Light from far away, the same everywhere.
    Sun,
}

/// A Lamp's card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LampCard {
    #[setting(label = "Kind", simple, group = "Lamp")]
    pub kind: LampKind,
    #[setting(label = "Colour", simple, keyframe, group = "Lamp")]
    pub colour: Color,
    #[setting(label = "Brightness", range(0, 1e9), span(0, 4), simple, keyframe, group = "Lamp")]
    pub brightness: f32,
    /// Which way it shines: 90 is straight down.
    #[setting(label = "Aim", span(-180, 180), unit = "°", keyframe, group = "Lamp")]
    pub aim: f32,
    /// How wide the spot's cone is.
    #[setting(label = "Spread", when = "kind=spot", range(1, 360), span(1, 360), unit = "°", keyframe, group = "Lamp")]
    pub spread: f32,
    #[setting(
        label = "Length",
        when = "kind=strip",
        range(1, 1e9),
        span(10, 2000),
        unit = "px",
        keyframe,
        group = "Lamp"
    )]
    pub length: f32,
    /// How soft the edges of its light and its shadows are.
    #[setting(label = "Softness", range(0, 1), span(0, 1), group = "Lamp")]
    pub softness: f32,
    /// The course and the pieces throw shadows in its light.
    #[setting(label = "Shadows", group = "Lamp")]
    pub shadows: bool,
}

impl Default for LampCard {
    fn default() -> Self {
        Self {
            kind: LampKind::Spot,
            colour: Color::rgb(0xff, 0xe2, 0xb8),
            brightness: 1.0,
            aim: 90.0,
            spread: 60.0,
            length: 400.0,
            softness: 0.3,
            shadows: true,
        }
    }
}
