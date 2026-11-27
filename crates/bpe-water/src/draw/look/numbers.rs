//! The look drawing's numbers on the card (`LU`, `LB` in `shaders/look.wgsl`), from a [`Look`].

use bpe_core::maths::Maths;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use super::inputs::{BackLook, ColourFrom, GRADIENT_SAMPLES, GlowFrom, LampKind, Look, MOST_LAMPS, MaterialKind};
use super::inputs::{LiquidLook, MaterialLook};
use crate::draw::plain::{LEAST_R, LEVEL, LONE_PEAK, MOST_COLOURED, RADIUS, linear3};
use crate::drops::numbers::MOST_LIQUIDS;
use crate::solid::Layout;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct LiquidU {
    colour: [f32; 4],
    deep: [f32; 4],
    glow: [f32; 4],
    edge: [f32; 4],
    surface: [f32; 4],
    body: [f32; 4],
    shape: [f32; 4],
    more: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct MaterialU {
    colour: [f32; 4],
    second: [f32; 4],
    shape: [f32; 4],
    edge: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct LampU {
    place: [f32; 4],
    colour: [f32; 4],
    aim: [f32; 4],
}

/// Matches `LU` in look.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct LookU {
    pub(crate) light: [f32; 4],
    pub(crate) counts: [u32; 4],
    pub(crate) back: [f32; 4],
    pub(crate) back_way: [f32; 4],
    pub(crate) course: [f32; 4],
    pub(crate) picture: [f32; 4],
    pub(crate) shot: [f32; 4],
    pub(crate) spread: [f32; 4],
    pub(crate) time: [f32; 4],
    liquids: [LiquidU; MOST_LIQUIDS],
    lamps: [LampU; MOST_LAMPS],
    materials: [MaterialU; MOST_COLOURED + 1],
}

/// Matches `LB` in look.wgsl: a light map's box (or a spread level's numbers).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct BoxU {
    pub(crate) lo: [f32; 4],
    pub(crate) size: [u32; 4],
}

/// A grid of square texels fixed to the course: its top left (world), texel (world px) and size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Grid {
    pub(crate) lo: Vec2,
    pub(crate) texel: f32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The course's world box: its first node to its last.
pub(crate) fn course_box(layout: &Layout) -> (Vec2, Vec2) {
    let lo = layout.origin;
    let hi = lo + Vec2::new((layout.cols.max(1) - 1) as f32, (layout.rows.max(1) - 1) as f32) * layout.cell;
    (lo, hi)
}

/// The texels of `texel` world px (doubled until within `most` a side) laid from the course's top
/// left that cover `lo` to `hi` (clipped to the course) and one more each side; `None` when that is
/// empty.
pub(crate) fn grid_over(layout: &Layout, lo: Vec2, hi: Vec2, texel: f32, most: u32) -> Option<Grid> {
    let (c_lo, c_hi) = course_box(layout);
    let (lo, hi) = (lo.max(c_lo), hi.min(c_hi));
    if !(hi.x > lo.x && hi.y > lo.y) {
        return None;
    }
    let mut texel = texel.max(0.25);
    loop {
        let i0 = ((lo - c_lo) / texel).floor() - 1.0;
        let i1 = ((hi - c_lo) / texel).ceil() + 1.0;
        let (w, h) = ((i1.x - i0.x).max(1.0) as u32, (i1.y - i0.y).max(1.0) as u32);
        if w <= most && h <= most {
            return Some(Grid { lo: c_lo + i0 * texel, texel, width: w, height: h });
        }
        texel *= 2.0;
    }
}

/// The whole course in texels of `texel` world px (doubled until within `most` a side), from its top
/// left.
pub(crate) fn whole_course(layout: &Layout, texel: f32, most: u32) -> Grid {
    let (lo, hi) = course_box(layout);
    let mut texel = texel.max(0.25);
    loop {
        let size = ((hi - lo) / texel).ceil().max(Vec2::ONE);
        let (w, h) = (size.x as u32, size.y as u32);
        if w <= most && h <= most {
            return Grid { lo, texel, width: w, height: h };
        }
        texel *= 2.0;
    }
}

/// A liquid's splat at its Smoothing: radius (cells) and, at `scale` picture px a world px on cells of
/// `cell` px, its radius in picture px and peak weight (as the plain drawing's `splat_for`: a body at
/// rest sums to 1, a widened splat is fainter but never too faint to show).
pub(crate) fn splat_of(smoothing: f32, cell: f32, scale: f32) -> (f32, f32, f32) {
    let radius = RADIUS * (0.6 + 0.8 * smoothing.clamp(0.0, 1.0));
    let body_peak = 3.0 / (4.0 * std::f32::consts::PI * radius * radius);
    let own = radius * cell * scale;
    let r = own.max(LEAST_R);
    let peak = if r > own { (body_peak * (own / r) * (own / r)).max(LONE_PEAK * LEVEL) } else { body_peak };
    (radius, r, peak)
}

fn c4(c: [f32; 3], w: f32) -> [f32; 4] {
    [c[0], c[1], c[2], w]
}

fn liquid_u(l: &LiquidLook, cell: f32, scale: f32) -> LiquidU {
    let (radius, r, peak) = splat_of(l.smoothing, cell, scale);
    let glow = linear3(l.glow_colour).map(|c| c * l.glow.max(0.0));
    let colour = linear3(l.colour);
    let deep = linear3(l.deep);
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    LiquidU {
        colour: c4(colour, finite(l.clarity, 0.0).max(0.0)),
        deep: c4(deep, finite(l.solid, 0.0).clamp(0.0, 1.0)),
        glow: c4(glow, if l.glow_from == GlowFrom::Speed { 1.0 } else { 0.0 }),
        edge: c4(linear3(l.outline_colour), finite(l.outline, 0.0).max(0.0)),
        surface: [
            finite(l.shine, 0.0).clamp(0.0, 1.0),
            finite(l.shine_size, 0.3).clamp(0.0, 1.0),
            finite(l.edge_sheen, 0.0).clamp(0.0, 1.0),
            finite(l.thin_film, 0.0).clamp(0.0, 1.0),
        ],
        body: [
            finite(l.bend, 0.0).max(0.0),
            finite(l.split, 0.0).clamp(0.0, 1.0),
            finite(l.ripples, 0.0).max(0.0),
            match l.colour_from {
                ColourFrom::ItsOwn => 0.0,
                ColourFrom::Speed => 1.0,
                ColourFrom::Depth => 2.0,
            },
        ],
        shape: [r, peak, finite(l.streaks, 0.0).max(0.0), l.bands.min(16) as f32],
        more: [finite(l.band_softness, 0.2).clamp(0.0, 1.0), radius, 0.0, 0.0],
    }
}

fn material_u(m: &MaterialLook) -> MaterialU {
    let kind = match m.kind {
        MaterialKind::Rock => 0.0,
        MaterialKind::Glass => 1.0,
        MaterialKind::Paper => 2.0,
        MaterialKind::Basalt => 3.0,
        MaterialKind::Flat => 4.0,
    };
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    MaterialU {
        colour: c4(linear3(m.colour), kind),
        second: c4(linear3(m.second), finite(m.texture, 120.0).max(1.0)),
        shape: [
            finite(m.depth, 0.0).max(0.0),
            finite(m.roughness, 0.6).clamp(0.0, 1.0),
            finite(m.outline, 0.0).max(0.0),
            finite(m.wet_marks, 0.0).clamp(0.0, 1.0),
        ],
        edge: c4(linear3(m.outline_colour), 0.0),
    }
}

/// The liquids a look draws with (none given: the Look card's default).
pub(crate) fn liquids_of(look: &Look) -> Vec<LiquidLook> {
    if look.liquids.is_empty() { vec![LiquidLook::default()] } else { look.liquids.clone() }
}

/// The gradients' rows: each liquid's (its Colour where it has none), then the back's.
pub(crate) fn gradients(look: &Look) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0f32; 4]; (MOST_LIQUIDS + 1) * GRADIENT_SAMPLES];
    let mut row = |r: usize, samples: &[[f32; 3]], or: [f32; 3]| {
        for s in 0..GRADIENT_SAMPLES {
            // (Resampled between the given samples.)
            let c = match samples.len() {
                0 => or,
                1 => samples[0],
                n => {
                    let x = s as f32 / (GRADIENT_SAMPLES - 1) as f32 * (n - 1) as f32;
                    let i = (x.floor() as usize).min(n - 2);
                    let f = x - i as f32;
                    [0, 1, 2].map(|k| samples[i][k] + (samples[i + 1][k] - samples[i][k]) * f)
                }
            };
            out[r * GRADIENT_SAMPLES + s] = c4(c, 1.0);
        }
    };
    for (n, l) in liquids_of(look).iter().take(MOST_LIQUIDS).enumerate() {
        row(n, &l.gradient, linear3(l.colour));
    }
    if let BackLook::Gradient { samples, .. } = &look.back {
        row(MOST_LIQUIDS, samples, [0.0; 3]);
    }
    out
}

/// What the uniform is told beyond the look: the picture's scale, the light map and spread grids,
/// the back picture's box.
pub(crate) struct Placing {
    pub(crate) scale: f32,
    pub(crate) shot: Option<Grid>,
    pub(crate) spread: Option<(Grid, u32)>,
    /// The back picture's box in the world (lo, size), when one is read.
    pub(crate) picture: Option<(Vec2, Vec2)>,
}

/// The paper's largest Depth (its cut-out shadow's offset), 0 with no paper.
fn paper_depth(look: &Look) -> f32 {
    std::iter::once(&look.rock)
        .chain(look.solids.iter().take(MOST_COLOURED))
        .filter(|m| m.kind == MaterialKind::Paper)
        .map(|m| if m.depth.is_finite() { m.depth.max(0.0) } else { 0.0 })
        .fold(0.0, f32::max)
}

pub(crate) fn look_u(look: &Look, layout: &Layout, placing: &Placing) -> LookU {
    let mut u = LookU::zeroed();
    let liquids = liquids_of(look);
    for (n, l) in liquids.iter().take(MOST_LIQUIDS).enumerate() {
        u.liquids[n] = liquid_u(l, layout.cell, placing.scale);
    }
    let rock = material_u(&look.rock);
    for m in u.materials.iter_mut() {
        *m = rock;
    }
    for (k, m) in look.solids.iter().take(MOST_COLOURED).enumerate() {
        u.materials[k + 1] = material_u(m);
    }
    let lamps: Vec<_> = look.lamps.iter().filter(|l| l.brightness > 0.0 && l.brightness.is_finite()).collect();
    for (n, lamp) in lamps.iter().take(MOST_LAMPS).enumerate() {
        let (s, c) = lamp.aim.to_radians().det_sin_cos();
        let kind = match lamp.kind {
            LampKind::Spot => 0.0,
            LampKind::Strip => 1.0,
            LampKind::Sun => 2.0,
        };
        let half = (lamp.spread.clamp(1.0, 360.0) * 0.5).to_radians();
        u.lamps[n] = LampU {
            place: [lamp.place.x, lamp.place.y, kind, if lamp.shadows { 1.0 } else { 0.0 }],
            colour: c4(linear3(lamp.colour).map(|v| v * lamp.brightness), lamp.softness.clamp(0.0, 1.0)),
            aim: [c, s, half, lamp.length.max(1.0)],
        };
    }
    let light = &look.light;
    let finite = |v: f32, or: f32| if v.is_finite() { v.max(0.0) } else { or };
    u.light =
        [finite(light.ambient, 1.0), finite(light.bounce, 0.0), finite(light.caustics, 0.0), finite(light.shafts, 0.0)];
    let (c_lo, c_hi) = course_box(layout);
    u.course = [c_lo.x, c_lo.y, c_hi.x, c_hi.y];
    let back_kind = match &look.back {
        BackLook::DarkerRock(darker) => {
            u.back = [0.0, 0.0, 0.0, darker.clamp(0.0, 1.0)];
            0
        }
        BackLook::Colour(c) => {
            u.back = c4(linear3(*c), 0.0);
            1
        }
        BackLook::Gradient { angle, .. } => {
            let (s, c) = angle.to_radians().det_sin_cos();
            let size = c_hi - c_lo;
            let length = (size.x * c).abs() + (size.y * s).abs();
            u.back_way = [c, s, length.max(1.0), 0.0];
            2
        }
        BackLook::Picture(_) => {
            if placing.picture.is_some() {
                3
            } else {
                u.back = [0.0, 0.0, 0.0, 0.5];
                0
            }
        }
    };
    if let Some((lo, size)) = placing.picture {
        u.picture = [lo.x, lo.y, size.x, size.y];
    }
    u.counts = [liquids.len().min(MOST_LIQUIDS) as u32, lamps.len().min(MOST_LAMPS) as u32, back_kind, 0];
    if let Some(g) = placing.shot {
        u.shot = [g.lo.x, g.lo.y, g.texel, 1.0];
    }
    if let Some((g, levels)) = placing.spread {
        u.spread = [g.lo.x, g.lo.y, g.texel, levels as f32];
        u.counts[3] = 1;
    }
    let ripples =
        liquids.iter().map(|l| if l.ripples.is_finite() { l.ripples.max(0.0) } else { 0.0 }).fold(0.0, f32::max);
    u.time = [if look.time.is_finite() { look.time } else { 0.0 }, ripples, paper_depth(look), 0.0];
    u
}

/// Whether anything gives off light or bounces it (the spread light is drawn).
pub(crate) fn spreads(look: &Look) -> bool {
    let glows = liquids_of(look).iter().any(|l| l.glow > 0.0 && l.glow.is_finite());
    let bounces = look.light.bounce > 0.0 && look.lamps.iter().any(|l| l.brightness > 0.0);
    glows || bounces
}

/// Whether any lamp gives light.
pub(crate) fn lit(look: &Look) -> bool {
    look.lamps.iter().any(|l| l.brightness > 0.0 && l.brightness.is_finite())
}
