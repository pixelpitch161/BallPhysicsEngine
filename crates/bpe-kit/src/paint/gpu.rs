//! Handing what is painted inside a shape to the graphics card: the gradient baked into the
//! frame's atlas and the pattern's numbers in a [`PaintGpu`].

use bpe_gfx::{BackgroundPaint, GradientAtlas, PaintGpu};
use bpe_runtime::CameraView;

use bpe_core::{Gradient, GradientForm, Layout};

use super::{Follow, Inputs, InsideFrom, InsidePaint, Paint, Pattern, Repeat, Space, offset, vary_gpu};

/// Pattern 4 in paint.wgsl: a gradient along a wall's line, the shape working out how far round
/// it each pixel is.
pub const PATH_PATTERN: f32 = 4.0;

/// A mask pattern's number in paint.wgsl (a gradient's is its form's).
fn mask_code(pattern: Pattern) -> f32 {
    match pattern {
        Pattern::None | Pattern::Gradient => 0.0,
        Pattern::Dots => 7.0,
        Pattern::Checker => 8.0,
        Pattern::Halves => 9.0,
        Pattern::Slices => 10.0,
        Pattern::Swirl => 11.0,
        Pattern::Face => 12.0,
    }
}

/// Row -2 in paint.wgsl: a shade of the instance's own colour instead of a gradient.
const SHADE_ROW: f32 = -2.0;

/// Where a gradient's form sits at its site: its turn in degrees, its centre in the site's own
/// units, how much the site's point is scaled by (1 / size), what happens past its end, and how
/// fast it scrolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placing {
    pub turn: f32,
    pub centre: [f32; 2],
    pub k: f32,
    pub repeat: Repeat,
    pub scroll: f32,
}

impl Placing {
    /// A site that takes its place from the gradient's own companions (Turn, Centre, Size, Beyond
    /// the last colour), with today's defaults where none is stored: 90°, the middle, 1, Clamp.
    pub fn of_layout(l: &Layout) -> Self {
        use bpe_core::gradient_form::defaults;
        let size = l.size.filter(|s| s.is_finite() && *s > 1e-6).unwrap_or(defaults::SIZE);
        Self {
            turn: l.turn.filter(|t| t.is_finite()).unwrap_or(defaults::TURN),
            centre: l.centre.filter(|c| c[0].is_finite() && c[1].is_finite()).unwrap_or([0.0; 2]),
            k: 1.0 / size,
            repeat: l.repeat.unwrap_or_default(),
            scroll: 0.0,
        }
    }
}

/// `g` laid out by its form (the stored one, else `default`) at `place`: its atlas row, the
/// form's code and numbers, its centre, turn, scale, repeat, scroll and grain. Free points also
/// bake their points into a row of `atlas` (`form[1]`).
pub fn form_gpu(g: &Gradient, default: GradientForm, place: Placing, atlas: &mut GradientAtlas) -> PaintGpu {
    let finite = |x: f32, or: f32| if x.is_finite() { x } else { or };
    let form = g.layout.form_or(default);
    let mut numbers = g.layout.numbers(form);
    if form == GradientForm::FreePoints {
        let points = bpe_core::gradient_form::free_points(g, &g.layout);
        numbers[1] = atlas.points_row(&points) as f32;
        numbers[2] = points.len() as f32;
        numbers[3] = if bpe_core::gradient_form::free_in_oklab(g.mix) { 1.0 } else { 0.0 };
    }
    PaintGpu {
        row: atlas.row(g) as f32,
        pattern: form.code(),
        grain: g.grain,
        angle: finite(place.turn, 90.0),
        scale: finite(place.k, 1.0),
        scroll: finite(place.scroll, 0.0),
        repeat: place.repeat.code(),
        form: numbers,
        place: [finite(place.centre[0], 0.0), finite(place.centre[1], 0.0), 0.0, 0.0],
        ..PaintGpu::default()
    }
}

/// How `i` paints inside one shape at `time` seconds, its angle turned by its spin. No pattern
/// paints nothing (row -1). A shade of the shape's own colour bakes nothing and puts the shade
/// in `t0` (row -2, see [`PaintGpu::row`]); its own gradient takes a row of `atlas`, its repeat
/// and grain, and `vary`, the instance's colour variation ([`vary_gpu`]). Gradient lays the
/// colours out by its form, placed by the pattern's Angle and Size.
pub fn inside_gpu(i: &InsidePaint, vary: [f32; 4], atlas: &mut GradientAtlas, time: f32) -> PaintGpu {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    if i.pattern == Pattern::None {
        return PaintGpu::default();
    }
    let angle = finite(i.angle + i.spin * time);
    let shape = if i.pattern == Pattern::Gradient {
        let place =
            Placing { turn: angle, centre: [0.0; 2], k: finite(i.size), repeat: i.repeat, scroll: finite(i.scroll) };
        // A shade bakes nothing: its colours come from the ball.
        let shade = i.from == InsideFrom::ShadeOfOwnColour;
        let mut scratch = GradientAtlas::default();
        let mut p = form_gpu(&i.colours, i.form, place, if shade { &mut scratch } else { atlas });
        // A shade has no stops of its own to place.
        if i.from == InsideFrom::ShadeOfOwnColour && p.pattern == GradientForm::FreePoints.code() {
            p.pattern = GradientForm::Round.code();
        }
        p
    } else {
        PaintGpu {
            pattern: mask_code(i.pattern),
            angle,
            scale: finite(i.size),
            scroll: finite(i.scroll),
            repeat: i.repeat.code(),
            ..PaintGpu::default()
        }
    };
    match i.from {
        InsideFrom::ShadeOfOwnColour => {
            PaintGpu { row: SHADE_ROW, t0: finite(i.shade).clamp(-1.0, 1.0), grain: 0.0, ..shape }
        }
        InsideFrom::OwnGradient => {
            let row = if i.pattern == Pattern::Gradient { shape.row } else { atlas.row(&i.colours) as f32 };
            PaintGpu { row, grain: i.colours.grain, vary, ..shape }
        }
    }
}

/// The repeat's number in paint.wgsl.
pub(crate) fn repeat_code(repeat: Repeat) -> f32 {
    repeat.code()
}

/// How `p` paints along a wall's line: a paint that follows Path paints its gradient's row in
/// `atlas` from the line's start to its end (pattern 4), moving by the paint's scroll, with the
/// paint's variation for this instance (`Inputs::seed`, at `Inputs::time`) in `vary`. Any other
/// paint paints nothing (row -1) and bakes nothing, so the shape keeps its own colour.
pub fn paint_gpu(p: &Paint, inputs: &Inputs, atlas: &mut GradientAtlas) -> PaintGpu {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    if p.follow != Follow::Path {
        return PaintGpu::default();
    }
    PaintGpu {
        row: atlas.row(&p.colours) as f32,
        pattern: PATH_PATTERN,
        grain: p.colours.grain,
        scroll: finite(p.scroll),
        repeat: repeat_code(p.repeat),
        vary: vary_gpu(p, inputs.seed, inputs.time),
        ..PaintGpu::default()
    }
}

/// How the background pass paints `p`. A Place paint leaves the place to the shader, which
/// works it out per pixel (its scroll running with the frame's time); a Time paint is a
/// single place for the whole picture, worked out here from `inputs`. The caller says in `Frame::background_on_screen` whether
/// the place is measured on the screen or in the world; the paint's own space is ignored.
pub fn background_gpu(p: &Paint, inputs: &Inputs, atlas: &mut GradientAtlas) -> BackgroundPaint {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    let grain = p.colours.grain;
    let row = atlas.row(&p.colours);
    // Exactly a straight blend of two colours in linear light: the shader mixes it itself.
    let plain = |s: &bpe_core::Stop| s.blend == bpe_core::GapBlend::Smooth && s.middle == 0.5;
    let ends = match p.colours.stops.as_slice() {
        [a, b]
            if a.at == 0.0
                && b.at == 1.0
                && p.colours.mix == bpe_core::Mix::Light
                && !p.colours.looped
                && p.colours.moves.is_still()
                && plain(a) =>
        {
            Some([a.color.to_linear(), b.color.to_linear()])
        }
        _ => None,
    };
    let place = [finite(p.angle), finite(p.span).max(1e-3), finite(p.centre.x), finite(p.centre.y)];
    if p.follow == Follow::Place {
        let form = p.form();
        let (mode, pattern, numbers) = match form {
            GradientForm::Straight => (1.0, 0.0, [0.0; 4]),
            GradientForm::Round => (2.0, 0.0, [0.0; 4]),
            // Any other form is laid over the place in spans from the centre.
            _ => {
                let unit = Placing { turn: p.angle, centre: [0.0; 2], k: 1.0, repeat: p.repeat, scroll: p.scroll };
                let g = form_gpu(&p.colours, form, unit, atlas);
                (3.0, g.pattern, g.form)
            }
        };
        return BackgroundPaint {
            row,
            place,
            scroll: finite(p.scroll),
            repeat: repeat_code(p.repeat),
            mode,
            grain,
            t0: 0.0,
            ends: if mode < 2.5 { ends } else { None },
            pattern,
            form: numbers,
        };
    }
    let t0 = offset(p, inputs);
    BackgroundPaint {
        row,
        place,
        t0,
        scroll: 0.0,
        repeat: repeat_code(p.repeat),
        mode: 0.0,
        grain,
        ends,
        pattern: 0.0,
        form: [0.0; 4],
    }
}

/// `p`'s gradient for a shader that works out the place along it itself (from a speed, a place
/// or a hash) and calls `paint_along`: its row in `atlas`, moved on by the scroll `time` seconds
/// in, with its grain and repeat.
pub fn along_gpu(p: &Paint, time: f32, atlas: &mut GradientAtlas) -> PaintGpu {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    PaintGpu {
        row: atlas.row(&p.colours) as f32,
        t0: finite(time * p.scroll),
        grain: p.colours.grain,
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
    use bpe_core::{Color, Gradient};

    #[test]
    fn a_screen_place_in_the_world_paints_what_offset_gives() {
        use bpe_core::gradient_form::{along, turned};
        use glam::Vec2;
        let camera = CameraView { center: Vec2::new(120.0, -40.0), zoom: 1.7, turn: 0.6 };
        let forms = GradientForm::ALL.iter().filter(|f| !matches!(f, GradientForm::FreePoints | GradientForm::Ripples));
        for &form in forms {
            for space in [Space::World, Space::Screen] {
                let mut colours = Paint::default().colours;
                colours.layout.form = Some(form);
                let p = Paint {
                    colours,
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
                    let raw = match form {
                        GradientForm::Straight => d.dot(Vec2::from_angle(angle.to_radians())) / span + 0.5,
                        _ => along(form, &p.colours.layout, turned(d / span, -angle), 1.0, 0.0),
                    };
                    let want = offset(&p, &inputs);
                    let got = raw - raw.floor();
                    assert!((got - want).abs() < 1e-3, "{form:?} {space:?} at {world}: {got} vs {want}");
                }
            }
        }
    }

    #[test]
    fn inside_gpu_codes() {
        use super::super::{InsideFrom, InsidePaint, Pattern};
        let mut atlas = GradientAtlas::default();
        let own = InsidePaint {
            pattern: Pattern::Gradient,
            form: GradientForm::Straight,
            from: InsideFrom::OwnGradient,
            colours: Gradient::even(&[Color::rgb(0, 0, 255), Color::rgb(0, 255, 0)]),
            repeat: Repeat::Mirror,
            angle: 10.0,
            spin: 90.0,
            size: 2.0,
            scroll: 0.5,
            ..InsidePaint::default()
        };
        let vary = [0.25, 0.0, 1.0, 0.0];
        assert_eq!(
            inside_gpu(&own, vary, &mut atlas, 2.0),
            PaintGpu {
                row: 0.0,
                t0: 0.0,
                pattern: 1.0,
                grain: 0.0,
                angle: 190.0,
                scale: 2.0,
                scroll: 0.5,
                repeat: 2.0,
                vary,
                form: [0.0; 4],
                place: [0.0; 4],
            }
        );
        assert_eq!(atlas.len(), 1);
        let face = InsidePaint { pattern: Pattern::Face, ..own.clone() };
        assert_eq!(inside_gpu(&face, vary, &mut atlas, 0.0).pattern, 12.0);
        let masks: Vec<f32> = [Pattern::Dots, Pattern::Checker, Pattern::Halves, Pattern::Slices, Pattern::Swirl]
            .into_iter()
            .map(|pattern| inside_gpu(&InsidePaint { pattern, ..own.clone() }, vary, &mut atlas, 0.0).pattern)
            .collect();
        assert_eq!(masks, [7.0, 8.0, 9.0, 10.0, 11.0]);
        // Gradient lays the colours out by every form's own code.
        for form in GradientForm::ALL {
            let p = inside_gpu(&InsidePaint { form, ..own.clone() }, vary, &mut atlas, 0.0);
            assert_eq!(p.pattern, form.code(), "{form:?}");
        }

        // A shade of the ball's own colour bakes nothing.
        let mut atlas = GradientAtlas::default();
        let shade = InsidePaint { from: InsideFrom::ShadeOfOwnColour, shade: -0.75, ..own.clone() };
        let gpu = inside_gpu(&shade, vary, &mut atlas, 0.0);
        assert_eq!((gpu.row, gpu.t0, gpu.pattern), (-2.0, -0.75, 1.0));
        assert!(atlas.is_empty());

        let none = InsidePaint { pattern: Pattern::None, ..own };
        assert_eq!(inside_gpu(&none, vary, &mut atlas, 0.0), PaintGpu::default());
        assert!(atlas.is_empty());
    }

    #[test]
    fn a_paint_off_the_path_paints_nothing_and_bakes_nothing() {
        let mut atlas = GradientAtlas::default();
        let p = Paint { colours: Gradient::even(&[Color::rgb(255, 0, 0), Color::WHITE]), ..Paint::default() };
        assert_eq!(paint_gpu(&p, &Inputs::default(), &mut atlas), PaintGpu::default());
        assert!(atlas.is_empty());
    }

    #[test]
    fn path_paints_along_the_line() {
        let mut atlas = GradientAtlas::default();
        let p = Paint {
            colours: Gradient::even(&[Color::rgb(255, 0, 0), Color::WHITE]),
            follow: Follow::Path,
            scroll: 0.25,
            repeat: Repeat::Repeat,
            ..Paint::default()
        };
        let gpu = paint_gpu(&p, &Inputs::default(), &mut atlas);
        assert_eq!(gpu, PaintGpu { row: 0.0, pattern: PATH_PATTERN, scroll: 0.25, repeat: 1.0, ..PaintGpu::default() });
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
        let mut round = colours.clone();
        round.layout.form = Some(GradientForm::Round);
        let centre = Paint { colours: round, ..place.clone() };
        assert_eq!(background_gpu(&centre, &Inputs::default(), &mut atlas).mode, 2.0);
        let mut spiral = colours.clone();
        spiral.layout.form = Some(GradientForm::Spiral);
        let other = background_gpu(&Paint { colours: spiral, ..place.clone() }, &Inputs::default(), &mut atlas);
        assert_eq!((other.mode, other.pattern, other.form), (3.0, 15.0, [1.0, 1.0, 0.0, 0.0]), "any other form");

        let time = Paint { follow: Follow::Time, scroll: 0.5, ..place };
        let gpu = background_gpu(&time, &Inputs { time: 1.0, ..Inputs::default() }, &mut atlas);
        assert_eq!((gpu.mode, gpu.scroll, gpu.t0), (0.0, 0.0, 0.5));
    }
}
