//! Ten starter patches, each exposing a few macros, for any scene to add from a menu.

use crate::catalog;
use bpe_core::{NoteLength, VoiceSettings};
use bpe_graph::{Catalog, Graph, Link, Param};

/// Builds a patch block by block, laid out left to right.
struct Patch {
    g: Graph,
    x: f32,
}

impl Patch {
    fn new(name: &str) -> Self {
        Self { g: Graph::new(name), x: 0.0 }
    }

    /// Adds a block with some knobs changed from their defaults.
    fn block(&mut self, kind: &str, knobs: &[(&str, f32)]) -> u32 {
        let spec = catalog().spec(kind).expect("a synth block");
        let id = self.g.add(spec, [self.x, 0.0]);
        self.x += 200.0;
        let node = self.g.node_mut(id).expect("just added");
        for &(name, v) in knobs {
            node.params.insert(name.into(), Param::Number(v));
        }
        id
    }

    fn choose(&mut self, id: u32, name: &str, choice: &str) {
        self.g.node_mut(id).expect("a block").params.insert(name.into(), Param::Choice(choice.into()));
    }

    fn wire(&mut self, from: u32, to: u32, pin: u8) {
        let ok = self.g.connect(Link { from: (from, 0), to: (to, pin) }, catalog());
        debug_assert!(ok, "wire {from} -> {to}.{pin}");
    }

    fn expose(&mut self, id: u32, knob: &str) {
        self.g.node_mut(id).expect("a block").exposed.insert(knob.into());
    }

    /// `source` through an amp shaped by an envelope, into the voice's output.
    fn out(&mut self, source: u32, env: &[(&str, f32)], gain: f32) -> (u32, u32) {
        let env = self.block("adsr", env);
        let amp = self.block("amp", &[("gain", gain)]);
        self.wire(source, amp, 0);
        self.wire(env, amp, 1);
        let out = self.block("voice_out", &[]);
        self.wire(amp, out, 0);
        (env, amp)
    }
}

fn voices(note_length: NoteLength) -> VoiceSettings {
    VoiceSettings { note_length, ..VoiceSettings::default() }
}

/// The starter patches, in menu order: (name, patch, how it plays notes).
pub fn starters() -> Vec<(&'static str, Graph, VoiceSettings)> {
    vec![
        ("Soft pluck", soft_pluck(), voices(NoteLength::Seconds(0.3))),
        ("Glass FM bell", glass_bell(), voices(NoteLength::Seconds(0.5))),
        ("Supersaw", supersaw(), voices(NoteLength::Beats(1.0))),
        ("Warm pad", warm_pad(), voices(NoteLength::Beats(2.0))),
        ("Sub bass", sub_bass(), VoiceSettings { mono: true, glide: 0.05, ..voices(NoteLength::Beats(0.5)) }),
        ("808", eight_oh_eight(), voices(NoteLength::Seconds(0.2))),
        ("Chip square", chip_square(), voices(NoteLength::Beats(0.5))),
        ("Noise snap", noise_snap(), voices(NoteLength::Seconds(0.1))),
        ("Wobble bass", wobble_bass(), VoiceSettings { mono: true, glide: 0.03, ..voices(NoteLength::Beats(1.0)) }),
        ("Marimba-ish", marimba_ish(), voices(NoteLength::Seconds(0.2))),
    ]
}

fn soft_pluck() -> Graph {
    let mut p = Patch::new("Soft pluck");
    let tri = p.block("osc", &[]);
    p.choose(tri, "wave", "triangle");
    let saw = p.block("osc", &[("cents", 7.0)]);
    let mix = p.block("mix", &[("level2", 0.35)]);
    p.wire(tri, mix, 0);
    p.wire(saw, mix, 1);
    let filter = p.block("filter", &[("cutoff", 2_500.0), ("key_track", 0.5)]);
    p.wire(mix, filter, 0);
    let (env, _) = p.out(filter, &[("attack", 0.002), ("decay", 0.4), ("sustain", 0.0), ("release", 0.3)], -3.0);
    p.expose(filter, "cutoff");
    p.expose(env, "decay");
    p.g
}

fn glass_bell() -> Graph {
    let mut p = Patch::new("Glass FM bell");
    let modulator = p.block("osc", &[("semitones", 21.69)]);
    p.choose(modulator, "wave", "sine");
    let mod_env = p.block("adsr", &[("attack", 0.0), ("decay", 0.8), ("sustain", 0.1), ("release", 0.8)]);
    let depth = p.block("amp", &[]);
    p.wire(modulator, depth, 0);
    p.wire(mod_env, depth, 1);
    let carrier = p.block("osc", &[("fm", 4.0)]);
    p.choose(carrier, "wave", "sine");
    p.wire(depth, carrier, 1);
    let (env, _) = p.out(carrier, &[("attack", 0.001), ("decay", 1.5), ("sustain", 0.0), ("release", 1.2)], -4.0);
    p.expose(carrier, "fm");
    p.expose(env, "decay");
    p.g
}

fn supersaw() -> Graph {
    let mut p = Patch::new("Supersaw");
    let osc = p.block("osc", &[("unison", 7.0), ("detune", 25.0), ("spread", 0.8)]);
    let filter = p.block("filter", &[("cutoff", 5_000.0), ("resonance", 0.1)]);
    p.wire(osc, filter, 0);
    p.out(filter, &[("attack", 0.01), ("decay", 0.3), ("sustain", 0.8), ("release", 0.4)], -8.0);
    p.expose(osc, "detune");
    p.expose(filter, "cutoff");
    p.g
}

fn warm_pad() -> Graph {
    let mut p = Patch::new("Warm pad");
    let saw = p.block("osc", &[("unison", 4.0), ("detune", 12.0), ("spread", 0.9)]);
    let square = p.block("osc", &[("semitones", -12.0)]);
    p.choose(square, "wave", "square");
    let mix = p.block("mix", &[("level2", 0.4)]);
    p.wire(saw, mix, 0);
    p.wire(square, mix, 1);
    let ladder = p.block("ladder", &[("cutoff", 1_200.0), ("resonance", 0.2)]);
    p.wire(mix, ladder, 0);
    let lfo = p.block("lfo", &[("rate", 0.3)]);
    let depth = p.block("scale", &[("scale", 0.5)]);
    p.wire(lfo, depth, 0);
    p.wire(depth, ladder, 1);
    let (env, _) = p.out(ladder, &[("attack", 0.6), ("decay", 1.0), ("sustain", 0.8), ("release", 1.5)], -6.0);
    p.expose(ladder, "cutoff");
    p.expose(env, "attack");
    p.expose(env, "release");
    p.g
}

fn sub_bass() -> Graph {
    let mut p = Patch::new("Sub bass");
    let sine = p.block("osc", &[("semitones", -12.0)]);
    p.choose(sine, "wave", "sine");
    let tri = p.block("osc", &[("semitones", -12.0)]);
    p.choose(tri, "wave", "triangle");
    let mix = p.block("mix", &[("level2", 0.3)]);
    p.wire(sine, mix, 0);
    p.wire(tri, mix, 1);
    let drive = p.block("drive", &[("amount", 0.03)]);
    p.wire(mix, drive, 0);
    let (env, _) = p.out(drive, &[("attack", 0.005), ("decay", 0.2), ("sustain", 0.9), ("release", 0.15)], -4.0);
    p.expose(drive, "amount");
    p.expose(env, "release");
    p.g
}

fn eight_oh_eight() -> Graph {
    let mut p = Patch::new("808");
    let pitch_env = p.block("adsr", &[("attack", 0.0), ("decay", 0.08), ("sustain", 0.0), ("release", 0.05)]);
    let sweep = p.block("scale", &[("scale", 12.0)]);
    p.wire(pitch_env, sweep, 0);
    let sine = p.block("osc", &[("semitones", -12.0)]);
    p.choose(sine, "wave", "sine");
    p.wire(sweep, sine, 0);
    let drive = p.block("drive", &[("amount", 0.05)]);
    p.wire(sine, drive, 0);
    let (env, _) = p.out(drive, &[("attack", 0.001), ("decay", 1.2), ("sustain", 0.0), ("release", 0.3)], -4.0);
    p.expose(env, "decay");
    p.expose(drive, "amount");
    p.expose(sweep, "scale");
    p.g
}

fn chip_square() -> Graph {
    let mut p = Patch::new("Chip square");
    let osc = p.block("osc", &[("width", 0.25)]);
    p.choose(osc, "wave", "pulse");
    let (env, _) = p.out(osc, &[("attack", 0.001), ("decay", 0.1), ("sustain", 0.6), ("release", 0.05)], -8.0);
    p.expose(osc, "width");
    p.expose(env, "release");
    p.g
}

fn noise_snap() -> Graph {
    let mut p = Patch::new("Noise snap");
    let noise = p.block("noise", &[]);
    let filter = p.block("filter", &[("cutoff", 3_000.0), ("resonance", 0.3)]);
    p.choose(filter, "mode", "band");
    p.wire(noise, filter, 0);
    let body = p.block("osc", &[]);
    p.choose(body, "wave", "triangle");
    let body_env = p.block("adsr", &[("attack", 0.0), ("decay", 0.06), ("sustain", 0.0), ("release", 0.03)]);
    let body_amp = p.block("amp", &[("gain", -6.0)]);
    p.wire(body, body_amp, 0);
    p.wire(body_env, body_amp, 1);
    let mix = p.block("mix", &[]);
    p.wire(filter, mix, 0);
    p.wire(body_amp, mix, 1);
    let (env, _) = p.out(mix, &[("attack", 0.001), ("decay", 0.12), ("sustain", 0.0), ("release", 0.08)], -2.0);
    p.expose(filter, "cutoff");
    p.expose(env, "decay");
    p.g
}

fn wobble_bass() -> Graph {
    let mut p = Patch::new("Wobble bass");
    let osc = p.block("osc", &[("semitones", -12.0), ("unison", 2.0), ("detune", 10.0)]);
    let ladder = p.block("ladder", &[("cutoff", 400.0), ("resonance", 0.5), ("drive", 0.3)]);
    p.wire(osc, ladder, 0);
    let lfo = p.block("lfo", &[]);
    p.choose(lfo, "sync", "1/8");
    let depth = p.block("scale", &[("scale", 2.5), ("offset", 1.0)]);
    p.wire(lfo, depth, 0);
    p.wire(depth, ladder, 1);
    p.out(ladder, &[("attack", 0.005), ("decay", 0.2), ("sustain", 0.9), ("release", 0.1)], -2.0);
    p.expose(ladder, "cutoff");
    p.expose(ladder, "resonance");
    p.expose(depth, "scale");
    p.g
}

fn marimba_ish() -> Graph {
    let mut p = Patch::new("Marimba-ish");
    let modulator = p.block("osc", &[("semitones", 24.0)]);
    p.choose(modulator, "wave", "sine");
    let mod_env = p.block("adsr", &[("attack", 0.0), ("decay", 0.05), ("sustain", 0.0), ("release", 0.05)]);
    let depth = p.block("amp", &[]);
    p.wire(modulator, depth, 0);
    p.wire(mod_env, depth, 1);
    let carrier = p.block("osc", &[("fm", 3.0)]);
    p.choose(carrier, "wave", "sine");
    p.wire(depth, carrier, 1);
    let (env, _) = p.out(carrier, &[("attack", 0.001), ("decay", 0.6), ("sustain", 0.0), ("release", 0.4)], -3.0);
    p.expose(carrier, "fm");
    p.expose(env, "decay");
    p.g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlayNote, compile, macros, render_channel};
    use std::hash::{Hash, Hasher};

    fn play(graph: &Graph, voices: &VoiceSettings) -> Vec<f32> {
        let program = compile(graph).unwrap_or_else(|e| panic!("{}: {e:?}", graph.name));
        let note = PlayNote { frame: 0, key: 60, velocity: 0.8, left: 1.0, right: 1.0, steal_at: None };
        render_channel(&program, &[note], voices, 0.5, 48_000.0, 48_000).samples
    }

    fn hash(x: &[f32]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in x {
            v.to_bits().hash(&mut h);
        }
        h.finish()
    }

    #[test]
    fn every_starter_compiles_sounds_and_repeats() {
        let all = starters();
        assert_eq!(all.len(), 10);
        for (name, graph, voices) in &all {
            let out = play(graph, voices);
            let rms = (out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32).sqrt();
            let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(rms > 0.005, "{name} is too quiet: {rms}");
            assert!(peak <= 1.5, "{name} peaks at {peak}");
            assert_eq!(hash(&out), hash(&play(graph, voices)), "{name} repeats");
        }
    }

    #[test]
    fn every_starter_has_two_to_four_macros() {
        for (name, graph, _) in starters() {
            let n = macros(&graph).len();
            assert!((2..=4).contains(&n), "{name} has {n} macros");
        }
    }
}
