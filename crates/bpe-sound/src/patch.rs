//! Everything the engine reads from the scene, built off the audio path so building may
//! allocate, and swapped in between blocks.

use crate::master::MasterKeys;
use crate::modmatrix::ModMatrix;
use crate::voices::knobs;
use crate::voices::shape::{EnvSpec, OscSpec, ShapeSpec};
use crate::{ShapeVoice, TempoMap, VoiceSource};
use bpe_core::{Address, Resolved, ResolvedSlot, Scene, Settings, SlotId, ThingId};
use bpe_dsp::{TableSet, WarpModeId, WaveBank, WaveShape};
use bpe_kit::sound::{ADSR, AdsrSettings, MixerSettings, OscSettings, VOICE, VoiceSettings, Wave};
use bpe_kit::{Kit, SceneSettings};
use std::sync::Arc;

/// A scene's sound settings, ready for the engine.
pub struct SoundPatch {
    pub master: MixerSettings,
    pub master_keys: MasterKeys,
    pub tempo: TempoMap,
    pub voices: Vec<Box<dyn VoiceSource>>,
    /// Some wavetable was still baking ([`Tables::Ready`]): those oscillators are silent until
    /// the patch is built again.
    pub pending: bool,
}

/// How a patch gets its wavetables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tables {
    /// Baked there and then if need be (the export and tests).
    Bake,
    /// Only those already baked; the rest start baking in the background (the editor, which
    /// must never wait).
    Ready,
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
        }
    }
}

/// The table a wave reads, for a tabled wave.
fn table_of(o: &OscSettings, tables: Tables, pending: &mut bool) -> Option<Arc<TableSet>> {
    if !o.on {
        return None;
    }
    let wave = match o.wave {
        Wave::Sine => WaveShape::Sine,
        Wave::Triangle => WaveShape::Triangle,
        Wave::Saw => WaveShape::Saw,
        Wave::Square => WaveShape::Square,
        Wave::Noise => return None,
    };
    tables.get(wave, WarpModeId::from_index(o.warp_mode.index()), pending)
}

fn osc_spec(o: &OscSettings, tables: Tables, pending: &mut bool) -> OscSpec {
    OscSpec {
        on: o.on,
        wave: o.wave,
        noise: o.noise,
        phase_mode: o.phase_mode,
        sub: o.sub,
        sub_octave: o.sub_octave,
        table: table_of(o, tables, pending),
    }
}

/// The voice of the shape `id` from its resolved Sound card, or `None` when its sound is off.
/// Its wavetables are baked if need be.
pub fn shape_voice(scene: &Scene, id: ThingId, card: &ResolvedSlot, index: usize, width: f32) -> Option<ShapeVoice> {
    voice_with(scene, id, card, index, width, Tables::Bake, &mut false)
}

fn voice_with(
    scene: &Scene,
    id: ThingId,
    card: &ResolvedSlot,
    index: usize,
    width: f32,
    tables: Tables,
    pending: &mut bool,
) -> Option<ShapeVoice> {
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
    let ids: Vec<_> = envs.iter().map(|e| e.id).collect();
    let layout = knobs::layout(&ids);
    let base = knobs::values(&layout, &|key| card.params.get(key).cloned());
    let keyed = layout
        .iter()
        .filter_map(|(key, _, i)| {
            let a = Address { thing: id, slot: Some(SlotId::Sound), key: key.clone() };
            scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).map(|t| (*i, t.clone()))
        })
        .collect();
    let matrix = ModMatrix::build(&v.routes, &knobs::targets(&layout));
    Some(ShapeVoice::new(ShapeSpec {
        thing: id,
        index,
        osc: [osc_spec(&v.a, tables, pending), osc_spec(&v.b, tables, pending)],
        envs,
        play_mode: v.play_mode,
        heard_from: v.play_heard_from.clone(),
        slots: v.notes_slots.clone(),
        bus: v.out_bus.clamp(1, 3) as u8,
        base,
        keyed,
        matrix,
        square: if [&v.a, &v.b].iter().any(|o| o.on && o.sub != bpe_kit::sound::SubWave::Off) {
            tables.get(WaveShape::Square, WarpModeId::Sync, pending)
        } else {
            None
        },
        width,
    }))
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

    pub fn build_with(scene: &Scene, resolved: &Resolved, _kit: &Kit, tables: Tables) -> SoundPatch {
        let mut pending = false;
        let root = resolved.things.get(&ThingId::ROOT);
        let master = root
            .and_then(|r| r.slots.get(&SlotId::Mixer))
            .map(|s| MixerSettings::from_params(&s.params))
            .unwrap_or_default();
        let track = |key: &str| {
            let a = Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key: key.into() };
            scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).cloned()
        };
        let master_keys = MasterKeys { input: track("master_input"), output: track("master_output") };
        let settings = root.map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
        let tempo = TempoMap::from_settings(&settings);
        let mut voices: Vec<Box<dyn VoiceSource>> = Vec::new();
        for id in &scene.order {
            let Some(thing) = resolved.things.get(id) else { continue };
            if let Some(card) = thing.slots.get(&SlotId::Sound)
                && let Some(v) = voice_with(scene, *id, card, voices.len(), settings.width as f32, tables, &mut pending)
            {
                voices.push(Box::new(v));
            }
        }
        #[cfg(feature = "test-tone")]
        for id in &scene.order {
            let Some(thing) = scene.things.get(id) else { continue };
            if !thing.enabled {
                continue;
            }
            if let Some(tone) = thing.tags.iter().find_map(|t| crate::voice::TestTone::from_tag(*id, t)) {
                voices.push(Box::new(tone));
            }
        }
        SoundPatch { master, master_keys, tempo, voices, pending }
    }

    /// Whether anything in it makes sound.
    pub fn is_silent(&self) -> bool {
        self.voices.is_empty()
    }
}
