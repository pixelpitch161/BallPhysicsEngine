//! Values only the sound settings use: modulation routes, note slots and drawn LFO shapes.

use crate::LayerId;
use serde::{Deserialize, Serialize};

/// The card key a sound card keeps its modulation routes under.
pub const ROUTES_KEY: &str = "routes";
/// The card key a sound card keeps its note slots under.
pub const SLOTS_KEY: &str = "notes_slots";

fn is_false(v: &bool) -> bool {
    !*v
}

/// One modulator driving one knob of the same card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModRoute {
    /// A modulator layer in this card.
    pub source: LayerId,
    /// A card key, layered or not: `"a_gain"`, `"3/cutoff"`.
    pub target: String,
    /// -1..=1, the fraction of the target's range (or of the way to silence on a scaling knob).
    pub amount: f32,
    /// Centred on the knob's value instead of starting from it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bipolar: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
}

impl ModRoute {
    /// Whether every number in it is usable.
    pub fn is_valid(&self) -> bool {
        self.amount.is_finite() && (-1.0..=1.0).contains(&self.amount) && !self.target.is_empty()
    }
}

/// One slot of notes: the MIDI numbers it plays (FL Studio naming, C5 = 60), lowest first and
/// each once, and the order they were clicked in (for the play orders that follow it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteSlot {
    pub notes: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_clicked: Vec<u8>,
}

impl NoteSlot {
    /// Whether the notes are sorted, unique and MIDI numbers (0..=127), and the click order
    /// names only notes of the slot.
    pub fn is_valid(&self) -> bool {
        self.notes.windows(2).all(|w| w[0] < w[1])
            && self.notes.iter().all(|&n| n <= 127)
            && self.order_clicked.iter().all(|n| self.notes.contains(n))
    }
}

/// The most points a drawn LFO shape has.
pub const MAX_CURVE_POINTS: usize = 128;

/// One point of a drawn LFO shape: `x` across one cycle (0..1, points in order, the first at 0
/// and the last at 1; two points at the same `x` make a jump), `y` its height (0..1) and `bend`
/// how the part from it to the next point bows (-1..1, 0 straight, above 0 bowed up on a rise).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bend: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl CurvePoint {
    pub const fn new(x: f32, y: f32, bend: f32) -> Self {
        CurvePoint { x, y, bend }
    }
}

/// Whether `points` is a usable shape: 2 to [`MAX_CURVE_POINTS`] points, finite, in order from
/// x 0 to x 1, heights 0..1 and bends -1..1.
pub fn curve_is_valid(points: &[CurvePoint]) -> bool {
    (2..=MAX_CURVE_POINTS).contains(&points.len())
        && points.first().is_some_and(|p| p.x == 0.0)
        && points.last().is_some_and(|p| p.x == 1.0)
        && points.windows(2).all(|w| w[0].x <= w[1].x)
        && points.iter().all(|p| {
            p.x.is_finite()
                && p.y.is_finite()
                && p.bend.is_finite()
                && (0.0..=1.0).contains(&p.y)
                && (-1.0..=1.0).contains(&p.bend)
        })
}
