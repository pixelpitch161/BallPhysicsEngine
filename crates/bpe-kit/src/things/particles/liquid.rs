//! The **Liquid** Look layer (particles part 6): the particles run together into one smooth
//! surface. Each particle is a blob of the field the graphics card cuts into a surface and shades
//! ([`bpe_gfx::Liquid`]); this side picks each blob's size, weight and colour and the surface's
//! look.
//!
//! Its **Surface** is the one balls and shapes wear (`modules::look::surface`, W15): in a scene
//! with a Lighting card the real lights shade it through that Surface; without one (or at Lit 0)
//! it draws its own pretend light, set in the "Without lighting" fold.

use std::sync::LazyLock;

use bpe_core::{Blend, Color, LayerId, Params, Resolved, SettingDef, Settings, SlotId, ThingId};
use bpe_gfx::{LiquidBlob, LiquidStyle};

use super::look::{ColourFrom, Common, FineDetail, MOST_COPIES, Only, look_settings};
use crate::modules::look::layers::particles::{ParticleFade, SizeOverLife};
use crate::modules::look::layers::trail::AroundPlace;
use crate::modules::look::surface::{self, SurfaceKind, SurfaceSettings};
use crate::paint::{Follow, Paint};

/// The layer kind.
pub const LIQUID: &str = "particles.liquid";

/// The pretend light's surface code (in [`LiquidStyle::a`]) for Surface `kind`: the nearest of the
/// six the pretend light draws (Matte, Plastic, Chrome, Metal, Glass, Toon).
pub fn pretend_code(kind: SurfaceKind) -> f32 {
    use SurfaceKind as K;
    match kind {
        K::Matte | K::Velvet => 0.0,
        K::Plastic | K::Jelly | K::Pearl | K::Flake | K::Lava => 1.0,
        K::Chrome | K::MirrorBall => 2.0,
        K::Metal | K::Brushed => 3.0,
        K::Glass | K::Frosted | K::Prism => LiquidStyle::GLASS,
        K::Toon => 5.0,
    }
}

look_settings! {
    /// Liquid: the particles as one smooth surface.
    LiquidLayer {
        /// How far each particle's blob reaches, in its sizes: bigger blobs run together sooner.
        #[setting(label = "Blob size", group = "Liquid", span(0, 12), simple, keyframe, pulse)]
        blob: f32 = 4.0,
        /// Where the surface is cut: higher, thinner and more broken up; lower, fuller.
        #[setting(label = "Level", group = "Liquid", span(0.05, 2), simple, keyframe)]
        level: f32 = 0.5,
        /// How soft its edge is.
        #[setting(label = "Edge softness", group = "Liquid", span(0, 10), unit = "px", keyframe)]
        softness: f32 = 1.5,
        /// How far in from its edge the surface rounds over (with lighting, how its edges turn to
        /// the lights).
        #[setting(label = "Roundness", group = "Liquid", span(0, 60), unit = "px", keyframe)]
        roundness: f32 = 14.0,
        /// What its surface is like, as a ball's: picking one sets the numbers below it. With a
        /// Lighting card the real lights shade it so; without one it takes the nearest of its own
        /// pretend looks.
        #[setting(label = "Surface", group = "Surface", simple)]
        surface: SurfaceKind = SurfaceKind::Plastic,
        /// The way its own light comes from, on the screen (0 from the right, -90 from above;
        /// only without a Lighting card, or at Lit 0).
        #[setting(label = "Light angle", group = "Without lighting", span(-180, 180), unit = "°", keyframe)]
        light_angle: f32 = -120.0,
        /// How sharp and bright its own highlight is (only without a Lighting card, or at Lit 0).
        #[setting(label = "Shine", group = "Without lighting", range(0, 1), keyframe, pulse)]
        shine: f32 = 0.6,
        /// Its own light round its edge (only without a Lighting card, or at Lit 0).
        #[setting(label = "Rim light", group = "Without lighting", range(0, 1), keyframe)]
        own_rim: f32 = 0.3,
        /// How far glass bends what is behind it (only without a Lighting card, or at Lit 0: with
        /// one, its Surface's Bends light does).
        #[setting(
            label = "Bend",
            group = "Without lighting",
            when = "surface=glass|frosted|prism",
            span(0, 60),
            unit = "px",
            keyframe
        )]
        bend: f32 = 12.0,
        /// How much glass takes on its colours (only without a Lighting card, or at Lit 0).
        #[setting(
            label = "Tint",
            group = "Without lighting",
            when = "surface=glass|frosted|prism",
            range(0, 1),
            keyframe
        )]
        tint: f32 = 0.3,
        /// The sky it reflects, above (only without a Lighting card, or at Lit 0: with one, the
        /// Lighting card's sky).
        #[setting(label = "Sky top", group = "Without lighting")]
        sky_top: Color = Color::rgb(225, 235, 255),
        /// The sky it reflects, below (only without a Lighting card, or at Lit 0).
        #[setting(label = "Sky bottom", group = "Without lighting")]
        sky_bottom: Color = Color::rgb(40, 40, 50),
    }
}

/// The Surface numbers a liquid leaves out: it throws no lamp light back (no thrower) and has no
/// tiles (a ball's only).
pub const NOT_ON_A_LIQUID: [&str; 3] = ["throws", "facets", "facet_gaps"];

/// The Liquid layer kind's settings: [`LiquidLayer`]'s with the Surface's numbers (as a Surface
/// sub-layer has them, but [`NOT_ON_A_LIQUID`]) right after its Surface, starting as Plastic's.
pub fn defs() -> &'static [SettingDef] {
    static DEFS: LazyLock<Vec<SettingDef>> = LazyLock::new(|| {
        let plastic = surface::preset_values(SurfaceKind::Plastic);
        let wears = |d: &&SettingDef| d.key != "surface" && !NOT_ON_A_LIQUID.contains(&d.key);
        let numbers = surface::sub_defs().iter().filter(wears).map(|d| {
            let mut d = d.clone();
            if let Some((_, v)) = plastic.iter().find(|(k, _)| *k == d.key) {
                d.default = v.clone();
            }
            d
        });
        let own = LiquidLayer::defs();
        let at = own.iter().position(|d| d.key == "surface").map_or(own.len(), |i| i + 1);
        own[..at].iter().cloned().chain(numbers).chain(own[at..].iter().cloned()).collect()
    });
    &DEFS
}

/// A Liquid layer's Surface, from its settings `p`.
pub fn surface_of(p: &Params) -> SurfaceSettings {
    SurfaceSettings::from_params(p)
}

/// `l`'s settings with its Surface's numbers, as picking that Surface in the editor writes them.
pub fn params_of(l: &LiquidLayer) -> Params {
    let mut p = l.to_params();
    for (k, v) in surface::preset_values(l.surface) {
        p.set(k, v);
    }
    p
}

/// The surface table index of Particles thing `id`'s Liquid layer `layer` in a lit frame: after
/// every thing's surface and every Surface sub-layer's ([`surface::taken`]), counting every Liquid
/// layer of every Particles Look in thing order then layer order (shown or not), so the draw and
/// the light hook agree from the scene alone.
pub fn surface_index(scene: &Resolved, id: ThingId, layer: LayerId) -> Option<u32> {
    let mut n = surface::taken(scene);
    for (tid, t) in &scene.things {
        let Some(look) = t.slots.get(&SlotId::Look).filter(|l| l.kind == super::look::CARD) else { continue };
        for l in look.layers.iter().filter(|l| l.kind == LIQUID && l.parent.is_none()) {
            n += 1;
            if *tid == id && l.id == layer {
                return Some(n);
            }
        }
    }
    None
}

/// How a liquid takes the real lights of a lit scene (W15): its layer's Lit (above 0), its
/// surface stamp ([`surface::stamp`]), how much it blocks light and its thing's link number
/// (`links::number_of`, for lights that light up only some things).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RealLight {
    pub lit: f32,
    pub stamp: f32,
    pub blocks: f32,
    pub link: f32,
}

impl LiquidLayer {
    /// The surface pass's numbers: its Surface `s`, lit by the real lights when `real` is some.
    pub fn style(&self, opacity: f32, s: &SurfaceSettings, real: Option<RealLight>) -> LiquidStyle {
        let sky = |c: Color| {
            let l = c.to_linear();
            [l[0], l[1], l[2], 0.0]
        };
        let code = pretend_code(self.surface);
        // Its own Glass bends lamp light as free glass; lit by the real lights, it bends it as a
        // glass shape does, through its surface (a bender the light hook makes), not here.
        let ior = if s.bends_light.is_finite() { s.bends_light } else { 0.0 };
        let bends = if real.is_none() && code == LiquidStyle::GLASS && ior > bpe_gfx::BENDS_FROM { ior } else { 0.0 };
        LiquidStyle {
            a: [self.level.max(1e-3), self.softness.max(0.0), self.roundness.max(0.0), code],
            b: [self.light_angle.to_radians(), self.shine.clamp(0.0, 1.0), self.bend, self.tint.clamp(0.0, 1.0)],
            c: [self.own_rim.clamp(0.0, 1.0), opacity.clamp(0.0, 1.0), bends, 0.0],
            sky_top: sky(self.sky_top),
            sky_bottom: sky(self.sky_bottom),
            d: real.map_or([0.0; 4], |r| [r.lit.max(0.0), r.stamp, r.blocks.clamp(0.0, 1.0), r.link]),
        }
    }

    /// The blob of a particle at `at`, half size `size`, showing `alpha` of itself, coloured
    /// `colour` (linear).
    pub fn blob(&self, at: glam::Vec2, size: f32, alpha: f32, colour: [f32; 4]) -> LiquidBlob {
        LiquidBlob {
            place: [at.x, at.y, (size * self.blob).max(0.0), alpha.clamp(0.0, 1.0)],
            colour: [colour[0], colour[1], colour[2], 1.0],
        }
    }
}

impl LiquidLayer {
    fn starting(s: Self) -> Self {
        s
    }
}
