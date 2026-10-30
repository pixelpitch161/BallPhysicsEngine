//! The synth catalog: every block a patch can use, as the node panel shows it, and how to make
//! each one.

use crate::blocks::*;
use crate::{Block, Knobs};
use bpe_graph::{NodeSpec, Param, ParamSpec, PinSpec, PinType, SpecList};
use std::sync::OnceLock;

const SYNC: &[&str] = &["off", "1/1", "1/2", "1/4", "1/8", "1/16", "1/4T", "1/8T"];

fn audio(name: &str) -> PinSpec {
    PinSpec::new(name, PinType::Audio)
}

fn number(name: &str) -> PinSpec {
    PinSpec::new(name, PinType::Number)
}

fn flag(name: &str, label: &str, default: bool) -> ParamSpec {
    ParamSpec::new(name, label, Param::Bool(default))
}

fn choice(name: &str, label: &str, choices: &[&str], default: &str) -> ParamSpec {
    ParamSpec { default: Param::Choice(default.into()), ..ParamSpec::choice(name, label, choices) }
}

/// Every block a patch can use.
pub fn catalog() -> &'static SpecList {
    static CATALOG: OnceLock<SpecList> = OnceLock::new();
    CATALOG.get_or_init(|| SpecList { specs: specs(), cycles: false })
}

fn specs() -> Vec<NodeSpec> {
    let n = ParamSpec::number;
    vec![
        NodeSpec::new("osc", "Oscillator", "Sources", "A wave at the note's pitch.")
            .input(number("pitch").help("Semitones up or down from the note"))
            .input(audio("fm").help("Bends the wave's phase; the FM knob sets how far"))
            .input(audio("sync").help("Starts the wave again each time this rises past zero"))
            .output(audio("out"))
            .param(choice("wave", "Wave", &["sine", "triangle", "saw", "square", "pulse"], "saw"))
            .param(n("width", "Pulse width", 0.5, 0.01, 0.99))
            .param(n("semitones", "Semitones", 0.0, -48.0, 48.0))
            .param(n("cents", "Cents", 0.0, -100.0, 100.0))
            .param(n("unison", "Unison", 1.0, 1.0, 8.0))
            .param(n("detune", "Detune", 20.0, 0.0, 100.0).help("Cents between the outer unison voices and the middle"))
            .param(n("spread", "Spread", 0.5, 0.0, 1.0).help("How wide the unison voices sit"))
            .param(n("fm", "FM", 0.0, 0.0, 10.0))
            .param(choice("phase", "Phase", &["note", "free"], "note")),
        NodeSpec::new("noise", "Noise", "Sources", "Hiss: white is bright, pink is softer.")
            .output(audio("out"))
            .param(choice("colour", "Colour", &["white", "pink"], "white")),
        NodeSpec::new("filter", "Filter", "Shaping", "Takes away highs, lows or a band.")
            .input(audio("in").required())
            .input(number("cutoff_mod").help("Octaves up or down from the cutoff"))
            .output(audio("out"))
            .param(choice("mode", "Mode", &["low", "high", "band", "notch"], "low"))
            .param(n("cutoff", "Cutoff", 2_000.0, 20.0, 20_000.0))
            .param(n("resonance", "Resonance", 0.0, 0.0, 1.0))
            .param(n("key_track", "Key tracking", 0.0, 0.0, 1.0).help("1 moves the cutoff with the note")),
        NodeSpec::new("ladder", "Ladder", "Shaping", "A warm four-pole low-pass that can be driven.")
            .input(audio("in").required())
            .input(number("cutoff_mod").help("Octaves up or down from the cutoff"))
            .output(audio("out"))
            .param(n("cutoff", "Cutoff", 2_000.0, 20.0, 20_000.0))
            .param(n("resonance", "Resonance", 0.0, 0.0, 1.0))
            .param(n("drive", "Drive", 0.0, 0.0, 1.0))
            .param(n("key_track", "Key tracking", 0.0, 0.0, 1.0)),
        NodeSpec::new("drive", "Drive", "Shaping", "Distortion.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(choice("mode", "Mode", &["soft", "hard", "fold"], "soft"))
            .param(n("amount", "Amount", 0.3, 0.0, 1.0))
            .param(n("bias", "Bias", 0.0, -1.0, 1.0)),
        NodeSpec::new("amp", "Amp", "Shaping", "Louder or quieter; wire an envelope into mod.")
            .input(audio("in").required())
            .input(number("mod").help("Multiplies the sound, usually by an envelope"))
            .output(audio("out"))
            .param(n("gain", "Gain dB", 0.0, -60.0, 12.0)),
        NodeSpec::new("adsr", "Envelope", "Modulators", "Rises when the note starts and falls after it ends.")
            .output(number("env"))
            .param(n("attack", "Attack", 0.005, 0.0, 10.0))
            .param(n("decay", "Decay", 0.2, 0.0, 10.0))
            .param(n("sustain", "Sustain", 0.7, 0.0, 1.0))
            .param(n("release", "Release", 0.3, 0.0, 10.0))
            .param(n("curve", "Curve", 0.0, -1.0, 1.0)),
        NodeSpec::new("lfo", "LFO", "Modulators", "A slow wave for wobbles and sweeps, from -1 to 1.")
            .output(number("out"))
            .param(choice("wave", "Wave", &["sine", "triangle", "saw", "square", "random"], "sine"))
            .param(n("rate", "Rate Hz", 2.0, 0.01, 50.0))
            .param(choice("sync", "Sync", SYNC, "off"))
            .param(flag("retrigger", "Restart each note", true))
            .param(n("phase", "Phase", 0.0, 0.0, 1.0)),
        NodeSpec::new("hold", "Sample and hold", "Modulators", "Picks a value at a steady rate and holds it.")
            .input(number("in"))
            .output(number("out"))
            .param(choice("source", "Source", &["noise", "input"], "noise"))
            .param(n("rate", "Rate Hz", 8.0, 0.01, 50.0))
            .param(choice("sync", "Sync", SYNC, "off")),
        NodeSpec::new("mix", "Mix", "Maths", "Adds up to four sounds, each with a level.")
            .input(audio("in1"))
            .input(audio("in2"))
            .input(audio("in3"))
            .input(audio("in4"))
            .output(audio("out"))
            .param(n("level1", "Level 1", 1.0, 0.0, 2.0))
            .param(n("level2", "Level 2", 1.0, 0.0, 2.0))
            .param(n("level3", "Level 3", 1.0, 0.0, 2.0))
            .param(n("level4", "Level 4", 1.0, 0.0, 2.0)),
        NodeSpec::new("add", "Add", "Maths", "a + b.").input(number("a")).input(number("b")).output(number("out")),
        NodeSpec::new("multiply", "Multiply", "Maths", "a × b.")
            .input(number("a"))
            .input(number("b"))
            .output(number("out")),
        NodeSpec::new("scale", "Scale and offset", "Maths", "in × scale + offset.")
            .input(number("in"))
            .output(number("out"))
            .param(n("scale", "Scale", 1.0, -10.0, 10.0))
            .param(n("offset", "Offset", 0.0, -10.0, 10.0)),
        NodeSpec::new("crossfade", "Crossfade", "Maths", "From a to b as mix goes from 0 to 1.")
            .input(audio("a"))
            .input(audio("b"))
            .input(number("mix"))
            .output(audio("out"))
            .param(n("mix", "Mix", 0.5, 0.0, 1.0)),
        NodeSpec::new("pitch_to_hz", "Pitch to Hz", "Maths", "A note number to a frequency.")
            .input(number("in"))
            .output(number("out")),
        NodeSpec::new("note", "Note", "Inputs", "The note being played.")
            .output(number("pitch"))
            .output(number("velocity"))
            .output(number("gate"))
            .output(number("time")),
        NodeSpec::new("macro", "Macro", "Inputs", "A value to show on the card: Show on the card to expose it.")
            .output(number("out"))
            .param(n("value", "Value", 0.5, 0.0, 1.0))
            .param(n("min", "Min", 0.0, -10_000.0, 10_000.0))
            .param(n("max", "Max", 1.0, -10_000.0, 10_000.0)),
        NodeSpec::new("voice_out", "Voice out", "Output", "What the voice sounds like. A patch has exactly one.")
            .input(audio("left").required())
            .input(audio("right").help("Leave empty to send the left to both sides")),
    ]
}

/// Makes a block of `kind` with its knobs.
pub(crate) fn make(kind: &str, k: &Knobs) -> Option<Box<dyn Block>> {
    Some(match kind {
        "osc" => Box::new(Osc::new(k)),
        "noise" => Box::new(Noise::new(k)),
        "filter" => Box::new(Svf::new(k)),
        "ladder" => Box::new(Ladder::new(k)),
        "drive" => Box::new(Drive::new(k)),
        "amp" => Box::new(Amp::new(k)),
        "adsr" => Box::new(Adsr::new(k)),
        "lfo" => Box::new(Lfo::new(k)),
        "hold" => Box::new(Hold::new(k)),
        "mix" => Box::new(Mix::new(k)),
        "add" => Box::new(Arith::add()),
        "multiply" => Box::new(Arith::multiply()),
        "scale" => Box::new(Scale::new(k)),
        "crossfade" => Box::new(Crossfade::new(k)),
        "pitch_to_hz" => Box::new(PitchToHz),
        "note" => Box::new(NoteIn),
        "macro" => Box::new(Macro::new(k)),
        "voice_out" => Box::new(VoiceOut),
        #[cfg(test)]
        "nan" => Box::new(testing::Nan),
        _ => return None,
    })
}

#[cfg(test)]
mod testing {
    use crate::{Block, Buf, Ctx};

    /// A broken block that makes samples that aren't numbers.
    pub struct Nan;

    impl Block for Nan {
        fn reset(&mut self, _cx: &Ctx) {}
        fn process(&mut self, _cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
            outputs[0] = [[f32::NAN; crate::BLOCK]; 2];
        }
    }
}
