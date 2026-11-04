//! The sound settings' schemas: card and layer kinds, registered with the kit. No sound is
//! made here; `bpe-sound` reads these settings.

pub mod mixer;

pub use mixer::{MixerKind, MixerSettings};

use bpe_core::Registry;

/// Adds every sound card and layer kind to `reg`.
pub fn register(reg: &mut Registry) {
    reg.add_module(MixerKind);
}
