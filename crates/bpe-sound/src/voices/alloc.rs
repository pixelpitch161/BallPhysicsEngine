//! Where a new note goes: a free slot, or (at the voice limit) the oldest note's, which fades out
//! first. Slots are made once, when the patch is built.

/// Notes a shape can hold at once, sounding and fading together.
pub const CAPACITY: usize = 32;
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
    /// sounding note is stolen when the limit is reached.
    pub fn start(&mut self, limit: usize, now: u64) -> Start {
        let limit = limit.clamp(1, CAPACITY);
        let oldest = |meta: &[Meta; CAPACITY], fading: bool| {
            (0..CAPACITY).filter(|&i| meta[i].active && meta[i].fading == fading).min_by_key(|&i| meta[i].started)
        };
        let steal = if self.sounding() >= limit { oldest(&self.meta, false) } else { None };
        if let Some(s) = steal {
            self.meta[s].fading = true;
        }
        let (slot, reused) = match (0..CAPACITY).find(|&i| !self.meta[i].active) {
            Some(free) => (free, false),
            // Every slot busy: the oldest fading one (there is one, since 32 >= the limit), or
            // at worst the stolen note's own.
            None => (oldest(&self.meta, true).or(steal).unwrap_or(0), true),
        };
        let steal = steal.filter(|&s| s != slot);
        self.meta[slot] = Meta { active: true, fading: false, started: now };
        Start { slot, steal, reused }
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
            let s = v.start(4, k);
            assert_eq!(s.steal, None);
        }
        let s = v.start(4, 10);
        assert_eq!(s.steal, Some(0), "the first note makes room");
        assert!(!s.reused);
        assert_eq!(v.sounding(), 4);
        assert_eq!(v.active(), 5);
    }

    #[test]
    fn a_burst_never_passes_the_limit_or_the_slots() {
        let mut v = Voices::new(|| ());
        for k in 0..200 {
            v.start(16, k);
            assert!(v.sounding() <= 16);
            assert!(v.active() <= CAPACITY);
        }
        // Mono: one at a time.
        let mut m = Voices::new(|| ());
        m.start(1, 0);
        let s = m.start(1, 1);
        assert_eq!(s.steal, Some(0));
    }
}
