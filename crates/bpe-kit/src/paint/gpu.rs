//! Handing a paint's inside pattern to the graphics card: the gradient baked into the frame's
//! atlas and the pattern's numbers in a [`PaintGpu`].

use bpe_core::Transition;
use bpe_gfx::{BackgroundPaint, GradientAtlas, PaintGpu};
use bpe_runtime::CameraView;

use super::{Follow, Inputs, Inside, Paint, PlaceShape, Repeat, Space, offset};

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

/// How `p` paints inside one shape at `time` seconds: its gradient's row in `atlas`, centred
/// where the paint puts this shape (`offset`), with the inside pattern's angle (turned by its
/// spin), repeats across the shape and scroll. No inside pattern paints nothing (row -1) and
/// bakes nothing, so the shape keeps its own colour; except a paint that follows Path, which
/// paints along the wall's line (pattern 4) from start to end, moving by the paint's scroll. A
/// pattern or path never runs off its gradient's end: the paint's Clamp mirrors it instead. An
/// inside pattern wins over Path, which then only gives the start (`Inputs::path`, 0 for a
/// wall).
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

/// How the background pass paints `p`. A Place paint leaves the place to the shader, which
/// works it out per pixel (its scroll running with the frame's time); a Time or Beat paint is a
/// single place for the whole picture, worked out here from `inputs` (`time` on the simulation
/// clock, `beats` on the video clock). The caller says in `Frame::background_on_screen` whether
/// the place is measured on the screen or in the world; the paint's own space is ignored.
pub fn background_gpu(p: &Paint, inputs: &Inputs, atlas: &mut GradientAtlas) -> BackgroundPaint {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    let steps = if p.colours.transition == Transition::Steps { 1.0 } else { 0.0 };
    let row = atlas.row(&p.colours);
    let ends = match p.colours.stops.as_slice() {
        [a, b] if a.at == 0.0 && b.at == 1.0 && p.colours.transition == Transition::Smooth => {
            Some([a.color.to_linear(), b.color.to_linear()])
        }
        _ => None,
    };
    let place = [finite(p.angle), finite(p.span).max(1e-3), finite(p.centre.x), finite(p.centre.y)];
    if p.follow == Follow::Place {
        let mode = if p.shape == PlaceShape::Centre { 2.0 } else { 1.0 };
        return BackgroundPaint {
            row,
            place,
            scroll: finite(p.scroll),
            repeat: repeat_code(p.repeat),
            mode,
            steps,
            t0: 0.0,
            ends,
        };
    }
    // Hard bands step one stop per beat, whatever the stops' places: land on that stop's place.
    let n = p.colours.stops.len().max(1);
    let t0 = if p.follow == Follow::Beat && steps > 0.5 {
        let k = finite(inputs.beats).max(0.0).floor() as usize % n;
        p.colours.stops.get(k).map_or(0.0, |s| finite(s.at).clamp(0.0, 1.0))
    } else {
        offset(p, inputs)
    };
    let repeat = if p.follow == Follow::Beat && steps > 0.5 { 0.0 } else { repeat_code(p.repeat) };
    BackgroundPaint { row, place, t0, scroll: 0.0, repeat, mode: 0.0, steps, ends }
}

/// `p`'s gradient for a shader that works out the place along it itself (from a speed, a place
/// or a hash) and calls `paint_along`: its row in `atlas`, moved on by the scroll `time` seconds
/// in, with its steps and repeat.
pub fn along_gpu(p: &Paint, time: f32, atlas: &mut GradientAtlas) -> PaintGpu {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    PaintGpu {
        row: atlas.row(&p.colours) as f32,
        t0: finite(time * p.scroll),
        steps: if p.colours.transition == Transition::Steps { 1.0 } else { 0.0 },
        repeat: repeat_code(p.repeat),
        ..PaintGpu::default()
    }
}

/// Where `p`'s place runs, for a shader that measures places in the world: the direction in
/// degrees, the span, and the centre. A place measured on the screen is turned into the same
/// place in the world through `camera` (screen = turned, zoomed world), so the shader paints
/// what [`offset`] gives for `Inputs::place_screen` without knowing the camera.
pub fn place_in_world(p: &Paint, camera: &CameraView) -> [f32; 4] {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    let span = finite(p.span).max(1e-3);
    let zoom = if camera.zoom.is_finite() && camera.zoom > 1e-6 { camera.zoom } else { 1.0 };
    match p.space {
        Space::World => [finite(p.angle), span, finite(p.centre.x), finite(p.centre.y)],
        Space::Screen => {
            let centre = camera.to_world(p.centre);
            [finite(p.angle + camera.turn.to_degrees()), span / zoom, finite(centre.x), finite(centre.y)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, Gradient, Stop};

    #[test]
    fn a_screen_place_in_the_world_paints_what_offset_gives() {
        use super::super::PlaceShape;
        use glam::Vec2;
        let camera = CameraView { center: Vec2::new(120.0, -40.0), zoom: 1.7, turn: 0.6 };
        for shape in [PlaceShape::Direction, PlaceShape::Centre] {
            for space in [Space::World, Space::Screen] {
                let p = Paint {
                    shape,
                    space,
                    angle: 30.0,
                    centre: Vec2::new(-50.0, 80.0),
                    span: 900.0,
                    repeat: Repeat::Repeat,
                    ..Paint::default()
                };
                let [angle, span, cx, cy] = place_in_world(&p, &camera);
                for world in [Vec2::new(0.0, 0.0), Vec2::new(300.0, -200.0), Vec2::new(-410.0, 650.0)] {
                    let inputs = Inputs { place: world, place_screen: camera.to_screen(world), ..Inputs::default() };
                    // What the shaders work out from the world place.
                    let d = world - Vec2::new(cx, cy);
                    let raw = match shape {
                        PlaceShape::Direction => d.dot(Vec2::from_angle(angle.to_radians())) / span + 0.5,
                        PlaceShape::Centre => d.length() / span,
                    };
                    let want = offset(&p, &inputs);
                    let got = raw - raw.floor();
                    assert!((got - want).abs() < 1e-4, "{shape:?} {space:?} at {world}: {got} vs {want}");
                }
            }
        }
    }

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

    #[test]
    fn a_background_paint_leaves_a_place_to_the_shader_and_fixes_time_and_beat() {
        let mut atlas = GradientAtlas::default();
        let colours = Gradient::even(&[Color::rgb(255, 0, 0), Color::WHITE]);
        let place = Paint { colours: colours.clone(), angle: 30.0, span: 800.0, scroll: 0.5, ..Paint::default() };
        let gpu = background_gpu(&place, &Inputs::default(), &mut atlas);
        assert_eq!((gpu.mode, gpu.scroll, gpu.t0, gpu.place), (1.0, 0.5, 0.0, [30.0, 800.0, 0.0, 0.0]));
        assert_eq!(gpu.ends, Some([Color::rgb(255, 0, 0).to_linear(), Color::WHITE.to_linear()]));
        let centre = Paint { shape: PlaceShape::Centre, ..place.clone() };
        assert_eq!(background_gpu(&centre, &Inputs::default(), &mut atlas).mode, 2.0);

        let time = Paint { follow: Follow::Time, scroll: 0.5, ..place.clone() };
        let gpu = background_gpu(&time, &Inputs { time: 1.0, ..Inputs::default() }, &mut atlas);
        assert_eq!((gpu.mode, gpu.scroll, gpu.t0), (0.0, 0.0, 0.5));
        let beat = Paint { follow: Follow::Beat, ..place };
        let gpu = background_gpu(&beat, &Inputs { beats: 0.5, ..Inputs::default() }, &mut atlas);
        assert_eq!((gpu.mode, gpu.t0), (0.0, 0.25));

        // Hard bands land on the stop's own place.
        let bands = Gradient {
            stops: vec![Stop { at: 0.0, color: Color::rgb(255, 0, 0) }, Stop { at: 0.7, color: Color::WHITE }],
            transition: Transition::Steps,
        };
        let beat = Paint { follow: Follow::Beat, colours: bands, ..Paint::default() };
        let gpu = background_gpu(&beat, &Inputs { beats: 1.2, ..Inputs::default() }, &mut atlas);
        assert_eq!((gpu.t0, gpu.steps), (0.7, 1.0));
    }
}
