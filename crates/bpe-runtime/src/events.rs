//! The event log: everything that happens in a scene, in step order.
//!
//! Sound, flashes, rules and the timeline all read this one log. It is append-only while the
//! simulation runs and identical every time the same scene is simulated (see [`Log::hash`]).

use bpe_core::ThingId;
use glam::Vec2;
use std::collections::BTreeMap;

use crate::camera::CameraView;

/// One side of an event: a thing, and which of its items (ball number, wall segment, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Party {
    pub thing: ThingId,
    pub item: u32,
}

/// What happened.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    /// Two things touched. `a` is the mover, `b` what it hit.
    Hit,
    /// An item appeared after the start.
    Spawn,
    /// An item was removed.
    Remove,
    /// An item entered a region.
    Enter,
    /// An item left a region.
    Leave,
    /// A beat of the music.
    Beat,
    /// A note was played.
    Note,
    /// Anything a rule or plugin names itself.
    Custom(String),
}

impl EventKind {
    /// A stable small number per kind, for hashing and compact tables.
    pub fn tag(&self) -> u8 {
        match self {
            EventKind::Hit => 0,
            EventKind::Spawn => 1,
            EventKind::Remove => 2,
            EventKind::Enter => 3,
            EventKind::Leave => 4,
            EventKind::Beat => 5,
            EventKind::Note => 6,
            EventKind::Custom(_) => 7,
        }
    }
}

/// One thing that happened, at one step.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The simulation step it happened in.
    pub step: u64,
    pub kind: EventKind,
    /// The item it happened to.
    pub a: Party,
    /// The other side, for hits and the like.
    pub b: Option<Party>,
    /// Where it happened, in scene pixels.
    pub point: Vec2,
    /// The contact normal for hits, pointing away from `b`; zero otherwise.
    pub normal: Vec2,
    /// The speed into the contact for hits, in pixels per second; 0 when it doesn't apply.
    pub speed: f32,
}

impl Event {
    /// Seconds from the start of the clip.
    pub fn time(&self, steps_per_second: u32) -> f64 {
        self.step as f64 / steps_per_second as f64
    }
}

/// The FNV-1a offset basis: the hash of an empty log.
pub const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x100_0000_01b3;

/// Every event so far, in step order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Log {
    events: Vec<Event>,
    /// Where the camera looked, from each step it moved: `(step, view)` in step order. Kept
    /// apart from the events so it never changes the log's hash.
    camera: Vec<(u64, CameraView)>,
    /// Where each ball went after its hits, for sounds that follow it. Not in the hash.
    tracks: BTreeMap<Party, Vec<TrackSample>>,
    /// The whole scene's motion, 60 times a second. Not in the hash.
    scene: Vec<SceneSample>,
    /// The marks things leave on canvases, in step order. Not in the hash.
    marks: Vec<Mark>,
}

/// The shape a mark stamps onto a canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkShape {
    Stroke,
    Splat,
    Ring,
    Star,
    Crack,
    Burst,
    Dots,
    /// A capsule whose radius runs from `radius` to `radius_to`.
    Smear,
}

/// What a mark is painted onto: a canvas thing, or the skin of an arena or obstacle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MarkTarget {
    Canvas(ThingId),
    Skin(ThingId),
}

/// One mark a thing leaves on a canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub step: u64,
    /// What it lands on.
    pub target: MarkTarget,
    pub shape: MarkShape,
    /// Where it starts, in canvas pixels from the canvas centre, before the canvas turns.
    pub from: Vec2,
    /// Where it ends; a stamp has `to == from`.
    pub to: Vec2,
    pub radius: f32,
    /// The radius at `to`; strokes and smears run from `radius` to it, other shapes equal `radius`.
    pub radius_to: f32,
    /// How a stamp is turned. For a stroke that continues a piece (variant bit 1), the direction
    /// of that piece, which sets where the two are cut apart.
    pub angle: f32,
    /// For a stroke, the part of its step (0 its start, 1 its end) over which the ball travelled
    /// along it; a piece of a longer move covers a part. The graphics card orders overlapping
    /// paint by it. Other marks: `[1.0, 1.0]`.
    pub within: [f32; 2],
    /// Which of a stamp shape's variants; for a stroke, bit 0 is its tip (0 round, 1 square),
    /// bit 1 says it continues the previous piece (no start cap) and bit 2 that the next piece
    /// continues it (no end cap).
    pub variant: u8,
    /// Linear light, not sRGB.
    pub colour: [f32; 4],
    /// The colour at `to`; equal to `colour` for marks of one colour.
    pub colour_to: [f32; 4],
}

/// How often motion is sampled for sound, per second.
pub const MOTION_RATE: u32 = 60;
/// How long a ball's track runs on after each of its hits, in seconds.
pub const TRACK_SECONDS: f64 = 12.0;

/// One moment of a ball's track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackSample {
    pub step: u64,
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
}

/// One moment of the whole scene's motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSample {
    pub step: u64,
    /// Balls alive, not counting silent ones (liquid).
    pub balls: u32,
    /// Σ ½·(r/20)²·|v|² / 10⁶ over those balls.
    pub energy: f32,
}

impl Log {
    /// Appends an event. Events must arrive in step order.
    pub fn push(&mut self, e: Event) {
        if let Some(last) = self.events.last() {
            debug_assert!(e.step >= last.step, "event log step order: {} after {}", e.step, last.step);
        }
        self.events.push(e);
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Notes where the camera looks from `step` on; nothing is kept when it hasn't moved.
    pub fn push_camera(&mut self, step: u64, view: CameraView) {
        if self.camera.last().is_none_or(|(_, last)| *last != view) {
            self.camera.push((step, view));
        }
    }

    /// Adds a moment of a ball's track. Samples arrive in step order.
    pub fn push_track(&mut self, ball: Party, s: TrackSample) {
        self.tracks.entry(ball).or_default().push(s);
    }

    /// A ball's track, in step order; empty when nothing was recorded.
    pub fn track(&self, ball: Party) -> &[TrackSample] {
        self.tracks.get(&ball).map_or(&[], |t| t.as_slice())
    }

    /// Adds a moment of the scene's motion.
    pub fn push_scene(&mut self, s: SceneSample) {
        self.scene.push(s);
    }

    pub fn scene_samples(&self) -> &[SceneSample] {
        &self.scene
    }

    /// Forgets the motion after `step`, when playback rewinds.
    pub fn truncate_motion(&mut self, step: u64) {
        for t in self.tracks.values_mut() {
            t.truncate(t.partition_point(|s| s.step <= step));
        }
        self.tracks.retain(|_, t| !t.is_empty());
        self.scene.truncate(self.scene.partition_point(|s| s.step <= step));
    }

    /// Adds a mark. Marks must arrive in step order.
    pub fn push_mark(&mut self, m: Mark) {
        if let Some(last) = self.marks.last() {
            debug_assert!(m.step >= last.step, "mark log step order: {} after {}", m.step, last.step);
        }
        self.marks.push(m);
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// The marks aimed at `target`, in the order they were made.
    pub fn marks_for(&self, target: MarkTarget) -> impl Iterator<Item = &Mark> {
        self.marks.iter().filter(move |m| m.target == target)
    }

    /// The marks made in steps `(after, upto]`.
    pub fn marks_between(&self, after: u64, upto: u64) -> &[Mark] {
        let start = self.marks.partition_point(|m| m.step <= after);
        let end = self.marks.partition_point(|m| m.step <= upto);
        &self.marks[start..end.max(start)]
    }

    /// Forgets the marks made after `step`, when playback rewinds.
    pub fn trim_marks_upto(&mut self, step: u64) {
        self.marks.truncate(self.marks.partition_point(|m| m.step <= step));
    }

    /// Forgets the marks made at or before `step`, once they are drawn and nothing reads them.
    pub fn drop_marks_upto(&mut self, step: u64) {
        self.marks.drain(..self.marks.partition_point(|m| m.step <= step));
    }

    /// Where the camera looked at `step`: the identity before it was first noted.
    pub fn camera_at(&self, step: u64) -> CameraView {
        let i = self.camera.partition_point(|(s, _)| *s <= step);
        if i == 0 { CameraView::default() } else { self.camera[i - 1].1 }
    }

    /// Forgets where the camera looked after `step`, when playback rewinds.
    pub fn truncate_camera(&mut self, step: u64) {
        let keep = self.camera.partition_point(|(s, _)| *s <= step);
        self.camera.truncate(keep);
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Drops every event after the first `len`, when playback rewinds.
    pub fn truncate(&mut self, len: usize) {
        self.events.truncate(len);
    }

    /// The events at or after `step`.
    pub fn since(&self, step: u64) -> &[Event] {
        let start = self.events.partition_point(|e| e.step < step);
        &self.events[start..]
    }

    /// The events from `from` (inclusive) to `to` (exclusive).
    pub fn between(&self, from: u64, to: u64) -> &[Event] {
        let start = self.events.partition_point(|e| e.step < from);
        let end = self.events.partition_point(|e| e.step < to);
        &self.events[start..end.max(start)]
    }

    /// Bytes the log's memory takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.events.capacity() * std::mem::size_of::<Event>()
            + self.tracks.values().map(|t| t.capacity() * std::mem::size_of::<TrackSample>()).sum::<usize>()
            + self.scene.capacity() * std::mem::size_of::<SceneSample>()
            + self.marks.capacity() * std::mem::size_of::<Mark>()
    }

    /// An FNV-1a hash of every field of every event. Equal logs hash equal on every platform.
    pub fn hash(&self) -> u64 {
        let mut h = HASH_OFFSET;
        let mut feed = |bytes: &[u8]| {
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(HASH_PRIME);
            }
        };
        for e in &self.events {
            feed(&e.step.to_le_bytes());
            feed(&[e.kind.tag()]);
            if let EventKind::Custom(name) = &e.kind {
                feed(name.as_bytes());
            }
            feed(&e.a.thing.0.to_le_bytes());
            feed(&e.a.item.to_le_bytes());
            match e.b {
                Some(b) => {
                    feed(&[1]);
                    feed(&b.thing.0.to_le_bytes());
                    feed(&b.item.to_le_bytes());
                }
                None => feed(&[0]),
            }
            feed(&e.point.x.to_bits().to_le_bytes());
            feed(&e.point.y.to_bits().to_le_bytes());
            feed(&e.normal.x.to_bits().to_le_bytes());
            feed(&e.normal.y.to_bits().to_le_bytes());
            feed(&e.speed.to_bits().to_le_bytes());
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::ThingId;
    use glam::Vec2;

    fn hit() -> Event {
        Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(2), item: 0 },
            b: Some(Party { thing: ThingId(1), item: 0 }),
            point: Vec2::new(1.0, 2.0),
            normal: Vec2::Y,
            speed: 1.0,
        }
    }

    #[test]
    fn slices_follow_steps() {
        let mut log = Log::default();
        for step in [1, 1, 3, 4, 4, 9] {
            log.push(Event { step, ..hit() });
        }
        assert_eq!(log.since(4).len(), 3);
        assert_eq!(log.between(1, 4).len(), 3);
        assert_eq!(log.between(5, 5).len(), 0);
    }

    #[test]
    fn hash_changes_with_any_field_and_not_with_time() {
        let mut a = Log::default();
        a.push(hit());
        let mut b = Log::default();
        b.push(Event { speed: 2.0, ..hit() });
        assert_ne!(a.hash(), b.hash());
        let mut c = Log::default();
        c.push(hit());
        assert_eq!(a.hash(), c.hash());
        assert_eq!(Log::default().hash(), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn tags_follow_the_spec_order() {
        let kinds = [
            EventKind::Hit,
            EventKind::Spawn,
            EventKind::Remove,
            EventKind::Enter,
            EventKind::Leave,
            EventKind::Beat,
            EventKind::Note,
            EventKind::Custom("x".into()),
        ];
        for (i, kind) in kinds.iter().enumerate() {
            assert_eq!(kind.tag() as usize, i);
        }
    }

    #[test]
    fn time_divides_step_by_rate() {
        assert_eq!(Event { step: 120, ..hit() }.time(240), 0.5);
    }

    #[test]
    fn truncate_drops_the_tail() {
        let mut log = Log::default();
        for step in [1, 2, 3] {
            log.push(Event { step, ..hit() });
        }
        log.truncate(1);
        assert_eq!(log.len(), 1);
        assert!(!log.is_empty());
        assert_eq!(log.events()[0].step, 1);
    }

    #[test]
    #[should_panic(expected = "step order")]
    fn pushing_an_earlier_step_panics_in_debug() {
        let mut log = Log::default();
        log.push(Event { step: 5, ..hit() });
        log.push(Event { step: 4, ..hit() });
    }

    fn mark(step: u64) -> Mark {
        Mark {
            step,
            target: MarkTarget::Canvas(ThingId(3)),
            shape: MarkShape::Splat,
            from: Vec2::ZERO,
            to: Vec2::ZERO,
            radius: 4.0,
            radius_to: 4.0,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour: [1.0, 0.5, 0.25, 1.0],
            colour_to: [1.0, 0.5, 0.25, 1.0],
        }
    }

    #[test]
    fn mark_target_filters_marks() {
        let mut log = Log::default();
        let (canvas, skin) = (MarkTarget::Canvas(ThingId(3)), MarkTarget::Skin(ThingId(3)));
        for (step, target) in [(1, canvas), (1, skin), (2, skin), (3, canvas)] {
            log.push_mark(Mark { target, ..mark(step) });
        }
        let steps = |t| log.marks_for(t).map(|m| m.step).collect::<Vec<_>>();
        assert_eq!(steps(canvas), [1, 3]);
        assert_eq!(steps(skin), [1, 2]);
        assert_eq!(log.marks_for(MarkTarget::Canvas(ThingId(4))).count(), 0);
        assert!(canvas < skin, "canvases sort before skins");
    }

    #[test]
    fn marks_between_picks_the_step_range() {
        let mut log = Log::default();
        for step in [1, 2, 2, 5] {
            log.push_mark(mark(step));
        }
        assert_eq!(log.marks_between(1, 2).len(), 2);
        assert_eq!(log.marks_between(2, 5).len(), 1);
        assert!(log.marks_between(5, 9).is_empty());
        assert!(log.marks_between(9, 3).is_empty());
    }

    #[test]
    fn marks_do_not_change_the_hash() {
        let mut plain = Log::default();
        plain.push(hit());
        let mut marked = plain.clone();
        marked.push_mark(mark(1));
        assert_eq!(plain.hash(), marked.hash());
        assert_eq!(marked.marks().len(), 1);
    }

    #[test]
    fn trim_drops_old_marks() {
        let mut log = Log::default();
        for step in [1, 2, 2, 5] {
            log.push_mark(mark(step));
        }
        log.trim_marks_upto(2);
        assert_eq!(log.marks().len(), 3);
        log.trim_marks_upto(0);
        assert!(log.marks().is_empty());
    }

    #[test]
    fn drop_forgets_drawn_marks() {
        let mut log = Log::default();
        for step in [1, 2, 2, 5] {
            log.push_mark(mark(step));
        }
        log.drop_marks_upto(2);
        assert_eq!(log.marks().iter().map(|m| m.step).collect::<Vec<_>>(), [5]);
        log.drop_marks_upto(9);
        assert!(log.marks().is_empty());
    }

    #[test]
    fn camera_track_ignores_hash() {
        let mut log = Log::default();
        log.push(hit());
        let before = log.hash();
        let view = CameraView { center: Vec2::new(10.0, 20.0), zoom: 2.0, turn: 0.1 };
        log.push_camera(3, view);
        log.push_camera(4, view);
        assert_eq!(log.hash(), before);
        assert_eq!(log.camera_at(5), view);
        assert_eq!(log.camera_at(1), CameraView::default());
        log.truncate_camera(2);
        assert_eq!(log.camera_at(5), CameraView::default());
    }
}
