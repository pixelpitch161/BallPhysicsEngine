//! Things with a Blend other than Normal at full amount are drawn into the frame's layers.

use bpe_core::{Blend, Color, Gradient, Scene, SlotId, ThingId, Value, resolve};
use bpe_gfx::{Frame, build_frame};

use crate::{Kit, kit, new_scene};

const BALL: ThingId = ThingId(2);

/// The module filling `slot` on `thing`.
fn module_of(_scene: &Scene, _kit: &Kit, thing: ThingId, slot: SlotId) -> (ThingId, SlotId) {
    (thing, slot)
}

fn set_blend(scene: &mut Scene, module: (bpe_core::ThingId, bpe_core::SlotId), blend: &str, amount: f64) {
    if scene.card(module.0, module.1).is_some_and(|c| c.kind == crate::modules::look::layers::CARD) {
        // A layered look mixes through its Blend layer.
        let layer = crate::modules::look::layers::BLEND;
        crate::testing::set_layer(scene, module.0, layer, "blend", Value::Choice(blend.into()));
        crate::testing::set_layer(scene, module.0, layer, "blend_amount", Value::Number(amount));
        return;
    }
    let params = &mut scene.card_mut(module.0, module.1).unwrap().params;
    params.set("blend", Value::Choice(blend.into()));
    params.set("blend_amount", Value::Number(amount));
}

/// A second ball beside the first, with a layered look of its own.
fn second_ball(scene: &mut Scene, kit: &Kit) -> ThingId {
    let id = crate::add_thing(scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().transform.position = glam::Vec2::new(150.0, -200.0);
    id
}

/// `scene` drawn at `time` seconds into its run.
fn frame_at(scene: &Scene, kit: &Kit, time: f64) -> Frame {
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = bpe_runtime::Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame
}

/// The starter scene with the ball's look set to `blend` at `amount`.
fn ball_blended(blend: &str, amount: f64) -> (Scene, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let look = module_of(&scene, &kit, BALL, SlotId::Look);
    set_blend(&mut scene, look, blend, amount);
    (scene, kit)
}

#[test]
fn a_multiply_look_puts_the_ball_in_a_layer() {
    let (scene, kit) = ball_blended("multiply", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    assert!(frame.layered.is_empty());
    assert_eq!(frame.layers.len(), 1);
    let layer = &frame.layers[0];
    assert_eq!((layer.blend, layer.amount), (Blend::Multiply, 1.0));
    assert_eq!(layer.frame.layered.len(), 1);
    assert!(!frame.paths.is_empty(), "the shape keeps its plain look");
}

#[test]
fn normal_at_full_amount_stays_in_the_main_frame() {
    let (scene, kit) = ball_blended("normal", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    assert!(frame.layers.is_empty());
    assert_eq!(frame.layered.len(), 1);
    // Normal at less than full amount fades the whole thing as one, so it is layered too.
    let (scene, kit) = ball_blended("normal", 0.5);
    let frame = frame_at(&scene, &kit, 0.0);
    assert!(frame.layered.is_empty());
    assert_eq!((frame.layers.len(), frame.layers[0].blend, frame.layers[0].amount), (1, Blend::Normal, 0.5));
}

#[test]
fn a_stuck_blended_thing_goes_into_the_overlays_layer() {
    let (mut scene, kit) = ball_blended("screen", 1.0);
    scene.things.get_mut(&BALL).unwrap().screen = Some(true);
    let frame = frame_at(&scene, &kit, 0.0);
    assert!(frame.layers.is_empty());
    let o = frame.overlay.as_deref().unwrap();
    assert!(o.layered.is_empty());
    assert_eq!(o.layers.len(), 1);
    assert_eq!((o.layers[0].blend, o.layers[0].frame.layered.len()), (Blend::Screen, 1));
    assert!(frame.has_overlay());
}

#[test]
fn trail_add_stays_in_the_main_frame_but_trail_screen_uses_a_layer() {
    use crate::modules::look::layers::TRAIL;
    use crate::testing::set_layer;
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let blend = |scene: &mut Scene, mode: &str, amount: f64| {
        set_layer(scene, BALL, TRAIL, "blend", Value::Choice(mode.into()));
        set_layer(scene, BALL, TRAIL, "blend_amount", Value::Number(amount));
    };
    blend(&mut scene, "add", 1.0);
    let frame = frame_at(&scene, &kit, 2.0);
    assert!(!frame.trails.is_empty(), "Add is the trail's own");
    assert!(frame.layers.is_empty(), "an empty layer is never kept");
    blend(&mut scene, "normal", 0.5);
    let frame = frame_at(&scene, &kit, 2.0);
    assert!(!frame.trails.is_empty() && frame.layers.is_empty(), "so is Normal at any amount");

    blend(&mut scene, "normal", 1.0);
    let plain = frame_at(&scene, &kit, 2.0);
    blend(&mut scene, "screen", 0.3);
    let frame = frame_at(&scene, &kit, 2.0);
    assert!(frame.trails.is_empty());
    assert_eq!(frame.layers.len(), 1);
    assert_eq!((frame.layers[0].blend, frame.layers[0].amount), (Blend::Screen, 0.3));
    assert!(frame.layers[0].behind(), "a trail behind the balls is mixed in before them");
    // The layer is mixed in at its amount later: in it the streak is plain, Normal at full.
    assert_eq!(frame.layers[0].frame.trails, plain.trails);
}

#[test]
fn two_things_with_the_same_mode_and_amount_share_one_layer() {
    let (mut scene, kit) = ball_blended("difference", 1.0);
    let other = second_ball(&mut scene, &kit);
    let other_look = module_of(&scene, &kit, other, SlotId::Look);
    set_blend(&mut scene, other_look, "difference", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    assert_eq!(frame.layers.len(), 1);
    let layer = &frame.layers[0].frame;
    assert_eq!(layer.layered.len(), 2, "both balls");
    assert!(frame.layered.is_empty());
    assert_eq!(frame.paths.len(), 1, "the shape stays in the main frame");

    let look = module_of(&scene, &kit, BALL, SlotId::Look);
    set_blend(&mut scene, look, "difference", 0.5);
    let frame = frame_at(&scene, &kit, 0.0);
    let kept: Vec<(Blend, f32)> = frame.layers.iter().map(|l| (l.blend, l.amount)).collect();
    assert_eq!(kept, [(Blend::Difference, 0.5), (Blend::Difference, 1.0)], "the first ball drew first");
}

#[test]
fn the_scene_itself_never_goes_into_a_layer() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let plain = frame_at(&scene, &kit, 0.0);
    let ball_look = module_of(&scene, &kit, BALL, SlotId::Look);
    set_blend(&mut scene, ball_look, "multiply", 0.5);
    let frame = frame_at(&scene, &kit, 0.0);
    assert_eq!((frame.width, frame.height, frame.bloom), (plain.width, plain.height, plain.bloom));
    assert_eq!(frame.background, plain.background);
    assert!(frame.layers.iter().all(|l| l.frame.background == bpe_gfx::BackgroundPaint::default()));
}

#[test]
fn a_layered_inside_pattern_keeps_a_valid_atlas_row() {
    use crate::modules::look::layers::PATTERN;
    use crate::testing::set_layer;
    let (mut scene, kit) = ball_blended("normal", 1.0);
    let look = module_of(&scene, &kit, BALL, SlotId::Look);
    set_layer(&mut scene, BALL, PATTERN, "in_pattern", Value::Choice("gradient".into()));
    set_layer(&mut scene, BALL, PATTERN, "in_colours~form", Value::Choice("stripes".into()));
    set_layer(&mut scene, BALL, PATTERN, "in_from", Value::Choice("own_gradient".into()));
    let colours = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
    set_layer(&mut scene, BALL, PATTERN, "in_colours", Value::Gradient(colours));
    let pattern = |f: &Frame| *f.look_layers.iter().find(|l| l.head[0] == bpe_gfx::LayerGpu::PATTERN).unwrap();
    let plain = frame_at(&scene, &kit, 0.0);
    let want = plain.gradients.rows()[pattern(&plain).paint.row as usize];
    set_blend(&mut scene, look, "overlay", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    let layer = &frame.layers[0].frame;
    let row = pattern(layer).paint.row;
    assert!(row >= 0.0 && (row as usize) < frame.gradients.len(), "row {row} of {}", frame.gradients.len());
    assert_eq!(frame.gradients.rows()[row as usize], want, "the row the renderer uploads is the ball's");
    assert!(layer.gradients.is_empty());
}

/// The starter scene with a particles layer on its ball of `blend` at `amount`, its own colours
/// following Life.
fn painted_particles(blend: &str, amount: f64) -> Frame {
    use crate::modules::look::layers::PARTICLES;
    use crate::testing::set_layer;
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    set_layer(&mut scene, BALL, PARTICLES, "life", Value::Number(2.0));
    set_layer(&mut scene, BALL, PARTICLES, "source", Value::Choice("own".into()));
    set_layer(&mut scene, BALL, PARTICLES, "follow", Value::Choice("life".into()));
    let colours = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
    set_layer(&mut scene, BALL, PARTICLES, "colours", Value::Gradient(colours));
    set_layer(&mut scene, BALL, PARTICLES, "blend", Value::Choice(blend.into()));
    set_layer(&mut scene, BALL, PARTICLES, "blend_amount", Value::Number(amount));
    frame_at(&scene, &kit, 4.0)
}

#[test]
fn particles_carry_the_blend_amount() {
    for (blend, amount) in [("normal", 0.25), ("add", 0.5), ("normal", 1.0)] {
        let frame = painted_particles(blend, amount);
        assert!(!frame.particles.is_empty() && frame.layers.is_empty(), "{blend}");
        let a = amount as f32;
        let ok = |c: f32| if blend == "add" { c < 0.0 && -c <= a + 1e-6 } else { c > 0.0 && c <= a + 1e-6 };
        assert!(frame.particles.iter().all(|p| ok(p.color[3])), "{blend} {amount}");
    }
}

#[test]
fn layered_particles_are_plain() {
    let plain = painted_particles("normal", 1.0);
    let frame = painted_particles("hue", 0.3);
    assert!(frame.particles.is_empty());
    assert_eq!((frame.layers.len(), frame.layers[0].blend), (1, Blend::Hue));
    let inside = &frame.layers[0].frame.particles;
    assert_eq!(inside, &plain.particles, "in the layer they are plain, Normal at full");
}

#[test]
fn a_layered_ball_s_added_particles_draw_in_the_frame() {
    use crate::modules::look::layers::PARTICLES;
    let (mut scene, kit) = ball_blended("subtract", 1.0);
    scene.add_layer(&kit.registry, BALL, SlotId::Look, PARTICLES).unwrap();
    let frame = frame_at(&scene, &kit, 4.0);
    assert!(!frame.particles.is_empty(), "the ball's additive particles are drawn in the frame");
    assert!(frame.layers[0].frame.particles.is_empty());
}

#[test]
fn a_look_blending_with_each_other_gets_its_own_layer() {
    let (mut scene, kit) = ball_blended("screen", 1.0);
    let other = second_ball(&mut scene, &kit);
    let other_look = module_of(&scene, &kit, other, SlotId::Look);
    set_blend(&mut scene, other_look, "screen", 1.0);
    let look = module_of(&scene, &kit, BALL, SlotId::Look);
    crate::testing::set_layer(&mut scene, BALL, crate::modules::look::layers::BLEND, "blend_each", Value::Bool(true));
    let frame = frame_at(&scene, &kit, 0.0);
    let kept: Vec<(Blend, f32, bool)> = frame.layers.iter().map(|l| (l.blend, l.amount, l.each)).collect();
    assert_eq!(kept, [(Blend::Screen, 1.0, true), (Blend::Screen, 1.0, false)], "the first ball, then the other");
    assert_eq!(frame.layers[0].frame.layered.len(), 1);

    // Overlay cannot blend with itself: the switch is ignored and the two share again.
    set_blend(&mut scene, other_look, "overlay", 1.0);
    set_blend(&mut scene, look, "overlay", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    let kept: Vec<(Blend, bool)> = frame.layers.iter().map(|l| (l.blend, l.each)).collect();
    assert_eq!(kept, [(Blend::Overlay, false)]);
}
