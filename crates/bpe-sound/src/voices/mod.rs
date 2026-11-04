//! The shape voice and its parts: which hits play, which notes, where each note goes, and the
//! knobs' places.

pub mod alloc;
pub mod hit_filter;
pub mod knobs;
pub mod notes;
pub mod shape;

pub use hit_filter::{HitFilter, STRONG_HIT, strength};
pub use notes::{NotePick, NotePicker, RandomPick};
pub use shape::{LEAVE_MS, ShapeSpec, ShapeVoice, note_hz};
