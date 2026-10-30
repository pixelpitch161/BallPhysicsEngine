//! Mixes the soundtrack on a background thread whenever an edit changes what it would sound
//! like, a moment after the last such edit.

use bpe_core::{Owner, Resolved, Scene, Settings, SlotId, ThingId};
use bpe_kit::{Kit, SceneSettings, TimeWarp};
use bpe_mix::ChannelCache;
use bpe_runtime::{EventKind, Log, Sims, World, fingerprint};
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long after the last sound-changing edit the mix starts.
pub const SOUND_DEBOUNCE: Duration = Duration::from_millis(250);

/// A finished mix.
pub struct Mixed {
    generation: u64,
    /// Interleaved stereo at the mixer's sample rate.
    pub track: Arc<Vec<f32>>,
    /// When every hit happens, in video seconds, for the transport's hit lane.
    pub hits: Vec<f32>,
    /// The scene's beats, in video seconds.
    pub beats: Vec<f32>,
    /// The beat sync's time warp the video plays through.
    pub warp: TimeWarp,
}

pub struct SoundJob {
    generation: u64,
    /// The sound fingerprint the last mix was started for.
    fingerprint: Option<u64>,
    dirty_since: Option<Instant>,
    running: bool,
    sender: Sender<Mixed>,
    receiver: Receiver<Mixed>,
    /// The hits of the last finished mix, in video seconds.
    pub hits: Vec<f32>,
    /// The beats of the last finished mix, in video seconds.
    pub beats: Vec<f32>,
    /// Each channel's last render, shared by the mixes one after another, so an edit to one
    /// sound re-renders only its channel.
    channels: Arc<Mutex<ChannelCache>>,
}

impl Default for SoundJob {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Self {
            generation: 0,
            fingerprint: None,
            dirty_since: None,
            running: false,
            sender,
            receiver,
            hits: Vec::new(),
            beats: Vec::new(),
            channels: Default::default(),
        }
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

    /// Schedules a mix although the scene reads the same: something it depends on, like a song
    /// that finished loading, has changed.
    pub fn remix(&mut self) {
        self.dirty_since = Some(Instant::now());
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
                self.beats = mixed.beats.clone();
                landed = Some(mixed);
            }
        }
        let due = self.dirty_since.is_some_and(|t| t.elapsed() >= SOUND_DEBOUNCE);
        if due && !self.running {
            self.dirty_since = None;
            self.running = true;
            self.generation += 1;
            let (generation, scene, resolved, kit, sender, ctx, channels) = (
                self.generation,
                scene.clone(),
                resolved.clone(),
                kit.clone(),
                self.sender.clone(),
                ctx.clone(),
                self.channels.clone(),
            );
            std::thread::spawn(move || {
                // One mix runs at a time, so the lock is never waited on.
                let mut cache = channels.lock().unwrap_or_else(|e| e.into_inner());
                let mut mixed = mix(&scene, &resolved, &kit, &mut cache);
                mixed.generation = generation;
                let _ = sender.send(mixed);
                ctx.request_repaint();
            });
        } else if self.dirty_since.is_some() {
            ctx.request_repaint_after(SOUND_DEBOUNCE);
        }
        landed
    }
}

/// Runs the whole scene and mixes it: the track, when every hit and beat happens in the video,
/// and the time warp the video plays through.
fn mix(scene: &Scene, resolved: &Resolved, kit: &Kit, cache: &mut ChannelCache) -> Mixed {
    let settings =
        resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    let mut world = World::build(scene, resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(settings.duration.max(0.0) as f64, &kit.sims, &mut log);
    let rate = world.steps_per_second;
    let warp = bpe_kit::warp_for(scene, resolved, &log);
    let hits = log
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::Hit)
        .map(|e| warp.video_time(e.time(rate)) as f32)
        .collect();
    let beats = bpe_kit::beats_for(&settings).into_iter().map(|b| b as f32).collect();
    let track = bpe_kit::mix_scene_cached(scene, resolved, &log, kit, cache).samples;
    Mixed { generation: 0, track: Arc::new(track), hits, beats, warp }
}

/// Everything the soundtrack depends on: the physics, every Sound module in use, the root's
/// chords, loudness, seed, length, width and music settings, and every key or pulse on a
/// Sound setting. Two scenes that differ only in how they look fingerprint the same, so a
/// colour edit never remixes.
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
        mix(&format!("{:?} {:?} {} {:?} {} {}", s.chords, s.loudness, s.seed, s.duration, s.width, s.height));
        mix(&format!(
            "{:?} {} {} {} {} {:?} {} {}",
            s.song, s.song_offset, s.song_volume, s.ducking, s.bpm, s.beat_sync, s.subdivisions, s.warp_strength
        ));
    }
    mix(&format!("{:?}", scene.library.progressions));
    mix(&format!("{:?}", scene.library.instruments));
    for (id, m) in scene.library.modules.iter().filter(|(_, m)| !m.macros.is_empty()) {
        mix(&format!("{id:?} {:?}", m.macros));
    }
    let sound_setting = |a: &bpe_core::Address| match (a.owner, a.slot) {
        (Owner::Thing(_), Some(slot)) => slot == SlotId::Sound,
        (Owner::Thing(_), None) => false,
        (Owner::Module(m), _) => scene.library.modules.get(&m).is_some_and(|m| m.slot == SlotId::Sound),
    };
    for (a, track) in scene.tracks.iter().filter(|(a, _)| sound_setting(a)) {
        mix(&format!("{a:?} {track:?}"));
    }
    for (a, pulse) in scene.pulses.iter().filter(|(a, _)| sound_setting(a)) {
        mix(&format!("{a:?} {pulse:?}"));
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
        let (&id, _) = scene.library.progressions.iter().next().unwrap();
        let am = bpe_core::Chord::new(9, bpe_core::Quality::Minor, bpe_core::Extension::Triad, 4);
        scene.set_progression(id, bpe_core::Progression { chords: vec![am], ..Default::default() }).unwrap();
        let chords = sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        assert_ne!(chords, moved);
        // And so do the instrument and a module's own macro values.
        let (&pluck, i) = scene.library.instruments.iter().next().unwrap();
        scene.set_instrument(pluck, bpe_core::Instrument { name: "Renamed".into(), ..i.clone() }).unwrap();
        let renamed = sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        assert_ne!(renamed, chords);
        scene.set_macro(sound, "1.cutoff", Some(400.0)).unwrap();
        assert_ne!(sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims), renamed);
    }

    #[test]
    fn mixing_a_scene_gives_a_track_and_its_hits() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mixed = mix(&scene, &resolved, &kit, &mut ChannelCache::default());
        let duration = SceneSettings::default().duration;
        assert_eq!(mixed.track.len(), (duration * bpe_mix::SAMPLE_RATE as f32) as usize * 2);
        assert!(!mixed.hits.is_empty());
        assert!(mixed.hits.windows(2).all(|w| w[0] <= w[1]));
        assert!(mixed.hits.iter().all(|&t| (0.0..=duration).contains(&t)));
        assert!(mixed.beats.is_empty() && mixed.warp.is_identity(), "no music, no beats");
        // A tempo gives beats; beat sync gives a warp.
        let bpm = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "bpm".into() };
        let sync = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "beat_sync".into() };
        scene.set_at(&kit.registry, &bpm, Value::Number(120.0)).unwrap();
        scene.set_at(&kit.registry, &sync, Value::Choice("warp".into())).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mixed = mix(&scene, &resolved, &kit, &mut ChannelCache::default());
        assert_eq!(mixed.beats.len(), 42);
        assert!(!mixed.warp.is_identity());
    }

    #[test]
    fn a_tempo_edit_changes_the_sound_fingerprint_and_a_look_key_does_not() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let reg = &kit.registry;
        let before = sound_fingerprint(&scene, &resolve(&scene, reg), &kit.sims);
        let look = module_named(&scene, "Plain look");
        let glow = Address { owner: Owner::Module(look), slot: None, key: "glow".into() };
        scene.set_key(reg, &glow, 1.0, Value::Number(1.0)).unwrap();
        assert_eq!(sound_fingerprint(&scene, &resolve(&scene, reg), &kit.sims), before);
        let bpm = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "bpm".into() };
        scene.set_at(reg, &bpm, Value::Number(100.0)).unwrap();
        let with_tempo = sound_fingerprint(&scene, &resolve(&scene, reg), &kit.sims);
        assert_ne!(with_tempo, before);
        // A key on a Sound setting changes it too.
        let sound = module_named(&scene, "Default sound");
        let volume = Address { owner: Owner::Module(sound), slot: None, key: "volume".into() };
        scene.set_key(reg, &volume, 3.0, Value::Number(-10.0)).unwrap();
        assert_ne!(sound_fingerprint(&scene, &resolve(&scene, reg), &kit.sims), with_tempo);
    }
}
