//! A final's take on disk (Water mode part 8): each kept moment's drops packed into planes and
//! compressed ([`frame`]), a water second's moments in a chunk file ([`chunk`]) with the wet marks
//! by its end ([`chunk::write_wet`], part 6b), and a body's state every few seconds in a restart
//! point ([`chunk::write_point`]). The take's folder, its head and how a take is found are the
//! kit's (`bpe-kit` `things/water/finals.rs`).

use std::hash::{DefaultHasher, Hash, Hasher};

pub mod chunk;
pub mod frame;

pub use chunk::{Chunk, ChunkInfo, ChunkWriter, read_point, read_wet, write_point, write_wet};
pub use frame::{FrameCodec, TakeFrame, byte_speed, speed_byte};

/// The take's format: what its files hold and how. A take of another format is not used. (2: each
/// chunk has its wet marks, part 6b; 3: each moment and restart point keeps where the moving parts
/// are, part 5.)
pub const TAKE_FORMAT: u32 = 3;
/// Bumped by hand when the engine's host code changes what it makes (its sources are hashed too).
pub const ENGINE_VERSION: u32 = 1;
/// The most drops a moment or a restart point of a take may hold (a garbled count is refused).
pub const MOST_DROPS: usize = 1 << 24;

/// A print of every source file of this crate (`build.rs`), so a program whose engine changed in
/// any way never takes an older take for its own.
const SOURCES_PRINT: u64 = include!(concat!(env!("OUT_DIR"), "/sources_print.rs"));

/// The engine's print: its shader and its sources, the take's format and the engine's version.
pub fn engine_print() -> u64 {
    let mut h = DefaultHasher::new();
    crate::drops::engine_source().hash(&mut h);
    SOURCES_PRINT.hash(&mut h);
    TAKE_FORMAT.hash(&mut h);
    ENGINE_VERSION.hash(&mut h);
    h.finish()
}
