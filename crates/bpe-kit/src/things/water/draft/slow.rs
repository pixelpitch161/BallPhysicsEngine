//! Slow motion (part 8): the Water card's Speed, read along the water's own time. A water second
//! at Speed `s` plays for `1 / s` seconds of video, so the scene's timeline (its keyframes, its
//! pours, its Length) stays the water's and the video is `∫ dτ / s(τ)` long. Where the water plays
//! slower than real time the final keeps more moments a frame (2, 4 or 8), so it stays smooth.
//!
//! Un-mix (part 4.4) is a stretch of the same clock: when the video reaches the Water card's To, it
//! plays the water backwards to From at the Speed there, then forwards from From again, and on to
//! the end. The water is worked out once, as without it; the video shows its moments twice more.

use bpe_core::{Resolved, Scene, SlotId, ThingId};

use super::Keyed;
use crate::modules::water::WaterWorld;

/// The slowest and fastest Speed.
pub const SLOWEST: f32 = 0.125;
pub const FASTEST: f32 = 4.0;
/// Water seconds between the times the video's clock is worked out at.
const STEP: f64 = 1.0 / 480.0;

/// A Water scene's Speed along its water time.
#[derive(Clone, Debug)]
pub struct Speeds {
    world: Keyed,
    keyed: bool,
    /// The video's time (seconds) at each `STEP` of water time, from 0 to the length asked for.
    table: Vec<f64>,
    /// The moments each water frame keeps ([`Speeds::keep_for`]), to the length asked for (one
    /// for every frame when Speed is not keyframed).
    keeps: Vec<u32>,
    /// The Un-mix stretch, From and To (water seconds), when it is on and lies inside the length.
    unmix: Option<(f64, f64)>,
}

impl Speeds {
    /// The scene's Speed, its video clock worked out for `length` water seconds.
    pub fn of(scene: &Scene, resolved: &Resolved, length: f64) -> Speeds {
        let world = Keyed::of(scene, resolved, ThingId::ROOT, SlotId::Water).unwrap_or_default();
        let keyed = world.tracks.iter().any(|(k, _)| k == "speed");
        let card = world.card::<WaterWorld>(0.0);
        let (from, to) = (f64::from(card.unmix_from), f64::from(card.unmix_to));
        let fits = from.is_finite() && to.is_finite() && from >= 0.0 && from < to && to <= length + 1e-9;
        let unmix = (card.unmix && fits).then_some((from, to));
        let mut speeds = Speeds { world, keyed, table: Vec::new(), keeps: Vec::new(), unmix };
        speeds.table_to(length);
        speeds
    }

    /// Real time throughout (a scene with no Water card, and the tests).
    pub fn real_time(length: f64) -> Speeds {
        let mut speeds =
            Speeds { world: Keyed::default(), keyed: false, table: Vec::new(), keeps: Vec::new(), unmix: None };
        speeds.table_to(length);
        speeds
    }

    fn table_to(&mut self, length: f64) {
        if !self.keyed {
            // (One Speed throughout: the clock is worked out exactly, with no table.)
            self.table = vec![0.0];
            self.keeps = vec![self.keep_at(0)];
            return;
        }
        let frames = (length.max(0.0) * 60.0).ceil() as u32;
        self.keeps = (0..frames).map(|k| self.keep_at(k)).collect();
        let n = (length.max(0.0) / STEP).ceil() as usize + 2;
        let mut table = Vec::with_capacity(n);
        let mut t = 0.0;
        table.push(0.0);
        for k in 1..n {
            // (The middle of the stretch: a ramp's video time is its speed's mean over it.)
            let s = f64::from(self.speed_at((k as f64 - 0.5) * STEP));
            t += STEP / s;
            table.push(t);
        }
        self.table = table;
    }

    /// Whether Speed is keyframed (or not 1).
    pub fn slowed(&self) -> bool {
        self.keyed || (self.speed_at(0.0) - 1.0).abs() > 1e-6
    }

    /// The Un-mix stretch the video plays backwards, From and To (water seconds), if any.
    pub fn unmix(&self) -> Option<(f64, f64)> {
        self.unmix
    }

    /// The same clock with no Un-mix (the draft, which plays forwards only).
    pub fn without_unmix(&self) -> Speeds {
        Speeds { unmix: None, ..self.clone() }
    }

    /// How the video's length differs from the water's, in words ("in slow motion", "with Un-mix",
    /// both), or nothing.
    pub fn longer_words(&self) -> Option<&'static str> {
        match (self.slowed(), self.unmix.is_some()) {
            (true, true) => Some("in slow motion, with Un-mix"),
            (true, false) => Some("in slow motion"),
            (false, true) => Some("with Un-mix"),
            (false, false) => None,
        }
    }

    /// The Un-mix stretch's ends on the video's clock without it (seconds): where To is, and how
    /// long the stretch plays.
    fn stretch(&self) -> Option<(f64, f64)> {
        let (from, to) = self.unmix?;
        let end = self.slow_video_time(to);
        Some((end, end - self.slow_video_time(from)))
    }

    /// The Speed at water time `tau` seconds.
    pub fn speed_at(&self, tau: f64) -> f32 {
        let s = if self.world.params.0.is_empty() && self.world.tracks.is_empty() {
            1.0
        } else {
            self.world.card::<WaterWorld>(tau).speed
        };
        if s.is_finite() { s.clamp(SLOWEST, FASTEST) } else { 1.0 }
    }

    /// The video's time (seconds) at which it first shows water time `tau` (past Un-mix's To, after
    /// the stretch has played backwards and forwards again).
    pub fn video_time(&self, tau: f64) -> f64 {
        let t = self.slow_video_time(tau);
        match (self.unmix, self.stretch()) {
            (Some((_, to)), Some((_, long))) if tau > to => t + 2.0 * long,
            _ => t,
        }
    }

    /// The video's time (seconds) at water time `tau`, slow motion's alone.
    fn slow_video_time(&self, tau: f64) -> f64 {
        let tau = tau.max(0.0);
        if !self.keyed {
            // (One Speed throughout: exactly, so a Length's frames come out as a Balls scene's do.)
            return tau / f64::from(self.speed_at(0.0));
        }
        let at = tau / STEP;
        let k = at.floor() as usize;
        if k + 1 >= self.table.len() {
            let last = self.table.len().saturating_sub(1);
            let past = tau - last as f64 * STEP;
            return self.table.get(last).copied().unwrap_or(0.0) + past / f64::from(self.speed_at(tau));
        }
        let f = at - k as f64;
        self.table[k] + (self.table[k + 1] - self.table[k]) * f
    }

    /// How long the video of `length` water seconds is.
    pub fn video_length(&self, length: f64) -> f64 {
        self.video_time(length)
    }

    /// The water time (seconds) the video shows at video time `t`: going backwards over Un-mix's
    /// stretch once it reaches To, then forwards again.
    pub fn water_time(&self, t: f64) -> f64 {
        let t = t.max(0.0);
        let Some((end, long)) = self.stretch() else {
            return self.slow_water_time(t);
        };
        let slow = if t <= end {
            t
        } else if t <= end + long {
            end - (t - end)
        } else if t <= end + 2.0 * long {
            (end - long) + (t - end - long)
        } else {
            t - 2.0 * long
        };
        self.slow_water_time(slow)
    }

    /// The water time (seconds) at video time `t`, slow motion's alone.
    fn slow_water_time(&self, t: f64) -> f64 {
        let t = t.max(0.0);
        if !self.keyed {
            return t * f64::from(self.speed_at(0.0));
        }
        let k = self.table.partition_point(|v| *v <= t);
        if k == 0 {
            return 0.0;
        }
        if k >= self.table.len() {
            let last = self.table.len() - 1;
            let past = t - self.table[last];
            return last as f64 * STEP + past * f64::from(self.speed_at(last as f64 * STEP));
        }
        let (a, b) = (self.table[k - 1], self.table[k]);
        let f = if b > a { (t - a) / (b - a) } else { 0.0 };
        (k as f64 - 1.0 + f) * STEP
    }

    /// How many moments water frame `frame` keeps: 1, 2, 4 or 8, enough that the video shows a new
    /// one every frame (or every frame and a quarter: a moment drawn a little after it is kept moves
    /// its drops on) where the water plays slowest in it.
    pub fn keep_for(&self, frame: u32) -> u32 {
        match (self.keyed, self.keeps.get(frame as usize)) {
            (false, _) => self.keeps.first().copied().unwrap_or(1),
            (true, Some(keep)) => *keep,
            (true, None) => self.keep_at(frame),
        }
    }

    fn keep_at(&self, frame: u32) -> u32 {
        let start = f64::from(frame) / 60.0;
        let least = [0.0, 0.5, 1.0].iter().map(|f| self.speed_at(start + f / 60.0)).fold(f32::MAX, f32::min);
        let want = 0.8 / least.max(SLOWEST);
        [1u32, 2, 4, 8].into_iter().find(|k| *k as f32 >= want - 1e-4).unwrap_or(8)
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// A constant Speed (the finals' tests).
    pub(crate) fn constant(speed: f32, length: f64) -> Speeds {
        let mut world = Keyed::default();
        world.params.set("speed", bpe_core::Value::Number(f64::from(speed)));
        let mut speeds = Speeds { world, keyed: false, table: Vec::new(), keeps: Vec::new(), unmix: None };
        speeds.table_to(length);
        speeds
    }
}
