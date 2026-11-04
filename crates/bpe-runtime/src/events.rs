//! The event log: everything that happens in a scene, in step order.
//!
//! Particles, trails and the timeline all read this one log. It is append-only while the
//! simulation runs and identical every time the same scene is simulated (see [`Log::hash`]).

use bpe_core::ThingId;
use glam::Vec2;

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
    /// An item was removed.
    Remove,
}

impl EventKind {
    /// A stable small number per kind, for hashing and compact tables.
    pub fn tag(&self) -> u8 {
        match self {
            EventKind::Hit => 0,
            // 1 and 3 to 7 were kinds that are gone; the numbers stay so hashes stay the same.
            EventKind::Remove => 2,
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
    /// How far through its step a hit happened, 0 (the start) to 1, so sound can place it on
    /// its exact sample; 0 for other events. Not part of [`Log::hash`].
    pub at: f32,
}

impl Event {
    /// The sample within its step that the event happened on, for steps `samples_per_step` long.
    pub fn sample_in_step(&self, samples_per_step: f64) -> f64 {
        self.at as f64 * samples_per_step
    }

    /// Seconds from the start of the clip.
    pub fn time(&self, steps_per_second: u32) -> f64 {
        self.step as f64 / steps_per_second as f64
    }
}

/// How far through a move a contact happened, 0 to 1, from the gap to it at the start (`d0`) and
/// at the end (`d1`, negative once past it), the move taken as straight and steady. A contact
/// that was already touching at the start, or a move that didn't close the gap, gives 0.
pub(crate) fn crossing(d0: f32, d1: f32) -> f32 {
    let closed = d0 - d1;
    // NaN gaps count as no crossing too.
    if d0.is_nan() || closed.is_nan() || d0 <= 0.0 || closed <= 1e-6 {
        return 0.0;
    }
    (d0 / closed).clamp(0.0, 1.0)
}

/// The fraction of a step that `within` (0..1) of substep `index` of `count` is at.
pub(crate) fn step_fraction(index: u32, count: u32, within: f32) -> f32 {
    let count = count.max(1);
    ((index as f32 + within) / count as f32).clamp(0.0, 1.0)
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
            at: 0.0,
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
        let mut d = Log::default();
        d.push(Event { at: 0.75, ..hit() });
        assert_eq!(a.hash(), d.hash(), "where in the step a hit happened is not hashed");
        assert_eq!(Log::default().hash(), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn tags_follow_the_spec_order() {
        assert_eq!(EventKind::Hit.tag(), 0);
        assert_eq!(EventKind::Remove.tag(), 2);
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
