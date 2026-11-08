//! The places ("spots") of a word, picture or grid source (particles part 3): each particle born
//! at one keeps it as its home. They are worked out in the thing's own frame (middle at 0, x
//! right, y down), once for each change of what they depend on, and shared (`Arc`) by every copy
//! of the world; a process-wide cache keeps the latest, so rebuilding a playback doesn't spell a
//! long word again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::Vec2;

use super::births::signed;
use super::{BornFrom, GridBy, GridCovers, MOST_EVER, ParticlesSettings, Uses, Weight, WordFrom};
use crate::modules::look::layers::particles::emit::hash;
use crate::things::balls::shapes::{FONT, letter_outlines_in};

/// The most places a source has (the cap on particles): past it the list is cut.
pub const MOST_SPOTS: usize = MOST_EVER as usize;
/// The most pixels a picture may have.
const MOST_PIXELS: usize = 64 << 20;
/// How many lists of places the cache keeps.
const CACHED: usize = 16;

const REGULAR: &[u8] = include_bytes!("../../../../bpe-studio/assets/fonts/IBMPlexSans-Regular.ttf");
const MEDIUM: &[u8] = include_bytes!("../../../../bpe-studio/assets/fonts/IBMPlexSans-Medium.ttf");

/// Everything a source's places depend on (numbers as their bits).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SpotsKey {
    /// A source without places.
    None,
    Word {
        text: String,
        height: u32,
        weight: Weight,
        spacing: u32,
        /// A font other than the built-in one (W12), `None` for the built-in IBM Plex Sans.
        font: Option<WordFont>,
    },
    Picture {
        path: String,
        modified: Option<std::time::SystemTime>,
        width: u32,
        uses: Uses,
        spacing: u32,
    },
    /// A Text thing's letters (Text part 4): equal by what they read (`hash`).
    TextThing {
        hash: u64,
        spacing: u32,
        letters: Letters,
    },
    Grid {
        size: [u32; 2],
        by: GridBy,
        columns: u32,
        rows: u32,
        spacing: u32,
        jitter: u32,
        stagger: bool,
    },
}

/// A Text font a word is spelt in, carried in a [`SpotsKey`]: equal and hashed by its name and
/// its file's hash.
#[derive(Clone, Debug)]
pub struct WordFont {
    pub name: String,
    pub data: Arc<crate::text::fonts::FontData>,
}

impl PartialEq for WordFont {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.data.id == other.data.id
    }
}

impl Eq for WordFont {}

impl std::hash::Hash for WordFont {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (&self.name, self.data.id).hash(h);
    }
}

impl Weight {
    /// Its number, as fonts count weights.
    pub fn number(self) -> f32 {
        match self {
            Weight::Regular => 400.0,
            Weight::Medium => 500.0,
            Weight::SemiBold => 600.0,
        }
    }
}

/// The font `s` spells its text in, when it isn't the built-in one: looked up among the Text
/// fonts in `tree` (the scene's own first), or kept from `was` when it names the same font.
pub fn word_font(s: &ParticlesSettings, tree: Option<&bpe_core::Scene>, was: &SpotsKey) -> Option<WordFont> {
    if s.font == crate::text::fonts::FALLBACK_NAME {
        return None;
    }
    if let SpotsKey::Word { font: Some(f), weight, .. } = was
        && f.name == s.font
        && *weight == s.weight
    {
        return Some(f.clone());
    }
    let fonts = tree.map(|t| &t.fonts)?;
    let data = crate::text::fonts::find(fonts, &s.font, s.weight.number()).font().clone();
    Some(WordFont { name: s.font.clone(), data })
}

/// A Text thing's letters carried in a [`SpotsKey`]: never compared or hashed (the key's hash
/// says what they read).
#[derive(Clone, Debug, Default)]
pub struct Letters(pub Arc<Vec<Vec<Vec2>>>);

impl PartialEq for Letters {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Letters {}

impl std::hash::Hash for Letters {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

/// What `s`'s places depend on; `video` is the video's size (a grid covering the screen); `tree`
/// the scene, for a source made of a Text thing, `me` the Particles thing.
pub fn key_of(
    s: &ParticlesSettings,
    video: Vec2,
    tree: Option<&bpe_core::Scene>,
    me: bpe_core::ThingId,
    was: &SpotsKey,
) -> SpotsKey {
    let bits = |x: f32| if x.is_finite() { x.to_bits() } else { 0 };
    match s.from {
        BornFrom::AWordOrPicture => match s.word_from {
            WordFrom::Text => SpotsKey::Word {
                text: s.text.clone(),
                height: bits(s.letter_height),
                weight: s.weight,
                spacing: bits(s.spacing),
                font: word_font(s, tree, was),
            },
            WordFrom::ATextThing => {
                use crate::things::text::shape::{letters_hash, letters_of, text_named};
                let Some((tree, text)) = tree.and_then(|t| Some((t, text_named(t, &s.text_thing, me)?))) else {
                    return SpotsKey::None;
                };
                let hash = letters_hash(tree, text);
                // The letters are only laid out again when what they read changed.
                let letters = match was {
                    SpotsKey::TextThing { hash: h, letters, .. } if *h == hash => letters.clone(),
                    _ => Letters(Arc::new(letters_of(tree, text))),
                };
                SpotsKey::TextThing { hash, spacing: bits(s.spacing), letters }
            }
            WordFrom::Picture => SpotsKey::Picture {
                path: s.picture.clone(),
                modified: std::fs::metadata(&s.picture).and_then(|m| m.modified()).ok(),
                width: bits(s.picture_width),
                uses: s.uses,
                spacing: bits(s.spacing),
            },
        },
        BornFrom::AGrid => {
            let size = match s.grid_covers {
                GridCovers::TheScreen => (video + Vec2::splat(2.0 * s.margin)).max(Vec2::ZERO),
                GridCovers::ARectangle => Vec2::new(s.width, s.height).max(Vec2::ZERO),
            };
            SpotsKey::Grid {
                size: [bits(size.x), bits(size.y)],
                by: s.grid_by,
                columns: s.columns,
                rows: s.rows,
                spacing: bits(s.spacing),
                jitter: bits(s.jitter),
                stagger: s.stagger,
            }
        }
        _ => SpotsKey::None,
    }
}

/// The places `key` makes.
pub fn spots_for(key: &SpotsKey) -> Arc<Vec<Vec2>> {
    if *key == SpotsKey::None {
        return Arc::new(Vec::new());
    }
    static CACHE: OnceLock<Mutex<HashMap<SpotsKey, Arc<Vec<Vec2>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(key).cloned()) {
        return hit;
    }
    let f = f32::from_bits;
    let out = Arc::new(match key {
        SpotsKey::None => Vec::new(),
        SpotsKey::Word { text, height, spacing, font: Some(font), .. } => {
            fill_spots(&letter_outlines_in(&font.data.bytes, text, at_least(f(*height), 1.0)), f(*spacing), true)
        }
        SpotsKey::Word { text, height, weight, spacing, font: None } => {
            word_spots(text, f(*height), *weight, f(*spacing))
        }
        SpotsKey::TextThing { spacing, letters, .. } => fill_spots(&letters.0, f(*spacing), false),
        SpotsKey::Picture { path, width, uses, spacing, .. } => picture_spots(path, f(*width), *uses, f(*spacing)),
        SpotsKey::Grid { size, by, columns, rows, spacing, jitter, stagger } => {
            grid_spots(Vec2::new(f(size[0]), f(size[1])), *by, (*columns, *rows), f(*spacing), f(*jitter), *stagger)
        }
    });
    if let Ok(mut c) = cache.lock() {
        if c.len() >= CACHED {
            c.clear();
        }
        c.insert(key.clone(), out.clone());
    }
    out
}

/// A finite number at least `least`, else `least`.
fn at_least(x: f32, least: f32) -> f32 {
    if x.is_finite() { x.max(least) } else { least }
}

/// A grid's places across `size` (full width and height), its middle at 0.
pub fn grid_spots(
    size: Vec2,
    by: GridBy,
    (columns, rows): (u32, u32),
    spacing: f32,
    jitter: f32,
    stagger: bool,
) -> Vec<Vec2> {
    let size = Vec2::new(at_least(size.x, 0.0), at_least(size.y, 0.0));
    let (n, cell) = match by {
        GridBy::Spacing => {
            let step = at_least(spacing, 1.0);
            let n = |w: f32| ((w / step + 1e-4).floor() as u64).max(1);
            ((n(size.x), n(size.y)), Vec2::splat(step))
        }
        GridBy::ColumnsAndRows => {
            let (c, r) = (columns.max(1) as u64, rows.max(1) as u64);
            ((c, r), Vec2::new(size.x / c as f32, size.y / r as f32))
        }
    };
    let jitter = if jitter.is_finite() { jitter } else { 0.0 };
    let mut out = Vec::new();
    for j in 0..n.1 {
        for i in 0..n.0 {
            if out.len() >= MOST_SPOTS {
                return out;
            }
            let mut p =
                Vec2::new((i as f32 - (n.0 - 1) as f32 * 0.5) * cell.x, (j as f32 - (n.1 - 1) as f32 * 0.5) * cell.y);
            if stagger && j % 2 == 1 {
                p.x += cell.x * 0.5;
            }
            if jitter != 0.0 {
                let seed = hash(&[i as u32, j as u32, (i >> 32) as u32, (j >> 32) as u32, 0x917d]);
                p += Vec2::new(signed(seed, 1), signed(seed, 2)) * cell * 0.5 * jitter;
            }
            out.push(p);
        }
    }
    out
}

/// The places filling `text`'s letters on a square lattice `spacing` apart, capitals `height`
/// tall, centred on the letters (as the balls' Word shape fills them).
pub fn word_spots(text: &str, height: f32, weight: Weight, spacing: f32) -> Vec<Vec2> {
    let font = match weight {
        Weight::Regular => REGULAR,
        Weight::Medium => MEDIUM,
        Weight::SemiBold => FONT,
    };
    fill_spots(&letter_outlines_in(font, text, at_least(height, 1.0)), spacing, true)
}

/// Places on a lattice `spacing` apart inside `shapes` (closed outlines, non-zero inside), the
/// lattice centred on them; `centre` moves them so the outlines' box is round 0.
pub fn fill_spots(shapes: &[Vec<Vec2>], spacing: f32, centre: bool) -> Vec<Vec2> {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in shapes.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if shapes.is_empty() || lo.x > hi.x {
        return Vec::new();
    }
    let step = lattice_step(hi - lo, spacing);
    let cells = ((hi - lo) / step).floor();
    let middle = (lo + hi) * 0.5;
    let start = middle - cells * step * 0.5;
    let mut out = Vec::new();
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    for j in 0..=cells.y as i64 {
        let y = start.y + j as f32 * step;
        // Where each edge crosses the row, and which way (the non-zero winding of `winding`).
        crossings.clear();
        for shape in shapes {
            for (k, a) in shape.iter().enumerate() {
                let b = shape[(k + 1) % shape.len()];
                let up = a.y <= y && b.y > y;
                let down = b.y <= y && a.y > y;
                if up || down {
                    let x = a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x);
                    crossings.push((x, if up { 1 } else { -1 }));
                }
            }
        }
        if crossings.is_empty() {
            continue;
        }
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        // A point's winding is the sum over the crossings to its right.
        let mut wind: i32 = crossings.iter().map(|c| c.1).sum();
        let mut next = 0;
        for i in 0..=cells.x as i64 {
            let x = start.x + i as f32 * step;
            while next < crossings.len() && crossings[next].0 <= x {
                wind -= crossings[next].1;
                next += 1;
            }
            if wind != 0 {
                out.push(if centre { Vec2::new(x, y) - middle } else { Vec2::new(x, y) });
                if out.len() >= MOST_SPOTS {
                    return out;
                }
            }
        }
    }
    out
}

/// The most cells a word's or picture's lattice walks.
const MOST_CELLS: f64 = 4.0 * MOST_SPOTS as f64;

/// The lattice step over `size` at `spacing`, widened (never narrowed) so the lattice has at
/// most [`MOST_CELLS`] cells: a huge picture or word can't hang the particles.
fn lattice_step(size: Vec2, spacing: f32) -> f32 {
    let step = at_least(spacing, 1.0);
    let (w, h) = (size.x.clamp(0.0, 1e9) as f64, size.y.clamp(0.0, 1e9) as f64);
    let cells = |s: f64| (w / s + 1.0) * (h / s + 1.0);
    if cells(step as f64) <= MOST_CELLS {
        return step;
    }
    // (w/s + 1)(h/s + 1) = N where (N − 1)s² − (w + h)s − wh = 0.
    let (a, b) = (MOST_CELLS - 1.0, w + h);
    let s = (b + (b * b + 4.0 * a * w * h).sqrt()) / (2.0 * a);
    ((s * 1.001) as f32).max(step)
}

/// A PNG picture's pixels as RGBA, with its size; `None` when it can't be read.
fn read_png(path: &str) -> Option<(Vec<u8>, usize, usize)> {
    let file = std::fs::File::open(path).ok()?;
    let limits = png::Limits { bytes: MOST_PIXELS * 4 + (16 << 20) };
    let mut decoder = png::Decoder::new_with_limits(std::io::BufReader::new(file), limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let size = reader.output_buffer_size()?;
    if size > MOST_PIXELS * 4 {
        return None;
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return None,
    };
    if w == 0 || h == 0 || buf.len() < w * h * channels {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in buf[..w * h * channels].chunks_exact(channels) {
        rgba.extend_from_slice(&match channels {
            1 => [px[0], px[0], px[0], 255],
            2 => [px[0], px[0], px[0], px[1]],
            3 => [px[0], px[1], px[2], 255],
            _ => [px[0], px[1], px[2], px[3]],
        });
    }
    Some((rgba, w, h))
}

/// The places filling the parts `uses` picks of the PNG at `path`, drawn `width` pixels wide,
/// on a square lattice `spacing` apart, centred. None when the picture can't be read.
pub fn picture_spots(path: &str, width: f32, uses: Uses, spacing: f32) -> Vec<Vec2> {
    if path.trim().is_empty() {
        return Vec::new();
    }
    let Some((rgba, w, h)) = read_png(path) else { return Vec::new() };
    let width = at_least(width, 1.0);
    let scale = width / w as f32;
    let size = Vec2::new(width, h as f32 * scale);
    let step = lattice_step(size, spacing);
    let cells = (size / step).floor();
    let start = size * 0.5 - cells * step * 0.5;
    let mut out = Vec::new();
    for j in 0..=cells.y as i64 {
        for i in 0..=cells.x as i64 {
            let p = start + Vec2::new(i as f32, j as f32) * step;
            let (px, py) = ((p.x / scale) as usize, (p.y / scale) as usize);
            if px >= w || py >= h {
                continue;
            }
            let c = &rgba[(py * w + px) * 4..][..4];
            let alpha = c[3] as f32 / 255.0;
            let light = (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0;
            let filled = match uses {
                Uses::SolidParts => alpha >= 0.5,
                Uses::BrightParts => light * alpha >= 0.5,
                Uses::DarkParts => (1.0 - light) * alpha >= 0.5,
            };
            if filled {
                out.push(p - size * 0.5);
                if out.len() >= MOST_SPOTS {
                    return out;
                }
            }
        }
    }
    out
}
