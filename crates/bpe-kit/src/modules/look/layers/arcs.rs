//! Electric arcs (`layer.arcs`): lightning between a thing's balls that come near each other,
//! jagged and flickering, drawn as strips (so the around sub-layers work on it as on a trail) and,
//! in a lit scene, lighting what is round them. Nothing is kept between frames: the arcs are
//! worked out from where the balls are at the moment drawn, seeded by the balls' numbers and the
//! flicker's tick, so scrubbing and export draw the same.

use bpe_core::{Blend, Choice, Color, Settings};
use bpe_gfx::Frame;
use glam::Vec2;

use super::around::AroundLayer;
use super::around_subs::{HitInfo, Pieces};
use super::trail::draw::{StripLook, strip_budget, strip_with_subs};
use super::trail::kinds::Path;
use super::trail::{AroundPlace, TrailFade, TrailLayer, TrailSource};

/// The layer kind.
pub const ARCS: &str = "layer.arcs";

/// The most arcs that light the scene in one frame (the brightest), so a crowd of arcs doesn't
/// slow every lit pixel down.
pub const MOST_ARC_LIGHTS: usize = 16;
/// Segments of the line each arc's light runs along.
const LIGHT_SEGMENTS: usize = 8;

/// When arcs show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ArcFire {
    /// Whenever two balls are near.
    #[default]
    Always,
    /// For a moment after either ball hits something.
    WhenTheyHit,
    /// For a moment after each beat of the scene's tempo.
    OnTheBeat,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ArcsLayer {
    /// Under the balls or over them.
    #[setting(label = "Where", group = "Arcs", simple)]
    pub place: AroundPlace,
    /// How close two balls' edges must come for an arc to jump between them.
    #[setting(label = "Reach", group = "Arcs", range(0, 1e9), span(0, 600), unit = "px", simple, keyframe, pulse)]
    pub reach: f32,
    /// When arcs show.
    #[setting(label = "Fire", group = "Arcs", simple)]
    pub fire: ArcFire,
    /// How long an arc shows after a hit or a beat.
    #[setting(
        label = "Fire length",
        group = "Arcs",
        when = "fire!=always",
        range(0.001, 1e9),
        span(0.02, 1),
        unit = "s"
    )]
    pub fire_length: f32,
    /// The most arcs drawn at once, the nearest pairs first.
    #[setting(label = "Most arcs", group = "Arcs", range(0, 1e9), span(0, 500))]
    pub most: f32,
    /// How likely each arc is to show on each flicker.
    #[setting(label = "Chance", group = "Flicker", span(0, 1), keyframe, pulse)]
    pub chance: f32,
    /// How many times a second the arcs jump to a new shape (0 holds one shape).
    #[setting(label = "Flicker", group = "Flicker", span(0, 60), unit = "/s", keyframe)]
    pub flicker: f32,
    /// How jagged an arc is: how far it strays to the side, as a share of its length.
    #[setting(label = "Jagged", group = "Shape", span(0, 1), keyframe, pulse)]
    pub jagged: f32,
    /// How many points make each arc.
    #[setting(label = "Detail", group = "Shape", range(2, 1e9), span(2, 64))]
    pub detail: f32,
    /// How many arcs fork once.
    #[setting(label = "Branches", group = "Shape", span(0, 1))]
    pub branches: f32,
    /// How wide the bright core is.
    #[setting(label = "Width", group = "Look", range(0, 1e9), span(0.5, 20), unit = "px", simple, keyframe, pulse)]
    pub width: f32,
    /// How wide the soft glow round the core is, in core widths (0 for none).
    #[setting(label = "Glow", group = "Look", span(0, 20), unit = "×", keyframe, pulse)]
    pub glow: f32,
    /// The two balls' colours, or its own.
    #[setting(label = "Colour from", group = "Look")]
    pub source: TrailSource,
    #[setting(label = "Colour", group = "Look", when = "source=own", keyframe)]
    pub colour: Color,
    /// How bright the arcs are.
    #[setting(label = "Brightness", group = "Look", span(0, 4), keyframe, pulse)]
    pub brightness: f32,
    /// How it mixes with what is under it.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    #[setting(label = "Blend amount", span(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
    /// In a lit scene, how brightly the arcs light what is round them (0 for not at all).
    #[setting(label = "Lights the scene", group = "Light", span(0, 4), keyframe, pulse)]
    pub lights: f32,
    /// How far their light reaches.
    #[setting(label = "Light reach", group = "Light", when = "lights", range(0, 1e9), span(0, 1500), unit = "px")]
    pub light_reach: f32,
}

impl Default for ArcsLayer {
    fn default() -> Self {
        Self {
            place: AroundPlace::InFront,
            reach: 160.0,
            fire: ArcFire::Always,
            fire_length: 0.25,
            most: 64.0,
            chance: 0.8,
            flicker: 14.0,
            jagged: 0.18,
            detail: 16.0,
            branches: 0.3,
            width: 2.5,
            glow: 6.0,
            source: TrailSource::Own,
            colour: Color::rgb(0xb8, 0xd4, 0xff),
            brightness: 1.5,
            blend: Blend::Add,
            blend_amount: 1.0,
            lights: 0.0,
            light_reach: 350.0,
        }
    }
}

/// One ball as the arcs see it.
#[derive(Clone, Copy, Debug)]
pub struct ArcBall {
    pub at: Vec2,
    pub radius: f32,
    /// Its number in its group, which seeds its arcs' shapes.
    pub number: u32,
    /// Its colour as seen, linear light.
    pub colour: [f32; 4],
    /// Seconds since it last hit something.
    pub hit_age: f32,
}

/// One arc (or a branch of one): its points, edge to edge, how strong it shows (0 to 1) and its
/// colour before its strength.
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    pub points: Vec<Vec2>,
    pub strength: f32,
    pub colour: [f32; 4],
}

/// What the arcs read of the moment: the time, and the beat (its phase 0 to 1 and seconds a beat)
/// when the scene has a tempo.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArcMoment {
    pub time: f32,
    pub beat: Option<(f32, f32)>,
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// 0 to 1 from a seed.
fn unit(seed: u32) -> f32 {
    (hash(seed) >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise along one line, -1 to 1.
fn noise(seed: u32, x: f32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let k = i as i32 as u32;
    let a = unit(seed ^ hash(k)) * 2.0 - 1.0;
    let b = unit(seed ^ hash(k.wrapping_add(1))) * 2.0 - 1.0;
    let s = f * f * (3.0 - 2.0 * f);
    a + (b - a) * s
}

/// The jagged line from `from` to `to` through `n` points, seeded by `seed`, its ends pinned.
fn bolt(from: Vec2, to: Vec2, n: usize, jagged: f32, seed: u32) -> Vec<Vec2> {
    let n = n.max(2);
    let d = to - from;
    let len = d.length();
    let side = if len > 0.0 { d.perp() / len } else { Vec2::ZERO };
    (0..n)
        .map(|k| {
            let t = k as f32 / (n - 1) as f32;
            let wave = noise(seed, t * 4.0) * 0.66 + noise(seed ^ 0x9e37_79b9, t * 11.0) * 0.34;
            from + d * t + side * (wave * jagged * len * (std::f32::consts::PI * t).sin())
        })
        .collect()
}

/// The arcs `a` draws between `balls` at `moment`, nearest pairs first, at most Most arcs (with
/// their branches after).
pub fn arcs(a: &ArcsLayer, balls: &[ArcBall], moment: ArcMoment) -> Vec<Arc> {
    let reach = a.reach.max(0.0);
    let most = a.most.max(0.0) as usize;
    if balls.len() < 2 || most == 0 || !reach.is_finite() {
        return Vec::new();
    }
    let firing = |b: &ArcBall| -> f32 {
        let length = a.fire_length.max(1e-3);
        match a.fire {
            ArcFire::Always => 1.0,
            ArcFire::WhenTheyHit => (1.0 - b.hit_age / length).clamp(0.0, 1.0),
            ArcFire::OnTheBeat => match moment.beat {
                Some((phase, seconds)) => (1.0 - phase * seconds / length).clamp(0.0, 1.0),
                None => 0.0,
            },
        }
    };
    // Pairs within reach, found through a grid of cells as wide as the reach and the biggest ball.
    let widest = balls.iter().map(|b| b.radius.max(0.0)).fold(0.0, f32::max);
    let cell = (reach + 2.0 * widest).max(1.0);
    let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
    let key = |p: Vec2| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32);
    for (i, b) in balls.iter().enumerate() {
        if b.at.is_finite() {
            grid.entry(key(b.at)).or_default().push(i);
        }
    }
    let order =
        |x: &(f32, usize, usize), y: &(f32, usize, usize)| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2));
    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    // Only the nearest `most` are drawn: past a few times that many, the rest are let go.
    let keep = most.saturating_mul(4).max(64);
    for (i, b) in balls.iter().enumerate() {
        if !b.at.is_finite() {
            continue;
        }
        let (cx, cy) = key(b.at);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(near) = grid.get(&(cx + dx, cy + dy)) else { continue };
                for &j in near.iter().filter(|&&j| j > i) {
                    let o = &balls[j];
                    let gap = b.at.distance(o.at) - b.radius.max(0.0) - o.radius.max(0.0);
                    if gap <= reach && b.at.distance(o.at) > 1e-3 {
                        pairs.push((gap, i, j));
                        if pairs.len() > keep {
                            pairs.select_nth_unstable_by(most - 1, order);
                            pairs.truncate(most);
                        }
                    }
                }
            }
        }
    }
    pairs.sort_by(order);
    let tick = if a.flicker > 0.0 { (moment.time * a.flicker).floor() as i64 as u32 } else { 0 };
    let detail = a.detail.clamp(2.0, 4096.0) as usize;
    let own = a.colour.to_linear();
    let mut out = Vec::new();
    let mut branches = Vec::new();
    for (gap, i, j) in pairs.into_iter().take(most) {
        let (b, o) = (&balls[i], &balls[j]);
        let fire = firing(b).max(firing(o));
        if fire <= 0.0 {
            continue;
        }
        let seed = hash(b.number.wrapping_mul(0x01f3_5a7b) ^ o.number.wrapping_mul(0x5bd1_e995) ^ hash(tick));
        if unit(seed ^ 0x51) >= a.chance {
            continue;
        }
        let close = if reach > 0.0 { (1.0 - gap.max(0.0) / reach).clamp(0.0, 1.0).sqrt() } else { 1.0 };
        let strength = close * fire;
        let colour = match a.source {
            TrailSource::Own => [own[0], own[1], own[2], 1.0],
            TrailSource::Ball => {
                let (p, q) = (b.colour, o.colour);
                [(p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5, (p[2] + q[2]) * 0.5, 1.0]
            }
        };
        let dir = (o.at - b.at).normalize_or_zero();
        let from = b.at + dir * b.radius.max(0.0);
        let to = o.at - dir * o.radius.max(0.0);
        let points = bolt(from, to, detail, a.jagged, seed);
        if unit(seed ^ 0xb7) < a.branches && points.len() > 3 {
            // A fork leaves from the middle third, turned off to one side.
            let at = points.len() / 3 + (unit(seed ^ 0x3c) * (points.len() / 3) as f32) as usize;
            let start = points[at];
            let turn = if unit(seed ^ 0x77) < 0.5 { -0.6 } else { 0.6 };
            let len = from.distance(to) * 0.4;
            let end = start + Vec2::from_angle(turn).rotate(dir) * len;
            let fork = bolt(start, end, (detail / 2).max(2), a.jagged * 1.4, seed ^ 0xf00d);
            branches.push(Arc { points: fork, strength: strength * 0.6, colour });
        }
        out.push(Arc { points, strength, colour });
    }
    out.extend(branches);
    out
}

/// Draws arcs `a` (the around layer `layer`) between `balls` at `moment` into `frame`, and lights
/// a lit scene with the brightest of them.
pub fn draw(layer: &AroundLayer, a: &ArcsLayer, balls: &[ArcBall], moment: ArcMoment, frame: &mut Frame) {
    if a.width <= 0.0 && a.glow <= 0.0 {
        return;
    }
    let found = arcs(a, balls, moment);
    if found.is_empty() {
        return;
    }
    let trail = TrailLayer {
        brightness: a.brightness.max(0.0),
        fade: TrailFade::None,
        blend: a.blend,
        blend_amount: a.blend_amount,
        ..TrailLayer::default()
    };
    let radius = balls.iter().map(|b| b.radius).fold(0.0, f32::max).max(1.0);
    let core = StripLook { trail: &trail, subs: &layer.subs, hit: HitInfo::default(), radius };
    let halo = StripLook { trail: &trail, subs: &[], hit: HitInfo::default(), radius };
    let mut pieces = Pieces::default();
    let mut points_left = strip_budget(&layer.subs) / 2;
    for arc in &found {
        let n = arc.points.len();
        if n > points_left {
            break;
        }
        points_left -= n;
        let mut along = 0.0;
        let arcs_along: Vec<f32> = (0..n)
            .map(|k| {
                if k > 0 {
                    along += arc.points[k].distance(arc.points[k - 1]);
                }
                along
            })
            .collect();
        let path = Path { points: arc.points.clone(), ago: vec![0.0; n], arc: arcs_along, corner: vec![false; n] };
        let c = arc.colour;
        let s = arc.strength;
        if a.glow > 0.0 {
            let soft = [c[0] * s * 0.3, c[1] * s * 0.3, c[2] * s * 0.3, s * 0.3];
            let widths = vec![a.width.max(0.0) * a.glow * 0.5; n];
            strip_with_subs(&halo, &path, &widths, &vec![soft; n], 1.0, &mut pieces);
        }
        if a.width > 0.0 {
            // The core runs white-hot towards its colour.
            let hot = [(c[0] * s + s * 0.6), (c[1] * s + s * 0.6), (c[2] * s + s * 0.6), s];
            strip_with_subs(&core, &path, &vec![a.width * 0.5; n], &vec![hot; n], 0.5, &mut pieces);
        }
    }
    pieces.into_frame(frame, a.place == AroundPlace::InFront);
}

/// Lights a lit scene with the brightest arcs `a` draws between `balls` at `moment` (none when
/// Lights the scene is 0).
pub fn light(a: &ArcsLayer, balls: &[ArcBall], moment: ArcMoment, scene: &mut bpe_gfx::LightScene) {
    if a.lights == 0.0 {
        return;
    }
    let mut found = arcs(a, balls, moment);
    found.sort_by(|x, y| y.strength.total_cmp(&x.strength));
    for arc in found.iter().filter(|x| x.points.len() > 1).take(MOST_ARC_LIGHTS) {
        // Every pixel walks every light's segments: a few along the bolt light it as well.
        let n = arc.points.len();
        let points: Vec<Vec2> =
            (0..=LIGHT_SEGMENTS).map(|k| arc.points[(k * (n - 1) + LIGHT_SEGMENTS / 2) / LIGHT_SEGMENTS]).collect();
        let c = arc.colour;
        let k = a.lights * a.brightness.max(0.0) * arc.strength;
        crate::things::light::draw::push_strip(
            scene,
            &points,
            [c[0] * k, c[1] * k, c[2] * k],
            a.light_reach.max(0.0),
            a.width.max(0.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(x: f32, number: u32) -> ArcBall {
        ArcBall { at: Vec2::new(x, 0.0), radius: 20.0, number, colour: [1.0, 0.0, 0.0, 1.0], hit_age: 1e6 }
    }

    #[test]
    fn near_balls_arc_and_far_ones_dont() {
        let a = ArcsLayer { chance: 1.0, branches: 0.0, ..ArcsLayer::default() };
        let moment = ArcMoment { time: 1.0, beat: None };
        // 100 px apart edge to edge: inside the 160 px reach. The third is far away.
        let balls = [ball(0.0, 1), ball(140.0, 2), ball(2000.0, 3)];
        let found = arcs(&a, &balls, moment);
        assert_eq!(found.len(), 1);
        let arc = &found[0];
        assert_eq!(arc.points.len(), 16);
        assert!(arc.points[0].distance(Vec2::new(20.0, 0.0)) < 1e-3, "starts at an edge");
        assert!(arc.points[15].distance(Vec2::new(120.0, 0.0)) < 1e-3, "ends at the other");
        assert!(arc.points.iter().any(|p| p.y.abs() > 0.5), "jagged");
        assert_eq!(arcs(&a, &balls, moment), found, "the same moment draws the same arcs");
        // A flicker later the arc takes another shape.
        let later = arcs(&a, &balls, ArcMoment { time: 1.0 + 1.0 / a.flicker, beat: None });
        assert_ne!(later[0].points, arc.points);
        // Only on hits: no hit, no arc.
        let on_hits = ArcsLayer { fire: ArcFire::WhenTheyHit, ..a.clone() };
        assert!(arcs(&on_hits, &balls, moment).is_empty());
        let mut hit = balls;
        hit[1].hit_age = 0.05;
        assert_eq!(arcs(&on_hits, &hit, moment).len(), 1);
    }

    #[test]
    fn arcs_draw_strips_and_light_a_lit_scene() {
        let a = ArcsLayer { chance: 1.0, lights: 1.0, ..ArcsLayer::default() };
        let layer = AroundLayer {
            id: bpe_core::LayerId(1),
            kind: super::super::around::AroundKind::Arcs(Box::new(a.clone())),
            subs: Vec::new(),
            only: Vec::new(),
            light: [0.0; 4],
            emit: None,
        };
        let balls = [ball(0.0, 1), ball(140.0, 2)];
        let mut frame = Frame::default();
        let moment = ArcMoment { time: 0.5, beat: None };
        draw(&layer, &a, &balls, moment, &mut frame);
        assert!(!frame.trails_front.is_empty(), "in front of the balls by default");
        let mut scene = bpe_gfx::LightScene::default();
        light(&a, &balls, moment, &mut scene);
        assert!(!scene.lights.is_empty() && !scene.segments.is_empty());
        assert_eq!(scene.lights[0].p0[3], 4.0, "a strip light");
    }
}
