//! Music for a scene: the song under the sounds, the beat grid things pulse and sync to, and
//! beat sync by time warp.

pub mod beats;
pub mod song_mix;
pub mod warp;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

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

static SONGS: Mutex<BTreeMap<String, Option<Arc<Song>>>> = Mutex::new(BTreeMap::new());

/// The song at `path`, decoded and analysed once per path; `None` for an empty path or a file
/// that can't be read (which is remembered too, so a missing file is not retried every step).
pub fn load_song(path: &str) -> Option<Arc<Song>> {
    if path.trim().is_empty() {
        return None;
    }
    let mut songs = SONGS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(song) = songs.get(path) {
        return song.clone();
    }
    let song = bpe_music::decode(path).ok().map(|pcm| {
        let features = bpe_music::analyze_decoded(path, &pcm);
        Arc::new(Song { duration: pcm.duration(), samples: pcm.samples, features })
    });
    songs.insert(path.to_owned(), song.clone());
    song
}

/// Forgets a loaded song, so a changed file is read again.
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
