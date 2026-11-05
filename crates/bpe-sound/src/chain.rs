//! Both oscillators' layer chains in one note: each oscillator's sound (after its Gain) runs
//! through its Modulation, Filter, Distortion and EQ layers top to bottom, and on to its Level.
//! A [`ChainSpec`] (built with the patch) says which layer reads which ([`ChainGraph`]); each
//! note keeps a [`VoiceChain`] of the layers' memories. Lanes render a block at a time, layer by
//! layer, unless some tap loops back: then every layer moves on one sample at a time, so a loop
//! reads its source exactly one sample late. Settings are read once a block and glided over it.

use crate::BLOCK;
use crate::chain_graph::{ChainGraph, Tap};
use crate::voices::knobs::{self as k, Values};
use crate::voices::note_hz;
use bpe_core::LayerId;
use bpe_dsp::fx::eq::{BandParams, Eq8};
use bpe_dsp::{
    FilterParams, FilterType, PmDelay, Shaper, ShaperParams, ShaperType, StereoFilter, am, db_to_gain, ring,
};
use bpe_kit::sound::{DistKind, FilterKind, ModKind};

/// What a running layer is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayerType {
    Modulation(ModKind),
    Filter(FilterType, bool),
    Distortion(ShaperType, u8),
    /// An EQ's bands as set: whether each is on, its shape, slope and stereo place hold while
    /// the patch stands; frequency, gain and Q come from the knobs every block.
    Eq([BandParams; 8]),
}

/// One running layer: what it is, where its knobs are and where its sound comes from.
#[derive(Clone, Debug)]
pub struct NodeSpec {
    pub id: LayerId,
    pub lane: u8,
    pub ty: LayerType,
    /// Its place among the chain layers (its knobs are [`k::chain`] of it).
    pub place: usize,
    /// The buffer its input is in.
    pub input: usize,
    /// A Modulation layer's source buffer, and whether it loops (read a sample late).
    pub source: Option<(usize, bool)>,
}

/// A patch's chains: the layers in render order and where each lane's sound ends up.
#[derive(Clone, Debug)]
pub struct ChainSpec {
    pub nodes: Vec<NodeSpec>,
    /// Each lane's output buffer.
    pub out: [usize; 2],
    /// Whether some tap loops back (every layer then moves on a sample at a time).
    pub per_sample: bool,
    /// The oscillators that must play for what is heard (a lane tapped by the other plays
    /// though its own switch is off).
    pub raw_needed: [bool; 2],
    pub loops: Vec<LayerId>,
}

/// Buffers: 0 and 1 the oscillators, then one per running layer.
pub const RAW_BUFFERS: usize = 2;

pub fn filter_type(k: FilterKind) -> FilterType {
    match k {
        FilterKind::LowPass => FilterType::LowPass,
        FilterKind::HighPass => FilterType::HighPass,
        FilterKind::BandPass => FilterType::BandPass,
        FilterKind::Notch => FilterType::Notch,
        FilterKind::Peak => FilterType::Peak,
        FilterKind::LowShelf => FilterType::LowShelf,
        FilterKind::HighShelf => FilterType::HighShelf,
        FilterKind::Ladder => FilterType::Ladder,
        FilterKind::Comb => FilterType::Comb,
        FilterKind::Formant => FilterType::Formant,
    }
}

pub fn shaper_type(k: DistKind) -> ShaperType {
    match k {
        DistKind::SoftClip => ShaperType::SoftClip,
        DistKind::HardClip => ShaperType::HardClip,
        DistKind::Tube => ShaperType::Tube,
        DistKind::Tape => ShaperType::Tape,
        DistKind::Diode => ShaperType::Diode,
        DistKind::Overdrive => ShaperType::Overdrive,
        DistKind::Wavefold => ShaperType::Wavefold,
        DistKind::Bitcrush => ShaperType::Bitcrush,
        DistKind::Downsample => ShaperType::Downsample,
        DistKind::Rectify => ShaperType::Rectify,
    }
}

impl ChainSpec {
    /// No layers: both oscillators go straight to their Levels.
    pub fn empty() -> ChainSpec {
        ChainSpec { nodes: Vec::new(), out: [0, 1], per_sample: false, raw_needed: [false; 2], loops: Vec::new() }
    }

    /// The running layers of `graph`, `types` giving each one's type (by its place), for the
    /// oscillators in `heard`. Layers nothing heard needs are left out.
    pub fn build(graph: &ChainGraph, types: &[LayerType], heard: [bool; 2]) -> ChainSpec {
        let (need, raw_needed) = graph.needed(heard);
        let buffer = |t: Tap| match t {
            Tap::Raw(o) => o as usize,
            Tap::Layer(i) => RAW_BUFFERS + i,
        };
        let nodes: Vec<NodeSpec> = graph
            .order
            .iter()
            .filter(|&&i| need[i])
            .map(|&i| NodeSpec {
                id: graph.layers[i].id,
                lane: graph.layers[i].lane,
                ty: types[i],
                place: i,
                input: buffer(graph.input[i]),
                source: graph.source[i].map(|(t, l)| (buffer(t), l)),
            })
            .collect();
        let per_sample = nodes.iter().any(|n| n.source.is_some_and(|(_, l)| l));
        ChainSpec {
            nodes,
            out: [buffer(graph.out[0]), buffer(graph.out[1])],
            per_sample,
            raw_needed,
            loops: graph.loops.clone(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Buffers a note's chain needs: the oscillators', then one per layer by place.
    pub fn buffers(&self) -> usize {
        RAW_BUFFERS + self.nodes.iter().map(|n| n.place + 1).max().unwrap_or(0)
    }
}

/// A number glided over a block.
#[derive(Clone, Copy, Debug)]
struct Glide {
    cur: f32,
    step: f32,
    left: u32,
}

impl Glide {
    fn new() -> Self {
        Glide { cur: 0.0, step: 0.0, left: 0 }
    }

    fn set(&mut self, target: f32, snap: bool) {
        if snap {
            *self = Glide { cur: target, step: 0.0, left: 0 };
        } else {
            self.step = (target - self.cur) / BLOCK as f32;
            self.left = BLOCK as u32;
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        if self.left > 0 {
            self.cur += self.step;
            self.left -= 1;
        }
        self.cur
    }
}

/// The dry sound held back by a distortion's latency, so Mix lines up.
#[derive(Clone, Copy, Debug)]
struct DryDelay {
    buf: [[f32; 16]; 2],
    pos: usize,
    len: usize,
}

impl DryDelay {
    fn new(len: usize) -> Self {
        DryDelay { buf: [[0.0; 16]; 2], pos: 0, len: len.min(15) }
    }

    #[inline]
    fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.buf[0][self.pos] = l;
        self.buf[1][self.pos] = r;
        let at = (self.pos + 16 - self.len) & 15;
        self.pos = (self.pos + 1) & 15;
        (self.buf[0][at], self.buf[1][at])
    }

    fn reset(&mut self) {
        self.buf = [[0.0; 16]; 2];
        self.pos = 0;
    }
}

/// A running layer's memory in one note.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // Kept inline: boxing would allocate when a voice is copied.
enum LayerDsp {
    Modulation { kind: ModKind, pm: [PmDelay; 2], amount: Glide },
    Filter(StereoFilter),
    Distortion(Box<Shaper>, DryDelay),
    Eq(Box<Eq8>, [BandParams; 8]),
}

/// What every layer shares: Output and Mix, glided.
#[derive(Clone, Debug)]
struct Node {
    id: LayerId,
    dsp: LayerDsp,
    output: Glide,
    mix: Glide,
    /// The layer's last sample (a loop reads it at the next block's start).
    last: [f32; 2],
}

impl Node {
    fn new(spec: &NodeSpec) -> Node {
        let dsp = match spec.ty {
            LayerType::Modulation(kind) => {
                LayerDsp::Modulation { kind, pm: [PmDelay::default(); 2], amount: Glide::new() }
            }
            LayerType::Filter(kind, slope24) => LayerDsp::Filter(StereoFilter::new(kind, slope24)),
            LayerType::Distortion(kind, factor) => {
                let s = Shaper::new(kind, factor);
                let lat = s.latency();
                LayerDsp::Distortion(Box::new(s), DryDelay::new(lat))
            }
            LayerType::Eq(bands) => LayerDsp::Eq(Box::new(Eq8::new()), bands),
        };
        Node { id: spec.id, dsp, output: Glide::new(), mix: Glide::new(), last: [0.0; 2] }
    }

    fn same_type(&self, other: &Node) -> bool {
        match (&self.dsp, &other.dsp) {
            (LayerDsp::Modulation { kind: a, .. }, LayerDsp::Modulation { kind: b, .. }) => a == b,
            (LayerDsp::Filter(a), LayerDsp::Filter(b)) => a.kind() == b.kind() && a.slope24() == b.slope24(),
            (LayerDsp::Distortion(a, _), LayerDsp::Distortion(b, _)) => {
                a.kind() == b.kind() && a.factor() == b.factor()
            }
            // Bands turned on or off or changing shape fade in the EQ itself.
            (LayerDsp::Eq(..), LayerDsp::Eq(..)) => true,
            _ => false,
        }
    }

    fn reset(&mut self) {
        match &mut self.dsp {
            LayerDsp::Modulation { pm, .. } => pm.iter_mut().for_each(PmDelay::reset),
            LayerDsp::Filter(f) => f.reset(),
            LayerDsp::Distortion(s, d) => {
                s.reset();
                d.reset();
            }
            LayerDsp::Eq(eq, _) => eq.reset(),
        }
        self.last = [0.0; 2];
        self.output.left = 0;
        self.mix.left = 0;
    }

    /// Takes over another note's memory of the same layer (no allocation).
    fn copy_from(&mut self, other: &Node) {
        if !self.same_type(other) {
            self.reset();
            return;
        }
        match (&mut self.dsp, &other.dsp) {
            (LayerDsp::Modulation { pm, amount, .. }, LayerDsp::Modulation { pm: p2, amount: a2, .. }) => {
                *pm = *p2;
                *amount = *a2;
            }
            (LayerDsp::Filter(f), LayerDsp::Filter(f2)) => f.copy_state_from(f2),
            (LayerDsp::Distortion(s, d), LayerDsp::Distortion(s2, d2)) => {
                s.copy_state_from(s2);
                *d = *d2;
            }
            // The whole EQ as it runs; this block's settings reach it from the new patch.
            (LayerDsp::Eq(eq, _), LayerDsp::Eq(e2, _)) => eq.as_mut().clone_from(e2),
            _ => {}
        }
        self.output = other.output;
        self.mix = other.mix;
        self.last = other.last;
    }

    /// Reads this block's settings (`v` the note's knobs, `pitch` the lane's note).
    fn set(&mut self, spec: &NodeSpec, v: &Values, pitch: f32, snap: bool) {
        let at = |key: usize| v[k::chain(spec.place, key)];
        let pct = |key: usize| (at(key) / 100.0).clamp(0.0, 1.0);
        self.mix.set(pct(k::MIX), snap);
        match &mut self.dsp {
            LayerDsp::Modulation { amount, .. } => {
                amount.set(pct(k::AMOUNT), snap);
                self.output.set(db_to_gain(at(k::OUTPUT)), snap);
            }
            LayerDsp::Filter(f) => {
                let kt = pct(k::KEY_TRACK);
                let cutoff = at(k::CUTOFF).max(1.0);
                let hz = if f.kind() == FilterType::Comb {
                    // At 100 % a comb rings at the note itself.
                    cutoff.powf(1.0 - kt) * note_hz(pitch).powf(kt)
                } else {
                    cutoff * (kt * (pitch - 60.0) / 12.0).exp2()
                };
                f.set(&FilterParams {
                    kind: f.kind(),
                    slope24: f.slope24(),
                    cutoff_hz: hz,
                    reso: pct(k::RESO),
                    drive: pct(k::DRIVE),
                    gain_db: at(k::CHAIN_GAIN),
                    vowel: at(k::VOWEL),
                    shift: at(k::SHIFT),
                });
                self.output.set(1.0, true);
            }
            LayerDsp::Distortion(s, _) => {
                s.set(&ShaperParams {
                    drive_db: at(k::DRIVE),
                    bits: at(k::BITS),
                    rate_hz: at(k::RATE),
                    bias: (at(k::BIAS) / 100.0).clamp(-1.0, 1.0),
                    tone: (at(k::TONE) / 100.0).clamp(-1.0, 1.0),
                    output_db: at(k::OUTPUT),
                });
                // The shaper takes its own Output.
                self.output.set(1.0, true);
            }
            LayerDsp::Eq(eq, fixed) => {
                let bands = std::array::from_fn(|b| BandParams {
                    freq: at(k::band_key(b, k::BAND_FREQ)),
                    gain_db: at(k::band_key(b, k::BAND_GAIN)),
                    q: at(k::band_key(b, k::BAND_Q)),
                    ..fixed[b]
                });
                eq.set(&bands);
                // The EQ glides its own Output.
                eq.set_output_db(at(k::OUTPUT));
                if snap {
                    eq.reset();
                }
                self.output.set(1.0, true);
            }
        }
    }

    /// One sample: `x` the input, `m` the source.
    #[inline]
    fn tick(&mut self, x: [f32; 2], m: [f32; 2]) -> [f32; 2] {
        let out = self.output.next();
        let mix = self.mix.next();
        let (wet, dry) = match &mut self.dsp {
            LayerDsp::Modulation { kind, pm, amount } => {
                let a = amount.next();
                let mut wet = [0.0; 2];
                let mut dry = x;
                for c in 0..2 {
                    (wet[c], dry[c]) = match kind {
                        ModKind::Fm => pm[c].fm(x[c], m[c], a),
                        ModKind::PhaseDistortion => pm[c].pd(x[c], m[c], a),
                        ModKind::Am => (am(x[c], m[c], a), x[c]),
                        ModKind::Ring => (ring(x[c], m[c], a), x[c]),
                    };
                }
                (wet, dry)
            }
            LayerDsp::Filter(f) => {
                let (l, r) = f.tick(x[0], x[1]);
                ([l, r], x)
            }
            LayerDsp::Distortion(s, d) => {
                let (l, r) = s.tick(x[0], x[1]);
                let (dl, dr) = d.tick(x[0], x[1]);
                ([l, r], [dl, dr])
            }
            LayerDsp::Eq(eq, _) => {
                let (l, r) = eq.tick(x[0], x[1]);
                ([l, r], x)
            }
        };
        [dry[0] + mix * (wet[0] * out - dry[0]), dry[1] + mix * (wet[1] * out - dry[1])]
    }
}

/// One stereo buffer.
pub type Buf = [[f32; BLOCK]; 2];

/// Both lanes' layers in one note.
#[derive(Clone, Debug, Default)]
pub struct VoiceChain {
    nodes: Vec<Node>,
    fresh: bool,
}

impl VoiceChain {
    /// The layers of `spec` (allocates: build it off the audio path).
    pub fn new(spec: &ChainSpec) -> VoiceChain {
        VoiceChain { nodes: spec.nodes.iter().map(Node::new).collect(), fresh: true }
    }

    /// No layers (no allocation).
    pub fn empty() -> VoiceChain {
        VoiceChain { nodes: Vec::new(), fresh: true }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Silences its layers' memories (a new note or a seek).
    pub fn reset(&mut self) {
        self.nodes.iter_mut().for_each(Node::reset);
        self.fresh = true;
    }

    /// Takes over what `old` (the same note before the patch changed) remembers, layer by
    /// layer: a layer still there of the same type keeps ringing, a new one starts silent.
    pub fn adopt(&mut self, old: &VoiceChain) {
        for n in &mut self.nodes {
            match old.nodes.iter().find(|o| o.id == n.id) {
                Some(o) => n.copy_from(o),
                None => n.reset(),
            }
        }
        self.fresh = old.fresh;
    }

    /// Runs one block. `bufs[0]` and `bufs[1]` hold the oscillators' sound (after Gain); each
    /// layer writes its own buffer, and `spec.out` names where each lane's sound ends up. `v` are
    /// the note's knobs; `pitch` each lane's note (MIDI, for key tracking).
    pub fn render(&mut self, spec: &ChainSpec, v: &Values, pitch: [f32; 2], bufs: &mut [Buf]) {
        if self.nodes.is_empty() {
            return;
        }
        let snap = std::mem::replace(&mut self.fresh, false);
        for (n, s) in self.nodes.iter_mut().zip(&spec.nodes) {
            n.set(s, v, pitch[s.lane as usize], snap);
        }
        if spec.per_sample {
            for t in 0..BLOCK {
                for i in 0..self.nodes.len() {
                    self.step(spec, i, t, bufs);
                }
            }
        } else {
            for i in 0..self.nodes.len() {
                for t in 0..BLOCK {
                    self.step(spec, i, t, bufs);
                }
            }
        }
        for (n, s) in self.nodes.iter_mut().zip(&spec.nodes) {
            let b = &bufs[RAW_BUFFERS + s.place];
            n.last = [b[0][BLOCK - 1], b[1][BLOCK - 1]];
        }
    }

    #[inline]
    fn step(&mut self, spec: &ChainSpec, i: usize, t: usize, bufs: &mut [Buf]) {
        let s = &spec.nodes[i];
        let x = [bufs[s.input][0][t], bufs[s.input][1][t]];
        let m = match s.source {
            None => [0.0; 2],
            Some((b, false)) => [bufs[b][0][t], bufs[b][1][t]],
            Some((b, true)) => {
                if t > 0 {
                    [bufs[b][0][t - 1], bufs[b][1][t - 1]]
                } else {
                    // The source's last sample of the block before.
                    self.last_of(spec, b)
                }
            }
        };
        let y = self.nodes[i].tick(x, m);
        let o = &mut bufs[RAW_BUFFERS + s.place];
        o[0][t] = y[0];
        o[1][t] = y[1];
    }

    /// The last sample of buffer `b` in the block before (silence for an oscillator).
    fn last_of(&self, spec: &ChainSpec, b: usize) -> [f32; 2] {
        if b < RAW_BUFFERS {
            return [0.0; 2];
        }
        spec.nodes.iter().position(|n| RAW_BUFFERS + n.place == b).map(|i| self.nodes[i].last).unwrap_or([0.0; 2])
    }
}
