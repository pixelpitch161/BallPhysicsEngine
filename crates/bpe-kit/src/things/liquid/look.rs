//! A Liquid thing's look (liquid part 3): its Look card's settings with the shared Surface's
//! numbers, and the layers its Look holds: **White water and foam**, **Bubbles**, **Things
//! inside**, **Mist**, **Caustics**, **Wet walls** and **Wet balls**. Each kind at most once; a
//! layer that is off (or missing) draws nothing. Every one of them only draws: nothing here moves
//! a drop or a ball, so no edit here restarts anything.

use std::sync::LazyLock;

use bpe_core::{Color, LayerSection, ResolvedSlot, SettingDef, Settings};

use super::cards::{LOOK, LookSettings};
use super::num;
use crate::modules::look::layers::Kind;
use crate::modules::look::surface::{self, SurfaceSettings};
use crate::things::particles::liquid::NOT_ON_A_LIQUID;

pub const FOAM: &str = "liquid.foam";
pub const BUBBLES: &str = "liquid.bubbles";
pub const INSIDE: &str = "liquid.inside";
pub const MIST: &str = "liquid.mist";
pub const CAUSTICS: &str = "liquid.caustics";
pub const WET_WALLS: &str = "liquid.wet_walls";
pub const WET_BALLS: &str = "liquid.wet_balls";

/// The Look card's settings: [`LookSettings`]'s with the Surface's numbers (as a Surface sub-layer
/// has them, but [`NOT_ON_A_LIQUID`]) right after its Surface, starting as the default Surface's.
pub fn look_defs() -> &'static [SettingDef] {
    static DEFS: LazyLock<Vec<SettingDef>> = LazyLock::new(|| {
        let start = surface::preset_values(LookSettings::default().surface);
        let wears = |d: &&SettingDef| d.key != "surface" && !NOT_ON_A_LIQUID.contains(&d.key);
        let numbers = surface::sub_defs().iter().filter(wears).map(|d| {
            let mut d = d.clone();
            if let Some((_, v)) = start.iter().find(|(k, _)| *k == d.key) {
                d.default = v.clone();
            }
            d
        });
        let own = LookSettings::defs();
        let at = own.iter().position(|d| d.key == "surface").map_or(own.len(), |i| i + 1);
        own[..at].iter().cloned().chain(numbers).chain(own[at..].iter().cloned()).collect()
    });
    &DEFS
}

/// Whether `key` is one of the Surface's numbers the Look card holds (shown by the Surface
/// section, not the card's own grid).
pub fn is_surface_number(key: &str) -> bool {
    key != "surface" && surface::sub_defs().iter().any(|d| d.key == key) && !NOT_ON_A_LIQUID.contains(&key)
}

/// The Look card's Surface (its numbers from the card's settings).
pub fn surface_of(look: Option<&ResolvedSlot>) -> SurfaceSettings {
    let mut s = look.map(|c| SurfaceSettings::from_params(&c.params)).unwrap_or_default();
    s.surface = LookSettings::read(look).surface;
    s
}

/// White water and foam: where the liquid is fast or splashed, near its top.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FoamLayer {
    #[setting(label = "Amount", range(0, 1e9), span(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// Drops faster than this foam (fully at twice it).
    #[setting(label = "From speed", range(0, 1e9), span(0, 2000), unit = "px/s", simple, keyframe)]
    pub from_speed: f32,
    /// How much a ball splashing in froths it up.
    #[setting(label = "Splashes foam", range(0, 1e9), span(0, 1), keyframe)]
    pub splashes: f32,
    /// How far below the top the foam shows fully (deeper it shows less).
    #[setting(label = "On top", range(0, 1e9), span(0, 120), unit = "px", keyframe)]
    pub on_top: f32,
    /// The size of its froth's bubbles.
    #[setting(label = "Froth size", range(0.5, 1e9), span(1, 12), unit = "px", keyframe)]
    pub froth: f32,
    #[setting(label = "Colour")]
    pub colour: Color,
}

impl Default for FoamLayer {
    fn default() -> Self {
        FoamLayer {
            amount: 0.8,
            from_speed: 450.0,
            splashes: 1.0,
            on_top: 24.0,
            froth: 3.0,
            colour: Color::rgb(255, 255, 255),
        }
    }
}

/// Bubbles rising through it and popping at the top.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BubblesLayer {
    /// How many: the share of places that have a bubble.
    #[setting(label = "Amount", range(0, 1), span(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    #[setting(label = "Size", range(0.5, 1e9), span(1, 30), unit = "px", simple, keyframe, pulse)]
    pub size: f32,
    #[setting(label = "Size variety", range(0, 1), span(0, 1), keyframe)]
    pub variety: f32,
    #[setting(label = "Rise speed", range(-1e9, 1e9), span(0, 400), unit = "px/s", keyframe, pulse)]
    pub rise: f32,
    #[setting(label = "Wobble", range(0, 1), span(0, 1), keyframe)]
    pub wobble: f32,
    #[setting(label = "Shine", range(0, 2), span(0, 1), keyframe)]
    pub shine: f32,
}

impl Default for BubblesLayer {
    fn default() -> Self {
        BubblesLayer { amount: 0.15, size: 5.0, variety: 0.6, rise: 70.0, wobble: 0.5, shine: 0.8 }
    }
}

/// Where flecks take their colour from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, bpe_core::Choice)]
pub enum FleckColour {
    /// The liquid's own colour where they are.
    TheLiquids,
    /// One colour.
    #[default]
    OneColour,
}

/// Things inside: flecks or glitter carried along by the liquid.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct InsideLayer {
    /// The share of the liquid's drops that carry a fleck.
    #[setting(label = "How many", range(0, 1), span(0, 0.2), simple, keyframe, pulse)]
    pub share: f32,
    #[setting(label = "Size", range(0.1, 1e9), span(0.5, 8), unit = "px", simple, keyframe, pulse)]
    pub size: f32,
    /// How much they glint now and then.
    #[setting(label = "Sparkle", range(0, 1), span(0, 1), keyframe, pulse)]
    pub sparkle: f32,
    #[setting(label = "Colour from")]
    pub colour_from: FleckColour,
    #[setting(label = "Colour", when = "colour_from=one_colour")]
    pub colour: Color,
}

impl Default for InsideLayer {
    fn default() -> Self {
        InsideLayer {
            share: 0.03,
            size: 1.6,
            sparkle: 0.7,
            colour_from: FleckColour::OneColour,
            colour: Color::rgb(255, 214, 120),
        }
    }
}

/// Mist: the spray thrown off it, as fine dots.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MistLayer {
    #[setting(label = "Amount", range(0, 1), span(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    #[setting(label = "Size", range(0.1, 1e9), span(0.3, 6), unit = "px", simple, keyframe)]
    pub size: f32,
}

impl Default for MistLayer {
    fn default() -> Self {
        MistLayer { amount: 0.7, size: 1.2 }
    }
}

/// Caustics: bright moving ripples of light inside it, strongest near the top.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CausticsLayer {
    #[setting(label = "Amount", range(0, 1e9), span(0, 2), simple, keyframe, pulse)]
    pub amount: f32,
    #[setting(label = "Size", range(1, 1e9), span(10, 300), unit = "px", simple, keyframe)]
    pub size: f32,
    #[setting(label = "Speed", range(0, 1e9), span(0, 3), keyframe, pulse)]
    pub speed: f32,
    /// How far down they reach before fading.
    #[setting(label = "Reach down", range(1, 1e9), span(20, 1500), unit = "px", keyframe)]
    pub deep: f32,
    #[setting(label = "Colour")]
    pub colour: Color,
}

impl Default for CausticsLayer {
    fn default() -> Self {
        CausticsLayer { amount: 0.5, size: 70.0, speed: 0.6, deep: 400.0, colour: Color::rgb(255, 255, 255) }
    }
}

/// Wet walls: where the liquid has been on a wall stays wet, runs down in streaks and dries.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WetWallsLayer {
    #[setting(label = "Amount", range(0, 1e9), span(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How long a wet wall takes to dry.
    #[setting(label = "Dries in", range(0.05, 1e9), span(0.2, 20), unit = "s", simple, keyframe)]
    pub dries: f32,
    /// How much the wet breaks into streaks.
    #[setting(label = "Streaks", range(0, 1), span(0, 1), keyframe)]
    pub streaks: f32,
}

impl Default for WetWallsLayer {
    fn default() -> Self {
        WetWallsLayer { amount: 0.5, dries: 4.0, streaks: 0.6 }
    }
}

/// Wet balls: balls come out of it shiny and tinted, with a drip, and dry. Needs the liquid to
/// push balls (only then does it know when they go in and come out).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WetBallsLayer {
    #[setting(label = "Shine", range(0, 1e9), span(0, 2), simple, keyframe, pulse)]
    pub shine: f32,
    /// How much they take on its colour while wet.
    #[setting(label = "Tint", range(0, 1), span(0, 1), keyframe)]
    pub tint: f32,
    #[setting(label = "Dries in", range(0.05, 1e9), span(0.2, 20), unit = "s", simple, keyframe)]
    pub dries: f32,
    /// How big the drip hanging under a wet ball is.
    #[setting(label = "Drips", range(0, 1e9), span(0, 1), keyframe)]
    pub drips: f32,
}

impl Default for WetBallsLayer {
    fn default() -> Self {
        WetBallsLayer { shine: 0.8, tint: 0.4, dries: 3.0, drips: 0.6 }
    }
}

/// The layers a Look card holds (each `None` when missing or off).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookLayers {
    pub foam: Option<FoamLayer>,
    pub bubbles: Option<BubblesLayer>,
    pub inside: Option<InsideLayer>,
    pub mist: Option<MistLayer>,
    pub caustics: Option<CausticsLayer>,
    pub wet_walls: Option<WetWallsLayer>,
    pub wet_balls: Option<WetBallsLayer>,
}

impl LookLayers {
    /// The layers of `look` (resolve leaves out the ones that are off); the first of a kind.
    pub fn read(look: Option<&ResolvedSlot>) -> LookLayers {
        let Some(card) = look.filter(|c| c.kind == LOOK) else { return LookLayers::default() };
        let of = |kind: &str| {
            card.layers.iter().find(|l| l.kind == kind && l.parent.is_none()).map(|l| card.layer_params(l.id))
        };
        LookLayers {
            foam: of(FOAM).map(|p| {
                let (s, d) = (FoamLayer::from_params(&p), FoamLayer::default());
                FoamLayer {
                    amount: num(s.amount, d.amount).max(0.0),
                    from_speed: num(s.from_speed, d.from_speed).max(0.0),
                    splashes: num(s.splashes, d.splashes).max(0.0),
                    on_top: num(s.on_top, d.on_top).max(0.0),
                    froth: num(s.froth, d.froth).max(0.5),
                    ..s
                }
            }),
            bubbles: of(BUBBLES).map(|p| {
                let (s, d) = (BubblesLayer::from_params(&p), BubblesLayer::default());
                BubblesLayer {
                    amount: num(s.amount, d.amount).clamp(0.0, 1.0),
                    size: num(s.size, d.size).max(0.5),
                    variety: num(s.variety, d.variety).clamp(0.0, 1.0),
                    rise: num(s.rise, d.rise),
                    wobble: num(s.wobble, d.wobble).clamp(0.0, 1.0),
                    shine: num(s.shine, d.shine).clamp(0.0, 2.0),
                }
            }),
            inside: of(INSIDE).map(|p| {
                let (s, d) = (InsideLayer::from_params(&p), InsideLayer::default());
                InsideLayer {
                    share: num(s.share, d.share).clamp(0.0, 1.0),
                    size: num(s.size, d.size).max(0.1),
                    sparkle: num(s.sparkle, d.sparkle).clamp(0.0, 1.0),
                    ..s
                }
            }),
            mist: of(MIST).map(|p| {
                let (s, d) = (MistLayer::from_params(&p), MistLayer::default());
                MistLayer { amount: num(s.amount, d.amount).clamp(0.0, 1.0), size: num(s.size, d.size).max(0.1) }
            }),
            caustics: of(CAUSTICS).map(|p| {
                let (s, d) = (CausticsLayer::from_params(&p), CausticsLayer::default());
                CausticsLayer {
                    amount: num(s.amount, d.amount).max(0.0),
                    size: num(s.size, d.size).max(1.0),
                    speed: num(s.speed, d.speed).max(0.0),
                    deep: num(s.deep, d.deep).max(1.0),
                    ..s
                }
            }),
            wet_walls: of(WET_WALLS).map(|p| {
                let (s, d) = (WetWallsLayer::from_params(&p), WetWallsLayer::default());
                WetWallsLayer {
                    amount: num(s.amount, d.amount).max(0.0),
                    dries: num(s.dries, d.dries).max(0.05),
                    streaks: num(s.streaks, d.streaks).clamp(0.0, 1.0),
                }
            }),
            wet_balls: of(WET_BALLS).map(|p| {
                let (s, d) = (WetBallsLayer::from_params(&p), WetBallsLayer::default());
                WetBallsLayer {
                    shine: num(s.shine, d.shine).max(0.0),
                    tint: num(s.tint, d.tint).clamp(0.0, 1.0),
                    dries: num(s.dries, d.dries).max(0.05),
                    drips: num(s.drips, d.drips).max(0.0),
                }
            }),
        }
    }
}

/// The Look card's layer kinds.
pub fn kinds() -> Vec<Kind> {
    let layer = |id, name, help, settings| Kind {
        id,
        name,
        help,
        section: LayerSection::Around,
        repeats: false,
        base: false,
        card: LOOK,
        takes_subs: false,
        settings,
        dynamic: crate::modules::look::layers::no_more,
    };
    vec![
        layer(
            FOAM,
            "White water and foam",
            "White froth where the liquid is fast or splashed, near its top.",
            FoamLayer::defs,
        ),
        layer(BUBBLES, "Bubbles", "Bubbles rising through the liquid and popping at the top.", BubblesLayer::defs),
        layer(
            INSIDE,
            "Things inside",
            "Flecks or glitter carried along by the liquid, glinting now and then.",
            InsideLayer::defs,
        ),
        layer(MIST, "Mist", "The spray thrown off the liquid, as fine dots.", MistLayer::defs),
        layer(CAUSTICS, "Caustics", "Bright moving ripples of light inside the liquid.", CausticsLayer::defs),
        layer(
            WET_WALLS,
            "Wet walls",
            "Where the liquid has been on a wall stays wet, runs down in streaks and dries.",
            WetWallsLayer::defs,
        ),
        layer(
            WET_BALLS,
            "Wet balls",
            "Balls come out shiny and tinted, with a drip, and dry (when the liquid pushes balls).",
            WetBallsLayer::defs,
        ),
    ]
}

/// Whether `kind` is one of the Look card's layer kinds.
pub fn is_look_layer(kind: &str) -> bool {
    [FOAM, BUBBLES, INSIDE, MIST, CAUSTICS, WET_WALLS, WET_BALLS].contains(&kind)
}
