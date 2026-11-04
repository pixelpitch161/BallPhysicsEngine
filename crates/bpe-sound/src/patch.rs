//! Everything the engine reads from the scene, built off the audio path so building may
//! allocate, and swapped in between blocks.

use crate::{TempoMap, VoiceSource};
use bpe_core::{Resolved, Scene, Settings, SlotId, ThingId};
use bpe_kit::sound::MixerSettings;
use bpe_kit::{Kit, SceneSettings};

/// A scene's sound settings, ready for the engine.
pub struct SoundPatch {
    pub master: MixerSettings,
    pub tempo: TempoMap,
    pub voices: Vec<Box<dyn VoiceSource>>,
}

impl SoundPatch {
    pub fn build(scene: &Scene, resolved: &Resolved, _kit: &Kit) -> SoundPatch {
        let root = resolved.things.get(&ThingId::ROOT);
        let master = root
            .and_then(|r| r.slots.get(&SlotId::Mixer))
            .map(|s| MixerSettings::from_params(&s.params))
            .unwrap_or_default();
        let tempo = root.map(|r| TempoMap::from_settings(&SceneSettings::from_params(&r.params))).unwrap_or_default();
        #[allow(unused_mut)]
        let mut voices: Vec<Box<dyn VoiceSource>> = Vec::new();
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
        let _ = scene;
        SoundPatch { master, tempo, voices }
    }

    /// Whether anything in it makes sound.
    pub fn is_silent(&self) -> bool {
        self.voices.is_empty()
    }
}
