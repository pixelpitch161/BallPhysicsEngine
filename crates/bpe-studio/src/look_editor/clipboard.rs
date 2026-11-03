//! What the Look editor copied: a layer and a whole look, kept in the editor (not the system
//! clipboard) while the selection changes, until something else is copied over them.

use bpe_core::{CopiedLayer, CopiedLook};

#[derive(Clone, Debug, Default)]
pub struct Clipboard {
    pub layer: Option<CopiedLayer>,
    pub look: Option<CopiedLook>,
}
