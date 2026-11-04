//! One bus's effects, run in order on its stereo sound once a block. Each effect's Mix
//! crossfades its dry and wet sound (equal gain for the tone and level effects, equal power for
//! the time effects), and its on switch fades it in and out so bypassing never clicks. Every
//! number setting may be keyed on the timeline; keys are read once a block.

use crate::hits::BlockHit;
use crate::lag::Lag;
use crate::voices::knobs::track_value;
use crate::{BLOCK, Meters, SAMPLE_RATE, TempoMap};
use bpe_core::Settings;
use bpe_core::{Address, LayerId, Params, Scene, SettingDef, SlotId, ThingId, Track, Value, layered_key};
use bpe_dsp::{Smoother, TruePeakLimiter, db_to_gain, gain_to_db};
use bpe_kit::sound::fx::{self, LimiterSettings};
use bpe_kit::sound::{BUSES, MIXER};

/// What an effect can read while it runs one block.
pub struct FxCx<'a> {
    /// The block's start in seconds.
    pub time: f64,
    pub tempo: &'a TempoMap,
    /// Every hit in the block (a Hits sidechain picks out its shape's).
    pub hits: &'a [BlockHit],
    /// Each bus's output from its last block, for a Bus sidechain.
    pub buses: &'a [[[f32; BLOCK]; 2]; BUSES],
    pub meters: &'a Meters,
}

/// The number settings of one effect that have keys, read once a block.
#[derive(Clone, Default)]
pub struct Keyed {
    keys: Vec<(&'static str, Track)>,
    now: Vec<f32>,
}

impl Keyed {
    /// The keyed settings of layer `id` (of the mixer card) among `defs`.
    fn of(scene: &Scene, id: LayerId, defs: &'static [SettingDef]) -> Keyed {
        let keys: Vec<(&'static str, Track)> = defs
            .iter()
            .filter(|d| d.flags.keyframe)
            .filter_map(|d| {
                let a = Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key: layered_key(id, d.key) };
                scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).map(|t| (d.key, t.clone()))
            })
            .collect();
        let now = vec![f32::NAN; keys.len()];
        Keyed { keys, now }
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Reads every key at `time`. Never allocates.
    fn at(&mut self, time: f64) {
        for (i, (_, t)) in self.keys.iter().enumerate() {
            self.now[i] = track_value(t, time).unwrap_or(f32::NAN);
        }
    }

    /// `key`'s keyed value now, or `base` when it has no keys.
    pub fn get(&self, key: &str, base: f32) -> f32 {
        match self.keys.iter().position(|(k, _)| *k == key) {
            Some(i) if self.now[i].is_finite() => self.now[i],
            _ => base,
        }
    }
}

/// How an effect's Mix crossfades.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MixLaw {
    /// Dry and wet add to one (tone and level effects, where both are the same sound).
    Gain,
    /// Dry and wet keep their power (time effects, whose wet is a different sound).
    Power,
}

/// One effect's sound-making part.
enum Fx {
    Limiter { dsp: Box<TruePeakLimiter>, set: LimiterSettings },
}

impl Fx {
    /// The effect for a layer of `kind` with these settings, or `None` for a kind this build
    /// doesn't know (it is passed over).
    fn build(kind: &str, p: &Params) -> Option<(Fx, MixLaw)> {
        Some(match kind {
            fx::LIMITER => {
                let set = LimiterSettings::from_params(p);
                (Fx::Limiter { dsp: Box::new(TruePeakLimiter::new()), set }, MixLaw::Gain)
            }
            _ => return None,
        })
    }

    /// Frames it delays the sound by.
    fn latency(&self) -> usize {
        match self {
            Fx::Limiter { dsp, .. } => dsp.latency(),
        }
    }

    fn reset(&mut self) {
        match self {
            Fx::Limiter { dsp, .. } => dsp.reset(),
        }
    }

    /// Takes over `old`'s memory when it is the same kind of effect (a patch swap).
    fn adopt(&mut self, old: &mut Fx) {
        match (self, old) {
            (Fx::Limiter { dsp, .. }, Fx::Limiter { dsp: o, .. }) => std::mem::swap(dsp, o),
        }
    }

    /// Hands its settings, keyed where they have keys, to the sound-making part.
    fn apply(&mut self, keyed: &Keyed, _tempo: &TempoMap) {
        match self {
            Fx::Limiter { dsp, set } => {
                dsp.set(keyed.get("ceiling", set.ceiling), keyed.get("release", set.release));
            }
        }
    }

    /// Runs one block in place (wet only); returns its gain reduction (dB, 0 or less) for the
    /// bus's meter.
    fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK], _cx: &FxCx, id: LayerId) -> f32 {
        match self {
            Fx::Limiter { dsp, .. } => {
                dsp.process(l, r);
                let gr = dsp.gain_reduction_db();
                let _ = id;
                gr
            }
        }
    }
}

/// One effect in the chain.
struct Slot {
    id: LayerId,
    kind: String,
    fx: Fx,
    law: MixLaw,
    keyed: Keyed,
    /// Mix, 0..1, as set.
    mix_base: f32,
    mix: Smoother,
    /// 1 while on, 0 while bypassed.
    on: Smoother,
    /// It was bypassed and silent: its memory is cleared before it comes back.
    asleep: bool,
    dry: [[f32; BLOCK]; 2],
    /// Lines the dry sound up with the wet when the effect delays it.
    dry_lag: Lag,
}

/// A bus's effects in order.
#[derive(Default)]
pub struct FxChain {
    slots: Vec<Slot>,
    latency: usize,
}

/// How long switching an effect on or off fades.
const BYPASS_MS: f32 = 10.0;

impl FxChain {
    /// The chain of bus `lane` (0..2) from the scene's mixer card's effect layers, in their
    /// order, bypassed ones included (they fade in and out when switched).
    pub fn build(scene: &Scene, lane: u8, registry: &bpe_core::Registry) -> FxChain {
        let mut slots = Vec::new();
        let Some(card) = scene.card(ThingId::ROOT, SlotId::Mixer).filter(|c| c.kind == MIXER) else {
            return FxChain::default();
        };
        for layer in card.layers.iter().filter(|l| fx::is_fx(&l.kind) && l.lane == Some(lane)) {
            let Some(kind) = registry.layer(&layer.kind).filter(|k| k.card() == MIXER) else { continue };
            let defs = kind.settings();
            let mut p = Params(defs.iter().map(|d| (d.key.to_owned(), d.default.clone())).collect());
            for d in defs {
                if let Some(v) = card.params.get(&layered_key(layer.id, d.key)) {
                    p.set(d.key, v.clone());
                }
            }
            let Some((fx, law)) = Fx::build(&layer.kind, &p) else { continue };
            let mix_base = match p.get("mix") {
                Some(Value::Number(m)) => (*m as f32 / 100.0).clamp(0.0, 1.0),
                _ => 1.0,
            };
            let mut mix = Smoother::new(Smoother::KNOB_MS);
            mix.reset(mix_base);
            let mut on = Smoother::new(BYPASS_MS);
            on.reset(if layer.enabled { 1.0 } else { 0.0 });
            let mut dry_lag = Lag::new();
            dry_lag.set(fx.latency());
            slots.push(Slot {
                id: layer.id,
                kind: layer.kind.clone(),
                fx,
                law,
                keyed: Keyed::of(scene, layer.id, defs),
                mix_base,
                mix,
                on,
                asleep: !layer.enabled,
                dry: [[0.0; BLOCK]; 2],
                dry_lag,
            });
        }
        let latency = slots.iter().map(|s| s.fx.latency()).sum();
        let mut chain = FxChain { slots, latency };
        chain.apply_all(&TempoMap::default(), 0.0);
        chain
    }

    /// Frames the chain delays the sound by.
    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The layers in it, in order.
    pub fn layers(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.slots.iter().map(|s| s.id)
    }

    fn apply_all(&mut self, tempo: &TempoMap, time: f64) {
        for s in &mut self.slots {
            s.keyed.at(time);
            s.fx.apply(&s.keyed, tempo);
            s.mix.set_target(s.keyed.get("mix", s.mix_base * 100.0) / 100.0);
        }
    }

    /// Silence, as new.
    pub fn reset(&mut self) {
        for s in &mut self.slots {
            s.fx.reset();
            s.dry_lag.reset();
            let (m, o) = (s.mix.target(), s.on.target());
            s.mix.reset(m);
            s.on.reset(o);
            s.asleep = o == 0.0;
        }
    }

    /// Takes over the memory of `old`'s effects of the same layer and kind, so tails ring on.
    pub fn adopt(&mut self, old: &mut FxChain) {
        for s in &mut self.slots {
            let Some(o) = old.slots.iter_mut().find(|o| o.id == s.id && o.kind == s.kind) else { continue };
            s.fx.adopt(&mut o.fx);
            if s.fx.latency() == o.fx.latency() {
                std::mem::swap(&mut s.dry_lag, &mut o.dry_lag);
            }
            let (m, on) = (s.mix.target(), s.on.target());
            s.mix.reset(o.mix.value());
            s.mix.set_target(m);
            s.on.reset(o.on.value());
            s.on.set_target(on);
            s.asleep = o.asleep;
            // Settings may have changed; the adopted part takes them on.
            s.keyed.at(0.0);
        }
    }

    /// Runs one block of the bus's sound in place; returns the most any effect turned it down
    /// (dB, 0 or less). Never allocates.
    pub fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK], cx: &FxCx) -> f32 {
        let mut reduction = 0.0f32;
        for s in &mut self.slots {
            if !s.keyed.is_empty() {
                s.keyed.at(cx.time);
                s.fx.apply(&s.keyed, cx.tempo);
                s.mix.set_target(s.keyed.get("mix", s.mix_base * 100.0) / 100.0);
            }
            if s.on.target() == 0.0 && s.on.settled() {
                // Bypassed: the sound passes, delayed as the effect would have (so the bus's
                // timing never changes), and the effect rests.
                s.asleep = true;
                s.dry_lag.process(l, r);
                continue;
            }
            if s.asleep {
                s.fx.reset();
                s.asleep = false;
            }
            s.dry[0] = *l;
            s.dry[1] = *r;
            let [dl, dr] = &mut s.dry;
            s.dry_lag.process(dl, dr);
            let gr = s.fx.process(l, r, cx, s.id);
            reduction = reduction.min(gr);
            for i in 0..BLOCK {
                let m = s.mix.next();
                let on = s.on.next();
                let w = m * on;
                let (gd, gw) = match s.law {
                    MixLaw::Gain => (1.0 - w, w),
                    MixLaw::Power => {
                        let a = w * std::f32::consts::FRAC_PI_2;
                        // The dry fades out only as far as the effect is on.
                        (1.0 - on + on * a.cos(), a.sin())
                    }
                };
                l[i] = s.dry[0][i] * gd + l[i] * gw;
                r[i] = s.dry[1][i] * gd + r[i] * gw;
            }
        }
        let _ = (gain_to_db(1.0), db_to_gain(0.0), SAMPLE_RATE);
        reduction
    }
}
