//! A list of a ball's samples or corners that copies cheaply and keeps repeats small (W7, S10 and
//! S14). Full chunks of [`CHUNK`] items are frozen and shared by every copy of the world, so a jump
//! point copies a few pointers and the open tail, not a whole path; and the same item over and over
//! at the end (a ball that has gone, or rests) is kept once with a count. It reads exactly as the
//! plain list it replaces: the same items at the same places.

use std::collections::VecDeque;
use std::sync::Arc;

/// Items in a frozen chunk.
pub const CHUNK: usize = 64;

/// A list with cheap copies: frozen shared chunks, an open tail, and a run of one repeated item at
/// the end.
#[derive(Clone, Debug)]
pub struct Track<T> {
    /// Full chunks of exactly [`CHUNK`] items, shared by every copy.
    chunks: VecDeque<Arc<[T]>>,
    /// The stored items after the chunks: fewer than [`CHUNK`].
    tail: Vec<T>,
    /// Stored items dropped from the front (of the first chunk, or of the tail when there are no
    /// chunks): always fewer than the stored items, or 0 with none.
    skip: usize,
    /// The newest item and how many times it comes in a row after the stored ones (at least
    /// once); `None` only when the list is empty.
    run: Option<(T, usize)>,
}

impl<T> Default for Track<T> {
    fn default() -> Self {
        Self { chunks: VecDeque::new(), tail: Vec::new(), skip: 0, run: None }
    }
}

impl<T: Copy + PartialEq> Track<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Items stored one by one (the run left out).
    fn stored(&self) -> usize {
        self.chunks.len() * CHUNK + self.tail.len() - self.skip
    }

    pub fn len(&self) -> usize {
        self.stored() + self.run.map_or(0, |r| r.1)
    }

    pub fn is_empty(&self) -> bool {
        self.run.is_none()
    }

    /// Whether every item is the same one (and there is at least one).
    pub fn is_one_run(&self) -> bool {
        self.run.is_some() && self.stored() == 0
    }

    /// The item at `i`, oldest first.
    pub fn get(&self, i: usize) -> Option<T> {
        let stored = self.stored();
        if i < stored {
            let j = i + self.skip;
            let in_chunks = self.chunks.len() * CHUNK;
            return Some(if j < in_chunks { self.chunks[j / CHUNK][j % CHUNK] } else { self.tail[j - in_chunks] });
        }
        self.run.filter(|r| i - stored < r.1).map(|r| r.0)
    }

    /// The oldest item.
    pub fn front(&self) -> Option<T> {
        self.get(0)
    }

    /// Adds `item` at the end.
    pub fn push_back(&mut self, item: T) {
        match &mut self.run {
            Some((same, n)) if *same == item => *n += 1,
            Some((same, n)) => {
                let (same, n) = (*same, *n);
                for _ in 0..n {
                    self.store(same);
                }
                self.run = Some((item, 1));
            }
            None => self.run = Some((item, 1)),
        }
    }

    /// Stores `item` after the stored ones, freezing the tail into a chunk when it fills.
    fn store(&mut self, item: T) {
        if self.tail.capacity() == 0 {
            self.tail.reserve_exact(CHUNK);
        }
        self.tail.push(item);
        if self.tail.len() == CHUNK {
            let full: Arc<[T]> = Arc::from(self.tail.as_slice());
            self.chunks.push_back(full);
            self.tail.clear();
        }
    }

    /// Drops the oldest item.
    pub fn pop_front(&mut self) {
        if self.stored() > 0 {
            self.skip += 1;
            if !self.chunks.is_empty() && self.skip == CHUNK {
                self.chunks.pop_front();
                self.skip = 0;
            } else if self.chunks.is_empty() && self.skip == self.tail.len() {
                // Nothing stored any more (a ball gone or at rest is all run): give the room back.
                self.tail = Vec::new();
                self.chunks = VecDeque::new();
                self.skip = 0;
            }
        } else if let Some((_, n)) = &mut self.run {
            *n -= 1;
            if *n == 0 {
                self.run = None;
            }
        }
    }

    /// The stored items, oldest first, from stored index `from` (chunks, then the tail).
    fn stored_from(&self, from: usize) -> impl DoubleEndedIterator<Item = T> + '_ {
        let j = from.min(self.stored()) + self.skip;
        let in_chunks = self.chunks.len();
        let (first, offset) = (j / CHUNK, j % CHUNK);
        let tail_from = j.saturating_sub(in_chunks * CHUNK);
        self.chunks
            .range(first.min(in_chunks)..)
            .enumerate()
            .map(move |(k, c)| if k == 0 { &c[offset..] } else { &c[..] })
            .chain(std::iter::once(&self.tail[tail_from..]))
            .flat_map(|s| s.iter().copied())
    }

    /// Every item, oldest first (and newest first with `.rev()`).
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = T> + '_ {
        self.iter_from(0)
    }

    /// The items from index `from` on, oldest first.
    pub fn iter_from(&self, from: usize) -> impl DoubleEndedIterator<Item = T> + '_ {
        let into_run = from.saturating_sub(self.stored());
        self.stored_from(from).chain(
            self.run.into_iter().flat_map(move |(item, n)| std::iter::repeat_n(item, n.saturating_sub(into_run))),
        )
    }

    /// The first index whose item `pred` says no to, when it says yes to a start and no to the rest
    /// (as `slice::partition_point`).
    pub fn partition_point(&self, pred: impl Fn(&T) -> bool) -> usize {
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.get(mid) {
                Some(item) if pred(&item) => lo = mid + 1,
                _ => hi = mid,
            }
        }
        lo
    }

    /// Bytes this copy takes: its own tail and its share of every chunk it shares with other
    /// copies (so the copies of a path together count it once).
    pub fn memory(&self) -> usize {
        let item = std::mem::size_of::<T>();
        self.tail.capacity() * item
            + self.chunks.capacity() * std::mem::size_of::<Arc<[T]>>()
            + self.chunks.iter().map(|c| CHUNK * item / Arc::strong_count(c).max(1)).sum::<usize>()
    }
}

impl<T: Copy + PartialEq> PartialEq for Track<T> {
    /// The same items, however they are stored.
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small xorshift, so the test needs nothing else.
    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    fn same<T: Copy + PartialEq + std::fmt::Debug>(t: &Track<T>, model: &VecDeque<T>) {
        assert_eq!(t.len(), model.len());
        assert_eq!(t.is_empty(), model.is_empty());
        assert_eq!(t.iter().collect::<Vec<_>>(), model.iter().copied().collect::<Vec<_>>());
        assert_eq!(t.iter().rev().collect::<Vec<_>>(), model.iter().rev().copied().collect::<Vec<_>>());
        for i in [0, 1, model.len() / 2, model.len().saturating_sub(1), model.len(), model.len() + 3] {
            assert_eq!(t.get(i), model.get(i).copied(), "at {i}");
            assert_eq!(
                t.iter_from(i).collect::<Vec<_>>(),
                model.range(i.min(model.len())..).copied().collect::<Vec<_>>(),
                "from {i}"
            );
        }
        assert_eq!(t.front(), model.front().copied());
    }

    /// Long random mixes of pushes (often repeating) and pops read exactly as a plain list, at a
    /// cap and without one.
    #[test]
    fn it_reads_as_a_plain_list() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for cap in [3usize, 63, 64, 65, 121, 300, usize::MAX] {
            let (mut t, mut model) = (Track::new(), VecDeque::new());
            let mut value = 0u32;
            for step in 0..4000u32 {
                let r = next(&mut seed);
                // Stretches of the same item, as a ball at rest or gone gives.
                if r.is_multiple_of(4) {
                    value = (r >> 8) as u32 % 1000;
                }
                if model.len() >= cap {
                    t.pop_front();
                    model.pop_front();
                }
                t.push_back(value);
                model.push_back(value);
                if r.is_multiple_of(97) {
                    let n = (r >> 20) as usize % (model.len() + 1);
                    for _ in 0..n {
                        t.pop_front();
                        model.pop_front();
                    }
                }
                if step.is_multiple_of(37) {
                    same(&t, &model);
                    for x in [0u32, 250, 500, 999, 1000] {
                        let ordered: Vec<u32> = (0..model.len() as u32).collect();
                        let mut s = Track::new();
                        ordered.iter().for_each(|v| s.push_back(*v));
                        assert_eq!(s.partition_point(|v| *v < x), ordered.partition_point(|v| *v < x));
                    }
                }
            }
            same(&t, &model);
        }
    }

    /// A copy shares the frozen chunks, and the two together count them once.
    #[test]
    fn copies_share_the_chunks() {
        let mut t = Track::new();
        for i in 0..1000u32 {
            t.push_back(i);
        }
        let alone = t.memory();
        let copy = t.clone();
        assert_eq!(copy, t);
        assert!(t.memory() + copy.memory() < alone + alone / 4, "{} + {} vs {alone}", t.memory(), copy.memory());
        // The same place over and over takes no more room.
        let before = t.memory();
        for _ in 0..10_000 {
            t.push_back(7);
        }
        assert_eq!(t.len(), 11_000);
        assert!(t.memory() <= before + CHUNK * 4);
        assert!(!t.is_one_run());
        let mut still = Track::new();
        for _ in 0..500 {
            still.push_back(1.5f32);
        }
        assert!(still.is_one_run());
        assert_eq!(still.len(), 500);
    }
}
