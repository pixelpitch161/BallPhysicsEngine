//! The event log: everything that happens in a scene, in step order.
//!
//! Sound, flashes, rules and the timeline all read this one log. It is append-only while the
//! simulation runs and identical every time the same scene is simulated (see [`Log::hash`]).

use bpe_core::ThingId;
use glam::Vec2;

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
}
