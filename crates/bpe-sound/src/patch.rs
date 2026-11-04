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
}

/// The table a wave reads, for a tabled wave.
fn table_of(o: &OscSettings) -> Option<Arc<TableSet>> {
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
    Some(WaveBank::get(wave, WarpModeId::from_index(o.warp_mode.index())))
}

fn osc_spec(o: &OscSettings) -> OscSpec {
    OscSpec {
        on: o.on,
        wave: o.wave,
        noise: o.noise,
        phase_mode: o.phase_mode,
        sub: o.sub,
        sub_octave: o.sub_octave,
        table: table_of(o),
    }
}

/// The voice of the shape `id` from its resolved Sound card, or `None` when its sound is off.
pub fn shape_voice(scene: &Scene, id: ThingId, card: &ResolvedSlot, index: usize, width: f32) -> Option<ShapeVoice> {
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
        osc: [osc_spec(&v.a), osc_spec(&v.b)],
        envs,
        play_mode: v.play_mode,
        heard_from: v.play_heard_from.clone(),
        slots: v.notes_slots.clone(),
        bus: v.out_bus.clamp(1, 3) as u8,
        base,
        keyed,
        matrix,
        square: WaveBank::get(WaveShape::Square, WarpModeId::Sync),
        width,
    }))
}

impl SoundPatch {
    pub fn build(scene: &Scene, resolved: &Resolved, _kit: &Kit) -> SoundPatch {
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
                && let Some(v) = shape_voice(scene, *id, card, voices.len(), settings.width as f32)
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
        SoundPatch { master, master_keys, tempo, voices }
    }

    /// Whether anything in it makes sound.
    pub fn is_silent(&self) -> bool {
        self.voices.is_empty()
    }
}
