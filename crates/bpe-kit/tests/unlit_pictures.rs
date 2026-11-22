//! No Lighting card, no change: every test scene keeps drawing exactly the picture it draws. The
//! hashes in `tests/golden-unlit*.txt` are made on the owner's graphics card, named on the file's
//! first line (`bpe_gfx::card_golden`); another card draws a few bits differently, so there the
//! test only checks that every scene still draws.
//!
//! Bless on the owner's card with the "Refresh GPU pictures" workflow (debug and `--release`: the
//! frames are built from the physics, whose trig differs a hair between the two), and only when a
//! change is meant to move unlit pictures.

use bpe_gfx::{CardGolden, Frame, Renderer, build_frame, card_golden, test_device};
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

const W: u32 = 135;
const H: u32 = 240;
const MOMENTS: [f64; 3] = [0.5, 2.0, 4.0];

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn scenes_without_a_lighting_card_draw_as_they_always_did() {
    let Ok((device, queue)) = test_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut renderer = Renderer::new(&device, W, H);
    let mut lines = String::from("# scene\tmoment\thash\n");
    for (name, scene) in scenes::all(&kit.registry) {
        assert!(!scene.things[&bpe_core::ThingId::ROOT].slots.contains_key(&bpe_core::SlotId::Lighting));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in MOMENTS {
            playback.seek(t);
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
            renderer.render(&device, &queue, &frame);
            // Programs for shader layers build the first time they are named: draw again once built.
            if renderer.programs_waiting() {
                renderer.render(&device, &queue, &frame);
            }
            let pixels = renderer.read_output(&device, &queue).unwrap();
            lines += &format!("{name}\t{t}\t{:016x}\n", fnv(&pixels));
        }
    }
    let file = if cfg!(debug_assertions) { "tests/golden-unlit-debug.txt" } else { "tests/golden-unlit.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    let (expected, driver_note) = match card_golden(&path, &lines) {
        CardGolden::Blessed => return,
        CardGolden::NotCompared(why) => return eprintln!("{why}"),
        CardGolden::Compare { expected, driver_note } => (expected, driver_note),
    };
    let differ: Vec<_> = expected
        .lines()
        .zip(lines.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| format!("  was  {a}\n  now  {b}"))
        .collect();
    assert!(
        differ.is_empty() && expected.lines().count() == lines.lines().count(),
        "unlit pictures changed:\n{}\n{}",
        differ.join("\n"),
        driver_note.unwrap_or_default()
    );
}
