//! The edits Video mode makes to a video, as plain functions: every one ends with
//! [`Video::tidy`], so a video is inside its limits after every edit (part 2 plan, decisions 3 to 10).

use bpe_core::video::{
    Clip, ClipId, KeptId, KeptScene, Layout, OnTopClip, Placement, Snap, Transition, TransitionKind, Video,
    VideoSettings,
};
use bpe_core::{Registry, Scene, TrackColour};
use bpe_kit::video::{scene_length, scene_settings};

/// The colours kept scenes take in turn: the first one no other kept scene has.
pub const TRACK_COLOURS: [TrackColour; 8] = [
    TrackColour::CYAN,
    TrackColour::PINK,
    TrackColour::LIME,
    TrackColour::ORANGE,
    TrackColour::VIOLET,
    TrackColour::YELLOW,
    TrackColour::TEAL,
    TrackColour::RED,
];

/// Puts every clip inside its limits.
pub fn tidy(video: &mut Video, reg: &Registry) {
    video.tidy(|s| scene_length(s, reg));
}

/// `time` on the video's grid (bar or beat lines from the first beat), or as it is when the grid is
/// off or `free` (Alt held). Never before 0.
pub fn snap(time: f64, settings: &VideoSettings, free: bool) -> f64 {
    let step = match settings.snap {
        _ if free => return time.max(0.0),
        Snap::Off => return time.max(0.0),
        Snap::Bar => settings.bar_seconds(),
        Snap::Beat => settings.beat_seconds(),
    };
    let from = settings.first_beat;
    let lines = ((time - from) / step).round();
    let snapped = from + lines * step;
    if snapped < 0.0 { snapped + step * (-snapped / step).ceil() } else { snapped }
}

/// Where a clip `length` long dragged to `start` lands: its head snaps, unless its tail is nearer a
/// grid line and within `reach` seconds of one (about 8 points on screen), so a clip can be lined
/// up by its tail as well.
pub fn snap_move(start: f64, length: f64, settings: &VideoSettings, free: bool, reach: f64) -> f64 {
    let head = snap(start, settings, free);
    let tail = snap(start + length, settings, free) - length;
    let by_tail = (tail - start).abs();
    if by_tail < (head - start).abs() && by_tail <= reach && tail >= 0.0 { tail } else { head }
}

/// The beat lines between `from` and `to` seconds: each one's time and whether a bar starts there.
pub fn grid_lines(settings: &VideoSettings, from: f64, to: f64) -> Vec<(f64, bool)> {
    let beat = settings.beat_seconds();
    let top = i64::from(settings.time_top.max(1));
    let first = ((from - settings.first_beat) / beat).floor() as i64;
    let mut lines = Vec::new();
    let mut n = first;
    loop {
        let t = settings.first_beat + n as f64 * beat;
        if t > to || lines.len() > 20_000 {
            break;
        }
        if t >= from.max(0.0) {
            lines.push((t, n.rem_euclid(top) == 0));
        }
        n += 1;
    }
    lines
}

/// The bar and beat at `time`, counted from 1 ("3.2" is bar 3, beat 2); before the first beat the
/// bars count back from 0.
pub fn bar_beat(settings: &VideoSettings, time: f64) -> (i64, i64) {
    let beats = ((time - settings.first_beat) / settings.beat_seconds() + 1e-6).floor() as i64;
    let top = i64::from(settings.time_top.max(1));
    (beats.div_euclid(top) + 1, beats.rem_euclid(top) + 1)
}

/// Keeps a copy of `scene` in the video under `name` (or "Scene N"), in the first track colour no
/// other kept scene has.
pub fn keep_scene(video: &mut Video, scene: Scene, name: &str) -> KeptId {
    let id = video.next_kept_id();
    let name = match name.trim() {
        "" => format!("Scene {}", video.scenes.len() + 1),
        name => name.to_owned(),
    };
    let used: Vec<TrackColour> = video.scenes.values().map(|k| k.colour).collect();
    let colour = TRACK_COLOURS
        .iter()
        .copied()
        .find(|c| !used.contains(c))
        .unwrap_or(TRACK_COLOURS[video.scenes.len() % TRACK_COLOURS.len()]);
    video.scenes.insert(id, KeptScene { name, colour, scene });
    id
}

/// A clip playing the whole of kept scene `kept` from `start`: the new clip's id, once tidied.
pub fn add_clip(video: &mut Video, reg: &Registry, kept: KeptId, start: f64) -> Option<ClipId> {
    let scene = &video.scenes.get(&kept)?.scene;
    let id = video.next_clip_id();
    let clip = Clip {
        id,
        scene: kept,
        start: start.max(0.0),
        length: scene_length(scene, reg),
        // An overlap made by dragging shows as a transition at once.
        transition_in: Transition { kind: TransitionKind::Crossfade, ..Transition::default() },
        ..Clip::default()
    };
    video.clips.push(clip);
    tidy(video, reg);
    video.clips.iter().any(|c| c.id == id).then_some(id)
}

/// A clip of the whole scene where the video ends now.
pub fn add_at_end(video: &mut Video, reg: &Registry, kept: KeptId) -> Option<ClipId> {
    let end = video.end();
    add_clip(video, reg, kept, end)
}

fn clip_mut(video: &mut Video, id: ClipId) -> Option<&mut Clip> {
    video.clip_mut(id)
}

/// The clip `id`, on either lane.
pub fn clip(video: &Video, id: ClipId) -> Option<&Clip> {
    video.clip(id)
}

/// Whether clip `id` is on the On top lane.
pub fn is_on_top(video: &Video, id: ClipId) -> bool {
    video.top(id).is_some()
}

/// The placement a new On top clip gets: a frame in the bottom right corner of a video `video`.
pub fn new_place(video: &Video) -> Placement {
    let size = (video.settings.width, video.settings.height);
    Placement::default().with_layout(Layout::Corner(bpe_core::video::Corner::BottomRight), size)
}

/// The first On top row with room from `start` for `length` seconds (no clip on it overlaps), or
/// the last row.
pub fn free_row(video: &Video, start: f64, length: f64) -> u32 {
    (0..Video::ON_TOP_ROWS)
        .find(|&row| {
            video
                .on_top
                .iter()
                .filter(|t| t.row == row)
                .all(|t| t.clip.end() <= start || t.clip.start >= start + length)
        })
        .unwrap_or(Video::ON_TOP_ROWS - 1)
}

/// A clip on the On top lane, row `row`, playing the whole of kept scene `kept` from `start`, in
/// the corner: the new clip's id, once tidied.
pub fn add_on_top(video: &mut Video, reg: &Registry, kept: KeptId, row: u32, start: f64) -> Option<ClipId> {
    let scene = &video.scenes.get(&kept)?.scene;
    let id = video.next_clip_id();
    let clip = Clip { id, scene: kept, start: start.max(0.0), length: scene_length(scene, reg), ..Clip::default() };
    let place = new_place(video);
    video.on_top.push(OnTopClip { clip, row, place });
    tidy(video, reg);
    video.top(id).map(|_| id)
}

/// Moves main clip `id` to the On top lane: the first row with room at its time, in the corner. It
/// keeps its id and everything else.
pub fn to_on_top(video: &mut Video, reg: &Registry, id: ClipId) {
    let Some(i) = index_of(video, id) else { return };
    let clip = video.clips.remove(i);
    let row = free_row(video, clip.start, clip.length);
    let place = new_place(video);
    video.on_top.push(OnTopClip { clip: Clip { transition_in: Transition::default(), ..clip }, row, place });
    tidy(video, reg);
}

/// Moves On top clip `id` to the main lane, with a Crossfade in (as a new main clip has).
pub fn to_main(video: &mut Video, reg: &Registry, id: ClipId) {
    let Some(i) = video.on_top.iter().position(|t| t.clip.id == id) else { return };
    let clip = video.on_top.remove(i).clip;
    let transition_in = Transition { kind: TransitionKind::Crossfade, ..Transition::default() };
    video.clips.push(Clip { transition_in, ..clip });
    tidy(video, reg);
}

/// A clip as Ctrl+C keeps it: the clip and, from the On top lane, its row and placement.
#[derive(Clone, Debug, PartialEq)]
pub struct Copied {
    pub clip: Clip,
    pub top: Option<(u32, Placement)>,
}

/// Clip `id` as copied.
pub fn copy_of(video: &Video, id: ClipId) -> Option<Copied> {
    match video.top(id) {
        Some(t) => Some(Copied { clip: t.clip.clone(), top: Some((t.row, t.place)) }),
        None => clip(video, id).map(|c| Copied { clip: c.clone(), top: None }),
    }
}

/// The clip's place in the main lane (clips are kept sorted by start).
pub fn index_of(video: &Video, id: ClipId) -> Option<usize> {
    video.clips.iter().position(|c| c.id == id)
}

/// Moves clip `id` to start at `start`.
pub fn move_clip(video: &mut Video, reg: &Registry, id: ClipId, start: f64) {
    if let Some(clip) = clip_mut(video, id) {
        clip.start = start.max(0.0);
    }
    tidy(video, reg);
}

/// Moves clip `id`'s left edge to `start`, its offset with it, so the picture at its end stays
/// where it is. Stops where the scene begins and at one frame long.
pub fn trim_start(video: &mut Video, reg: &Registry, id: ClipId, start: f64) {
    let frame = video.seconds(1);
    if let Some(clip) = clip_mut(video, id) {
        let end = clip.end();
        // How far the edge moves: not before the scene's start, not past one frame before the end.
        let most_back = -clip.offset;
        let most_forward = clip.length - frame;
        let shift = (start.max(0.0) - clip.start).clamp(most_back, most_forward.max(most_back));
        clip.start += shift;
        clip.offset += shift;
        clip.length = end - clip.start;
    }
    tidy(video, reg);
}

/// Moves clip `id`'s right edge to `end` (its length; `tidy` stops it at the scene's end).
pub fn trim_end(video: &mut Video, reg: &Registry, id: ClipId, end: f64) {
    if let Some(clip) = clip_mut(video, id) {
        clip.length = end - clip.start;
    }
    tidy(video, reg);
}

/// Makes clip `incoming` overlap the clip before it by `seconds` (its transition's length), by
/// moving its start; `tidy` keeps it to half the shorter clip.
pub fn set_overlap(video: &mut Video, reg: &Registry, incoming: ClipId, seconds: f64) {
    let Some(i) = index_of(video, incoming).filter(|&i| i > 0) else { return };
    let before_end = video.clips[i - 1].end();
    video.clips[i].start = (before_end - seconds.max(0.0)).max(video.clips[i - 1].start);
    tidy(video, reg);
}

/// Takes clip `id` (on either lane) out. False when there was none.
pub fn delete_clip(video: &mut Video, id: ClipId) -> bool {
    let before = video.clips.len() + video.on_top.len();
    video.clips.retain(|c| c.id != id);
    video.on_top.retain(|t| t.clip.id != id);
    video.clips.len() + video.on_top.len() != before
}

/// Puts a copy of `copied` (from this video or another moment) at `start` with an id of its own, on
/// the lane (and row) it came from. Refused when its kept scene is no longer in the video.
pub fn paste_clip(video: &mut Video, reg: &Registry, copied: &Copied, start: f64) -> Option<ClipId> {
    if !video.scenes.contains_key(&copied.clip.scene) {
        return None;
    }
    let id = video.next_clip_id();
    let clip = Clip { id, start: start.max(0.0), ..copied.clip.clone() };
    match copied.top {
        Some((row, place)) => video.on_top.push(OnTopClip { clip, row, place }),
        None => video.clips.push(clip),
    }
    tidy(video, reg);
    video.clip(id).map(|_| id)
}

/// A copy of clip `id` just after it, on its lane.
pub fn duplicate_clip(video: &mut Video, reg: &Registry, id: ClipId) -> Option<ClipId> {
    let original = copy_of(video, id)?;
    let end = original.clip.end();
    paste_clip(video, reg, &original, end)
}

/// Takes kept scene `kept` and every clip playing it (on both lanes) out of the video.
pub fn remove_scene(video: &mut Video, kept: KeptId) {
    video.scenes.remove(&kept);
    video.clips.retain(|c| c.scene != kept);
    video.on_top.retain(|t| t.clip.scene != kept);
}

/// How many clips (on both lanes) play kept scene `kept`.
pub fn clips_of(video: &Video, kept: KeptId) -> usize {
    video.all_clips().filter(|c| c.scene == kept).count()
}

/// A new video made from `scene`: kept and played whole as its first clip at 0, with the scene's
/// size, frame rate, tempo and time signature, so the clip looks and lands as it does alone.
pub fn new_video_from(scene: &Scene, reg: &Registry) -> Video {
    let s = scene_settings(scene, reg);
    let mut video = Video::new();
    video.settings.width = s.width.max(16);
    video.settings.height = s.height.max(16);
    video.settings.fps = s.fps.clamp(1, 240);
    video.settings.tempo = s.tempo.clamp(20.0, 400.0);
    video.settings.time_top = s.time_top.clamp(1, 16);
    video.settings.time_bottom = s.time_bottom.beats();
    video.settings.first_beat = bpe_kit::things::scene::first_beat_seconds(s.first_beat);
    let kept = keep_scene(&mut video, scene.clone(), &scene.name);
    add_clip(&mut video, reg, kept, 0.0);
    video
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;

    fn reg() -> Registry {
        bpe_kit::kit().registry
    }

    /// A scene `seconds` long.
    fn scene_of(reg: &Registry, seconds: f64) -> Scene {
        let mut scene = bpe_kit::new_studio_scene(reg);
        let root = scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap();
        root.params.set("duration", Value::Number(seconds));
        scene
    }

    /// A video at 120 BPM 4/4 (a bar is 2 s) with a kept scene 8 s long.
    fn video(reg: &Registry) -> (Video, KeptId) {
        let mut video = Video::new();
        let kept = keep_scene(&mut video, scene_of(reg, 8.0), "Drop");
        (video, kept)
    }

    #[test]
    fn snap_follows_bars_beats_or_nothing() {
        let mut s = VideoSettings::default();
        assert_eq!(snap(2.9, &s, false), 2.0);
        assert_eq!(snap(3.1, &s, false), 4.0);
        assert_eq!(snap(3.1, &s, true), 3.1, "Alt drags free");
        s.snap = Snap::Beat;
        assert_eq!(snap(1.3, &s, false), 1.5);
        s.snap = Snap::Off;
        assert_eq!(snap(1.3, &s, false), 1.3);
        s.snap = Snap::Bar;
        s.first_beat = 0.5;
        assert_eq!(snap(2.2, &s, false), 2.5, "bars count from the first beat");
        assert_eq!(snap(0.1, &s, false), 0.5, "never before 0");
        assert_eq!(snap(-3.0, &s, false), 0.5);
    }

    #[test]
    fn a_move_snaps_by_its_nearer_end() {
        let s = VideoSettings::default();
        // A clip 3 s long dragged to 2.9: its head is 0.9 off bar 2, its tail (5.9) 0.1 off bar 6.
        assert_eq!(snap_move(2.9, 3.0, &s, false, 0.2), 3.0);
        assert_eq!(snap_move(2.1, 3.0, &s, false, 0.2), 2.0);
        // The tail is nearer but too far away to pull: the head lands on its bar.
        assert_eq!(snap_move(0.6, 3.0, &s, false, 0.16), 0.0);
    }

    #[test]
    fn bars_and_beats_count_from_one() {
        let s = VideoSettings::default();
        assert_eq!(bar_beat(&s, 0.0), (1, 1));
        assert_eq!(bar_beat(&s, 2.5), (2, 2));
        let lines = grid_lines(&s, 0.0, 2.0);
        assert_eq!(lines, vec![(0.0, true), (0.5, false), (1.0, false), (1.5, false), (2.0, true)]);
    }

    #[test]
    fn kept_scenes_are_named_and_take_the_next_colour() {
        let reg = reg();
        let mut video = Video::new();
        let a = keep_scene(&mut video, scene_of(&reg, 4.0), "Ring");
        let b = keep_scene(&mut video, scene_of(&reg, 4.0), "  ");
        assert_eq!(video.scenes[&a].name, "Ring");
        assert_eq!(video.scenes[&b].name, "Scene 2");
        assert_ne!(video.scenes[&a].colour, video.scenes[&b].colour);
        for _ in 0..7 {
            keep_scene(&mut video, scene_of(&reg, 4.0), "x");
        }
        assert_eq!(video.scenes.len(), 9, "a ninth scene still gets a colour");
    }

    #[test]
    fn a_new_clip_plays_its_whole_scene() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        let id = add_clip(&mut video, &reg, kept, 1.0).unwrap();
        let c = clip(&video, id).unwrap();
        assert_eq!((c.start, c.length, c.offset), (1.0, 8.0, 0.0));
        assert_eq!(c.transition_in.kind, TransitionKind::Crossfade);
        let next = add_at_end(&mut video, &reg, kept).unwrap();
        assert_eq!(clip(&video, next).unwrap().start, 9.0);
        assert!(add_clip(&mut video, &reg, KeptId(99), 0.0).is_none(), "no such scene");
    }

    #[test]
    fn moving_a_clip_past_its_neighbour_reorders_them() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        let a = add_clip(&mut video, &reg, kept, 0.0).unwrap();
        let b = add_at_end(&mut video, &reg, kept).unwrap();
        clip_mut(&mut video, b).unwrap().transition_in.kind = TransitionKind::ZoomThrough;
        move_clip(&mut video, &reg, a, 14.0);
        assert_eq!(video.clips[0].id, b, "b comes first now");
        assert_eq!(video.clips[1].id, a);
        assert_eq!(clip(&video, b).unwrap().transition_in.kind, TransitionKind::ZoomThrough, "it keeps its own");
        // a overlaps b (8..16) by 2 s, within half the shorter clip.
        assert_eq!(video.overlap(1), 2.0);
        move_clip(&mut video, &reg, a, 9.0);
        assert_eq!(clip(&video, a).unwrap().start, 12.0, "an overlap stops at half the shorter clip");
    }

    #[test]
    fn trimming_the_left_edge_keeps_the_end_picture() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        let a = add_clip(&mut video, &reg, kept, 2.0).unwrap();
        trim_start(&mut video, &reg, a, 3.5);
        let c = clip(&video, a).unwrap().clone();
        assert_eq!((c.start, c.offset, c.end()), (3.5, 1.5, 10.0));
        trim_start(&mut video, &reg, a, 0.0);
        let c = clip(&video, a).unwrap();
        assert_eq!((c.start, c.offset, c.end()), (2.0, 0.0, 10.0), "stops where the scene begins");
        trim_start(&mut video, &reg, a, 50.0);
        let c = clip(&video, a).unwrap();
        assert_eq!(video.frames(c.length), 1, "stops at one frame long");
        assert_eq!(c.end(), 10.0);
    }

    #[test]
    fn trimming_the_right_edge_stops_at_the_scene_end() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        let a = add_clip(&mut video, &reg, kept, 0.0).unwrap();
        trim_start(&mut video, &reg, a, 2.0);
        trim_end(&mut video, &reg, a, 6.0);
        assert_eq!(clip(&video, a).unwrap().length, 4.0);
        trim_end(&mut video, &reg, a, 40.0);
        assert_eq!(clip(&video, a).unwrap().length, 6.0, "offset 2 + 6 is the scene's 8 s");
    }

    #[test]
    fn an_overlap_is_set_by_moving_the_incoming_clip() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        add_clip(&mut video, &reg, kept, 0.0).unwrap();
        let b = add_at_end(&mut video, &reg, kept).unwrap();
        set_overlap(&mut video, &reg, b, 1.0);
        assert_eq!(video.overlap(1), 1.0);
        set_overlap(&mut video, &reg, b, 7.0);
        assert_eq!(video.overlap(1), 4.0, "half the shorter clip at most");
        set_overlap(&mut video, &reg, b, 0.0);
        assert_eq!(video.overlap(1), 0.0);
    }

    #[test]
    fn delete_paste_duplicate_and_remove() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        let a = add_clip(&mut video, &reg, kept, 0.0).unwrap();
        let copy = copy_of(&video, a).unwrap();
        let d = duplicate_clip(&mut video, &reg, a).unwrap();
        assert_eq!(clip(&video, d).unwrap().start, 8.0);
        let p = paste_clip(&mut video, &reg, &copy, 20.0).unwrap();
        assert_eq!(video.clips.len(), 3);
        assert_ne!(p, a);
        assert!(delete_clip(&mut video, p));
        assert!(!delete_clip(&mut video, p));
        assert_eq!(clips_of(&video, kept), 2);
        remove_scene(&mut video, kept);
        assert!(video.clips.is_empty() && video.scenes.is_empty());
        assert!(paste_clip(&mut video, &reg, &copy, 0.0).is_none(), "its scene is gone");
    }

    #[test]
    fn a_new_video_takes_the_scene_s_shape_and_beat() {
        let reg = reg();
        let mut scene = scene_of(&reg, 6.0);
        scene.name = "Ring Drop".into();
        let root = scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap();
        root.params.set("tempo", Value::Number(128.0));
        root.params.set("fps", Value::Int(30));
        root.params.set("width", Value::Int(1080));
        root.params.set("height", Value::Int(1080));
        let video = new_video_from(&scene, &reg);
        assert_eq!((video.settings.width, video.settings.height, video.settings.fps), (1080, 1080, 30));
        assert_eq!(video.settings.tempo, 128.0);
        assert_eq!(video.clips.len(), 1);
        assert_eq!(video.clips[0].length, 6.0);
        assert_eq!(video.scenes.values().next().unwrap().name, "Ring Drop");
    }

    #[test]
    fn on_top_clips_are_added_moved_and_moved_between_lanes() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        let main = add_clip(&mut video, &reg, kept, 0.0).unwrap();
        let top = add_on_top(&mut video, &reg, kept, 0, 2.0).unwrap();
        assert!(is_on_top(&video, top) && !is_on_top(&video, main));
        assert_eq!(video.top(top).unwrap().place, new_place(&video), "a new one sits in the corner");
        assert_eq!(free_row(&video, 4.0, 1.0), 1, "row 1 is taken there");
        assert_eq!(free_row(&video, 10.0, 1.0), 0);
        move_clip(&mut video, &reg, top, 3.0);
        trim_end(&mut video, &reg, top, 6.0);
        let c = clip(&video, top).unwrap();
        assert_eq!((c.start, c.length), (3.0, 3.0), "the id-based edits find it on the On top lane");
        video.top_mut(top).unwrap().row = 9;
        tidy(&mut video, &reg);
        assert_eq!(video.top(top).unwrap().row, 2, "rows are clamped");
        to_main(&mut video, &reg, top);
        assert!(!is_on_top(&video, top) && clip(&video, top).is_some(), "it keeps its id");
        assert_eq!(clip(&video, top).unwrap().transition_in.kind, TransitionKind::Crossfade);
        to_on_top(&mut video, &reg, top);
        assert!(is_on_top(&video, top));
        assert_eq!(video.top(top).unwrap().row, 0);
        assert_eq!(clips_of(&video, kept), 2);
    }

    #[test]
    fn copy_paste_duplicate_and_delete_keep_the_lane() {
        let reg = reg();
        let (mut video, kept) = video(&reg);
        let top = add_on_top(&mut video, &reg, kept, 1, 0.0).unwrap();
        video.top_mut(top).unwrap().place.opacity = 0.5;
        let copied = copy_of(&video, top).unwrap();
        assert_eq!(copied.top.map(|(row, p)| (row, p.opacity)), Some((1, 0.5)));
        let pasted = paste_clip(&mut video, &reg, &copied, 20.0).unwrap();
        assert_eq!(video.top(pasted).map(|t| (t.row, t.clip.start, t.place.opacity)), Some((1, 20.0, 0.5)));
        let d = duplicate_clip(&mut video, &reg, top).unwrap();
        assert_eq!(video.top(d).map(|t| (t.row, t.clip.start)), Some((1, 8.0)));
        assert!(delete_clip(&mut video, d));
        assert!(video.top(d).is_none());
        remove_scene(&mut video, kept);
        assert!(video.on_top.is_empty());
    }
}
