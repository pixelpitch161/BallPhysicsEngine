//! The DAHDSR envelope: Delay, Attack, Hold, Decay, Sustain and Release, each moving segment
//! bent by its own curve. The gate (when Release starts) is the voice's business.

use crate::BLOCK;

/// An envelope's times (seconds), sustain (0..1) and curves (-1..1, 0 straight) for the attack,
/// decay and release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvParams {
    pub delay: f32,
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub curves: [f32; 3],
}

impl Default for EnvParams {
    fn default() -> Self {
        Self { delay: 0.0, attack: 0.002, hold: 0.0, decay: 0.35, sustain: 0.0, release: 0.2, curves: [0.0; 3] }
    }
}

/// Where an envelope is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    /// Fading out fast because its voice was taken for a new note.
    Steal,
}

/// A linear 0..1 segment position bent by curve `c` (-1..1): `t^(2^(-3c))`, so 0 is straight
/// and the end points stay put.
#[inline]
pub fn bend(t: f32, c: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if c.abs() < 1e-4 {
        return t;
    }
    t.powf((-3.0 * c.clamp(-1.0, 1.0)).exp2())
}

/// One envelope.
#[derive(Clone, Debug)]
pub struct Envelope {
    params: EnvParams,
    stage: Stage,
    /// Samples into the stage.
    pos: u32,
    /// Samples the stage lasts.
    len: u32,
    level: f32,
    /// Where the stage started from.
    from: f32,
    /// The top (scaled by velocity).
    peak: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

fn samples(seconds: f32) -> u32 {
    (seconds.max(0.0) * crate::SAMPLE_RATE).round() as u32
}

impl Envelope {
    pub fn new() -> Self {
        Self { params: EnvParams::default(), stage: Stage::Idle, pos: 0, len: 0, level: 0.0, from: 0.0, peak: 1.0 }
    }

    /// New times and levels; the stage running now keeps its length.
    pub fn set(&mut self, p: EnvParams) {
        self.params = p;
    }

    pub fn params(&self) -> &EnvParams {
        &self.params
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    /// Whether it has finished (silent, and nothing more to come).
    pub fn is_done(&self) -> bool {
        match self.stage {
            Stage::Idle => true,
            Stage::Sustain => self.level <= 0.0 && self.params.sustain <= 0.0,
            _ => false,
        }
    }

    fn enter(&mut self, stage: Stage) {
        self.stage = stage;
        self.pos = 0;
        self.from = self.level;
        let p = &self.params;
        self.len = match stage {
            Stage::Delay => samples(p.delay),
            Stage::Attack => samples(p.attack),
            Stage::Hold => samples(p.hold),
            Stage::Decay => samples(p.decay),
            Stage::Release => samples(p.release),
            Stage::Idle | Stage::Sustain | Stage::Steal => self.len,
        };
    }

    /// Starts a note at `velocity_scale` (the top level, 0..1). A note already sounding rises
    /// again from where it is, so nothing clicks.
    pub fn trigger(&mut self, velocity_scale: f32) {
        self.peak = velocity_scale.clamp(0.0, 1.0);
        self.enter(Stage::Delay);
    }

    /// The gate closed: Release from where it is (an envelope already done stays done).
    pub fn release(&mut self) {
        if !matches!(self.stage, Stage::Idle | Stage::Release | Stage::Steal) {
            self.enter(Stage::Release);
        }
    }

    /// Fades out over `fade_ms`, then stops.
    pub fn steal(&mut self, fade_ms: f32) {
        if self.stage == Stage::Idle {
            return;
        }
        self.enter(Stage::Steal);
        self.len = samples(fade_ms / 1000.0).max(1);
    }

    /// Silent and stopped, as new.
    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
        self.pos = 0;
        self.len = 0;
    }

    /// The next sample's level.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        loop {
            // Stages that last no time are passed straight through.
            if self.pos < self.len || matches!(self.stage, Stage::Idle | Stage::Sustain) {
                break;
            }
            match self.stage {
                Stage::Delay => self.enter(Stage::Attack),
                Stage::Attack => {
                    self.level = self.peak;
                    self.enter(Stage::Hold)
                }
                Stage::Hold => self.enter(Stage::Decay),
                Stage::Decay => {
                    self.level = self.params.sustain * self.peak;
                    self.enter(Stage::Sustain)
                }
                Stage::Release | Stage::Steal => {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
                Stage::Idle | Stage::Sustain => unreachable!(),
            }
        }
        let t = if self.len == 0 { 1.0 } else { (self.pos + 1) as f32 / self.len as f32 };
        let c = &self.params.curves;
        self.level = match self.stage {
            Stage::Idle => 0.0,
            Stage::Delay => self.from,
            Stage::Attack => self.from + (self.peak - self.from) * bend(t, c[0]),
            Stage::Hold => self.peak,
            Stage::Decay => {
                let s = self.params.sustain * self.peak;
                s + (self.peak - s) * (1.0 - bend(t, c[1]))
            }
            Stage::Sustain => self.params.sustain * self.peak,
            Stage::Release => self.from * (1.0 - bend(t, c[2])),
            Stage::Steal => self.from * (1.0 - t),
        };
        if !matches!(self.stage, Stage::Idle | Stage::Sustain) {
            self.pos += 1;
        }
        self.level
    }

    /// The next block of levels.
    pub fn next_block(&mut self, out: &mut [f32; BLOCK]) {
        for v in out.iter_mut() {
            *v = self.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: f32) -> f32 {
        n / 1000.0
    }

    fn params() -> EnvParams {
        EnvParams {
            delay: ms(10.0),
            attack: ms(5.0),
            hold: ms(20.0),
            decay: ms(50.0),
            sustain: 0.5,
            release: ms(30.0),
            curves: [0.0; 3],
        }
    }

    /// The stage of every sample from a trigger.
    fn stages(env: &mut Envelope, n: usize) -> Vec<(Stage, f32)> {
        (0..n)
            .map(|_| {
                let v = env.next();
                (env.stage(), v)
            })
            .collect()
    }

    fn first(s: &[(Stage, f32)], stage: Stage) -> usize {
        s.iter().position(|(st, _)| *st == stage).unwrap()
    }

    #[test]
    fn stages_take_their_set_times() {
        let mut env = Envelope::new();
        env.set(params());
        env.trigger(1.0);
        let s = stages(&mut env, 10_000);
        let at = |n: f32| (n * 48.0) as i64;
        let close = |a: usize, b: i64| (a as i64 - b).abs() <= 1;
        assert!(close(first(&s, Stage::Attack), at(10.0)), "{}", first(&s, Stage::Attack));
        assert!(close(first(&s, Stage::Hold), at(15.0)));
        assert!(close(first(&s, Stage::Decay), at(35.0)));
        assert!(close(first(&s, Stage::Sustain), at(85.0)));
        assert!(s[..at(10.0) as usize].iter().all(|(_, v)| *v == 0.0), "silent through the delay");
        assert!((s[at(20.0) as usize].1 - 1.0).abs() < 1e-6, "at the top in the hold");
        assert!((s[9_000].1 - 0.5).abs() < 1e-6, "at sustain");
        env.release();
        let r = stages(&mut env, 2_000);
        assert!(close(first(&r, Stage::Idle), at(30.0)), "{}", first(&r, Stage::Idle));
        assert!(env.is_done());
    }

    #[test]
    fn retrigger_starts_from_the_current_level() {
        let mut env = Envelope::new();
        env.set(EnvParams { delay: 0.0, ..params() });
        env.trigger(1.0);
        stages(&mut env, (48.0 * 40.0) as usize);
        let before = env.level();
        assert!(before > 0.5);
        env.trigger(1.0);
        let next = env.next();
        assert!((next - before).abs() < 0.25, "no jump: {before} to {next}");
        // A retrigger with a delay holds where it is through the delay.
        env.set(params());
        env.trigger(1.0);
        let held = env.next();
        assert!((held - next).abs() < 1e-6);
    }

    #[test]
    fn steal_fades_in_5ms() {
        let mut env = Envelope::new();
        env.set(EnvParams { delay: 0.0, sustain: 1.0, ..params() });
        env.trigger(1.0);
        stages(&mut env, 4_800);
        env.steal(5.0);
        let s = stages(&mut env, 1_000);
        let end = first(&s, Stage::Idle);
        assert!((end as i64 - 240).abs() <= 1, "{end}");
        let worst = s.windows(2).map(|w| (w[0].1 - w[1].1).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1.0 / 200.0, "an even fade: {worst}");
    }

    #[test]
    fn curves_bend_but_keep_end_points() {
        for c in [-1.0, -0.4, 0.0, 0.4, 1.0] {
            assert_eq!(bend(0.0, c), 0.0);
            assert!((bend(1.0, c) - 1.0).abs() < 1e-6);
        }
        assert!(bend(0.5, 0.4) > 0.5 && bend(0.5, -0.4) < 0.5);
        let mut env = Envelope::new();
        env.set(EnvParams { delay: 0.0, hold: 0.0, curves: [0.0, 0.6, 0.0], ..params() });
        env.trigger(1.0);
        let s = stages(&mut env, 48 * 60);
        let d = first(&s, Stage::Decay);
        let mid = s[d + 48 * 25].1;
        assert!(mid < 0.75, "a bent decay falls faster first: {mid}");
        let end = first(&s, Stage::Sustain);
        assert!((s[end].1 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn sustain_zero_ends_without_release_tail() {
        let mut env = Envelope::new();
        env.set(EnvParams { sustain: 0.0, ..params() });
        env.trigger(0.8);
        stages(&mut env, 48 * 90);
        assert!(env.is_done(), "done after the decay with nothing held");
        // Closing the gate early releases from where it is.
        env.trigger(0.8);
        stages(&mut env, 48 * 40);
        env.release();
        assert_eq!(env.stage(), Stage::Release);
        stages(&mut env, 48 * 31);
        assert!(env.is_done());
    }
}
