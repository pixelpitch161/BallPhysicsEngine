//! The glyph atlas: one texture of signed distance fields that every text thing draws from.
//! Letters are rasterised once with `fontdue`, turned into distance fields and packed into rows;
//! the renderer uploads the whole atlas again whenever its generation changes.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use super::{FontFace, font};

/// Letters are rasterised at this many pixels per em before being turned into distance fields.
pub const RASTER_PX: f32 = 64.0;
/// The distance field reaches this many atlas pixels either side of each letter's edge.
pub const SPREAD: usize = 10;
/// The atlas is this many texels square.
pub const ATLAS: u32 = 2048;

/// Where a letter is in the atlas and how it sits on the pen, in atlas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// Texture coordinates: left, top, right, bottom.
    pub uv: [f32; 4],
    /// Width and height of the padded bitmap. Both 0 for a letter with nothing to draw.
    pub size: [f32; 2],
    /// Left edge from the pen, and bottom edge above the baseline, of the padded bitmap.
    pub offset: [f32; 2],
}

impl Slot {
    pub fn is_empty(&self) -> bool {
        self.size[0] <= 0.0 || self.size[1] <= 0.0
    }
}

/// A square `R8Unorm` texture of glyph distance fields: 0.5 is the glyph's edge, higher is
/// inside.
pub struct Atlas {
    /// Width and height in texels.
    pub size: u32,
    /// `size * size` bytes, row by row from the top (empty until the first letter is drawn).
    pub pixels: Vec<u8>,
    /// Goes up every time the pixels change.
    pub generation: u64,
    glyphs: BTreeMap<(FontFace, u16), Slot>,
    /// Left edge and top of the row being filled, and its height.
    cursor: [u32; 2],
    row_height: u32,
}

impl Atlas {
    pub(crate) const fn new(size: u32) -> Self {
        Self { size, pixels: Vec::new(), generation: 0, glyphs: BTreeMap::new(), cursor: [0, 0], row_height: 0 }
    }

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

    /// Forgets every letter and starts packing from the top left again.
    pub fn clear(&mut self) {
        self.glyphs.clear();
        self.pixels.clear();
        self.cursor = [0, 0];
        self.row_height = 0;
        self.generation += 1;
    }

    /// How many letters are packed.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// The letter's place in the atlas, rasterising and packing it the first time. A full atlas
    /// is emptied and the letter packed afresh, so quads laid out earlier in that frame point at
    /// the wrong letters for one frame. `None` only for a letter bigger than the atlas.
    pub fn glyph(&mut self, face: FontFace, index: u16) -> Option<Slot> {
        if let Some(slot) = self.glyphs.get(&(face, index)) {
            return Some(*slot);
        }
        let (metrics, coverage) = font(face).font.rasterize_indexed(index, RASTER_PX);
        let pad = SPREAD as u32;
        let offset = [metrics.xmin as f32 - pad as f32, metrics.ymin as f32 - pad as f32];
        if metrics.width == 0 || metrics.height == 0 {
            let slot = Slot { uv: [0.0; 4], size: [0.0; 2], offset };
            self.glyphs.insert((face, index), slot);
            return Some(slot);
        }
        let (width, height) = (metrics.width as u32 + 2 * pad, metrics.height as u32 + 2 * pad);
        if width > self.size || height > self.size {
            return None;
        }
        let at = match self.room(width, height) {
            Some(at) => at,
            None => {
                self.clear();
                self.room(width, height)?
            }
        };
        let field = distance_field(&coverage, metrics.width, metrics.height, SPREAD);
        self.blit(at, width, height, &field);
        let scale = 1.0 / self.size as f32;
        let slot = Slot {
            uv: [
                at[0] as f32 * scale,
                at[1] as f32 * scale,
                (at[0] + width) as f32 * scale,
                (at[1] + height) as f32 * scale,
            ],
            size: [width as f32, height as f32],
            offset,
        };
        self.glyphs.insert((face, index), slot);
        Some(slot)
    }

    /// Takes a `width` × `height` spot on the current row or the next, if there is one.
    fn room(&mut self, width: u32, height: u32) -> Option<[u32; 2]> {
        if self.cursor[0] + width > self.size {
            self.cursor = [0, self.cursor[1] + self.row_height];
            self.row_height = 0;
        }
        if self.cursor[1] + height > self.size {
            return None;
        }
        let at = self.cursor;
        self.cursor[0] += width;
        self.row_height = self.row_height.max(height);
        Some(at)
    }

    fn blit(&mut self, at: [u32; 2], width: u32, height: u32, field: &[u8]) {
        let wanted = (self.size * self.size) as usize;
        if self.pixels.len() != wanted {
            self.pixels.clear();
            self.pixels.resize(wanted, 0);
        }
        let stride = self.size as usize;
        for y in 0..height as usize {
            let row = (at[1] as usize + y) * stride + at[0] as usize;
            self.pixels[row..row + width as usize]
                .copy_from_slice(&field[y * width as usize..(y + 1) * width as usize]);
        }
        self.generation += 1;
    }
}

/// Turns a coverage bitmap into a signed distance field with `spread` pixels of padding on every
/// side: 128 on the edge, brighter inside, darker outside, reaching 0 and 255 `spread` pixels
/// away. Uses exact Euclidean distance transforms (Felzenszwalb and Huttenlocher) of the inside
/// and the outside, with the coverage giving sub-pixel edge positions.
pub fn distance_field(coverage: &[u8], width: usize, height: usize, spread: usize) -> Vec<u8> {
    let (w, h) = (width + 2 * spread, height + 2 * spread);
    let mut inside = vec![0.0f32; w * h];
    for y in 0..height {
        for x in 0..width {
            inside[(y + spread) * w + x + spread] = coverage[y * width + x] as f32 / 255.0;
        }
    }
    const FAR: f32 = 1e10;
    // Squared distance to the nearest pixel that is mostly inside, and mostly outside.
    let mut to_inside: Vec<f32> = inside.iter().map(|&c| if c > 0.5 { 0.0 } else { FAR }).collect();
    let mut to_outside: Vec<f32> = inside.iter().map(|&c| if c > 0.5 { FAR } else { 0.0 }).collect();
    transform_2d(&mut to_inside, w, h);
    transform_2d(&mut to_outside, w, h);
    let mut out = vec![0u8; w * h];
    for i in 0..w * h {
        // Shift by the pixel's own coverage so edges land between pixels smoothly.
        let distance = if inside[i] > 0.5 {
            to_outside[i].sqrt() - 0.5 + (inside[i] - 0.5)
        } else {
            -(to_inside[i].sqrt() - 0.5) - (0.5 - inside[i])
        };
        let value = 0.5 + distance / (2.0 * spread as f32);
        out[i] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out
}

fn transform_2d(grid: &mut [f32], w: usize, h: usize) {
    let n = w.max(h);
    let (mut f, mut d, mut v, mut z) = (vec![0.0; n], vec![0.0; n], vec![0usize; n], vec![0.0; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        transform_1d(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..(y + 1) * w]);
        transform_1d(&f[..w], &mut d[..w], &mut v, &mut z);
        grid[y * w..(y + 1) * w].copy_from_slice(&d[..w]);
    }
}

/// One-dimensional squared distance transform: the lower envelope of parabolas.
fn transform_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        loop {
            let p = v[k];
            let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * (q as f32 - p as f32));
            if s <= z[k] && k > 0 {
                k -= 1;
                continue;
            }
            if s <= z[k] {
                // k == 0: the new parabola replaces the first one.
                v[0] = q;
                z[1] = f32::INFINITY;
                break;
            }
            k += 1;
            v[k] = q;
            z[k] = s;
            z[k + 1] = f32::INFINITY;
            break;
        }
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f32 - p as f32;
        *out = dq * dq + f[p];
    }
}

static GLYPHS: Mutex<Atlas> = Mutex::new(Atlas::new(ATLAS));

/// The one atlas, for the text layout to fill and the renderer to upload.
pub fn glyphs() -> MutexGuard<'static, Atlas> {
    GLYPHS.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_field_is_bright_inside_and_dark_outside() {
        // A solid 4 × 4 block with 2 pixels of padding.
        let field = distance_field(&[255; 16], 4, 4, 2);
        assert_eq!(field.len(), 64);
        let at = |x: usize, y: usize| field[y * 8 + x];
        assert!(at(3, 3) > 200, "{}", at(3, 3));
        assert!(at(4, 4) > 200);
        assert!(at(1, 1) < 128, "{}", at(1, 1));
        assert_eq!(at(0, 0), 0);
        assert!(at(2, 3) > 128 && at(1, 3) < 128, "the edge sits between the block and its padding");
        // The field is symmetric.
        assert_eq!(at(1, 3), at(6, 3));
        assert_eq!(at(3, 1), at(3, 6));
    }

    #[test]
    fn a_full_atlas_starts_afresh_with_a_new_generation() {
        let mut atlas = Atlas::new(128);
        let h = font(FontFace::Anton).font.lookup_glyph_index('H');
        let first = atlas.glyph(FontFace::Anton, h).unwrap();
        assert!(!first.is_empty());
        assert_eq!(atlas.generation, 1);
        assert_eq!(atlas.pixels.len(), 128 * 128);
        assert_eq!(atlas.glyph(FontFace::Anton, h), Some(first), "a packed letter is found again");
        // Letters roughly 84 texels wide fill a 128-texel atlas quickly; packing more clears it.
        let before = atlas.generation;
        for c in "ABCDEFGHIJ".chars() {
            let index = font(FontFace::Anton).font.lookup_glyph_index(c);
            assert!(atlas.glyph(FontFace::Anton, index).is_some());
        }
        assert!(atlas.generation > before + 1);
        assert!(atlas.len() <= 4, "after a clear only the latest letters are packed: {}", atlas.len());
        let space = font(FontFace::Anton).font.lookup_glyph_index(' ');
        assert!(atlas.glyph(FontFace::Anton, space).unwrap().is_empty());
    }
}
