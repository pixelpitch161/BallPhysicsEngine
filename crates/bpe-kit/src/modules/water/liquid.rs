//! A Water scene's Liquid cards: **Feel**, **Look**, **Mixing** and **Heat**. Settings today's
//! Liquid thing also has keep its names, ranges and spans.

use bpe_core::{Choice, Color, Gradient, Settings};

/// How a Liquid flows.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FeelCard {
    /// 0 runs like water; higher is syrup, then honey, then paste.
    #[setting(label = "Thickness", range(0, 1e9), span(0, 1), simple, keyframe, group = "How it flows")]
    pub thickness: f32,
    /// How strongly it pulls itself into drops and smooth edges.
    #[setting(label = "Surface pull", range(0, 1e9), span(0, 1), keyframe, group = "How it flows")]
    pub surface_pull: f32,
    /// How heavy it is next to other liquids: a heavier one sinks under a lighter one.
    #[setting(label = "Weight", range(0.05, 1e9), span(0.1, 10), simple, keyframe, group = "How it flows")]
    pub weight: f32,
    /// How much it clings to the course instead of running off.
    #[setting(label = "Stickiness", range(0, 1e9), span(0, 1), keyframe, group = "How it flows")]
    pub stickiness: f32,
}

impl Default for FeelCard {
    fn default() -> Self {
        Self { thickness: 0.02, surface_pull: 0.5, weight: 1.0, stickiness: 0.0 }
    }
}

/// What a Liquid's colour is read from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum ColourFrom {
    /// Its one Colour (and what it has mixed with).
    #[default]
    ItsOwn,
    Speed,
    Heat,
    Age,
    Depth,
}

/// What makes a Liquid glow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum GlowFrom {
    #[default]
    Always,
    /// The hotter, the brighter.
    Heat,
    /// The faster, the brighter.
    Speed,
}

/// How mixing colours show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MixShows {
    #[default]
    Shimmer,
    Glow,
    Foam,
}

/// A Liquid's Look card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WaterLook {
    #[setting(label = "Colour", simple, keyframe, group = "Its colour")]
    pub colour: Color,
    /// Its own colour, or a gradient read from how fast, hot, old or deep it is.
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

    /// A skin that forms where it has cooled or been still (0: none).
    #[setting(label = "Crust", range(0, 1), span(0, 1), keyframe, group = "Crust")]
    pub crust: f32,
    #[setting(label = "Crust colour", when = "crust", keyframe, group = "Crust")]
    pub crust_colour: Color,
    /// How long it takes to form.
    #[setting(label = "Forms in", when = "crust", range(0.05, 600), span(0.1, 20), unit = "s", group = "Crust")]
    pub crust_forms_in: f32,
    /// How much it cracks where the liquid under it moves.
    #[setting(label = "Cracks", when = "crust", range(0, 1), span(0, 1), group = "Crust")]
    pub crust_cracks: f32,

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
    #[setting(label = "Foam lasts", when = "foam", range(0.05, 60), span(0.1, 5), unit = "s", group = "Extras")]
    pub foam_life: f32,
    /// Bubbles rising inside it.
    #[setting(label = "Bubbles", range(0, 1), span(0, 1), keyframe, group = "Extras")]
    pub bubbles: f32,
    #[setting(label = "Bubble size", when = "bubbles", range(0.5, 1e9), span(1, 40), unit = "px", group = "Extras")]
    pub bubbles_size: f32,
    #[setting(label = "Bubble colour", when = "bubbles", keyframe, group = "Extras")]
    pub bubbles_colour: Color,
    #[setting(label = "Bubbles last", when = "bubbles", range(0.05, 60), span(0.1, 5), unit = "s", group = "Extras")]
    pub bubbles_life: f32,
    /// Fine spray thrown up where it lands hard.
    #[setting(label = "Mist", range(0, 1), span(0, 1), keyframe, group = "Extras")]
    pub mist: f32,
    #[setting(label = "Mist size", when = "mist", range(0.5, 1e9), span(1, 40), unit = "px", group = "Extras")]
    pub mist_size: f32,
    #[setting(label = "Mist colour", when = "mist", keyframe, group = "Extras")]
    pub mist_colour: Color,
    #[setting(label = "Mist lasts", when = "mist", range(0.05, 60), span(0.1, 5), unit = "s", group = "Extras")]
    pub mist_life: f32,
    /// Bright sparks thrown off where it is hot or fast.
    #[setting(label = "Sparks", range(0, 1), span(0, 1), keyframe, group = "Extras")]
    pub sparks: f32,
    #[setting(label = "Spark size", when = "sparks", range(0.5, 1e9), span(1, 40), unit = "px", group = "Extras")]
    pub sparks_size: f32,
    #[setting(label = "Spark colour", when = "sparks", keyframe, group = "Extras")]
    pub sparks_colour: Color,
    #[setting(label = "Sparks last", when = "sparks", range(0.05, 60), span(0.1, 5), unit = "s", group = "Extras")]
    pub sparks_life: f32,

    /// How sharp the swirls of colour stay: 0 soft, 1 crisp streaks.
    #[setting(label = "Swirl detail", range(0, 1), span(0, 1), keyframe, group = "Mixing shows")]
    pub swirl_detail: f32,
    /// Shows where colours are mixing.
    #[setting(label = "Mixing shows", range(0, 1), span(0, 1), keyframe, group = "Mixing shows")]
    pub mixing_shows: f32,
    #[setting(label = "Shows as", when = "mixing_shows", group = "Mixing shows")]
    pub shows_as: MixShows,
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
            crust: 0.0,
            crust_colour: Color::rgb(0x2a, 0x22, 0x20),
            crust_forms_in: 3.0,
            crust_cracks: 0.5,
            outline: 0.0,
            outline_colour: Color::rgb(0, 0, 0),
            bands: 0,
            band_softness: 0.2,
            foam: 0.0,
            foam_size: 6.0,
            foam_colour: Color::WHITE,
            foam_life: 1.0,
            bubbles: 0.0,
            bubbles_size: 6.0,
            bubbles_colour: Color::WHITE,
            bubbles_life: 1.0,
            mist: 0.0,
            mist_size: 6.0,
            mist_colour: Color::WHITE,
            mist_life: 1.0,
            sparks: 0.0,
            sparks_size: 6.0,
            sparks_colour: Color::rgb(0xff, 0xb0, 0x40),
            sparks_life: 1.0,
            swirl_detail: 0.5,
            mixing_shows: 0.0,
            shows_as: MixShows::Shimmer,
        }
    }
}

/// When a Liquid's colours mix with its neighbours'.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MixesWhen {
    #[default]
    Always,
    OnlyWhenStirred,
    OnlyOnHits,
}

/// A Liquid's Mixing card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MixingCard {
    /// How fast colours run together where liquids meet: 0 never, 1 at once.
    #[setting(label = "Mix speed", range(0, 1), span(0, 1), simple, group = "Colour mixing")]
    pub mix_speed: f32,
    #[setting(label = "Mixes when", group = "Colour mixing")]
    pub mixes_when: MixesWhen,
    /// How fast its colours drift to Fades to (0: never).
    #[setting(label = "Fades", range(0, 1e9), span(0, 1), unit = "/s", group = "Colour mixing")]
    pub fades: f32,
    #[setting(label = "Fades to", when = "fades", group = "Colour mixing")]
    pub fades_to: Color,
    /// Keep the last Seconds of the liquid so it can play backwards.
    #[setting(label = "Remember for Un-mix", group = "Un-mix")]
    pub remember: bool,
    #[setting(label = "Seconds", when = "remember", range(0.1, 60), span(1, 20), unit = "s", group = "Un-mix")]
    pub unmix_seconds: f32,
    /// While on, the liquid plays its remembered seconds backwards (colours un-mix too).
    #[setting(label = "Un-mixing", when = "remember", group = "Un-mix")]
    pub unmixing: bool,
}

impl Default for MixingCard {
    fn default() -> Self {
        Self {
            mix_speed: 0.5,
            mixes_when: MixesWhen::Always,
            fades: 0.0,
            fades_to: Color::WHITE,
            remember: false,
            unmix_seconds: 5.0,
            unmixing: false,
        }
    }
}

/// A Liquid's Heat card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HeatCard {
    /// How hot new liquid is (0 cold, 1 hot).
    #[setting(label = "Temperature", range(0, 1), span(0, 1), simple, keyframe, group = "Heat")]
    pub temperature: f32,
    /// How fast it loses heat at its surface.
    #[setting(label = "Cools", range(0, 1e9), span(0, 2), unit = "/s", keyframe, group = "Heat")]
    pub cools: f32,
    /// How fast it takes heat from Heaters.
    #[setting(label = "Warms", range(0, 1e9), span(0, 2), unit = "/s", keyframe, group = "Heat")]
    pub warms: f32,
    /// How much thicker it gets as it cools (lava).
    #[setting(label = "Cold thickens it", range(0, 1e9), span(0, 1), keyframe, group = "Heat")]
    pub cold_thickens: f32,
    /// It turns solid when it is cold enough, and melts again when warm.
    #[setting(label = "Freezes", group = "Heat")]
    pub freezes: bool,
    #[setting(label = "Freezes below", when = "freezes", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub freezes_below: f32,
    #[setting(label = "Melts above", when = "freezes", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub melts_above: f32,
    /// How fast it dries up at its surface (0: never).
    #[setting(label = "Dries up", range(0, 1e9), span(0, 1), unit = "/s", keyframe, group = "Heat")]
    pub dries: f32,
    /// Dried liquid leaves a mark where it was.
    #[setting(label = "Stains", when = "dries", group = "Heat")]
    pub stains: bool,
    #[setting(label = "Stain strength", when = "dries and stains", range(0, 1), span(0, 1), keyframe, group = "Heat")]
    pub stain_strength: f32,
}

impl Default for HeatCard {
    fn default() -> Self {
        let old = crate::things::liquid::LiquidSettings::default();
        Self {
            temperature: old.temperature,
            cools: old.cools,
            warms: old.warms,
            cold_thickens: old.cold_thickens,
            freezes: old.freezes,
            freezes_below: old.freezes_below,
            melts_above: old.melts_above,
            dries: old.dries,
            stains: old.stains,
            stain_strength: old.stain_strength,
        }
    }
}
