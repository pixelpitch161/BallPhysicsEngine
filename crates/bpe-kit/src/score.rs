//! Scores how satisfying a run is likely to be, so the explorer can rank variations and a batch
//! can keep only the best. Six parts, each 0 (bad) to 1 (great), all from one simulation pass:
//!
//! - **Rhythm**: hits at a pleasing pace, fairly regular, and on the beat when the scene has one.
//! - **Build-up**: more happens towards the end than at the start.
//! - **Climax**: the biggest moment comes in the last third, and something still happens at the end.
//! - **Motion**: balls keep moving, stay on screen, and counts don't run away.
//! - **Variety**: hits of different strengths, a pattern with some swing, counts that change.
//! - **Sound**: a comfortable number of hits a second, not a wash of the loudest ones.
//!
//! The total is a weighted average, with weights the user's thumbs up and down nudge.

use crate::music::{beats_for, root_settings};
use bpe_core::{Resolved, Scene, SlotId, ThingId};
use bpe_runtime::{EventKind, Log, Sims, World, actions::MAX_BALLS};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Hits are grouped into time slots this long, about the shortest gap the ear hears as two
/// separate sounds in a busy passage.
pub const SLOT: f64 = 1.0 / 16.0;
/// How often ball counts and speeds are sampled, in seconds.
pub const SAMPLE_EVERY: f64 = 0.1;
/// A hit within this many seconds of a beat counts as on the beat.
pub const ON_BEAT: f64 = 0.05;
/// Lowest and highest a weight can be nudged to by thumbs up and down.
const WEIGHT_RANGE: (f32, f32) = (0.05, 3.0);
/// A moment counts as fully seen when this many of its balls, or all of them if there are fewer,
/// are inside the picture.
const ENOUGH_TO_SEE: u32 = 8;

/// The six parts of a score, each 0 to 1. The same shape holds the weights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Parts {
    pub rhythm: f32,
    pub build_up: f32,
    pub climax: f32,
    pub motion: f32,
    pub variety: f32,
    pub sound: f32,
}

impl Parts {
    /// How much each part counts to begin with.
    pub const WEIGHTS: Parts = Parts { rhythm: 1.0, build_up: 0.7, climax: 1.0, motion: 1.0, variety: 0.8, sound: 0.5 };

    /// (name, value, what it means) for each part, in order, for tooltips and reports.
    pub fn list(&self) -> [(&'static str, f32, &'static str); 6] {
        [
            ("Rhythm", self.rhythm, "Hits at a pleasing pace, fairly regular, on the beat"),
            ("Build-up", self.build_up, "More happens towards the end than at the start"),
            ("Climax", self.climax, "The biggest moment comes near the end"),
            ("Motion", self.motion, "Balls keep moving and stay on screen"),
            ("Variety", self.variety, "Hits of different strengths and a pattern that changes"),
            ("Sound", self.sound, "A comfortable number of hits, not a wall of the loudest"),
        ]
    }

    fn array(&self) -> [f32; 6] {
        [self.rhythm, self.build_up, self.climax, self.motion, self.variety, self.sound]
    }

    fn from_array(w: [f32; 6]) -> Parts {
        Parts { rhythm: w[0], build_up: w[1], climax: w[2], motion: w[3], variety: w[4], sound: w[5] }
    }

    /// The weighted average of the parts under `weights`, 0 to 1.
    pub fn total(&self, weights: &Parts) -> f32 {
        let (mut sum, mut weight) = (0.0, 0.0);
        for (value, w) in self.array().iter().zip(weights.array()) {
            sum += value.clamp(0.0, 1.0) * w.max(0.0);
            weight += w.max(0.0);
        }
        if weight > 0.0 { sum / weight } else { 0.0 }
    }
}

/// A run's score: the total under some weights, and the parts it came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub total: f32,
    pub parts: Parts,
}

/// Ball counts and speeds at one moment of the run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub time: f64,
    /// Live balls (liquid particles don't count).
    pub count: u32,
    /// Of those, how many are inside the picture.
    pub on_screen: u32,
    pub fastest: f32,
    pub average: f32,
}

/// What one simulation pass tells the scorer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampled {
    pub duration: f64,
    /// Every hit: (time in seconds, speed into the contact).
    pub hits: Vec<(f64, f32)>,
    /// A sample every [`SAMPLE_EVERY`] seconds.
    pub frames: Vec<Sample>,
    /// The scene's beat grid, empty without one.
    pub beats: Vec<f64>,
    /// Hits slower than this make no sound; hits this fast are at full volume.
    pub quietest: f32,
    pub loudest: f32,
}

/// Runs the scene once without drawing or mixing, sampling as it goes. `None` when `stop` was
/// set before the run finished.
pub fn sample(scene: &Scene, resolved: &Resolved, sims: &Sims, stop: &AtomicBool) -> Option<Sampled> {
    let settings = root_settings(resolved);
    let duration = settings.duration.max(0.0) as f64;
    let (half_w, half_h) = (settings.width as f32 * 0.5, settings.height as f32 * 0.5);
    let (quietest, loudest) = sound_thresholds(resolved);
    let mut world = World::build(scene, resolved, sims);
    let mut log = Log::default();
    let mut out = Sampled { duration, beats: beats_for(&settings), quietest, loudest, ..Sampled::default() };
    let samples = (duration / SAMPLE_EVERY).ceil() as usize;
    for i in 1..=samples {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let time = (i as f64 * SAMPLE_EVERY).min(duration);
        world.advance_to(time, sims, &mut log);
        let (mut count, mut on_screen, mut fastest, mut total) = (0u32, 0u32, 0.0f32, 0.0f32);
        for bodies in world.bodies.values().filter(|b| !b.silent) {
            for (i, &alive) in bodies.alive.iter().enumerate() {
                if !alive {
                    continue;
                }
                let (p, v) = (bodies.position[i], bodies.velocity[i]);
                if !(p.is_finite() && v.is_finite()) {
                    continue;
                }
                count += 1;
                if p.x.abs() <= half_w && p.y.abs() <= half_h {
                    on_screen += 1;
                }
                let speed = v.length();
                fastest = fastest.max(speed);
                total += speed;
            }
        }
        let average = if count > 0 { total / count as f32 } else { 0.0 };
        out.frames.push(Sample { time, count, on_screen, fastest, average });
    }
    let rate = world.steps_per_second.max(1);
    out.hits = log.events().iter().filter(|e| e.kind == EventKind::Hit).map(|e| (e.time(rate), e.speed)).collect();
    Some(out)
}

/// The root's sound module decides which hits can be heard.
fn sound_thresholds(resolved: &Resolved) -> (f32, f32) {
    let params = resolved.things.get(&ThingId::ROOT).and_then(|r| r.slots.get(&SlotId::Sound)).map(|s| &s.params);
    let number = |key: &str, fallback: f32| -> f32 {
        match params.and_then(|p| p.get(key)) {
            Some(bpe_core::Value::Number(x)) => *x as f32,
            _ => fallback,
        }
    };
    let quietest = number("quietest_hit", 20.0).max(0.0);
    (quietest, number("loudest_hit", 600.0).max(quietest + 1.0))
}

/// The score of a sampled run under `weights`.
pub fn score(sampled: &Sampled, weights: &Parts) -> Score {
    let activity = Activity::new(sampled);
    let parts = Parts {
        rhythm: rhythm(&activity, &sampled.beats),
        build_up: build_up(&activity, sampled),
        climax: climax(&activity, sampled),
        motion: motion(&activity, sampled),
        variety: variety(&activity, sampled),
        sound: sound(&activity, sampled),
    };
    // Hits that sound good are worth nothing when the balls making them are out of the picture,
    // so the whole score shrinks with how much of the run you can't see.
    Score { total: parts.total(weights) * seen(sampled).powi(2), parts }
}

/// How much of the run you can see, 0 to 1: the share of moments with enough balls in the picture.
fn seen(s: &Sampled) -> f32 {
    let moments = s.frames.iter().filter(|f| f.count > 0);
    let (sum, n) = moments.fold((0.0, 0u32), |(sum, n), f| {
        (sum + (f.on_screen as f32 / f.count.min(ENOUGH_TO_SEE) as f32).min(1.0), n + 1)
    });
    if n > 0 { sum / n as f32 } else { 1.0 }
}

/// Nudges the weights after a thumbs up (`like`) or down on a run with these parts, by one step
/// of online logistic regression: parts that were high on liked runs count more.
pub fn learn(weights: &mut Parts, parts: &Parts, like: bool) {
    const RATE: f32 = 0.4;
    const SHARPNESS: f32 = 4.0;
    let x: [f32; 6] = parts.array().map(|v| v - 0.5);
    let mut w = weights.array();
    let guess = 1.0 / (1.0 + (-SHARPNESS * w.iter().zip(&x).map(|(w, x)| w * x).sum::<f32>()).exp());
    let error = if like { 1.0 } else { 0.0 } - guess;
    for (w, x) in w.iter_mut().zip(x) {
        *w = (*w + RATE * error * SHARPNESS * x).clamp(WEIGHT_RANGE.0, WEIGHT_RANGE.1);
    }
    *weights = Parts::from_array(w);
}

/// Hits you can hear, sorted into slots.
struct Activity {
    /// Per slot: summed loudness of the hits in it, each hit 0 to 1.
    slots: Vec<f32>,
    /// Time of the first audible hit in each slot that has one.
    onsets: Vec<f64>,
    /// Loudness of every audible hit, 0 to 1.
    strengths: Vec<f32>,
}

impl Activity {
    fn new(s: &Sampled) -> Self {
        let count = ((s.duration / SLOT).ceil() as usize).max(1);
        let mut slots = vec![0.0f32; count];
        let mut first = vec![f64::NAN; count];
        let mut strengths = Vec::new();
        for &(time, speed) in &s.hits {
            if speed < s.quietest {
                continue;
            }
            let strength = ((speed - s.quietest) / (s.loudest - s.quietest)).clamp(0.05, 1.0);
            let slot = ((time / SLOT) as usize).min(count - 1);
            slots[slot] += strength;
            strengths.push(strength);
            if first[slot].is_nan() {
                first[slot] = time;
            }
        }
        let onsets = first.into_iter().filter(|t| !t.is_nan()).collect();
        Self { slots, onsets, strengths }
    }

    /// Activity in each slot, 0 to 1 (a few loud hits fill a slot).
    fn level(&self, slot: usize) -> f32 {
        1.0 - (-self.slots[slot]).exp()
    }

    fn mean_level(&self, range: std::ops::Range<usize>) -> f32 {
        let len = range.len().max(1) as f32;
        range.map(|i| self.level(i)).sum::<f32>() / len
    }
}

/// 1 inside `low..high`, falling off smoothly (on a log scale) outside it.
fn band(value: f32, low: f32, high: f32) -> f32 {
    if value <= 0.0 {
        return 0.0;
    }
    let off = if value < low {
        (low / value).ln()
    } else if value > high {
        (value / high).ln()
    } else {
        0.0
    };
    (-off * off / (2.0 * 0.45 * 0.45)).exp()
}

/// Mean and spread (standard deviation over the mean) of a list.
fn mean_and_wobble(xs: &[f64]) -> (f64, f32) {
    if xs.is_empty() {
        return (0.0, 0.0);
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let spread = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt();
    (mean, (spread / mean.abs().max(1e-6)) as f32)
}

fn rhythm(activity: &Activity, beats: &[f64]) -> f32 {
    let slots = activity.slots.len() as f32;
    // Share of slots with a sound in them: a few a second is pleasing, every slot is a wash.
    let busy = activity.onsets.len() as f32 / slots;
    let pace = band(busy, 0.1, 0.55);
    if activity.onsets.len() < 3 {
        return pace * 0.5;
    }
    let gaps: Vec<f64> = activity.onsets.windows(2).map(|w| w[1] - w[0]).collect();
    let (_, wobble) = mean_and_wobble(&gaps);
    // Some swing is fine; a wildly uneven pattern feels random.
    let regular = (-(wobble - 0.4).max(0.0) * 1.2).exp();
    if beats.is_empty() {
        return 0.65 * pace + 0.35 * regular;
    }
    let on_beat =
        activity.onsets.iter().filter(|&&t| near_beat(beats, t)).count() as f32 / activity.onsets.len() as f32;
    // Random hits land within 50 ms of a beat about a fifth of the time at 120 bpm.
    let beat_score = ((on_beat - 0.2) / 0.5).clamp(0.0, 1.0);
    0.4 * pace + 0.2 * regular + 0.4 * beat_score
}

/// Whether `time` is within [`ON_BEAT`] of a beat. `beats` must be sorted.
pub fn near_beat(beats: &[f64], time: f64) -> bool {
    let i = beats.partition_point(|&b| b < time);
    let after = beats.get(i).map_or(f64::INFINITY, |b| b - time);
    let before = i.checked_sub(1).map_or(f64::INFINITY, |j| time - beats[j]);
    after.min(before) <= ON_BEAT
}

/// How much is going on between `from` and `to`: sound, balls and speed.
fn energy(activity: &Activity, s: &Sampled, from: f64, to: f64) -> [f32; 3] {
    let slot = |t: f64| ((t / SLOT) as usize).min(activity.slots.len());
    let sound = activity.mean_level(slot(from)..slot(to));
    let sample = |t: f64| ((t / SAMPLE_EVERY) as usize).min(s.frames.len());
    let picked = &s.frames[sample(from)..sample(to).max(sample(from))];
    let n = picked.len().max(1) as f32;
    let balls = picked.iter().map(|f| f.count as f32).sum::<f32>() / n;
    let speed = picked.iter().map(|f| f.average).sum::<f32>() / n;
    [sound, balls, speed]
}

fn build_up(activity: &Activity, s: &Sampled) -> f32 {
    let d = s.duration;
    if d <= 0.0 {
        return 0.0;
    }
    let start = energy(activity, s, 0.0, d / 3.0);
    let end = energy(activity, s, d * 2.0 / 3.0, d);
    // Relative growth of each measure from the first third to the last, -1 to 1.
    let growth = |a: f32, b: f32| if a + b > 1e-6 { (b - a) / (a + b) } else { 0.0 };
    let g = 0.5 * growth(start[0], end[0]) + 0.3 * growth(start[1], end[1]) + 0.2 * growth(start[2], end[2]);
    // A video that goes silent scores low however it started.
    let alive = (end[0] / 0.05).min(1.0);
    (0.5 + 0.5 * (2.5 * g).tanh()) * (0.3 + 0.7 * alive)
}

fn climax(activity: &Activity, s: &Sampled) -> f32 {
    let d = s.duration;
    let slots = activity.slots.len();
    if d <= 0.0 || activity.onsets.is_empty() {
        return 0.0;
    }
    // The busiest second, found with a sliding window.
    let window = ((1.0 / SLOT) as usize).clamp(1, slots);
    let mut sum: f32 = (0..window).map(|i| activity.level(i)).sum();
    let (mut best, mut best_at) = (sum, 0);
    for i in window..slots {
        sum += activity.level(i) - activity.level(i - window);
        if sum > best + 1e-4 {
            best = sum;
            best_at = i + 1 - window;
        }
    }
    let peak_time = (best_at + window / 2) as f64 * SLOT;
    let peak = (peak_time / d) as f32;
    let peak_score = if peak >= 0.66 {
        1.0
    } else if peak >= 0.4 {
        0.4 + (peak - 0.4) / 0.26 * 0.6
    } else {
        0.25
    };
    // Something still happening in the last part, compared with the whole.
    let overall = activity.mean_level(0..slots);
    let tail = activity.mean_level((slots * 85 / 100)..slots);
    let ending = if overall > 0.0 { (tail / overall / 1.2).min(1.0) } else { 0.0 };
    0.55 * peak_score + 0.45 * ending
}

fn motion(activity: &Activity, s: &Sampled) -> f32 {
    // Long silences feel like the video stopped.
    let quiet = longest_gap(&activity.onsets, s.duration) as f32;
    let silence = (1.0 - ((quiet - 2.0) / 6.0).clamp(0.0, 1.0)).max(0.0);
    if s.frames.is_empty() {
        return 0.3 + 0.6 * silence;
    }
    let n = s.frames.len() as f32;
    let stalled = s.frames.iter().filter(|f| f.count == 0 || f.fastest < 15.0).count() as f32 / n;
    let on_screen =
        s.frames.iter().map(|f| if f.count > 0 { f.on_screen as f32 / f.count as f32 } else { 0.0 }).sum::<f32>() / n;
    // Balls multiplying until they hit the limit, then staying pinned there for most of the
    // video, looks like a glitch. Scenes that simply start with many balls are fine.
    let start = s.frames.first().map_or(0, |f| f.count);
    let cap = MAX_BALLS as u32;
    let capped = s.frames.iter().filter(|f| f.count >= cap && f.count > start).count() as f32 / n;
    let runaway = 1.0 - ((capped - 0.4) / 0.6).clamp(0.0, 1.0) * 0.6;
    (1.0 - stalled).powf(1.5) * (0.3 + 0.7 * on_screen) * (0.4 + 0.6 * silence) * runaway
}

fn variety(activity: &Activity, s: &Sampled) -> f32 {
    if activity.strengths.len() < 3 {
        return 0.0;
    }
    // Hits of different strengths sound richer than one thud repeated.
    let strengths: Vec<f64> = activity.strengths.iter().map(|&x| x as f64).collect();
    let (_, strength_wobble) = mean_and_wobble(&strengths);
    let strength = band(strength_wobble, 0.2, 1.0);
    // A pattern with some swing, but not noise.
    let gaps: Vec<f64> = activity.onsets.windows(2).map(|w| w[1] - w[0]).collect();
    let (_, gap_wobble) = mean_and_wobble(&gaps);
    let pattern = band(gap_wobble, 0.15, 1.2);
    // Counts that change over the run (balls appearing or going) add interest.
    let most = s.frames.iter().map(|f| f.count).max().unwrap_or(0);
    let least = s.frames.iter().map(|f| f.count).min().unwrap_or(0);
    let counts = if most > 0 { 0.5 + 0.5 * (most - least) as f32 / most as f32 } else { 0.0 };
    0.4 * strength + 0.3 * pattern + 0.3 * counts
}

fn sound(activity: &Activity, s: &Sampled) -> f32 {
    if s.duration <= 0.0 {
        return 0.0;
    }
    let per_second = activity.strengths.len() as f32 / s.duration as f32;
    let pace = band(per_second, 1.5, 12.0);
    // A run where most hits are at full volume is harsh.
    let loud =
        activity.strengths.iter().filter(|&&x| x >= 0.999).count() as f32 / activity.strengths.len().max(1) as f32;
    let gentle = 1.0 - ((loud - 0.5) / 0.5).clamp(0.0, 1.0);
    0.7 * pace + 0.3 * gentle
}

/// The longest stretch with no audible hit, including before the first and after the last.
fn longest_gap(onsets: &[f64], duration: f64) -> f64 {
    let mut last = 0.0;
    let mut longest: f64 = 0.0;
    for &t in onsets {
        longest = longest.max(t - last);
        last = t;
    }
    longest.max(duration - last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{new_scene, registry, resolve_scene, sims};
    use bpe_core::{Address, Owner, Registry, Scene, ThingId, Value, ValueType};
    use std::sync::atomic::AtomicBool;

    fn own(id: ThingId, key: &str) -> Address {
        Address { owner: Owner::Thing(id), slot: None, key: key.into() }
    }

    fn short(reg: &Registry, scene: &mut Scene) {
        scene.set_at(reg, &own(ThingId::ROOT, "duration"), Value::Number(6.0)).unwrap();
    }

    fn sampled(scene: &Scene) -> Sampled {
        let reg = registry();
        let resolved = resolve_scene(scene, &reg);
        sample(scene, &resolved, &sims(), &AtomicBool::new(false)).unwrap()
    }

    fn within_unit(parts: &Parts) {
        for (name, value, _) in parts.list() {
            assert!((0.0..=1.0).contains(&value), "{name} = {value}");
        }
    }

    #[test]
    fn a_still_scene_scores_near_zero_and_a_lively_one_higher() {
        let reg = registry();
        let mut lively = new_scene(&reg);
        short(&reg, &mut lively);
        let mut still = lively.clone();
        let balls = still.things.values().find(|t| t.kind == "balls").map(|t| t.id).unwrap();
        still.remove_thing(balls).unwrap();
        let ValueType::Choice(names) =
            reg.thing("balls").unwrap().settings().iter().find(|d| d.key == "arrangement").unwrap().ty
        else {
            panic!()
        };
        let block = names.iter().find(|n| n.eq_ignore_ascii_case("block")).unwrap();
        lively.set_at(&reg, &own(balls, "arrangement"), Value::Choice((*block).into())).unwrap();
        lively.set_at(&reg, &own(balls, "columns"), Value::Int(8)).unwrap();
        lively.set_at(&reg, &own(balls, "rows"), Value::Int(5)).unwrap();
        lively.set_at(&reg, &own(balls, "spacing"), Value::Number(30.0)).unwrap();

        let quiet = sampled(&still);
        assert!(quiet.hits.is_empty());
        assert_eq!(quiet.frames.len(), 60, "a sample every 0.1 s for 6 s");
        let quiet_score = score(&quiet, &Parts::WEIGHTS);
        within_unit(&quiet_score.parts);
        assert!(quiet_score.total < 0.25, "{quiet_score:?}");

        let busy = sampled(&lively);
        assert!(busy.hits.len() > 20, "{} hits", busy.hits.len());
        assert!(busy.frames.iter().all(|f| f.count == 40 && f.on_screen <= 40));
        assert!(busy.frames.iter().any(|f| f.fastest > 100.0 && f.average > 0.0));
        let busy_score = score(&busy, &Parts::WEIGHTS);
        within_unit(&busy_score.parts);
        assert!(busy_score.total > quiet_score.total + 0.2, "{busy_score:?} vs {quiet_score:?}");
        assert!(busy_score.parts.motion > 0.5, "{busy_score:?}");
    }

    fn hand_sampled(hits: Vec<(f64, f32)>, beats: Vec<f64>) -> Sampled {
        let frames = (1..=100)
            .map(|i| Sample { time: i as f64 * 0.1, count: 10, on_screen: 10, fastest: 400.0, average: 250.0 })
            .collect();
        Sampled { duration: 10.0, hits, frames, beats, quietest: 20.0, loudest: 600.0 }
    }

    #[test]
    fn balls_out_of_the_picture_sink_the_whole_score() {
        // A spinning tall box carries its balls out of the 9:16 frame: all of them gone for four
        // seconds, and a third of them gone for the rest. The hits still sound good, but nobody
        // keeps a clip they can't see.
        let hits: Vec<(f64, f32)> = (0..40).map(|i| (i as f64 * 0.25 + 0.05, 150.0 + (i % 5) as f32 * 80.0)).collect();
        let seen = hand_sampled(hits.clone(), vec![]);
        let mut lost = hand_sampled(hits, vec![]);
        for (i, f) in lost.frames.iter_mut().enumerate() {
            f.on_screen = if (30..70).contains(&i) { 0 } else { 7 };
        }
        let (seen, lost) = (score(&seen, &Parts::WEIGHTS), score(&lost, &Parts::WEIGHTS));
        assert!(lost.total < seen.total * 0.6, "{lost:?} vs {seen:?}");
        // A ball slipping out for a moment costs little.
        let mut blip =
            hand_sampled((0..40).map(|i| (i as f64 * 0.25 + 0.05, 150.0 + (i % 5) as f32 * 80.0)).collect(), vec![]);
        blip.frames[50].on_screen = 9;
        assert!(score(&blip, &Parts::WEIGHTS).total > seen.total * 0.97);
    }

    #[test]
    fn rhythm_rewards_hits_on_the_beat() {
        let beats: Vec<f64> = (0..20).map(|i| i as f64 * 0.5).collect();
        let on: Vec<(f64, f32)> = (0..20).map(|i| (i as f64 * 0.5 + 0.01, 300.0)).collect();
        let off: Vec<(f64, f32)> = (0..20).map(|i| (i as f64 * 0.5 + 0.23, 300.0)).collect();
        let on_grid = score(&hand_sampled(on.clone(), beats.clone()), &Parts::WEIGHTS);
        let off_grid = score(&hand_sampled(off, beats), &Parts::WEIGHTS);
        assert!(on_grid.parts.rhythm > off_grid.parts.rhythm + 0.3, "{on_grid:?} vs {off_grid:?}");
        let no_beats = score(&hand_sampled(on, Vec::new()), &Parts::WEIGHTS);
        assert!(no_beats.parts.rhythm > 0.5, "a steady pace scores well on its own: {no_beats:?}");
        assert!(near_beat(&[1.0, 2.0], 1.04) && !near_beat(&[1.0, 2.0], 1.5));
    }

    #[test]
    fn build_up_and_climax_like_an_ending_that_grows() {
        let growing: Vec<(f64, f32)> =
            (0..200).map(|i| (10.0 - 10.0 * (1.0 - i as f64 / 200.0).powi(2), 300.0)).collect();
        let fading: Vec<(f64, f32)> = growing.iter().map(|(t, s)| (10.0 - t, *s)).collect();
        let grows = score(&hand_sampled(growing, Vec::new()), &Parts::WEIGHTS);
        let fades = score(&hand_sampled(fading, Vec::new()), &Parts::WEIGHTS);
        assert!(grows.parts.build_up > fades.parts.build_up + 0.3, "{grows:?} vs {fades:?}");
        assert!(grows.parts.climax > fades.parts.climax + 0.3, "{grows:?} vs {fades:?}");
    }

    #[test]
    fn learning_moves_weights_toward_what_was_liked_and_clamps() {
        let mut weights = Parts::WEIGHTS;
        let dramatic = Parts { rhythm: 0.5, build_up: 0.5, climax: 0.95, motion: 0.5, variety: 0.5, sound: 0.5 };
        learn(&mut weights, &dramatic, true);
        assert!(weights.climax > Parts::WEIGHTS.climax, "{weights:?}");
        assert!((weights.rhythm - Parts::WEIGHTS.rhythm).abs() < 1e-6, "parts in the middle teach nothing");
        for _ in 0..100 {
            learn(&mut weights, &dramatic, true);
        }
        assert!(weights.climax <= 3.0);
        let mut weights = Parts::WEIGHTS;
        for _ in 0..100 {
            learn(&mut weights, &dramatic, false);
        }
        assert!(weights.climax >= 0.05 && weights.climax < Parts::WEIGHTS.climax);
        let total = dramatic.total(&Parts::WEIGHTS);
        assert!((0.0..=1.0).contains(&total));
        let text = ron::to_string(&weights).unwrap();
        assert_eq!(ron::from_str::<Parts>(&text).unwrap(), weights);
    }

    #[test]
    fn sampling_stops_when_asked() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        short(&reg, &mut scene);
        let resolved = resolve_scene(&scene, &reg);
        let stop = AtomicBool::new(true);
        assert!(sample(&scene, &resolved, &sims(), &stop).is_none());
    }
}
