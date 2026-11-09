//! What the glass passes read: dichroic surfaces, caustics' glass balls, the rainbow and the
//! spheres' grid (Stage).

use bytemuck::{Pod, Zeroable};

use super::*;

/// The first [`MOST_DICHROIC`] dichroic surfaces of `surfaces` as `[index, Dichroic, shift, hue]`,
/// and how many.
pub fn dichroic_list(surfaces: &[SurfaceGpu]) -> ([[f32; 4]; MOST_DICHROIC], usize) {
    let mut out = [[0.0; 4]; MOST_DICHROIC];
    let mut n = 0;
    for (i, s) in surfaces.iter().enumerate() {
        if n == MOST_DICHROIC {
            break;
        }
        // Only see-through ones colour a shadow (bending glass is solid to the march: its
        // photons carry the colour).
        let bends = s.e[3] > crate::BENDS_FROM && s.e[3].is_finite();
        if s.g[0] > 0.0 && s.a[3] > 0.0 && !bends && s.g.iter().take(3).all(|v| v.is_finite()) {
            out[n] = [i as f32, s.g[0].min(1.0), s.g[1], s.g[2]];
            n += 1;
        }
    }
    (out, n)
}

/// What the surfaces and the stage read (see `Stage` in the lighting shader, part 2a).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(super) struct StageUniform {
    pub(super) room: [f32; 4],
    pub(super) floor: [f32; 4],
    pub(super) more: [f32; 4],
    pub(super) sky_top: [f32; 4],
    pub(super) sky_bottom: [f32; 4],
    pub(super) grid: [f32; 4],
    pub(super) grid_size: [f32; 4],
    /// Part 2b: depth of field (on, focus, blur, Reflection depth).
    pub(super) dof: [f32; 4],
    /// How many mirror walls, 1 with a behind picture, 1 with the background's own light, 1 in
    /// Stage with a light field.
    pub(super) counts: [f32; 4],
}

/// What the caustics read (see `CausticU` in the lighting shader, part 3).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(super) struct CausticUniform {
    pub(super) counts: [f32; 4],
    pub(super) more: [f32; 4],
}

/// A glass ball the caustics trace light through (see `CausticGlass` in the lighting shader).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CausticGlass {
    /// Centre xy, radius, see-through (a mirror: its Facet gaps).
    pub a: [f32; 4],
    /// The colour it lets through (normalised, linear), bends light (a mirror: what it gives back,
    /// its turn).
    pub b: [f32; 4],
    /// Rainbow split, frost, 1 for a mirror (glass and light part 4a), a mirror's Facets.
    pub c: [f32; 4],
}

/// The balls light goes through to make caustics: see-through (at least 0.05) and bending light,
/// with Glass caustics on; and on a Stage with Mirror light on and shadows not Off, the balls that
/// throw light (glass and light part 4a; only lights that cast shadows throw, as in the 2D pass).
pub fn caustic_glass(scene: &LightScene) -> Vec<CausticGlass> {
    let s = &scene.settings;
    let glass_on = s.bake_more[2] != 0.0;
    // Glass and light part 4a: on a Stage, mirror balls throw their light here too.
    let mirrors_on = s.stage[0] > 0.5 && s.bent[2] != 0.0 && s.bent[2].is_finite() && s.shadows[0] < 2.5;
    scene
        .spheres
        .iter()
        .filter_map(|sp| {
            let see = sp.a[3];
            let index = sp.c[0].max(0.0) as usize;
            let sf = scene.surfaces.get(index).copied().unwrap_or(SurfaceGpu::MATTE);
            let ior = sf.e[3];
            let finite = sp.a.iter().take(3).all(|v| v.is_finite());
            if !(finite && sp.a[2] > 0.0) {
                return None;
            }
            if glass_on && see >= 0.05 && ior > 1.0001 {
                let top = sp.b[0].max(sp.b[1]).max(sp.b[2]).max(1e-3);
                let through = crate::dichroic::Dichroic::of(sf.g).through(crate::dichroic::SPHERE_COS);
                let tint = [0, 1, 2].map(|k| (sp.b[k] / top).clamp(0.0, 1.0) * through[k]);
                return Some(CausticGlass {
                    a: [sp.a[0], sp.a[1], sp.a[2], see.min(1.0)],
                    b: [tint[0], tint[1], tint[2], ior],
                    c: [sf.c[0].max(0.0), sf.e[0].max(0.0), 0.0, 0.0],
                });
            }
            let t = scene.throwers.iter().find(|t| t.ball && t.index as usize == index).filter(|_| mirrors_on)?;
            let turn = if sp.c[2].is_finite() { sp.c[2] } else { 0.0 };
            let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
            let back = crate::dichroic::Dichroic::of(sf.g).back(crate::dichroic::SPHERE_COS);
            Some(CausticGlass {
                a: [sp.a[0], sp.a[1], sp.a[2], finite(t.gaps).min(1.0)],
                b: [finite(t.colour[0]) * back[0], finite(t.colour[1]) * back[1], finite(t.colour[2]) * back[2], turn],
                c: [0.0, 0.0, 1.0, finite(t.facets)],
            })
        })
        .collect()
}

/// The Lighting card's rainbow numbers made safe to draw with (glass and light part 4b): none
/// when Rainbow isn't above 0 or isn't a number, the size at least 1.
pub(super) fn rainbow_of(r: &[f32; 4]) -> [f32; 4] {
    let ok = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    if !(r[0] > 0.0 && r[0].is_finite()) {
        return [0.0; 4];
    }
    [r[0], ok(r[1]).max(1.0), ok(r[2]), ok(r[3]).min(1.0)]
}

/// Where the rainbow's middle is (world): where the Lighting card puts it, or opposite the
/// brightest sun (1.3 bow sizes from `middle`, the camera's, along the way its light goes;
/// straight down with no sun).
pub fn rainbow_centre(scene: &LightScene, middle: [f32; 2]) -> [f32; 2] {
    let s = &scene.settings;
    if s.rainbow_at[2] > 0.5 && s.rainbow_at[0].is_finite() && s.rainbow_at[1].is_finite() {
        return [s.rainbow_at[0], s.rainbow_at[1]];
    }
    let size = if s.rainbow[1].is_finite() { s.rainbow[1].max(1.0) } else { 1.0 };
    let mut way = [0.0f32, 1.0];
    let mut best = 0.0f32;
    for l in &scene.lights {
        if l.p0[3] > 1.5 && l.p0[3] < 2.5 {
            let bright = l.p1[0].max(l.p1[1]).max(l.p1[2]);
            let len = l.p3[0].hypot(l.p3[1]);
            if bright > best && len > 1e-6 && len.is_finite() {
                best = bright;
                way = [l.p3[0] / len, l.p3[1] / len];
            }
        }
    }
    [middle[0] + way[0] * 1.3 * size, middle[1] + way[1] * 1.3 * size]
}

/// How many photons each (light, glass ball) pair sends at a Quality (4: the bake, a sample),
/// fewer when there are many pairs: at most 2^20 a frame live, 2^23 a sample baking, at least 64
/// a pair.
pub fn photons_per_pair(quality: f32, pairs: usize) -> u32 {
    let (mut per, most) = match quality.round() as i32 {
        i32::MIN..=0 => (1024u32, 1u64 << 20),
        1 => (4096, 1 << 20),
        2 => (16384, 1 << 20),
        3 => (32768, 1 << 20),
        _ => (65536, 1 << 23),
    };
    while per > 64 && per as u64 * pairs as u64 > most {
        per /= 2;
    }
    per
}

/// The most cells a sphere grid has along a side.
pub(super) const MOST_CELLS: f32 = 256.0;

/// A uniform grid over the stage's spheres, built on the CPU each frame: shadow rays and contact
/// shading look up the spheres near a point by its cell. Each sphere is listed in every cell its
/// disc reaches, widened by [`SPHERE_MARGIN`] of its own radii so penumbras and contact shading
/// near a ball find it; a shadow ray walks every cell it crosses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SphereGrid {
    /// The grid's corner (world), cell size, the largest radius.
    pub grid: [f32; 4],
    /// Cells across, cells down, [`SPHERE_MARGIN`], 0.
    pub size: [f32; 4],
    /// Each cell's first item and how many, row by row.
    pub cells: Vec<[u32; 2]>,
    /// The spheres' indices, cell by cell.
    pub items: Vec<u32>,
}

/// How far past its edge a ball's contact shading reaches, in its radii.
pub const SPHERE_MARGIN: f32 = 3.0;

impl SphereGrid {
    pub fn build(spheres: &[SphereGpu]) -> SphereGrid {
        let live: Vec<(usize, &SphereGpu)> = spheres
            .iter()
            .enumerate()
            .filter(|(_, s)| s.a.iter().take(3).all(|v| v.is_finite()) && s.a[2] > 0.0)
            .collect();
        if live.is_empty() {
            return SphereGrid {
                grid: [0.0, 0.0, 1.0, 0.0],
                size: [1.0, 1.0, 0.0, 0.0],
                cells: vec![[0, 0]],
                items: Vec::new(),
            };
        }
        let rmax = live.iter().map(|(_, s)| s.a[2]).fold(0.0f32, f32::max);
        // Each ball is listed out to its own radius plus SPHERE_MARGIN radii, where its contact
        // shading reaches; shadow rays walk every cell they cross, so they need no more.
        let reach_of = |s: &SphereGpu| s.a[2] * (1.0 + SPHERE_MARGIN);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for (_, s) in &live {
            let reach = reach_of(s);
            for k in 0..2 {
                lo[k] = lo[k].min(s.a[k] - reach);
                hi[k] = hi[k].max(s.a[k] + reach);
            }
        }
        let (w, h) = ((hi[0] - lo[0]).max(1e-3), (hi[1] - lo[1]).max(1e-3));
        let mean = live.iter().map(|(_, s)| s.a[2]).sum::<f32>() / live.len() as f32;
        let mut cell = (2.0 * mean).max((w * h / live.len() as f32).sqrt());
        cell = cell.max(w / MOST_CELLS).max(h / MOST_CELLS);
        let nx = ((w / cell).ceil() as usize).max(1);
        let ny = ((h / cell).ceil() as usize).max(1);
        let cell_of = |v: f32, k: usize, n: usize| (((v - lo[k]) / cell).floor().max(0.0) as usize).min(n - 1);
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
        for (i, s) in &live {
            let reach = reach_of(s);
            let (x0, x1) = (cell_of(s.a[0] - reach, 0, nx), cell_of(s.a[0] + reach, 0, nx));
            let (y0, y1) = (cell_of(s.a[1] - reach, 1, ny), cell_of(s.a[1] + reach, 1, ny));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    lists[y * nx + x].push(*i as u32);
                }
            }
        }
        let mut cells = Vec::with_capacity(lists.len());
        let mut items = Vec::new();
        for list in lists {
            cells.push([items.len() as u32, list.len() as u32]);
            items.extend(list);
        }
        SphereGrid { grid: [lo[0], lo[1], cell, rmax], size: [nx as f32, ny as f32, SPHERE_MARGIN, 0.0], cells, items }
    }
}
