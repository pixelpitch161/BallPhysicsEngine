//! Every number knob of a shape's sound, by a fixed place in a list: the two oscillators, the
//! shape's own knobs, then each envelope's. The engine keeps the knobs' values in a plain array
//! it can key and modulate every block without allocating.

use crate::modmatrix::{TargetInfo, TargetTable};
use bpe_core::{LayerId, SettingDef, Settings, Value, layered_key};
use bpe_kit::sound::{
    AdsrSettings, DISTORTION, DistortionSettings, FILTER, FilterSettings, MAX_CHAIN, MODULATION, ModulationSettings,
    VoiceSettings,
};

/// Knobs of one oscillator, from its key prefix on.
pub const OSC_KEYS: [&str; 14] = [
    "warp",
    "gain",
    "level",
    "octave",
    "semi",
    "cents",
    "phase",
    "unison",
    "detune",
    "spread",
    "blend",
    "random_phase",
    "pan",
    "sub_level",
];
pub const WARP: usize = 0;
pub const GAIN: usize = 1;
pub const LEVEL: usize = 2;
pub const OCTAVE: usize = 3;
pub const SEMI: usize = 4;
pub const CENTS: usize = 5;
pub const PHASE: usize = 6;
pub const UNISON: usize = 7;
pub const DETUNE: usize = 8;
pub const SPREAD: usize = 9;
pub const BLEND: usize = 10;
pub const RANDOM_PHASE: usize = 11;
pub const PAN: usize = 12;
pub const SUB_LEVEL: usize = 13;
pub const OSC_LEN: usize = OSC_KEYS.len();

/// The shape's own knobs, after both oscillators.
pub const SHAPE_KEYS: [&str; 7] =
    ["play_voices", "play_glide", "play_min_strength", "play_retrigger", "out_volume", "out_pan", "out_follow_hit"];
pub const VOICES: usize = 2 * OSC_LEN;
pub const GLIDE: usize = VOICES + 1;
pub const MIN_STRENGTH: usize = VOICES + 2;
pub const RETRIGGER: usize = VOICES + 3;
pub const VOLUME: usize = VOICES + 4;
pub const OUT_PAN: usize = VOICES + 5;
pub const FOLLOW_HIT: usize = VOICES + 6;
pub const SHAPE_LEN: usize = VOICES + SHAPE_KEYS.len();

/// One envelope's knobs.
pub const ADSR_KEYS: [&str; 12] = [
    "delay",
    "attack",
    "hold",
    "decay",
    "sustain",
    "release",
    "velocity",
    "attack_curve",
    "decay_curve",
    "release_curve",
    "gate_ms",
    "gate_beats",
];
pub const DELAY: usize = 0;
pub const ATTACK: usize = 1;
pub const HOLD: usize = 2;
pub const DECAY: usize = 3;
pub const SUSTAIN: usize = 4;
pub const RELEASE: usize = 5;
pub const VELOCITY: usize = 6;
pub const ATTACK_CURVE: usize = 7;
pub const DECAY_CURVE: usize = 8;
pub const RELEASE_CURVE: usize = 9;
pub const GATE_MS: usize = 10;
pub const GATE_BEATS: usize = 11;
pub const ADSR_LEN: usize = ADSR_KEYS.len();

/// The most envelopes a shape's notes run (more are left silent).
pub const MAX_ENVS: usize = 8;
/// Where the chain layers' knobs start, after the envelopes'.
pub const CHAIN_START: usize = SHAPE_LEN + MAX_ENVS * ADSR_LEN;

/// One chain layer's knobs: every key any of the three kinds has, each kind using its own.
pub const CHAIN_KEYS: [&str; 14] = [
    "amount",
    "output",
    "mix",
    "cutoff",
    "reso",
    "drive",
    "key_track",
    "gain",
    "vowel",
    "shift",
    "bits",
    "rate",
    "bias",
    "tone",
];
pub const AMOUNT: usize = 0;
pub const OUTPUT: usize = 1;
pub const MIX: usize = 2;
pub const CUTOFF: usize = 3;
pub const RESO: usize = 4;
pub const DRIVE: usize = 5;
pub const KEY_TRACK: usize = 6;
pub const CHAIN_GAIN: usize = 7;
pub const VOWEL: usize = 8;
pub const SHIFT: usize = 9;
pub const BITS: usize = 10;
pub const RATE: usize = 11;
pub const BIAS: usize = 12;
pub const TONE: usize = 13;
pub const CHAIN_LEN: usize = CHAIN_KEYS.len();

/// Every knob.
pub const KNOBS: usize = CHAIN_START + MAX_CHAIN * CHAIN_LEN;

/// Oscillator `osc`'s knob `k`.
pub const fn osc(osc: usize, k: usize) -> usize {
    osc * OSC_LEN + k
}

/// Envelope `env`'s knob `k`.
pub const fn env(env: usize, k: usize) -> usize {
    SHAPE_LEN + env * ADSR_LEN + k
}

/// Chain layer `layer`'s knob `k` (the layer's place among the card's running chain layers).
pub const fn chain(layer: usize, k: usize) -> usize {
    CHAIN_START + layer * CHAIN_LEN + k
}

/// The knobs' values, by place.
pub type Values = [f32; KNOBS];

fn number(v: &Value) -> Option<f32> {
    match v {
        Value::Number(x) => Some(*x as f32),
        Value::Int(i) => Some(*i as f32),
        _ => None,
    }
}

fn def_of(defs: &'static [SettingDef], key: &str) -> Option<&'static SettingDef> {
    defs.iter().find(|d| d.key == key)
}

/// Every knob's card key (layered for envelopes), description and place, for a card whose
/// envelopes are `envs` in order.
pub fn layout(envs: &[LayerId]) -> Vec<(String, &'static SettingDef, usize)> {
    layout_with(envs, &[])
}

/// [`layout`] with the card's running chain layers (id and kind, in their places' order).
pub fn layout_with(envs: &[LayerId], chain_layers: &[(LayerId, &str)]) -> Vec<(String, &'static SettingDef, usize)> {
    let mut out = osc_shape_env_layout(envs);
    for (place, (id, kind)) in chain_layers.iter().take(MAX_CHAIN).enumerate() {
        let defs: &'static [SettingDef] = match *kind {
            MODULATION => ModulationSettings::defs(),
            FILTER => FilterSettings::defs(),
            DISTORTION => DistortionSettings::defs(),
            _ => continue,
        };
        for (k, key) in CHAIN_KEYS.iter().enumerate() {
            if let Some(d) = def_of(defs, key) {
                out.push((layered_key(*id, key), d, chain(place, k)));
            }
        }
    }
    out
}

fn osc_shape_env_layout(envs: &[LayerId]) -> Vec<(String, &'static SettingDef, usize)> {
    let voice = VoiceSettings::defs();
    let adsr = AdsrSettings::defs();
    let mut out = Vec::with_capacity(KNOBS);
    for (o, prefix) in ["a_", "b_"].iter().enumerate() {
        for (k, key) in OSC_KEYS.iter().enumerate() {
            let full = format!("{prefix}{key}");
            if let Some(d) = def_of(voice, &full) {
                out.push((full, d, osc(o, k)));
            }
        }
    }
    for (k, key) in SHAPE_KEYS.iter().enumerate() {
        if let Some(d) = def_of(voice, key) {
            out.push(((*key).to_owned(), d, VOICES + k));
        }
    }
    for (e, id) in envs.iter().take(MAX_ENVS).enumerate() {
        for (k, key) in ADSR_KEYS.iter().enumerate() {
            if let Some(d) = def_of(adsr, key) {
                out.push((layered_key(*id, key), d, env(e, k)));
            }
        }
    }
    out
}

/// Every knob's value from a card's resolved params (defaults where missing).
pub fn values(layout: &[(String, &'static SettingDef, usize)], get: &dyn Fn(&str) -> Option<Value>) -> Values {
    let mut v = [0.0; KNOBS];
    for (key, def, i) in layout {
        v[*i] = get(key).as_ref().and_then(number).or_else(|| number(&def.default)).unwrap_or(0.0);
    }
    v
}

/// The knobs routes can reach.
pub fn targets(layout: &[(String, &'static SettingDef, usize)]) -> TargetTable {
    let mut t = TargetTable::default();
    for (key, def, i) in layout {
        let range = def.range.unwrap_or((0.0, 1.0));
        t.insert(key.clone(), TargetInfo { index: *i as u16, range, log: false });
    }
    t
}

/// The value of a keyed setting at a moment, as a knob value.
pub fn track_value(track: &bpe_core::Track, time: f64) -> Option<f32> {
    track.value_at(time).as_ref().and_then(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_knob_has_its_own_place_and_a_real_setting() {
        let l = layout(&[LayerId(0), LayerId(3)]);
        let mut seen = std::collections::BTreeSet::new();
        for (key, def, i) in &l {
            assert!(seen.insert(*i), "{key} shares place {i}");
            assert!(def.key == key.rsplit('/').next().unwrap() || key.ends_with(def.key), "{key}");
        }
        assert_eq!(l.len(), 2 * OSC_LEN + SHAPE_KEYS.len() + 2 * ADSR_LEN);
        let v = values(&l, &|_| None);
        assert_eq!(v[osc(0, UNISON)], 1.0);
        assert_eq!(v[osc(1, DETUNE)], 20.0);
        assert_eq!(v[VOICES], 16.0);
        assert_eq!(v[env(1, DECAY)], 350.0);
        assert_eq!(v[env(0, GATE_MS)], 100.0);
    }

    #[test]
    fn chain_knobs_have_places_of_their_own() {
        let l = layout_with(&[LayerId(0)], &[(LayerId(5), FILTER), (LayerId(6), DISTORTION), (LayerId(7), MODULATION)]);
        let mut seen = std::collections::BTreeSet::new();
        for (key, _, i) in &l {
            assert!(seen.insert(*i), "{key} shares place {i}");
            assert!(*i < KNOBS);
        }
        let place = |key: &str| l.iter().find(|(k, _, _)| k == key).map(|(_, _, i)| *i);
        assert_eq!(place("5/cutoff"), Some(chain(0, CUTOFF)));
        assert_eq!(place("6/drive"), Some(chain(1, DRIVE)));
        assert_eq!(place("7/amount"), Some(chain(2, AMOUNT)));
        assert_eq!(place("7/cutoff"), None, "Modulation has no cutoff");
        let v = values(&l, &|_| None);
        assert_eq!(v[chain(0, CUTOFF)], 2000.0);
        assert_eq!(v[chain(1, DRIVE)], 12.0);
        assert_eq!(v[chain(2, MIX)], 100.0);
    }
}
