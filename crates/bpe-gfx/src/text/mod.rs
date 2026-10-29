//! Text drawn over the picture, as one quad per letter reading a signed-distance atlas.
//!
//! Letters are rasterised once per font with `fontdue`, turned into distance fields and packed
//! into one atlas ([`atlas`]). Laying a line out is a few microseconds of pen arithmetic, and
//! the distance field gives clean edges at any size plus an outline and glow for free.

pub mod atlas;

use std::sync::OnceLock;

use glam::Vec2;

pub use atlas::{Atlas, glyphs};
use atlas::{RASTER_PX, SPREAD};

use crate::frame::{GlyphInstance, blend_alpha};

/// The built-in fonts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontFace {
    /// A tall bold display face.
    #[default]
    Anton,
    /// A soft rounded face.
    VarelaRound,
    /// A bold typewriter face.
    SpaceMono,
}

impl FontFace {
    pub const ALL: [FontFace; 3] = [FontFace::Anton, FontFace::VarelaRound, FontFace::SpaceMono];

    fn bytes(self) -> &'static [u8] {
        match self {
            FontFace::Anton => include_bytes!("../../fonts/Anton-Regular.ttf"),
            FontFace::VarelaRound => include_bytes!("../../fonts/VarelaRound-Regular.ttf"),
            FontFace::SpaceMono => include_bytes!("../../fonts/SpaceMono-Bold.ttf"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Center,
    Left,
    Right,
}

/// How a run of text is coloured. Colours are linear light; a negative alpha is additive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub color: [f32; 4],
    /// Outline width in world units.
    pub outline: f32,
    pub outline_color: [f32; 4],
    /// Glow strength, 0 to 1.
    pub glow: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { color: [1.0; 4], outline: 0.0, outline_color: [0.0, 0.0, 0.0, 1.0], glow: 0.0 }
    }
}

/// A loaded font plus the size of its capital letters.
pub(crate) struct LoadedFont {
    pub font: fontdue::Font,
    /// Height of a capital H as a fraction of the em.
    cap_height: f32,
    /// Distance from one line's baseline to the next, as a fraction of the em.
    line_height: f32,
}

/// The loaded font for a face, read once.
pub(crate) fn font(face: FontFace) -> &'static LoadedFont {
    static FONTS: [OnceLock<LoadedFont>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    FONTS[face as usize].get_or_init(|| {
        let settings = fontdue::FontSettings { scale: RASTER_PX, ..Default::default() };
        let font = fontdue::Font::from_bytes(face.bytes(), settings).expect("the built-in fonts load");
        let cap = font.metrics('H', 1000.0);
        let cap_height = if cap.height > 0 { cap.height as f32 / 1000.0 } else { 0.7 };
        let line_height = font.horizontal_line_metrics(1000.0).map_or(1.2, |m| m.new_line_size / 1000.0).max(0.8);
        LoadedFont { font, cap_height, line_height }
    })
}

fn line_width(font: &fontdue::Font, indices: &[u16], em: f32) -> f32 {
    let mut width = 0.0;
    let mut previous = None;
    for &index in indices {
        if let Some(previous) = previous {
            width += font.horizontal_kern_indexed(previous, index, em).unwrap_or(0.0);
        }
        width += font.metrics_indexed(index, em).advance_width;
        previous = Some(index);
    }
    width
}

/// The lines of `text` as glyph indices, and the widest line's width at cap size `px`.
fn lines_of(text: &str, face: FontFace, px: f32) -> (Vec<Vec<u16>>, f32) {
    let f = font(face);
    let em = px.max(0.01) / f.cap_height;
    let lines: Vec<Vec<u16>> =
        text.split('\n').map(|line| line.chars().map(|c| f.font.lookup_glyph_index(c)).collect()).collect();
    let widest = lines.iter().map(|l| line_width(&f.font, l, em)).fold(0.0, f32::max);
    (lines, widest)
}

/// Size of `text` at cap size `px`, as (widest line, total height) in world units.
pub fn measure_text(text: &str, face: FontFace, px: f32) -> Vec2 {
    let f = font(face);
    let (lines, widest) = lines_of(text, face, px);
    let em = px.max(0.01) / f.cap_height;
    Vec2::new(widest, px + (lines.len().saturating_sub(1)) as f32 * em * f.line_height)
}

/// Lays `text` out around `anchor` (the centre of its block) at cap size `px`, shrinking it so
/// its widest line fits `max_width` when that is above 0. The first `visible` characters (line
/// breaks count) get a quad each in `out`; the rest only move the pen. Returns the block's size.
#[allow(clippy::too_many_arguments)]
pub fn layout_text(
    text: &str,
    face: FontFace,
    px: f32,
    anchor: Vec2,
    align: TextAlign,
    max_width: f32,
    visible: usize,
    style: &TextStyle,
    out: &mut Vec<GlyphInstance>,
) -> Vec2 {
    let f = font(face);
    let (lines, widest) = lines_of(text, face, px);
    let fit = if max_width > 0.0 && widest > max_width { (max_width / widest).max(0.02) } else { 1.0 };
    let px = px.max(0.01) * fit;
    let em = px / f.cap_height;
    // World units per atlas pixel, and per full step of the distance value.
    let unit = em / RASTER_PX;
    let distance_unit = 2.0 * SPREAD as f32 * unit;
    let line_height = em * f.line_height;
    let height = px + (lines.len().saturating_sub(1)) as f32 * line_height;
    let color = blend_alpha(style.color);
    let outline_color = blend_alpha(style.outline_color);
    let params = [style.outline.max(0.0) * fit, style.glow.clamp(0.0, 1.0), distance_unit, 0.0];
    let mut atlas = glyphs();
    let mut shown = 0;
    'lines: for (line_index, indices) in lines.iter().enumerate() {
        let baseline = anchor.y - height * 0.5 + px + line_index as f32 * line_height;
        let width = line_width(&f.font, indices, em);
        let mut pen = match align {
            TextAlign::Left => anchor.x - widest * fit * 0.5,
            TextAlign::Center => anchor.x - width * 0.5,
            TextAlign::Right => anchor.x + widest * fit * 0.5 - width,
        };
        let mut previous = None;
        for &index in indices {
            if let Some(previous) = previous {
                pen += f.font.horizontal_kern_indexed(previous, index, em).unwrap_or(0.0);
            }
            previous = Some(index);
            let advance = f.font.metrics_indexed(index, em).advance_width;
            shown += 1;
            if shown > visible {
                break 'lines;
            }
            if let Some(slot) = atlas.glyph(face, index)
                && !slot.is_empty()
            {
                let x = pen + slot.offset[0] * unit;
                let top = baseline - (slot.offset[1] + slot.size[1]) * unit;
                let rect = [x, top, x + slot.size[0] * unit, baseline - slot.offset[1] * unit];
                out.push(GlyphInstance { rect, uv: slot.uv, color, outline_color, params });
            }
            pen += advance;
        }
        // The line break itself counts as a character for typing in.
        shown += 1;
    }
    Vec2::new(widest * fit, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_makes_one_quad_per_visible_letter_and_measures_the_rest() {
        let mut out = Vec::new();
        let size = layout_text(
            "Hi there",
            FontFace::Anton,
            50.0,
            Vec2::ZERO,
            TextAlign::Center,
            0.0,
            2,
            &TextStyle::default(),
            &mut out,
        );
        assert_eq!(out.len(), 2, "H and i");
        assert_eq!(size, measure_text("Hi there", FontFace::Anton, 50.0));
        assert!(size.x > 100.0 && size.y == 50.0, "{size:?}");
        out.clear();
        layout_text(
            "Hi there",
            FontFace::Anton,
            50.0,
            Vec2::ZERO,
            TextAlign::Center,
            0.0,
            usize::MAX,
            &TextStyle::default(),
            &mut out,
        );
        assert_eq!(out.len(), 7, "the space has no quad");
        // The letters are centred on the anchor and sit inside the measured box.
        let left = out.iter().map(|g| g.rect[0]).fold(f32::MAX, f32::min);
        let right = out.iter().map(|g| g.rect[2]).fold(f32::MIN, f32::max);
        assert!((left + right).abs() < 15.0, "{left} {right}");
        assert!(out.iter().all(|g| g.rect[1] < g.rect[3] && g.uv[0] < g.uv[2]));
        assert_eq!(out[0].params[2], 2.0 * SPREAD as f32 * (50.0 / font(FontFace::Anton).cap_height) / RASTER_PX);
        // Two lines stack.
        let two = measure_text("a\nb", FontFace::Anton, 50.0);
        assert!(two.y > 90.0, "{two:?}");
    }

    #[test]
    fn long_text_shrinks_to_the_width() {
        let mut out = Vec::new();
        let full = measure_text("A rather long caption", FontFace::VarelaRound, 60.0);
        assert!(full.x > 300.0);
        let size = layout_text(
            "A rather long caption",
            FontFace::VarelaRound,
            60.0,
            Vec2::ZERO,
            TextAlign::Left,
            300.0,
            usize::MAX,
            &TextStyle::default(),
            &mut out,
        );
        assert!(size.x <= 300.0 + 1e-3 && size.x > 290.0, "{size:?}");
        assert!(size.y < 60.0, "it shrank: {size:?}");
        let left = out.iter().map(|g| g.rect[0]).fold(f32::MAX, f32::min);
        assert!(left >= -150.0 - 10.0, "left-aligned text starts at the block's left edge: {left}");
    }

    #[test]
    fn every_face_lays_out() {
        for face in FontFace::ALL {
            let mut out = Vec::new();
            let style = TextStyle { outline: 3.0, glow: 0.5, ..TextStyle::default() };
            layout_text("Ab", face, 40.0, Vec2::new(10.0, 20.0), TextAlign::Right, 0.0, usize::MAX, &style, &mut out);
            assert_eq!(out.len(), 2, "{face:?}");
            assert!(out.iter().all(|g| g.rect[3] - g.rect[1] > 20.0), "{face:?}");
            assert_eq!(out[0].params[0], 3.0);
            assert_eq!(out[0].params[1], 0.5);
        }
        assert!(!glyphs().is_empty());
    }
}
