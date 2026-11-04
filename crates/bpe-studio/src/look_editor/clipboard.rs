//! What the Look editor copied: a layer and a whole look (and the Sound tab a whole sound), kept in the editor (not the system
//! clipboard) while the selection changes, until something else is copied over them.

use bpe_core::{CopiedLayer, CopiedLook};

#[derive(Clone, Debug, Default)]
pub struct Clipboard {
    pub layer: Option<CopiedLayer>,
    pub look: Option<CopiedLook>,
    /// A shape's whole sound card (Copy sound on the Sound tab).
    pub sound: Option<bpe_core::Slot>,
    /// A sound chain layer (Copy layer on a strip) and the shape it came from, pasted into
    /// either oscillator.
    pub chain_layer: Option<(bpe_core::ThingId, CopiedLayer)>,
}
