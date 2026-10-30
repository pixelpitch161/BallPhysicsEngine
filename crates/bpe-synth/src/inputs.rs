//! What the balls do, as the input blocks read it: values fixed at the hit that started a voice,
//! a ball's track while the voice sounds, and scene-wide series on the video clock. Nothing
//! here knows about scenes; the mixer fills these from the run's log.

use crate::BLOCK;
use bpe_graph::{Graph, Param};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

/// One input an input block can read, with the physics range Follow… maps to 0 and 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputDef {
    pub name: &'static str,
    pub label: &'static str,
    pub low: f32,
    pub high: f32,
}

const fn def(name: &'static str, label: &'static str, low: f32, high: f32) -> InputDef {
    InputDef { name, label, low, high }
}

/// Fixed when the note starts. Raw units: px/s, degrees, px, seconds; the rest 0 to 1.
pub const HIT_INPUTS: &[InputDef] = &[
    def("speed", "Hit speed", 0.0, 1200.0),
    def("angle", "Hit angle", 0.0, 90.0),
    def("side", "Side hit", 0.0, 1.0),
    def("tag", "Hit thing's tag", 0.0, 1.0),
    def("size", "Ball size", 0.0, 60.0),
    def("hue", "Colour hue", 0.0, 1.0),
    def("brightness", "Colour brightness", 0.0, 1.0),
    def("height", "Height in the frame", 0.0, 1.0),
    def("left_right", "Left-right in the frame", 0.0, 1.0),
    def("hits", "Hit count", 0.0, 20.0),
    def("age", "Age", 0.0, 30.0),
];

/// Followed while the voice sounds.
pub const LIVE_INPUTS: &[InputDef] = &[
    def("speed", "Speed", 0.0, 1200.0),
    def("height", "Height in the frame", 0.0, 1.0),
    def("left_right", "Left-right in the frame", 0.0, 1.0),
    def("distance", "Distance from the camera centre", 0.0, 960.0),
];

/// The whole scene, on the video clock. Channel level is in dBFS.
pub const SCENE_INPUTS: &[InputDef] = &[
    def("balls", "Ball count", 0.0, 50.0),
    def("energy", "Motion energy", 0.0, 5.0),
    def("hit_rate", "Hits per second", 0.0, 20.0),
    def("beat", "Beat phase", 0.0, 1.0),
    def("bar", "Bar phase", 0.0, 1.0),
    def("chord_root", "Chord root", 0.0, 1.0),
    def("chord_quality", "Chord quality", 0.0, 1.0),
    def("bass", "Song bass", 0.0, 1.0),
    def("mid", "Song mid", 0.0, 1.0),
    def("high", "Song high", 0.0, 1.0),
    def("channel", "Channel level", -48.0, -6.0),
];

/// The table for an input block kind.
pub fn inputs_of(kind: &str) -> &'static [InputDef] {
    match kind {
        "hit_input" => HIT_INPUTS,
        "live_input" => LIVE_INPUTS,
        "scene_input" => SCENE_INPUTS,
        _ => &[],
    }
}

/// Beats in a bar, for Bar phase.
const BEATS_PER_BAR: f64 = 4.0;
/// What a channel level reads when there is nothing there, in dBFS.
pub const SILENT_DB: f32 = -90.0;

fn hash_f32s(h: &mut DefaultHasher, values: &[f32]) {
    for v in values {
        v.to_bits().hash(h);
    }
}

/// A ball's live values after a hit, sampled evenly from the moment the note starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// Samples per second.
    pub rate: f32,
    /// [`LIVE_INPUTS`] order; sample `i` is `i / rate` seconds after the note starts.
    pub samples: Vec<[f32; 4]>,
}

impl Track {
    /// The values `seconds` after the note started, blended between samples. The last sample
    /// holds once the recording ends; an empty track is all zeros.
    pub fn at(&self, seconds: f32) -> [f32; 4] {
        let Some(last) = self.samples.last() else { return [0.0; 4] };
        let x = (seconds.max(0.0) * self.rate.max(1e-6)).min(f32::MAX);
        let i = x.floor() as usize;
        if !x.is_finite() || i + 1 >= self.samples.len() {
            return *last;
        }
        let f = x - i as f32;
        let (a, b) = (self.samples[i], self.samples[i + 1]);
        std::array::from_fn(|k| a[k] + (b[k] - a[k]) * f)
    }
}

/// What a hit gives the voice it starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitPhysics {
    /// [`HIT_INPUTS`] order. The tag slot is unused: the block checks `tags`.
    pub values: [f32; 11],
    /// The tags of the thing it hit.
    pub tags: Vec<String>,
    pub track: Option<Arc<Track>>,
}

impl HitPhysics {
    /// A hash of everything a voice can read from it.
    pub fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        hash_f32s(&mut h, &self.values);
        self.tags.hash(&mut h);
        if let Some(t) = &self.track {
            t.rate.to_bits().hash(&mut h);
            for s in &t.samples {
                hash_f32s(&mut h, s);
            }
        }
        h.finish()
    }
}

/// The scene-wide series, on the video clock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneSignals {
    /// Samples per second of the series below.
    pub rate: f32,
    pub balls: Vec<f32>,
    pub energy: Vec<f32>,
    pub hit_rate: Vec<f32>,
    pub bass: Vec<f32>,
    pub mid: Vec<f32>,
    pub high: Vec<f32>,
    /// The beat grid, in video seconds.
    pub beats: Vec<f64>,
    /// The chord's root and quality, each 0 to 1, sampled at `rate`; they step, not blend.
    pub chords: Vec<[f32; 2]>,
    /// Each channel's level by module name, in dBFS per [`BLOCK`] frames at 48 kHz.
    pub levels: BTreeMap<String, Arc<Vec<f32>>>,
}

/// `series` at `time`, blended, 0 outside it.
fn blend(series: &[f32], rate: f32, time: f64) -> f32 {
    if series.is_empty() || !time.is_finite() || time < 0.0 || rate <= 0.0 {
        return 0.0;
    }
    let x = time * rate as f64;
    let i = x.floor() as usize;
    if i >= series.len() {
        return *series.last().unwrap_or(&0.0);
    }
    let j = (i + 1).min(series.len() - 1);
    let f = (x - i as f64) as f32;
    series[i] + (series[j] - series[i]) * f
}

/// How far through the beat (`per` = 1) or bar (`per` = 4) `time` is, 0 before the first beat.
/// After the last beat the last gap repeats.
fn phase(beats: &[f64], time: f64, per: f64) -> f32 {
    if beats.is_empty() || !time.is_finite() || time < beats[0] {
        return 0.0;
    }
    let i = beats.partition_point(|b| *b <= time) - 1;
    let gap = if i + 1 < beats.len() {
        beats[i + 1] - beats[i]
    } else if beats.len() >= 2 {
        beats[beats.len() - 1] - beats[beats.len() - 2]
    } else {
        return 0.0;
    };
    if gap <= 0.0 {
        return 0.0;
    }
    let beat = (i as f64) + ((time - beats[i]) / gap).max(0.0);
    ((beat / per).fract()) as f32
}

impl SceneSignals {
    /// The raw value of a scene input at `time` (video seconds). `channel` names the channel a
    /// Channel level reads.
    pub fn value(&self, input: &str, channel: &str, time: f64) -> f32 {
        match input {
            "balls" => blend(&self.balls, self.rate, time),
            "energy" => blend(&self.energy, self.rate, time),
            "hit_rate" => blend(&self.hit_rate, self.rate, time),
            "bass" => blend(&self.bass, self.rate, time),
            "mid" => blend(&self.mid, self.rate, time),
            "high" => blend(&self.high, self.rate, time),
            "beat" => phase(&self.beats, time, 1.0),
            "bar" => phase(&self.beats, time, BEATS_PER_BAR),
            "chord_root" | "chord_quality" => {
                if self.chords.is_empty() || !time.is_finite() || time < 0.0 || self.rate <= 0.0 {
                    return 0.0;
                }
                let i = ((time * self.rate as f64) as usize).min(self.chords.len() - 1);
                self.chords[i][(input == "chord_quality") as usize]
            }
            "channel" => {
                let Some(levels) = self.levels.get(channel) else { return SILENT_DB };
                if !time.is_finite() || time < 0.0 {
                    return SILENT_DB;
                }
                let i = (time * 48_000.0 / BLOCK as f64) as usize;
                levels.get(i).copied().unwrap_or(SILENT_DB)
            }
            _ => 0.0,
        }
    }

    /// A hash of everything a graph can read from it.
    pub fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.rate.to_bits().hash(&mut h);
        for s in [&self.balls, &self.energy, &self.hit_rate, &self.bass, &self.mid, &self.high] {
            s.len().hash(&mut h);
            hash_f32s(&mut h, s);
        }
        for b in &self.beats {
            b.to_bits().hash(&mut h);
        }
        for c in &self.chords {
            hash_f32s(&mut h, c);
        }
        for (name, l) in &self.levels {
            name.hash(&mut h);
            hash_f32s(&mut h, l);
        }
        h.finish()
    }
}

/// Everything input blocks can read, handed to a voice or an effects run when it starts.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub hit: Option<Arc<HitPhysics>>,
    pub scene: Option<Arc<SceneSignals>>,
}

/// Which inputs a graph reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Uses {
    pub hit: bool,
    pub live: bool,
    pub scene: bool,
    /// The channels its Channel level blocks read, by name.
    pub channels: BTreeSet<String>,
}

/// Which inputs the switched-on blocks of `graph` read.
pub fn uses(graph: &Graph) -> Uses {
    let mut u = Uses::default();
    for n in graph.nodes.iter().filter(|n| n.enabled) {
        match n.kind.as_str() {
            "hit_input" => u.hit = true,
            "live_input" => u.live = true,
            "scene_input" => {
                u.scene = true;
                let input = match n.params.get("input") {
                    Some(Param::Choice(c)) | Some(Param::Text(c)) => c.as_str(),
                    _ => SCENE_INPUTS[0].name,
                };
                if input == "channel"
                    && let Some(Param::Text(c)) = n.params.get("channel")
                {
                    u.channels.insert(c.clone());
                }
            }
            _ => {}
        }
    }
    u
}
