//! Light sensing (lighting spec part 7, optional and one-way): how lit a thing is, for Follow
//! light, the Light and Shadow spreads, Moths and the Light modulator. Everything reads the CPU
//! light probe ([`probe::Probe`]); nothing that makes light ever reads it back.

pub mod probe;

use std::sync::Arc;

use bpe_core::{Address, Registry, Resolved, Scene, SlotId, ThingId, Value};
use bpe_runtime::{LightAt, LightMaker, SoundAt};

use crate::modules::lighting::LIGHTING;
use crate::modules::look::layers::gives_light::{SHAPE_SUB_LIGHT, SUB_LIGHT};
use crate::modules::look::layers::lighting::{BLOCKS_LIGHT, LIT};
use probe::{LIGHTING_KINDS, LOOKED, Probe};

/// The kit's [`LightMaker`]: the probe, for worlds whose physics follows light.
pub struct KitLight;

impl LightMaker for KitLight {
    fn make(&self, scene: &Scene, resolved: &Resolved) -> Arc<dyn LightAt> {
        Arc::new(Probe::new(scene, resolved, None))
    }

    fn fingerprint(&self, scene: &Scene) -> u64 {
        fingerprint(scene)
    }
}

/// A scene's light probe, the lights following `sound` where they follow sound.
pub fn probe(scene: &Scene, resolved: &Resolved, sound: Option<Arc<dyn SoundAt>>) -> Arc<dyn LightAt> {
    Arc::new(Probe::new(scene, resolved, sound))
}

/// Whether anything in `scene` senses light: a setting following light, a setting spread by
/// Light or Shadow, a Motion card with Moths, or a Light modulator that is on. Only then does the
/// editor or the renderer make a probe.
pub fn uses_light(scene: &Scene) -> bool {
    if scene.pulses.values().any(|p| p.source.follows_light()) {
        return true;
    }
    scene.things.values().any(|t| {
        spreads_by_light(&t.params)
            || t.slots.iter().any(|(slot, s)| {
                spreads_by_light(&s.params)
                    || (*slot == SlotId::Motion && crate::modules::motion::moths_on(&s.params))
                    || (*slot == SlotId::Sound
                        && s.layers.iter().any(|l| l.enabled && l.kind == crate::sound::LIGHT_MOD))
            })
    })
}

/// Whether some setting in `params` spreads across the balls by Light or Shadow.
pub fn spreads_by_light(params: &bpe_core::Params) -> bool {
    use bpe_core::Choice;
    params.iter().any(|(k, v)| {
        k.ends_with(bpe_core::vary::BY)
            && matches!(v, Value::Choice(n) if bpe_core::vary::VaryBy::from_name(n).is_some_and(|b| b.reads_light()))
    })
}

/// Whether the lighting reads the setting at `a`: a Light's or a Rig's settings, the scene's
/// Lighting card, the background's light, a Gives off light sub-layer's and every layer's
/// Lighting settings. Such a setting may never follow light (sensing is one-way).
pub fn lighting_reads(scene: &Scene, reg: &Registry, a: &Address) -> bool {
    let Some(thing) = scene.things.get(&a.thing) else { return false };
    if LIGHTING_KINDS.contains(&thing.kind.as_str()) {
        return true;
    }
    match a.slot {
        Some(SlotId::Lighting) => true,
        None => a.thing == ThingId::ROOT && a.key.starts_with("bg_glow"),
        Some(slot) => {
            let key = bpe_core::vary::base_key(&a.key);
            let Some((layer, plain)) = bpe_core::split_layered(key) else {
                // A look's Surface (how it takes light, Bulge too) is the lighting's.
                use bpe_core::Settings;
                return slot == SlotId::Look
                    && crate::modules::look::surface::SurfaceSettings::defs().iter().any(|d| d.key == key);
            };
            if matches!(plain, LIT | BLOCKS_LIGHT) {
                return true;
            }
            let kind = thing.slots.get(&slot).and_then(|s| s.layer(layer)).map(|l| l.kind.as_str());
            let _ = reg;
            matches!(kind, Some(SUB_LIGHT | SHAPE_SUB_LIGHT))
        }
    }
}

/// Whether the setting at `a` may follow light: the scene has a Lighting card and the lighting
/// doesn't read the setting.
pub fn can_follow_light(scene: &Scene, reg: &Registry, a: &Address) -> bool {
    has_lighting(scene) && !lighting_reads(scene, reg, a)
}

/// Whether the scene has a Lighting card.
pub fn has_lighting(scene: &Scene) -> bool {
    scene.things.get(&ThingId::ROOT).and_then(|r| r.slots.get(&SlotId::Lighting)).is_some_and(|s| s.kind == LIGHTING)
}

/// A hash of everything the probe reads: the lights and rigs, the Lighting card and the looks
/// of balls, emitters and shapes, with their keys and pulses.
pub fn fingerprint(scene: &Scene) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    let read = |id: &ThingId, slot: Option<SlotId>| -> bool {
        let Some(t) = scene.things.get(id) else { return false };
        if LIGHTING_KINDS.contains(&t.kind.as_str()) {
            return true;
        }
        match slot {
            Some(SlotId::Lighting) => *id == ThingId::ROOT,
            Some(SlotId::Look) => LOOKED.contains(&t.kind.as_str()),
            _ => false,
        }
    };
    for id in &scene.order {
        let Some(t) = scene.things.get(id) else { continue };
        if LIGHTING_KINDS.contains(&t.kind.as_str()) {
            eat(format!("{id:?}{}{:?}{}{:?}", t.kind, t.parent, t.enabled, t.params).as_bytes());
        }
        for (slot, s) in &t.slots {
            if read(id, Some(*slot)) {
                eat(format!("{id:?}{slot:?}{s:?}").as_bytes());
            }
        }
    }
    for (a, track) in &scene.tracks {
        if read(&a.thing, a.slot) {
            eat(format!("{a:?}{track:?}").as_bytes());
        }
    }
    for (a, pulse) in &scene.pulses {
        if read(&a.thing, a.slot) {
            eat(format!("{a:?}{pulse:?}").as_bytes());
        }
    }
    h
}

#[cfg(test)]
mod tests;
