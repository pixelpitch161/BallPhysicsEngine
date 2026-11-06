//! **Gives off light** (lighting spec part 1b): a sub-layer that makes its layer a lamp. With a
//! Lighting card on the scene the light comes from where the layer covers (after its cuts) and
//! lights everything round it through the light field, with soft shadows from what blocks light;
//! the layer itself then shows its own colours, unlit. Without the card it changes nothing.
//!
//! The sub-layer writes no record of its own: its light is folded into its layer's record
//! ([`bpe_gfx::LayerGpu::emit`]) or, for trails, particles and the wavefront, stamped on what
//! they draw ([`Emit::stamp`]). Several on one layer add up.

use bpe_core::{Choice, Color, Settings};

/// The ball sub-layer kind's id.
pub const SUB_LIGHT: &str = "sub.light";
/// The shape sub-layer kind's id.
pub const SHAPE_SUB_LIGHT: &str = "shape.sub.light";
/// What the sub-layer kind says about itself.
pub const HELP: &str = "Makes its layer give off light that lights everything round it (with a Lighting card).";

/// Where the light's colour comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum EmitFrom {
    /// The layer's own colours, wherever it covers.
    #[default]
    ItsColours,
    /// A colour you pick.
    Own,
}

/// The Gives off light sub-layer's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct GivesLight {
    /// Where the light's colour comes from.
    #[setting(label = "Colour from", simple)]
    pub colour_from: EmitFrom,
    /// The light's colour.
    #[setting(label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Color,
    /// How bright its light is. Below 0 it gives darkness.
    #[setting(label = "Strength", span(0, 8), simple, keyframe, pulse)]
    pub strength: f32,
    /// Gives darkness instead of light: it takes light away from what is round it.
    #[setting(label = "Negative")]
    pub negative: bool,
    /// How much brighter it flares when its ball (or shape) is hit (not on trails and particles).
    #[setting(label = "Flare on hit", span(0, 8), unit = "×", keyframe, group = "On hits")]
    pub flare: f32,
    /// How long a flare takes to fade.
    #[setting(
        label = "Flare length",
        when = "flare",
        range(0.001, 1e9),
        span(0.02, 2),
        unit = "s",
        keyframe,
        group = "On hits"
    )]
    pub flare_length: f32,
}

impl Default for GivesLight {
    fn default() -> Self {
        Self {
            colour_from: EmitFrom::ItsColours,
            colour: Color::rgb(0xFF, 0xE6, 0xC0),
            strength: 2.0,
            negative: false,
            flare: 0.0,
            flare_length: 0.4,
        }
    }
}

/// The light a layer gives off, added up from its shown Gives off light sub-layers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Emit {
    /// Own colour times strength (linear rgb).
    pub own: [f32; 3],
    /// What the layer's own colours are multiplied by.
    pub tint: [f32; 3],
    /// The biggest flare on hits and its length (seconds).
    pub flare: f32,
    pub length: f32,
    /// Whether the layer has a shown Gives off light at all (it is then a lamp, unlit).
    pub on: bool,
}

impl Emit {
    /// The light `g` gives off.
    pub fn of(g: &GivesLight) -> Emit {
        let mut e = Emit::default();
        e.add(g);
        e
    }

    /// Adds the light `g` gives off.
    pub fn add(&mut self, g: &GivesLight) {
        let k = if g.negative { -g.strength } else { g.strength };
        let k = if k.is_finite() { k } else { 0.0 };
        match g.colour_from {
            EmitFrom::Own => {
                let c = g.colour.to_linear();
                for (o, c) in self.own.iter_mut().zip(c) {
                    *o += c * k;
                }
            }
            EmitFrom::ItsColours => {
                for t in &mut self.tint {
                    *t += k;
                }
            }
        }
        let flare = if g.flare.is_finite() { g.flare } else { 0.0 };
        if flare.abs() > self.flare.abs() {
            self.flare = flare;
            self.length = g.flare_length.max(0.001);
        }
        self.on = true;
    }

    /// The light of every Gives off light among `subs` (sub-layers of any kind; the rest are
    /// passed over); `None` when there is none.
    pub fn of_subs<'a>(subs: impl IntoIterator<Item = &'a super::gpu::SubLayer>) -> Option<Emit> {
        let mut out: Option<Emit> = None;
        for s in subs {
            if let super::gpu::SubLayer::Light(g) = s {
                out.get_or_insert_with(Emit::default).add(g);
            }
        }
        out
    }

    /// As a layer record carries it ([`bpe_gfx::LayerGpu::emit`]).
    pub fn gpu(&self) -> [[f32; 4]; 2] {
        [
            [self.own[0], self.own[1], self.own[2], self.flare],
            [self.tint[0], self.tint[1], self.tint[2], self.length.max(0.001)],
        ]
    }

    /// The light given off where straight linear colour `c` covers (trails, particles).
    pub fn at(&self, c: [f32; 4]) -> [f32; 4] {
        [self.own[0] + self.tint[0] * c[0], self.own[1] + self.tint[1] * c[1], self.own[2] + self.tint[2] * c[2], 0.0]
    }
}

/// The light of `emit`, or none, as a layer record carries it.
pub fn gpu_of(emit: Option<Emit>) -> [[f32; 4]; 2] {
    emit.map_or([[0.0; 4]; 2], |e| e.gpu())
}

/// The light of `emit` without its flare on hits, as a record carries it: for what an around
/// layer draws as balls (ghosts, mini balls), which reads its ball's hits but has no flare.
pub fn gpu_steady(emit: Option<Emit>) -> [[f32; 4]; 2] {
    gpu_of(emit.map(|e| Emit { flare: 0.0, ..e }))
}

/// A layer's Lighting ([`super::lighting::of`]) with its Lit taken away when it gives off light:
/// a lamp shows its own colours.
pub fn unlit_if_lamp(light: [f32; 4], emit: Option<Emit>) -> [f32; 4] {
    if emit.is_some_and(|e| e.on) { [0.0, light[1], light[2], light[3]] } else { light }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_colour_and_its_colours_add_up_and_negative_takes_light_away() {
        let own = GivesLight { colour_from: EmitFrom::Own, colour: Color::WHITE, strength: 2.0, ..Default::default() };
        let its = GivesLight { strength: 3.0, ..Default::default() };
        let dark = GivesLight { strength: 1.0, negative: true, ..Default::default() };
        let mut e = Emit::of(&own);
        e.add(&its);
        e.add(&dark);
        assert_eq!(e.own, [2.0, 2.0, 2.0]);
        assert_eq!(e.tint, [2.0, 2.0, 2.0]);
        assert_eq!(e.at([0.5, 0.0, 1.0, 1.0]), [3.0, 2.0, 4.0, 0.0]);
        assert!(e.on);
    }

    #[test]
    fn a_lamp_is_unlit_and_the_rest_keep_their_lit() {
        let e = Emit::of(&GivesLight::default());
        assert_eq!(unlit_if_lamp([1.0, 1.0, 0.5, 0.0], Some(e)), [0.0, 1.0, 0.5, 0.0]);
        assert_eq!(unlit_if_lamp([1.0, 1.0, 0.5, 0.0], None), [1.0, 1.0, 0.5, 0.0]);
        assert_eq!(gpu_of(None), [[0.0; 4]; 2]);
    }

    #[test]
    fn the_defaults_and_keys() {
        let g = GivesLight::default();
        assert_eq!((g.strength, g.flare, g.colour_from), (2.0, 0.0, EmitFrom::ItsColours));
        let keys: Vec<&str> = GivesLight::defs().iter().map(|d| d.key).collect();
        assert_eq!(keys, ["colour_from", "colour", "strength", "negative", "flare", "flare_length"]);
    }
}
