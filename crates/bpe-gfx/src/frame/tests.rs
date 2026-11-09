use super::*;
use bpe_core::{Address, Ease, Value, resolve, testkit};

/// Whether `p` lies inside one of the strip's triangles.
fn strip_covers(out: &[TrailVertex], p: Vec2) -> bool {
    out.chunks(3).any(|t| {
        let [a, b, c] = [t[0].position, t[1].position, t[2].position].map(Vec2::from);
        let side = |u: Vec2, v: Vec2| (v - u).perp_dot(p - u);
        let (x, y, z) = (side(a, b), side(b, c), side(c, a));
        (x >= -1e-3 && y >= -1e-3 && z >= -1e-3) || (x <= 1e-3 && y <= 1e-3 && z <= 1e-3)
    })
}

#[test]
fn a_strip_s_sharp_corners_are_round_outside_and_straight_runs_get_nothing_more() {
    let style = StreakStyle::default();
    // Straight: two triangles a segment, nothing else.
    let mut straight = Vec::new();
    let line = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0)];
    strip(&mut straight, &line, |_| 10.0, |_| [1.0; 4], style, 1.0);
    assert_eq!(straight.len(), 12);
    // A bounce: down to the floor at (100, 100) and back up. Outside the corner, straight below
    // it, the strip used to leave a square notch.
    let mut bounce = Vec::new();
    let v = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Vec2::new(200.0, 0.0)];
    strip(&mut bounce, &v, |_| 10.0, |_| [1.0; 4], style, 1.0);
    assert!(bounce.len() > 12, "the corner gets a join");
    assert!(strip_covers(&bounce, Vec2::new(100.0, 109.0)), "the outside of the corner is filled");
    assert!(!strip_covers(&bounce, Vec2::new(100.0, 115.0)), "but no further than the strip reaches");
    // Every triangle is finite.
    assert!(bounce.iter().all(|v| v.position.iter().chain(&v.edge).all(|x| x.is_finite())));
    // Straight back the way it came (a ball rolling into a wall): the end is still rounded,
    // whichever way the sign of zero falls.
    for back in [Vec2::new(50.0, 0.0), Vec2::new(-50.0, 0.0)] {
        let mut reverse = Vec::new();
        let (corner, way) = (Vec2::new(100.0, 0.0) * back.x.signum(), back.x.signum());
        strip(&mut reverse, &[Vec2::ZERO, corner, back], |_| 10.0, |_| [1.0; 4], style, 1.0);
        assert!(strip_covers(&reverse, corner + Vec2::new(9.0 * way, 0.0)), "the far end is capped");
        assert!(strip_covers(&reverse, corner + Vec2::new(6.0 * way, 6.0)), "round, both sides");
        assert!(strip_covers(&reverse, corner + Vec2::new(6.0 * way, -6.0)), "round, both sides");
    }
}

#[test]
fn free_glass_is_bending_sprites_and_glass_liquid() {
    let mut frame = Frame::default();
    assert_eq!(frame.free_glass_box(), None);
    let mut bubble = Particle::new(0, Vec2::new(100.0, 50.0), 0.0, 10.0, [1.0; 4]);
    frame.particles.push(bubble);
    assert_eq!(frame.free_glass_box(), None, "a sprite that doesn't bend");
    bubble.light[2] = 1.33;
    frame.particles.push(bubble);
    let one = frame.free_glass_box().expect("a bending sprite");
    assert_eq!(one.index, FREE_BENDER);
    assert!(one.lo[0] <= 85.0 && one.hi[0] >= 115.0 && one.lo[1] <= 35.0 && one.hi[1] >= 65.0, "{one:?}");
    let blob = crate::liquid::LiquidBlob { place: [-200.0, 0.0, 20.0, 1.0], colour: [1.0; 4] };
    let mut style = crate::liquid::LiquidStyle { a: [0.5, 1.0, 1.0, 4.0], ..Default::default() };
    frame.liquids.push(crate::liquid::Liquid { blobs: vec![blob], style, front: false });
    assert_eq!(frame.free_glass_box(), Some(one), "a glass liquid with Bends light 1");
    style.c[2] = 1.33;
    frame.liquids.push(crate::liquid::Liquid { blobs: vec![blob], style, front: false });
    let both = frame.free_glass_box().unwrap();
    assert!(both.lo[0] <= -240.0 && both.hi[0] >= 115.0, "{both:?}");
    // Inside a blend layer's own frame too.
    let mut outer = Frame::default();
    outer.layers.push(BlendLayer { blend: Blend::Add, amount: 1.0, each: false, z: 0, place: Place::Blend, frame });
    assert_eq!(outer.free_glass_box(), Some(both));
}
use bpe_runtime::{Event, EventKind, Log, Party, Sims, World};

struct FakeScene;
impl ThingDraw for FakeScene {
    fn kind(&self) -> &'static str {
        "scene"
    }
    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        frame.width = 100;
        frame.height = 200;
        frame.time = cx.time as f32;
        frame.bloom = 0.5;
    }
}

struct FakeBalls;
impl ThingDraw for FakeBalls {
    fn kind(&self) -> &'static str {
        "balls"
    }
    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let radius = match cx.resolved.params.get("radius") {
            Some(Value::Number(r)) => *r as f32,
            _ => 0.0,
        };
        frame.layered.push(LayeredInstance {
            center: [1.0, 2.0],
            radius,
            body: [1.0; 4],
            ..LayeredInstance::default()
        });
        frame.flashes.push(FlashInstance { center: [0.0; 2], size: 1.0, color: [1.0; 4] });
    }
}

#[test]
fn oversized_lists_are_cut_to_whole_triangles_that_fit() {
    let mut f = Frame { trails: vec![TrailVertex::default(); 107], ..Frame::default() };
    f.particles = vec![Particle::new(0, Vec2::ZERO, 0.0, 1.0, [1.0; 4]); 50];
    let mut inner = Frame { trails_front: vec![TrailVertex::default(); 107], ..Frame::default() };
    inner.particles_front = f.particles.clone();
    f.overlay = Some(Box::new(inner));
    f.fit_lists_to(100, 40);
    assert_eq!((f.trails.len(), f.particles.len()), (96, 40));
    let o = f.overlay.as_ref().unwrap();
    assert_eq!((o.trails_front.len(), o.particles_front.len()), (96, 40));
}

#[test]
fn build_frame_fills_from_each_thing_and_starts_clean() {
    let reg = testkit::registry();
    let mut scene = bpe_core::Scene::empty(&reg);
    let balls = testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
    scene.things.get_mut(&balls).unwrap().params.set("radius", Value::Number(9.0));
    testkit::push(&mut scene, "folder", bpe_core::ThingId::ROOT);
    let resolved = resolve(&scene, &reg);
    let sims = Sims::default();
    let world = World::build(&scene, &resolved, &sims);
    let mut draws = Draws::default();
    draws.add(FakeScene);
    draws.add(FakeBalls);
    let mut frame = Frame::default();
    for _ in 0..2 {
        build_frame(&scene, &resolved, &world, &Log::default(), 1.5, &draws, &mut frame);
    }
    assert_eq!((frame.width, frame.height, frame.time, frame.bloom), (100, 200, 1.5, 0.5));
    assert_eq!(frame.layered.len(), 1, "lists are cleared between frames");
    assert_eq!(frame.layered[0].radius, 9.0);
    assert_eq!(frame.flashes.len(), 1);
    assert!(frame.paths.is_empty());
    assert!(draws.thing("folder").is_none());
}

#[test]
fn a_keyed_setting_changes_the_frame_at_its_time() {
    let reg = testkit::registry();
    let mut scene = bpe_core::Scene::empty(&reg);
    let balls = testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
    let size = Address { thing: balls, slot: None, key: "radius".into() };
    let first = scene.set_key(&reg, &size, 0.0, Value::Number(2.0)).unwrap();
    scene.set_key(&reg, &size, 2.0, Value::Number(10.0)).unwrap();
    scene.set_ease(&size, first, Ease::Linear);
    let resolved = resolve(&scene, &reg);
    let world = World::build(&scene, &resolved, &Sims::default());
    let mut draws = Draws::default();
    draws.add(FakeBalls);
    let mut frame = Frame::default();
    let radius_at = |time: f64, frame: &mut Frame| {
        build_frame(&scene, &resolved, &world, &Log::default(), time, &draws, frame);
        frame.layered[0].radius
    };
    assert_eq!(radius_at(0.0, &mut frame), 2.0);
    assert!((radius_at(1.0, &mut frame) - 6.0).abs() < 1e-5);
    assert_eq!(radius_at(5.0, &mut frame), 10.0);
    assert_eq!(resolved.things[&balls].params.get("radius"), Some(&Value::Number(6.0)), "the input is untouched");
}

#[test]
fn layer_records_and_layered_instances_fit_the_gpu() {
    assert_eq!(std::mem::size_of::<LayerGpu>() % 16, 0);
    assert_eq!(std::mem::size_of::<LayerGpu>(), 288, "as layers.wgsl lays it out");
    assert!(std::mem::size_of::<LayeredInstance>() <= 176);
    assert_eq!(std::mem::size_of::<LayeredInstance>() % 16, 0);
    let mut f = Frame::default();
    f.layered.push(LayeredInstance { misc: [1.0, 7.25, 0.0, 0.0], ..Default::default() });
    f.look_layers.push(LayerGpu::default());
    assert!(f.draws_thing(7.0) && !f.draws_nothing());
    f.clear();
    assert!(f.layered.is_empty() && f.look_layers.is_empty());
}

struct OwnBlend;
impl ThingDraw for OwnBlend {
    fn kind(&self) -> &'static str {
        "balls"
    }
    fn draw(&self, _cx: &DrawCx, frame: &mut Frame) {
        frame.layered.push(LayeredInstance::default());
    }
    fn blend(&self, _r: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        Some((Blend::Screen, 0.5, false))
    }
}

#[test]
fn a_thing_that_names_its_own_blend_is_drawn_into_that_layer() {
    let reg = testkit::registry();
    let mut scene = bpe_core::Scene::empty(&reg);
    testkit::push(&mut scene, "balls", bpe_core::ThingId::ROOT);
    let resolved = resolve(&scene, &reg);
    let world = World::build(&scene, &resolved, &Sims::default());
    let mut draws = Draws::default();
    draws.add(OwnBlend);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
    assert!(frame.layered.is_empty());
    assert_eq!(frame.layers.len(), 1);
    assert_eq!((frame.layers[0].blend, frame.layers[0].amount), (Blend::Screen, 0.5));
    assert_eq!(frame.layers[0].frame.layered.len(), 1);
}

#[test]
fn hit_strength_decays_and_saturates() {
    let hit = |step: u64, speed: f32| Event {
        step,
        kind: EventKind::Hit,
        a: Party { thing: bpe_core::ThingId(2), item: 0 },
        b: None,
        point: glam::Vec2::ZERO,
        normal: glam::Vec2::X,
        speed,
        at: 0.0,
    };
    let mut log = Log::default();
    log.push(hit(240, 600.0));
    assert_eq!(hit_strength(&log, 0.5, 240), 0.0, "a hit in the future does not count");
    assert!((hit_strength(&log, 1.0, 240) - 1.0).abs() < 1e-6);
    assert!((hit_strength(&log, 1.1, 240) - (-1.0f32).exp()).abs() < 1e-4);
    assert_eq!(hit_strength(&log, 1.5, 240), 0.0, "hits older than 0.3 s are gone");
    let mut many = Log::default();
    for _ in 0..3 {
        many.push(hit(240, 300.0));
    }
    assert_eq!(hit_strength(&many, 1.0, 240), 1.0, "three half hits saturate");
    assert!(hit_strength(&Log::default(), 1.0, 240) == 0.0);
}

#[test]
fn module_draws_run_after_thing_draws_for_every_slot_they_fill() {
    struct Marker;
    impl ModuleDraw for Marker {
        fn kind(&self) -> &'static str {
            "look.plain"
        }
        fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame) {
            assert_eq!(slot, SlotId::Look);
            assert_eq!(frame.layered.len(), 1, "the things drew first");
            frame.flashes.push(FlashInstance { center: [cx.thing.id.0 as f32, 0.0], ..Default::default() });
        }
    }
    let reg = testkit::registry();
    let mut scene = bpe_core::Scene::empty(&reg);
    scene.add_slot(&reg, ThingId::ROOT, SlotId::Look, "look.plain").unwrap();
    let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
    scene.add_slot(&reg, balls, SlotId::Look, "look.plain").unwrap();
    let resolved = resolve(&scene, &reg);
    let world = World::build(&scene, &resolved, &Sims::default());
    let mut draws = Draws::default();
    draws.add(FakeScene);
    draws.add(FakeBalls);
    draws.add_module(Marker);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
    // The balls' own flash first, then the marker's for the root and for the balls.
    let centres: Vec<f32> = frame.flashes.iter().map(|f| f.center[0]).collect();
    assert_eq!(centres, vec![0.0, 0.0, balls.0 as f32]);
    assert!(draws.module("look.plain").is_some() && draws.module("look.other").is_none());
    frame.clear();
    assert!(frame.flashes.is_empty() && frame.effects.is_empty() && frame.hits.is_empty());
}

#[test]
fn layer_mut_keeps_normal_at_full_amount_in_the_frame_and_groups_the_rest_by_mode_and_amount() {
    use bpe_core::Blend;
    let mut frame = Frame { width: 100, height: 200, time: 1.5, video: (300, 400), ..Frame::default() };
    frame.camera.zoom = 2.0;
    frame.bloom = 0.25;
    frame.layer_mut(Blend::Normal, 1.0, false).layered.push(LayeredInstance::default());
    assert_eq!(frame.layered.len(), 1, "Normal at full amount is the frame itself");
    assert!(frame.layers.is_empty());
    frame.layer_mut(Blend::Multiply, 1.0, false).layered.push(LayeredInstance::default());
    frame.layer_mut(Blend::Screen, 0.5, false).paths.push(PathInstance::default());
    frame.layer_mut(Blend::Multiply, 1.0, false).layered.push(LayeredInstance::default());
    frame.layer_mut(Blend::Normal, 0.5, false).paths.push(PathInstance::default());
    let kept: Vec<(Blend, f32)> = frame.layers.iter().map(|l| (l.blend, l.amount)).collect();
    assert_eq!(kept, [(Blend::Multiply, 1.0), (Blend::Screen, 0.5), (Blend::Normal, 0.5)], "first-use order");
    let l = &frame.layers[0].frame;
    assert_eq!(l.layered.len(), 2, "the same mode and amount share a layer");
    assert_eq!((l.width, l.height, l.time, l.video), (100, 200, 1.5, (300, 400)));
    assert_eq!((l.camera, l.bloom), (frame.camera, 0.25));
    frame.clear();
    assert!(frame.layers.is_empty(), "clear empties the layers");
}

#[test]
fn each_layers_and_plain_layers_do_not_share() {
    use bpe_core::Blend;
    let mut frame = Frame::default();
    frame.layer_mut(Blend::Screen, 1.0, true).layered.push(LayeredInstance::default());
    frame.layer_mut(Blend::Screen, 1.0, false).layered.push(LayeredInstance::default());
    frame.layer_mut(Blend::Screen, 1.0, true).layered.push(LayeredInstance::default());
    let kept: Vec<(Blend, f32, bool)> = frame.layers.iter().map(|l| (l.blend, l.amount, l.each)).collect();
    assert_eq!(kept, [(Blend::Screen, 1.0, true), (Blend::Screen, 1.0, false)]);
    assert_eq!(frame.layers[0].frame.layered.len(), 2);
    // A mode that cannot combine with itself drops the switch, and so shares.
    frame.layer_mut(Blend::Overlay, 1.0, true).layered.push(LayeredInstance::default());
    frame.layer_mut(Blend::Overlay, 1.0, false).layered.push(LayeredInstance::default());
    assert_eq!((frame.layers.len(), frame.layers[2].each, frame.layers[2].frame.layered.len()), (3, false, 2));
    frame.layer_mut(Blend::Normal, 1.0, true).layered.push(LayeredInstance::default());
    assert_eq!((frame.layers.len(), frame.layered.len()), (3, 1), "Normal at full amount is the frame itself");
}

#[test]
fn a_layer_counts_towards_the_overlay_and_is_culled_with_the_frame() {
    use bpe_core::Blend;
    let mut frame = Frame::default();
    frame.overlay_mut();
    assert!(!frame.has_overlay());
    frame.overlay_mut().layer_mut(Blend::Screen, 1.0, false).layered.push(LayeredInstance::default());
    assert!(frame.has_overlay(), "a stuck blended thing is something to draw");
    frame.camera.zoom = 4.0;
    let layer = frame.layer_mut(Blend::Multiply, 1.0, false);
    layer.layered.push(LayeredInstance { center: [0.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
    layer.layered.push(LayeredInstance { center: [5000.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
    frame.cull();
    assert_eq!(frame.layers[0].frame.layered.len(), 1, "what the camera can't see is dropped in layers too");
}

#[test]
fn culling_layered_balls_keeps_each_with_its_own_run() {
    let mut frame = Frame::default();
    frame.camera.zoom = 4.0;
    let at = |x: f32| LayeredInstance { center: [x, 0.0], radius: 5.0, ..LayeredInstance::default() };
    // A plain thing whose ball leaves the shot, then a thing with a program: one seen, one not.
    frame.layered = vec![at(5000.0), at(0.0), at(10.0), at(-6000.0)];
    frame.layered_runs = vec![
        crate::LayeredRun { program: None, start: 0, end: 1 },
        crate::LayeredRun { program: Some(3), start: 1, end: 4 },
    ];
    frame.cull();
    assert_eq!(frame.layered.len(), 2);
    assert_eq!(frame.layered_runs, [crate::LayeredRun { program: Some(3), start: 0, end: 2 }]);
}

#[test]
fn a_layer_is_culled_through_the_frames_camera_after_the_view_changes() {
    use bpe_core::Blend;
    // Built through a camera zoomed in on the middle of the video...
    let mut frame = Frame::default();
    frame.camera.zoom = 4.0;
    let layer = frame.layer_mut(Blend::Screen, 1.0, false);
    layer.layered.push(LayeredInstance { center: [2000.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
    layer.layered.push(LayeredInstance { center: [90_000.0, 0.0], radius: 5.0, ..LayeredInstance::default() });
    // ...then looked at by the editor's World view, zoomed far out, on a wider picture.
    frame.camera = CameraView { center: Vec2::ZERO, zoom: 0.1, turn: 0.0 };
    (frame.width, frame.height) = (2000, 1000);
    frame.cull();
    let kept: Vec<[f32; 2]> = frame.layers[0].frame.layered.iter().map(|b| b.center).collect();
    assert_eq!(kept, [[2000.0, 0.0]], "the layer is culled through the frame's own view");
}

#[test]
fn minus_zero_and_zero_amounts_share_a_layer() {
    use bpe_core::Blend;
    let mut frame = Frame::default();
    frame.layer_mut(Blend::Screen, -0.0, false).layered.push(LayeredInstance::default());
    frame.layer_mut(Blend::Screen, 0.0, false).layered.push(LayeredInstance::default());
    assert_eq!(frame.layers.len(), 1);
    assert_eq!(frame.layers[0].amount.to_bits(), 0.0f32.to_bits());
}

#[test]
fn set_view_moves_the_layers_with_the_frame() {
    use bpe_core::Blend;
    let mut frame = Frame::default();
    frame.layer_mut(Blend::Screen, 1.0, false).layered.push(LayeredInstance::default());
    frame.overlay_mut().layer_mut(Blend::Add, 0.5, false).layered.push(LayeredInstance::default());
    let camera = CameraView { center: Vec2::new(10.0, 20.0), zoom: 0.5, turn: 0.25 };
    frame.set_view(camera, (300, 200));
    let l = &frame.layers[0].frame;
    assert_eq!((l.camera, l.width, l.height), (camera, 300, 200));
    let o = frame.overlay.as_deref().unwrap();
    assert_eq!((o.width, o.height, o.camera), (300, 200, CameraView::default()), "stuck things keep no camera");
    let ol = &o.layers[0].frame;
    assert_eq!((ol.width, ol.height, ol.camera), (300, 200, CameraView::default()));
}

struct Painter;
impl ThingDraw for Painter {
    fn kind(&self) -> &'static str {
        "balls"
    }
    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let red = bpe_core::Gradient::solid(bpe_core::Color::rgb(255, 0, 0));
        let row = frame.gradients.row(&red);
        let paint = PaintGpu { row: row as f32, ..PaintGpu::default() };
        frame.look_layers.push(LayerGpu { paint, ..LayerGpu::ZERO });
        frame.layered.push(LayeredInstance { range: [0, 1, 0, 0], ..LayeredInstance::default() });
        frame.hits.push(HitMark {
            thing: cx.thing.id,
            other: None,
            point: [0.0; 2],
            normal: [0.0, 1.0],
            time: 0.0,
            speed: 1.0,
            color: [1.0; 4],
        });
    }
}

/// A module that marks each thing it draws for with a trail vertex, and says whether it saw
/// the thing's hit.
struct Sees(bool);
impl ModuleDraw for Sees {
    fn kind(&self) -> &'static str {
        "look.plain"
    }
    fn draws_add_itself(&self) -> bool {
        self.0
    }
    fn draw(&self, cx: &DrawCx, _slot: SlotId, frame: &mut Frame) {
        let seen = frame.hits.iter().any(|h| h.thing == cx.thing.id);
        frame.trails.push(TrailVertex { color: [seen as u32 as f32; 4], ..TrailVertex::default() });
    }
}

/// One ball group with a look slot whose blend and amount are set straight on the resolved
/// scene, drawn by [`Painter`] and the look's module by [`Sees`].
fn routed(blend: &str, amount: f64, screen: bool, adds_itself: bool) -> Frame {
    routed_each(blend, amount, screen, adds_itself, false)
}

/// [`routed`], with the look's Blend with each other switch set to `each`.
fn routed_each(blend: &str, amount: f64, screen: bool, adds_itself: bool, each: bool) -> Frame {
    let reg = testkit::registry();
    let mut scene = bpe_core::Scene::empty(&reg);
    let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
    scene.add_slot(&reg, balls, SlotId::Look, "look.plain").unwrap();

    let mut resolved = resolve(&scene, &reg);
    let r = std::sync::Arc::make_mut(resolved.things.get_mut(&balls).unwrap());
    r.screen = screen;
    let params = &mut r.slots.get_mut(&SlotId::Look).unwrap().params;
    params.set("blend", Value::Choice(blend.into()));
    params.set("blend_amount", Value::Number(amount));
    if each {
        params.set("blend_each", Value::Bool(true));
    }
    let world = World::build(&scene, &resolved, &Sims::default());
    let mut draws = Draws::default();
    draws.add(Painter);
    draws.add_module(Sees(adds_itself));
    let mut frame = Frame::default();
    frame.gradients.row(&bpe_core::Gradient::solid(bpe_core::Color::rgb(0, 255, 0)));
    build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
    frame
}

#[test]
fn a_blended_thing_draws_into_a_layer_that_paints_from_the_frames_atlas() {
    use bpe_core::{Blend, Color};
    let plain = routed("normal", 1.0, false, false);
    assert!(plain.layers.is_empty());
    assert_eq!((plain.layered.len(), plain.trails.len()), (1, 1));

    let frame = routed("multiply", 1.0, false, false);
    assert!(frame.layered.is_empty() && frame.trails.is_empty());
    assert_eq!(frame.layers.len(), 1, "the thing and its module share the layer");
    let layer = &frame.layers[0];
    assert_eq!((layer.blend, layer.amount), (Blend::Multiply, 1.0));
    assert_eq!((layer.frame.layered.len(), layer.frame.trails.len()), (1, 1));
    assert!(layer.frame.gradients.is_empty(), "a layer keeps no atlas of its own");
    let row = layer.frame.look_layers[0].paint.row as usize;
    assert_eq!(frame.gradients.rows()[row][0], Color::rgb(255, 0, 0).to_linear(), "its row is in the frame's atlas");
    assert_eq!(frame.hits.len(), 1, "the hit is the frame's, for every module to read");
    assert!(frame.layers[0].frame.hits.is_empty());
    assert_eq!(layer.frame.trails[0].color[0], 1.0, "a layered module still sees the thing's hits");
}

#[test]
fn a_thing_blending_with_each_other_draws_with_its_look_into_an_each_layer() {
    use bpe_core::Blend;
    let frame = routed_each("multiply", 1.0, false, false, true);
    assert_eq!(frame.layers.len(), 1, "the thing and its look's module share the layer");
    let layer = &frame.layers[0];
    assert_eq!((layer.blend, layer.amount, layer.each), (Blend::Multiply, 1.0, true));
    assert_eq!((layer.frame.layered.len(), layer.frame.trails.len()), (1, 1));
    assert!(!routed("multiply", 1.0, false, false).layers[0].each, "off by default");
}

#[test]
fn a_module_that_adds_itself_keeps_normal_and_add_in_the_frame() {
    use bpe_core::Blend;
    for (blend, amount) in [("normal", 0.5), ("add", 0.25), ("add", 1.0)] {
        let frame = routed(blend, amount, false, true);
        assert_eq!(frame.trails.len(), 1, "{blend} {amount}: the module draws in the frame");
        assert_eq!(frame.layers.len(), 1, "{blend} {amount}: the thing itself is layered");
        assert!(frame.layers[0].frame.trails.is_empty());
    }
    let frame = routed("screen", 1.0, false, true);
    assert!(frame.trails.is_empty());
    assert_eq!((frame.layers.len(), frame.layers[0].blend), (1, Blend::Screen));
    assert_eq!(frame.layers[0].frame.trails.len(), 1);
}

#[test]
fn a_stuck_blended_thing_draws_into_the_overlays_layer() {
    let frame = routed("screen", 0.5, true, false);
    assert!(frame.layers.is_empty());
    let o = frame.overlay.as_deref().unwrap();
    assert!(o.layered.is_empty());
    assert_eq!((o.layers.len(), o.layers[0].frame.layered.len()), (1, 1));
    assert!(frame.has_overlay());
    let row = o.layers[0].frame.look_layers[0].paint.row as usize;
    assert!(row < o.gradients.len(), "its row is in the overlay's atlas");
}

#[test]
fn a_layer_nothing_was_drawn_into_is_dropped() {
    struct Nothing;
    impl ThingDraw for Nothing {
        fn kind(&self) -> &'static str {
            "balls"
        }
        fn draw(&self, _: &DrawCx, frame: &mut Frame) {
            frame.gradients.row(&bpe_core::Gradient::solid(bpe_core::Color::rgb(1, 2, 3)));
        }
    }
    let reg = testkit::registry();
    let mut scene = bpe_core::Scene::empty(&reg);
    let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
    scene.add_slot(&reg, balls, SlotId::Look, "look.plain").unwrap();

    let mut resolved = resolve(&scene, &reg);
    let params = &mut std::sync::Arc::make_mut(resolved.things.get_mut(&balls).unwrap())
        .slots
        .get_mut(&SlotId::Look)
        .unwrap()
        .params;
    params.set("blend", Value::Choice("darken".into()));
    let world = World::build(&scene, &resolved, &Sims::default());
    let mut draws = Draws::default();
    draws.add(Nothing);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &draws, &mut frame);
    assert!(frame.layers.is_empty());
    assert_eq!(frame.gradients.len(), 1, "what it baked is still in the frame's atlas");
}

#[test]
fn a_streak_tapers_from_the_head_to_the_tail() {
    let mut out = Vec::new();
    let points = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0)];
    push_streak(&mut out, &points, 10.0, 1.0, [1.0, 0.5, 0.0, 1.0], 1.0);
    assert_eq!(out.len(), 12, "two segments, two triangles each");
    assert!(out.iter().any(|v| v.edge[1] == 10.0), "full width at the head");
    assert!(out.iter().any(|v| v.edge[1] == 0.0 && v.edge[0].abs() == 1.0), "a pixel's margin at the tail");
    assert!(out.iter().any(|v| v.color[3] == 0.0), "faded out at the tail");
    assert!(out.iter().all(|v| v.position[1].abs() <= 11.0), "the strip stays within reach of the line");
    out.clear();
    push_streak(&mut out, &points, 10.0, 0.5, [1.0, 0.5, 0.0, 1.0], 1.0);
    assert!(out.iter().any(|v| v.edge[1] == 5.0), "half the width at the tail with taper 0.5");
    out.clear();
    push_streak(&mut out, &points[..1], 10.0, 1.0, [1.0; 4], 1.0);
    assert!(out.is_empty());
}

#[test]
fn an_additive_streak_fades_its_light_not_its_alpha() {
    let mut out = Vec::new();
    push_streak(&mut out, &[Vec2::ZERO, Vec2::X * 50.0], 4.0, 1.0, [1.0, 1.0, 1.0, -1.0], 1.0);
    assert!(out.iter().all(|v| v.color[3] == -1.0));
    assert!(out.iter().any(|v| v.color[0] == 1.0), "full light at the head");
    assert!(out.iter().any(|v| v.color[0] == 0.0), "the tail adds no light");
}

#[test]
fn thing_codes_start_at_one() {
    assert_eq!(thing_code(ThingId::ROOT), 1.0);
    assert_eq!(thing_code(ThingId(41)), 42.0);
}

#[test]
fn blend_alpha_marks_additive_colours() {
    assert_eq!(blend_alpha([1.0, 0.0, 0.0, 0.0]), [1.0, 0.0, 0.0, -1.0]);
    assert_eq!(blend_alpha([0.0, 0.0, 0.0, 0.0]), [0.0; 4]);
    assert_eq!(blend_alpha([0.2, 0.0, 0.0, 1.0]), [0.2, 0.0, 0.0, 1.0]);
}
