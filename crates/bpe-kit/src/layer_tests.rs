//! Things with a Blend other than Normal at full amount are drawn into the frame's layers.

use bpe_core::{Blend, Color, Gradient, Scene, SlotId, ThingId, Value, resolve, resolve_slot};
use bpe_gfx::{Frame, build_frame};

use crate::{Kit, kit, new_scene};

const ARENA: ThingId = ThingId(1);
const BALL: ThingId = ThingId(2);

/// The module filling `slot` on `thing`.
fn module_of(scene: &Scene, kit: &Kit, thing: ThingId, slot: SlotId) -> bpe_core::ModuleId {
    resolve_slot(scene, &kit.registry, thing, slot, &mut Vec::new()).unwrap().module
}

fn set_blend(scene: &mut Scene, module: bpe_core::ModuleId, blend: &str, amount: f64) {
    let params = &mut scene.library.modules.get_mut(&module).unwrap().params;
    params.set("blend", Value::Choice(blend.into()));
    params.set("blend_amount", Value::Number(amount));
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
    assert!(frame.balls.is_empty() && frame.looks.is_empty());
    assert_eq!(frame.layers.len(), 1);
    let layer = &frame.layers[0];
    assert_eq!((layer.blend, layer.amount), (Blend::Multiply, 1.0));
    assert_eq!(layer.frame.balls.len(), 1);
    assert!(!frame.shapes.is_empty(), "the arena keeps its plain look");
}

#[test]
fn normal_at_full_amount_stays_in_the_main_frame() {
    let (scene, kit) = ball_blended("normal", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    assert!(frame.layers.is_empty());
    assert_eq!(frame.balls.len(), 1);
    // Normal at less than full amount fades the whole thing as one, so it is layered too.
    let (scene, kit) = ball_blended("normal", 0.5);
    let frame = frame_at(&scene, &kit, 0.0);
    assert!(frame.balls.is_empty());
    assert_eq!((frame.layers.len(), frame.layers[0].blend, frame.layers[0].amount), (1, Blend::Normal, 0.5));
}

#[test]
fn a_stuck_blended_thing_goes_into_the_overlays_layer() {
    let (mut scene, kit) = ball_blended("screen", 1.0);
    scene.things.get_mut(&BALL).unwrap().screen = Some(true);
    let frame = frame_at(&scene, &kit, 0.0);
    assert!(frame.layers.is_empty());
    let o = frame.overlay.as_deref().unwrap();
    assert!(o.balls.is_empty());
    assert_eq!(o.layers.len(), 1);
    assert_eq!((o.layers[0].blend, o.layers[0].frame.balls.len()), (Blend::Screen, 1));
    assert!(frame.has_overlay());
}

#[test]
fn trail_add_stays_in_the_main_frame_but_trail_screen_uses_a_layer() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let trail = scene.add_module(&kit.registry, "trail.streak", "Streak").unwrap();
    scene.link(&kit.registry, BALL, SlotId::Trail, trail).unwrap();
    let frame = frame_at(&scene, &kit, 2.0);
    assert!(!frame.trails.is_empty(), "Add is the trail's own");
    assert!(frame.layers.is_empty(), "an empty layer is never kept");
    set_blend(&mut scene, trail, "normal", 0.5);
    let frame = frame_at(&scene, &kit, 2.0);
    assert!(!frame.trails.is_empty() && frame.layers.is_empty(), "so is Normal at any amount");

    set_blend(&mut scene, trail, "normal", 1.0);
    let plain = frame_at(&scene, &kit, 2.0);
    set_blend(&mut scene, trail, "screen", 0.3);
    let frame = frame_at(&scene, &kit, 2.0);
    assert!(frame.trails.is_empty());
    assert_eq!(frame.layers.len(), 1);
    assert_eq!((frame.layers[0].blend, frame.layers[0].amount), (Blend::Screen, 0.3));
    // The layer is mixed in at its amount later: in it the streak is plain, Normal at full.
    assert_eq!(frame.layers[0].frame.trails, plain.trails);
}

#[test]
fn two_things_with_the_same_mode_and_amount_share_one_layer() {
    let (mut scene, kit) = ball_blended("difference", 1.0);
    let arena_look = module_of(&scene, &kit, ARENA, SlotId::Look);
    set_blend(&mut scene, arena_look, "difference", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    assert_eq!(frame.layers.len(), 1);
    let layer = &frame.layers[0].frame;
    assert!(!layer.shapes.is_empty() && layer.balls.len() == 1, "the arena and the ball");
    assert!(frame.shapes.is_empty() && frame.balls.is_empty());

    let look = module_of(&scene, &kit, BALL, SlotId::Look);
    set_blend(&mut scene, look, "difference", 0.5);
    let frame = frame_at(&scene, &kit, 0.0);
    let kept: Vec<(Blend, f32)> = frame.layers.iter().map(|l| (l.blend, l.amount)).collect();
    assert_eq!(kept, [(Blend::Difference, 1.0), (Blend::Difference, 0.5)], "the arena drew first");
}

#[test]
fn the_scene_itself_never_goes_into_a_layer() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let plain = frame_at(&scene, &kit, 0.0);
    let arena_look = module_of(&scene, &kit, ARENA, SlotId::Look);
    set_blend(&mut scene, arena_look, "multiply", 0.5);
    let frame = frame_at(&scene, &kit, 0.0);
    assert_eq!((frame.width, frame.height, frame.bloom), (plain.width, plain.height, plain.bloom));
    assert_eq!(frame.background, plain.background);
    assert!(frame.layers.iter().all(|l| l.frame.background == bpe_gfx::BackgroundPaint::default()));
}

#[test]
fn a_layered_inside_pattern_keeps_a_valid_atlas_row() {
    let (mut scene, kit) = ball_blended("normal", 1.0);
    let look = module_of(&scene, &kit, BALL, SlotId::Look);
    let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
    params.set("inside", Value::Choice("stripes".into()));
    params.set("colours", Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)])));
    let plain = frame_at(&scene, &kit, 0.0);
    let want = plain.gradients.rows()[plain.balls[0].paint.row as usize];
    set_blend(&mut scene, look, "overlay", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    let layer = &frame.layers[0].frame;
    let row = layer.balls[0].paint.row;
    assert!(row >= 0.0 && (row as usize) < frame.gradients.len(), "row {row} of {}", frame.gradients.len());
    assert_eq!(frame.gradients.rows()[row as usize], want, "the row the renderer uploads is the ball's");
    assert!(layer.gradients.is_empty());
}

/// The starter scene with a root burst of `blend` at `amount`, its own colours following Life.
fn painted_bursts(blend: &str, amount: f64) -> Frame {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let m = scene.add_module(&kit.registry, "particles.burst", "Sparks").unwrap();
    let params = &mut scene.library.modules.get_mut(&m).unwrap().params;
    params.set("life", Value::Number(2.0));
    params.set("source", Value::Choice("own".into()));
    params.set("follow", Value::Choice("life".into()));
    params.set("colours", Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)])));
    scene.link(&kit.registry, BALL, SlotId::Particles, m).unwrap();
    set_blend(&mut scene, m, blend, amount);
    frame_at(&scene, &kit, 4.0)
}

#[test]
fn bursts_painted_on_the_gpu_carry_the_blend_amount() {
    for (blend, amount) in [("normal", 0.25), ("add", 0.5), ("normal", 1.0)] {
        let frame = painted_bursts(blend, amount);
        assert!(!frame.bursts.is_empty() && frame.layers.is_empty(), "{blend}");
        assert!(frame.bursts.iter().all(|b| b.follow == 1 && b.amount == amount as f32), "{blend} {amount}");
    }
}

#[test]
fn layered_bursts_are_plain_and_still_see_the_hits() {
    let plain = painted_bursts("normal", 1.0);
    let frame = painted_bursts("hue", 0.3);
    assert!(frame.bursts.is_empty());
    assert_eq!((frame.layers.len(), frame.layers[0].blend), (1, Blend::Hue));
    let bursts = &frame.layers[0].frame.bursts;
    assert_eq!(bursts.len(), plain.bursts.len(), "the hits reached the layered module");
    assert!(bursts.iter().all(|b| !b.additive && b.amount == 1.0));
    let row = bursts[0].paint.row as usize;
    assert_eq!(frame.gradients.rows()[row], plain.gradients.rows()[plain.bursts[0].paint.row as usize]);
}

#[test]
fn a_layered_ball_still_hands_its_hits_to_the_frame() {
    let (mut scene, kit) = ball_blended("subtract", 1.0);
    let m = scene.add_module(&kit.registry, "particles.burst", "Sparks").unwrap();
    scene.link(&kit.registry, BALL, SlotId::Particles, m).unwrap();
    let frame = frame_at(&scene, &kit, 4.0);
    assert!(!frame.hits.is_empty());
    assert!(!frame.bursts.is_empty(), "the ball's additive bursts are drawn in the frame");
    assert!(frame.layers[0].frame.hits.is_empty());
}

#[test]
fn a_look_blending_with_each_other_gets_its_own_layer() {
    let (mut scene, kit) = ball_blended("screen", 1.0);
    let arena_look = module_of(&scene, &kit, ARENA, SlotId::Look);
    set_blend(&mut scene, arena_look, "screen", 1.0);
    let look = module_of(&scene, &kit, BALL, SlotId::Look);
    scene.library.modules.get_mut(&look).unwrap().params.set("blend_each", Value::Bool(true));
    let frame = frame_at(&scene, &kit, 0.0);
    let kept: Vec<(Blend, f32, bool)> = frame.layers.iter().map(|l| (l.blend, l.amount, l.each)).collect();
    assert_eq!(kept, [(Blend::Screen, 1.0, false), (Blend::Screen, 1.0, true)], "the arena, then the ball");
    assert_eq!(frame.layers[1].frame.balls.len(), 1);

    // Overlay cannot blend with itself: the switch is ignored and the two share again.
    set_blend(&mut scene, arena_look, "overlay", 1.0);
    set_blend(&mut scene, look, "overlay", 1.0);
    let frame = frame_at(&scene, &kit, 0.0);
    let kept: Vec<(Blend, bool)> = frame.layers.iter().map(|l| (l.blend, l.each)).collect();
    assert_eq!(kept, [(Blend::Overlay, false)]);
}
