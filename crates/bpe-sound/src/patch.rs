//! Everything the engine reads from the scene, built off the audio path so building may
//! allocate, and swapped in between blocks.

use crate::buses::{BusesSpec, ShapeRoute};
use crate::chain::{ChainSpec, LayerType, filter_type, shaper_type};
use crate::chain_graph::{ChainGraph, chain_layers};
use crate::master::MasterKeys;
use crate::modmatrix::ModMatrix;
use crate::voices::knobs;
use crate::voices::shape::{DistSpec, EnvSpec, HitSpec, LfoSpec, LightSpec, ModsSpec, OscSpec, Probe, ShapeSpec};
use crate::voices::slots::NotesSpec;
use crate::{ShapeVoice, TempoMap, VoiceSource};
use bpe_core::{Address, LayerId, Resolved, ResolvedSlot, Scene, Settings, SlotId, ThingId, Value};
use bpe_dsp::{LoopMode, SampleData, SamplerParams, TableSet, WarpModeId, WaveBank, WaveShape};
use bpe_kit::sound::{
    ADSR, AdsrSettings, CHAIN_KINDS, ChainEqSettings, DISTANCE, DistanceSettings, DistortionSettings, EQ, FILTER,
    FilterSettings, HIT, HitSettings, LFO, LIGHT_MOD, LfoSettings, LightModSettings, MODULATION, MixerSettings,
    ModulationSettings, OscSettings, SampleLoop, Slope, VOICE, VoiceSettings, Wave,
};
use bpe_kit::things::shape::ShapeSettings;
use bpe_kit::{Kit, SceneSettings};
use std::sync::Arc;

/// A scene's sound settings, ready for the engine.
pub struct SoundPatch {
    pub master: MixerSettings,
    pub master_keys: MasterKeys,
    pub tempo: TempoMap,
    pub voices: Vec<Box<dyn VoiceSource>>,
    /// Where each voice's sound goes, by the voice's place.
    pub routes: Vec<ShapeRoute>,
    /// The three buses and their effects.
    pub buses: BusesSpec,
    /// Some wavetable was still baking ([`Tables::Ready`]): those oscillators are silent until
    /// the patch is built again.
    pub pending: bool,
    /// Where the clip ends (the scene's Duration), for the Master's End fade.
    pub end: f64,
    /// The Mixer's backing track, when it has one that is on and decoded (W11).
    pub backing: Option<Backing>,
}

/// A backing track ready to play: the song, the sound-time sample its first frame plays on, and
/// its level.
#[derive(Clone, Debug)]
pub struct Backing {
    pub audio: Arc<SampleData>,
    pub start: i64,
    pub gain: f32,
}

impl Backing {
    /// The backing track named by the Mixer settings `m`, when it is on and the scene keeps it.
    pub fn of(scene: &Scene, m: &MixerSettings, tables: Tables, pending: &mut bool) -> Option<Backing> {
        if !m.backing_on || m.backing.is_empty() {
            return None;
        }
        let audio = tables.sample(scene, &m.backing, pending)?;
        let start = (m.backing_at as f64 * crate::SAMPLE_RATE as f64).round() as i64;
        Some(Backing { audio, start, gain: bpe_dsp::db_to_gain(m.backing_level) })
    }

    /// Adds the song's block starting at sound-time sample `at` to `l`/`r`. Never allocates.
    pub fn add(&self, at: u64, l: &mut [f32; crate::BLOCK], r: &mut [f32; crate::BLOCK]) {
        let first = at as i64 - self.start;
        let frames = self.audio.frames() as i64;
        if first >= frames || first + crate::BLOCK as i64 <= 0 {
            return;
        }
        let (sl, sr) = self.audio.channels();
        for i in 0..crate::BLOCK {
            let j = first + i as i64;
            if (0..frames).contains(&j) {
                l[i] += sl[j as usize] * self.gain;
                r[i] += sr[j as usize] * self.gain;
            }
        }
    }
}

/// How a patch gets its wavetables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tables {
    /// Baked there and then if need be (the export and tests).
    Bake,
    /// Only those already baked; the rest start baking in the background (the editor, which
    /// must never wait).
    Ready,
    /// None at all: only the settings are wanted (Follow sound reads no waves).
    Skip,
}

impl Tables {
    fn get(self, wave: WaveShape, mode: WarpModeId, pending: &mut bool) -> Option<Arc<TableSet>> {
        match self {
            Tables::Bake => Some(WaveBank::get(wave, mode)),
            Tables::Ready => {
                let set = WaveBank::ready(wave, mode);
                *pending |= set.is_none();
                set
            }
            Tables::Skip => None,
        }
    }

    /// The decoded sound file the scene keeps as `name` (W11), the same way: decoded there and
    /// then, only if ready (else it starts decoding and the patch is pending), or none. A missing
    /// or unreadable file is none and never pending.
    pub(crate) fn sample(self, scene: &Scene, name: &str, pending: &mut bool) -> Option<Arc<SampleData>> {
        let bytes = &scene.audio.get(name)?.bytes;
        match self {
            Tables::Bake => crate::SampleBank::get(bytes).ok(),
            Tables::Ready => match crate::SampleBank::ready(bytes) {
                Some(d) => d.ok(),
                None => {
                    *pending = true;
                    None
                }
            },
            Tables::Skip => None,
        }
    }
}

/// The table a wave reads, for a tabled wave that plays (`runs`: it is on, or the other
/// oscillator's layers tap it).
fn table_of(o: &OscSettings, runs: bool, tables: Tables, pending: &mut bool) -> Option<Arc<TableSet>> {
    if !runs {
        return None;
    }
    let wave = match o.wave {
        Wave::Sine => WaveShape::Sine,
        Wave::Triangle => WaveShape::Triangle,
        Wave::Saw => WaveShape::Saw,
        Wave::Square => WaveShape::Square,
        Wave::Noise | Wave::Sample => return None,
    };
    tables.get(wave, WarpModeId::from_index(o.warp_mode.index()), pending)
}

/// How a sample oscillator plays its sample.
pub(crate) fn sampler_params(o: &OscSettings) -> SamplerParams {
    let frames_per_ms = crate::SAMPLE_RATE as f64 / 1000.0;
    SamplerParams {
        start: o.sample_start as f64 / 100.0,
        loop_mode: match o.sample_loop {
            SampleLoop::Off => LoopMode::Off,
            SampleLoop::Forward => LoopMode::Forward,
            SampleLoop::PingPong => LoopMode::PingPong,
        },
        loop_start: o.sample_loop_start as f64 / 100.0,
        loop_end: o.sample_loop_end as f64 / 100.0,
        fade_frames: o.sample_loop_fade.max(0.0) as f64 * frames_per_ms,
        reverse: o.sample_reverse,
    }
}

fn osc_spec(scene: &Scene, o: &OscSettings, runs: bool, tables: Tables, pending: &mut bool) -> OscSpec {
    let sample = if runs && o.wave == Wave::Sample { tables.sample(scene, &o.sample, pending) } else { None };
    OscSpec {
        sampler: sampler_params(o),
        sample_root: o.sample_root.clamp(0, 127) as f32,
        sample_follow: o.sample_follow,
        sample,
        on: o.on,
        wave: o.wave,
        noise: o.noise,
        phase_mode: o.phase_mode,
        sub: o.sub,
        sub_octave: o.sub_octave,
        table: table_of(o, runs, tables, pending),
    }
}

/// The voice of the shape `id` from its resolved Sound card, or `None` when its sound is off.
/// Its wavetables are baked if need be.
pub fn shape_voice(scene: &Scene, id: ThingId, card: &ResolvedSlot, index: usize, width: f32) -> Option<ShapeVoice> {
    let shape = shape_settings(scene, id);
    let top = scene_top(scene);
    let b = Build { index, width, shape: &shape, tempo_top: top, light: None };
    voice_with(scene, id, card, &b, Tables::Bake, &mut false)
}

/// The shape's own settings as set (its outline is all the voice reads of them).
fn shape_settings(scene: &Scene, id: ThingId) -> ShapeSettings {
    scene.things.get(&id).map(|t| ShapeSettings::from_params(&t.params)).unwrap_or_default()
}

fn scene_top(scene: &Scene) -> u32 {
    scene.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params).time_top).unwrap_or(4)
}

/// What a voice is built with besides its card.
struct Build<'a> {
    index: usize,
    width: f32,
    shape: &'a ShapeSettings,
    tempo_top: u32,
    /// The scene's light probe, for Light modulators.
    light: Option<Probe>,
}

/// The light probe a patch's Light modulators read: made when something in the scene senses
/// light and the scene has a Lighting card (without one they read nothing). Lights that follow
/// sound are read as set here (the sound can't hear itself through the light).
fn probe_of(scene: &Scene, resolved: &Resolved) -> Option<Probe> {
    let senses = bpe_kit::sensing::has_lighting(scene) && bpe_kit::sensing::uses_light(scene);
    senses.then(|| Probe(bpe_kit::sensing::probe(scene, resolved, None)))
}

fn voice_with(
    scene: &Scene,
    id: ThingId,
    card: &ResolvedSlot,
    b: &Build,
    tables: Tables,
    pending: &mut bool,
) -> Option<ShapeVoice> {
    spec_with(scene, id, card, b, tables, pending).map(ShapeVoice::new)
}

/// Every sounding shape's voice settings, in scene order, without wavetables: what Follow sound
/// reads.
pub fn shape_specs(scene: &Scene, resolved: &Resolved) -> Vec<ShapeSpec> {
    let settings =
        resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    let mut out = Vec::new();
    let light = probe_of(scene, resolved);
    for id in &scene.order {
        let Some(thing) = resolved.things.get(id) else { continue };
        let Some(card) = thing.slots.get(&SlotId::Sound) else { continue };
        let shape = ShapeSettings::from_params(&thing.params);
        let top = TempoMap::from_settings(&settings).top();
        let b = Build {
            index: out.len(),
            width: settings.width as f32,
            shape: &shape,
            tempo_top: top,
            light: light.clone(),
        };
        if let Some(spec) = spec_with(scene, *id, card, &b, Tables::Skip, &mut false) {
            out.push(spec);
        }
    }
    out
}

/// Thing `id`'s spec alone, without the light probe (what a rule can reach of its voice; the
/// editor's menu asks for it as it draws).
pub(crate) fn spec_of(scene: &Scene, resolved: &Resolved, id: ThingId) -> Option<ShapeSpec> {
    let settings =
        resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    let thing = resolved.things.get(&id)?;
    let card = thing.slots.get(&SlotId::Sound)?;
    let shape = ShapeSettings::from_params(&thing.params);
    let top = TempoMap::from_settings(&settings).top();
    let b = Build { index: 0, width: settings.width as f32, shape: &shape, tempo_top: top, light: None };
    spec_with(scene, id, card, &b, Tables::Skip, &mut false)
}

fn spec_with(
    scene: &Scene,
    id: ThingId,
    card: &ResolvedSlot,
    b: &Build,
    tables: Tables,
    pending: &mut bool,
) -> Option<ShapeSpec> {
    let Build { index, width, shape, tempo_top, .. } = *b;
    if card.kind != VOICE {
        return None;
    }
    let v = VoiceSettings::from_params(&card.params);
    if !v.enabled {
        return None;
    }
    let envs: Vec<EnvSpec> = card
        .layers
        .iter()
        .filter(|l| l.kind == ADSR)
        .take(knobs::MAX_ENVS)
        .map(|l| {
            let a = AdsrSettings::from_params(&card.layer_params(l.id));
            EnvSpec { id: l.id, gate_mode: a.gate_mode, gate_unit: a.gate_unit }
        })
        .collect();
    let track = |key: &str| {
        let a = Address { thing: id, slot: Some(SlotId::Sound), key: key.into() };
        scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).cloned()
    };
    let ids: Vec<_> = envs.iter().map(|e| e.id).collect();
    let (graph, chain) = chain_of(scene, id, card, [v.a.on, v.b.on]);
    let chain_ids: Vec<(LayerId, &str)> = graph.layers.iter().map(|l| (l.id, l.kind.as_str())).collect();
    let mut mods = mods_of(id, card, &v.routes);
    if !mods.lights.is_empty() {
        mods.probe = b.light.clone();
    }
    let mod_ids = knobs::ModIds {
        lfos: mods.lfos.iter().map(|m| m.id).collect(),
        dists: mods.dists.iter().map(|m| m.id).collect(),
        hits: mods.hits.iter().map(|m| m.id).collect(),
        lights: mods.lights.iter().map(|m| m.id).collect(),
    };
    let layout = knobs::layout_all(&ids, &chain_ids, &mod_ids);
    let base = knobs::values(&layout, &|key| card.params.get(key).cloned());
    let mut knob_keys: Vec<(String, usize)> = layout.iter().map(|(key, _, i)| (key.clone(), *i)).collect();
    knob_keys.sort();
    knob_keys.dedup_by(|a, b| a.0 == b.0);
    let keyed = layout
        .iter()
        .filter_map(|(key, _, i)| {
            let a = Address { thing: id, slot: Some(SlotId::Sound), key: key.clone() };
            scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).map(|t| (*i, t.clone()))
        })
        .collect();
    // Only modulators that run move knobs: a switched-off one's routes rest (kept on the card for
    // when it is on again), instead of reading as 0 and, on a gain, silencing the shape.
    let runs = |l: &LayerId| {
        ids.contains(l)
            || mod_ids.lfos.contains(l)
            || mod_ids.dists.contains(l)
            || mod_ids.hits.contains(l)
            || mod_ids.lights.contains(l)
    };
    let routes: Vec<_> = v.routes.iter().filter(|r| runs(&r.source)).cloned().collect();
    let matrix = ModMatrix::build(&routes, &knobs::targets(&layout), &ids);
    Some(ShapeSpec {
        thing: id,
        index,
        osc: [
            osc_spec(scene, &v.a, v.a.on || chain.raw_needed[0], tables, pending),
            osc_spec(scene, &v.b, v.b.on || chain.raw_needed[1], tables, pending),
        ],
        envs,
        play_mode: v.play_mode,
        play_when: v.play_when,
        zone: zone_of(&v, &mods),
        heard_from: v.play_heard_from.clone(),
        balls: scene.things.get(&id).and_then(|t| crate::voices::BallMoments::of_thing(&t.kind, &v)),
        slots: v.notes_slots.clone(),
        notes: NotesSpec::from_settings(
            &v,
            tempo_top,
            [track("notes_strum"), track("notes_every_seconds"), track("notes_jump")],
        ),
        // Only a shape has sides (a Text's hits read side 0).
        side_path: (v.notes_plays == bpe_kit::sound::Plays::BySide
            && scene.things.get(&id).is_some_and(|t| t.kind == bpe_kit::things::shape::KIND))
        .then(|| shape.outline_path()),
        bus: v.out_bus.clamp(1, 3) as u8,
        base,
        keyed,
        knob_keys,
        matrix,
        square: if [&v.a, &v.b]
            .iter()
            .zip(chain.raw_needed)
            .any(|(o, tapped)| (o.on || tapped) && o.sub != bpe_kit::sound::SubWave::Off)
        {
            tables.get(WaveShape::Square, WarpModeId::Sync, pending)
        } else {
            None
        },
        width,
        chain: Arc::new(chain),
        mods,
    })
}

/// Whether a modulator can reach the sound card's knob `key` (switched-off layers' knobs too):
/// the oscillators', the envelopes', the chain layers', the other modulators' and Output's
/// number knobs.
pub fn routable(card: &bpe_core::Slot, key: &str) -> bool {
    let ids = |kind: &str, most: usize| -> Vec<LayerId> {
        card.layers.iter().filter(|l| l.kind == kind).take(most).map(|l| l.id).collect()
    };
    let envs = ids(ADSR, knobs::MAX_ENVS);
    let chain: Vec<(LayerId, &str)> = card
        .layers
        .iter()
        .filter(|l| CHAIN_KINDS.contains(&l.kind.as_str()))
        .map(|l| (l.id, l.kind.as_str()))
        .collect();
    let mods = knobs::ModIds {
        lfos: ids(LFO, knobs::MAX_EACH),
        dists: ids(DISTANCE, knobs::MAX_EACH),
        hits: ids(HIT, knobs::MAX_EACH),
        lights: ids(LIGHT_MOD, knobs::MAX_EACH),
    };
    knobs::layout_all(&envs, &chain, &mods).iter().any(|(k, _, _)| k == key)
}

/// The Distance whose range holds the notes for In range: the one named by its layer id, or
/// the first that is on.
fn zone_of(v: &VoiceSettings, mods: &ModsSpec) -> Option<usize> {
    if v.play_when != bpe_kit::sound::PlayWhen::InRange || mods.dists.is_empty() {
        return None;
    }
    let named = v.play_zone.trim().parse::<u32>().ok();
    named.and_then(|id| mods.dists.iter().position(|d| d.id.0 == id)).or(Some(0))
}

/// The card's LFO, Distance, Hit and Light layers that are on (up to eight of each, in card order),
/// and the order they run in.
fn mods_of(id: ThingId, card: &ResolvedSlot, routes: &[bpe_core::ModRoute]) -> ModsSpec {
    let of = |kind: &'static str| card.layers.iter().filter(move |l| l.kind == kind).take(knobs::MAX_EACH);
    let lfos: Vec<LfoSpec> = of(LFO)
        .map(|l| {
            let settings = LfoSettings::from_params(&card.layer_params(l.id));
            LfoSpec {
                id: l.id,
                curve: crate::mods::lfo::curve_of(&settings),
                settings,
                seed: crate::mods::lfo::seed_of(id, l.id),
            }
        })
        .collect();
    let dists: Vec<DistSpec> = of(DISTANCE)
        .map(|l| {
            let settings = DistanceSettings::from_params(&card.layer_params(l.id));
            DistSpec { id: l.id, heard: Arc::new(settings.heard_from.clone()), settings }
        })
        .collect();
    let hits: Vec<HitSpec> =
        of(HIT).map(|l| HitSpec { id: l.id, pick: HitSettings::from_params(&card.layer_params(l.id)).pick }).collect();
    let lights: Vec<LightSpec> = of(LIGHT_MOD)
        .map(|l| {
            let settings = LightModSettings::from_params(&card.layer_params(l.id));
            LightSpec { id: l.id, heard: Arc::new(settings.heard_from.clone()), settings }
        })
        .collect();
    let running: Vec<LayerId> = card
        .layers
        .iter()
        .map(|l| l.id)
        .filter(|l| {
            lfos.iter().any(|m| m.id == *l)
                || dists.iter().any(|m| m.id == *l)
                || hits.iter().any(|m| m.id == *l)
                || lights.iter().any(|m| m.id == *l)
        })
        .collect();
    let (order, _) = crate::mods::mod_order(routes, &running);
    ModsSpec { lfos, dists, hits, lights, order, probe: None }
}

/// The shape's layer chains: their structure from the scene's card (switched-off layers keep
/// their places), their values from the resolved card, for the oscillators in `heard`.
fn chain_of(scene: &Scene, id: ThingId, card: &ResolvedSlot, heard: [bool; 2]) -> (ChainGraph, ChainSpec) {
    let Some(own) = scene.card(id, SlotId::Sound) else {
        let graph = ChainGraph::build(&[]);
        return (graph, ChainSpec::empty());
    };
    let get = |k: &str| card.params.get(k).or_else(|| own.params.get(k)).cloned();
    let graph = ChainGraph::build(&chain_layers(&own.layers, &get));
    let types: Vec<LayerType> = graph
        .layers
        .iter()
        .map(|l| {
            let p = card.layer_params(l.id);
            match l.kind.as_str() {
                MODULATION => LayerType::Modulation(ModulationSettings::from_params(&p).kind),
                FILTER => {
                    let f = FilterSettings::from_params(&p);
                    LayerType::Filter(filter_type(f.kind), f.slope == Slope::Db24)
                }
                EQ => LayerType::Eq(crate::fx_chain::bands_of(ChainEqSettings::from_params(&p).bands())),
                _ => {
                    let d = DistortionSettings::from_params(&p);
                    LayerType::Distortion(shaper_type(d.kind), d.quality.factor())
                }
            }
        })
        .collect();
    let chain = ChainSpec::build(&graph, &types, heard);
    (graph, chain)
}

/// The scene settings a patch reads (the tempo, the width the pans spread over, where the clip
/// ends for the end fade).
pub const PATCH_SCENE_KEYS: [&str; 6] = ["tempo", "time_top", "time_bottom", "first_beat", "width", "duration"];

/// A hash of everything [`SoundPatch::build`] reads of the scene and nothing else, so the editor
/// builds the patch again only when the sound could have changed (not on a colour drag). The
/// hash is the process's own: never saved.
pub fn patch_fingerprint(scene: &Scene, resolved: &Resolved) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut put = |what: &dyn std::fmt::Debug| format!("{what:?}").hash(&mut h);
    let root = resolved.things.get(&ThingId::ROOT);
    for key in PATCH_SCENE_KEYS {
        put(&root.and_then(|r| r.params.get(key)));
    }
    put(&root.and_then(|r| r.slots.get(&SlotId::Mixer)));
    put(&scene.card(ThingId::ROOT, SlotId::Mixer));
    for id in &scene.order {
        let Some(thing) = scene.things.get(id) else { continue };
        let card = resolved.things.get(id).and_then(|r| r.slots.get(&SlotId::Sound));
        let own = thing.slots.get(&SlotId::Sound);
        // A test tone comes from a tag (the `test-tone` feature).
        let tones = thing.tags.iter().any(|t| t.starts_with("test-"));
        if card.is_none() && own.is_none() && !tones {
            continue;
        }
        put(&(id, &thing.kind, thing.enabled, &thing.tags));
        put(&card);
        put(&own);
        // By side reads the shape's outline.
        if card.is_some_and(|c| matches!(c.params.get("notes_plays"), Some(Value::Choice(p)) if p == "by_side")) {
            put(&resolved.things.get(id).map(|r| &r.params));
        }
    }
    let sounds = |slot: Option<SlotId>| matches!(slot, Some(SlotId::Sound | SlotId::Mixer));
    for (a, t) in scene.tracks.iter().filter(|(a, _)| sounds(a.slot)) {
        put(&(a, t));
    }
    for (a, p) in scene.pulses.iter().filter(|(a, _)| sounds(a.slot)) {
        put(&(a, p));
    }
    // Samples and the backing track are named by the settings above; the bytes under a name.
    for (name, a) in &scene.audio {
        put(&(name, a.bytes.len(), Arc::as_ptr(&a.bytes) as *const u8 as usize));
    }
    // Light modulators read the lights through the probe.
    if bpe_kit::sensing::uses_light(scene) {
        put(&(bpe_kit::sensing::has_lighting(scene), bpe_kit::sensing::fingerprint(scene)));
    }
    h.finish()
}

impl SoundPatch {
    /// The patch, its wavetables baked if need be.
    pub fn build(scene: &Scene, resolved: &Resolved, kit: &Kit) -> SoundPatch {
        Self::build_with(scene, resolved, kit, Tables::Bake)
    }

    /// The patch with only the wavetables already baked ([`Tables::Ready`]); never waits.
    pub fn build_ready(scene: &Scene, resolved: &Resolved, kit: &Kit) -> SoundPatch {
        Self::build_with(scene, resolved, kit, Tables::Ready)
    }

    pub fn build_with(scene: &Scene, resolved: &Resolved, kit: &Kit, tables: Tables) -> SoundPatch {
        let mut pending = false;
        let root = resolved.things.get(&ThingId::ROOT);
        let card = root.and_then(|r| r.slots.get(&SlotId::Mixer));
        let master = card.map(|s| MixerSettings::from_params(&s.params)).unwrap_or_default();
        let track = |key: &str| {
            let a = Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key: key.into() };
            scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).cloned()
        };
        let master_keys = MasterKeys { input: track("master_input"), output: track("master_output") };
        let settings = root.map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
        let tempo = TempoMap::from_settings(&settings);
        let buses = BusesSpec::build(scene, &master, &kit.registry, &tempo);
        let mut voices: Vec<Box<dyn VoiceSource>> = Vec::new();
        let mut routes = Vec::new();
        let light = probe_of(scene, resolved);
        for id in &scene.order {
            let Some(thing) = resolved.things.get(id) else { continue };
            let shape = ShapeSettings::from_params(&thing.params);
            let b = Build {
                index: voices.len(),
                width: settings.width as f32,
                shape: &shape,
                tempo_top: tempo.top(),
                light: light.clone(),
            };
            if let Some(card) = thing.slots.get(&SlotId::Sound)
                && let Some(v) = voice_with(scene, *id, card, &b, tables, &mut pending)
            {
                voices.push(Box::new(v));
                routes.push(ShapeRoute::of(scene, *id, &VoiceSettings::from_params(&card.params)));
            }
        }
        #[cfg(feature = "test-tone")]
        for id in &scene.order {
            let Some(thing) = scene.things.get(id) else { continue };
            if !thing.enabled {
                continue;
            }
            if let Some(tone) = thing.tags.iter().find_map(|t| crate::voice::TestTone::from_tag(*id, t)) {
                let bus = thing.tags.iter().find_map(|t| t.strip_prefix("test-bus=")?.parse().ok()).unwrap_or(1);
                voices.push(Box::new(tone));
                routes.push(ShapeRoute::main_only(bus));
            }
        }
        let backing = Backing::of(scene, &master, tables, &mut pending);
        SoundPatch {
            master,
            master_keys,
            tempo,
            voices,
            routes,
            buses,
            pending,
            end: settings.duration as f64,
            backing,
        }
    }

    /// Whether anything in it makes sound.
    pub fn is_silent(&self) -> bool {
        self.voices.is_empty() && self.backing.is_none()
    }

    /// The things whose hits it can hear, sorted: every voice's thing and every shape a bus
    /// compressor keys on. The engine queues only their hits.
    pub fn heard(&self) -> Vec<ThingId> {
        let mut out: Vec<ThingId> = self.voices.iter().map(|v| v.thing()).chain(self.buses.hit_keys()).collect();
        out.sort();
        out.dedup();
        out
    }

    /// How long a seek renders unheard before the point it lands on, in seconds, so every note
    /// and effect tail that would still be ringing there is: the longest voice's note and the
    /// buses' effects after it, plus a margin, from [`MIN_WARM_UP`] to [`MAX_WARM_UP`].
    pub fn warm_up(&self) -> f64 {
        let voices = self.voices.iter().map(|v| v.warm_up(&self.tempo)).fold(0.0, f64::max);
        let held = self.voices.iter().any(|v| v.holds());
        warm_up_of(voices, self.buses.tail(), held)
    }
}

/// The least a seek warms up, in seconds.
pub const MIN_WARM_UP: f64 = 0.5;
/// The least for a sound that holds notes (a drone, or a gate held until the next hit), whose
/// notes can have started any time before.
pub const HELD_WARM_UP: f64 = 4.0;
/// The most a seek warms up, however long the tails (a frozen reverb never ends).
pub const MAX_WARM_UP: f64 = 30.0;
/// Added to the longest tail.
const WARM_UP_MARGIN: f64 = 0.25;

/// The warm-up for voices whose longest note lasts `voices` seconds into buses ringing `buses`
/// seconds after it; `held` when a voice holds notes.
pub fn warm_up_of(voices: f64, buses: f64, held: bool) -> f64 {
    let floor = if held { HELD_WARM_UP } else { MIN_WARM_UP };
    let tail = voices + buses + WARM_UP_MARGIN;
    if tail.is_nan() { MAX_WARM_UP } else { tail.clamp(floor, MAX_WARM_UP) }
}
