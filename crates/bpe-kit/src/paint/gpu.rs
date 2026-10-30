//! Handing a paint's inside pattern to the graphics card: the gradient baked into the frame's
//! atlas and the pattern's numbers in a [`PaintGpu`].

use bpe_core::Transition;
use bpe_gfx::{GradientAtlas, PaintGpu};

use super::{Follow, Inputs, Inside, Paint, Repeat, offset};

/// Pattern 4 in paint.wgsl: a gradient along a wall's line, the shape working out how far round
/// it each pixel is.
pub const PATH_PATTERN: f32 = 4.0;

/// The pattern's number in paint.wgsl.
fn pattern_code(inside: Inside) -> f32 {
    match inside {
        Inside::None => 0.0,
        Inside::Stripes => 1.0,
        Inside::Rings => 2.0,
        Inside::Sweep => 3.0,
    }
}

/// The repeat's number in paint.wgsl.
pub(crate) fn repeat_code(repeat: Repeat) -> f32 {
    match repeat {
        Repeat::Clamp => 0.0,
        Repeat::Repeat => 1.0,
        Repeat::Mirror => 2.0,
    }
}

/// How `p` paints inside one shape at `time` seconds: its gradient's row in `atlas`, starting
/// where the paint puts this shape (`offset`), with the inside pattern's angle (turned by its
/// spin), size and scroll. No inside pattern paints nothing (row -1) and bakes nothing, so the
/// shape keeps its own colour; except a paint that follows Path, which paints along the wall's
/// line (pattern 4) once from start to end, moving by the paint's scroll. An inside pattern wins
/// over Path, which then only gives the start (`Inputs::path`, 0 for a wall).
pub fn paint_gpu(p: &Paint, inputs: &Inputs, atlas: &mut GradientAtlas, time: f32) -> PaintGpu {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    let steps = if p.colours.transition == Transition::Steps { 1.0 } else { 0.0 };
    if p.inside == Inside::None {
        if p.follow != Follow::Path {
            return PaintGpu::default();
        }
        return PaintGpu {
            row: atlas.row(&p.colours) as f32,
            pattern: PATH_PATTERN,
            steps,
            scroll: finite(p.scroll),
            repeat: repeat_code(p.repeat),
            ..PaintGpu::default()
        };
    }
    PaintGpu {
        row: atlas.row(&p.colours) as f32,
        t0: offset(p, inputs),
        pattern: pattern_code(p.inside),
        steps,
        angle: finite(p.inside_angle + p.inside_spin * time),
        scale: finite(p.inside_scale),
        scroll: finite(p.inside_scroll),
        repeat: repeat_code(p.repeat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, Gradient};

    #[test]
    fn no_inside_pattern_paints_nothing_and_bakes_nothing() {
        let mut atlas = GradientAtlas::default();
        let p = Paint { colours: Gradient::even(&[Color::rgb(255, 0, 0), Color::WHITE]), ..Paint::default() };
        assert_eq!(paint_gpu(&p, &Inputs::default(), &mut atlas, 1.0), PaintGpu::default());
        assert!(atlas.is_empty());
    }

    #[test]
    fn rings_spin_and_start_where_the_paint_puts_the_ball() {
        let mut atlas = GradientAtlas::default();
        let p = Paint {
            colours: Gradient::even(&[Color::rgb(255, 0, 0), Color::WHITE]),
            follow: Follow::Order,
            inside: Inside::Rings,
            inside_angle: 10.0,
            inside_spin: 90.0,
            inside_scale: 3.0,
            inside_scroll: 0.5,
            repeat: Repeat::Mirror,
            ..Paint::default()
        };
        let inputs = Inputs { index: 1, count: 3, ..Inputs::default() };
        let gpu = paint_gpu(&p, &inputs, &mut atlas, 2.0);
        assert_eq!(
            gpu,
            PaintGpu {
                row: 0.0,
                t0: 0.5,
                pattern: 2.0,
                steps: 0.0,
                angle: 190.0,
                scale: 3.0,
                scroll: 0.5,
                repeat: 2.0
            }
        );
        assert_eq!(atlas.len(), 1);
        // The same gradient on the next ball shares the row.
        assert_eq!(paint_gpu(&p, &Inputs { index: 2, ..inputs }, &mut atlas, 2.0).row, 0.0);
        assert_eq!(atlas.len(), 1);
    }

    #[test]
    fn path_paints_along_the_line_without_an_inside_pattern() {
        let mut atlas = GradientAtlas::default();
        let p = Paint {
            colours: Gradient::even(&[Color::rgb(255, 0, 0), Color::WHITE]),
            follow: Follow::Path,
            scroll: 0.25,
            repeat: Repeat::Repeat,
            ..Paint::default()
        };
        let gpu = paint_gpu(&p, &Inputs::default(), &mut atlas, 3.0);
        assert_eq!(gpu, PaintGpu { row: 0.0, pattern: PATH_PATTERN, scroll: 0.25, repeat: 1.0, ..PaintGpu::default() });
        // An inside pattern wins: Path only places its start.
        let rings = Paint { inside: Inside::Rings, ..p };
        let gpu = paint_gpu(&rings, &Inputs { path: 0.5, ..Inputs::default() }, &mut atlas, 0.0);
        assert_eq!((gpu.pattern, gpu.t0), (2.0, 0.5));
        assert_eq!(atlas.len(), 1);
    }
}
