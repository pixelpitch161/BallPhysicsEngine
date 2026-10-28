//! Making custom shapes from files: every path of an SVG drawing, or the outlines traced
//! around the solid parts of a picture.

use crate::Vec2;
use crate::shapes::{Path, Polyline, Segment, Shape, simplify};
use anyhow::{Context, bail};

/// A shape with where it sits once the whole file is fitted into the arena.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// Normalized: centred, farthest point 1 away.
    pub shape: Shape,
    pub center: Vec2,
    /// Distance from `center` to the shape's farthest point, in pixels.
    pub size: f32,
}

/// Which pixels of a picture count as solid when tracing it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TraceBy {
    /// See-through pixels are empty. Pictures without transparency fall back to `Dark`.
    #[default]
    Alpha,
    /// Dark pixels are solid, as in a black drawing on white paper.
    Dark,
    /// Light pixels are solid, as in a white drawing on a black background.
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceOptions {
    pub by: TraceBy,
    /// 0 to 1: where solid starts, in alpha or brightness.
    pub threshold: f32,
    /// How far, in picture pixels, the traced outline may stray to save points.
    pub smoothing: f32,
    /// Outlines smaller than this share of the picture's area are dropped as specks.
    pub min_area: f32,
}

impl Default for TraceOptions {
    fn default() -> Self {
        Self { by: TraceBy::Alpha, threshold: 0.5, smoothing: 0.8, min_area: 0.0005 }
    }
}

/// Pictures bigger than this many pixels on a side are scaled down before tracing.
const MAX_TRACE_SIDE: u32 = 768;

/// Every path in an SVG drawing as one shape, with its transforms applied, scaled so the
/// drawing's farthest point is `fit` pixels from the arena's middle.
pub fn import_svg(data: &[u8], name: &str, fit: f32) -> anyhow::Result<Placed> {
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).context("this file isn't an SVG drawing")?;
    let mut paths = Vec::new();
    collect_svg_paths(tree.root(), &mut paths);
    if paths.is_empty() {
        bail!("the drawing has no lines or shapes in it");
    }
    let mut shape = Shape { name: name.into(), paths };
    shape.normalize().context("the drawing has no size")?;
    Ok(Placed { shape, center: Vec2::ZERO, size: fit })
}

fn collect_svg_paths(group: &usvg::Group, out: &mut Vec<Path>) {
    use usvg::tiny_skia_path::PathSegment;
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => collect_svg_paths(group, out),
            usvg::Node::Path(path) => {
                let transform = path.abs_transform();
                let map = |p: usvg::tiny_skia_path::Point| {
                    let mut p = p;
                    transform.map_point(&mut p);
                    Vec2::new(p.x, p.y)
                };
                let mut current: Option<Path> = None;
                for segment in path.data().segments() {
                    match segment {
                        PathSegment::MoveTo(p) => {
                            out.extend(current.take().filter(|c| !c.segments.is_empty()));
                            current = Some(Path { closed: false, start: map(p), segments: Vec::new() });
                        }
                        PathSegment::LineTo(p) => push(&mut current, Segment::Line(map(p))),
                        PathSegment::QuadTo(c, p) => push(&mut current, Segment::Quad(map(c), map(p))),
                        PathSegment::CubicTo(c1, c2, p) => push(&mut current, Segment::Cubic(map(c1), map(c2), map(p))),
                        PathSegment::Close => {
                            if let Some(mut c) = current.take() {
                                c.closed = true;
                                let start = c.start;
                                out.push(c);
                                current = Some(Path { closed: false, start, segments: Vec::new() });
                            }
                        }
                    }
                }
                out.extend(current.take().filter(|c| !c.segments.is_empty()));
            }
            _ => {}
        }
    }
}

fn push(current: &mut Option<Path>, segment: Segment) {
    current.get_or_insert_with(Path::default).segments.push(segment);
}

/// Normalizes a shape given in picture pixels. `origin` and `scale` turn picture pixels into
/// pixels centred on the arena.
fn place(mut shape: Shape, origin: Vec2, scale: f32) -> anyhow::Result<Placed> {
    let (middle, reach) = shape.normalize().context("the outline has no size")?;
    Ok(Placed { shape, center: (middle - origin) * scale, size: reach * scale })
}

/// Traces the solid parts of a picture (PNG, JPEG, BMP, GIF or WebP) into outlines, biggest
/// first, scaled so the whole picture fits `fit` pixels from the arena's middle. The biggest
/// outline is meant to be the arena and the rest obstacles inside it.
pub fn trace_image(data: &[u8], name: &str, options: &TraceOptions, fit: f32) -> anyhow::Result<Vec<Placed>> {
    let picture = image::load_from_memory(data).context("this file isn't a picture I can read")?;
    let picture = if picture.width().max(picture.height()) > MAX_TRACE_SIDE {
        picture.resize(MAX_TRACE_SIDE, MAX_TRACE_SIDE, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let rgba = picture.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut by = options.by;
    if by == TraceBy::Alpha && rgba.pixels().all(|p| p[3] == 255) {
        by = TraceBy::Dark;
    }
    let threshold = options.threshold.clamp(0.0, 1.0);
    let solid: Vec<bool> = rgba
        .pixels()
        .map(|p| {
            let alpha = p[3] as f32 / 255.0;
            let bright = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0;
            match by {
                TraceBy::Alpha => alpha >= threshold,
                TraceBy::Dark => alpha >= 0.5 && bright < threshold,
                TraceBy::Light => alpha >= 0.5 && bright >= threshold,
            }
        })
        .collect();
    let loops = trace_mask(&solid, width as usize, height as usize);
    let min_area = options.min_area.max(0.0) * (width * height) as f32;
    let mut outlines: Vec<Polyline> = loops
        .into_iter()
        .map(|points| Polyline { closed: true, points: simplify(&points, options.smoothing.max(0.0), true) })
        .filter(|line| line.points.len() >= 3 && line.signed_area().abs() >= min_area.max(1.0))
        .collect();
    if outlines.is_empty() {
        bail!("nothing solid found in the picture; try another threshold or trace by brightness");
    }
    outlines.sort_by(|a, b| b.signed_area().abs().total_cmp(&a.signed_area().abs()));
    let origin = Vec2::new(width as f32, height as f32) * 0.5;
    let scale = fit / origin.length().max(1.0);
    outlines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let label = if i == 0 { name.to_string() } else { format!("{name} {}", i + 1) };
            let shape = Shape { name: label, paths: vec![Path::polygon(&line.points, true)] };
            place(shape, origin, scale)
        })
        .collect()
}

/// Marching squares: closed outlines (in pixel units, corners at whole numbers) around every
/// solid region and every hole of a `width` by `height` mask.
pub fn trace_mask(solid: &[bool], width: usize, height: usize) -> Vec<Vec<Vec2>> {
    // Sample corners of a grid one pixel bigger on every side, so every outline closes.
    let at = |x: isize, y: isize| -> bool {
        x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height && solid[y as usize * width + x as usize]
    };
    // Edges are named by integer ids, so pieces join exactly: horizontal edge (x, y) runs from
    // corner (x, y) to (x + 1, y), vertical edge (x, y) from (x, y) to (x, y + 1).
    let cols = width + 2;
    let rows = height + 2;
    let h_edge = |x: usize, y: usize| (y * cols + x) * 2;
    let v_edge = |x: usize, y: usize| (y * cols + x) * 2 + 1;
    let midpoint = |edge: usize| {
        let cell = edge / 2;
        let (x, y) = ((cell % cols) as f32 - 1.0, (cell / cols) as f32 - 1.0);
        if edge.is_multiple_of(2) { Vec2::new(x + 0.5, y) } else { Vec2::new(x, y + 0.5) }
    };
    // Each solid region's outline runs with solid on the same side, so every edge has at most
    // one piece leaving it.
    let mut next = vec![usize::MAX; cols * rows * 2];
    let mut starts = Vec::new();
    for y in 0..rows - 1 {
        for x in 0..cols - 1 {
            let (px, py) = (x as isize - 1, y as isize - 1);
            let (tl, tr, br, bl) = (at(px, py), at(px + 1, py), at(px + 1, py + 1), at(px, py + 1));
            let case = (tl as u8) << 3 | (tr as u8) << 2 | (br as u8) << 1 | bl as u8;
            let (top, right, bottom, left) = (h_edge(x, y), v_edge(x + 1, y), h_edge(x, y + 1), v_edge(x, y));
            // Pieces go so the solid side is on the right when walking along them.
            let pieces: &[(usize, usize)] = match case {
                0 | 15 => &[],
                1 => &[(left, bottom)],
                2 => &[(bottom, right)],
                3 => &[(left, right)],
                4 => &[(right, top)],
                // Solid corners that touch diagonally stay separate regions.
                5 => &[(right, top), (left, bottom)],
                6 => &[(bottom, top)],
                7 => &[(left, top)],
                8 => &[(top, left)],
                9 => &[(top, bottom)],
                10 => &[(top, left), (bottom, right)],
                11 => &[(top, right)],
                12 => &[(right, left)],
                13 => &[(right, bottom)],
                14 => &[(bottom, left)],
                _ => unreachable!(),
            };
            for &(from, to) in pieces {
                next[from] = to;
                starts.push(from);
            }
        }
    }
    let mut loops = Vec::new();
    for start in starts {
        if next[start] == usize::MAX {
            continue;
        }
        let mut points = Vec::new();
        let mut edge = start;
        while next[edge] != usize::MAX {
            points.push(midpoint(edge));
            let to = next[edge];
            next[edge] = usize::MAX;
            edge = to;
        }
        if points.len() >= 3 {
            loops.push(points);
        }
    }
    loops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::inside;

    #[test]
    fn a_filled_square_traces_to_one_loop() {
        let (w, h) = (10, 8);
        let mut mask = vec![false; w * h];
        for y in 2..6 {
            for x in 3..8 {
                mask[y * w + x] = true;
            }
        }
        let loops = trace_mask(&mask, w, h);
        assert_eq!(loops.len(), 1);
        let line = Polyline { closed: true, points: simplify(&loops[0], 0.01, true) };
        // Corners are cut by half a pixel, so 8 points: an octagon hugging the 5 x 4 block.
        assert_eq!(line.points.len(), 8);
        assert!((line.signed_area().abs() - (20.0 - 0.5)).abs() < 1e-3, "{}", line.signed_area());
        assert!(inside(std::slice::from_ref(&line), Vec2::new(5.0, 4.0)));
    }

    #[test]
    fn a_ring_traces_to_an_outline_and_a_hole() {
        let (w, h) = (12, 12);
        let mask: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32 - 5.5, (i / w) as f32 - 5.5);
                let r = (x * x + y * y).sqrt();
                (2.5..5.0).contains(&r)
            })
            .collect();
        let loops = trace_mask(&mask, w, h);
        assert_eq!(loops.len(), 2);
    }

    #[test]
    fn empty_and_full_masks() {
        assert!(trace_mask(&[], 0, 0).is_empty());
        assert!(trace_mask(&[false; 16], 4, 4).is_empty());
        assert_eq!(trace_mask(&[true; 16], 4, 4).len(), 1);
        assert_eq!(trace_mask(&[true], 1, 1).len(), 1);
    }

    #[test]
    fn checkerboard_corners_never_join_wrongly() {
        // Two pixels touching only at a corner make two separate loops.
        let mask = [true, false, false, true];
        assert_eq!(trace_mask(&mask, 2, 2).len(), 2);
    }

    fn sample(name: &str) -> Vec<u8> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes/shapes");
        std::fs::read(dir.join(name)).unwrap()
    }

    #[test]
    fn svg_heart_imports_as_a_closed_shape() {
        let placed = import_svg(&sample("heart.svg"), "Heart", 450.0).unwrap();
        assert!(placed.shape.is_closed());
        assert!((placed.size - 450.0).abs() < 1.0, "size {}", placed.size);
        assert!(placed.center.length() < 30.0, "centre {}", placed.center);
    }

    #[test]
    fn svg_transforms_are_applied() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <g transform="translate(50 0)"><rect x="0" y="0" width="50" height="10"/></g>
            <rect x="0" y="90" width="10" height="10" transform="rotate(45 5 95)"/>
        </svg>"#;
        let placed = import_svg(svg, "Boxes", 100.0).unwrap();
        assert_eq!(placed.shape.paths.len(), 2);
        let (lo, hi) = placed.shape.bounds().unwrap();
        // The wide bar sits top right and the small square bottom left, so the drawing spans both.
        assert!(hi.x - lo.x > 1.2 && hi.y - lo.y > 1.2, "{lo} {hi}");
    }

    #[test]
    fn bad_files_give_errors_not_panics() {
        assert!(import_svg(b"not an svg", "x", 100.0).is_err());
        assert!(import_svg(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#, "x", 100.0).is_err());
        assert!(trace_image(b"\x89PNG broken", "x", &TraceOptions::default(), 100.0).is_err());
    }

    #[test]
    fn traced_picture_gives_the_biggest_outline_first() {
        let placed = trace_image(&sample("blobs.png"), "Blobs", &TraceOptions::default(), 500.0).unwrap();
        assert!(placed.len() >= 2, "{} outlines", placed.len());
        assert!(placed.windows(2).all(|w| w[0].size >= w[1].size * 0.5));
        assert!(placed.iter().all(|p| p.shape.is_closed()));
        // The obstacles sit inside the big outline.
        let arena = &placed[0];
        let lines = arena.shape.flatten(arena.size);
        for small in &placed[1..] {
            let local = (small.center - arena.center) / arena.size;
            assert!(inside(&lines, local), "{} at {}", small.shape.name, small.center);
        }
    }
}
