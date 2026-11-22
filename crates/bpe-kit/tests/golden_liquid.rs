//! Golden liquids (liquid part 1): every liquid test scene must keep making the same liquid. Once
//! a second, how many drops have been born (worked out on the processor, exactly) and how many
//! there are (on the graphics card) are written down. The drops themselves move on the card, whose
//! arithmetic can differ a little between drivers, so their places are pinned on this machine
//! only: a seek back and forth, and a second run, must give exactly the same drops. A liquid that
//! pushes balls (part 2) moves them, so on this machine a seek must give exactly the same balls and
//! log too; one that doesn't must leave them hitting exactly as they do without it.
//!
//! The drop counts depend on the card, so the golden is made on the owner's card, named on its
//! first line (`bpe_gfx::card_golden`); on another card only the seeking and second-run checks
//! apply. When a change is meant to change the liquid, refresh it with the "Refresh GPU pictures"
//! workflow (debug and `--release`) and say why in the pull request.

use bpe_gfx::swarm::{SwarmGpu, use_for_now};
use bpe_kit::things::liquid::{hash_of, state_of};
use bpe_kit::{kit, scenes};
use bpe_runtime::{Playback, main_part};

const SECONDS: u32 = 4;

#[test]
fn every_liquid_scene_keeps_its_liquid() {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let _card = use_for_now(Some(SwarmGpu::for_device(&device, &queue)));
    let kit = kit();
    let mut lines = String::from("# scene\tsecond\tborn\tdrops\n");
    for (name, scene) in scenes::liquid_scenes(&kit.registry) {
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
        let id = scene.order.iter().copied().find(|t| scene.things[t].kind == "liquid").expect("a liquid");
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        for second in 1..=SECONDS {
            playback.seek(second as f64);
            let st = state_of(playback.world(), id).expect("its state");
            let body = st.host.as_ref().and_then(|h| h.body.as_ref()).expect("a body on the card");
            lines += &format!("{name}\t{second}\t{}\t{}\n", st.born, body.read().len());
        }
        let last = hash_of(state_of(playback.world(), id).unwrap());
        // Seeking around replays them exactly, and so does a second run.
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in [3.0, 1.0, SECONDS as f64] {
            jumpy.seek(t);
        }
        assert_eq!(hash_of(state_of(jumpy.world(), id).unwrap()), last, "{name}: seeking");
        let main = main_part(&scene, &resolved, &kit.sims);
        if main.things.contains_key(&id) {
            // It pushes the balls: they go the same way after a seek.
            assert_eq!(jumpy.world().bodies, playback.world().bodies, "{name}: the balls after seeking");
            assert_eq!(jumpy.log().hash(), playback.log().hash(), "{name}: the log after seeking");
        } else {
            // The balls hit exactly as they do without the liquid.
            let mut alone = Playback::new(&scene, &main, kit.sims.clone());
            alone.seek(SECONDS as f64);
            assert_eq!(alone.log().hash(), playback.log().hash(), "{name}: the liquid moved the balls");
        }
    }
    let file = if cfg!(debug_assertions) { "tests/golden-liquid-debug.txt" } else { "tests/golden-liquid.txt" };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    match bpe_gfx::card_golden(&path, &lines) {
        bpe_gfx::CardGolden::Blessed => {}
        bpe_gfx::CardGolden::NotCompared(why) => eprintln!("{why}"),
        bpe_gfx::CardGolden::Compare { expected, driver_note } => {
            assert_eq!(expected, lines, "the liquid changed {}", driver_note.unwrap_or_default())
        }
    }
}
