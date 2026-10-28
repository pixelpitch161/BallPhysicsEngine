//! Captions drawn over the picture: hooks, counters and timers.
//!
//! Letters are rasterised once per font with `fontdue` (pure Rust), turned into signed distance
//! fields and packed into one atlas texture. Each frame the visible captions are laid out on the
//! CPU into one quad per letter and drawn in a single call, so six captions cost a few
//! microseconds of layout and one small upload. The distance field gives clean edges at any size
//! plus an outline and glow for free.

use bpe_scene::{Align, Caption, CaptionState, Color, Font, Scene, TextValues, Vec2};
use bpe_sim::Counts;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Letters are rasterised at this many pixels per em before being turned into distance fields.
const RASTER_PX: f32 = 64.0;
/// The distance field reaches this many atlas pixels either side of each letter's edge.
/// Must match SPREAD in text.wgsl.
const SPREAD: usize = 10;
const ATLAS_SIZE: u32 = 2048;
const DISPLAY: &[u8] = include_bytes!("../fonts/Anton-Regular.ttf");
const ROUNDED: &[u8] = include_bytes!("../fonts/VarelaRound-Regular.ttf");
const MONO: &[u8] = include_bytes!("../fonts/SpaceMono-Bold.ttf");

/// One letter's quad, as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GlyphInstance {
    /// Left, top, right, bottom in world units.
    pub rect: [f32; 4],
    /// Atlas coordinates of the same corners.
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub outline_color: [f32; 4],
    /// Output pixels per atlas pixel, outline width and glow reach in output pixels, glow strength.
    pub params: [f32; 4],
}

/// Where a letter is in the atlas and how it sits on the baseline, in atlas pixels.
#[derive(Clone, Copy, Debug)]
struct AtlasGlyph {
    /// Top-left corner in the atlas, or `None` for letters with nothing to draw (spaces).
    at: Option<[u32; 2]>,
    width: u32,
    height: u32,
    /// Left edge from the pen, and bottom edge above the baseline, of the padded bitmap.
    left: f32,
    bottom: f32,
}

/// A loaded font plus the size of its capital letters.
struct LoadedFont {
    font: fontdue::Font,
    /// Height of a capital H as a fraction of the em.
    cap_height: f32,
    /// Distance from one line's baseline to the next, as a fraction of the em.
    line_height: f32,
}

impl LoadedFont {
    fn new(bytes: &[u8]) -> Result<Self, String> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings { scale: RASTER_PX, ..Default::default() })
            .map_err(|e| e.to_string())?;
        let cap = font.metrics('H', 1000.0);
        let cap_height = if cap.height > 0 { cap.height as f32 / 1000.0 } else { 0.7 };
        let line_height = font.horizontal_line_metrics(1000.0).map_or(1.2, |m| m.new_line_size / 1000.0).max(0.8);
        Ok(Self { font, cap_height, line_height })
    }
}

/// Fonts by which caption setting they are for. Built-ins load on first use; files are read
/// once per path.
#[derive(Default)]
pub struct FontBook {
    fonts: Vec<LoadedFont>,
    by_key: HashMap<Font, usize>,
    folder: Option<PathBuf>,
    errors: Vec<(String, String)>,
}

impl FontBook {
    /// Sets the folder relative font paths are looked up in (usually the scene file's folder).
    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        if self.folder != folder {
            self.folder = folder;
            self.by_key.retain(|key, _| !matches!(key, Font::File(_)));
        }
    }

    /// Font files that couldn't be read, as (path, reason). Captions using them fall back to
    /// the display font.
    pub fn errors(&self) -> &[(String, String)] {
        &self.errors
    }

    fn index(&mut self, key: &Font) -> usize {
        if let Some(&index) = self.by_key.get(key) {
            return index;
        }
        let loaded = match key {
            Font::Display => LoadedFont::new(DISPLAY),
            Font::Rounded => LoadedFont::new(ROUNDED),
            Font::Mono => LoadedFont::new(MONO),
            Font::File(path) => {
                let full = resolve(self.folder.as_deref(), path);
                std::fs::read(&full).map_err(|e| e.to_string()).and_then(|bytes| LoadedFont::new(&bytes))
            }
        };
        let index = match loaded {
            Ok(font) => {
                self.fonts.push(font);
                self.fonts.len() - 1
            }
            Err(error) => {
                if let Font::File(path) = key {
                    self.errors.retain(|(p, _)| p != path);
                    self.errors.push((path.clone(), error));
                }
                self.index(&Font::Display)
            }
        };
        self.by_key.insert(key.clone(), index);
        index
    }
}

fn resolve(folder: Option<&Path>, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    match folder {
        Some(folder) if path.is_relative() => folder.join(path),
        _ => path,
    }
}

/// Packs letters' distance fields into rows of the atlas and remembers where each one went.
struct Atlas {
    /// Width and height in pixels.
    size: u32,
    glyphs: HashMap<(usize, u16), AtlasGlyph>,
    /// Left edge, top and height of the row being filled.
    cursor: [u32; 2],
    row_height: u32,
    /// Bitmaps waiting to be copied to the GPU, as (corner, width, height, pixels).
    uploads: Vec<([u32; 2], u32, u32, Vec<u8>)>,
    /// The atlas filled up and was emptied, so every letter must be packed again.
    generation: u64,
}

impl Atlas {
    fn new(size: u32) -> Self {
        Self { size, glyphs: HashMap::new(), cursor: [0, 0], row_height: 0, uploads: Vec::new(), generation: 0 }
    }

    fn clear(&mut self) {
        self.glyphs.clear();
        self.cursor = [0, 0];
        self.row_height = 0;
        self.uploads.clear();
        self.generation += 1;
    }

    /// The letter's place in the atlas, rasterising and packing it the first time. Returns
    /// `None` if the atlas is full.
    fn glyph(&mut self, fonts: &FontBook, font: usize, index: u16) -> Option<AtlasGlyph> {
        if let Some(glyph) = self.glyphs.get(&(font, index)) {
            return Some(*glyph);
        }
        let (metrics, coverage) = fonts.fonts[font].font.rasterize_indexed(index, RASTER_PX);
        let pad = SPREAD as u32;
        let (width, height) = (metrics.width as u32 + 2 * pad, metrics.height as u32 + 2 * pad);
        let left = metrics.xmin as f32 - pad as f32;
        let bottom = metrics.ymin as f32 - pad as f32;
        if metrics.width == 0 || metrics.height == 0 {
            let glyph = AtlasGlyph { at: None, width: 0, height: 0, left, bottom };
            self.glyphs.insert((font, index), glyph);
            return Some(glyph);
        }
        if self.cursor[0] + width > self.size {
            self.cursor = [0, self.cursor[1] + self.row_height];
            self.row_height = 0;
        }
        if self.cursor[1] + height > self.size || width > self.size {
            return None;
        }
        let at = self.cursor;
        self.cursor[0] += width;
        self.row_height = self.row_height.max(height);
        let field = distance_field(&coverage, metrics.width, metrics.height, SPREAD);
        self.uploads.push((at, width, height, field));
        let glyph = AtlasGlyph { at: Some(at), width, height, left, bottom };
        self.glyphs.insert((font, index), glyph);
        Some(glyph)
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

/// Lays out captions into letter quads. Holds the fonts and the atlas, so it can be used on its
/// own (for tests and measuring) without a GPU.
pub struct TextLayout {
    pub fonts: FontBook,
    atlas: Atlas,
    /// Width of the video in world units. Captions too wide for it shrink to fit, keeping a small
    /// margin. 0 lets captions be any width.
    pub fit_width: f32,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self { fonts: FontBook::default(), atlas: Atlas::new(ATLAS_SIZE), fit_width: 0.0 }
    }
}

/// Letters to draw this frame: over the finished picture, and under the effects.
#[derive(Clone, Debug, Default)]
pub struct TextQuads {
    pub over: Vec<GlyphInstance>,
    pub under: Vec<GlyphInstance>,
}

impl TextLayout {
    /// Fills `quads` with every caption visible at `values.time`. `pixel` is world units per
    /// output pixel.
    pub fn layout(&mut self, captions: &[Caption], values: &TextValues, pixel: f32, quads: &mut TextQuads) {
        for attempt in 0..2 {
            quads.over.clear();
            quads.under.clear();
            let mut fitted = true;
            for caption in captions {
                let Some(state) = caption.state_at(values) else { continue };
                let out = if caption.under_effects { &mut quads.under } else { &mut quads.over };
                fitted &= self.caption(caption, &state, pixel, !caption.under_effects, out);
            }
            if fitted || attempt == 1 {
                // On the second go, letters that still don't fit are left out.
                return;
            }
            // The atlas is full of letters from earlier frames: start it afresh and lay the frame
            // out again, so every quad points at the new atlas.
            self.atlas.clear();
        }
    }

    /// Adds one caption's letters. Returns false if the atlas ran out of room.
    fn caption(
        &mut self,
        caption: &Caption,
        state: &CaptionState,
        pixel: f32,
        srgb: bool,
        out: &mut Vec<GlyphInstance>,
    ) -> bool {
        let font_index = self.fonts.index(&caption.font);
        let font = &self.fonts.fonts[font_index];
        let anchor = caption.position + state.offset;
        let fit = self.fit(caption, font, &state.text, anchor.x);
        let size = caption.size.max(1.0) * state.scale * fit;
        let em = size / font.cap_height;
        // World units per atlas pixel.
        let unit = em / RASTER_PX;
        let line_height = em * font.line_height;
        let encode = |c: Color, alpha: f32| {
            let mut rgba = if srgb {
                [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, c.a as f32 / 255.0]
            } else {
                c.to_linear()
            };
            rgba[3] *= alpha;
            rgba
        };
        let color = encode(caption.color, state.alpha);
        let outline_color = encode(caption.outline_color, state.alpha);
        let outline_px = caption.outline.max(0.0) * state.scale * fit / pixel;
        let glow_reach = (SPREAD as f32 * unit / pixel - outline_px).max(0.0);
        let params = [unit / pixel, outline_px, caption.glow.clamp(0.0, 1.0), glow_reach];

        let mut shown = 0;
        let mut fitted = true;
        for (line_index, line) in state.text.split('\n').enumerate() {
            let baseline = anchor.y + size * 0.5 + line_index as f32 * line_height;
            let chars: Vec<(char, u16)> = line.chars().map(|c| (c, font.font.lookup_glyph_index(c))).collect();
            let width = line_width(&font.font, &chars, em);
            let mut pen = match caption.align {
                Align::Left => anchor.x,
                Align::Centre => anchor.x - width * 0.5,
                Align::Right => anchor.x - width,
            };
            let mut previous = None;
            for (i, &(c, index)) in chars.iter().enumerate() {
                if let Some(previous) = previous {
                    pen += font.font.horizontal_kern_indexed(previous, index, em).unwrap_or(0.0);
                }
                previous = Some(index);
                let advance = font.font.metrics_indexed(index, em).advance_width;
                shown += 1;
                if shown > state.visible {
                    return fitted;
                }
                if c.is_whitespace() {
                    pen += advance;
                    continue;
                }
                let Some(glyph) = self.atlas.glyph(&self.fonts, font_index, index) else {
                    fitted = false;
                    pen += advance;
                    continue;
                };
                let font = &self.fonts.fonts[font_index];
                let Some(at) = glyph.at else {
                    pen += advance;
                    continue;
                };
                let mut x = pen + glyph.left * unit;
                if state.glitch > 0.0 {
                    let jump = hash(state.glitch_seed.wrapping_mul(31).wrapping_add(i as u32 * 7 + line_index as u32));
                    if jump > 0.6 {
                        x += (jump - 0.8) * size * 0.5 * state.glitch;
                    }
                }
                let top = baseline - (glyph.bottom + glyph.height as f32) * unit;
                let rect = [x, top, x + glyph.width as f32 * unit, baseline - glyph.bottom * unit];
                let scale = 1.0 / self.atlas.size as f32;
                let uv = [
                    at[0] as f32 * scale,
                    at[1] as f32 * scale,
                    (at[0] + glyph.width) as f32 * scale,
                    (at[1] + glyph.height) as f32 * scale,
                ];
                if state.glitch > 0.05 {
                    // Red and cyan ghosts either side, like a broken screen.
                    let split = size * 0.05 * state.glitch;
                    for (dx, tint) in [(-split, [1.0, 0.1, 0.3]), (split, [0.1, 0.9, 1.0])] {
                        let ghost = [tint[0], tint[1], tint[2], color[3] * 0.6];
                        out.push(GlyphInstance {
                            rect: [rect[0] + dx, rect[1], rect[2] + dx, rect[3]],
                            uv,
                            color: ghost,
                            outline_color: [0.0; 4],
                            params: [params[0], 0.0, 0.0, 0.0],
                        });
                    }
                }
                out.push(GlyphInstance { rect, uv, color, outline_color, params });
                pen += font.font.metrics_indexed(index, em).advance_width;
            }
            // The line break itself counts as a character for typing in.
            shown += 1;
        }
        fitted
    }

    /// How much to shrink a caption so its widest line fits across the video, from 1 (no
    /// shrinking) down. Lines grow from the anchor towards the side they are lined up to.
    fn fit(&self, caption: &Caption, font: &LoadedFont, text: &str, anchor_x: f32) -> f32 {
        if self.fit_width <= 0.0 {
            return 1.0;
        }
        let half = self.fit_width * 0.5 * 0.94;
        let room = match caption.align {
            Align::Left => half - anchor_x,
            Align::Right => anchor_x + half,
            Align::Centre => 2.0 * (half - anchor_x.abs()),
        };
        let em = caption.size.max(1.0) / font.cap_height;
        let widest = text
            .split('\n')
            .map(|line| {
                let chars: Vec<(char, u16)> = line.chars().map(|c| (c, font.font.lookup_glyph_index(c))).collect();
                line_width(&font.font, &chars, em)
            })
            .fold(0.0, f32::max);
        if widest <= room || widest <= 0.0 { 1.0 } else { (room.max(0.0) / widest).max(0.05) }
    }

    /// Size of the caption's text in world units at scale 1, as (widest line, total height).
    pub fn measure(&mut self, caption: &Caption, text: &str) -> Vec2 {
        let index = self.fonts.index(&caption.font);
        let font = &self.fonts.fonts[index];
        let em = caption.size.max(1.0) / font.cap_height;
        let mut widest: f32 = 0.0;
        let mut lines = 0;
        for line in text.split('\n') {
            let chars: Vec<(char, u16)> = line.chars().map(|c| (c, font.font.lookup_glyph_index(c))).collect();
            widest = widest.max(line_width(&font.font, &chars, em));
            lines += 1;
        }
        Vec2::new(widest, caption.size + (lines - 1) as f32 * em * font.line_height)
    }
}

fn line_width(font: &fontdue::Font, chars: &[(char, u16)], em: f32) -> f32 {
    let mut width = 0.0;
    let mut previous = None;
    for &(_, index) in chars {
        if let Some(previous) = previous {
            width += font.horizontal_kern_indexed(previous, index, em).unwrap_or(0.0);
        }
        width += font.metrics_indexed(index, em).advance_width;
        previous = Some(index);
    }
    width
}

fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The live values captions show, read from a playback at its current time.
pub fn text_values(scene: &Scene, playback: &bpe_sim::Playback) -> TextValues {
    let counts = playback.counts_at(playback.simulation().time());
    values_with(scene, playback, &counts)
}

/// [`text_values`] with the running totals already worked out.
fn values_with(scene: &Scene, playback: &bpe_sim::Playback, counts: &Counts) -> TextValues {
    let time = playback.simulation().time();
    let rings = match scene.container {
        bpe_scene::Container::Rings { count, .. } => count,
        _ => 0,
    };
    // Each hit lights the pulse by its strength, and it fades over about a fifth of a second.
    let full_speed = scene.sound.full_volume_speed.max(1.0);
    let mut pulse: f32 = 0.0;
    for hit in playback.recent_events(0.4).iter().filter(|e| e.kind.is_hit()) {
        let age = (time - hit.time).max(0.0) as f32;
        pulse = pulse.max((hit.speed / full_speed).min(1.0) * (-age * 10.0).exp());
    }
    TextValues {
        time: time as f32,
        duration: scene.video.duration,
        balls: counts.balls,
        hits: counts.hits,
        escapes: counts.escapes,
        rings,
        // Rule counters by number, for {counter:1} and so on.
        counters: counts.counters.iter().enumerate().map(|(i, v)| ((i + 1).to_string(), v.round() as i64)).collect(),
        seed: scene.sound.seed,
        hit_pulse: pulse,
        ..TextValues::default()
    }
}

/// The live values the captions show this frame. Part of [`crate::FrameData`].
#[derive(Clone, Debug, Default)]
pub struct TextFrame {
    pub values: TextValues,
    /// Words from rules' "Show text" blocks on screen now, in the big centre style.
    pub rule_captions: Vec<Caption>,
}

/// How far back to look for rules' text still on screen.
const RULE_CAPTION_MAX_SECONDS: f64 = 30.0;

impl TextFrame {
    pub fn fill(&mut self, scene: &Scene, playback: &bpe_sim::Playback, counts: &Counts) {
        self.rule_captions.clear();
        let time = playback.simulation().time();
        let rules = playback.simulation().rules();
        for event in playback.recent_events(RULE_CAPTION_MAX_SECONDS) {
            if let Some(bpe_sim::rules::RuleOutput::Caption { text, seconds }) = rules.output(event)
                && !text.trim().is_empty()
                && time < event.time + seconds.max(0.1) as f64
            {
                let mut caption = Caption::new(text, bpe_scene::CaptionStyle::BigCentre);
                caption.show_from = event.time as f32;
                caption.show_to = (event.time + seconds.max(0.1) as f64) as f32;
                caption.anim_seconds = caption.anim_seconds.min(seconds.max(0.1) * 0.3);
                // A newer one takes the place of an older one that is still showing.
                self.rule_captions.retain(|c| c.text != caption.text);
                self.rule_captions.push(caption);
            }
        }
        if !scene.captions.is_empty() || !self.rule_captions.is_empty() {
            self.values = values_with(scene, playback, counts);
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TextGlobals {
    view: [f32; 4],
}

/// Draws captions on the GPU. One renderer per output size, owned by [`crate::Renderer`].
pub struct Captions {
    pub layout: TextLayout,
    quads: TextQuads,
    /// Output and HDR picture formats, for making `gpu` when it's first needed.
    formats: (wgpu::TextureFormat, wgpu::TextureFormat),
    /// The atlas and pipelines, made on the first frame with a caption, so the many renderers
    /// of the explorer's previews cost nothing for scenes without words.
    gpu: Option<TextGpu>,
}

/// The graphics card side of caption drawing.
struct TextGpu {
    globals: wgpu::Buffer,
    atlas: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    over: wgpu::RenderPipeline,
    under: wgpu::RenderPipeline,
    over_buffer: crate::GrowableBuffer,
    under_buffer: crate::GrowableBuffer,
}

impl Captions {
    pub fn new(output_format: wgpu::TextureFormat, hdr_format: wgpu::TextureFormat) -> Self {
        Self {
            layout: TextLayout::default(),
            quads: TextQuads::default(),
            formats: (output_format, hdr_format),
            gpu: None,
        }
    }
}

impl TextGpu {
    fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat, hdr_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/text.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text globals"),
            size: std::mem::size_of::<TextGlobals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text atlas"),
            size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
        let make = |format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("captions"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_glyph"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_glyph"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            globals,
            atlas,
            bind_group,
            over: make(output_format),
            under: make(hdr_format),
            over_buffer: crate::GrowableBuffer::new("caption letters"),
            under_buffer: crate::GrowableBuffer::new("caption letters under effects"),
        }
    }

    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        buffer: &crate::GrowableBuffer,
        count: usize,
    ) {
        if count == 0 {
            return;
        }
        let mut pass = crate::begin_pass(encoder, target, "captions", wgpu::LoadOp::Load);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, buffer.slice());
        pass.draw(0..4, 0..count as u32);
    }
}

impl Captions {
    /// Lays out this frame's captions and uploads anything new. Call before [`Self::draw_under`]
    /// and [`Self::draw_over`].
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        text: &TextFrame,
        pixel: f32,
    ) {
        self.quads.over.clear();
        self.quads.under.clear();
        if scene.captions.iter().all(|c| !c.enabled) && text.rule_captions.is_empty() {
            return;
        }
        let (output_format, hdr_format) = self.formats;
        let gpu = self.gpu.get_or_insert_with(|| TextGpu::new(device, output_format, hdr_format));
        let video = &scene.video;
        let globals =
            TextGlobals { view: [2.0 / video.width.max(1) as f32, 2.0 / video.height.max(1) as f32, pixel, 0.0] };
        queue.write_buffer(&gpu.globals, 0, bytemuck::bytes_of(&globals));
        self.layout.fit_width = video.width as f32;
        if text.rule_captions.is_empty() {
            self.layout.layout(&scene.captions, &text.values, pixel, &mut self.quads);
        } else {
            let all: Vec<Caption> = scene.captions.iter().chain(&text.rule_captions).cloned().collect();
            self.layout.layout(&all, &text.values, pixel, &mut self.quads);
        }
        // After the atlas starts afresh, old letters left in the texture are harmless: nothing
        // points at them any more, and new ones are written over them.
        for (at, width, height, pixels) in self.layout.atlas.uploads.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: at[0], y: at[1], z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: Some(height) },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
        }
        gpu.over_buffer.upload(device, queue, bytemuck::cast_slice(&self.quads.over));
        gpu.under_buffer.upload(device, queue, bytemuck::cast_slice(&self.quads.under));
    }

    /// Draws captions that sit under the effects into the HDR picture.
    pub fn draw_under(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(gpu) = &self.gpu {
            gpu.draw(encoder, target, &gpu.under, &gpu.under_buffer, self.quads.under.len());
        }
    }

    /// Draws captions on top of the finished picture.
    pub fn draw_over(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(gpu) = &self.gpu {
            gpu.draw(encoder, target, &gpu.over, &gpu.over_buffer, self.quads.over.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Anim, CaptionStyle};

    fn values(time: f32) -> TextValues {
        TextValues { time, duration: 20.0, balls: 7, ..TextValues::default() }
    }

    #[test]
    fn distance_field_is_bright_inside_and_dark_outside() {
        // A 6x6 filled square.
        let coverage = vec![255u8; 36];
        let field = distance_field(&coverage, 6, 6, 4);
        let w = 14;
        assert_eq!(field.len(), w * w);
        let at = |x: usize, y: usize| field[y * w + x];
        assert!(at(7, 7) > 160, "centre {}", at(7, 7));
        assert!(at(0, 0) < 30, "corner {}", at(0, 0));
        // Edge pixels sit near the middle value.
        assert!((at(4, 7) as i32 - 128).abs() < 40, "edge {}", at(4, 7));
        // Values rise steadily from outside to inside.
        let row: Vec<u8> = (0..8).map(|x| at(x, 7)).collect();
        assert!(row.windows(2).all(|p| p[0] <= p[1]), "{row:?}");
    }

    #[test]
    fn distance_field_of_nothing_is_all_outside() {
        let field = distance_field(&[0u8; 4], 2, 2, 3);
        assert!(field.iter().all(|&v| v < 128));
        let empty = distance_field(&[], 0, 0, 2);
        assert_eq!(empty.len(), 16);
    }

    #[test]
    fn captions_lay_out_one_quad_per_letter() {
        let mut layout = TextLayout::default();
        let mut quads = TextQuads::default();
        let caption = Caption { anim_in: Anim::None, ..Caption::new("{balls} balls", CaptionStyle::Counter) };
        layout.layout(std::slice::from_ref(&caption), &values(1.0), 1.0, &mut quads);
        // "7 balls" has six letters and one space.
        assert_eq!(quads.over.len(), 6);
        assert!(quads.under.is_empty());
        // Centred on the caption's position.
        let left = quads.over.iter().map(|q| q.rect[0]).fold(f32::MAX, f32::min);
        let right = quads.over.iter().map(|q| q.rect[2]).fold(f32::MIN, f32::max);
        assert!(((left + right) * 0.5 - caption.position.x).abs() < caption.size * 0.3);
        // Letters run left to right.
        assert!(quads.over.windows(2).all(|q| q[0].rect[0] < q[1].rect[0]));
        let under = Caption { under_effects: true, ..caption };
        layout.layout(&[under], &values(1.0), 1.0, &mut quads);
        assert_eq!((quads.over.len(), quads.under.len()), (0, 6));
    }

    #[test]
    fn typewriter_and_hidden_captions_draw_fewer_letters() {
        let mut layout = TextLayout::default();
        let mut quads = TextQuads::default();
        let typing =
            Caption { anim_in: Anim::Typewriter, anim_seconds: 1.0, ..Caption::new("abcd", CaptionStyle::Subtitle) };
        layout.layout(std::slice::from_ref(&typing), &values(0.5), 1.0, &mut quads);
        assert_eq!(quads.over.len(), 2);
        let hidden = Caption { show_from: 5.0, ..typing };
        layout.layout(&[hidden], &values(0.5), 1.0, &mut quads);
        assert!(quads.over.is_empty());
    }

    #[test]
    fn every_font_and_style_lays_out() {
        let mut layout = TextLayout::default();
        let mut quads = TextQuads::default();
        for font in [Font::Display, Font::Rounded, Font::Mono, Font::File("missing.ttf".into())] {
            for &(text, style) in bpe_scene::TEMPLATES {
                let caption = Caption { font: font.clone(), anim_in: Anim::None, ..Caption::new(text, style) };
                layout.layout(&[caption], &values(3.0), 0.5, &mut quads);
                assert!(!quads.over.is_empty(), "{text} in {font:?}");
                for quad in &quads.over {
                    assert!(quad.rect.iter().chain(&quad.uv).all(|v| v.is_finite()));
                    assert!(quad.rect[0] < quad.rect[2] && quad.rect[1] < quad.rect[3]);
                }
            }
        }
        assert_eq!(layout.fonts.errors().len(), 1);
    }

    #[test]
    fn long_captions_shrink_to_fit_the_video() {
        let mut layout = TextLayout { fit_width: 1080.0, ..TextLayout::default() };
        let values = TextValues { time: 5.0, duration: 10.0, ..TextValues::default() };
        let mut quads = TextQuads::default();
        let short = Caption::new("Escape?", CaptionStyle::Hook);
        let long = Caption::new("How many balls will be left at the very end?", CaptionStyle::Hook);
        for caption in [short, long] {
            layout.layout(std::slice::from_ref(&caption), &values, 1.0, &mut quads);
            let left = quads.over.iter().map(|g| g.rect[0]).fold(f32::MAX, f32::min);
            let right = quads.over.iter().map(|g| g.rect[2]).fold(f32::MIN, f32::max);
            assert!(left > -540.0 && right < 540.0, "{}: {left}..{right}", caption.text);
        }
        // Without a video width, nothing shrinks.
        layout.fit_width = 0.0;
        let long = Caption::new("How many balls will be left at the very end?", CaptionStyle::Hook);
        layout.layout(&[long], &values, 1.0, &mut quads);
        let right = quads.over.iter().map(|g| g.rect[2]).fold(f32::MIN, f32::max);
        assert!(right > 540.0);
    }

    #[test]
    fn mono_digits_are_the_same_width() {
        let mut layout = TextLayout::default();
        let caption = Caption { font: Font::Mono, ..Caption::new("", CaptionStyle::Timer) };
        assert_eq!(layout.measure(&caption, "0:11").x, layout.measure(&caption, "8:08").x);
        let two_lines = layout.measure(&caption, "a\nb");
        assert!(two_lines.y > caption.size * 1.5);
    }

    #[test]
    fn a_full_atlas_starts_afresh() {
        // An atlas with room for a handful of letters.
        let mut layout = TextLayout { fonts: FontBook::default(), atlas: Atlas::new(200), fit_width: 0.0 };
        let mut quads = TextQuads::default();
        let caption = |text: &str| Caption { anim_in: Anim::None, ..Caption::new(text, CaptionStyle::Hook) };
        // One new letter a frame: each frame still draws its letter after the atlas fills up.
        for letter in 'A'..='Z' {
            layout.layout(&[caption(&letter.to_string())], &values(1.0), 1.0, &mut quads);
            assert_eq!(quads.over.len(), 1, "{letter}");
        }
        assert!(layout.atlas.generation > 0);
        // Text that can never fit draws what it can instead of looping forever.
        let long: String = ('A'..='Z').collect();
        layout.layout(&[caption(&long)], &values(1.0), 1.0, &mut quads);
        assert!(!quads.over.is_empty() && quads.over.len() < 26);
    }

    #[test]
    fn glitch_adds_coloured_ghosts() {
        let mut layout = TextLayout::default();
        let mut quads = TextQuads::default();
        let caption = Caption { anim_in: Anim::Glitch, anim_seconds: 1.0, ..Caption::new("Hi", CaptionStyle::Hook) };
        let mut most = 0;
        for i in 0..10 {
            layout.layout(std::slice::from_ref(&caption), &values(i as f32 * 0.05), 1.0, &mut quads);
            most = most.max(quads.over.len());
        }
        assert_eq!(most, 6);
    }

    /// A GPU device, or `None` to skip. Set BPE_REQUIRE_GPU=1 to fail instead of skipping, so a
    /// machine without a graphics driver can't pass GPU tests by accident.
    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        match crate::headless_device() {
            Ok(device) => Some(device),
            Err(error) if std::env::var_os("BPE_REQUIRE_GPU").is_some() => panic!("no GPU adapter: {error}"),
            Err(_) => {
                eprintln!("no GPU adapter available, skipping");
                None
            }
        }
    }

    fn render(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, time: f64) -> Vec<u8> {
        let mut renderer = crate::Renderer::new(device, 270, 480);
        let mut playback = bpe_sim::Playback::new(scene);
        playback.seek(time);
        let frame = crate::FrameData::build(scene, &playback, renderer.pixel_size(scene));
        renderer.render(device, queue, scene, time, &frame);
        renderer.read_output(device, queue).unwrap()
    }

    fn empty_scene() -> Scene {
        Scene { spawners: Vec::new(), ..Scene::default() }
    }

    #[test]
    fn captions_draw_white_letters_with_dark_outlines() {
        let Some((device, queue)) = gpu() else { return };
        let mut scene = empty_scene();
        let plain = render(&device, &queue, &scene, 1.0);
        scene.captions.push(Caption {
            anim_in: Anim::None,
            glow: 0.0,
            ..Caption::new("HELLO", CaptionStyle::BigCentre)
        });
        let text = render(&device, &queue, &scene, 1.0);
        let white = text.chunks(4).filter(|p| p[0] > 240 && p[1] > 240 && p[2] > 240).count();
        assert!(white > 500, "{white} white pixels");
        // Outside the caption's band the picture is untouched.
        let row = 270 * 4;
        assert_eq!(&text[..row * 100], &plain[..row * 100]);
        // The default outline puts a dark ring around the letters.
        assert!(text.chunks(4).any(|p| p[0] < 5 && p[1] < 5 && p[2] < 12));
    }

    #[test]
    fn show_text_rules_draw_their_words_for_a_while() {
        let Some((device, queue)) = gpu() else { return };
        let mut scene = empty_scene();
        scene.video.duration = 6.0;
        use bpe_graph::Catalog;
        let catalog = bpe_sim::rules::catalog();
        let mut graph = bpe_graph::Graph::new("Say it");
        let at = graph.add(catalog.spec("at").unwrap(), [0.0, 0.0]);
        graph.node_mut(at).unwrap().params.insert("time".into(), bpe_graph::Param::Number(2.0));
        let say = graph.add(catalog.spec("caption").unwrap(), [260.0, 0.0]);
        graph.node_mut(say).unwrap().params.insert("seconds".into(), bpe_graph::Param::Number(2.0));
        graph.connect(bpe_graph::Link { from: (at, 0), to: (say, 0) }, catalog);
        scene.logic = vec![graph];
        let white = |p: Vec<u8>| p.chunks(4).filter(|p| p[0] > 240 && p[1] > 240 && p[2] > 240).count();
        let before = white(render(&device, &queue, &scene, 1.0));
        let during = white(render(&device, &queue, &scene, 3.0));
        let after = white(render(&device, &queue, &scene, 5.0));
        assert!(during > before + 300 && during > after + 300, "{before} {during} {after}");
    }

    #[test]
    fn counters_change_as_balls_appear() {
        let Some((device, queue)) = gpu() else { return };
        let mut scene = Scene::multiplying_balls();
        scene.captions.push(Caption {
            anim_in: Anim::None,
            react: bpe_scene::React::None,
            ..Caption::new("{balls}", CaptionStyle::Counter)
        });
        let early = render(&device, &queue, &scene, 0.5);
        let late = render(&device, &queue, &scene, 8.0);
        let band = |p: &[u8]| {
            // Rows of the counter, near the bottom of the frame.
            let (top, bottom) = (480 * 3 / 4, 480 * 7 / 8);
            p[top * 270 * 4..bottom * 270 * 4].to_vec()
        };
        assert_ne!(band(&early), band(&late));
    }

    #[test]
    fn captions_under_effects_go_through_the_effects() {
        let Some((device, queue)) = gpu() else { return };
        let mut scene = empty_scene();
        scene.effects.push(bpe_scene::Effect::new("mirror"));
        let caption = Caption {
            anim_in: Anim::None,
            position: Vec2::new(-300.0, 0.0),
            ..Caption::new("A", CaptionStyle::BigCentre)
        };
        scene.captions.push(caption.clone());
        let over = render(&device, &queue, &scene, 1.0);
        scene.captions[0].under_effects = true;
        let under = render(&device, &queue, &scene, 1.0);
        assert_ne!(over, under);
    }
}
