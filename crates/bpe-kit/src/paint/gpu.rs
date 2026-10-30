//! Handing a paint's inside pattern to the graphics card: the gradient baked into the frame's
//! atlas and the pattern's numbers in a [`PaintGpu`].

use bpe_core::Transition;
use bpe_gfx::{GradientAtlas, PaintGpu};

use super::{Inputs, Inside, Paint, Repeat, offset};

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
fn repeat_code(repeat: Repeat) -> f32 {
    match repeat {
        Repeat::Clamp => 0.0,
        Repeat::Repeat => 1.0,
        Repeat::Mirror => 2.0,
    }
}

/// How `p` paints inside one shape at `time` seconds: its gradient's row in `atlas`, starting
/// where the paint puts this shape (`offset`), with the inside pattern's angle (turned by its
/// spin), size and scroll. No inside pattern paints nothing (row -1) and bakes nothing, so the
/// shape keeps its own colour.
pub fn paint_gpu(p: &Paint, inputs: &Inputs, atlas: &mut GradientAtlas, time: f32) -> PaintGpu {
    if p.inside == Inside::None {
        return PaintGpu::default();
    }
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    PaintGpu {
        row: atlas.row(&p.colours) as f32,
        t0: offset(p, inputs),
        pattern: pattern_code(p.inside),
        steps: if p.colours.transition == Transition::Steps { 1.0 } else { 0.0 },
        angle: finite(p.inside_angle + p.inside_spin * time),
        scale: finite(p.inside_scale),
        scroll: finite(p.inside_scroll),
        repeat: repeat_code(p.repeat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::Follow;
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
}
