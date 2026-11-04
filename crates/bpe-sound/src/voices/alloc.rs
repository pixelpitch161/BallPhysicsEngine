//! Where a new note goes: a free slot, or (at the voice limit) the oldest note's, which fades out
//! first. A note never stops without its fade: when a burst brings more hits than there is room
//! for, notes not yet heard (started in this block) give way first, and a hit with no room at
//! all is dropped. Slots are made once, when the patch is built.

/// The highest Voices setting.
pub const MAX_VOICES: usize = 32;
/// Notes a shape can hold at once, sounding and fading together: room for every note at the
/// limit and as many fading out.
pub const CAPACITY: usize = 2 * MAX_VOICES;
/// How long a stolen note takes to fade out.
pub const STEAL_MS: f32 = 5.0;

/// What the allocator knows of one slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub active: bool,
    /// Fading out (stolen or finished); no longer counts against the limit.
    pub fading: bool,
    /// The sound sample it started on (older notes are stolen first).
    pub started: u64,
}

/// A fixed set of note slots.
#[derive(Clone, Debug)]
pub struct Voices<V> {
    pub slots: Vec<V>,
    pub meta: [Meta; CAPACITY],
}

/// Where a new note goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Start {
    pub slot: usize,
    /// A sounding note that must fade out to make room.
    pub steal: Option<usize>,
    /// The slot was still fading (all were busy) and is cut short.
    pub reused: bool,
}

impl<V> Voices<V> {
    /// [`CAPACITY`] slots made by `make` (allocates: build it off the audio path).
    pub fn new(make: impl Fn() -> V) -> Self {
        Voices { slots: (0..CAPACITY).map(|_| make()).collect(), meta: [Meta::default(); CAPACITY] }
    }

    /// Notes sounding and not fading.
    pub fn sounding(&self) -> usize {
        self.meta.iter().filter(|m| m.active && !m.fading).count()
    }

    /// Slots in use, fading ones too.
    pub fn active(&self) -> usize {
        self.meta.iter().filter(|m| m.active).count()
    }

    /// A slot for a note starting at sample `now` with at most `limit` sounding: the oldest
    /// sounding note is stolen when the limit is reached. A note that started at or after
    /// `block_start` has not been heard yet, so it gives up its slot at once; one that has been
    /// heard fades out, and the new note takes a free slot. `None` when there is none: the hit
    /// is dropped rather than a note cut dead.
    pub fn start(&mut self, limit: usize, now: u64, block_start: u64) -> Option<Start> {
        let limit = limit.clamp(1, MAX_VOICES);
        let steal = if self.sounding() >= limit {
            (0..CAPACITY).filter(|&i| self.meta[i].active && !self.meta[i].fading).min_by_key(|&i| self.meta[i].started)
        } else {
            None
        };
        let start = match steal {
            Some(s) if self.meta[s].started >= block_start => Start { slot: s, steal: None, reused: true },
            _ => {
                let free = (0..CAPACITY).find(|&i| !self.meta[i].active)?;
                if let Some(s) = steal {
                    self.meta[s].fading = true;
                }
                Start { slot: free, steal, reused: false }
            }
        };
        self.meta[start.slot] = Meta { active: true, fading: false, started: now };
        Some(start)
    }

    /// The note in `slot` starts fading out.
    pub fn fade(&mut self, slot: usize) {
        self.meta[slot].fading = true;
    }

    /// The note in `slot` is over.
    pub fn free(&mut self, slot: usize) {
        self.meta[slot] = Meta::default();
    }

    /// Every note over.
    pub fn clear(&mut self) {
        self.meta = [Meta::default(); CAPACITY];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_is_stolen_at_the_limit() {
        let mut v = Voices::new(|| ());
        for k in 0..4 {
            let s = v.start(4, k, 0).unwrap();
            assert_eq!(s.steal, None);
        }
        let s = v.start(4, 10, 10).unwrap();
        assert_eq!(s.steal, Some(0), "the first note makes room");
        assert!(!s.reused);
        assert_eq!(v.sounding(), 4);
        assert_eq!(v.active(), 5);
    }

    #[test]
    fn a_burst_never_passes_the_limit_or_the_slots() {
        let mut v = Voices::new(|| ());
        for k in 0..200 {
            v.start(16, k, 0);
            assert!(v.sounding() <= 16);
            assert!(v.active() <= CAPACITY);
        }
        // Mono: one at a time.
        let mut m = Voices::new(|| ());
        m.start(1, 0, 0);
        let s = m.start(1, 1, 1).unwrap();
        assert_eq!(s.steal, Some(0));
    }

    #[test]
    fn notes_not_yet_heard_give_way_and_heard_ones_always_fade() {
        let mut v = Voices::new(|| ());
        for k in 0..MAX_VOICES as u64 {
            v.start(MAX_VOICES, k, 0);
        }
        // A block at sample 1000 brings 100 hits: the 32 heard notes fade, then the new ones
        // (not yet heard) give way to each other.
        for k in 0..100 {
            let s = v.start(MAX_VOICES, 1000 + k % 64, 1000).expect("there is always room for a new block's hits");
            if k < MAX_VOICES as u64 {
                assert!(s.steal.is_some() && !s.reused, "hit {k} fades a heard note");
            } else {
                assert!(s.reused && s.steal.is_none(), "hit {k} takes an unheard note's slot");
            }
        }
        assert_eq!(v.sounding(), MAX_VOICES);
        assert_eq!(v.active(), CAPACITY);
        // The next block, before the fades are over: no room, so the hit is dropped.
        assert!(v.start(MAX_VOICES, 1064, 1064).is_none());
        assert_eq!(v.active(), CAPACITY);
    }
}
