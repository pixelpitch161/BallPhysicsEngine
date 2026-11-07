//! The **Trails** Look layer (particles part 6): each particle's recent path drawn as a line behind
//! it, thinning and fading to its tail. A draw is worked out from the state alone, so the state
//! keeps each particle's last places in a ring ([`TrailRing`]), only while a Trails layer asks.

use bpe_core::{Blend, Color, Settings};
use bpe_gfx::{StreakStyle, push_trail};
use glam::Vec2;

use super::look::{ColourFrom, Common, FineDetail, MOST_COPIES, Only, look_settings};
use super::state::Speck;
use crate::modules::look::layers::around_subs::{
    GLOW, OUTLINE, Pieces, SHADOW, coloured, darker, glow_light, like_layer, outline_reach, shadow_offset, sub_colour,
};
use crate::modules::look::layers::gpu::SubLayer;
use crate::modules::look::layers::particles::{ParticleFade, SizeOverLife};
use crate::modules::look::layers::trail::AroundPlace;
use crate::paint::{Follow, Paint};

/// The layer kind.
pub const TRAILS: &str = "particles.trails";
/// The most places a particle's ring keeps.
pub const MOST_TRAIL_POINTS: usize = 256;
/// The most places the ring keeps over all particles (32 MB): a big crowd keeps fewer a particle,
/// so its trails are shorter, rather than a state too big to copy for the playback's snapshots.
pub const MOST_RING_PLACES: usize = 4_000_000;
/// The most points one trail is drawn through.
pub const MOST_LENGTH: u32 = 64;
/// The most particle steps between two points.
pub const MOST_EVERY: u32 = 4;
/// The anti-aliasing margin each strip edge keeps, in world units.
const AA: f32 = 1.0;

look_settings! {
    /// Trails: each particle's recent path drawn as a line.
    TrailsLayer {
        /// How many points each trail runs through.
        #[setting(label = "Length", group = "Trails", range(2, 64), unit = "points", simple)]
        length: u32 = 16,
        /// How many particle steps (1/120 s each) apart the points are: further apart, longer trails.
        #[setting(label = "Every", group = "Trails", range(1, 4), unit = "steps")]
        every: u32 = 2,
        /// How wide a trail is at its head, as a share of the particle's size.
        #[setting(label = "Width", group = "Trails", span(0, 4), simple, keyframe, pulse)]
        width: f32 = 1.0,
        /// How much thinner it gets towards its tail.
        #[setting(label = "Taper", group = "Trails", range(0, 1), keyframe)]
        taper: f32 = 0.7,
        /// How much it fades towards its tail.
        #[setting(label = "Fade along", group = "Trails", range(0, 1), keyframe)]
        fade_along: f32 = 1.0,
        /// How soft its edges are.
        #[setting(label = "Softness", group = "Trails", range(0, 1), keyframe)]
        softness: f32 = 0.3,
    }
}

impl TrailsLayer {
    /// The places it reads back: Length points, Every steps apart.
    pub fn reach(&self) -> usize {
        let length = self.length.clamp(2, MOST_LENGTH) as usize;
        let every = self.every.clamp(1, MOST_EVERY) as usize;
        ((length - 1) * every + 1).min(MOST_TRAIL_POINTS)
    }
}

/// Every particle's last `points` places, one a particle step: particle *i*'s at
/// `places[i × points ..]`, the place at particle step *k* in slot `k % points`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrailRing {
    pub points: usize,
    pub places: Vec<Vec2>,
}

impl TrailRing {
    /// Bytes it takes.
    pub fn memory(&self) -> usize {
        self.places.capacity() * std::mem::size_of::<Vec2>()
    }

    /// Keeps `points` places a particle from now on; a change starts every ring again from where
    /// each particle is.
    pub fn set_points(&mut self, points: usize, specks: &[Speck]) {
        let points = points.min(MOST_TRAIL_POINTS);
        if points == self.points && self.places.len() == specks.len() * points {
            return;
        }
        self.points = points;
        self.places.clear();
        if points > 0 {
            self.places.extend(specks.iter().flat_map(|s| std::iter::repeat_n(s.place, points)));
        }
    }

    /// Gives the particles born since the last step their rows, filled with their places.
    pub fn grow(&mut self, specks: &[Speck]) {
        if self.points == 0 {
            return;
        }
        let rows = self.places.len() / self.points;
        for s in specks.iter().skip(rows) {
            self.places.extend(std::iter::repeat_n(s.place, self.points));
        }
    }

    /// Notes every particle's place at particle step `step`.
    pub fn write(&mut self, specks: &[Speck], step: u32) {
        if self.points == 0 {
            return;
        }
        let slot = step as usize % self.points;
        for (i, s) in specks.iter().enumerate() {
            if let Some(p) = self.places.get_mut(i * self.points + slot) {
                *p = s.place;
            }
        }
    }

    /// Keeps the rows of the particles `keep` says (in order), dropping the rest.
    pub fn keep(&mut self, keep: &[bool]) {
        if self.points == 0 {
            return;
        }
        let n = self.points;
        let mut to = 0;
        for (i, &k) in keep.iter().enumerate() {
            if !k {
                continue;
            }
            if to != i {
                self.places.copy_within(i * n..(i + 1) * n, to * n);
            }
            to += 1;
        }
        self.places.truncate(to * n);
    }

    /// Particle `i`'s places, newest first, `every` steps apart, at most `length`, none before
    /// it was born: at particle step `now`.
    pub fn recent(&self, i: usize, s: &Speck, now: u32, every: u32, length: u32) -> Vec<Vec2> {
        let n = self.points;
        if n == 0 || (i + 1) * n > self.places.len() {
            return Vec::new();
        }
        let every = every.clamp(1, MOST_EVERY);
        let lived = now.saturating_sub(s.born) as usize;
        let row = &self.places[i * n..(i + 1) * n];
        let mut out = Vec::with_capacity(length as usize);
        for k in 0..length.clamp(2, MOST_LENGTH) as usize {
            let back = k * every as usize;
            if back > lived || back >= n {
                break;
            }
            out.push(row[(now as usize + n - back % n) % n]);
        }
        out
    }
}

/// Draws particle `i`'s trail into `pieces`: `base` its colour before the sub-layers, `alpha`
/// how much shows, `size` its half size. `hit` feeds the colour sub-layers; `jump` is the
/// longest segment drawn (longer ones are a wrap or a leap: the trail stops there).
#[allow(clippy::too_many_arguments)]
pub fn draw_one(
    t: &TrailsLayer,
    c: &Common,
    subs: &[SubLayer],
    points: &[Vec2],
    base: [f32; 4],
    alpha: f32,
    size: f32,
    hit: crate::modules::look::layers::around_subs::HitInfo,
    jump: f32,
    pieces: &mut Pieces,
) {
    let mut n = points.len();
    if let Some(cut) = points.windows(2).position(|w| w[0].distance(w[1]) > jump) {
        n = cut + 1;
    }
    if n < 2 {
        return;
    }
    let points = &points[..n];
    let along = |k: usize| k as f32 / (n - 1) as f32;
    let head = (size * t.width).max(0.0);
    if head <= 0.0 {
        return;
    }
    let widths: Vec<f32> = (0..n).map(|k| head * (1.0 - t.taper.clamp(0.0, 1.0) * along(k))).collect();
    let fade: Vec<f32> =
        (0..n).map(|k| alpha * (1.0 - t.fade_along.clamp(0.0, 1.0) * along(k) * along(k)).max(0.0)).collect();
    let own = coloured(subs, subs.len(), base, hit);
    let shaded: Vec<[f32; 4]> = fade.iter().map(|f| strip_colour(super::draw::shade(c, own, *f))).collect();
    let adds = shaded.first().is_some_and(|c| c[3] < 0.0);
    let style = StreakStyle { softness: t.softness.clamp(0.0, 1.0), band: 0.0 };
    for (at, sub) in subs.iter().enumerate() {
        match sub {
            SubLayer::Shadow(s) if s.opacity > 0.0 => {
                let off = shadow_offset(s, 1.0);
                let moved: Vec<Vec2> = points.iter().map(|p| *p + off).collect();
                let k = s.colour.to_linear();
                let colours: Vec<[f32; 4]> =
                    fade.iter().map(|f| [k[0], k[1], k[2], s.opacity.clamp(0.0, 1.0) * f]).collect();
                let softness = (s.softness.max(0.0) / head.max(1e-3)).min(1.0).max(style.softness);
                let st = StreakStyle { softness, band: 0.0 };
                push_trail(&mut pieces.strips[SHADOW], &moved, &widths, &colours, st, AA);
            }
            SubLayer::Glow(g) if g.strength > 0.0 && g.opacity > 0.0 => {
                let same = coloured(subs, at, base, hit);
                let wide: Vec<f32> = widths.iter().map(|w| w + g.size.max(0.0)).collect();
                let light: Vec<[f32; 4]> = (0..n)
                    .map(|k| glow_light(g, sub_colour(g.colour_from, &g.colour, along(k), same), fade[k]))
                    .collect();
                let st = StreakStyle { softness: 1.0, band: 0.0 };
                push_trail(&mut pieces.strips[GLOW], points, &wide, &light, st, AA);
            }
            SubLayer::Outline(o) if o.width > 0.0 && o.opacity > 0.0 => {
                let same = coloured(subs, at, base, hit);
                let (_, band) = outline_reach(o, head);
                let wide: Vec<f32> = widths.iter().map(|w| outline_reach(o, *w).0).collect();
                let cs: Vec<[f32; 4]> = (0..n)
                    .map(|k| {
                        let c = sub_colour(o.colour_from, &o.colour, along(k), darker(same));
                        strip_colour(like_layer(adds, c, (o.opacity.clamp(0.0, 1.0) * fade[k]).min(1.0)))
                    })
                    .collect();
                let st = StreakStyle { softness: 0.0, band };
                push_trail(&mut pieces.strips[OUTLINE], points, &wide, &cs, st, AA);
            }
            _ => {}
        }
    }
    let own_list = crate::modules::look::layers::around_subs::OWN;
    push_trail(&mut pieces.strips[own_list], points, &widths, &shaded, style, AA);
}

/// A sprite's colour as a strip corner's: added light is the light itself faded in, alpha -1.
fn strip_colour(c: [f32; 4]) -> [f32; 4] {
    if c[3] < 0.0 { [c[0] * -c[3], c[1] * -c[3], c[2] * -c[3], -1.0] } else { c }
}

impl TrailsLayer {
    fn starting(s: Self) -> Self {
        s
    }
}
