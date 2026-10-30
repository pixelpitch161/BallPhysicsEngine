//! The synth catalog: every block a patch can use, as the node panel shows it, and how to make
//! each one.

use crate::blocks::*;
use crate::inputs::{HIT_INPUTS, InputDef, LIVE_INPUTS, SCENE_INPUTS};
use crate::{Block, Knobs};
use bpe_graph::{NodeSpec, Param, ParamSpec, PinSpec, PinType, SpecList};
use std::sync::OnceLock;

const SWEEP: &[&str] = &["off", "lfo", "1/1", "1/2", "1/4", "1/8", "1/16"];
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

/// An input block reading one of `table`, mapped to 0 to 1.
fn input_block(kind: &str, title: &str, help: &str, table: &[InputDef], smoothing: f32) -> NodeSpec {
    let names: Vec<&str> = table.iter().map(|d| d.name).collect();
    let n = ParamSpec::number;
    NodeSpec::new(kind, title, "Inputs", help)
        .output(number("out"))
        .param(choice("input", "Input", &names, names[0]))
        .param(n("low", "In low", 0.0, -10_000.0, 10_000.0).help("The value that gives 0"))
        .param(n("high", "In high", 1.0, -10_000.0, 10_000.0).help("The value that gives 1"))
        .param(choice("curve", "Curve", &["linear", "exponential", "S"], "linear"))
        .param(n("smoothing", "Smoothing ms", smoothing, 0.0, 2_000.0))
}

/// Every block a patch can use.
pub fn catalog() -> &'static SpecList {
    static CATALOG: OnceLock<SpecList> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let specs = specs()
            .into_iter()
            .map(|spec| {
                let kind = spec.kind.clone();
                followable(&kind).iter().fold(spec, |spec, knob| {
                    spec.input(number(knob).help("Added to the knob while something is wired in"))
                })
            })
            .collect();
        SpecList { specs, cycles: false }
    })
}

/// The knobs of a block kind that can move while it plays, each with an input pin after the
/// block's own inputs: what Follow… can wire into.
pub fn followable(kind: &str) -> &'static [&'static str] {
    match kind {
        "osc" => &["semitones", "cents", "width", "detune", "fm"],
        "filter" => &["cutoff", "resonance"],
        "ladder" => &["cutoff", "resonance", "drive"],
        "drive" => &["amount", "tone", "mix"],
        "amp" => &["gain"],
        "eq" => &["low_gain", "mid_gain", "high_gain"],
        "bitcrush" => &["bits", "downsample", "mix"],
        "delay" => &["feedback", "mix"],
        "chorus" => &["rate", "depth", "mix"],
        "reverb" => &["mix", "width"],
        "width" => &["width"],
        "compressor" => &["threshold", "makeup"],
        "crossfade" => &["mix"],
        "lfo" => &["rate"],
        "macro" => &["value"],
        _ => &[],
    }
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
        NodeSpec::new(
            "sampler",
            "Sampler",
            "Sources",
            "Plays a WAV file at the note's pitch, or the nearest of a folder of notes.",
        )
        .output(audio("out"))
        .param(
            ParamSpec::new("file", "File or folder", Param::Text(String::new()))
                .help("A WAV file, or a folder of WAVs named by note such as C4.wav"),
        )
        .param(n("root", "Root note", 60.0, 0.0, 127.0).help("The note a single file was recorded at; 60 is middle C"))
        .param(n("start", "Start", 0.0, 0.0, 1.0))
        .param(flag("loop", "Loop", false)),
        NodeSpec::new("filter", "Filter", "Shaping", "Takes away highs, lows or a band.")
            .input(audio("in").required())
            .input(number("cutoff_mod").help("Octaves up or down from the cutoff"))
            .output(audio("out"))
            .param(choice("mode", "Mode", &["low", "high", "band", "notch"], "low"))
            .param(n("cutoff", "Cutoff", 2_000.0, 20.0, 20_000.0))
            .param(n("resonance", "Resonance", 0.0, 0.0, 1.0))
            .param(n("key_track", "Key tracking", 0.0, 0.0, 1.0).help("1 moves the cutoff with the note"))
            .param(choice("sweep", "Sweep", SWEEP, "off").help("Moves the cutoff up and down on its own"))
            .param(n("sweep_rate", "Sweep rate Hz", 1.0, 0.01, 50.0))
            .param(n("sweep_depth", "Sweep depth", 0.0, 0.0, 4.0).help("Octaves up and down")),
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
            .param(choice("mode", "Mode", &["soft", "hard", "fold", "tube"], "soft"))
            .param(n("amount", "Amount", 0.3, 0.0, 1.0))
            .param(n("bias", "Bias", 0.0, -1.0, 1.0))
            .param(n("tone", "Tone", 1.0, 0.0, 1.0).help("Darker towards 0"))
            .param(n("mix", "Mix", 1.0, 0.0, 1.0)),
        NodeSpec::new("eq", "EQ", "Effects", "Three bands of boost or cut, with low and high cuts.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(n("low_cut", "Low cut", 20.0, 20.0, 1_000.0))
            .param(n("low_gain", "Low dB", 0.0, -18.0, 18.0))
            .param(n("low_freq", "Low freq", 120.0, 20.0, 1_000.0))
            .param(n("mid_gain", "Mid dB", 0.0, -18.0, 18.0))
            .param(n("mid_freq", "Mid freq", 1_000.0, 100.0, 10_000.0))
            .param(n("mid_q", "Mid width Q", 0.7, 0.1, 10.0))
            .param(n("high_gain", "High dB", 0.0, -18.0, 18.0))
            .param(n("high_freq", "High freq", 8_000.0, 1_000.0, 20_000.0))
            .param(n("high_cut", "High cut", 20_000.0, 1_000.0, 20_000.0)),
        NodeSpec::new("bitcrush", "Bitcrush", "Effects", "Fewer bits and a lower sample rate, for grit.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(n("bits", "Bits", 8.0, 1.0, 16.0))
            .param(n("downsample", "Downsample", 1.0, 1.0, 32.0))
            .param(n("mix", "Mix", 1.0, 0.0, 1.0)),
        NodeSpec::new("delay", "Delay", "Effects", "Echoes. Inside a patch it runs on every voice.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(choice("time_mode", "Time", &["ms", "1/4", "1/8", "1/16", "1/2", "1/1"], "ms"))
            .param(choice("feel", "Feel", &["straight", "dotted", "triplet"], "straight"))
            .param(n("ms", "Milliseconds", 250.0, 1.0, 2_000.0))
            .param(n("feedback", "Feedback", 0.35, 0.0, 1.0))
            .param(flag("ping_pong", "Ping-pong", false))
            .param(n("low_cut", "Low cut", 20.0, 20.0, 2_000.0))
            .param(n("high_cut", "High cut", 20_000.0, 500.0, 20_000.0))
            .param(n("mix", "Mix", 0.3, 0.0, 1.0)),
        NodeSpec::new("chorus", "Chorus", "Effects", "Copies on slowly moving short delays, for width.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(n("rate", "Rate Hz", 0.8, 0.01, 10.0))
            .param(n("depth", "Depth ms", 3.0, 0.0, 10.0))
            .param(n("voices", "Voices", 2.0, 1.0, 4.0))
            .param(n("mix", "Mix", 0.5, 0.0, 1.0)),
        NodeSpec::new("reverb", "Reverb", "Effects", "A room or hall. Inside a patch it runs on every voice.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(n("size", "Size", 0.6, 0.0, 1.0))
            .param(n("decay", "Decay s", 2.0, 0.1, 60.0).help("Seconds for the tail to fall by 60 dB"))
            .param(n("damping", "Damping", 0.4, 0.0, 1.0))
            .param(n("predelay", "Pre-delay ms", 10.0, 0.0, 200.0))
            .param(n("width", "Width", 1.0, 0.0, 1.0))
            .param(n("mix", "Mix", 0.3, 0.0, 1.0)),
        NodeSpec::new(
            "compressor",
            "Compressor",
            "Effects",
            "Turns loud parts down; wire another sound into sidechain to duck under it.",
        )
        .input(audio("in").required())
        .input(audio("sidechain").help("Listen to this instead of the input"))
        .output(audio("out"))
        .param(n("threshold", "Threshold dB", -18.0, -60.0, 0.0))
        .param(n("ratio", "Ratio", 4.0, 1.0, 20.0))
        .param(n("attack", "Attack ms", 10.0, 0.0, 200.0))
        .param(n("release", "Release ms", 100.0, 1.0, 2_000.0))
        .param(n("knee", "Knee dB", 6.0, 0.0, 24.0))
        .param(n("makeup", "Makeup dB", 0.0, -24.0, 24.0)),
        NodeSpec::new("clipper", "Clipper", "Effects", "Shaves off peaks, cleanly at four times the rate.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(choice("mode", "Mode", &["soft", "hard"], "soft"))
            .param(n("drive", "Drive dB", 0.0, 0.0, 24.0)),
        NodeSpec::new("limiter", "Limiter", "Effects", "Never lets the sound past the ceiling, even between samples.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(n("ceiling", "Ceiling dB", -1.0, -6.0, 0.0))
            .param(n("release", "Release ms", 100.0, 1.0, 1_000.0)),
        NodeSpec::new("width", "Stereo width", "Effects", "0 is mono, 1 unchanged, 2 twice as wide.")
            .input(audio("in").required())
            .output(audio("out"))
            .param(n("width", "Width", 1.0, 0.0, 2.0)),
        NodeSpec::new("follower", "Envelope follower", "Modulators", "How loud a sound is, as a control from 0.")
            .input(audio("in"))
            .output(number("level"))
            .param(n("attack", "Attack", 0.01, 0.0, 1.0))
            .param(n("release", "Release", 0.2, 0.0, 5.0)),
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
        input_block(
            "hit_input",
            "At the hit",
            "A value from the hit that started the note, fixed for the voice.",
            HIT_INPUTS,
            0.0,
        )
        .param(
            ParamSpec::new("tag", "Tag", Param::Text(String::new()))
                .help("For Hit thing's tag: 1 when the thing hit has this tag"),
        ),
        input_block("live_input", "Live", "The ball's value while the note sounds.", LIVE_INPUTS, 30.0),
        input_block("scene_input", "Scene", "A value from the whole scene, on the video's clock.", SCENE_INPUTS, 30.0)
            .param(
                ParamSpec::new("channel", "Channel", Param::Text(String::new()))
                    .help("For Channel level: the sound module to listen to"),
            ),
        NodeSpec::new("macro", "Macro", "Inputs", "A value to show on the card: Show on the card to expose it.")
            .output(number("out"))
            .param(n("value", "Value", 0.5, 0.0, 1.0))
            .param(n("min", "Min", 0.0, -10_000.0, 10_000.0))
            .param(n("max", "Max", 1.0, -10_000.0, 10_000.0)),
        NodeSpec::new("fx_in", "In", "Output", "The sound coming into the effects.").output(audio("out")),
        NodeSpec::new("fx_out", "Out", "Output", "The sound the effects give back. Effects have exactly one.")
            .input(audio("left").required())
            .input(audio("right").help("Leave empty to keep the left's stereo")),
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
        "sampler" => Box::new(Sampler::new(k)),
        "filter" => Box::new(Svf::new(k)),
        "ladder" => Box::new(Ladder::new(k)),
        "drive" => Box::new(Drive::new(k)),
        "amp" => Box::new(Amp::new(k)),
        "eq" => Box::new(Equaliser::new(k)),
        "compressor" => Box::new(Compressor::new(k)),
        "clipper" => Box::new(Clipper::new(k)),
        "limiter" => Box::new(Limiter::new(k)),
        "delay" => Box::new(Delay::new(k)),
        "chorus" => Box::new(Chorus::new(k)),
        "reverb" => Box::new(Reverb::new(k)),
        "bitcrush" => Box::new(Bitcrush::new(k)),
        "width" => Box::new(Width::new(k)),
        "follower" => Box::new(Follower::new(k)),
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
        "hit_input" => Box::new(PhysicsIn::new(Reads::Hit, k)),
        "live_input" => Box::new(PhysicsIn::new(Reads::Live, k)),
        "scene_input" => Box::new(PhysicsIn::new(Reads::Scene, k)),
        "voice_out" | "fx_out" => Box::new(VoiceOut),
        // Never run: the program fills its register with the incoming sound.
        "fx_in" => Box::new(Mix::new(k)),
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
