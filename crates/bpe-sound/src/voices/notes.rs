//! Which notes a hit plays. Part 2 picks one note at random from the first slot; part 5 adds the
//! other play orders behind [`NotePicker`].

use bpe_core::NoteSlot;

/// The most notes one hit can play.
pub const MAX_PICK: usize = 16;

/// What a picker reads for one hit.
pub struct PickCx<'a> {
    pub slots: &'a [NoteSlot],
    /// A number made from the hit itself (where and when it happened), the same however
    /// playback got there.
    pub seed: u64,
}

/// The notes one hit plays.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NotePick {
    notes: [u8; MAX_PICK],
    len: u8,
    /// Seconds between the notes of a strummed chord (0 for together).
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
}

/// One of the first slot's notes, at random (the same for the same hit every time).
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomPick;

/// A well-mixed 64-bit hash of `x`.
pub fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl NotePicker for RandomPick {
    fn pick(&mut self, cx: &PickCx) -> NotePick {
        let Some(slot) = cx.slots.first() else { return NotePick::default() };
        match slot.notes.len() {
            0 => NotePick::default(),
            1 => NotePick::one(slot.notes[0]),
            n => NotePick::one(slot.notes[(mix(cx.seed) % n as u64) as usize]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_pick_is_the_same_for_the_same_hit() {
        let slots = vec![NoteSlot { notes: vec![60, 64, 67], order_clicked: vec![] }];
        let pick = |seed| RandomPick.pick(&PickCx { slots: &slots, seed }).notes().to_vec();
        assert_eq!(pick(5), pick(5));
        let seen: std::collections::BTreeSet<u8> = (0..64).map(|s| pick(s)[0]).collect();
        assert_eq!(seen.len(), 3, "every note gets played");
        assert!(RandomPick.pick(&PickCx { slots: &[], seed: 1 }).is_empty());
        let empty = vec![NoteSlot::default()];
        assert!(RandomPick.pick(&PickCx { slots: &empty, seed: 1 }).is_empty());
    }
}
