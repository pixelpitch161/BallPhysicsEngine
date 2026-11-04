//! Which slot a shape plays: the slots take turns By tempo (on the scene's bar lines), By time,
//! By hits or only where a Slot keyframe says, in the Order chosen. A Slot keyframe jumps to
//! its slot whatever the mode.
//!
//! Tempo and time changes fall on lines worked out from the absolute sample, so every shape
//! (and every seek) agrees on where they are. The clock steps from one slot to the next as it
//! goes; the voice runs it from the start of the clip after a seek, so it plays the same however
//! playback got there.

use super::notes::mix;
use crate::{SAMPLE_RATE, TempoMap};
use bpe_core::{Track, Value};
use bpe_kit::sound::{BeatsOrBars, Change, MAX_SLOTS, Plays, SlotOrder, StrumDir, VoiceSettings};

/// A shape's notes settings that are not slots, as the engine reads them.
#[derive(Clone, Debug)]
pub struct NotesSpec {
    pub plays: Plays,
    pub repeat: u32,
    pub octave_range: u8,
    pub strum_ms: f32,
    pub strum_track: Option<Track>,
    pub strum_dir: StrumDir,
    pub change: Change,
    /// By tempo: beats per slot (bars already counted out).
    pub every_beats: u32,
    pub every_seconds: f32,
    pub every_seconds_track: Option<Track>,
    pub every_hits: u32,
    pub order: SlotOrder,
    /// The Slot keyframes.
    pub jump: Option<Track>,
}

impl Default for NotesSpec {
    fn default() -> Self {
        Self::from_settings(&VoiceSettings::default(), 4, [None, None, None])
    }
}

impl NotesSpec {
    /// The spec from the settings, `top` beats in a bar, and the keys of Strum, Every (seconds)
    /// and Jump to slot.
    pub fn from_settings(v: &VoiceSettings, top: u32, [strum, every_track, jump]: [Option<Track>; 3]) -> Self {
        let every = v.notes_every_beats.clamp(1, 64) as u32;
        NotesSpec {
            plays: v.notes_plays,
            repeat: v.notes_repeat.clamp(1, 16) as u32,
            octave_range: v.notes_octave_range.clamp(0, 3) as u8,
            strum_ms: v.notes_strum.clamp(0.0, 200.0),
            strum_track: strum,
            strum_dir: v.notes_strum_dir,
            change: v.notes_change,
            every_beats: if v.notes_every_unit == BeatsOrBars::Bars { every * top.max(1) } else { every },
            every_seconds: v.notes_every_seconds.clamp(0.1, 60.0),
            every_seconds_track: every_track,
            every_hits: v.notes_every_hits.clamp(1, 256) as u32,
            order: v.notes_order,
            jump,
        }
    }

    /// Strum at `time`, in seconds.
    pub fn strum_s(&self, time: f64) -> f32 {
        let ms = self.strum_track.as_ref().and_then(|t| super::knobs::track_value(t, time)).unwrap_or(self.strum_ms);
        ms.clamp(0.0, 200.0) / 1000.0
    }

    /// The length of one slot for By tempo or By time at `time`, in seconds (`None` for the
    /// other modes).
    pub fn period_s(&self, tempo: &TempoMap, time: f64) -> Option<f64> {
        match self.change {
            Change::ByTempo => Some(tempo.seconds_of_beats(self.every_beats as f64)),
            Change::ByTime => {
                let s = self
                    .every_seconds_track
                    .as_ref()
                    .and_then(|t| super::knobs::track_value(t, time))
                    .unwrap_or(self.every_seconds);
                Some(s.clamp(0.1, 60.0) as f64)
            }
            _ => None,
        }
    }
}

/// The sample the jump key `i` of `track` lands on, and the slot (0 first) it names.
fn jump_key(track: &Track, i: usize) -> (u64, usize) {
    let k = &track.keys[i];
    let slot = match k.value {
        Value::Int(v) => v,
        Value::Number(v) => v.round() as i64,
        _ => 1,
    };
    ((k.time.max(0.0) * SAMPLE_RATE as f64).round() as u64, (slot.clamp(1, MAX_SLOTS as i64) - 1) as usize)
}

/// The number of the line (of lines `period` seconds apart from the start) at or before sample
/// `s`: line k sits on sample round(k × period × rate).
fn line_at(s: u64, period: f64) -> u64 {
    let per = period * SAMPLE_RATE as f64;
    if per <= 0.0 {
        return 0;
    }
    let mut k = (s as f64 / per).floor() as u64;
    while k > 0 && (k as f64 * per).round() as u64 > s {
        k -= 1;
    }
    while ((k + 1) as f64 * per).round() as u64 <= s {
        k += 1;
    }
    k
}

/// A change of slot waiting in the block: where it lands, and a jump's slot (`None` for the
/// mode's next).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    offset: u32,
    jump: Option<usize>,
}

const MAX_PENDING: usize = 16;

/// The slot a shape plays, block by block.
#[derive(Clone, Debug)]
pub struct SlotClock {
    /// How many slots there are (at least one).
    n: usize,
    seed: u64,
    current: usize,
    /// Changes made so far (seeds Random and Shuffle; Plays orders start again at each).
    changes: u64,
    /// Accepted hits since the last change (By hits).
    hits: u64,
    /// Ping-pong is on its way down.
    down: bool,
    /// Shuffle's bag and how far through it.
    bag: [u8; MAX_SLOTS],
    bag_at: usize,
    /// The tempo or time line the last block ended on, and the period it was counted in.
    line: Option<(u64, f64)>,
    /// The next jump key not yet passed.
    next_key: usize,
    /// Where the block's samples begin, and its changes still to apply.
    start: u64,
    pending: [Pending; MAX_PENDING],
    n_pending: usize,
    changed: bool,
}

impl SlotClock {
    /// A clock over `n` slots, at slot 1; `seed` makes its Random and Shuffle orders its own.
    pub fn new(n: usize, seed: u64) -> SlotClock {
        let mut c = SlotClock {
            n: n.clamp(1, MAX_SLOTS),
            seed,
            current: 0,
            changes: 0,
            hits: 0,
            down: false,
            bag: [0; MAX_SLOTS],
            bag_at: MAX_SLOTS,
            line: None,
            next_key: 0,
            start: 0,
            pending: [Pending { offset: 0, jump: None }; MAX_PENDING],
            n_pending: 0,
            changed: false,
        };
        c.reset();
        c
    }

    /// Back to slot 1 at the start of the clip.
    pub fn reset(&mut self) {
        self.current = 0;
        self.changes = 0;
        self.hits = 0;
        self.down = false;
        self.bag_at = MAX_SLOTS;
        self.line = None;
        self.next_key = 0;
        self.n_pending = 0;
        self.changed = false;
    }

    /// Takes over another clock's place (a patch swap), with this one's slot count.
    pub fn take_over(&mut self, old: &SlotClock) {
        let n = self.n;
        let seed = self.seed;
        *self = old.clone();
        self.n = n;
        self.seed = seed;
        self.current = self.current.min(n - 1);
        self.bag_at = MAX_SLOTS;
        self.n_pending = 0;
    }

    /// The slot playing now, 0 first.
    pub fn current(&self) -> usize {
        self.current
    }

    /// Changes made since the start: a number that tells one turn of a slot from the next.
    pub fn turn(&self) -> u64 {
        self.changes
    }

    /// Whether the slot changed during the last block.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// The slot after this one in `order`.
    fn next(&mut self, order: SlotOrder) -> usize {
        let n = self.n;
        if n == 1 {
            return 0;
        }
        let c = self.current;
        match order {
            SlotOrder::Forward => (c + 1) % n,
            SlotOrder::Reverse => (c + n - 1) % n,
            SlotOrder::PingPong => {
                if self.down && c == 0 {
                    self.down = false;
                } else if !self.down && c + 1 >= n {
                    self.down = true;
                }
                if self.down { c - 1 } else { c + 1 }
            }
            SlotOrder::Random => (c + 1 + (mix(self.seed ^ mix(self.changes + 1)) % (n as u64 - 1)) as usize) % n,
            SlotOrder::Shuffle => {
                if self.bag_at >= n {
                    for (i, b) in self.bag.iter_mut().enumerate().take(n) {
                        *b = i as u8;
                    }
                    let mut r = mix(self.seed ^ mix(self.changes.wrapping_add(0xBA6)));
                    for i in (1..n).rev() {
                        r = mix(r.wrapping_add(i as u64));
                        self.bag.swap(i, (r % (i as u64 + 1)) as usize);
                    }
                    // A new round doesn't start on the slot just played.
                    if self.bag[0] as usize == c {
                        self.bag.swap(0, n - 1);
                    }
                    self.bag_at = 0;
                }
                let s = self.bag[self.bag_at] as usize;
                self.bag_at += 1;
                s
            }
        }
    }

    fn apply(&mut self, p: Pending, spec: &NotesSpec) {
        match p.jump {
            Some(slot) => {
                self.current = slot.min(self.n - 1);
                self.down = self.current + 1 >= self.n && self.n > 1;
                self.bag_at = MAX_SLOTS;
            }
            // One slot has nowhere to go: no change, so its play order runs on.
            None if self.n == 1 => {
                self.hits = 0;
                return;
            }
            None => self.current = self.next(spec.order),
        }
        self.changes += 1;
        self.hits = 0;
        self.changed = true;
    }

    /// Finds the block's tempo, time and jump changes (the block is `len` samples from sound
    /// sample `start`). Apply them with [`SlotClock::until`] as the block's hits come.
    pub fn begin_block(&mut self, start: u64, len: u32, time: f64, spec: &NotesSpec, tempo: &TempoMap) {
        self.changed = false;
        self.start = start;
        self.n_pending = 0;
        let end = start + len as u64;
        let mut n = 0;
        let push = |p: Pending, n: &mut usize, pending: &mut [Pending; MAX_PENDING]| {
            if *n < MAX_PENDING {
                pending[*n] = p;
                *n += 1;
            }
        };
        if let Some(period) = spec.period_s(tempo, time) {
            // The line the last block ended on (worked out afresh when Every moved, so a keyed
            // Every never fires a burst of changes).
            let before = match self.line {
                Some((l, p)) if p == period => l,
                _ if start == 0 => 0,
                _ => line_at(start - 1, period),
            };
            let line = line_at(end - 1, period);
            for k in before + 1..=line {
                let at = ((k as f64 * period * SAMPLE_RATE as f64).round() as u64).max(start);
                push(Pending { offset: (at - start) as u32, jump: None }, &mut n, &mut self.pending);
            }
            self.line = Some((line, period));
        } else {
            self.line = None;
        }
        if let Some(track) = &spec.jump {
            while self.next_key < track.keys.len() {
                let (at, slot) = jump_key(track, self.next_key);
                if at >= end {
                    break;
                }
                push(Pending { offset: at.saturating_sub(start) as u32, jump: Some(slot) }, &mut n, &mut self.pending);
                self.next_key += 1;
            }
        }
        // In sample order; a jump after a mode change on the same sample wins.
        self.pending[..n].sort_by_key(|p| (p.offset, p.jump.is_some()));
        self.n_pending = n;
    }

    /// Applies the changes that land at or before `offset` in the block. A change on a hit's
    /// own sample comes before the hit.
    pub fn until(&mut self, offset: u32, spec: &NotesSpec) {
        let mut i = 0;
        while i < self.n_pending && self.pending[i].offset <= offset {
            let p = self.pending[i];
            self.apply(p, spec);
            i += 1;
        }
        self.pending.copy_within(i..self.n_pending, 0);
        self.n_pending -= i;
    }

    /// Applies every change left in the block.
    pub fn end_block(&mut self, spec: &NotesSpec) {
        self.until(u32::MAX, spec);
    }

    /// An accepted hit was played: By hits moves on after every Every hits.
    pub fn hit(&mut self, spec: &NotesSpec) {
        if spec.change != Change::ByHits {
            return;
        }
        self.hits += 1;
        if self.hits >= spec.every_hits as u64 {
            self.apply(Pending { offset: 0, jump: None }, spec);
        }
    }

    /// The samples where the slot changes from the start of the clip to `end`, and the slot it
    /// changes to, for a mode that needs no hits (By hits gives none). For the timeline.
    pub fn changes_until(spec: &NotesSpec, n: usize, seed: u64, tempo: &TempoMap, end: u64) -> Vec<(u64, usize)> {
        let mut c = SlotClock::new(n, seed);
        let mut out = Vec::new();
        let block = crate::BLOCK as u64;
        let mut start = 0;
        while start < end {
            c.begin_block(start, block as u32, start as f64 / SAMPLE_RATE as f64, spec, tempo);
            while c.n_pending > 0 {
                let at = c.pending[0].offset;
                let before = c.changes;
                c.until(at, spec);
                if c.changes != before {
                    out.push((start + at as u64, c.current));
                }
            }
            start += block;
        }
        out
    }
}
