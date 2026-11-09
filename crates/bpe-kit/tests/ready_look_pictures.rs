//! Picture tests for the ready-made looks on the software GPU: every look draws something
//! on the dark ground, and no two looks of a family draw the same picture.

use std::collections::HashMap;

use bpe_core::{Color, Scene, Settings, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, test_device};
use bpe_kit::ready_looks::{self, Wears};
use bpe_kit::things::shape::{ShapeFill, ShapeSettings, ShapeStart};
use bpe_kit::{SceneSettings, kit};
use bpe_runtime::{CameraView, Log, World};

/// The picture's size in pixels, and how much of the world it shows across.
const SIDE: u32 = 96;
const WORLD: u32 = 320;
const GROUND: Color = Color::rgb(0x15, 0x15, 0x15);

/// A look's id and the pixels it drew.
type Picture = (&'static str, Vec<u8>);

/// The look on a still ball of radius 80 (or a solid 260 × 170 ellipse) at the middle of a dark scene.
fn dressed(look: &ready_looks::ReadyLook) -> Scene {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = Scene::empty(reg);
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    root.params =
        SceneSettings { background: bpe_kit::paint::Paint::solid(GROUND), ..SceneSettings::default() }.to_params();
    let kind = match look.wears {
        Wears::Balls => "balls",
        Wears::Shape => "shape",
    };
    let id = ready_looks::add_with_look(&mut scene, reg, kind, ThingId::ROOT, look).unwrap();
    let thing = scene.things.get_mut(&id).unwrap();
    thing.transform = bpe_core::Transform::default();
    match look.wears {
        Wears::Balls => {
            thing.params.set("radius", Value::Number(80.0));
            thing.params.set("velocity", Value::Vec2([0.0, 0.0]));
        }
        Wears::Shape => {
            let own = ShapeSettings {
                start: ShapeStart::Ellipse,
                width: 260.0,
                height: 170.0,
                fill: ShapeFill::Filled,
                ..ShapeSettings::default()
            };
            thing.params = own.to_params();
        }
    }
    scene
}

#[test]
fn every_ready_look_draws_and_no_two_of_a_family_look_the_same() {
    let Ok((device, queue)) = test_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut renderer = Renderer::new(&device, SIDE, SIDE);
    let mut seen: HashMap<(Wears, &str), Vec<Picture>> = HashMap::new();
    for look in ready_looks::all() {
        let scene = dressed(look);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        frame.set_view(CameraView::default(), (WORLD, WORLD));
        renderer.forget_effect_history();
        renderer.render(&device, &queue, &frame);
        assert!(renderer.program_errors().is_empty(), "{}: {:?}", look.id, renderer.program_errors());
        let pixels = renderer.read_output(&device, &queue).unwrap();
        // The middle of the picture is the thing's, not the ground's.
        let middle = ((SIDE / 2 * SIDE + SIDE / 2) * 4) as usize;
        let ground = [GROUND.r, GROUND.g, GROUND.b];
        let far = (0..3).map(|c| pixels[middle + c].abs_diff(ground[c])).max().unwrap();
        // Dark looks (Ink, Neon ring...) are dark in the middle: look for anything drawn at all.
        let drawn = pixels.chunks(4).filter(|p| (0..3).any(|c| p[c].abs_diff(ground[c]) > 24)).count();
        assert!(far > 2 || drawn > 50, "{} draws nothing", look.id);
        let family = seen.entry((look.wears, look.family)).or_default();
        for (other, theirs) in family.iter() {
            assert_ne!(theirs, &pixels, "{} and {other} look the same", look.id);
        }
        family.push((look.id, pixels));
    }
}
