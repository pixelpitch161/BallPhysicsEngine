//! Every play order: how each hit picks notes from the slot playing.
//!
//! An order reads the hit's step: how many accepted hits on this shape came before it since
//! the slot began, divided by Repeat each. Every order but Random walk is worked out from the
//! step alone, so it plays the same however playback got there; Random walk keeps its place,
//! and the voice counts its hits from the start of the clip even after a seek (see
//! [`super::shape::ShapeVoice::catch_up`]), so it does too.

use super::notes::{MAX_PICK, NotePick, NotePicker, PickCx, mix};
use bpe_kit::sound::{Plays, StrumDir};

/// The most notes a run (a slot's notes over its octave range) holds.
pub const MAX_RUN: usize = 128;

/// The notes an order runs over: `notes` (lowest first, or in click order), then each again an
/// octave up for every octave of `octave_range`.
pub struct Run {
    notes: [u8; MAX_RUN],
    len: usize,
}

impl Run {
    /// `notes` and their copies up to `octave_range` octaves higher, each pitch once, in pitch
    /// order.
    pub fn new(notes: &[u8], octave_range: u8) -> Run {
        Run::make(notes, octave_range, true)
    }

    /// As [`Run::new`], or (`sorted` false) in the order given, each pitch where it first comes.
    fn make(notes: &[u8], octave_range: u8, sorted: bool) -> Run {
        let mut run = Run { notes: [0; MAX_RUN], len: 0 };
        for k in 0..=octave_range.min(3) as u16 {
            for &n in notes {
                let n = n as u16 + 12 * k;
                if n > 127 || run.len >= MAX_RUN || run.notes[..run.len].contains(&(n as u8)) {
                    continue;
                }
                let at = if sorted { run.notes[..run.len].partition_point(|&m| m < n as u8) } else { run.len };
                run.notes.copy_within(at..run.len, at + 1);
                run.notes[at] = n as u8;
                run.len += 1;
            }
        }
        run
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> u8 {
        self.notes[i % self.len.max(1)]
    }
}

/// The run of the slot in `cx`, in pitch order (or click order for Order clicked).
fn run_of(cx: &PickCx, clicked: bool) -> Run {
    if !clicked {
        return Run::new(&cx.slot.notes, cx.octave_range);
    }
    // The click order, then any note it lacks, lowest first.
    let mut order = [0u8; MAX_RUN];
    let mut len = 0;
    for &n in cx.slot.order_clicked.iter().chain(cx.slot.notes.iter()) {
        if len < MAX_RUN && cx.slot.notes.contains(&n) && !order[..len].contains(&n) {
            order[len] = n;
            len += 1;
        }
    }
    Run::make(&order[..len], cx.octave_range, false)
}

/// The step of this hit: hits since the slot began, held for Repeat each.
fn step(cx: &PickCx) -> u64 {
    cx.hit_index / cx.repeat.max(1) as u64
}

/// The place in a run of `len` that the step `s` plays, for the orders worked out from the
/// step alone.
pub fn place(plays: Plays, s: u64, len: usize, seed: u64) -> usize {
    if len <= 1 {
        return 0;
    }
    let n = len as u64;
    match plays {
        Plays::Up => (s % n) as usize,
        Plays::Down => (n - 1 - s % n) as usize,
        Plays::UpDown | Plays::DownUp => {
            let period = 2 * (n - 1);
            let p = s % period;
            let up = if p < n { p } else { period - p } as usize;
            if plays == Plays::UpDown { up } else { len - 1 - up }
        }
        Plays::Converge | Plays::Diverge => {
            let k = (s % n) as usize;
            let k = if plays == Plays::Converge { k } else { len - 1 - k };
            // Outside in: lowest, highest, second lowest, second highest…
            if k % 2 == 0 { k / 2 } else { len - 1 - k / 2 }
        }
        Plays::Shuffle => {
            if len == 2 {
                // Two notes without a repeat can only take turns.
                return (s % 2) as usize;
            }
            let round = s / n;
            let bag = |r: u64| shuffled(len, mix(seed ^ mix(r.wrapping_add(0x5EED))));
            let mut perm = bag(round);
            // A round never starts on the note the last one ended on (swapping the first two
            // leaves this round's own last note where it is).
            if round > 0 && perm[0] == bag(round - 1)[len - 1] {
                perm.swap(0, 1);
            }
            perm[(s % n) as usize] as usize
        }
        _ => (mix(seed ^ mix(s.wrapping_add(1))) % n) as usize,
    }
}

/// 0..len in an order shuffled by `seed` (Fisher–Yates).
fn shuffled(len: usize, seed: u64) -> [u8; MAX_RUN] {
    let mut p = [0u8; MAX_RUN];
    let len = len.min(MAX_RUN);
    for (i, x) in p.iter_mut().enumerate().take(len) {
        *x = i as u8;
    }
    let mut r = seed;
    for i in (1..len).rev() {
        r = mix(r.wrapping_add(i as u64));
        let j = (r % (i as u64 + 1)) as usize;
        p.swap(i, j);
    }
    p
}

/// Any note at random, each step on its own (the same for the same step every time).
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomPick;

impl NotePicker for RandomPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let run = run_of(cx, false);
        if run.is_empty() {
            return NotePick::default();
        }
        NotePick::one(run.get(place(Plays::Random, step(cx), run.len(), cx.seed)))
    }
}

/// An order worked out from the step alone: Shuffle, Up, Down, Up-down, Down-up, Converge,
/// Diverge.
#[derive(Clone, Copy, Debug)]
pub struct StepPick(pub Plays);

impl NotePicker for StepPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let run = run_of(cx, false);
        if run.is_empty() {
            return NotePick::default();
        }
        NotePick::one(run.get(place(self.0, step(cx), run.len(), cx.seed)))
    }
}

/// One step up or down from the last note, at random, turning back at either end. It starts
/// each slot on the lowest note.
#[derive(Clone, Copy, Debug, Default)]
pub struct WalkPick {
    /// The step it is at and its place in the run (`None` before the first).
    at: Option<(u64, usize)>,
}

impl NotePicker for WalkPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let run = run_of(cx, false);
        if run.is_empty() {
            return NotePick::default();
        }
        let s = step(cx);
        let len = run.len();
        let pos = match self.at {
            Some((last, pos)) if last == s => pos.min(len - 1),
            Some((last, pos)) if s == last + 1 => {
                let up = mix(cx.seed ^ mix(s.wrapping_add(0xA11C))) & 1 == 1;
                let pos = pos.min(len - 1);
                // At either end it turns back, so it never stands still.
                let up = (up || pos == 0) && pos + 1 < len;
                if up {
                    pos + 1
                } else {
                    // One note only: it stays.
                    pos.saturating_sub(1)
                }
            }
            // A new slot (the step starts again) or a jump: from the lowest.
            _ => 0,
        };
        self.at = Some((s, pos));
        NotePick::one(run.get(pos))
    }

    fn reset(&mut self) {
        self.at = None;
    }
}

/// In the order the notes were clicked.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClickedPick;

impl NotePicker for ClickedPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let run = run_of(cx, true);
        if run.is_empty() {
            return NotePick::default();
        }
        NotePick::one(run.get((step(cx) % run.len() as u64) as usize))
    }
}

/// What picks a note by its place: the ball's number, the side hit, or the letter hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Ball,
    Side,
    Letter,
}

/// Ball n plays the run's nth note (By ball), side n the nth (By side, side 1 the lowest), or
/// letter n the nth (By letter).
#[derive(Clone, Copy, Debug)]
pub struct WhoPick {
    pub who: Who,
}

impl NotePicker for WhoPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let run = run_of(cx, false);
        if run.is_empty() {
            return NotePick::default();
        }
        let who = match self.who {
            Who::Ball => cx.ball_number,
            Who::Side => cx.side,
            Who::Letter => cx.letter,
        };
        NotePick::one(run.get(who as usize % run.len()))
    }
}

/// The pitch class of a note letter A to G (either case), C 0.
pub fn note_letter(ch: char) -> Option<u8> {
    Some(match ch.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    })
}

/// Spell notes: a hit letter A to G plays its own note name in the octave of the slot's lowest
/// note (C5, MIDI 60, when the slot is empty); any other letter, or a hit on no letter, plays
/// nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpellPick;

impl NotePicker for SpellPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let Some(class) = cx.letter_char.and_then(note_letter) else { return NotePick::default() };
        let lowest = cx.slot.notes.iter().copied().min().unwrap_or(60);
        let note = lowest - lowest % 12 + class;
        if note > 127 { NotePick::default() } else { NotePick::one(note) }
    }
}

/// Every note of the run together, or strummed: Strum spreads the starts over its length, in
/// the strum's direction.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChordPick;

impl NotePicker for ChordPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let run = run_of(cx, false);
        let n = run.len().min(MAX_PICK);
        let mut pick = NotePick::default();
        if n == 0 {
            return pick;
        }
        let random = if cx.strum_dir == StrumDir::Random { n } else { 0 };
        let order = shuffled(random, mix(cx.seed ^ mix(step(cx).wrapping_add(0x57A))));
        for (i, &shuffled_i) in order.iter().enumerate().take(n) {
            let j = match cx.strum_dir {
                StrumDir::Up => i,
                StrumDir::Down => n - 1 - i,
                StrumDir::Random => shuffled_i as usize,
            };
            pick.push(run.get(j));
        }
        pick.strum = if n > 1 { cx.strum_s.max(0.0) / (n - 1) as f32 } else { 0.0 };
        pick
    }
}

/// The picker for a play order. One value, no allocation, so a voice holds it inline.
#[derive(Clone, Copy, Debug)]
pub enum AnyPick {
    Random(RandomPick),
    Step(StepPick),
    Walk(WalkPick),
    Clicked(ClickedPick),
    Who(WhoPick),
    Spell(SpellPick),
    Chord(ChordPick),
}

impl AnyPick {
    pub fn new(plays: Plays) -> AnyPick {
        match plays {
            Plays::Random => AnyPick::Random(RandomPick),
            Plays::RandomWalk => AnyPick::Walk(WalkPick::default()),
            Plays::OrderClicked => AnyPick::Clicked(ClickedPick),
            Plays::ByBall => AnyPick::Who(WhoPick { who: Who::Ball }),
            Plays::BySide => AnyPick::Who(WhoPick { who: Who::Side }),
            Plays::ByLetter => AnyPick::Who(WhoPick { who: Who::Letter }),
            Plays::SpellNotes => AnyPick::Spell(SpellPick),
            Plays::AllAtOnce => AnyPick::Chord(ChordPick),
            p => AnyPick::Step(StepPick(p)),
        }
    }
}

impl NotePicker for AnyPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        match self {
            AnyPick::Random(p) => p.pick(cx),
            AnyPick::Step(p) => p.pick(cx),
            AnyPick::Walk(p) => p.pick(cx),
            AnyPick::Clicked(p) => p.pick(cx),
            AnyPick::Who(p) => p.pick(cx),
            AnyPick::Spell(p) => p.pick(cx),
            AnyPick::Chord(p) => p.pick(cx),
        }
    }

    fn reset(&mut self) {
        if let AnyPick::Walk(p) = self {
            p.reset();
        }
    }
}
