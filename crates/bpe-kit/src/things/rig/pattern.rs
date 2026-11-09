//! A rig's pattern, worked out from the moment drawn: which step it is on, and what each fixture
//! does in it. Pure: the same moment (and log) always gives the same light.

use bpe_core::{Gradient, ThingId};
use bpe_runtime::{EventKind, Log};

use super::{Order, Pattern, RigColours, RigSettings, StepsOn};
use crate::things::scene::SceneSettings;

/// Where a pattern is: the step it is on and how far through it, in steps (`INFINITY` before the
/// first hit of a pattern on hits).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub step: i64,
    pub phase: f32,
}

/// What a rig does to one of its lamps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drive {
    /// Multiplies the light's strength.
    pub strength: f32,
    /// Its colour from the rig's list (linear, before strength), if the list sets it.
    pub colour: Option<[f32; 3]>,
    /// Added to the light's direction, in degrees.
    pub turn: f32,
    /// Moves the lamp sideways (along x), in pixels.
    pub sway: f32,
}

impl Drive {
    /// No change.
    pub const NONE: Drive = Drive { strength: 1.0, colour: None, turn: 0.0, sway: 0.0 };

    /// This drive, then `outer` (a rig further up) on top: strengths multiply, turns and sways
    /// add, and the nearer rig's colour wins.
    pub fn then(self, outer: Drive) -> Drive {
        Drive {
            strength: self.strength * outer.strength,
            colour: self.colour.or(outer.colour),
            turn: self.turn + outer.turn,
            sway: self.sway + outer.sway,
        }
    }
}

/// Seconds in one beat and beats in a bar at the scene's tempo, the same as the sound's
/// `TempoMap` (and its clamps).
pub fn beat_seconds(s: &SceneSettings) -> (f64, u32) {
    let bpm = (s.tempo as f64).clamp(20.0, 300.0);
    let bottom = s.time_bottom.beats().clamp(1, 64);
    (60.0 / bpm * 4.0 / bottom as f64, s.time_top.clamp(1, 16))
}

/// The moments of the hits heard from `heard` up to `time`, a hit closer than `gap` after the
/// last one counted not counting. A hit by or against a heard thing is heard.
pub fn hits_heard(log: &Log, steps_per_second: u32, heard: &[ThingId], time: f64, gap: f64) -> Vec<f64> {
    let rate = steps_per_second.max(1);
    let last = (time * rate as f64 + 1e-6).floor().max(0.0) as u64 + 1;
    let mut out: Vec<f64> = Vec::new();
    for e in log.between(0, last) {
        if e.kind != EventKind::Hit {
            continue;
        }
        let by = heard.contains(&e.a.thing) || e.b.is_some_and(|b| heard.contains(&b.thing));
        if !by {
            continue;
        }
        let at = e.time(rate);
        if at > time + 1e-9 {
            break;
        }
        if out.last().is_none_or(|&l| at - l >= gap) {
            out.push(at);
        }
    }
    out
}

/// Where the pattern is at `time`, for steps that come with time (not hits).
pub fn clock_in_time(s: &RigSettings, scene: &SceneSettings, time: f64) -> Clock {
    let (beat, top) = beat_seconds(scene);
    let length = match s.steps_on {
        StepsOn::Beats => beat * s.every_beats.max(1e-3) as f64,
        StepsOn::Bars => beat * top as f64 * s.every_bars.max(1e-3) as f64,
        StepsOn::Seconds | StepsOn::Hits => s.every_seconds.max(1e-3) as f64,
    };
    // Beats and bars count from the scene's first beat; seconds from the start.
    let from = match s.steps_on {
        StepsOn::Beats | StepsOn::Bars => crate::things::scene::first_beat_seconds(scene.first_beat),
        StepsOn::Seconds | StepsOn::Hits => 0.0,
    };
    let x = (time - from) / length;
    let step = x.floor();
    Clock { step: step as i64, phase: (x - step) as f32 }
}

/// Where the pattern is at `time` when it steps on hits: every `every_hits`-th heard hit starts
/// a step that lasts `step_lasts` seconds.
pub fn clock_on_hits(s: &RigSettings, hits: &[f64], time: f64) -> Clock {
    let every = s.every_hits.max(1) as usize;
    let step = hits.len() / every;
    if step == 0 {
        return Clock { step: 0, phase: f32::INFINITY };
    }
    let start = hits[step * every - 1];
    Clock { step: step as i64, phase: ((time - start) / s.step_lasts.max(1e-3) as f64) as f32 }
}

/// How many places `n` fixtures make in `order`: the middle-out orders light pairs together.
pub fn place_count(order: Order, n: usize) -> usize {
    match order {
        Order::FromTheMiddleOut | Order::ToTheMiddle => n.div_ceil(2),
        _ => n,
    }
}

/// The place fixture `i` of `n` takes in `order`.
pub fn place_of(order: Order, i: usize, n: usize, seed: u32) -> usize {
    let n = n.max(1);
    let i = i.min(n - 1);
    let middle_out = || ((2 * i as i64 - (n as i64 - 1)).unsigned_abs() / 2) as usize;
    match order {
        Order::Forward | Order::BackAndForth => i,
        Order::Backward => n - 1 - i,
        Order::FromTheMiddleOut => middle_out(),
        Order::ToTheMiddle => place_count(order, n) - 1 - middle_out(),
        Order::Shuffled => shuffled(n, seed)[i],
    }
}

/// A seeded order of `0..n` (Fisher–Yates), the same every time for the same seed.
fn shuffled(n: usize, seed: u32) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    for k in (1..n).rev() {
        let j = (hash(seed, k as i64, 0x5u64) % (k as u64 + 1)) as usize;
        v.swap(k, j);
    }
    v
}

/// A well-mixed number from a seed, a step and a fixture.
fn hash(seed: u32, step: i64, item: u64) -> u64 {
    let x =
        (seed as u64) ^ (step as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ item.wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    bpe_core::dice::splitmix64(x)
}

/// A number from 0 up to (not including) 1.
fn hash01(seed: u32, step: i64, item: u64) -> f32 {
    bpe_core::dice::unit24_of_64(hash(seed, step, item))
}

/// The rig's colour list, linear: its colours' stops in order.
pub fn colour_list(colours: &Gradient) -> Vec<[f32; 3]> {
    colours
        .step_stops()
        .iter()
        .map(|s| {
            let c = s.color.to_linear();
            [c[0], c[1], c[2]]
        })
        .collect()
}

/// Where the chase's head is at step `k` over `m` places.
fn head(order: Order, k: i64, m: usize) -> i64 {
    let m = m.max(1) as i64;
    if order == Order::BackAndForth {
        if m <= 1 {
            return 0;
        }
        let period = 2 * (m - 1);
        let r = k.rem_euclid(period);
        return if r < m { r } else { period - r };
    }
    k.rem_euclid(m)
}

/// How bright a light is that was last lit `ago` steps back, `phase` into this step: full in its
/// own step, then fading over `tail` steps.
fn tail_level(ago: i64, phase: f32, tail: f32) -> f32 {
    if ago == 0 {
        return 1.0;
    }
    if tail <= 0.0 || !phase.is_finite() {
        return 0.0;
    }
    let e = (ago - 1) as f32 + phase;
    (1.0 - e / tail).max(0.0)
}

/// The furthest back a pattern looks for when a light was last lit.
const MOST_LOOK_BACK: i64 = 4096;

/// What a rig's pattern needs that is the same for all its fixtures in a frame: the colour list
/// and each fixture's place. Made once per rig, then [`Prepared::drive`] per lamp.
pub struct Prepared {
    list: Vec<[f32; 3]>,
    /// Each fixture's place, by its number.
    places: Vec<usize>,
    /// How many places there are.
    count: usize,
}

impl Prepared {
    pub fn new(s: &RigSettings, n: usize) -> Prepared {
        let list = if s.colours_from == RigColours::TheList { colour_list(&s.colours) } else { Vec::new() };
        let places = match s.order {
            Order::Shuffled => shuffled(n, s.seed),
            order => (0..n).map(|i| place_of(order, i, n, s.seed)).collect(),
        };
        Prepared { list, places, count: place_count(s.order, n) }
    }

    /// What the rig does to fixture `i` at `clock`.
    pub fn drive(&self, s: &RigSettings, clock: Clock, i: usize) -> Drive {
        let Some(&p) = self.places.get(i) else { return Drive::NONE };
        let list = &self.list;
        let pick = |k: i64| (!list.is_empty()).then(|| list[k.rem_euclid(list.len() as i64) as usize]);
        let (k, phase) = (clock.step, clock.phase);
        let (m, p) = (self.count.max(1) as i64, p as i64);
        let tail = s.tail.max(0.0);
        let (level, colour, wave) = match s.pattern {
            Pattern::Still => (1.0, pick(p), 0.0),
            Pattern::Chase => {
                let width = s.lit_at_once.max(0.0).ceil().min(1e9) as i64;
                let ago = if width == 0 {
                    None
                } else if s.order == Order::BackAndForth {
                    // Lit while within `width` places behind the head, the way it is going.
                    let lit = |step: i64| {
                        let h = head(s.order, step, m as usize);
                        let way = (h - head(s.order, step - 1, m as usize)).signum();
                        let behind = (h - p) * way;
                        if way == 0 { h == p } else { (0..width).contains(&behind) }
                    };
                    let period = (2 * (m - 1)).max(1);
                    (0..=period.min(MOST_LOOK_BACK)).find(|&ago| lit(k - ago))
                } else {
                    // `d` places behind the head: lit within `width`, else lit `d − width + 1` steps ago.
                    let d = (head(s.order, k, m as usize) - p).rem_euclid(m);
                    Some(if d < width { 0 } else { d - width + 1 })
                };
                match ago {
                    Some(ago) => (tail_level(ago, phase, tail), pick(k - ago), 0.0),
                    None => (0.0, None, 0.0),
                }
            }
            Pattern::Strobe => {
                let flash = s.flash_length;
                let level = if phase < flash {
                    1.0
                } else if tail > 0.0 && phase.is_finite() {
                    (1.0 - (phase - flash) / tail).max(0.0)
                } else {
                    0.0
                };
                (level, pick(k), 0.0)
            }
            Pattern::Sweep => {
                let x = k as f64 + if phase.is_finite() { phase as f64 } else { 0.0 };
                let wave = (std::f64::consts::PI * x + std::f64::consts::TAU * p as f64 * s.offset as f64).sin() as f32;
                (1.0, pick(p), wave)
            }
            Pattern::AlternateColours => (1.0, pick(p + k), 0.0),
            Pattern::Random => {
                let look = (tail.ceil().min(1e9) as i64 + 1).min(MOST_LOOK_BACK);
                let lit = |step: i64| hash01(s.seed, step, i as u64) < s.lit_share;
                match (0..=look).find(|&ago| lit(k - ago)) {
                    Some(ago) => {
                        let at = k - ago;
                        let colour = (!list.is_empty())
                            .then(|| list[(hash(s.seed, at, i as u64 ^ 0xc01) % list.len() as u64) as usize]);
                        (tail_level(ago, phase, tail), colour, 0.0)
                    }
                    None => (0.0, None, 0.0),
                }
            }
        };
        let off = s.off_level;
        Drive {
            strength: s.brightness * (off + (1.0 - off) * level),
            colour,
            turn: 0.5 * s.sweep_angle * wave,
            sway: 0.5 * s.sway * wave,
        }
    }
}

/// What the rig does to fixture `i` of `n` at `clock` (one lamp; a rig drawing many prepares once).
pub fn drive(s: &RigSettings, clock: Clock, i: usize, n: usize) -> Drive {
    Prepared::new(s, n).drive(s, clock, i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Color;

    fn rig(f: impl FnOnce(&mut RigSettings)) -> RigSettings {
        let mut s = RigSettings { off_level: 0.0, tail: 0.0, ..RigSettings::default() };
        f(&mut s);
        s
    }

    fn levels(s: &RigSettings, step: i64, phase: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| drive(s, Clock { step, phase }, i, n).strength).collect()
    }

    #[test]
    fn a_forward_chase_lights_one_light_a_step_round_and_round() {
        let s = rig(|_| {});
        assert_eq!(levels(&s, 0, 0.5, 4), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(levels(&s, 2, 0.5, 4), [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(levels(&s, 5, 0.0, 4), [0.0, 1.0, 0.0, 0.0]);
        let back = rig(|s| s.order = Order::Backward);
        assert_eq!(levels(&back, 0, 0.0, 4), [0.0, 0.0, 0.0, 1.0]);
        let two = rig(|s| s.lit_at_once = 2.0);
        assert_eq!(levels(&two, 2, 0.0, 4), [0.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn a_chase_tail_fades_over_its_steps() {
        let s = rig(|s| s.tail = 2.0);
        let l = levels(&s, 3, 0.5, 4);
        assert_eq!(l[3], 1.0, "the head");
        assert!((l[2] - 0.75).abs() < 1e-6, "one step back, half way: {l:?}");
        assert!((l[1] - 0.25).abs() < 1e-6, "two steps back: {l:?}");
        assert_eq!(l[0], 0.0);
        let off = rig(|s| s.off_level = 0.2);
        assert_eq!(levels(&off, 0, 0.0, 2), [1.0, 0.2]);
    }

    #[test]
    fn middle_out_and_back_and_forth_orders() {
        let s = rig(|s| s.order = Order::FromTheMiddleOut);
        assert_eq!(levels(&s, 0, 0.0, 4), [0.0, 1.0, 1.0, 0.0], "the middle pair first");
        assert_eq!(levels(&s, 1, 0.0, 4), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(levels(&s, 0, 0.0, 5), [0.0, 0.0, 1.0, 0.0, 0.0]);
        let to = rig(|s| s.order = Order::ToTheMiddle);
        assert_eq!(levels(&to, 0, 0.0, 4), [1.0, 0.0, 0.0, 1.0]);
        let bounce = rig(|s| s.order = Order::BackAndForth);
        let heads: Vec<usize> =
            (0..7).map(|k| levels(&bounce, k, 0.0, 4).iter().position(|&l| l == 1.0).unwrap()).collect();
        assert_eq!(heads, [0, 1, 2, 3, 2, 1, 0]);
        // Two lit at once: the head and the one behind it, the way it is going.
        let two = rig(|s| {
            s.order = Order::BackAndForth;
            s.lit_at_once = 2.0;
        });
        assert_eq!(levels(&two, 2, 0.0, 6), [0.0, 1.0, 1.0, 0.0, 0.0, 0.0], "going up");
        assert_eq!(levels(&two, 7, 0.0, 6), [0.0, 0.0, 0.0, 1.0, 1.0, 0.0], "coming back: head at 3, 4 behind it");
    }

    #[test]
    fn a_shuffled_order_is_every_light_once_and_keeps_to_its_seed() {
        let a: Vec<usize> = (0..8).map(|i| place_of(Order::Shuffled, i, 8, 3)).collect();
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(sorted, (0..8).collect::<Vec<_>>());
        assert_eq!(a, (0..8).map(|i| place_of(Order::Shuffled, i, 8, 3)).collect::<Vec<_>>());
        assert_ne!(a, (0..8).map(|i| place_of(Order::Shuffled, i, 8, 4)).collect::<Vec<_>>());
    }

    #[test]
    fn a_strobe_flashes_every_light_for_its_flash_length() {
        let s = rig(|s| {
            s.pattern = Pattern::Strobe;
            s.flash_length = 0.25;
        });
        assert_eq!(levels(&s, 3, 0.1, 3), [1.0, 1.0, 1.0]);
        assert_eq!(levels(&s, 3, 0.3, 3), [0.0, 0.0, 0.0]);
        let tailed = rig(|s| {
            s.pattern = Pattern::Strobe;
            s.tail = 0.5;
        });
        assert!((levels(&tailed, 0, 0.5, 1)[0] - 0.5).abs() < 1e-6);
        assert_eq!(levels(&tailed, 0, f32::INFINITY, 1)[0], 0.0, "dark before the first hit");
    }

    #[test]
    fn alternate_colours_move_one_light_a_step() {
        let s = rig(|s| {
            s.pattern = Pattern::AlternateColours;
            s.colours_from = RigColours::TheList;
            s.colours = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        });
        let red = Some([1.0, 0.0, 0.0]);
        let blue = Some([0.0, 0.0, 1.0]);
        let at = |k: i64, i: usize| drive(&s, Clock { step: k, phase: 0.0 }, i, 4).colour;
        assert_eq!([at(0, 0), at(0, 1), at(0, 2)], [red, blue, red]);
        assert_eq!([at(1, 0), at(1, 1), at(1, 2)], [blue, red, blue]);
        assert_eq!(drive(&s, Clock { step: 0, phase: 0.0 }, 0, 4).strength, 1.0);
        let own = rig(|s| s.pattern = Pattern::AlternateColours);
        assert_eq!(drive(&own, Clock { step: 0, phase: 0.0 }, 0, 4).colour, None, "the lights' own colours");
    }

    #[test]
    fn random_keeps_to_its_seed_and_lights_about_its_share() {
        let s = rig(|s| {
            s.pattern = Pattern::Random;
            s.lit_share = 0.3;
        });
        let lit: usize = (0..200).map(|k| levels(&s, k, 0.0, 10).iter().filter(|&&l| l == 1.0).count()).sum();
        assert!((400..800).contains(&lit), "about 30% of 2000: {lit}");
        assert_eq!(levels(&s, 17, 0.0, 10), levels(&s, 17, 0.0, 10));
        let other = rig(|s| {
            s.pattern = Pattern::Random;
            s.lit_share = 0.3;
            s.seed = 9;
        });
        assert_ne!(
            (0..20).map(|k| levels(&s, k, 0.0, 10)).collect::<Vec<_>>(),
            (0..20).map(|k| levels(&other, k, 0.0, 10)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_sweep_swings_there_in_one_step_and_back_in_the_next() {
        let s = rig(|s| {
            s.pattern = Pattern::Sweep;
            s.sweep_angle = 60.0;
            s.sway = 100.0;
            s.offset = 0.5;
        });
        let d = drive(&s, Clock { step: 0, phase: 0.5 }, 0, 2);
        assert!((d.turn - 30.0).abs() < 1e-4 && (d.sway - 50.0).abs() < 1e-3, "{d:?}");
        assert!((drive(&s, Clock { step: 1, phase: 0.5 }, 0, 2).turn + 30.0).abs() < 1e-4);
        let neighbour = drive(&s, Clock { step: 0, phase: 0.5 }, 1, 2);
        assert!((neighbour.turn + 30.0).abs() < 1e-4, "half a swing behind: {neighbour:?}");
    }

    #[test]
    fn clocks_count_beats_bars_seconds_and_hits() {
        let scene = SceneSettings::default();
        let s = rig(|_| {});
        assert_eq!(clock_in_time(&s, &scene, 1.25), Clock { step: 2, phase: 0.5 }, "120 BPM: half-second beats");
        let bars = rig(|s| s.steps_on = StepsOn::Bars);
        assert_eq!(clock_in_time(&bars, &scene, 5.0), Clock { step: 2, phase: 0.5 });
        let hits = rig(|s| {
            s.steps_on = StepsOn::Hits;
            s.every_hits = 2;
            s.step_lasts = 0.5;
        });
        assert_eq!(clock_on_hits(&hits, &[0.5], 1.0).step, 0);
        assert_eq!(clock_on_hits(&hits, &[0.5, 1.0, 1.5], 1.25), Clock { step: 1, phase: 0.5 });
    }

    #[test]
    fn heard_hits_skip_others_and_hits_too_close() {
        use bpe_runtime::{Event, Party};
        let mut log = Log::default();
        let hit = |step: u64, thing: u32| Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(thing), item: 0 },
            b: None,
            point: glam::Vec2::ZERO,
            normal: glam::Vec2::ZERO,
            speed: 100.0,
            at: 0.0,
        };
        for e in [hit(10, 2), hit(11, 2), hit(30, 3), hit(100, 2), hit(500, 2)] {
            log.push(e);
        }
        let heard = hits_heard(&log, 100, &[ThingId(2)], 2.0, 0.05);
        assert_eq!(heard, vec![0.1, 1.0], "the second hit too close, thing 3 not heard, the last not yet");
    }
}
