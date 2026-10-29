//! Mixes the soundtrack on a background thread whenever an edit changes what it would sound
//! like, a moment after the last such edit.

use bpe_core::{Resolved, Scene, Settings, SlotId, ThingId};
use bpe_kit::{Kit, SceneSettings};
use bpe_runtime::{EventKind, Log, Sims, World, fingerprint};
use eframe::egui;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// How long after the last sound-changing edit the mix starts.
pub const SOUND_DEBOUNCE: Duration = Duration::from_millis(250);

/// A finished mix.
pub struct Mixed {
    generation: u64,
    /// Interleaved stereo at the mixer's sample rate.
    pub track: Arc<Vec<f32>>,
    /// When every hit happens, in seconds, for the transport's hit lane.
    pub hits: Vec<f32>,
}

pub struct SoundJob {
    generation: u64,
    /// The sound fingerprint the last mix was started for.
    fingerprint: Option<u64>,
    dirty_since: Option<Instant>,
    running: bool,
    sender: Sender<Mixed>,
    receiver: Receiver<Mixed>,
    /// The hits of the last finished mix.
    pub hits: Vec<f32>,
}

impl Default for SoundJob {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Self { generation: 0, fingerprint: None, dirty_since: None, running: false, sender, receiver, hits: Vec::new() }
    }
}

impl SoundJob {
    /// Call after every edit: a mix is scheduled if the scene would sound different now.
    pub fn mark(&mut self, scene: &Scene, resolved: &Resolved, sims: &Sims) {
        let fingerprint = sound_fingerprint(scene, resolved, sims);
        if self.fingerprint != Some(fingerprint) {
            self.fingerprint = Some(fingerprint);
            self.dirty_since = Some(Instant::now());
        }
    }

    /// The soundtrack doesn't match the scene yet.
    pub fn busy(&self) -> bool {
        self.dirty_since.is_some() || self.running
    }

    /// Call every frame: collects a finished mix, and starts one when an edit is due. Returns
    /// the new track when one landed.
    pub fn poll(&mut self, ctx: &egui::Context, scene: &Scene, resolved: &Resolved, kit: &Arc<Kit>) -> Option<Mixed> {
        let mut landed = None;
        while let Ok(mixed) = self.receiver.try_recv() {
            self.running = false;
            if mixed.generation == self.generation {
                self.hits = mixed.hits.clone();
                landed = Some(mixed);
            }
        }
        let due = self.dirty_since.is_some_and(|t| t.elapsed() >= SOUND_DEBOUNCE);
        if due && !self.running {
            self.dirty_since = None;
            self.running = true;
            self.generation += 1;
            let (generation, scene, resolved, kit, sender, ctx) =
                (self.generation, scene.clone(), resolved.clone(), kit.clone(), self.sender.clone(), ctx.clone());
            std::thread::spawn(move || {
                let (track, hits) = mix(&scene, &resolved, &kit);
                let _ = sender.send(Mixed { generation, track: Arc::new(track), hits });
                ctx.request_repaint();
            });
        } else if self.dirty_since.is_some() {
            ctx.request_repaint_after(SOUND_DEBOUNCE);
        }
        landed
    }
}

/// Runs the whole scene and mixes it: the track and when every hit happens.
fn mix(scene: &Scene, resolved: &Resolved, kit: &Kit) -> (Vec<f32>, Vec<f32>) {
    let settings =
        resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    let mut world = World::build(scene, resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(settings.duration.max(0.0) as f64, &kit.sims, &mut log);
    let rate = world.steps_per_second;
    let hits = log.events().iter().filter(|e| e.kind == EventKind::Hit).map(|e| e.time(rate) as f32).collect();
    (bpe_kit::mix_scene(scene, resolved, &log, kit).samples, hits)
}

/// Everything the soundtrack depends on: the physics, every Sound module in use, and the
/// root's chords, loudness, seed, length and width. Two scenes that differ only in how they
/// look fingerprint the same, so a colour edit never remixes.
pub fn sound_fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    let mut h = fingerprint(scene, resolved, sims) ^ 0x9e37_79b9_7f4a_7c15;
    let mut mix = |text: &str| {
        for byte in text.bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for (id, r) in &resolved.things {
        if let Some(s) = r.slots.get(&SlotId::Sound) {
            mix(&id.0.to_string());
            mix(&s.kind);
            mix(&format!("{:?}", s.params));
        }
    }
    if let Some(root) = resolved.things.get(&ThingId::ROOT) {
        let s = SceneSettings::from_params(&root.params);
        mix(&format!("{:?} {:?} {} {:?} {}", s.chords, s.loudness, s.seed, s.duration, s.width));
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Address, ModuleId, Owner, ThingId, Value, resolve};

    fn module_named(scene: &bpe_core::Scene, name: &str) -> ModuleId {
        scene.library.modules.values().find(|m| m.name == name).map(|m| m.id).expect(name)
    }

    #[test]
    fn a_look_edit_does_not_change_the_sound_fingerprint() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let before = sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        let look = module_named(&scene, "Plain look");
        let glow = Address { owner: Owner::Module(look), slot: None, key: "glow".into() };
        scene.set_at(&kit.registry, &glow, Value::Number(1.0)).unwrap();
        assert_eq!(sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims), before);
    }

    #[test]
    fn a_volume_edit_does() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let before = sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        let sound = module_named(&scene, "Default sound");
        let volume = Address { owner: Owner::Module(sound), slot: None, key: "volume".into() };
        scene.set_at(&kit.registry, &volume, Value::Number(-6.0)).unwrap();
        let after = sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        assert_ne!(after, before);
        // So does a physics edit, and so does the chord progression the notes come from.
        let gravity = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "gravity".into() };
        scene.set_at(&kit.registry, &gravity, Value::Vec2([0.0, 10.0])).unwrap();
        let moved = sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        assert_ne!(moved, after);
        let chords = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "chords".into() };
        scene.set_at(&kit.registry, &chords, Value::Text("Am 4".into())).unwrap();
        assert_ne!(sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims), moved);
    }

    #[test]
    fn mixing_a_scene_gives_a_track_and_its_hits() {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let (track, hits) = mix(&scene, &resolved, &kit);
        let duration = SceneSettings::default().duration;
        assert_eq!(track.len(), (duration * bpe_mix::SAMPLE_RATE as f32) as usize * 2);
        assert!(!hits.is_empty());
        assert!(hits.windows(2).all(|w| w[0] <= w[1]));
        assert!(hits.iter().all(|&t| (0.0..=duration).contains(&t)));
    }
}
