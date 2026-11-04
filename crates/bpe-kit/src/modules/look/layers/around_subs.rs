//! What a trail's or particles layer's sub-layers do to what it draws: Recolour, Flash on hit
//! and Colour on hit change its colours; a Shadow and a Glow draw more of it under its own, an
//! Outline more over it. The pieces land in the frame in that order.

use bpe_core::Gradient;
use bpe_gfx::{Frame, Particle, TrailVertex};
use glam::Vec2;

use super::OutlineSits;
use super::gpu::SubLayer;
use super::subs::{SubColourFrom, SubGlow, SubOutline, SubShadow};
use crate::paint::recolour::{flash, hue_on_hit, recolour};

/// What an around layer draws, in the order it lands in the frame: shadows, glows, its own,
/// then outlines.
#[derive(Default)]
pub struct Pieces {
    pub strips: [Vec<TrailVertex>; 4],
    pub particles: [Vec<Particle>; 4],
}

pub const SHADOW: usize = 0;
pub const GLOW: usize = 1;
pub const OWN: usize = 2;
pub const OUTLINE: usize = 3;

impl Pieces {
    /// Puts them into the frame's lists behind the balls, or in front of them.
    pub fn into_frame(self, frame: &mut Frame, front: bool) {
        let (strips, particles) = if front {
            (&mut frame.trails_front, &mut frame.particles_front)
        } else {
            (&mut frame.trails, &mut frame.particles)
        };
        for s in self.strips {
            strips.extend(s);
        }
        for p in self.particles {
            particles.extend(p);
        }
    }

    /// How many particles in all.
    pub fn particle_count(&self) -> usize {
        self.particles.iter().map(Vec::len).sum()
    }
}

/// The hits a colour sub-layer reads: the last one (seconds ago, strength) and how many so far.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitInfo {
    pub last: Option<(f32, f32)>,
    pub hits: u32,
}

/// `c` through the colour sub-layers of `subs` (Recolour, Flash on hit, Colour on hit) before
/// the `upto`th.
pub fn coloured(subs: &[SubLayer], upto: usize, c: [f32; 4], hit: HitInfo) -> [f32; 4] {
    let mut rgb = [c[0], c[1], c[2]];
    for sub in &subs[..upto.min(subs.len())] {
        rgb = match sub {
            SubLayer::Recolour(r) => recolour(r, rgb),
            SubLayer::Flash(f) => match hit.last {
                Some((age, strength)) => flash(f, rgb, age, strength),
                None => rgb,
            },
            SubLayer::Hue(h) => hue_on_hit(h, rgb, hit.hits),
            _ => rgb,
        };
    }
    [rgb[0], rgb[1], rgb[2], c[3]]
}

/// A sub-layer's own colour at `s` along, or `same` when it takes the layer's.
pub fn sub_colour(from: SubColourFrom, own: &Gradient, s: f32, same: [f32; 4]) -> [f32; 4] {
    match from {
        SubColourFrom::SameAsLayer => same,
        SubColourFrom::Own => own.sample(s),
    }
}

/// How far an outline reaches from the middle of something `half` wide, as it sits, and the
/// share of that its band covers.
pub fn outline_reach(o: &SubOutline, half: f32) -> (f32, f32) {
    let w = o.width.max(0.0);
    let reach = match o.sits {
        OutlineSits::Outside => half + w,
        OutlineSits::Centre => half + w * 0.5,
        OutlineSits::Inside => half,
    };
    (reach, if reach > 0.0 { (w / reach).clamp(0.0, 1.0) } else { 0.0 })
}

/// Where a shadow falls from what casts it, `unit` world units to its Distance's one.
pub fn shadow_offset(s: &SubShadow, unit: f32) -> Vec2 {
    Vec2::from_angle(s.angle.to_radians()) * s.distance * unit
}

/// A colour darkened for an outline that takes its layer's colour, so it shows.
pub fn darker(c: [f32; 4]) -> [f32; 4] {
    [c[0] * 0.7, c[1] * 0.7, c[2] * 0.7, c[3]]
}

/// The light a glow adds, from `c` (linear light, straight), `fade` of it.
pub fn glow_light(g: &SubGlow, c: [f32; 4], fade: f32) -> [f32; 4] {
    let k = g.strength.max(0.0) * g.opacity.clamp(0.0, 1.0) * fade;
    [c[0] * k, c[1] * k, c[2] * k, -1.0]
}

/// `c` drawn the way the layer draws: added when the layer adds light (alpha -1 then), else
/// covering at `alpha`.
pub fn like_layer(adds: bool, c: [f32; 4], alpha: f32) -> [f32; 4] {
    if adds { [c[0] * alpha, c[1] * alpha, c[2] * alpha, -1.0] } else { [c[0], c[1], c[2], alpha] }
}

/// What a particle's sub-layers add round it.
pub struct Extras<'a> {
    pub subs: &'a [SubLayer],
    /// Its colour before the sub-layers, linear light.
    pub base: [f32; 4],
    /// How far along its trail or its life it is, 0 to 1, for a sub-layer's own colours.
    pub s: f32,
    /// How faded it is, 0 to 1.
    pub fade: f32,
    /// World units to a sub-layer's size unit (a ball's radius; 1 on a shape).
    pub unit: f32,
    pub hit: HitInfo,
    /// The opacity an outline is drawn with.
    pub outline: f32,
    /// How much of an added colour's light the sprites add (they add it twice over).
    pub added: f32,
}

/// The light colour `c` (alpha -1) as a particle's, `added` of it.
fn as_added(c: [f32; 4], added: f32) -> [f32; 4] {
    if c[3] < 0.0 { [c[0], c[1], c[2], -added] } else { c }
}

/// Puts `own` (already coloured and shaded) into `out` with a shadow and a glow under it and an
/// outline over it, as `e`'s sub-layers say.
pub fn particle_with_extras(own: Particle, e: &Extras, out: &mut Pieces) {
    let adds = own.color[3] < 0.0;
    let (size, soft) = (own.place[3], own.style[2]);
    for (k, sub) in e.subs.iter().enumerate() {
        match sub {
            SubLayer::Shadow(sh) if sh.opacity > 0.0 => {
                let c = sh.colour.to_linear();
                let colour = [c[0], c[1], c[2], sh.opacity.clamp(0.0, 1.0) * e.fade];
                let mut q = Particle { color: colour, ..own };
                q.place[0] += shadow_offset(sh, e.unit).x;
                q.place[1] += shadow_offset(sh, e.unit).y;
                q.style[2] = soft.max((sh.softness.max(0.0) * e.unit / size.max(1e-3)).min(1.0));
                out.particles[SHADOW].push(q);
            }
            SubLayer::Glow(g) if g.strength > 0.0 && g.opacity > 0.0 => {
                let same = coloured(e.subs, k, e.base, e.hit);
                let light = glow_light(g, sub_colour(g.colour_from, &g.colour, e.s, same), e.fade);
                let grow = g.size.max(0.0) * e.unit;
                let mut q = Particle { color: as_added(light, e.added), ..own };
                q.place[3] = size + grow;
                q.style[0] += grow;
                q.style[2] = 1.0;
                out.particles[GLOW].push(q);
            }
            SubLayer::Outline(o) if o.width > 0.0 && o.opacity > 0.0 => {
                let same = coloured(e.subs, k, e.base, e.hit);
                let (reach, band) = outline_reach(o, size);
                let c = sub_colour(o.colour_from, &o.colour, e.s, darker(same));
                let alpha = (o.opacity.clamp(0.0, 1.0) * e.fade * e.outline).min(1.0);
                let mut q = Particle { color: as_added(like_layer(adds, c, alpha), e.added), ..own };
                q.style[0] += reach - size;
                q.place[3] = reach;
                q.style[3] = band;
                q.style[2] = 0.0;
                out.particles[OUTLINE].push(q);
            }
            _ => {}
        }
    }
    out.particles[OWN].push(own);
}
