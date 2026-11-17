//! A Liquid thing's starting sounds (liquid part 5): the spec's splash voices as ready-made Sound
//! cards a producer starts from (Enable sound on a Liquid thing gives Splash; the Sound tab's
//! Start from row gives any of them). Each is an ordinary Sound card: oscillators, envelopes,
//! modulators, chain layers and which of the liquid's moments play it.

use super::chain::FILTER;
use super::mods::HIT;
use super::voice::{NoiseColour, VoiceSettings, Wave, starter_voice};
use bpe_core::{Choice, Layer, LayerId, ModRoute, ROUTES_KEY, Registry, Settings, Slot, Value, layered_key};

/// The starting sounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LiquidPreset {
    /// White noise through a band-pass that sweeps down: a ball splashing in.
    #[default]
    Splash,
    /// A sine whose pitch springs up: a ball going in.
    Plop,
    /// A high, short sine chirping up: a drop landing.
    Drip,
    /// A low triangle through a wobbling low-pass: a ball coming out.
    Glug,
    /// Short grains of pink noise, high-passed: the liquid pouring.
    Pour,
    /// Little rising sine blips at random pitches: bubbles from balls moving under it.
    Bubbles,
}

impl LiquidPreset {
    pub const ALL: [LiquidPreset; 6] = [
        LiquidPreset::Splash,
        LiquidPreset::Plop,
        LiquidPreset::Drip,
        LiquidPreset::Glug,
        LiquidPreset::Pour,
        LiquidPreset::Bubbles,
    ];

    /// Its name on the Sound tab.
    pub fn name(self) -> &'static str {
        match self {
            LiquidPreset::Splash => "Splash",
            LiquidPreset::Plop => "Plop",
            LiquidPreset::Drip => "Drip",
            LiquidPreset::Glug => "Glug",
            LiquidPreset::Pour => "Pour",
            LiquidPreset::Bubbles => "Bubbles",
        }
    }

    /// What it sounds like and what plays it, for the Sound tab's hover text.
    pub fn help(self) -> &'static str {
        match self {
            LiquidPreset::Splash => "Noise swept down a band-pass, played when a ball splashes in",
            LiquidPreset::Plop => "A sine springing up in pitch, played when a ball goes in",
            LiquidPreset::Drip => "A short high chirp, played when a ball splashes in",
            LiquidPreset::Glug => "A low wobbling tone, played when a ball comes out",
            LiquidPreset::Pour => "Grains of noise while the liquid pours",
            LiquidPreset::Bubbles => "Rising blips at random pitches from balls moving under it",
        }
    }
}

const ADSR1: LayerId = LayerId(0);
const ADSR2: LayerId = LayerId(1);
const LFO1: LayerId = LayerId(4);
/// The ids the presets add after the starter's six layers.
const FILTER1: LayerId = LayerId(6);
const HIT1: LayerId = LayerId(7);

fn num(card: &mut Slot, layer: LayerId, key: &str, v: f64) {
    card.params.set(&layered_key(layer, key), Value::Number(v));
}

fn route(source: LayerId, target: String, amount: f32, invert: bool) -> ModRoute {
    ModRoute { source, target, amount, bipolar: false, invert }
}

fn on(card: &mut Slot, id: LayerId) {
    if let Some(l) = card.layers.iter_mut().find(|l| l.id == id) {
        l.enabled = true;
    }
}

/// Envelope `id` as a plucked shape: no sustain.
fn pluck(card: &mut Slot, id: LayerId, attack: f64, decay: f64, release: f64) {
    num(card, id, "attack", attack);
    num(card, id, "hold", 0.0);
    num(card, id, "decay", decay);
    num(card, id, "sustain", 0.0);
    num(card, id, "release", release);
}

/// A filter layer on Osc A's chain.
fn filter(card: &mut Slot, kind: &str, cutoff: f64, reso: f64) {
    card.layers.push(Layer {
        id: FILTER1,
        kind: FILTER.into(),
        name: "Filter 1".into(),
        enabled: true,
        lane: Some(0),
        ..Layer::default()
    });
    card.params.set(&layered_key(FILTER1, "kind"), Value::Choice(kind.into()));
    num(card, FILTER1, "cutoff", cutoff);
    num(card, FILTER1, "reso", reso);
}

/// The Sound card `preset` starts from, switched on.
pub fn liquid_sound(reg: &Registry, preset: LiquidPreset) -> Slot {
    let mut card = starter_voice(reg);
    let mut v = VoiceSettings::from_params(&card.params);
    v.moments_splash = false;
    v.play_voices = 8;
    let mut routes = vec![route(ADSR1, "a_gain".into(), 1.0, false)];
    match preset {
        LiquidPreset::Splash => {
            v.a.wave = Wave::Noise;
            v.a.noise = NoiseColour::White;
            v.moments_splash = true;
            v.play_full_at = 1200.0;
            pluck(&mut card, ADSR1, 0.0, 260.0, 120.0);
            filter(&mut card, "band_pass", 900.0, 25.0);
            on(&mut card, ADSR2);
            pluck(&mut card, ADSR2, 0.0, 320.0, 100.0);
            routes.push(route(ADSR2, layered_key(FILTER1, "cutoff"), 0.3, false));
        }
        LiquidPreset::Plop => {
            v.a.wave = Wave::Sine;
            v.moments_go_in = true;
            v.play_size_pitch = 12.0;
            pluck(&mut card, ADSR1, 1.0, 140.0, 60.0);
            on(&mut card, ADSR2);
            pluck(&mut card, ADSR2, 0.0, 60.0, 30.0);
            // Inverted, the envelope starts at nothing and rises as it decays: the pitch springs up.
            routes.push(route(ADSR2, "a_semi".into(), 0.25, true));
        }
        LiquidPreset::Drip => {
            v.a.wave = Wave::Sine;
            v.a.octave = 1;
            v.moments_splash = true;
            v.play_size_pitch = 12.0;
            pluck(&mut card, ADSR1, 0.0, 90.0, 40.0);
            on(&mut card, ADSR2);
            pluck(&mut card, ADSR2, 0.0, 30.0, 20.0);
            routes.push(route(ADSR2, "a_semi".into(), 0.3, true));
        }
        LiquidPreset::Glug => {
            v.a.wave = Wave::Triangle;
            v.a.octave = -1;
            v.moments_come_out = true;
            pluck(&mut card, ADSR1, 5.0, 300.0, 120.0);
            filter(&mut card, "low_pass", 600.0, 45.0);
            on(&mut card, LFO1);
            num(&mut card, LFO1, "rate_hz", 9.0);
            routes.push(route(LFO1, layered_key(FILTER1, "cutoff"), 0.15, false));
        }
        LiquidPreset::Pour => {
            v.a.wave = Wave::Noise;
            v.a.noise = NoiseColour::Pink;
            v.moments_pour = true;
            v.play_gap = 0.0;
            v.play_voices = 16;
            v.play_full_at = 600.0;
            pluck(&mut card, ADSR1, 2.0, 60.0, 40.0);
            filter(&mut card, "high_pass", 1500.0, 10.0);
        }
        LiquidPreset::Bubbles => {
            v.a.wave = Wave::Sine;
            v.a.octave = 1;
            v.moments_bubbles = true;
            v.play_gap = 15.0;
            v.play_voices = 16;
            v.play_full_at = 400.0;
            pluck(&mut card, ADSR1, 0.0, 70.0, 30.0);
            on(&mut card, ADSR2);
            pluck(&mut card, ADSR2, 0.0, 50.0, 20.0);
            routes.push(route(ADSR2, "a_semi".into(), 0.2, true));
            card.layers.push(Layer {
                id: HIT1,
                kind: HIT.into(),
                name: "Hit 1".into(),
                enabled: true,
                ..Layer::default()
            });
            card.params.set(&layered_key(HIT1, "pick"), Value::Choice("random".into()));
            routes.push(route(HIT1, "a_semi".into(), 0.25, false));
        }
    }
    for (k, value) in v.to_params().iter() {
        card.params.set(k, value.clone());
    }
    card.params.set(ROUTES_KEY, Value::Routes(routes));
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::VOICE;

    #[test]
    fn every_preset_is_a_sound_card_that_plays_some_moment() {
        let reg = crate::registry();
        for p in LiquidPreset::ALL {
            let card = liquid_sound(&reg, p);
            assert_eq!(card.kind, VOICE);
            let v = VoiceSettings::from_params(&card.params);
            assert!(v.enabled, "{p:?} is on");
            let moments = [v.moments_splash, v.moments_go_in, v.moments_come_out, v.moments_pour, v.moments_bubbles];
            assert_eq!(moments.iter().filter(|m| **m).count(), 1, "{p:?} plays one moment");
            for l in &card.layers {
                assert!(reg.layer(&l.kind).is_some(), "{p:?}: {} is a kit layer", l.kind);
            }
            let Some(Value::Routes(routes)) = card.params.get(ROUTES_KEY) else { panic!("{p:?} has routes") };
            for r in routes {
                assert!(card.layers.iter().any(|l| l.id == r.source && l.enabled), "{p:?}: {r:?} from a layer on");
            }
        }
    }
}
