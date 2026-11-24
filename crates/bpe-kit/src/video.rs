//! Playing a video's clips (`docs/superpowers/specs/2026-11-24-video-arrangement-design.md`):
//! the scene copy a clip plays ([`clip_scene`]), what is on screen at each frame ([`moment`]) and
//! how a scene of another shape covers the video ([`cover`]). The data is `bpe_core::video`.

use bpe_core::video::{Clip, TransitionKind, Video};
use bpe_core::video_save::{self, VideoLoadError};
use bpe_core::{Choice, Registry, Scene, Settings, SlotId, ThingId, Value};

use crate::sound::MixerSettings;
use crate::things::scene::{SceneSettings, TimeBottom};

/// The scene's resolved settings (Scene settings, the root).
pub fn scene_settings(scene: &Scene, reg: &Registry) -> SceneSettings {
    let resolved = bpe_core::resolve(scene, reg);
    resolved.things.get(&ThingId::ROOT).map(|t| SceneSettings::from_params(&t.params)).unwrap_or_default()
}

/// The scene's Length in seconds.
pub fn scene_length(scene: &Scene, reg: &Registry) -> f64 {
    f64::from(scene_settings(scene, reg).duration)
}

/// Opens a video file and puts its clips inside their limits. Every caller opens videos here.
pub fn open_video(text: &str, reg: &Registry) -> Result<Video, VideoLoadError> {
    let mut video = video_save::load(text, reg)?;
    video.tidy(|s| scene_length(s, reg));
    Ok(video)
}

/// The copy of its kept scene a clip plays: at the video's frame rate; when it follows the video's
/// tempo, with the video's tempo, time signature and a First beat that puts the scene's beats on
/// the video's grid; and with its backing track on only when the clip plays the scene's song.
/// Nothing else changes.
pub fn clip_scene(video: &Video, clip: &Clip, reg: &Registry) -> Scene {
    let mut scene = video.scenes.get(&clip.scene).map(|k| k.scene.clone()).unwrap_or_else(|| Scene::empty(reg));
    let s = &video.settings;
    if let Some(root) = scene.things.get_mut(&ThingId::ROOT) {
        root.params.set("fps", Value::Int(i64::from(s.fps)));
        if clip.follow_video_tempo {
            let bottom = TimeBottom::from_name(&s.time_bottom.to_string()).unwrap_or_default();
            root.params.set("tempo", Value::Number(f64::from(s.tempo)));
            root.params.set("time_top", Value::Int(i64::from(s.time_top)));
            root.params.set("time_bottom", Value::Choice(bottom.name().into()));
            root.params.set("first_beat", Value::Number(clip_first_beat(video, clip)));
        }
    }
    let resolved = bpe_core::resolve(&scene, reg);
    let backing_on = resolved
        .things
        .get(&ThingId::ROOT)
        .and_then(|t| t.slots.get(&SlotId::Mixer))
        .is_some_and(|c| MixerSettings::from_params(&c.params).backing_on);
    if let Some(card) = scene.card_mut(ThingId::ROOT, SlotId::Mixer) {
        card.params.set("backing_on", Value::Bool(clip.scene_song && backing_on));
    }
    scene
}

/// The scene First beat that lines the clip's scene up with the video's beat grid: the video's
/// first beat in scene time, wrapped into one bar.
pub fn clip_first_beat(video: &Video, clip: &Clip) -> f64 {
    let bar = video.settings.bar_seconds();
    (video.settings.first_beat - clip.start + clip.offset).rem_euclid(bar)
}

/// A clip on screen, and the scene time it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnScreen {
    /// Its index in `Video::clips`.
    pub clip: usize,
    /// Seconds into its scene.
    pub scene_time: f64,
    /// The scene frame at the video's rate (`scene_time × fps`, exact).
    pub scene_frame: u32,
}

/// What a frame of the video shows: one clip (`new`), two in a transition, or nothing in a gap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub old: Option<OnScreen>,
    pub new: Option<OnScreen>,
    /// How far through the transition, 0 to 1 (0 outside one).
    pub progress: f32,
    /// The transition's look (Cut outside one).
    pub kind: TransitionKind,
}

/// What video frame `frame` shows. Worked out in whole frames, so nothing drifts.
pub fn moment(video: &Video, frame: u32) -> Moment {
    let f = i64::from(frame);
    let fps = f64::from(video.settings.fps.max(1));
    let on = |i: usize| -> Option<OnScreen> {
        let c = &video.clips[i];
        let (start, length, offset) = (video.frames(c.start), video.frames(c.length), video.frames(c.offset));
        (start..start + length).contains(&f).then(|| {
            let scene_frame = f - start + offset;
            OnScreen { clip: i, scene_time: scene_frame as f64 / fps, scene_frame: scene_frame as u32 }
        })
    };
    let mut showing: Vec<OnScreen> = (0..video.clips.len()).filter_map(on).collect();
    let none = Moment { old: None, new: None, progress: 0.0, kind: TransitionKind::Cut };
    match showing.len() {
        0 => none,
        1 => Moment { new: showing.pop(), ..none },
        _ => {
            // Clips overlap only their neighbours, so two at most: the later one is coming in.
            let (old, new) = (showing[0], showing[showing.len() - 1]);
            let clip = &video.clips[new.clip];
            let start = video.frames(clip.start);
            let overlap = video.frames(video.overlap(new.clip)).max(1);
            let progress = ((f - start) as f64 / overlap as f64) as f32;
            Moment { old: Some(old), new: Some(new), progress, kind: clip.transition_in.kind }
        }
    }
}

/// How a scene covers the video: the size it's drawn at and the part of that picture shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoverFit {
    /// The size to draw the scene at: its own shape, both sides at least the video's.
    pub draw: (u32, u32),
    /// The middle part shown, as x, y, width, height in the drawn picture (0 to 1).
    pub crop: [f32; 4],
}

/// The scene's shape scaled to cover the video (sides rounded up to even numbers), and the
/// middle part with the video's shape. The same size is drawn as it is.
pub fn cover(scene_size: (u32, u32), video_size: (u32, u32)) -> CoverFit {
    let (sw, sh) = (f64::from(scene_size.0.max(1)), f64::from(scene_size.1.max(1)));
    let (vw, vh) = (f64::from(video_size.0.max(1)), f64::from(video_size.1.max(1)));
    if scene_size == video_size {
        return CoverFit { draw: video_size, crop: [0.0, 0.0, 1.0, 1.0] };
    }
    let scale = (vw / sw).max(vh / sh);
    let even = |x: f64| ((x - 1e-9).ceil() as u32).div_ceil(2) * 2;
    let draw = (even(sw * scale).max(video_size.0), even(sh * scale).max(video_size.1));
    let (dw, dh) = (f64::from(draw.0), f64::from(draw.1));
    let (w, h) = (vw / dw, vh / dh);
    CoverFit { draw, crop: [((1.0 - w) / 2.0) as f32, ((1.0 - h) / 2.0) as f32, w as f32, h as f32] }
}

/// `fit` drawn no bigger than `max` pixels a side (the graphics card's largest picture), at the
/// same shape and with the same crop: a very wide or tall scene is drawn smaller and stretched.
pub fn within(fit: CoverFit, max: u32) -> CoverFit {
    let big = fit.draw.0.max(fit.draw.1);
    if big <= max {
        return fit;
    }
    let k = f64::from(max) / f64::from(big);
    let even = |x: u32| (((f64::from(x) * k) as u32) / 2 * 2).max(2);
    CoverFit { draw: (even(fit.draw.0), even(fit.draw.1)), crop: fit.crop }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::TrackColour;
    use bpe_core::video::{ClipId, KeptId, KeptScene, Transition};

    fn video(reg: &Registry, clips: &[(f64, f64, f64)]) -> Video {
        let mut v = Video::new();
        let mut scene = crate::new_studio_scene(reg);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(10.0));
        v.scenes.insert(KeptId(0), KeptScene { name: "A".into(), colour: TrackColour::PINK, scene });
        for (i, &(start, length, offset)) in clips.iter().enumerate() {
            v.clips.push(Clip {
                id: ClipId(i as u32),
                start,
                length,
                offset,
                transition_in: Transition { kind: TransitionKind::Crossfade, ..Transition::default() },
                ..Clip::default()
            });
        }
        v
    }

    #[test]
    fn following_the_tempo_sets_the_scene_beat_grid() {
        let reg = crate::registry();
        let mut v = video(&reg, &[(3.0, 4.0, 2.0)]);
        v.settings.tempo = 128.0;
        v.settings.first_beat = 0.5;
        let clip = v.clips[0].clone();
        let s = scene_settings(&clip_scene(&v, &clip, &reg), &reg);
        assert_eq!((s.tempo, s.time_top, s.time_bottom), (128.0, 4, TimeBottom::Four));
        let bar = 60.0 / 128.0 * 4.0;
        assert!((bar - 1.875f64).abs() < 1e-12);
        assert!((f64::from(s.first_beat) - 1.375).abs() < 1e-6, "{}", s.first_beat);
        let first = clip_first_beat(&v, &clip);
        assert!((first - 1.375).abs() < 1e-12);
        let beat = 60.0 / 128.0;
        for scene_time in [0.0, 0.3, 1.375, 2.0, 3.7, 5.99] {
            let in_scene = ((scene_time - first) / beat).rem_euclid(4.0);
            let video_time = scene_time - clip.offset + clip.start;
            let in_video = ((video_time - 0.5) / beat).rem_euclid(4.0);
            assert!((in_scene - in_video).abs() < 1e-9, "{scene_time}: {in_scene} vs {in_video}");
        }
    }

    #[test]
    fn not_following_keeps_the_scene_tempo() {
        let reg = crate::registry();
        let mut v = video(&reg, &[(3.0, 4.0, 2.0)]);
        v.settings.tempo = 128.0;
        v.clips[0].follow_video_tempo = false;
        let s = scene_settings(&clip_scene(&v, &v.clips[0], &reg), &reg);
        assert_eq!((s.tempo, s.first_beat), (120.0, 0.0));
    }

    #[test]
    fn the_clip_scene_plays_at_the_video_rate() {
        let reg = crate::registry();
        let mut v = video(&reg, &[(0.0, 4.0, 0.0)]);
        v.settings.fps = 30;
        assert_eq!(scene_settings(&clip_scene(&v, &v.clips[0], &reg), &reg).fps, 30);
    }

    #[test]
    fn scene_song_off_mutes_the_backing_track() {
        let reg = crate::registry();
        let mut v = video(&reg, &[(0.0, 4.0, 0.0)]);
        let on = |v: &Video| {
            let scene = clip_scene(v, &v.clips[0], &reg);
            let r = bpe_core::resolve(&scene, &reg);
            MixerSettings::from_params(&r.things[&ThingId::ROOT].slots[&SlotId::Mixer].params).backing_on
        };
        assert!(!on(&v));
        v.clips[0].scene_song = true;
        assert!(on(&v));
        let mixer = v.scenes.get_mut(&KeptId(0)).unwrap().scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap();
        mixer.params.set("backing_on", Value::Bool(false));
        assert!(!on(&v), "a backing track the scene has off stays off");
    }

    #[test]
    fn moment_finds_old_and_new_in_a_transition() {
        let reg = crate::registry();
        let v = video(&reg, &[(0.0, 2.0, 0.0), (1.5, 2.5, 1.0)]);
        let m = moment(&v, 30);
        assert_eq!((m.old, m.new.map(|o| o.clip)), (None, Some(0)));
        let m = moment(&v, 90);
        assert_eq!((m.old.map(|o| o.clip), m.new.map(|o| o.clip), m.progress), (Some(0), Some(1), 0.0));
        assert_eq!(m.kind, TransitionKind::Crossfade);
        assert_eq!(moment(&v, 105).progress, 0.5);
        let m = moment(&v, 120);
        assert_eq!((m.old, m.new.map(|o| o.clip)), (None, Some(1)));
        let new = m.new.unwrap();
        assert!((new.scene_time - 1.5).abs() < 1e-12);
        assert_eq!(new.scene_frame, 90);
    }

    #[test]
    fn moment_is_empty_in_a_gap_and_after_the_end() {
        let reg = crate::registry();
        let v = video(&reg, &[(0.0, 1.0, 0.0), (2.0, 1.0, 0.0)]);
        for frame in [60, 90, 119, 180, 500] {
            let m = moment(&v, frame);
            assert!(m.old.is_none() && m.new.is_none(), "frame {frame}");
        }
        assert_eq!(moment(&v, 120).new.map(|o| (o.clip, o.scene_frame)), Some((1, 0)));
    }

    #[test]
    fn cover_fits_a_square_scene() {
        let fit = cover((1080, 1080), (1080, 1920));
        assert_eq!(fit.draw, (1920, 1920));
        assert_eq!(fit.crop, [0.21875, 0.0, 0.5625, 1.0]);
        let wide = cover((1920, 1080), (1080, 1920));
        assert_eq!(wide.draw.1, 1920);
        assert!(wide.draw.0 >= 3412 && wide.draw.0.is_multiple_of(2));
    }

    #[test]
    fn a_cover_too_big_for_the_card_is_drawn_smaller_with_the_same_crop() {
        let fit = cover((7680, 1080), (1080, 1920));
        assert!(fit.draw.0 > 8192, "{:?}", fit.draw);
        let small = within(fit, 8192);
        assert!(small.draw.0 <= 8192 && small.draw.1 <= 8192, "{:?}", small.draw);
        assert!(small.draw.0.is_multiple_of(2) && small.draw.1.is_multiple_of(2));
        let (a, b) = (fit.draw.0 as f64 / fit.draw.1 as f64, small.draw.0 as f64 / small.draw.1 as f64);
        assert!((a - b).abs() / a < 0.01, "same shape: {a} vs {b}");
        assert_eq!(small.crop, fit.crop);
        assert_eq!(within(cover((270, 480), (270, 480)), 8192), cover((270, 480), (270, 480)));
    }

    #[test]
    fn cover_of_the_same_size_is_untouched() {
        assert_eq!(cover((270, 480), (270, 480)), CoverFit { draw: (270, 480), crop: [0.0, 0.0, 1.0, 1.0] });
    }
}
