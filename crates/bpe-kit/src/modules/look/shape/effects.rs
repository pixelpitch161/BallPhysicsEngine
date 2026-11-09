//! A Text's experimental effects (Text part 5): layers in the Reaction section of its `look.shape`
//! card beside the letter moves, offered only on a Text, listed by the editor under
//! Experimental. Echo, Mirror, Colour split and Glitch copy, cut and colour the Text's drawn
//! letters; Melt reshapes their outlines (`things::text::fx`). Drawing only, pure in time.

use bpe_core::{Choice, SettingDef, Settings, Target};
use glam::Vec2;

use super::moves::MoveKind;

pub const ECHO: &str = "text.fx.echo";
pub const MIRROR: &str = "text.fx.mirror";
pub const SPLIT: &str = "text.fx.split";
pub const GLITCH: &str = "text.fx.glitch";
pub const MELT: &str = "text.fx.melt";

/// Every effect kind, in the add menu's order.
pub const EFFECTS: [&str; 5] = [ECHO, MIRROR, SPLIT, GLITCH, MELT];

/// Whether layer kind `kind` is an experimental effect (rather than a letter move).
pub fn is_effect(kind: &str) -> bool {
    EFFECTS.contains(&kind)
}

/// Echo: copies of the letters stacked toward a point that can wander, each smaller and turned
/// round the colour wheel, drawn behind the letters.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EchoEffect {
    /// How much the copies show (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How many copies.
    #[setting(label = "Copies", range(1, 64), span(1, 32), simple)]
    pub copies: u32,
    /// The point the copies stack toward, from the text's middle.
    #[setting(label = "Toward", unit = "px", simple, keyframe)]
    pub toward: Vec2,
    /// How far the point circles round Toward.
    #[setting(label = "Wander", span(0, 600), range(0, 100000), unit = "px", keyframe)]
    pub wander: f32,
    /// How many times a second the point goes round.
    #[setting(label = "Wander speed", when = "wander", span(-1, 1), unit = "per s", keyframe)]
    pub wander_speed: f32,
    /// How far toward the point the last copy goes (1 all the way).
    #[setting(label = "Reach", range(0, 1), keyframe)]
    pub reach: f32,
    /// How big the last copy is.
    #[setting(label = "Last size", span(0, 2), range(0, 100), unit = "×", keyframe)]
    pub last_size: f32,
    /// How far round the colour wheel each copy is turned from the one before.
    #[setting(label = "Hue each", span(-90, 90), unit = "°", keyframe)]
    pub hue: f32,
    /// How fast every copy's colour turns.
    #[setting(label = "Hue spin", span(-180, 180), unit = "°/s", keyframe)]
    pub hue_spin: f32,
    /// How far the copies take a pure colour of their own at their brightness (white letters
    /// need it to show colours).
    #[setting(label = "Colourise", range(0, 1), keyframe)]
    pub colourise: f32,
    /// How much the last copy shows.
    #[setting(label = "Last opacity", range(0, 1), keyframe)]
    pub last_opacity: f32,
}

impl Default for EchoEffect {
    fn default() -> Self {
        Self {
            amount: 1.0,
            copies: 12,
            toward: Vec2::new(0.0, -260.0),
            wander: 0.0,
            wander_speed: 0.2,
            reach: 0.8,
            last_size: 0.5,
            hue: 15.0,
            hue_spin: 0.0,
            colourise: 0.0,
            last_opacity: 0.4,
        }
    }
}

/// What Mirror turns its copies round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MirrorRound {
    #[default]
    TheTextsMiddle,
    TheMiddleOfThePicture,
    APoint,
}

/// Mirror: copies of the letters turned round a middle, 2 to 12 ways, flipped and cut to
/// wedges for a kaleidoscope.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MirrorEffect {
    /// How much the copies show (0 none, 1 all); the letters themselves stay.
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How many ways round: the letters and this many less one copies.
    #[setting(label = "Ways", range(2, 12), simple)]
    pub ways: u32,
    /// What the copies turn round.
    #[setting(label = "Round", simple)]
    pub round: MirrorRound,
    /// The point they turn round, for A point: on the screen for a Text on the screen, else in
    /// the world.
    #[setting(label = "Centre", when = "round=a_point", unit = "px", keyframe)]
    pub centre: Vec2,
    /// Every second copy also flipped over, as in a kaleidoscope.
    #[setting(label = "Mirrored copies")]
    pub mirrored: bool,
    /// Each copy (the letters too) cut to its own wedge round the middle, so the copies meet
    /// at the wedges' edges.
    #[setting(label = "Cut to wedges")]
    pub wedges: bool,
    /// How far the whole set is turned.
    #[setting(label = "Turn", span(-180, 180), unit = "°", keyframe)]
    pub turn: f32,
    /// How fast the whole set turns.
    #[setting(label = "Spin", span(-180, 180), unit = "°/s", keyframe)]
    pub spin: f32,
    /// How far round the colour wheel each copy is turned from the one before.
    #[setting(label = "Hue each copy", span(-180, 180), unit = "°", keyframe)]
    pub hue: f32,
    /// How far the copies take a pure colour of their own at their brightness.
    #[setting(label = "Colourise", range(0, 1), keyframe)]
    pub colourise: f32,
}

impl Default for MirrorEffect {
    fn default() -> Self {
        Self {
            amount: 1.0,
            ways: 6,
            round: MirrorRound::TheTextsMiddle,
            centre: Vec2::ZERO,
            mirrored: false,
            wedges: false,
            turn: 0.0,
            spin: 0.0,
            hue: 0.0,
            colourise: 0.0,
        }
    }
}

/// Colour split: red and blue copies of the letters pulled apart, further right after a hit.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SplitEffect {
    /// How much of the split shows (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How far red goes one way and blue the other.
    #[setting(label = "Distance", span(0, 40), range(0, 10000), unit = "px", simple, keyframe, pulse)]
    pub distance: f32,
    /// Which way red goes on the screen (0 to the right).
    #[setting(label = "Angle", span(-180, 180), unit = "°", keyframe)]
    pub angle: f32,
    /// How far the split wobbles back and forth on its own.
    #[setting(label = "Wobble", span(0, 40), range(0, 10000), unit = "px", keyframe)]
    pub wobble: f32,
    /// How fast it wobbles.
    #[setting(label = "Wobble speed", when = "wobble", span(0.1, 30), range(0, 1000), unit = "per s")]
    pub wobble_speed: f32,
    /// Another number gives another wobble.
    #[setting(label = "Seed", when = "wobble", span(0, 100), range(0, 1e9))]
    pub seed: u32,
    /// How much further it splits right after a hit (1: four times as far).
    #[setting(label = "Harder on hits", range(0, 1), keyframe)]
    pub harder: f32,
    /// Whose hits.
    #[setting(label = "Hits from", when = "harder")]
    pub hits_of: Target,
    /// How long it takes to settle after a hit.
    #[setting(label = "Settles over", when = "harder", span(0.05, 2), range(0.01, 60), unit = "s")]
    pub settles: f32,
}

impl Default for SplitEffect {
    fn default() -> Self {
        Self {
            amount: 1.0,
            distance: 8.0,
            angle: 0.0,
            wobble: 0.0,
            wobble_speed: 6.0,
            seed: 0,
            harder: 0.0,
            hits_of: Target::Anything,
            settles: 0.4,
        }
    }
}

/// Glitch: bands of the picture across the letters torn sideways at random moments, more and
/// further right after a hit.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct GlitchEffect {
    /// The share of bands torn at a moment.
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How tall each band is.
    #[setting(label = "Band height", span(2, 100), range(1, 10000), unit = "px", simple, keyframe)]
    pub band: f32,
    /// The furthest a torn band moves sideways.
    #[setting(label = "Tear", span(0, 200), range(0, 10000), unit = "px", simple, keyframe, pulse)]
    pub tear: f32,
    /// How often the tears change.
    #[setting(label = "Changes a second", span(0, 60), range(0, 1000), keyframe)]
    pub rate: f32,
    /// How far red and blue part in a torn band.
    #[setting(label = "Tear colour split", span(0, 30), range(0, 1000), unit = "px", keyframe)]
    pub tear_split: f32,
    /// Another number gives other tears.
    #[setting(label = "Seed", span(0, 100), range(0, 1e9))]
    pub seed: u32,
    /// How much more it tears right after a hit (1: every band, three times as far).
    #[setting(label = "Harder on hits", range(0, 1), keyframe)]
    pub harder: f32,
    /// Whose hits.
    #[setting(label = "Hits from", when = "harder")]
    pub hits_of: Target,
    /// How long it takes to settle after a hit.
    #[setting(label = "Settles over", when = "harder", span(0.05, 2), range(0.01, 60), unit = "s")]
    pub settles: f32,
}

impl Default for GlitchEffect {
    fn default() -> Self {
        Self {
            amount: 0.2,
            band: 16.0,
            tear: 40.0,
            rate: 10.0,
            tear_split: 6.0,
            seed: 0,
            harder: 0.6,
            hits_of: Target::Anything,
            settles: 0.4,
        }
    }
}

/// Melt: each column of the letters drips and stretches down the picture, then starts again.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MeltEffect {
    /// How far the drips run (0 none, 1 all).
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How long the longest drips get.
    #[setting(label = "Drip", span(0, 600), range(0, 100000), unit = "px", simple, keyframe)]
    pub drip: f32,
    /// How much of the melt happens each second.
    #[setting(label = "Speed", span(0.01, 2), range(0, 1000), unit = "per s", keyframe)]
    pub speed: f32,
    /// When it starts melting.
    #[setting(label = "Starts at", span(0, 30), range(0, 1e6), unit = "s")]
    pub start: f32,
    /// How often it starts again from the start (0: it melts once and stays).
    #[setting(label = "Starts again every", span(0, 30), range(0, 1e6), unit = "s")]
    pub every: f32,
    /// How wide each drip is.
    #[setting(label = "Drip width", span(2, 60), range(0.5, 10000), unit = "px", keyframe)]
    pub width: f32,
    /// How much the tops of the letters sink too (0: only the bottoms run).
    #[setting(label = "Sag", range(0, 1), keyframe)]
    pub sag: f32,
    /// Another number gives other drips.
    #[setting(label = "Seed", span(0, 100), range(0, 1e9))]
    pub seed: u32,
}

impl Default for MeltEffect {
    fn default() -> Self {
        Self { amount: 1.0, drip: 160.0, speed: 0.2, start: 0.0, every: 0.0, width: 12.0, sag: 0.25, seed: 0 }
    }
}

/// Every effect kind, in the add menu's order.
pub fn kinds() -> Vec<MoveKind> {
    let k =
        |id, name, help, settings: fn() -> &'static [SettingDef]| MoveKind { id, name, help, settings, shapes: false };
    vec![
        k(
            ECHO,
            "Echo",
            "Copies of the letters stacked toward a moving point, each smaller and another colour.",
            EchoEffect::defs,
        ),
        k(
            MIRROR,
            "Mirror",
            "Copies of the letters turned round a middle, 2 to 12 ways: a kaleidoscope.",
            MirrorEffect::defs,
        ),
        k(SPLIT, "Colour split", "Red and blue copies pulled apart, further right after a hit.", SplitEffect::defs),
        k(GLITCH, "Glitch", "Bands of the letters torn sideways now and then, harder on hits.", GlitchEffect::defs),
        k(MELT, "Melt", "The letters drip and stretch down the picture, then start again.", MeltEffect::defs),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_fit_only_a_text_and_are_no_moves() {
        let reg = crate::registry();
        for id in EFFECTS {
            let k = reg.layer(id).unwrap();
            assert!(k.fits(crate::things::text::KIND) && !k.fits(crate::things::shape::KIND), "{id}");
            assert_eq!(k.section(), bpe_core::LayerSection::Reaction);
            assert!(!super::super::moves::MOVES.contains(&id));
            assert_eq!(k.settings()[0].key, "amount", "{id}: Amount first");
        }
        assert!(bpe_core::check_registry(&reg).is_empty(), "{:?}", bpe_core::check_registry(&reg));
    }
}
