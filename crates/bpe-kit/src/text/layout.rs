//! Laying words out: a Text's words, case, wrap, alignment, spacing, weight and slant in a font →
//! straight lines of [`Letter`]s, each with its outline as clean contours (y down, px). Shaping is
//! rustybuzz (kerning, ligatures, the weight axis), outlines skrifa, and each glyph's contours are
//! merged under the non-zero rule with i_overlay, because variable fonts overlap their contours.
//! Layouts are cached for the process.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use bpe_core::Choice;
use glam::Vec2;

use super::fonts::FontData;

/// Where each line sits across the block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Align {
    Left,
    #[default]
    Centre,
    Right,
}

/// How the words' letters are cased.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Case {
    #[default]
    AsTyped,
    Capitals,
    LowerCase,
    TitleCase,
}

/// What to lay out and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutSpec<'a> {
    pub words: &'a str,
    /// The height of a capital letter, px.
    pub size: f32,
    /// 100 … 1000, for fonts with a weight axis.
    pub weight: f32,
    /// Degrees; positive leans the tops right.
    pub slant: f32,
    /// Px added after every character.
    pub letter_spacing: f32,
    /// × the font's own line height.
    pub line_spacing: f32,
    pub align: Align,
    /// Px; 0 doesn't wrap.
    pub wrap: f32,
    pub case: Case,
}

/// One drawn glyph.
#[derive(Clone, Debug, PartialEq)]
pub struct Letter {
    pub ch: char,
    /// The font's glyph drawn (several characters can make one, a ligature).
    pub glyph: u16,
    /// Its place among the drawn letters, from 0.
    pub index: u32,
    /// Its line (wrapped lines count), from 0.
    pub line: u32,
    /// Its word, counted over the whole text, from 0.
    pub word: u32,
    /// Its paragraph (the line it was typed on, before wrapping), from 0.
    pub paragraph: u32,
    /// Its pen position on its baseline in the block.
    pub origin: Vec2,
    /// How far the pen moves past it (px, without the letter spacing).
    pub advance: f32,
    /// Its outline, relative to `origin`, px, y down: closed contours, holes included.
    pub contours: Vec<Vec<Vec2>>,
    /// Its outline's bounds in the block.
    pub min: Vec2,
    pub max: Vec2,
}

impl Letter {
    /// How many points its contours have.
    pub fn points(&self) -> usize {
        self.contours.iter().map(Vec::len).sum()
    }
}

/// Words laid out: their letters, the block's bounds (its middle at 0), the step from one line's
/// baseline to the next and how many lines there are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayout {
    pub letters: Vec<Letter>,
    pub min: Vec2,
    pub max: Vec2,
    pub line_step: f32,
    pub lines: u32,
}

/// The most letters one Text draws; past it, the rest are left out.
pub const MOST_LETTERS: usize = 4000;
/// The most points one letter's outline has.
pub const MOST_POINTS: usize = 2048;
/// How far a flattened curve may stray from the true one, px at the drawn size.
pub const TOLERANCE: f32 = 0.2;
/// The most characters read from the words (enough for [`MOST_LETTERS`] with spaces).
const MOST_CHARS: usize = 4 * MOST_LETTERS;

/// `spec` laid out in `font`, cached.
pub fn layout(font: &FontData, spec: &LayoutSpec) -> Arc<TextLayout> {
    type Key = (u64, [u32; 6], u8, u8, String);
    static CACHE: OnceLock<Mutex<HashMap<Key, Arc<TextLayout>>>> = OnceLock::new();
    let bits = [
        spec.size.to_bits(),
        spec.weight.to_bits(),
        spec.slant.to_bits(),
        spec.letter_spacing.to_bits(),
        spec.line_spacing.to_bits(),
        spec.wrap.to_bits(),
    ];
    let key = (font.id, bits, spec.align as u8, spec.case as u8, spec.words.to_string());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let made = Arc::new(lay_out(font, spec));
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= MOST_LAYOUTS {
        cache.clear();
    }
    cache.insert(key, made.clone());
    made
}

/// The most layouts kept at once: enough for a label on each of a crowd of balls, each its own
/// words, and their values changing.
const MOST_LAYOUTS: usize = 4096;

/// `words` in `case`.
pub fn cased(words: &str, case: Case) -> String {
    match case {
        Case::AsTyped => words.to_string(),
        Case::Capitals => words.to_uppercase(),
        Case::LowerCase => words.to_lowercase(),
        Case::TitleCase => {
            let mut out = String::with_capacity(words.len());
            let mut start = true;
            for c in words.chars() {
                if c.is_whitespace() {
                    start = true;
                    out.push(c);
                } else if start {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    out.extend(c.to_lowercase());
                }
            }
            out
        }
    }
}

/// One shaped glyph of a paragraph: its glyph, the character it came from, where the pen puts it
/// (px from the paragraph's start, with its offsets) and how far it moves the pen (letter spacing
/// included).
struct Shaped {
    glyph: u16,
    ch: char,
    x: f32,
    y: f32,
    advance: f32,
    pen: f32,
}

/// A word of a paragraph: its glyphs (a range of the paragraph's) and the spaces before it.
struct Word {
    from: usize,
    to: usize,
}

/// [`layout`] without the cache.
fn lay_out(font: &FontData, spec: &LayoutSpec) -> TextLayout {
    use skrifa::MetadataProvider;
    use skrifa::instance::Size;
    let empty = TextLayout::default();
    if !(spec.size.is_finite() && spec.size > 0.0) {
        return empty;
    }
    let Ok(sk) = skrifa::FontRef::new(&font.bytes) else { return empty };
    let Some(mut face) = rustybuzz::Face::from_slice(&font.bytes, 0) else { return empty };
    let weight = font.weight_axis.map(|(lo, hi)| spec.weight.clamp(lo, hi));
    let location = match weight {
        Some(w) => sk.axes().location([("wght", w)]),
        None => skrifa::instance::Location::default(),
    };
    if let Some(w) = weight {
        face.set_variations(&[rustybuzz::Variation { tag: rustybuzz::ttf_parser::Tag::from_bytes(b"wght"), value: w }]);
    }
    let metrics = sk.metrics(Size::unscaled(), &location);
    let em = metrics.units_per_em.max(1) as f32;
    let cap = metrics.cap_height.filter(|c| *c > 0.0).unwrap_or(0.7 * em);
    let scale = spec.size / cap;
    let height = (metrics.ascent - metrics.descent + metrics.leading).max(0.1 * em);
    let line_step = height * scale * spec.line_spacing.max(0.0);
    let spacing = if spec.letter_spacing.is_finite() { spec.letter_spacing } else { 0.0 };
    let shear = spec.slant.clamp(-89.0, 89.0).to_radians().tan();
    let words: String = cased(spec.words, spec.case).chars().take(MOST_CHARS).collect();

    // Shape each paragraph, then break it into lines between words.
    let mut lines: Vec<Vec<Shaped>> = Vec::new();
    let mut line_words: Vec<Vec<u32>> = Vec::new();
    let mut line_paragraphs: Vec<u32> = Vec::new();
    let mut word_count = 0u32;
    for (p, paragraph) in words.split('\n').enumerate() {
        let paragraph = paragraph.trim_end_matches('\r');
        let shaped = shape(&face, paragraph, scale, spacing);
        // Words: runs of glyphs that aren't spaces.
        let mut ws: Vec<Word> = Vec::new();
        let mut k = 0;
        while k < shaped.len() {
            if shaped[k].ch.is_whitespace() {
                k += 1;
                continue;
            }
            let from = k;
            while k < shaped.len() && !shaped[k].ch.is_whitespace() {
                k += 1;
            }
            ws.push(Word { from, to: k });
        }
        if ws.is_empty() {
            lines.push(Vec::new());
            line_words.push(Vec::new());
            line_paragraphs.push(p as u32);
            continue;
        }
        // Greedy wrapping on the shaped widths: a word that doesn't fit after the last starts a
        // line; a word wider than the wrap has a line of its own.
        let wrap = if spec.wrap.is_finite() && spec.wrap > 0.0 { spec.wrap } else { f32::INFINITY };
        let start_of = |w: &Word| shaped[w.from].pen;
        let end_of = |w: &Word| shaped[w.to - 1].pen + shaped[w.to - 1].advance - spacing;
        let mut first = 0;
        for i in 0..=ws.len() {
            let breaks = i == ws.len() || (i > first && end_of(&ws[i]) - start_of(&ws[first]) > wrap);
            if !breaks {
                continue;
            }
            let x0 = start_of(&ws[first]);
            let mut line = Vec::new();
            let mut wl = Vec::new();
            for w in &ws[first..i] {
                for g in &shaped[w.from..w.to] {
                    line.push(Shaped { x: g.x - x0, pen: g.pen - x0, ..*g });
                    wl.push(word_count);
                }
                word_count += 1;
            }
            lines.push(line);
            line_words.push(wl);
            line_paragraphs.push(p as u32);
            first = i;
        }
    }

    // The block: its widest line, and its first baseline so the middle sits at 0.
    let width_of = |line: &[Shaped]| line.last().map_or(0.0, |g| g.pen + g.advance - spacing);
    let widest = lines.iter().map(|l| width_of(l)).fold(0.0, f32::max);
    let count = lines.len() as u32;
    let first_baseline = (spec.size - (count.saturating_sub(1)) as f32 * line_step) * 0.5;

    let glyphs = sk.outline_glyphs();
    let mut outlines: HashMap<u16, Option<Arc<Vec<Vec<Vec2>>>>> = HashMap::new();
    let weight_bits = weight.map_or(0, f32::to_bits);
    let mut letters = Vec::new();
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    'lines: for (n, (line, wl)) in lines.iter().zip(&line_words).enumerate() {
        let w = width_of(line);
        let left = match spec.align {
            Align::Left => -widest * 0.5,
            Align::Centre => -w * 0.5,
            Align::Right => widest * 0.5 - w,
        };
        let baseline = first_baseline + n as f32 * line_step;
        for (g, &word) in line.iter().zip(wl) {
            let Some(contours) = outlines
                .entry(g.glyph)
                .or_insert_with(|| {
                    cached_outline((font.id, g.glyph, weight_bits, scale.to_bits(), shear.to_bits()), || {
                        outline(&glyphs, g.glyph, &location, scale, shear)
                    })
                })
                .clone()
            else {
                continue;
            };
            let contours = Vec::clone(&contours);
            let origin = Vec2::new(left + g.x, baseline + g.y);
            let (mut a, mut b) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for p in contours.iter().flatten() {
                a = a.min(*p + origin);
                b = b.max(*p + origin);
            }
            lo = lo.min(a);
            hi = hi.max(b);
            letters.push(Letter {
                ch: g.ch,
                glyph: g.glyph,
                index: letters.len() as u32,
                line: n as u32,
                word,
                paragraph: line_paragraphs[n],
                origin,
                advance: g.advance - spacing,
                contours,
                min: a,
                max: b,
            });
            if letters.len() >= MOST_LETTERS {
                break 'lines;
            }
        }
    }
    if letters.is_empty() {
        (lo, hi) = (Vec2::ZERO, Vec2::ZERO);
    }
    TextLayout { letters, min: lo, max: hi, line_step, lines: count }
}

/// A glyph's outline at one weight, scale and shear: (font id, glyph, weight bits, scale bits, shear bits).
type OutlineKey = (u64, u16, u32, u32, u32);

/// Glyph outlines already flattened and merged, kept for the process so words that change every frame
/// (a timer) only shape again. Cleared when it grows past [`MOST_OUTLINES`].
fn cached_outline(key: OutlineKey, make: impl FnOnce() -> Option<Vec<Vec<Vec2>>>) -> Option<Arc<Vec<Vec<Vec2>>>> {
    static CACHE: OnceLock<Mutex<HashMap<OutlineKey, Option<Arc<Vec<Vec<Vec2>>>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let made = make().map(Arc::new);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= MOST_OUTLINES {
        cache.clear();
    }
    cache.insert(key, made.clone());
    made
}

/// The most glyph outlines kept at once.
const MOST_OUTLINES: usize = 4096;

/// Glyph `glyph` of `font` alone, at `weight` and `slant`, scaled so a capital is `size` px high: its
/// contours from its pen position (y down), as [`layout`] draws it. `None` for a glyph that draws nothing.
pub fn glyph_outline(font: &FontData, glyph: u16, weight: f32, slant: f32, size: f32) -> Option<Vec<Vec<Vec2>>> {
    use skrifa::MetadataProvider;
    use skrifa::instance::Size;
    if !(size.is_finite() && size > 0.0) {
        return None;
    }
    let sk = skrifa::FontRef::new(&font.bytes).ok()?;
    let weight = font.weight_axis.map(|(lo, hi)| weight.clamp(lo, hi));
    let location = match weight {
        Some(w) => sk.axes().location([("wght", w)]),
        None => skrifa::instance::Location::default(),
    };
    let metrics = sk.metrics(Size::unscaled(), &location);
    let em = metrics.units_per_em.max(1) as f32;
    let cap = metrics.cap_height.filter(|c| *c > 0.0).unwrap_or(0.7 * em);
    let scale = size / cap;
    let shear = slant.clamp(-89.0, 89.0).to_radians().tan();
    let key = (font.id, glyph, weight.map_or(0, f32::to_bits), scale.to_bits(), shear.to_bits());
    let glyphs = sk.outline_glyphs();
    cached_outline(key, || outline(&glyphs, glyph, &location, scale, shear)).map(|c| Vec::clone(&c))
}

/// `text` shaped in `face`: each glyph's place and advance in px (a missing glyph takes no room).
fn shape(face: &rustybuzz::Face, text: &str, scale: f32, spacing: f32) -> Vec<Shaped> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    // Left to right always (right-to-left scripts aren't laid out, as the spec says).
    buffer.set_direction(rustybuzz::Direction::LeftToRight);
    let out = rustybuzz::shape(face, &[], buffer);
    let mut pen = 0.0;
    let mut shaped = Vec::with_capacity(out.len());
    for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
        let ch = text.get(info.cluster as usize..).and_then(|s| s.chars().next()).unwrap_or(' ');
        let glyph = info.glyph_id as u16;
        // A character the font lacks is left out and takes no room; a space keeps its own.
        let advance = if glyph == 0 && !ch.is_whitespace() { 0.0 } else { pos.x_advance as f32 * scale + spacing };
        shaped.push(Shaped {
            glyph,
            ch,
            x: pen + pos.x_offset as f32 * scale,
            y: -(pos.y_offset as f32) * scale,
            advance,
            pen,
        });
        pen += advance;
    }
    shaped
}

/// Glyph `gid`'s outline in px (y down, from its pen position), sheared by `shear`, its contours
/// merged under the non-zero rule; `None` for a glyph that draws nothing (a space, .notdef).
fn outline(
    glyphs: &skrifa::outline::OutlineGlyphCollection,
    gid: u16,
    location: &skrifa::instance::Location,
    scale: f32,
    shear: f32,
) -> Option<Vec<Vec<Vec2>>> {
    use skrifa::instance::Size;
    use skrifa::outline::DrawSettings;
    if gid == 0 {
        return None;
    }
    let glyph = glyphs.get(skrifa::GlyphId::new(gid as u32))?;
    let mut tolerance = TOLERANCE / scale.max(1e-6);
    for _ in 0..12 {
        let mut pen = Flatten { tolerance, ..Flatten::default() };
        glyph.draw(DrawSettings::unhinted(Size::unscaled(), location), &mut pen).ok()?;
        skrifa::outline::OutlinePen::close(&mut pen);
        let points: usize = pen.contours.iter().map(Vec::len).sum();
        if points > MOST_POINTS * 4 {
            tolerance *= 2.0;
            continue;
        }
        let place = |p: Vec2| {
            let q = Vec2::new(p.x, -p.y) * scale;
            Vec2::new(q.x - q.y * shear, q.y)
        };
        let contours: Vec<Vec<Vec2>> = pen
            .contours
            .iter()
            .map(|c| c.iter().map(|p| place(*p)).collect())
            .filter(|c: &Vec<Vec2>| c.len() >= 3)
            .collect();
        let merged = merge(&contours);
        if merged.is_empty() {
            return None;
        }
        if merged.iter().map(Vec::len).sum::<usize>() > MOST_POINTS {
            tolerance *= 2.0;
            continue;
        }
        return Some(merged);
    }
    None
}

/// `contours` as clean outlines under the non-zero rule: overlaps joined, holes kept, no edges
/// crossing. Contours of under 3 points are dropped.
fn merge(contours: &[Vec<Vec2>]) -> Vec<Vec<Vec2>> {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::float::simplify::SimplifyShape;
    if contours.is_empty() {
        return Vec::new();
    }
    let input: Vec<Vec<[f64; 2]>> =
        contours.iter().map(|c| c.iter().map(|p| [p.x as f64, p.y as f64]).collect()).collect();
    let shapes = input.simplify_shape(FillRule::NonZero);
    shapes
        .into_iter()
        .flatten()
        .map(|c| c.into_iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect::<Vec<_>>())
        .filter(|c| c.len() >= 3)
        .collect()
}

/// Flattens glyph outlines (font units, y up) into polygons, each curve cut into even steps few
/// enough to stay within `tolerance` of it.
#[derive(Default)]
struct Flatten {
    tolerance: f32,
    contours: Vec<Vec<Vec2>>,
    current: Vec<Vec2>,
}

impl Flatten {
    fn last(&self) -> Vec2 {
        self.current.last().copied().unwrap_or(Vec2::ZERO)
    }

    /// Steps that keep a curve whose second derivative is at most `bend` within the tolerance:
    /// a chord over a step h strays at most bend·h²/8.
    fn steps(&self, bend: f32) -> u32 {
        ((bend / (8.0 * self.tolerance.max(1e-9))).sqrt().ceil() as u32).clamp(1, 256)
    }
}

impl skrifa::outline::OutlinePen for Flatten {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.current.push(Vec2::new(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push(Vec2::new(x, y));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (a, c, b) = (self.last(), Vec2::new(cx0, cy0), Vec2::new(x, y));
        let n = self.steps(2.0 * (a - 2.0 * c + b).length());
        for k in 1..=n {
            let t = k as f32 / n as f32;
            self.current.push(a.lerp(c, t).lerp(c.lerp(b, t), t));
        }
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, c0, c1, b) = (self.last(), Vec2::new(cx0, cy0), Vec2::new(cx1, cy1), Vec2::new(x, y));
        let bend = 6.0 * (a - 2.0 * c0 + c1).length().max((c0 - 2.0 * c1 + b).length());
        let n = self.steps(bend);
        for k in 1..=n {
            let t = k as f32 / n as f32;
            let (p, q, r) = (a.lerp(c0, t), c0.lerp(c1, t), c1.lerp(b, t));
            self.current.push(p.lerp(q, t).lerp(q.lerp(r, t), t));
        }
    }
    fn close(&mut self) {
        // A contour's closing point repeats its first.
        if self.current.len() > 1 && self.current.first() == self.current.last() {
            self.current.pop();
        }
        if self.current.len() > 2 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::fonts::{find_in, repo_fonts_dir};
    use std::collections::BTreeMap;

    fn font(name: &str) -> Arc<FontData> {
        let found = find_in(Some(&repo_fonts_dir()), &BTreeMap::new(), name, 400.0);
        assert!(found.problem().is_none(), "{name}");
        found.font().clone()
    }

    fn font_at(name: &str, weight: f32) -> Arc<FontData> {
        find_in(Some(&repo_fonts_dir()), &BTreeMap::new(), name, weight).font().clone()
    }

    fn spec(words: &str, size: f32) -> LayoutSpec<'_> {
        LayoutSpec {
            words,
            size,
            weight: 400.0,
            slant: 0.0,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align: Align::Centre,
            wrap: 0.0,
            case: Case::AsTyped,
        }
    }

    fn lay(font: &FontData, s: LayoutSpec) -> Arc<TextLayout> {
        layout(font, &s)
    }

    #[test]
    fn size_is_the_capital_height() {
        let l = lay(&font("Anton"), spec("H", 100.0));
        assert_eq!(l.letters.len(), 1);
        let h = &l.letters[0];
        assert!(((h.max.y - h.min.y) - 100.0).abs() < 1.0, "{}", h.max.y - h.min.y);
        // The baseline is where the letter ends.
        assert!((h.max.y - h.origin.y).abs() < 1.0);
    }

    #[test]
    fn the_block_is_centred_on_the_middle() {
        let l = lay(&font("Anton"), spec("HI", 100.0));
        assert!(((l.min.x + l.max.x) * 0.5).abs() < 1.0, "{} {}", l.min, l.max);
        // Halfway between the cap top and the baseline of one line is 0.
        let base = l.letters[0].origin.y;
        assert!(((base - 100.0) + base).abs() < 1e-3, "{base}");
        let two = lay(&font("Anton"), spec("H\nH", 100.0));
        let (first, last) = (two.letters[0].origin.y, two.letters[1].origin.y);
        assert!(((first - 100.0) + last).abs() < 1e-3, "{first} {last}");
        assert!((last - first - two.line_step).abs() < 1e-3);
    }

    #[test]
    fn kerning_is_applied() {
        let inter = font("Inter");
        let av = lay(&inter, spec("AV", 100.0));
        let (a, v) = (lay(&inter, spec("A", 100.0)), lay(&inter, spec("V", 100.0)));
        let together = av.letters[1].origin.x - av.letters[0].origin.x;
        assert!(together < a.letters[0].advance - 1.0, "{together} vs {}", a.letters[0].advance);
        assert!(v.letters.len() == 1);
    }

    #[test]
    fn letters_count_skips_spaces_and_newlines() {
        let l = lay(&font("Anton"), spec("A B\nC", 100.0));
        let got: Vec<(char, u32, u32, u32)> = l.letters.iter().map(|x| (x.ch, x.index, x.line, x.word)).collect();
        assert_eq!(got, [('A', 0, 0, 0), ('B', 1, 0, 1), ('C', 2, 1, 2)]);
        assert_eq!(l.lines, 2);
    }

    #[test]
    fn letter_spacing_and_line_spacing_move_letters() {
        let anton = font("Anton");
        let plain = lay(&anton, spec("HH", 100.0));
        let spaced = lay(&anton, LayoutSpec { letter_spacing: 20.0, ..spec("HH", 100.0) });
        let gap = |l: &TextLayout| l.letters[1].origin.x - l.letters[0].origin.x;
        assert!((gap(&spaced) - gap(&plain) - 20.0).abs() < 1e-3);
        let double = lay(&anton, LayoutSpec { line_spacing: 2.0, ..spec("H\nH", 100.0) });
        let single = lay(&anton, spec("H\nH", 100.0));
        assert!((double.line_step - 2.0 * single.line_step).abs() < 1e-3);
    }

    #[test]
    fn align_left_and_right() {
        let anton = font("Anton");
        let at = |align| {
            let l = lay(&anton, LayoutSpec { align, ..spec("WIDE\nI", 100.0) });
            let i = l.letters.last().unwrap().clone();
            (i.min.x, i.max.x, l.min.x, l.max.x)
        };
        let (li, _, lmin, _) = at(Align::Left);
        let (ci, cx, _, _) = at(Align::Centre);
        let (_, rx, _, rmax) = at(Align::Right);
        assert!(li - lmin < 15.0, "left: {li} {lmin}");
        assert!(((ci + cx) * 0.5).abs() < 5.0, "centre: {ci} {cx}");
        assert!(rmax - rx < 15.0, "right: {rx} {rmax}");
    }

    #[test]
    fn wrap_breaks_between_words_and_keeps_long_words_whole() {
        let anton = font("Anton");
        let three = lay(&anton, spec("three", 100.0));
        let fits = three.max.x - three.min.x + 20.0;
        let l = lay(&anton, LayoutSpec { wrap: fits, ..spec("one two three", 100.0) });
        assert_eq!(l.lines, 3);
        let tiny = lay(&anton, LayoutSpec { wrap: 1.0, ..spec("one two three", 100.0) });
        assert_eq!((tiny.lines, tiny.letters.len()), (3, 11), "one word a line, nothing lost");
        assert_eq!(lay(&anton, spec("one two three", 100.0)).lines, 1);
    }

    #[test]
    fn case_changes_the_letters() {
        let anton = font("Anton");
        let caps = lay(&anton, LayoutSpec { case: Case::Capitals, ..spec("abc", 100.0) });
        assert_eq!(caps.letters.iter().map(|l| l.ch).collect::<String>(), "ABC");
        let title = lay(&anton, LayoutSpec { case: Case::TitleCase, ..spec("hello WORLD", 100.0) });
        assert_eq!(title.letters.iter().map(|l| l.ch).collect::<String>(), "HelloWorld");
        let lower = lay(&anton, LayoutSpec { case: Case::LowerCase, ..spec("ABC", 100.0) });
        assert_eq!(lower.letters.iter().map(|l| l.ch).collect::<String>(), "abc");
    }

    #[test]
    fn slant_leans_letters() {
        let anton = font("Anton");
        let l = lay(&anton, LayoutSpec { slant: 20.0, ..spec("I", 100.0) });
        let i = &l.letters[0];
        let pts: Vec<Vec2> = i.contours.iter().flatten().copied().collect();
        let top = pts.iter().filter(|p| p.y < -95.0).map(|p| p.x).fold(f32::MIN, f32::max);
        let bottom = pts.iter().filter(|p| p.y > -5.0).map(|p| p.x).fold(f32::MIN, f32::max);
        let want = 100.0 * 20f32.to_radians().tan();
        assert!((top - bottom - want).abs() < 4.0, "{} vs {want}", top - bottom);
    }

    #[test]
    fn weight_changes_a_variable_font() {
        let w = |weight| {
            let l = lay(&font_at("Inter", weight), LayoutSpec { weight, ..spec("I", 100.0) });
            l.letters[0].max.x - l.letters[0].min.x
        };
        assert!(w(900.0) > w(100.0) * 1.5, "{} {}", w(900.0), w(100.0));
    }

    #[test]
    fn holes_are_contours() {
        let anton = font("Anton");
        assert_eq!(lay(&anton, spec("O", 100.0)).letters[0].contours.len(), 2);
        assert_eq!(lay(&anton, spec("B", 100.0)).letters[0].contours.len(), 3);
    }

    fn crosses(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
        let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
        let (d1, d2) = (side(a, b, c), side(a, b, d));
        let (d3, d4) = (side(c, d, a), side(c, d, b));
        d1 * d2 < -1e-6 && d3 * d4 < -1e-6
    }

    #[test]
    fn overlaps_are_merged() {
        for (name, weight) in [("Inter", 700.0), ("Montserrat", 900.0), ("Nunito", 1000.0)] {
            let l = lay(&font_at(name, weight), LayoutSpec { weight, ..spec("B8&@", 200.0) });
            assert_eq!(l.letters.len(), 4, "{name}");
            for letter in &l.letters {
                let edges: Vec<(usize, Vec2, Vec2)> = letter
                    .contours
                    .iter()
                    .enumerate()
                    .flat_map(|(i, c)| (0..c.len()).map(move |k| (i, c[k], c[(k + 1) % c.len()])))
                    .collect();
                for (x, &(i, a, b)) in edges.iter().enumerate() {
                    for &(j, c, d) in &edges[x + 1..] {
                        if i != j {
                            assert!(!crosses(a, b, c, d), "{name} '{}': contours {i} and {j} cross", letter.ch);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn missing_glyphs_are_left_out() {
        let bebas = font("Bebas Neue");
        assert_eq!(lay(&bebas, spec("A😀B", 100.0)).letters.len(), 2);
        assert_eq!(lay(&bebas, spec("A\tB", 100.0)).letters.len(), 2);
        assert_eq!(lay(&bebas, spec("AЖB", 100.0)).letters.len(), 2, "Bebas Neue has no Cyrillic");
        // The space after a missing one still counts.
        let with = lay(&bebas, spec("A😀 B", 100.0));
        let without = lay(&bebas, spec("A B", 100.0));
        let gap = |l: &TextLayout| l.letters[1].origin.x - l.letters[0].origin.x;
        assert!((gap(&with) - gap(&without)).abs() < 1e-3);
    }

    #[test]
    fn nothing_to_draw() {
        let anton = font("Anton");
        for words in ["", "   ", "\n\n"] {
            assert!(lay(&anton, spec(words, 100.0)).letters.is_empty(), "{words:?}");
        }
        assert!(lay(&anton, spec("A", 0.0)).letters.is_empty());
        assert!(lay(&anton, spec("A", -5.0)).letters.is_empty());
        assert!(lay(&anton, spec("A", f32::NAN)).letters.is_empty());
        assert_eq!(lay(&anton, spec("\n\n", 100.0)).lines, 3);
    }

    #[test]
    fn a_huge_text_is_capped() {
        let words = "A".repeat(20_000);
        let start = std::time::Instant::now();
        let l = lay(&font("Anton"), spec(&words, 40.0));
        assert_eq!(l.letters.len(), MOST_LETTERS);
        assert!(start.elapsed().as_secs_f32() < 2.0, "{:?}", start.elapsed());
    }

    #[test]
    fn points_per_letter_are_capped_and_fine() {
        let anton = font("Anton");
        for size in [10.0, 100.0, 1000.0, 20000.0] {
            for letter in &lay(&anton, spec("O&@Sg", size)).letters {
                assert!(letter.points() <= MOST_POINTS, "{size}: {}", letter.points());
            }
        }
        // A curve flattens within the tolerance: every point of the true curve is near the line.
        use skrifa::outline::OutlinePen;
        let mut pen = Flatten { tolerance: TOLERANCE, ..Flatten::default() };
        pen.move_to(0.0, 0.0);
        pen.quad_to(500.0, 1000.0, 1000.0, 0.0);
        pen.curve_to(1200.0, -800.0, 300.0, -900.0, 0.0, 0.0);
        let line = pen.current.clone();
        let (a, c, b) = (Vec2::ZERO, Vec2::new(500.0, 1000.0), Vec2::new(1000.0, 0.0));
        let near = |p: Vec2| {
            (0..line.len() - 1)
                .map(|k| {
                    let (u, v) = (line[k], line[k + 1]);
                    let h = ((p - u).dot(v - u) / (v - u).length_squared().max(1e-12)).clamp(0.0, 1.0);
                    (p - u - (v - u) * h).length()
                })
                .fold(f32::MAX, f32::min)
        };
        for k in 0..=1000 {
            let t = k as f32 / 1000.0;
            let p = a.lerp(c, t).lerp(c.lerp(b, t), t);
            assert!(near(p) <= TOLERANCE + 1e-3, "{t}: {}", near(p));
        }
        assert!(line.len() < 200, "not far more points than it needs: {}", line.len());
    }

    #[test]
    fn letters_know_their_paragraph_and_glyph() {
        let f = font("Anton");
        let mut s = spec("AB CD\nEF", 40.0);
        s.wrap = 40.0;
        let l = lay(&f, s);
        let paragraphs: Vec<u32> = l.letters.iter().map(|l| l.paragraph).collect();
        let lines: Vec<u32> = l.letters.iter().map(|l| l.line).collect();
        assert_eq!(paragraphs, [0, 0, 0, 0, 1, 1]);
        assert_eq!(lines, [0, 0, 1, 1, 2, 2], "wrapped lines count, paragraphs don't");
        assert!(l.letters.iter().all(|l| l.glyph != 0));
        assert_eq!(l.letters[0].glyph, lay(&f, spec("A", 10.0)).letters[0].glyph);
    }

    #[test]
    fn a_glyph_alone_matches_its_letter() {
        let f = font("Anton");
        let l = lay(&f, spec("A", 50.0));
        let a = &l.letters[0];
        let alone = glyph_outline(&f, a.glyph, 400.0, 0.0, 50.0).unwrap();
        assert_eq!(alone, a.contours);
        assert!(glyph_outline(&f, 0, 400.0, 0.0, 50.0).is_none());
    }
}
