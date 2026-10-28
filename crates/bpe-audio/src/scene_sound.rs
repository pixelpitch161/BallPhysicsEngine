//! Everything about a scene's sound, worked out together so the preview, the export and the
//! timeline all agree: the song and its beats, the event log the simulation makes with them,
//! beat sync's time warp, and the mixed soundtrack.

use crate::{Mix, MixOptions};
use anyhow::Context;
use bpe_music::AudioFeatures;
use bpe_scene::{Scene, SyncMode};
use bpe_sim::beat_sync::{self, TimeWarp};
use bpe_sim::{Event, SimInputs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// A song ready to mix and follow.
#[derive(Clone, Debug)]
pub struct Song {
    /// Interleaved stereo at [`crate::SAMPLE_RATE`].
    pub samples: Arc<Vec<f32>>,
    /// The song's beats and levels, in song time.
    pub features: Arc<AudioFeatures>,
    pub path: PathBuf,
}

/// The last few songs loaded, by file, size and change time, so edits don't decode again.
static SONGS: Mutex<Vec<KeptSong>> = Mutex::new(Vec::new());
/// A loaded song with the file's path, size and change time when it was read.
type KeptSong = (PathBuf, u64, Option<SystemTime>, Song);
const KEPT_SONGS: usize = 3;

/// Loads the scene's song, if it has one. Relative paths start from `base_dir`.
pub fn load_song(scene: &Scene, base_dir: Option<&Path>) -> anyhow::Result<Option<Song>> {
    let Some(path) = scene.music.song.as_deref().filter(|p| !p.trim().is_empty()) else { return Ok(None) };
    let full = match base_dir {
        Some(dir) if Path::new(path).is_relative() => dir.join(path),
        _ => PathBuf::from(path),
    };
    let meta = std::fs::metadata(&full).with_context(|| format!("can't find the song {}", full.display()))?;
    let (len, changed) = (meta.len(), meta.modified().ok());
    {
        let kept = SONGS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((.., song)) = kept.iter().find(|(p, l, c, _)| *p == full && *l == len && *c == changed) {
            return Ok(Some(song.clone()));
        }
    }
    let pcm = bpe_music::decode(&full)?;
    let features = bpe_music::analyze_decoded(&full, &pcm);
    let song = Song { samples: Arc::new(pcm.samples), features: Arc::new(features), path: full.clone() };
    let mut kept = SONGS.lock().unwrap_or_else(|e| e.into_inner());
    kept.retain(|(p, ..)| *p != full);
    if kept.len() >= KEPT_SONGS {
        kept.remove(0);
    }
    kept.push((full, len, changed, song.clone()));
    Ok(Some(song))
}

/// The scene's song if it's already loaded, without reading the file. The editor uses this to
/// start the preview with the right beats straight after an edit.
pub fn cached_song(scene: &Scene, base_dir: Option<&Path>) -> Option<Song> {
    let path = scene.music.song.as_deref().filter(|p| !p.trim().is_empty())?;
    let full = match base_dir {
        Some(dir) if Path::new(path).is_relative() => dir.join(path),
        _ => PathBuf::from(path),
    };
    let meta = std::fs::metadata(&full).ok()?;
    let kept = SONGS.lock().unwrap_or_else(|e| e.into_inner());
    kept.iter()
        .find(|(p, l, c, _)| *p == full && *l == meta.len() && *c == meta.modified().ok())
        .map(|(.., song)| song.clone())
}

/// The song's analysis moved into video time: the video starts `offset` seconds into the song.
pub fn in_video_time(features: &AudioFeatures, offset: f32) -> AudioFeatures {
    let offset = offset as f64;
    let shift = |times: &[f64]| times.iter().map(|t| t - offset).filter(|t| *t >= 0.0).collect::<Vec<_>>();
    let frames = (offset * features.rate as f64).round() as i64;
    let move_levels = |levels: &[f32]| -> Vec<f32> {
        if frames >= 0 {
            levels.iter().skip(frames as usize).copied().collect()
        } else {
            std::iter::repeat_n(0.0, (-frames) as usize).chain(levels.iter().copied()).collect()
        }
    };
    AudioFeatures {
        rate: features.rate,
        bass: move_levels(&features.bass),
        mid: move_levels(&features.mid),
        treble: move_levels(&features.treble),
        level: move_levels(&features.level),
        onset: move_levels(&features.onset),
        beats: shift(&features.beats),
        downbeats: shift(&features.downbeats),
        bpm: features.bpm,
    }
}

/// What the simulation should follow for this scene: the song (in video time) or a steady beat.
pub fn sim_inputs(scene: &Scene, song: Option<&Song>) -> SimInputs {
    let features = song.map(|s| Arc::new(in_video_time(&s.features, scene.music.offset)));
    beat_sync::inputs(scene, features)
}

/// A scene's sound and the event log it was made from.
#[derive(Clone, Debug, Default)]
pub struct SceneSound {
    pub mix: Mix,
    /// Every event, in simulation time.
    pub events: Vec<Event>,
    /// What to simulate with, so pictures match the sound. With a time warp, the beats in here
    /// are in simulation time.
    pub inputs: SimInputs,
    /// Warp mode's clock, when on. Show simulation time `warp.sim_time(t)` at video time `t`.
    pub warp: Option<TimeWarp>,
    /// Beats (with Lock's subdivisions) in video time, for the timeline and the on-beat share.
    pub beats: Vec<f64>,
    /// Share of sounding hits within the beat sync tolerance of a beat, when there are beats.
    pub on_beat: Option<f32>,
    /// Why the song couldn't be used, if it couldn't. The rest of the sound still plays.
    pub song_error: Option<String>,
    /// The song, for drawing its waveform.
    pub song: Option<Song>,
}

impl SceneSound {
    /// The event log with every time in video time, for things drawn along the video.
    pub fn video_events(&self) -> Vec<Event> {
        let mut events = self.events.clone();
        if let Some(warp) = &self.warp {
            events.iter_mut().for_each(|e| e.time = warp.video_time(e.time));
        }
        events
    }

    /// The simulation time to show at video time `time`.
    pub fn sim_time(&self, time: f64) -> f64 {
        self.warp.as_ref().map_or(time, |w| w.sim_time(time))
    }
}

/// What a scene's simulation follows and the clock it runs on, worked out without making any
/// sound: enough to show any moment of the scene.
#[derive(Clone, Debug, Default)]
pub struct SceneClock {
    pub inputs: SimInputs,
    pub warp: Option<TimeWarp>,
    pub song: Option<Song>,
    pub song_error: Option<String>,
}

impl SceneClock {
    /// The simulation time to show at video time `time`.
    pub fn sim_time(&self, time: f64) -> f64 {
        self.warp.as_ref().map_or(time, |w| w.sim_time(time))
    }
}

/// Loads the scene's song and, in Warp mode, fits the clock to it.
pub fn scene_clock(scene: &Scene, base_dir: Option<&Path>) -> SceneClock {
    let (song, song_error) = match load_song(scene, base_dir) {
        Ok(song) => (song, None),
        Err(error) => (None, Some(format!("{error:#}"))),
    };
    let mut inputs = sim_inputs(scene, song.as_ref());
    let mut warp = None;
    if scene.music.beat_sync.mode == SyncMode::Warp {
        // Fit the warp to a plain run, then move the beats (and the song's bars) into
        // simulation time so beat events and rules on them line up with the picture.
        let duration = scene.video.duration as f64;
        let reach = duration * (1.0 + scene.music.beat_sync.warp_strength.clamp(0.0, 0.9) as f64) + 1.0;
        let plain = simulate(scene, &inputs, reach);
        let fitted = TimeWarp::fit(scene, &plain, &beat_sync::beat_grid(scene, &inputs.beats));
        let to_sim = |times: &[f64]| times.iter().map(|&t| fitted.sim_time(t)).collect::<Vec<_>>();
        inputs.beats = to_sim(&inputs.beats);
        if let Some(music) = &inputs.music {
            let mut moved = (**music).clone();
            moved.beats = to_sim(&music.beats);
            moved.downbeats = to_sim(&music.downbeats);
            inputs.music = Some(Arc::new(moved));
        }
        warp = Some(fitted);
    }
    SceneClock { inputs, warp, song, song_error }
}

/// Simulates the scene with its song and beat sync, and mixes its soundtrack.
pub fn render_scene(scene: &Scene, base_dir: Option<&Path>) -> anyhow::Result<SceneSound> {
    let SceneClock { inputs, warp, song, song_error } = scene_clock(scene, base_dir);
    let duration = scene.video.duration as f64;
    let end = warp.as_ref().map_or(duration, |w| w.sim_end(duration));
    let events = simulate(scene, &inputs, end);
    let map = |t: f64| warp.as_ref().map_or(t, |w| w.video_time(t));
    let options = MixOptions {
        base_dir,
        song: song.as_ref().map(|s| s.samples.as_slice()),
        time_map: warp.as_ref().map(|_| &map as &(dyn Fn(f64) -> f64 + Sync)),
    };
    let mix = crate::mix(scene, &events, &options)?;
    let video_beats: Vec<f64> = inputs.beats.iter().map(|&b| map(b)).collect();
    let beats = beat_sync::subdivided(&video_beats, scene.music.beat_sync.subdivisions);
    let on_beat = (!beats.is_empty()).then(|| {
        let min_speed = scene.sound.min_impact_speed;
        let sounding = events.iter().filter(|e| e.kind.is_bounce() && e.speed >= min_speed).map(|e| map(e.time));
        beat_sync::on_beat_share_of(sounding.filter(|&t| t <= duration), &beats, scene.music.beat_sync.tolerance as f64)
    });
    Ok(SceneSound { mix, events, inputs, warp, beats, on_beat, song_error, song })
}

fn simulate(scene: &Scene, inputs: &SimInputs, until: f64) -> Vec<Event> {
    bpe_sim::collect_events_until(scene, inputs, until)
}

/// Share of hits and escapes (0 to 1) within `tolerance` seconds of a beat, for the scorer
/// and the editor. `beats` must be sorted.
pub fn on_beat_share(events: &[Event], beats: &[f64], tolerance: f64) -> f32 {
    beat_sync::on_beat_share(events, beats, tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_analysis_moves_into_video_time() {
        let features = AudioFeatures {
            rate: 200.0,
            level: (0..400).map(|i| i as f32).collect(),
            bass: vec![0.0; 400],
            mid: vec![0.0; 400],
            treble: vec![0.0; 400],
            onset: vec![0.0; 400],
            beats: vec![0.5, 1.0, 1.5],
            downbeats: vec![0.5],
            bpm: 120.0,
        };
        let later = in_video_time(&features, 1.0);
        assert_eq!(later.beats, vec![0.0, 0.5]);
        assert!(later.downbeats.is_empty());
        assert_eq!(later.level[0], 200.0);
        let earlier = in_video_time(&features, -0.5);
        assert_eq!(earlier.beats, vec![1.0, 1.5, 2.0]);
        assert_eq!(earlier.level[100], 0.0);
        assert_eq!(earlier.level.len(), 500);
    }

    #[test]
    fn plain_scenes_have_no_beats_and_sound_as_before() {
        let scene = Scene::default();
        let sound = render_scene(&scene, None).unwrap();
        assert!(sound.inputs.beats.is_empty() && sound.on_beat.is_none() && sound.warp.is_none());
        let classic = crate::render_soundtrack(&scene, &bpe_sim::collect_wall_hits(&scene), None).unwrap();
        assert_eq!(sound.mix.track.samples, classic.samples);
    }

    #[test]
    fn a_missing_song_is_reported_but_the_bounces_still_play() {
        let mut scene = Scene::default();
        scene.video.duration = 3.0;
        scene.music.song = Some("/no/such/song.mp3".into());
        let sound = render_scene(&scene, None).unwrap();
        assert!(sound.song_error.is_some());
        assert!(sound.mix.track.samples.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn warp_mode_maps_hits_onto_beats() {
        let mut scene = Scene::growing_ball();
        scene.rules = Default::default();
        scene.video.duration = 12.0;
        scene.music.bpm = 110.0;
        scene.music.beat_sync.mode = SyncMode::Warp;
        let sound = render_scene(&scene, None).unwrap();
        assert!(sound.warp.is_some());
        assert!(sound.on_beat.unwrap() > 0.85, "{:?}", sound.on_beat);
        // Played notes are in video time, on the beat grid.
        let on: Vec<f64> = sound.mix.notes.iter().map(|n| n.time).filter(|&t| t < 11.0).collect();
        let share = beat_sync::on_beat_share_of(on, &sound.beats, 0.02);
        assert!(share > 0.85, "{share}");
    }
}
