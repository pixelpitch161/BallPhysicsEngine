//! Music for a scene: the song under the sounds, the beat grid things pulse and sync to, and
//! beat sync by time warp.

pub mod beats;
pub mod song_mix;
pub mod warp;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use bpe_core::{Resolved, Scene, Settings, ThingId};
use bpe_music::AudioFeatures;
use bpe_runtime::{EventKind, Log};

pub use beats::{DEFAULT_BPM, bands_of, beat_grid, distance_to_beat, on_beat_share, subdivided};
pub use song_mix::mix_in;
pub use warp::TimeWarp;

use crate::things::scene::{BeatSync, SceneSettings};

/// A decoded, analysed song.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    /// Interleaved stereo at the mix rate (48 kHz).
    pub samples: Vec<f32>,
    pub duration: f64,
    pub features: AudioFeatures,
}

/// One path's place in the cache: decoded once, by whoever asks first. The map's lock is only
/// held to find the entry, never while decoding.
#[derive(Default)]
struct Entry {
    song: OnceLock<Option<Arc<Song>>>,
    /// A thread of its own is decoding it (see [`load_song_in_background`]).
    background: AtomicBool,
}

static SONGS: Mutex<BTreeMap<String, Arc<Entry>>> = Mutex::new(BTreeMap::new());

fn entry(path: &str) -> Arc<Entry> {
    SONGS.lock().unwrap_or_else(|e| e.into_inner()).entry(path.to_owned()).or_default().clone()
}

fn decode(path: &str) -> Option<Arc<Song>> {
    bpe_music::decode(path).ok().map(|pcm| {
        let features = bpe_music::analyze_decoded(path, &pcm);
        Arc::new(Song { duration: pcm.duration(), samples: pcm.samples, features })
    })
}

/// The song at `path`, decoded and analysed once per path; `None` for an empty path, for a file
/// that can't be read (which is remembered too, so a missing file is not retried every step),
/// and for a song still loading in the background, which is not waited for.
pub fn load_song(path: &str) -> Option<Arc<Song>> {
    if path.trim().is_empty() {
        return None;
    }
    let e = entry(path);
    if let Some(song) = e.song.get() {
        return song.clone();
    }
    if e.background.load(Ordering::Acquire) {
        return None;
    }
    e.song.get_or_init(|| decode(path)).clone()
}

/// Where a song's loading stands.
#[derive(Clone, Debug)]
pub enum SongState {
    /// A thread is decoding it; [`load_song`] says there is no song until it lands.
    Loading,
    Ready(Arc<Song>),
    /// The file couldn't be read.
    Unreadable,
}

/// Starts decoding the song on a thread of its own, unless it is loaded or loading already, and
/// says how far it is. The editor asks this before building a run, so the window never waits
/// for a song, and rebuilds once it is `Ready`. `None` for an empty path.
pub fn load_song_in_background(path: &str) -> Option<SongState> {
    if path.trim().is_empty() {
        return None;
    }
    let e = entry(path);
    if let Some(song) = e.song.get() {
        return Some(song.clone().map_or(SongState::Unreadable, SongState::Ready));
    }
    if !e.background.swap(true, Ordering::AcqRel) {
        let path = path.to_owned();
        let e = e.clone();
        std::thread::spawn(move || {
            e.song.get_or_init(|| decode(&path));
        });
    }
    Some(SongState::Loading)
}

/// Forgets a loaded (or unreadable) song, so a changed or fixed file is read again.
pub fn forget_song(path: &str) {
    SONGS.lock().unwrap_or_else(|e| e.into_inner()).remove(path);
}

/// Whether the scene follows a beat at all: a tempo, beat sync or a song.
pub fn wants_beats(s: &SceneSettings) -> bool {
    s.bpm > 0.0 || s.beat_sync != BeatSync::Off || !s.song.trim().is_empty()
}

/// The scene's beat grid in video seconds: empty unless it [`wants_beats`].
pub fn beats_for(s: &SceneSettings) -> Vec<f64> {
    if !wants_beats(s) {
        return Vec::new();
    }
    let song = load_song(&s.song);
    beat_grid(s.bpm, song.as_ref().map(|song| &song.features), s.song_offset, s.duration as f64)
}

/// The beats chords are laid on: the scene's beat grid, or a steady 120 bpm when the scene has
/// no beat of its own.
pub fn chord_beats(s: &SceneSettings) -> Vec<f64> {
    let beats = beats_for(s);
    if beats.is_empty() { beat_grid(0.0, None, 0.0, s.duration as f64) } else { beats }
}

/// The root's settings as resolved.
pub fn root_settings(resolved: &Resolved) -> SceneSettings {
    resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default()
}

/// The time warp a finished run plays back through: the identity unless the scene's beat sync
/// is Warp, in which case every hit is pulled towards the nearest beat.
pub fn warp_for(_scene: &Scene, resolved: &Resolved, log: &Log) -> TimeWarp {
    let s = root_settings(resolved);
    if s.beat_sync != BeatSync::Warp {
        return TimeWarp::identity();
    }
    let rate = s.steps_per_second.max(1);
    let hits: Vec<f64> = log.events().iter().filter(|e| e.kind == EventKind::Hit).map(|e| e.time(rate)).collect();
    let beats = subdivided(&beats_for(&s), s.subdivisions);
    TimeWarp::fit(&hits, &beats, s.warp_strength as f64, s.duration as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kit, mix_scene, new_scene};
    use bpe_core::{Value, resolve};

    fn temp_wav(name: &str, seconds: f32, rate: u32) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-kit-music-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..(seconds * rate as f32) as usize {
            let s = (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * 0.5;
            writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
        path
    }

    #[test]
    fn a_generated_wav_loads_resampled_with_a_duration() {
        let path = temp_wav("load", 2.0, 44_100);
        let song = load_song(path.to_str().unwrap()).expect("the wav loads");
        assert!((song.duration - 2.0).abs() < 0.01, "{}", song.duration);
        assert_eq!(song.samples.len(), (song.duration * 48_000.0).round() as usize * 2);
        assert!(song.features.level.iter().any(|&l| l > 0.0));
        // The second load comes from the cache: the same song.
        assert!(Arc::ptr_eq(&song, &load_song(path.to_str().unwrap()).unwrap()));
        assert!(load_song("").is_none());
        assert!(load_song("/nowhere/at/all.mp3").is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_background_load_never_blocks_and_lands_for_everyone() {
        let path = temp_wav("background", 2.0, 44_100);
        let path = path.to_str().unwrap();
        assert!(matches!(load_song_in_background(path), Some(SongState::Loading)));
        // Meanwhile a plain load doesn't wait: it has no song, or the finished one.
        let early = load_song(path);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let song = loop {
            match load_song_in_background(path) {
                Some(SongState::Ready(song)) => break song,
                Some(SongState::Loading) => assert!(std::time::Instant::now() < deadline, "still loading"),
                other => panic!("{other:?}"),
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(early.is_none_or(|s| Arc::ptr_eq(&s, &song)));
        assert!(Arc::ptr_eq(&load_song(path).unwrap(), &song), "now everyone gets it");
        assert!(
            matches!(load_song_in_background("/nowhere/else.mp3"), Some(SongState::Loading)),
            "a missing file loads (and fails) in the background too"
        );
        assert!(load_song_in_background("").is_none());
        forget_song(path);
        assert!(matches!(load_song_in_background(path), Some(SongState::Loading)), "forgotten, so loaded again");
        let _ = std::fs::remove_dir_all(std::path::Path::new(path).parent().unwrap());
    }

    #[test]
    fn a_missing_song_mixes_silently_without_it() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(3.0, &kit.sims, &mut log);
        let plain = mix_scene(&scene, &resolved, &log, &kit);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("song", Value::Text("/nowhere/x.mp3".into()));
        let resolved = resolve(&scene, &kit.registry);
        assert_eq!(mix_scene(&scene, &resolved, &log, &kit).samples, plain.samples);
        let s = root_settings(&resolved);
        assert!(wants_beats(&s), "a song, even a missing one, asks for beats");
        assert_eq!(beats_for(&s).len(), 42, "and without the song's beats the grid is 120 bpm");
        assert!(!wants_beats(&SceneSettings::default()));
        assert!(beats_for(&SceneSettings::default()).is_empty());
    }

    #[test]
    fn a_scene_with_a_song_is_mastered_once() {
        let path = temp_wav("mastered", 4.0, 48_000);
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
        root.params.set("song", Value::Text(path.to_str().unwrap().into()));
        root.params.set("duration", Value::Number(4.0));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(4.0, &kit.sims, &mut log);
        let track = mix_scene(&scene, &resolved, &log, &kit);
        let report = track.report.expect("mastered");
        assert!((report.integrated + 14.0).abs() < 1.0, "{} LUFS", report.integrated);
        assert!(report.true_peak <= -0.9, "{} dBTP", report.true_peak);
        assert!(
            (bpe_mix::integrated_lufs(&track.samples) - report.integrated).abs() < 0.1,
            "the report is of the final mix"
        );
        // A lower ceiling holds over the song too: the master runs last.
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("ceiling", Value::Number(-3.0));
        let resolved = resolve(&scene, &kit.registry);
        let track = mix_scene(&scene, &resolved, &log, &kit);
        let measured = 20.0 * bpe_synth::blocks::true_peak(&track.samples).max(1e-9).log10();
        assert!(measured <= -2.9, "the samples peak at {measured} dBTP");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_song_plays_under_the_mix_and_warp_follows_the_setting() {
        let path = temp_wav("mix", 3.0, 48_000);
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
        root.params.set("song", Value::Text(path.to_str().unwrap().into()));
        root.params.set("duration", Value::Number(3.0));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(3.0, &kit.sims, &mut log);
        let track = mix_scene(&scene, &resolved, &log, &kit);
        assert_eq!(track.frames(), 3 * 48_000);
        // The song's tone fills what would otherwise be silence at the very start.
        assert!(track.samples[..4800].iter().any(|s| s.abs() > 0.01));
        assert!(warp_for(&scene, &resolved, &log).is_identity());
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("beat_sync", Value::Choice("warp".into()));
        let resolved = resolve(&scene, &kit.registry);
        let warp = warp_for(&scene, &resolved, &log);
        assert!(!warp.is_identity() || log.events().iter().all(|e| e.kind != EventKind::Hit));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
