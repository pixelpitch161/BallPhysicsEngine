//! A Video (`.ballvideo`): a song and scene clips laid out in time, exported as one video
//! (`docs/superpowers/specs/2026-11-24-video-arrangement-design.md`).
//!
//! The scenes a video plays are kept inside it ([`Video::scenes`]), so it never changes when a
//! scene file does. Its clips sit on one main lane, sorted by start; two clips that overlap make a
//! transition as long as the overlap. Times are video seconds, always whole frames of the video
//! (see [`Video::tidy`]). Nothing here simulates or draws.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::scene_audio::SceneAudio;
use crate::{Scene, TrackColour};

/// A video: its settings, its song, the scenes it keeps and the clips that play them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
    pub format: u32,
    pub settings: VideoSettings,
    pub song: Option<Song>,
    pub scenes: BTreeMap<KeptId, KeptScene>,
    pub clips: Vec<Clip>,
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

/// The video's own size, rate, beat grid and Master.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Beats a minute.
    pub tempo: f32,
    /// Beats in a bar.
    pub time_top: u32,
    /// The beat's note: 2, 4, 8 or 16.
    pub time_bottom: u32,
    /// Seconds into the video where bar 1 starts.
    pub first_beat: f64,
    /// What dragging snaps to (the editor's business).
    pub snap: Snap,
    pub master: VideoMaster,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            width: 1080,
            height: 1920,
            fps: 60,
            tempo: 120.0,
            time_top: 4,
            time_bottom: 4,
            first_beat: 0.0,
            snap: Snap::Bar,
            master: VideoMaster::default(),
        }
    }
}

impl VideoSettings {
    /// Seconds in one beat.
    pub fn beat_seconds(&self) -> f64 {
        let tempo = f64::from(self.tempo.max(1.0));
        // The bottom note scales the beat as a scene's time signature does: 4 = a crotchet.
        60.0 / tempo * 4.0 / f64::from(self.time_bottom.max(1))
    }

    /// Seconds in one bar.
    pub fn bar_seconds(&self) -> f64 {
        self.beat_seconds() * f64::from(self.time_top.max(1))
    }
}

/// The video's Master, the same knobs as a scene's: Input and Output (dB), the true-peak
/// limiter's Ceiling (dB) and Release (ms), and the End fade (s).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoMaster {
    pub input: f32,
    pub output: f32,
    pub ceiling: f32,
    pub release: f32,
    pub end_fade: f32,
}

impl Default for VideoMaster {
    fn default() -> Self {
        Self { input: 0.0, output: 0.0, ceiling: -1.0, release: 60.0, end_fade: 0.0 }
    }
}

/// What dragging a clip snaps to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Snap {
    #[default]
    Bar,
    Beat,
    Off,
}

/// The song under the whole video, kept as its file's bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Song {
    pub name: String,
    pub audio: SceneAudio,
    /// Video seconds where the song starts (below 0 skips its intro).
    pub starts_at: f64,
    pub level_db: f32,
    pub on: bool,
}

impl Default for Song {
    fn default() -> Self {
        Self { name: String::new(), audio: SceneAudio::new(Vec::new().into()), starts_at: 0.0, level_db: 0.0, on: true }
    }
}

/// A kept scene's id within its video.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeptId(pub u32);

/// A scene kept inside a video: a copy, so editing it never changes the file it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeptScene {
    pub name: String,
    /// Marks its clips in the arrangement.
    pub colour: TrackColour,
    pub scene: Scene,
}

impl Default for KeptScene {
    fn default() -> Self {
        Self { name: String::new(), colour: TrackColour::CYAN, scene: Scene::default() }
    }
}

/// A clip's id within its video.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClipId(pub u32);

/// A clip: a kept scene played from `offset` for `length`, starting at `start` in the video.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clip {
    pub id: ClipId,
    pub scene: KeptId,
    /// Video seconds where the clip begins.
    pub start: f64,
    /// Seconds it plays.
    pub length: f64,
    /// Seconds into the scene where it begins.
    pub offset: f64,
    /// The scene's sound in the mix; 0 dB is as it is.
    pub level_db: f32,
    /// The scene plays at the video's tempo, time signature and beat grid.
    pub follow_video_tempo: bool,
    /// The scene's own backing track plays.
    pub scene_song: bool,
    /// How the clip before turns into this one (a Cut when they don't overlap).
    pub transition_in: Transition,
}

impl Default for Clip {
    fn default() -> Self {
        Self {
            id: ClipId(0),
            scene: KeptId(0),
            start: 0.0,
            length: 1.0,
            offset: 0.0,
            level_db: 0.0,
            follow_video_tempo: true,
            scene_song: false,
            transition_in: Transition::default(),
        }
    }
}

impl Clip {
    /// Video seconds where the clip ends.
    pub fn end(&self) -> f64 {
        self.start + self.length
    }
}

/// How one clip turns into the next over their overlap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transition {
    pub kind: TransitionKind,
    pub sound: TransitionSound,
}

/// The picture of a transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum TransitionKind {
    #[default]
    Cut,
    Crossfade,
    /// Old to the colour over the first half, the colour to new over the second.
    FadeThrough {
        colour: [u8; 3],
    },
    /// A crossfade with the colour added, peaking near the middle.
    Flash {
        colour: [u8; 3],
    },
    /// A straight edge sweeps across; `softness` is 0 to 0.3 of the screen.
    Wipe {
        direction: Direction,
        softness: f32,
    },
    /// The new picture pushes the old one out.
    Push {
        direction: Direction,
    },
    /// The old picture grows and fades, the new one grows in from small.
    ZoomThrough,
}

impl TransitionKind {
    /// Fade through black.
    pub const FADE_THROUGH: TransitionKind = TransitionKind::FadeThrough { colour: [0, 0, 0] };
    /// A white flash.
    pub const FLASH: TransitionKind = TransitionKind::Flash { colour: [255, 255, 255] };
    /// A wipe to the left, softly edged.
    pub const WIPE: TransitionKind = TransitionKind::Wipe { direction: Direction::Left, softness: 0.05 };
}

/// Which way a wipe or a push goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    #[default]
    Left,
    Right,
}

/// The sound of a transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionSound {
    /// The two clips' sounds crossfade, equal power.
    #[default]
    Crossfade,
    /// Neither fades.
    KeepBoth,
}

impl Video {
    /// The format this version writes.
    pub const FORMAT: u32 = 1;
    /// The longest a video may be, in seconds.
    pub const MAX_SECONDS: f64 = 1800.0;

    /// A video with the defaults and nothing in it.
    pub fn new() -> Self {
        Self {
            format: Self::FORMAT,
            settings: VideoSettings::default(),
            song: None,
            scenes: BTreeMap::new(),
            clips: Vec::new(),
        }
    }

    fn fps(&self) -> f64 {
        f64::from(self.settings.fps.max(1))
    }

    /// `seconds` as a whole number of video frames.
    pub fn frames(&self, seconds: f64) -> i64 {
        (seconds * self.fps()).round() as i64
    }

    /// Video frame `frame` in seconds.
    pub fn seconds(&self, frame: i64) -> f64 {
        frame as f64 / self.fps()
    }

    /// Puts the clips in order and inside every limit: sorted by start; start, length and offset
    /// rounded to whole frames; offset at least 0; the clip inside its scene's Length
    /// (`scene_length`, passed in because bpe-kit resolves it); at least one frame long; inside
    /// [`Video::MAX_SECONDS`]; a clip that would overlap the clip two before moved to start where
    /// the clip before ends; the overlap with the clip before at most half the shorter clip. Clips
    /// whose kept scene is missing go.
    pub fn tidy(&mut self, scene_length: impl Fn(&Scene) -> f64) {
        let scenes = &self.scenes;
        self.clips.retain(|c| scenes.contains_key(&c.scene));
        self.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let max = self.frames(Self::MAX_SECONDS);
        // Each clip as whole frames: (start, length, offset).
        let mut framed: Vec<(i64, i64, i64)> = Vec::with_capacity(self.clips.len());
        let mut keep = Vec::with_capacity(self.clips.len());
        for clip in &self.clips {
            let scene_frames = self.frames(scene_length(&self.scenes[&clip.scene].scene)).max(1);
            let mut start = self.frames(clip.start).max(0);
            let offset = self.frames(clip.offset).clamp(0, scene_frames - 1);
            let mut length = self.frames(clip.length).clamp(1, scene_frames - offset);
            let n = framed.len();
            if n >= 2 {
                let (s2, l2, _) = framed[n - 2];
                if start < s2 + l2 {
                    let (s1, l1, _) = framed[n - 1];
                    start = s1 + l1;
                }
            }
            if let Some(&(s1, l1, _)) = framed.last() {
                // Never before the clip before, and never over half the shorter clip.
                start = start.max(s1);
                let most = l1.min(length) / 2;
                start = start.max(s1 + l1 - most);
            }
            if start >= max {
                keep.push(false);
                continue;
            }
            length = length.min(max - start);
            framed.push((start, length, offset));
            keep.push(true);
        }
        let mut keep = keep.into_iter();
        self.clips.retain(|_| keep.next().unwrap_or(false));
        let fps = self.fps();
        for (clip, (start, length, offset)) in self.clips.iter_mut().zip(framed) {
            clip.start = start as f64 / fps;
            clip.length = length as f64 / fps;
            clip.offset = offset as f64 / fps;
        }
    }

    /// Where the last clip ends: 0 with no clips.
    pub fn end(&self) -> f64 {
        self.clips.iter().map(Clip::end).fold(0.0, f64::max)
    }

    /// Frames in the whole video.
    pub fn frame_count(&self) -> u32 {
        self.frames(self.end()).max(0) as u32
    }

    /// How long clip `i` overlaps the clip before it: 0 for the first, or when they don't overlap.
    pub fn overlap(&self, i: usize) -> f64 {
        if i == 0 || i >= self.clips.len() {
            return 0.0;
        }
        (self.clips[i - 1].end() - self.clips[i].start).max(0.0)
    }

    /// A copy without the kept scenes no clip plays.
    pub fn unused_scenes_removed(&self) -> Video {
        let mut out = self.clone();
        out.scenes.retain(|id, _| self.clips.iter().any(|c| c.scene == *id));
        out
    }

    /// The id the next kept scene gets.
    pub fn next_kept_id(&self) -> KeptId {
        KeptId(self.scenes.keys().next_back().map_or(0, |k| k.0 + 1))
    }

    /// The id the next clip gets.
    pub fn next_clip_id(&self) -> ClipId {
        ClipId(self.clips.iter().map(|c| c.id.0 + 1).max().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_with(clips: &[(f64, f64, f64)], scene_len: f64) -> (Video, impl Fn(&Scene) -> f64) {
        let mut v = Video::new();
        v.scenes.insert(KeptId(0), KeptScene::default());
        for (i, &(start, length, offset)) in clips.iter().enumerate() {
            v.clips.push(Clip { id: ClipId(i as u32), start, length, offset, ..Clip::default() });
        }
        (v, move |_: &Scene| scene_len)
    }

    #[test]
    fn new_video_has_the_defaults() {
        let v = Video::new();
        assert_eq!(v.format, 1);
        let s = &v.settings;
        assert_eq!((s.width, s.height, s.fps), (1080, 1920, 60));
        assert_eq!((s.tempo, s.time_top, s.time_bottom, s.first_beat), (120.0, 4, 4, 0.0));
        assert_eq!(s.snap, Snap::Bar);
        let m = &s.master;
        assert_eq!((m.input, m.output, m.ceiling, m.release, m.end_fade), (0.0, 0.0, -1.0, 60.0, 0.0));
        assert!(v.clips.is_empty() && v.scenes.is_empty() && v.song.is_none());
        let c = Clip::default();
        assert_eq!(c.level_db, 0.0);
        assert!(c.follow_video_tempo);
        assert_eq!(c.transition_in, Transition { kind: TransitionKind::Cut, sound: TransitionSound::Crossfade });
        assert_eq!(TransitionKind::FADE_THROUGH, TransitionKind::FadeThrough { colour: [0, 0, 0] });
        assert_eq!(TransitionKind::FLASH, TransitionKind::Flash { colour: [255; 3] });
        assert_eq!(TransitionKind::WIPE, TransitionKind::Wipe { direction: Direction::Left, softness: 0.05 });
        assert_eq!(Video::MAX_SECONDS, 1800.0);
    }

    #[test]
    fn tidy_rounds_to_whole_frames() {
        let (mut v, len) = video_with(&[(1.004, 2.009, 0.0)], 10.0);
        v.tidy(len);
        assert_eq!(v.clips[0].start, 1.0);
        assert_eq!(v.clips[0].length, 121.0 / 60.0);
        assert!((v.clips[0].length - 2.0167).abs() < 1e-4);
    }

    #[test]
    fn tidy_keeps_clips_inside_their_scene() {
        let (mut v, len) = video_with(&[(0.0, 9.0, 4.0)], 10.0);
        v.tidy(&len);
        assert_eq!((v.clips[0].offset, v.clips[0].length), (4.0, 6.0));
        let (mut v, len) = video_with(&[(0.0, 2.0, -1.0)], 10.0);
        v.tidy(len);
        assert_eq!(v.clips[0].offset, 0.0);
    }

    #[test]
    fn tidy_limits_overlap_to_half_the_shorter_clip() {
        let (mut v, len) = video_with(&[(0.0, 4.0, 0.0), (3.0, 2.0, 0.0)], 10.0);
        v.tidy(&len);
        assert_eq!((v.clips[0].start, v.clips[0].length), (0.0, 4.0));
        assert_eq!(v.clips[1].start, 3.0);
        assert_eq!(v.overlap(1), 1.0);
        let (mut v, len) = video_with(&[(0.0, 4.0, 0.0), (2.5, 2.0, 0.0)], 10.0);
        v.tidy(len);
        assert_eq!(v.clips[1].start, 3.0);
        assert_eq!(v.overlap(1), 1.0);
    }

    #[test]
    fn tidy_moves_a_clip_that_would_overlap_two_away() {
        let (mut v, len) = video_with(&[(0.0, 4.0, 0.0), (3.0, 5.0, 0.0), (3.5, 5.5, 0.0)], 10.0);
        v.tidy(len);
        let (a, c) = (&v.clips[0], &v.clips[2]);
        assert!(c.start >= 7.5, "C starts at {}", c.start);
        assert!(c.start >= a.end());
        assert!(v.overlap(2) <= v.clips[1].length.min(c.length) / 2.0);
    }

    #[test]
    fn tidy_drops_clips_of_missing_scenes_and_keeps_inside_the_longest_video() {
        let (mut v, len) = video_with(&[(0.0, 2.0, 0.0), (1799.0, 5.0, 0.0), (1801.0, 1.0, 0.0)], 10.0);
        v.clips.push(Clip { scene: KeptId(9), ..Clip::default() });
        v.tidy(len);
        assert_eq!(v.clips.len(), 2);
        assert_eq!(v.end(), 1800.0);
        assert_eq!(v.frame_count(), 1800 * 60);
    }

    #[test]
    fn unused_kept_scenes_are_dropped_by_unused_scenes_removed() {
        let (mut v, _) = video_with(&[(0.0, 2.0, 0.0)], 10.0);
        v.scenes.insert(KeptId(1), KeptScene::default());
        let kept = v.unused_scenes_removed();
        assert_eq!(kept.scenes.keys().copied().collect::<Vec<_>>(), vec![KeptId(0)]);
    }
}
