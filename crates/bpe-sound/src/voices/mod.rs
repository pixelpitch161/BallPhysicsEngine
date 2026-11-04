//! The shape voice and its parts: which hits play, which notes, where each note goes, and the
//! knobs' places.

pub mod alloc;
pub mod hit_filter;
pub mod knobs;
pub mod notes;
pub mod pick;
pub mod shape;
pub mod slots;

pub use hit_filter::{HitFilter, STRONG_HIT, strength};
pub use notes::{NotePick, NotePicker, PickCx};
pub use pick::{AnyPick, RandomPick};
pub use shape::{LEAVE_MS, ShapeSpec, ShapeVoice, note_hz};
pub use slots::{NotesSpec, SlotClock, clock_seed, slot_changes};
