//! Saving and opening videos (`.ballvideo`, see [`crate::video`]).

use std::fmt;

use crate::save::{LoadError, mend_loaded};
use crate::video::Video;
use crate::{Registry, Scene};

/// The file extension of a video.
pub const VIDEO_EXTENSION: &str = "ballvideo";

/// Why a video file didn't open.
#[derive(Clone, Debug, PartialEq)]
pub enum VideoLoadError {
    /// A later version wrote it, in this format.
    Newer(u32),
    /// It isn't a readable video. Holds the parser's message.
    Broken(String),
    /// One of the scenes kept in it didn't open.
    Scene { name: String, error: LoadError },
    /// A clip (by its id) names a kept scene that isn't in the file.
    MissingScene { clip: u32 },
}

impl fmt::Display for VideoLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VideoLoadError::Newer(_) => write!(f, "This video was made with a newer version of Ball Studio"),
            VideoLoadError::Broken(e) => write!(f, "This video file is damaged: {e}"),
            VideoLoadError::Scene { name, error } => write!(f, "The scene “{name}” in this video didn't open: {error}"),
            VideoLoadError::MissingScene { .. } => write!(f, "A clip in this video names a scene that isn't in it"),
        }
    }
}

impl std::error::Error for VideoLoadError {}

/// The video as pretty RON text. Kept scenes no clip plays, and each kept scene's pictures and
/// sound files nothing names, stay out of the file.
pub fn save(video: &Video) -> String {
    let mut video = video.unused_scenes_removed();
    for kept in video.scenes.values_mut() {
        if let Some(s) = kept.scene.without_unused_pictures() {
            kept.scene = s;
        }
        if let Some(s) = kept.scene.without_unused_audio() {
            kept.scene = s;
        }
    }
    ron::ser::to_string_pretty(&video, ron::ser::PrettyConfig::default()).expect("videos always serialize")
}

/// Opens a saved video, mending each kept scene as opening a scene file does. It doesn't put the
/// clips inside their limits: `bpe_kit::video::open_video` does (it knows each scene's Length).
pub fn load(text: &str, reg: &Registry) -> Result<Video, VideoLoadError> {
    let probe: probe::Video = ron::from_str(text).map_err(|e| VideoLoadError::Broken(e.to_string()))?;
    match probe.format {
        0 => return Err(VideoLoadError::Broken("it has no format number".into())),
        v if v > Video::FORMAT => return Err(VideoLoadError::Newer(v)),
        _ => {}
    }
    let mut video: Video = ron::from_str(text).map_err(|e| VideoLoadError::Broken(e.to_string()))?;
    if let Some(c) = video.all_clips().find(|c| !video.scenes.contains_key(&c.scene)) {
        return Err(VideoLoadError::MissingScene { clip: c.id.0 });
    }
    for kept in video.scenes.values_mut() {
        let scene: &mut Scene = &mut kept.scene;
        mend_loaded(scene, reg).map_err(|error| VideoLoadError::Scene { name: kept.name.clone(), error })?;
    }
    video.format = Video::FORMAT;
    Ok(video)
}

mod probe {
    /// Only the format number; every other field is skipped.
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    pub struct Video {
        pub format: u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_audio::SceneAudio;
    use crate::video::{
        Clip, ClipId, Direction, KeptId, KeptScene, Layout, OnTopClip, Placement, Song, Transition, TransitionKind,
        TransitionSound,
    };
    use crate::{ThingId, TrackColour, testkit};

    fn sample() -> (Video, Registry) {
        let r = testkit::registry();
        let mut v = Video::new();
        v.song = Some(Song {
            name: "beat.wav".into(),
            audio: SceneAudio::new(vec![1, 2, 3, 4, 5, 6, 7, 8].into()),
            starts_at: -0.5,
            level_db: -3.0,
            on: true,
        });
        for (i, name) in ["Ring", "Drop"].into_iter().enumerate() {
            let mut scene = Scene::empty(&r);
            testkit::push(&mut scene, "balls", ThingId::ROOT);
            scene.name = name.into();
            v.scenes.insert(KeptId(i as u32), KeptScene { name: name.into(), colour: TrackColour::LIME, scene });
        }
        v.clips = vec![
            Clip { id: ClipId(0), scene: KeptId(0), start: 0.0, length: 4.0, ..Clip::default() },
            Clip {
                id: ClipId(1),
                scene: KeptId(1),
                start: 3.0,
                length: 4.0,
                offset: 1.5,
                transition_in: Transition {
                    kind: TransitionKind::Wipe { direction: Direction::Up, softness: 0.2 },
                    sound: TransitionSound::KeepBoth,
                },
                ..Clip::default()
            },
            Clip {
                id: ClipId(2),
                scene: KeptId(0),
                start: 6.5,
                length: 2.0,
                level_db: -6.0,
                follow_video_tempo: false,
                scene_song: true,
                transition_in: Transition { kind: TransitionKind::FLASH, sound: TransitionSound::Crossfade },
                ..Clip::default()
            },
        ];
        let mut place = Placement { border_colour: [10, 20, 30], fade_in: 0.5, ..Placement::default() };
        place = place.with_layout(Layout::TopHalf, (1080, 1920));
        let clip = Clip { id: ClipId(7), scene: KeptId(0), start: 1.0, length: 2.0, pan: -0.5, ..Clip::default() };
        v.on_top = vec![OnTopClip { clip, row: 2, place }];
        (v, r)
    }

    #[test]
    fn save_then_load_is_the_same_video() {
        let (v, r) = sample();
        assert_eq!(load(&save(&v), &r).unwrap(), v);
    }

    #[test]
    fn a_clip_with_a_missing_scene_is_refused() {
        let (mut v, r) = sample();
        v.clips[1].scene = KeptId(9);
        let text = ron::ser::to_string_pretty(&v, ron::ser::PrettyConfig::default()).unwrap();
        let e = load(&text, &r).unwrap_err();
        assert_eq!(e, VideoLoadError::MissingScene { clip: 1 });
        assert_eq!(e.to_string(), "A clip in this video names a scene that isn't in it");
        let (mut v, r) = sample();
        v.on_top[0].clip.scene = KeptId(9);
        let text = ron::ser::to_string_pretty(&v, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(load(&text, &r).unwrap_err(), VideoLoadError::MissingScene { clip: 7 }, "on the On top lane too");
    }

    #[test]
    fn a_newer_video_says_so() {
        let (v, r) = sample();
        let text = save(&v).replacen("format: 1", "format: 2", 1);
        let e = load(&text, &r).unwrap_err();
        assert_eq!(e, VideoLoadError::Newer(2));
        assert_eq!(e.to_string(), "This video was made with a newer version of Ball Studio");
    }

    #[test]
    fn a_damaged_video_says_so() {
        let r = testkit::registry();
        assert!(matches!(load("not a video", &r), Err(VideoLoadError::Broken(_))));
        assert!(load("(clips: [])", &r).unwrap_err().to_string().starts_with("This video file is damaged"));
    }

    #[test]
    fn unused_kept_scenes_are_left_out_of_the_file() {
        let (mut v, r) = sample();
        v.scenes.insert(KeptId(7), KeptScene { name: "Spare".into(), ..KeptScene::default() });
        let text = save(&v);
        assert!(!text.contains("Spare"));
        assert_eq!(load(&text, &r).unwrap().scenes.len(), 2);
    }
}
