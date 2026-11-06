//! The Light modulator (lighting part 7): how much light falls on the balls, or how much of it
//! their shadows take, from Dark (nothing) to Bright (full). Read from the scene's light probe
//! once a physics step (the world is at most a step ahead of the block) and smoothed. Without a
//! probe (no Lighting card) it reads nothing.

use super::{BLOCK_S, ModCx, Modulator, NoteCx, bend};
use bpe_core::{Target, ThingId};
use bpe_dsp::lfo::smooth_coefficient;
use bpe_kit::sound::{LightFollows, LightModSettings, LightReads};
use bpe_runtime::{LightAt, Log, Party, World};
use std::sync::Arc;

/// A Light's settings for one block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightParams {
    pub reads: LightReads,
    pub follows: LightFollows,
    pub dark: f32,
    pub bright: f32,
    /// -1..1.
    pub curve: f32,
    pub smooth_ms: f32,
    pub invert: bool,
    pub lit_at: f32,
    pub full_at: f32,
}

impl Default for LightParams {
    fn default() -> Self {
        Self::of(&LightModSettings::default())
    }
}

impl LightParams {
    pub fn of(s: &LightModSettings) -> Self {
        LightParams {
            reads: s.reads,
            follows: s.follows,
            dark: s.dark,
            bright: s.bright,
            curve: s.curve / 100.0,
            smooth_ms: s.smooth,
            invert: s.invert,
            lit_at: s.lit_at,
            full_at: s.full_at as f32,
        }
    }
}

/// `v` from Dark (0) to Bright (1), clamped. Dark and Bright the same: a step there.
fn between(v: f32, dark: f32, bright: f32) -> f32 {
    if (bright - dark).abs() < 1e-6 {
        return if v >= dark { 1.0 } else { 0.0 };
    }
    ((v - dark) / (bright - dark)).clamp(0.0, 1.0)
}

/// The Light's value now, before smoothing: `None` when it follows one ball that is gone (the
/// value then holds). `ball` is the note's own ball for Each note's own ball.
#[allow(clippy::too_many_arguments)]
pub fn light_value(
    p: &LightParams,
    heard_from: &Target,
    light: &dyn LightAt,
    world: &World,
    log: &Log,
    shape: ThingId,
    ball: Option<Party>,
) -> Option<f32> {
    let time = world.time();
    let read = |thing: ThingId, row: usize| {
        let b = light.ball(world, log, thing, row, time);
        match p.reads {
            LightReads::Light => b.light,
            LightReads::Shadow => b.shadow,
        }
    };
    let raw = match p.follows {
        LightFollows::EachNotesBall => {
            let ball = ball?;
            let b = world.bodies.get(&ball.thing)?;
            let i = ball.item as usize;
            if i >= b.position.len() || !b.alive.get(i).copied().unwrap_or(false) {
                return None;
            }
            between(read(ball.thing, i), p.dark, p.bright)
        }
        follows => {
            let (mut most, mut least, mut sum, mut count, mut lit) = (0.0f32, 1.0f32, 0.0f64, 0usize, 0usize);
            for (thing, b) in &world.bodies {
                if !world.names(heard_from, shape, *thing) {
                    continue;
                }
                for i in 0..b.position.len() {
                    if !b.alive.get(i).copied().unwrap_or(false) {
                        continue;
                    }
                    let v = read(*thing, i);
                    most = most.max(v);
                    least = least.min(v);
                    sum += v as f64;
                    count += 1;
                    if v >= p.lit_at {
                        lit += 1;
                    }
                }
            }
            if count == 0 {
                0.0
            } else {
                match follows {
                    LightFollows::HowManyLit => (lit as f32 / p.full_at.max(1.0)).min(1.0),
                    LightFollows::BrightestBall => between(most, p.dark, p.bright),
                    LightFollows::DarkestBall => between(least, p.dark, p.bright),
                    _ => between((sum / count as f64) as f32, p.dark, p.bright),
                }
            }
        }
    };
    let v = bend(raw, p.curve);
    Some(if p.invert { 1.0 - v } else { v })
}

/// One Light (shared by the shape, or one per note for Each note's own ball).
#[derive(Clone, Debug)]
pub struct LightMod {
    p: LightParams,
    heard_from: Arc<Target>,
    ball: Option<Party>,
    value: f32,
    /// The physics step its raw value was read at, and that value: it is read once a step.
    read: Option<(u64, Option<f32>)>,
    fresh: bool,
}

impl LightMod {
    /// Cloning it later allocates nothing (the balls it hears are shared).
    pub fn new(heard_from: Arc<Target>) -> Self {
        LightMod { p: LightParams::default(), heard_from, ball: None, value: 0.0, read: None, fresh: true }
    }

    pub fn set_params(&mut self, p: LightParams) {
        if p != self.p {
            self.read = None;
        }
        self.p = p;
    }

    pub fn params(&self) -> &LightParams {
        &self.p
    }

    /// Takes over another's value (a patch swap), so it carries on smoothly.
    pub fn take_over(&mut self, old: &LightMod) {
        self.value = old.value;
        self.fresh = old.fresh;
        self.ball = old.ball;
    }
}

impl Modulator for LightMod {
    fn value(&self) -> f32 {
        self.value
    }

    fn next_block(&mut self, cx: &ModCx) {
        let Some(light) = cx.light else {
            // No Lighting card: nothing to read.
            self.value = 0.0;
            return;
        };
        let step = cx.world.step;
        let raw = match self.read {
            Some((at, raw)) if at == step => raw,
            _ => {
                let raw = light_value(&self.p, &self.heard_from, light, cx.world, cx.log, cx.shape, self.ball);
                self.read = Some((step, raw));
                raw
            }
        };
        let Some(raw) = raw else {
            // Its ball has gone: hold.
            self.fresh = false;
            return;
        };
        if std::mem::take(&mut self.fresh) {
            self.value = raw;
        } else {
            let c = smooth_coefficient(self.p.smooth_ms.max(0.0) as f64 / 1000.0, BLOCK_S);
            self.value += (raw - self.value) * c;
        }
    }

    fn note_on(&mut self, cx: &NoteCx) {
        if self.p.follows == LightFollows::EachNotesBall && cx.ball != self.ball {
            self.ball = cx.ball;
            self.read = None;
            self.fresh = true;
        }
    }

    fn reset(&mut self) {
        self.fresh = true;
        self.value = 0.0;
        self.ball = None;
        self.read = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_to_bright_maps_to_nothing_to_full() {
        assert_eq!(between(0.05, 0.1, 0.8), 0.0);
        assert_eq!(between(0.9, 0.1, 0.8), 1.0);
        assert!((between(0.45, 0.1, 0.8) - 0.5).abs() < 1e-6);
        assert_eq!(between(0.5, 0.5, 0.5), 1.0, "Dark and Bright the same: a step");
    }
}
