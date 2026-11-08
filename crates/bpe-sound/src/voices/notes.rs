//! Which notes a hit plays: the [`NotePicker`] of each play order (in [`super::pick`]) reads a
//! [`PickCx`] and gives a [`NotePick`].

use bpe_core::NoteSlot;
use bpe_kit::sound::StrumDir;

/// The most notes one hit can play.
pub const MAX_PICK: usize = 16;

/// What a picker reads for one hit.
pub struct PickCx<'a> {
    /// The slot playing (an empty one plays nothing).
    pub slot: &'a NoteSlot,
    /// Accepted hits on this shape before this one since the slot began.
    pub hit_index: u64,
    /// The hitting ball's number in its group (its place in the group's shape), 0 first.
    pub ball_number: u32,
    /// The side of the shape the hit was on, 0 first.
    pub side: u32,
    /// The letter of a Text the hit was on, 0 first (0 on anything else).
    pub letter: u32,
    /// That letter's character, for Spell notes (`None` on anything but a Text's letter).
    pub letter_char: Option<char>,
    /// A number made from the shape and the slot's turn, the same however playback got there.
    pub seed: u64,
    /// Repeat each: hits each note (or chord) plays for.
    pub repeat: u32,
    /// Octaves above the slot's notes the order runs on into, 0..=3.
    pub octave_range: u8,
    /// All at once: seconds from the first note of the strum to the last.
    pub strum_s: f32,
    pub strum_dir: StrumDir,
}

impl<'a> PickCx<'a> {
    /// A hit with nothing but its slot and step (tests and auditions).
    pub fn plain(slot: &'a NoteSlot, hit_index: u64) -> Self {
        PickCx {
            slot,
            hit_index,
            ball_number: 0,
            side: 0,
            letter: 0,
            letter_char: None,
            seed: 0,
            repeat: 1,
            octave_range: 0,
            strum_s: 0.0,
            strum_dir: StrumDir::Up,
        }
    }
}

/// The notes one hit plays, in the order they start.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NotePick {
    notes: [u8; MAX_PICK],
    len: u8,
    /// Seconds between the starts of one note and the next (0 for together).
    pub strum: f32,
}

impl NotePick {
    pub fn one(note: u8) -> Self {
        let mut p = NotePick::default();
        p.push(note);
        p
    }

    pub fn push(&mut self, note: u8) {
        if (self.len as usize) < MAX_PICK {
            self.notes[self.len as usize] = note;
            self.len += 1;
        }
    }

    pub fn notes(&self) -> &[u8] {
        &self.notes[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Chooses the notes for each hit.
pub trait NotePicker: Send {
    fn pick(&mut self, cx: &PickCx) -> NotePick;
    /// Forgets where it was (a seek; the voice then counts the hits again from the start).
    fn reset(&mut self) {}
}

/// A well-mixed 64-bit hash of `x`.
pub fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}
