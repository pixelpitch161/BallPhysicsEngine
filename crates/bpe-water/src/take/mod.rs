//! A final's take on disk (Water mode part 8): each kept moment's drops packed into planes and
//! compressed ([`frame`]), a water second's moments in a chunk file ([`chunk`]), and a body's state
//! every few seconds in a restart point ([`chunk::write_point`]). The take's folder, its head and
//! how a take is found are the kit's (`bpe-kit` `things/water/finals.rs`).

use std::hash::{DefaultHasher, Hash, Hasher};

pub mod chunk;
pub mod frame;

pub use chunk::{Chunk, ChunkInfo, ChunkWriter, read_point, write_point};
pub use frame::{FrameCodec, TakeFrame, byte_speed, speed_byte};

/// The take's format: what its files hold and how. A take of another format is not used.
pub const TAKE_FORMAT: u32 = 1;
/// Bumped by hand when the engine's host code changes what it makes (its shader is hashed).
pub const ENGINE_VERSION: u32 = 1;

/// The engine's print: its shader, the take's format and the engine's version.
pub fn engine_print() -> u64 {
    let mut h = DefaultHasher::new();
    crate::drops::engine_source().hash(&mut h);
    TAKE_FORMAT.hash(&mut h);
    ENGINE_VERSION.hash(&mut h);
    h.finish()
}
