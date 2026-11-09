//! Distance tiles for the fast lane (Text part 2): each glyph's outline at a canonical size worked
//! out once on the CPU into a [`bpe_gfx::MASK_TILE`]² tile like a brush tile (G the signed distance
//! to its edge as [`bpe_gfx::tile_distance`] stores it, R how much of the texel it covers). A fast
//! letter's quad reads it in place of walking its outline (paths.wgsl, kind 3). Cached for the
//! process.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::Vec2;

use super::fonts::FontData;
use super::layout::glyph_outline;

/// The capital height tiles are made at, px.
pub const TILE_CAP: f32 = 100.0;
/// How much of the tile's half width the glyph's larger half extent takes: the rest is room for the
/// distance to reach past its edge.
pub const TILE_FILL: f32 = 0.75;

/// The least half width of a tile, px at [`TILE_CAP`]: small marks (a full stop, a colon) get as
/// much room for their layers to reach past them as a capital does.
pub const TILE_LEAST_HALF: f32 = 0.6 * TILE_CAP;

/// One glyph's tile and where it sits: its middle (from the glyph's pen position, y down) and half
/// its width, in px at [`TILE_CAP`].
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphTile {
    pub hash: u64,
    pub bytes: Arc<[u8]>,
    pub centre: Vec2,
    pub half: f32,
}

/// How far past a glyph's edge its tile's distance reaches, as a share of the tile's half width.
pub fn reach_share() -> f32 {
    bpe_gfx::TILE_DISTANCE_REACH
}

/// Glyph `glyph` of `font` at `weight` and `slant` as a tile; `None` for a glyph that draws nothing.
pub fn tile_for(font: &FontData, glyph: u16, weight: f32, slant: f32) -> Option<Arc<GlyphTile>> {
    type Key = (u64, u16, u32, u32);
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<Arc<GlyphTile>>>>> = OnceLock::new();
    // Rounded (a whole weight, half a degree), so a keyed Weight or Slant makes a few tiles, not
    // one a frame.
    let weight = font.weight_axis.map_or(0.0, |(lo, hi)| weight.clamp(lo, hi).round());
    let slant = (slant * 2.0).round() * 0.5;
    let key = (font.id, glyph, weight.to_bits(), slant.to_bits());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let made =
        glyph_outline(font, glyph, weight, slant, TILE_CAP).and_then(|c| tile_of(&c, key_hash(key))).map(Arc::new);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= 4096 {
        cache.clear();
    }
    cache.insert(key, made.clone());
    made
}

/// A hash naming a tile among a frame's tiles.
fn key_hash(key: (u64, u16, u32, u32)) -> u64 {
    // FNV-1a over the key, marked apart from brush strokes' hashes by a leading word.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    eat(b"glyph tile");
    eat(&key.0.to_le_bytes());
    eat(&key.1.to_le_bytes());
    eat(&key.2.to_le_bytes());
    eat(&key.3.to_le_bytes());
    h
}

/// `contours` (closed, px, even-odd) as a tile.
pub fn tile_of(contours: &[Vec<Vec2>], hash: u64) -> Option<GlyphTile> {
    let points = contours.iter().flatten();
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in points {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if !(lo.x <= hi.x && lo.y <= hi.y) {
        return None;
    }
    let centre = 0.5 * (lo + hi);
    let half = ((hi - lo).max_element() * 0.5 / TILE_FILL).max(TILE_LEAST_HALF);
    let n = bpe_gfx::MASK_TILE;
    let texel = 2.0 * half / n as f32;
    let reach = bpe_gfx::TILE_DISTANCE_REACH * half;
    let segments: Vec<(Vec2, Vec2)> = contours
        .iter()
        .filter(|c| c.len() >= 3)
        .flat_map(|c| (0..c.len()).map(move |k| (c[k], c[(k + 1) % c.len()])))
        .collect();
    let mut bytes = vec![0u8; bpe_gfx::MASK_TILE_BYTES];
    for y in 0..n {
        for x in 0..n {
            let t = Vec2::new((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            let p = centre + t * half;
            let mut best = f32::MAX;
            let mut inside = false;
            for &(a, b) in &segments {
                let e = b - a;
                let h = ((p - a).dot(e) / e.length_squared().max(1e-12)).clamp(0.0, 1.0);
                best = best.min((p - a - e * h).length_squared());
                if bpe_core::shape_path::crosses(a, b, p) {
                    inside = !inside;
                }
            }
            let d = best.sqrt().min(reach * 2.0);
            let signed = if inside { d } else { -d };
            let i = 2 * (y * n + x);
            bytes[i] = ((0.5 + signed / texel).clamp(0.0, 1.0) * 255.0).round() as u8;
            bytes[i + 1] = bpe_gfx::tile_distance(signed / half);
        }
    }
    Some(GlyphTile { hash, bytes: bytes.into(), centre, half })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_tile_reads_inside_and_out() {
        let square =
            vec![vec![Vec2::new(0.0, 0.0), Vec2::new(120.0, 0.0), Vec2::new(120.0, 120.0), Vec2::new(0.0, 120.0)]];
        let t = tile_of(&square, 1).unwrap();
        assert_eq!(t.centre, Vec2::splat(60.0));
        assert!((t.half - 80.0).abs() < 1e-4, "the square fills three quarters of the tile");
        // A small mark gets the least half width.
        let dot = vec![vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]];
        assert_eq!(tile_of(&dot, 2).unwrap().half, TILE_LEAST_HALF);
        let n = bpe_gfx::MASK_TILE;
        let at = |x: usize, y: usize| (t.bytes[2 * (y * n + x)], t.bytes[2 * (y * n + x) + 1]);
        let (cover, d) = at(n / 2, n / 2);
        assert_eq!(cover, 255);
        assert_eq!(d, 255, "deep inside reads the most inside");
        let (cover, d) = at(1, 1);
        assert_eq!((cover, d), (0, 0), "the tile's corner is far outside");
        // On the edge (x = 0 of the square sits at tile x = 1/8 of the way across).
        let edge = n / 8;
        let (_, d) = at(edge, n / 2);
        assert!((d as i32 - 128).abs() <= 8, "{d}");
    }

    #[test]
    fn a_glyph_tile_has_its_hole() {
        let found = crate::text::fonts::find_in(
            Some(&crate::text::fonts::repo_fonts_dir()),
            &Default::default(),
            "Anton",
            400.0,
        );
        let font = found.font();
        let l = crate::text::layout(
            font,
            &crate::text::LayoutSpec {
                words: "O",
                size: 50.0,
                weight: 400.0,
                slant: 0.0,
                letter_spacing: 0.0,
                line_spacing: 1.0,
                align: crate::text::Align::Centre,
                wrap: 0.0,
                case: crate::text::Case::AsTyped,
            },
        );
        let t = tile_for(font, l.letters[0].glyph, 400.0, 0.0).unwrap();
        let n = bpe_gfx::MASK_TILE;
        let middle = t.bytes[2 * ((n / 2) * n + n / 2)];
        assert_eq!(middle, 0, "the O's hole is empty");
        assert!(Arc::ptr_eq(&t, &tile_for(font, l.letters[0].glyph, 400.0, 0.0).unwrap()), "cached");
    }
}
