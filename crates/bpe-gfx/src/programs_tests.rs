//! Picture tests for Shader layers inside balls (in-ball programs), on the software GPU.

use std::borrow::Cow;

use crate::effects::{Builtin, defaults};
use crate::pictures::{pixel_sized, save};
use crate::{Frame, LayerGpu, LayeredInstance, LayeredRun, LookProgram, PaintGpu, Renderer, headless_device};
use bpe_core::{Blend, Color, Gradient};
use glam::Vec2;

const W: u32 = 540;
const H: u32 = 960;
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

fn ball(x: f32, first: u32, count: u32) -> LayeredInstance {
    LayeredInstance {
        center: [x, 0.0],
        radius: 200.0,
        seed: 0.3,
        body: WHITE,
        stretch: [1.0, 0.0, 1.0, 1.0],
        form: [0.0, 0.15, 0.0, 1.45],
        misc: [1.0, 3.0, 0.0, 0.0],
        vary: crate::NO_VARY,
        range: [first, count, 0, 0],
        group: [0.0; 4],
        place: [0.0; 4],
    }
}

fn body() -> LayerGpu {
    LayerGpu { amount: [1.0, 0.0, 0.0, 0.0], ..LayerGpu::ZERO }
}

/// A shader record running effect slot `slot` with `effect`'s default settings.
fn shader(effect: &str, slot: u32, opacity: f32) -> LayerGpu {
    let packed = defaults(Builtin::find(effect).unwrap().header());
    let mut extra = [[0.0; 4]; 4];
    for (i, v) in packed.iter().enumerate() {
        extra[i / 4][i % 4] = *v;
    }
    LayerGpu {
        head: [LayerGpu::SHADER, Blend::Normal.shader_code(), 0, 0],
        amount: [opacity, 0.0, 0.0, 0.0],
        p0: [1.0, 1.0, 0.0, slot as f32],
        extra,
        ..LayerGpu::ZERO
    }
}

/// A pattern record of `paint` over the layers under it.
fn pattern(frame: &mut Frame, gradient: Gradient, kind: f32, angle: f32) -> LayerGpu {
    let row = frame.gradients.row(&gradient);
    LayerGpu {
        head: [LayerGpu::PATTERN, 0, 0, 0],
        amount: [1.0, 0.0, 0.0, 0.0],
        paint: PaintGpu { row: row as f32, pattern: kind, angle, ..PaintGpu::default() },
        ..LayerGpu::ZERO
    }
}

/// One white ball at the centre wearing `records`, drawn with a program running `sources`.
fn frame_with(records: Vec<LayerGpu>, sources: &[Cow<'static, str>]) -> Frame {
    let mut frame = Frame { bloom: 0.0, time: 1.3, ..Frame::default() };
    frame.layered.push(ball(0.0, 0, records.len() as u32));
    frame.look_layers = records;
    if !sources.is_empty() {
        frame.programs.push(LookProgram::new(sources.to_vec()));
        frame.layered_runs.push(LayeredRun { program: Some(0), start: 0, end: 1 });
    }
    frame
}

fn builtin(id: &str) -> Cow<'static, str> {
    Cow::Borrowed(Builtin::find(id).unwrap().wgsl)
}

/// Draws `frame`, giving the pixels and the renderer's program errors.
/// A frame's pixels and the programs' errors.
type Drawn = Option<(Vec<u8>, Vec<(u64, String)>)>;

fn draw(frame: &Frame) -> Drawn {
    let (device, queue) = headless_device().ok()?;
    let mut r = Renderer::new(&device, W, H);
    r.render(&device, &queue, frame);
    Some((r.read_output(&device, &queue).unwrap(), r.program_errors()))
}

fn at(px: &[u8], x: f32, y: f32) -> [u8; 4] {
    pixel_sized(px, W, H, Vec2::new(x, y))
}

#[test]
fn a_lava_layer_paints_inside_the_ball_only_and_a_pattern_above_it_still_shows() {
    let plain = frame_with(vec![body()], &[]);
    let lava = frame_with(vec![body(), shader("lava", 0, 1.0)], &[builtin("lava")]);
    let Some((a, _)) = draw(&plain) else { return };
    let (b, errors) = draw(&lava).unwrap();
    save("program-lava", &b, W, H);
    assert!(errors.is_empty(), "{errors:?}");
    let changed_inside = [(0.0, 0.0), (80.0, -60.0), (-100.0, 90.0)].iter().any(|&(x, y)| at(&a, x, y) != at(&b, x, y));
    assert!(changed_inside, "lava changes the ball");
    for (x, y) in [(260.0, 0.0), (0.0, 300.0), (-400.0, -400.0)] {
        assert_eq!(at(&a, x, y), at(&b, x, y), "nothing changes outside ({x}, {y})");
    }
    // A flat red pattern above the shader covers it.
    let mut over = frame_with(vec![], &[builtin("lava")]);
    let red = pattern(&mut over, Gradient::solid(Color::rgb(255, 0, 0)), 0.0, 0.0);
    over.look_layers = vec![body(), shader("lava", 0, 1.0), red];
    over.layered[0].range[1] = 3;
    let (c, _) = draw(&over).unwrap();
    let middle = at(&c, 0.0, 0.0);
    assert!(middle[0] > 240 && middle[1] < 10 && middle[2] < 10, "{middle:?}");
}

#[test]
fn a_kaleidoscope_layer_mirrors_the_pattern_under_it() {
    let mut frame = frame_with(vec![], &[builtin("kaleidoscope")]);
    let ramp = pattern(&mut frame, Gradient::even(&[Color::rgb(0, 0, 0), Color::rgb(255, 255, 255)]), 1.0, 30.0);
    let mut k = shader("kaleidoscope", 0, 1.0);
    // No spin, so the mirror holds at any time.
    k.extra[0][1] = 0.0;
    frame.look_layers = vec![body(), ramp, k];
    frame.layered[0].range[1] = 3;
    let Some((px, errors)) = draw(&frame) else { return };
    save("program-kaleidoscope", &px, W, H);
    assert!(errors.is_empty(), "{errors:?}");
    let pairs = [(60.0, 80.0), (120.0, 40.0), (-70.0, 110.0), (30.0, 150.0)];
    for (x, y) in pairs {
        let (top, bottom) = (at(&px, x, -y), at(&px, x, y));
        assert!(top[0].abs_diff(bottom[0]) <= 3, "({x}, ±{y}): {top:?} vs {bottom:?}");
    }
    // Without it the ramp isn't mirrored.
    let mut bare = frame.clone();
    bare.look_layers.pop();
    bare.layered[0].range[1] = 2;
    let (b, _) = draw(&bare).unwrap();
    assert!(pairs.iter().any(|&(x, y)| at(&b, x, -y)[0].abs_diff(at(&b, x, y)[0]) > 20), "the ramp alone is lopsided");
}

#[test]
fn opacity_zero_draws_as_no_layer_and_broken_code_draws_without_it_and_says_why() {
    let plain = frame_with(vec![body()], &[]);
    let hidden = frame_with(vec![body(), shader("lava", 0, 0.0)], &[builtin("lava")]);
    let Some((a, _)) = draw(&plain) else { return };
    let (b, _) = draw(&hidden).unwrap();
    assert!(a == b, "opacity 0 draws nothing");
    let broken: Cow<'static, str> =
        Cow::Borrowed("// name: Broken\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return nope(uv); }\n");
    let mut bad = frame_with(vec![body(), shader("lava", 0, 1.0)], &[broken]);
    bad.look_layers[1].extra = [[0.0; 4]; 4];
    let (c, errors) = draw(&bad).unwrap();
    assert!(a == c, "the broken layer is left out");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].0, bad.programs[0].key);
    assert!(errors[0].1.contains("nope"), "{}", errors[0].1);
}

#[test]
fn two_balls_with_different_effects_draw_with_two_programs_the_same_every_time() {
    let mut frame = Frame { bloom: 0.0, time: 0.7, ..Frame::default() };
    frame.look_layers = vec![body(), shader("lava", 0, 1.0), body(), shader("glitch", 0, 1.0)];
    frame.layered.push(ball(-260.0, 0, 2));
    frame.layered.push(ball(260.0, 2, 2));
    frame.programs = vec![LookProgram::new(vec![builtin("lava")]), LookProgram::new(vec![builtin("glitch")])];
    frame.layered_runs =
        vec![LayeredRun { program: Some(0), start: 0, end: 1 }, LayeredRun { program: Some(1), start: 1, end: 2 }];
    let mut plain = frame.clone();
    plain.programs.clear();
    plain.layered_runs.clear();
    let Some((a, errors)) = draw(&frame) else { return };
    save("program-two", &a, W, H);
    assert!(errors.is_empty(), "{errors:?}");
    let (p, _) = draw(&plain).unwrap();
    assert_ne!(at(&a, -260.0, 0.0), at(&p, -260.0, 0.0), "lava on the left");
    let (b, _) = draw(&frame).unwrap();
    assert!(a == b, "drawing is repeatable");
}

#[test]
fn own_code_problems_are_found_without_a_gpu() {
    assert_eq!(crate::in_ball_problem(Builtin::find("lava").unwrap().wgsl), None);
    let unknown = "// name: Broken\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return nope(uv); }\n";
    assert!(crate::in_ball_problem(unknown).is_some_and(|e| e.contains("nope")));
    let syntax = "// name: Broken\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return vec3<f32>(1.0 }\n";
    assert!(crate::in_ball_problem(syntax).is_some());
    assert!(crate::in_ball_problem("no header at all").is_some());
}

/// A group shader record running effect slot `slot` with `effect`'s default settings at `amount`:
/// over the whole ball when `whole`, reaching `around` radii past its edge.
fn group_shader(effect: &str, slot: u32, amount: f32, whole: bool, around: f32) -> LayerGpu {
    let s = shader(effect, slot, amount);
    LayerGpu {
        head: [LayerGpu::GROUP_SHADER, Blend::Normal.shader_code(), 0, 0],
        p0: [1.0, 1.0, if whole { 1.0 } else { 0.0 }, slot as f32],
        p1: [around, 0.0, 0.0, 0.0],
        ..s
    }
}

/// Two white balls of radius 150 at y = ∓200, each wearing `records`, as one group 700 across.
fn pair_with(records: Vec<LayerGpu>, sources: &[Cow<'static, str>]) -> Frame {
    let mut frame = Frame { bloom: 0.0, time: 0.4, ..Frame::default() };
    let n = records.len() as u32;
    frame.look_layers = records.iter().chain(&records).copied().collect();
    for (i, y) in [-200.0f32, 200.0].into_iter().enumerate() {
        let mut b = ball(0.0, i as u32 * n, n);
        b.center = [0.0, y];
        b.radius = 150.0;
        b.place = [0.0, y, 700.0, 0.0];
        frame.layered.push(b);
    }
    frame.programs.push(LookProgram::new(sources.to_vec()));
    frame.layered_runs.push(LayeredRun { program: Some(0), start: 0, end: 2 });
    frame
}

#[test]
fn a_group_shader_spreads_one_effect_across_the_balls_where_a_ball_shader_repeats_on_each() {
    let flow = [builtin("rainbow_flow")];
    let group = pair_with(vec![body(), group_shader("rainbow_flow", 0, 1.0, true, 0.0)], &flow);
    let each = pair_with(vec![body(), shader("rainbow_flow", 0, 1.0)], &flow);
    let Some((g, errors)) = draw(&group) else { return };
    save("program-group-shader", &g, W, H);
    assert!(errors.is_empty(), "{errors:?}");
    let (e, _) = draw(&each).unwrap();
    let far = |a: [u8; 4], b: [u8; 4]| (0..3).map(|i| a[i].abs_diff(b[i]) as u32).sum::<u32>();
    let (top, bottom) = (at(&g, 0.0, -200.0), at(&g, 0.0, 200.0));
    assert!(far(top, bottom) > 60, "one effect across the group: {top:?} vs {bottom:?}");
    let (top, bottom) = (at(&e, 0.0, -200.0), at(&e, 0.0, 200.0));
    assert!(far(top, bottom) <= 3, "the same on each ball: {top:?} vs {bottom:?}");
}

#[test]
fn a_group_shader_at_amount_zero_changes_nothing_and_around_reaches_past_the_edge() {
    let flow = [builtin("rainbow_flow")];
    let plain = pair_with(vec![body()], &[]);
    let none = pair_with(vec![body(), group_shader("rainbow_flow", 0, 0.0, true, 1.0)], &flow);
    let Some((a, _)) = draw(&plain) else { return };
    let (b, errors) = draw(&none).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(a == b, "amount 0 changes nothing");
    let inside = pair_with(vec![body(), group_shader("rainbow_flow", 0, 1.0, true, 0.0)], &flow);
    let mut around = pair_with(vec![body(), group_shader("rainbow_flow", 0, 1.0, true, 1.0)], &flow);
    for b in &mut around.layered {
        b.form[3] = 2.6;
    }
    let (i, _) = draw(&inside).unwrap();
    let (o, _) = draw(&around).unwrap();
    save("program-group-around", &o, W, H);
    // Just past the top ball's edge, to its side.
    let past = (175.0, -200.0);
    assert_eq!(at(&i, past.0, past.1), at(&a, past.0, past.1), "inside only stays in the ball");
    assert_ne!(at(&o, past.0, past.1), at(&a, past.0, past.1), "around shows past the edge");
}

#[test]
fn a_group_shader_can_change_one_layer_only() {
    let flow = [builtin("rainbow_flow")];
    let plain = pair_with(vec![body()], &[]);
    // The body names the group record after it (index 1, bit 0).
    let mut b = body();
    b.amount[2] = 1.0;
    b.amount[3] = 1.0;
    let shaded = pair_with(vec![b, group_shader("rainbow_flow", 0, 1.0, false, 0.0)], &flow);
    let unnamed = pair_with(vec![body(), group_shader("rainbow_flow", 0, 1.0, false, 0.0)], &flow);
    let Some((a, _)) = draw(&plain) else { return };
    let (s, errors) = draw(&shaded).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let (u, _) = draw(&unnamed).unwrap();
    assert_ne!(at(&s, 0.0, -200.0), at(&a, 0.0, -200.0), "the body takes the effect");
    assert!(u == a, "a group shader no layer names, not over the whole ball, does nothing");
}

#[test]
fn a_group_shader_covering_half_the_group_from_the_top_reaches_only_the_top_ball() {
    let flow = [builtin("rainbow_flow")];
    let plain = pair_with(vec![body()], &[]);
    // Half the group left uncovered, from the top (90°), with a hard edge.
    let mut half = group_shader("rainbow_flow", 0, 1.0, true, 0.0);
    half.p1 = [0.0, 0.5, 90f32.to_radians(), 0.0];
    let covered = pair_with(vec![body(), half], &flow);
    let Some((a, _)) = draw(&plain) else { return };
    let (b, errors) = draw(&covered).unwrap();
    save("program-group-covers-half", &b, W, H);
    assert!(errors.is_empty(), "{errors:?}");
    assert_ne!(at(&b, 0.0, -200.0), at(&a, 0.0, -200.0), "the top ball is covered");
    assert_eq!(at(&b, 0.0, 200.0), at(&a, 0.0, 200.0), "the bottom ball isn't");
}

#[test]
fn programs_built_in_the_background_draw_without_the_effect_until_ready_and_are_shared() {
    let plain = frame_with(vec![body()], &[]);
    let lava = frame_with(vec![body(), shader("lava", 0, 1.0)], &[builtin("lava")]);
    let Ok((device, queue)) = headless_device() else { return };
    let store = crate::ProgramStore::in_background();
    let mut r = Renderer::new(&device, W, H);
    r.share_programs(&store);
    r.render(&device, &queue, &plain);
    let without = r.read_output(&device, &queue).unwrap();
    r.render(&device, &queue, &lava);
    let first = r.read_output(&device, &queue).unwrap();
    if first == without {
        assert!(r.programs_waiting(), "a frame without its effect says so, to be drawn again");
    }
    // The first frame may only miss the effect: it never stalls to build it.
    store.wait();
    assert!(!store.pending());
    r.render(&device, &queue, &lava);
    let ready = r.read_output(&device, &queue).unwrap();
    assert!(!r.programs_waiting());
    assert!(first == without || first == ready, "a frame draws with or without the effect, nothing else");
    assert!(ready != without, "once built, the effect shows");
    assert!(r.program_errors().is_empty(), "{:?}", r.program_errors());
    // Another renderer on the store draws it straight away, with no build of its own.
    let mut other = Renderer::new(&device, W, H);
    other.share_programs(&store);
    other.render(&device, &queue, &lava);
    assert!(!store.pending());
    assert!(other.read_output(&device, &queue).unwrap() == ready);
}
