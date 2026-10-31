//! Mixes the soundtrack on a background thread whenever an edit changes what it would sound
//! like, a moment after the last such edit.

use bpe_core::{Owner, Resolved, Scene, Settings, SlotId, ThingId};
use bpe_kit::{Kit, SceneSettings, TimeWarp};
use bpe_mix::ChannelCache;
use bpe_runtime::{EventKind, Log, Sims, World, fingerprint};
use eframe::egui;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long after the last sound-changing edit the mix starts.
pub const SOUND_DEBOUNCE: Duration = Duration::from_millis(250);

/// How much of the run a mix simulates between looks at whether a newer edit replaced it.
const STALE_CHECK_SECONDS: f64 = 0.5;

/// What the mixes keep from one to the next: each channel's last render, and the last physics
/// run, so a sound edit neither re-renders the channels it didn't touch nor simulates the
/// scene again.
#[derive(Default)]
pub(crate) struct MixMemory {
    channels: ChannelCache,
    physics: Option<PhysicsRun>,
    /// How many times a mix has simulated the scene, for the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    simulated: usize,
}

/// One finished physics run and what it was made from.
struct PhysicsRun {
    key: u64,
    log: Arc<Log>,
    steps_per_second: u32,
}

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
    /// Plain sentences about sounds that couldn't play: a patch that doesn't compile, a voice
    /// that made a bad sample, a sample file that can't be read.
    pub problems: Vec<String>,
    /// What the master stage measured, for the meters.
    pub report: Option<bpe_mix::MasterReport>,
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
    /// The last finished mix's master readings, for the meters.
    pub report: Option<bpe_mix::MasterReport>,
    /// Each channel's last render and the last physics run, shared by the mixes one after
    /// another, so an edit to one sound re-renders only its channel.
    memory: Arc<Mutex<MixMemory>>,
    /// The generation of the newest mix wanted: a running mix older than this gives up.
    wanted: Arc<AtomicU64>,
    /// Bumped by [`SoundJob::remix`], so the next mix simulates again (a song that finished
    /// loading can change the run).
    epoch: u64,
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
            report: None,
            memory: Default::default(),
            wanted: Default::default(),
            epoch: 0,
        }
    }
}

impl SoundJob {
    /// Call after every edit: a mix is scheduled if the scene would sound different now.
    pub fn mark(&mut self, scene: &Scene, resolved: &Resolved, sims: &Sims) {
        let fingerprint = sound_fingerprint(scene, resolved, sims);
        if self.fingerprint != Some(fingerprint) {
            self.fingerprint = Some(fingerprint);
            self.edited();
        }
    }

    /// A newer mix is due: the one running, if any, is out of date and stops early.
    fn edited(&mut self) {
        self.dirty_since = Some(Instant::now());
        self.wanted.store(self.generation + 1, Ordering::Relaxed);
    }

    /// Schedules a mix although the scene reads the same: something it depends on, like a song
    /// that finished loading, has changed.
    pub fn remix(&mut self) {
        self.epoch += 1;
        self.edited();
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
                self.report = mixed.report.clone();
                landed = Some(mixed);
            }
        }
        let due = self.dirty_since.is_some_and(|t| t.elapsed() >= SOUND_DEBOUNCE);
        if due && !self.running {
            self.dirty_since = None;
            self.running = true;
            self.generation += 1;
            self.wanted.store(self.generation, Ordering::Relaxed);
            let (generation, scene, resolved, kit, sender, ctx, memory, wanted, epoch) = (
                self.generation,
                scene.clone(),
                resolved.clone(),
                kit.clone(),
                self.sender.clone(),
                ctx.clone(),
                self.memory.clone(),
                self.wanted.clone(),
                self.epoch,
            );
            std::thread::spawn(move || {
                // One mix runs at a time, so the lock is never waited on.
                let mut memory = memory.lock().unwrap_or_else(|e| e.into_inner());
                let stale = || wanted.load(Ordering::Relaxed) != generation;
                let mixed = mix(&scene, &resolved, &kit, &mut memory, epoch, &stale);
                // A mix that gave up still says it finished, so the next one can start, with a
                // generation no mix has, so it is never played.
                let mixed = match mixed {
                    Some(mixed) => Mixed { generation, ..mixed },
                    None => Mixed::abandoned(),
                };
                let _ = sender.send(mixed);
                ctx.request_repaint();
            });
        } else if self.dirty_since.is_some() {
            ctx.request_repaint_after(SOUND_DEBOUNCE);
        }
        landed
    }
}

impl Mixed {
    /// What a mix that gave up for a newer one sends: generation 0, which no mix has, so it is
    /// never played.
    fn abandoned() -> Self {
        Self {
            generation: 0,
            track: Default::default(),
            hits: Vec::new(),
            beats: Vec::new(),
            warp: TimeWarp::identity(),
            problems: Vec::new(),
            report: None,
        }
    }
}

/// Runs the whole scene and mixes it: the track, when every hit and beat happens in the video,
/// and the time warp the video plays through. The physics run is kept in `memory` and used
/// again while the physics is the same, as it is after any sound edit. Gives up, with
/// `None`, as soon as `stale` says a newer edit has replaced this mix.
fn mix(
    scene: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    memory: &mut MixMemory,
    epoch: u64,
    stale: &dyn Fn() -> bool,
) -> Option<Mixed> {
    let settings =
        resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
    let duration = settings.duration.max(0.0) as f64;
    let record_tracks = bpe_kit::uses_live_inputs(scene);
    let key = {
        let mut h = fingerprint(scene, resolved, &kit.sims) ^ 0x2545_f491_4f6c_dd1d;
        for part in [duration.to_bits(), record_tracks as u64, epoch] {
            h = (h ^ part).wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    if memory.physics.as_ref().is_none_or(|run| run.key != key) {
        memory.physics = None;
        let mut world = World::build(scene, resolved, &kit.sims);
        world.record_tracks = record_tracks;
        let mut log = Log::default();
        let mut at = 0.0;
        while at < duration {
            if stale() {
                return None;
            }
            at = (at + STALE_CHECK_SECONDS).min(duration);
            world.advance_to(at, &kit.sims, &mut log);
        }
        memory.simulated += 1;
        memory.physics = Some(PhysicsRun { key, log: Arc::new(log), steps_per_second: world.steps_per_second });
    }
    let run = memory.physics.as_ref().expect("simulated above");
    let (log, rate) = (run.log.clone(), run.steps_per_second);
    if stale() {
        return None;
    }
    let warp = bpe_kit::warp_for(scene, resolved, &log);
    let hits = log
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::Hit)
        .map(|e| warp.video_time(e.time(rate)) as f32)
        .collect();
    let beats = bpe_kit::beats_for(&settings).into_iter().map(|b| b as f32).collect();
    let mixed = bpe_kit::mix_scene_cached(scene, resolved, &log, kit, &mut memory.channels);
    Some(Mixed {
        generation: 0,
        track: Arc::new(mixed.samples),
        hits,
        beats,
        warp,
        problems: mixed.problems,
        report: mixed.report,
    })
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
        mix(&format!(
            "{:?} {:?} {} {:?} {} {} {:?} {} {} {} {}",
            s.chords,
            s.loudness,
            s.seed,
            s.duration,
            s.width,
            s.height,
            s.clipper,
            s.clip_drive,
            s.ceiling,
            s.auto_release,
            s.release
        ));
        mix(&format!(
            "{:?} {} {} {} {} {:?} {} {}",
            s.song, s.song_offset, s.song_volume, s.ducking, s.bpm, s.beat_sync, s.subdivisions, s.warp_strength
        ));
    }
    mix(&format!("{:?}", scene.library.progressions));
    // An instrument's name and where its blocks sit don't sound.
    for (id, i) in &scene.library.instruments {
        let source = match &i.source {
            bpe_core::InstrumentSource::Patch { graph } => bpe_synth::sound_key(graph),
            built_in => format!("{built_in:?}"),
        };
        mix(&format!("{id:?} {source} {:?} {}", i.voices, bpe_synth::sound_key(&i.fx)));
    }
    for (id, m) in scene.library.modules.iter().filter(|(_, m)| !m.macros.is_empty()) {
        mix(&format!("{id:?} {:?}", m.macros));
    }
    // Effects, sends and buses: a bus's name doesn't sound.
    for (id, m) in scene.library.modules.iter().filter(|(_, m)| !m.channel.is_default()) {
        mix(&format!("{id:?} {} {:?}", bpe_synth::sound_key(&m.channel.inserts), m.channel.sends));
    }
    for b in &scene.library.buses {
        mix(&format!("{:?} {} {}", b.id, b.volume, bpe_synth::sound_key(&b.fx)));
    }
    mix(&bpe_synth::sound_key(&scene.library.master_fx));
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
    use bpe_core::{Address, ModuleId, Owner, SettingValue, ThingId, Value, resolve};

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
    fn moving_a_patch_block_does_not_change_the_sound_fingerprint() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let id = scene.add_instrument(bpe_core::Instrument {
            fx: Default::default(),
            name: "Patch".into(),
            source: bpe_core::InstrumentSource::Patch { graph: bpe_synth::blank_patch() },
            voices: Default::default(),
        });
        let print = |scene: &bpe_core::Scene| sound_fingerprint(scene, &resolve(scene, &kit.registry), &kit.sims);
        let before = print(&scene);
        let mut i = scene.library.instruments[&id].clone();
        if let bpe_core::InstrumentSource::Patch { graph } = &mut i.source {
            graph.nodes[0].pos = [900.0, 900.0];
        }
        i.name = "Renamed".into();
        scene.set_instrument(id, i.clone()).unwrap();
        assert_eq!(print(&scene), before, "where blocks sit and what it's called don't sound");
        if let bpe_core::InstrumentSource::Patch { graph } = &mut i.source {
            graph.links.clear();
        }
        scene.set_instrument(id, i).unwrap();
        assert_ne!(print(&scene), before, "unwiring does");
    }

    #[test]
    fn a_bus_edit_remixes() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let print = |scene: &Scene| {
            let resolved = resolve(scene, &kit.registry);
            sound_fingerprint(scene, &resolved, &kit.sims)
        };
        let before = print(&scene);
        assert_eq!(before, print(&scene), "nothing changed");
        scene.library.buses[0].volume = -6.0;
        let bus = print(&scene);
        assert_ne!(bus, before, "a bus volume");
        scene.library.master_fx = bpe_synth::chain_graph(&[("width".into(), Default::default(), true)]);
        let master = print(&scene);
        assert_ne!(master, bus, "master effects");
        let sound = *scene.library.modules.iter().find(|(_, m)| m.slot == SlotId::Sound).unwrap().0;
        let id = scene.library.buses[0].id;
        scene.library.modules.get_mut(&sound).unwrap().channel.sends.insert(id, -12.0);
        let send = print(&scene);
        assert_ne!(send, master, "a send");
        let pluck = *scene.library.instruments.keys().next().unwrap();
        scene.library.instruments.get_mut(&pluck).unwrap().fx =
            bpe_synth::chain_graph(&[("chorus".into(), Default::default(), true)]);
        assert_ne!(print(&scene), send, "instrument effects");
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
        let mut voices = i.voices;
        voices.limit = 4;
        scene.set_instrument(pluck, bpe_core::Instrument { voices, ..i.clone() }).unwrap();
        let renamed = sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        assert_ne!(renamed, chords);
        scene.set_macro(sound, "1.cutoff", Some(400.0)).unwrap();
        assert_ne!(sound_fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims), renamed);
    }

    #[test]
    fn a_broken_patch_says_why_in_the_mix() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let mut graph = bpe_synth::blank_patch();
        graph.nodes.retain(|n| n.kind != "voice_out");
        let id = scene.add_instrument(bpe_core::Instrument {
            fx: Default::default(),
            name: "Broken".into(),
            source: bpe_core::InstrumentSource::Patch { graph },
            voices: Default::default(),
        });
        let sound = module_named(&scene, "Default sound");
        scene
            .library
            .modules
            .get_mut(&sound)
            .unwrap()
            .params
            .set("instrument", bpe_core::InstrumentRef(Some(id)).to_value());
        let resolved = resolve(&scene, &kit.registry);
        let mixed = mix(&scene, &resolved, &kit, &mut MixMemory::default(), 0, &|| false).unwrap();
        assert!(
            mixed.problems.iter().any(|p| p.starts_with("Broken: the patch doesn't compile")),
            "{:?}",
            mixed.problems
        );
    }

    #[test]
    fn mixing_a_scene_gives_a_track_and_its_hits() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mixed = mix(&scene, &resolved, &kit, &mut MixMemory::default(), 0, &|| false).unwrap();
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
        let mixed = mix(&scene, &resolved, &kit, &mut MixMemory::default(), 0, &|| false).unwrap();
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

    #[test]
    fn a_sound_edit_mixes_without_simulating_again_and_sounds_the_same_as_from_scratch() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let mut memory = MixMemory::default();
        let run = |scene: &Scene, memory: &mut MixMemory| {
            mix(scene, &resolve(scene, &kit.registry), &kit, memory, 0, &|| false).unwrap()
        };
        run(&scene, &mut memory);
        assert_eq!(memory.simulated, 1);
        let sound = module_named(&scene, "Default sound");
        let volume = Address { owner: Owner::Module(sound), slot: None, key: "volume".into() };
        scene.set_at(&kit.registry, &volume, Value::Number(-9.0)).unwrap();
        let kept = run(&scene, &mut memory);
        assert_eq!(memory.simulated, 1, "a sound edit keeps the run");
        let fresh = run(&scene, &mut MixMemory::default());
        assert_eq!(kept.track, fresh.track);
        assert_eq!(kept.hits, fresh.hits);
        // A physics edit simulates again.
        let gravity = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "gravity".into() };
        scene.set_at(&kit.registry, &gravity, Value::Vec2([0.0, 10.0])).unwrap();
        run(&scene, &mut memory);
        assert_eq!(memory.simulated, 2, "a physics edit doesn't");
        // So does a remix, which bumps the epoch (a song may have finished loading).
        mix(&scene, &resolve(&scene, &kit.registry), &kit, &mut memory, 1, &|| false).unwrap();
        assert_eq!(memory.simulated, 3, "nor does a remix");
    }

    #[test]
    fn a_mix_a_newer_edit_replaced_gives_up() {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let mut memory = MixMemory::default();
        let resolved = resolve(&scene, &kit.registry);
        assert!(mix(&scene, &resolved, &kit, &mut memory, 0, &|| true).is_none());
        assert_eq!(memory.simulated, 0, "it stops while simulating");
        assert!(memory.physics.is_none(), "and keeps no half run");
    }

    #[test]
    fn an_edit_during_a_mix_marks_the_running_mix_out_of_date() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let mut job = SoundJob::default();
        let resolved = resolve(&scene, &kit.registry);
        job.mark(&scene, &resolved, &kit.sims);
        // As poll does when it starts the mix.
        job.generation += 1;
        job.wanted.store(job.generation, Ordering::Relaxed);
        let running = job.generation;
        let sound = module_named(&scene, "Default sound");
        let volume = Address { owner: Owner::Module(sound), slot: None, key: "volume".into() };
        scene.set_at(&kit.registry, &volume, Value::Number(-9.0)).unwrap();
        job.mark(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        assert_ne!(job.wanted.load(Ordering::Relaxed), running);
    }
}
