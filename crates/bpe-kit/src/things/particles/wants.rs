//! What a Particles thing's stepping reads from its Look (particles part 6): how many past places
//! its Trails layers need and which marks its Marks layers draw. A change of them rebuilds the
//! particle playback ([`bpe_runtime::ThingSim::beside_print_extra`]); a change of a colour doesn't.

use bpe_core::ResolvedSlot;

use super::look::{CARD, Drawn, layers_of};
use super::marks::MOST_MARKS_EVER;
use super::trail::MOST_TRAIL_POINTS;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wants {
    /// Places kept a particle: the most a shown Trails layer reads back (0: no ring).
    pub trail_points: usize,
    /// The marks the shown Marks layers draw (`marks::ON_*`, 0: none kept).
    pub marks_why: u8,
    /// The most marks a shown Marks layer keeps.
    pub most_marks: usize,
    /// How long a mark lasts in the layer it lasts longest in, when every one fades.
    pub marks_last: Option<f32>,
}

impl Wants {
    /// What the shown layers of the Look card `look` ask for.
    pub fn read(look: Option<&ResolvedSlot>) -> Wants {
        let mut w = Wants::default();
        let Some(look) = look.filter(|l| l.kind == CARD) else { return w };
        let mut lasts: Option<Option<f32>> = None;
        for layer in layers_of(look) {
            match &layer.drawn {
                Drawn::Trails(t) => w.trail_points = w.trail_points.max(t.reach()),
                Drawn::Marks(m) => {
                    let why = m.why();
                    if why == 0 {
                        continue;
                    }
                    w.marks_why |= why;
                    w.most_marks = w.most_marks.max(m.most.clamp(1, MOST_MARKS_EVER) as usize);
                    lasts = Some(match (lasts, m.lasts()) {
                        (None, l) => l,
                        (Some(Some(a)), Some(b)) => Some(a.max(b)),
                        _ => None,
                    });
                }
                _ => {}
            }
        }
        w.trail_points = w.trail_points.min(MOST_TRAIL_POINTS);
        w.marks_last = lasts.flatten();
        w
    }

    /// A hash of it, for the particle playback's fingerprint.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut put = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        };
        put(&(self.trail_points as u64).to_le_bytes());
        put(&[self.marks_why]);
        put(&(self.most_marks as u64).to_le_bytes());
        put(&self.marks_last.map_or(u32::MAX, f32::to_bits).to_le_bytes());
        h
    }
}
