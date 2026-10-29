//! The glyph atlas: one texture of signed distance fields that every text thing draws from.
//! The text module fills it; the renderer uploads it again whenever its generation changes.

use std::sync::{Mutex, MutexGuard};

/// A square `R8Unorm` texture of glyph distance fields: 0.5 is the glyph's edge, higher is
/// inside.
pub struct Atlas {
    /// Width and height in texels.
    pub size: u32,
    /// `size * size` bytes, row by row from the top. Empty until something is rasterised.
    pub pixels: Vec<u8>,
    /// Goes up every time the pixels change.
    pub generation: u64,
}

impl Atlas {
    /// The pixels as the texture wants them: `size * size` bytes, zero where nothing was drawn.
    pub fn texels(&self) -> std::borrow::Cow<'_, [u8]> {
        let wanted = (self.size * self.size) as usize;
        if self.pixels.len() == wanted {
            std::borrow::Cow::Borrowed(&self.pixels)
        } else {
            let mut out = self.pixels.clone();
            out.resize(wanted, 0);
            std::borrow::Cow::Owned(out)
        }
    }
}

static GLYPHS: Mutex<Atlas> = Mutex::new(Atlas { size: 4, pixels: Vec::new(), generation: 0 });

/// The one atlas, for the text module to fill and the renderer to upload.
pub fn glyphs() -> MutexGuard<'static, Atlas> {
    GLYPHS.lock().unwrap_or_else(|e| e.into_inner())
}
