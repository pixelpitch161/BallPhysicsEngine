//! How a Liquid thing draws: its drops, straight from the card's buffers (the host's body), run
//! together into a surface (the particles' liquid pass), each drop in its own colour as mixed.
//! Every Liquid thing draws only its own drops, with its own Look.
//!
//! Liquid part 3: the full look (its Surface, depth and clarity, bending what is behind it,
//! ripples, thin film, outline, the light it gives off, where colours mix) and its layers (foam,
//! bubbles, things inside, mist, caustics, wet walls, wet balls). With a Lighting card (and Lit
//! above 0) the real lights shade it through its Surface, as they do the Particles Liquid layer
//! (W15): its colour, height and surface index go into the light passes.

use bpe_core::{Resolved, SlotId, ThingId};
use bpe_gfx::liquid_sim::STAIN_BASE;
use bpe_gfx::{DrawCx, Frame, Liquid, LiquidStyle, ThingDraw};

use super::KIND;
use super::cards::{ColourFrom, LOOK, LookSettings, ShowsAs};
use super::look::{FleckColour, LookLayers, surface_of};
use super::state::{host_of, state_of};
use crate::modules::look::surface;
use crate::things::particles::liquid::pretend_code;

pub struct LiquidDraw;

/// Each drop's reach, in cells, at Smoothing `smoothing`: 1 + 2 × smoothing (the default 0.25
/// gives part 1's 1.5: at 1 cell the random packing of poured drops showed as lumps and pits all
/// over the surface).
pub fn reach_of(smoothing: f32) -> f32 {
    1.0 + 2.0 * smoothing.clamp(0.0, 4.0)
}
/// Where a stain's surface is cut, against its peak field (1 for one lone drop): low, so one
/// dried drop shows.
const STAIN_LEVEL: f32 = 0.35;

/// How a liquid takes the real lights of a lit scene: its Lit (above 0), its surface stamp, how
/// much it blocks light and its thing's link number.
pub type Real = crate::things::particles::liquid::RealLight;

fn lin(c: bpe_core::Color) -> [f32; 3] {
    let l = c.to_linear();
    [l[0], l[1], l[2]]
}

fn four(c: [f32; 3], w: f32) -> [f32; 4] {
    [c[0], c[1], c[2], w]
}

/// The surface pass's numbers for `look` (with its `layers`), the drops of liquid
/// `index` on the grid at `origin` with cells `cell` wide, lit by the real lights when `real` is
/// some.
pub fn style_of(
    look: &LookSettings,
    layers: &LookLayers,
    index: usize,
    (origin, cell): (glam::Vec2, f32),
    real: Option<Real>,
) -> LiquidStyle {
    let one = look.colour.to_linear();
    let reach = reach_of(look.smoothing);
    let inside = std::f32::consts::PI * reach * reach;
    let sky = |c| four(lin(c), 0.0);
    let code = pretend_code(look.surface);
    let mut style = LiquidStyle {
        a: [look.level * inside, look.softness, look.roundness, code],
        b: [look.light_angle.to_radians(), look.shine, look.bend, look.tint],
        c: [look.own_rim, look.opacity, 0.0, 0.0],
        sky_top: sky(look.sky_top),
        sky_bottom: sky(look.sky_bottom),
        d: real.map_or([0.0; 4], |r| [r.lit.max(0.0), r.stamp, r.blocks.clamp(0.0, 1.0), r.link]),
        e: [origin.x, origin.y, cell, cell * reach],
        f: [look.swirl_detail, 1.0, index as f32, if look.colour_from == ColourFrom::OneColour { 1.0 } else { 0.0 }],
        g: [one[0], one[1], one[2], 0.0],
        h: [look.clarity, look.soaks_in, look.sky_reflects, look.split],
        i: four(lin(look.deep), look.outline),
        j: [look.ripples, look.ripple_size, look.thin_film, look.film_size],
        k: [
            look.glow,
            look.mixing_shows,
            match look.shows_as {
                ShowsAs::Shimmer => 0.0,
                ShowsAs::Glow => 1.0,
                ShowsAs::Foam => 2.0,
            },
            0.0,
        ],
        l: four(lin(look.outline_colour), 0.0),
        v: [0.0, 0.0, 0.0, 1.0],
        w: [one[0], one[1], one[2], 0.0],
        ..LiquidStyle::default()
    };
    if let Some(f) = &layers.foam {
        style.m = [f.amount, f.from_speed, f.splashes, f.on_top];
        style.n = four(lin(f.colour), f.froth);
    }
    if let Some(b) = &layers.bubbles {
        style.o = [b.amount, b.size, b.variety, b.rise];
        style.p[0] = b.wobble;
        style.p[1] = b.shine;
    }
    if let Some(c) = &layers.caustics {
        style.p[2] = c.amount;
        style.p[3] = c.size;
        style.q[0] = c.speed;
        style.q[1] = c.deep;
        style.t = four(lin(c.colour), 0.0);
    }
    if let Some(i) = &layers.inside {
        style.q[2] = i.share;
        style.q[3] = i.size;
        style.r = four(lin(i.colour), if i.colour_from == FleckColour::OneColour { 1.0 } else { 0.0 });
        style.s[0] = i.sparkle;
    }
    if let Some(m) = &layers.mist {
        style.s[1] = m.amount;
        style.s[2] = m.size;
    }
    if let Some(w) = &layers.wet_walls {
        let floor = (-w.dries / bpe_gfx::liquid_sim::DRY_SECONDS).exp();
        style.u = [w.amount, floor, w.streaks, 0.0];
    }
    style
}

/// Whether the scene has a Lighting card (so its frames are lit).
fn lit_scene(scene: &Resolved) -> bool {
    scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Lighting))
}

/// The surface table index of Liquid thing `id` in a lit frame: after every thing's and Surface
/// sub-layer's ([`surface::taken`]) and every Particles Liquid layer's, by its place among the
/// scene's Liquid things (tree order), so the draw and the light hook agree from the scene alone.
pub fn surface_index(scene: &Resolved, id: ThingId) -> Option<u32> {
    let mut n = surface::taken(scene) + crate::things::particles::liquid::layers_taken(scene);
    for (tid, t) in &scene.things {
        if t.kind != KIND {
            continue;
        }
        n += 1;
        if *tid == id {
            return Some(n);
        }
    }
    None
}

/// How `cx`'s liquid takes the real lights, when the scene is lit and its Lit is above 0.
fn real_of(cx: &DrawCx, look: &LookSettings, s: &surface::SurfaceSettings) -> Option<Real> {
    if !lit_scene(cx.scene) || look.lit <= 0.0 {
        return None;
    }
    let index = surface_index(cx.scene, cx.thing.id).unwrap_or(0);
    let blocks = if look.blocks_light { surface::keeps_of(surface::passes(s)) } else { 0.0 };
    let link = crate::things::light::links::number_of(cx.scene, cx.thing.id);
    Some(Real { lit: look.lit, stamp: surface::stamp(index, s), blocks, link })
}

impl ThingDraw for LiquidDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(index) = state_of(cx.world, cx.thing.id).and_then(|s| s.index) else { return };
        let Some(host) = host_of(cx.world).and_then(|h| h.host.as_ref()) else { return };
        let Some(body) = host.body.as_ref() else { return };
        let card = cx.resolved.slots.get(&SlotId::Look).filter(|c| c.kind == LOOK);
        let look = LookSettings::read(card);
        let layers = LookLayers::read(card);
        let s = surface_of(card);
        let real = real_of(cx, &look, &s);
        let grid = (host.layout.origin, host.layout.cell);
        let style = style_of(&look, &layers, index, grid, real);
        frame.liquids.push(Liquid { blobs: Vec::new(), style, front: look.in_front, drops: Some(body.source()) });
        // Its stains (part 4): flat marks where drops dried, behind the balls, never lit.
        let me = state_of(cx.world, cx.thing.id).map(|s| &s.settings);
        if let Some(s) = me.filter(|s| s.dries > 0.0 && s.stains) {
            let reach = reach_of(look.smoothing);
            let flat = LookSettings {
                level: STAIN_LEVEL / (std::f32::consts::PI * reach * reach),
                roundness: 0.0,
                shine: 0.0,
                bend: 0.0,
                split: 0.0,
                ripples: 0.0,
                thin_film: 0.0,
                outline: 0.0,
                glow: 0.0,
                softness: look.softness.max(1.0),
                opacity: look.opacity * s.stain_strength,
                ..look.clone()
            };
            let stains = STAIN_BASE as usize + index;
            let style = style_of(&flat, &LookLayers::default(), stains, grid, None);
            frame.liquids.push(Liquid { blobs: Vec::new(), style, front: false, drops: Some(body.source()) });
        }
        if let Some(w) = &layers.wet_balls {
            let wet = super::wet::wetness(cx.log, cx.world.steps_per_second, cx.time, cx.thing.id, w.dries);
            let colour = lin(look.colour);
            super::wet::draw_wet_balls(frame, cx.world, &wet, colour, w);
        }
    }

    /// Its Surface, in its place in the lit frame's table; a liquid whose Surface bends light and
    /// that blocks light bends lamp light as a glass shape does (a bender round its Area).
    fn light(&self, cx: &DrawCx, scene: &mut bpe_gfx::LightScene) {
        let card = cx.resolved.slots.get(&SlotId::Look).filter(|c| c.kind == LOOK);
        let look = LookSettings::read(card);
        if look.lit <= 0.0 {
            return;
        }
        let Some(index) = surface_index(cx.scene, cx.thing.id) else { return };
        let s = surface_of(card);
        scene.set_surface(index, s.gpu(bpe_gfx::SurfaceGpu::SHEET));
        scene.refracts |= s.see_through();
        // It never shadows its own body (liquid fixes part B).
        if look.blocks_light {
            scene.liquid_surfaces.push(index);
        }
        if look.blocks_light && surface::bends(&s) {
            let Some(host) = host_of(cx.world).and_then(|h| h.host.as_ref()) else { return };
            let l = &host.layout;
            let far = l.origin + glam::Vec2::new(l.cols as f32, l.rows as f32) * l.cell;
            scene.benders.extend(bpe_gfx::Bender::round([l.origin, far], 2.0, index));
        }
    }
}
