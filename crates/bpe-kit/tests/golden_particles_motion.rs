//! Golden moving particles (particles part 3): every scene of the forces and looks that move and
//! form particles must keep making exactly the same particles. Once a second, the hash of every particle's place, velocity and life left is
//! written down; a seek back and forth must give the same, and the balls must hit exactly as
//! they do without the particles.
//!
//! When a change is meant to move the particles, run with `BLESS_GOLDEN=1` (once with
//! `--release` and once without) and say why in the pull request.

use bpe_kit::things::particles::sim::state_of;
use bpe_kit::things::particles::step::hash_of;
use bpe_kit::{kit, scenes};
use bpe_runtime::{Playback, main_part};

const SECONDS: u32 = 6;

#[test]
fn every_moving_particle_scene_keeps_its_particles() {
    let kit = kit();
    let mut lines = String::from("# scene\tsecond\tparticles\thash\n");
    for (name, scene) in scenes::particle_motion_scenes(&kit.registry) {
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
        let id = scene.order.iter().copied().find(|t| scene.things[t].kind == "particles").expect("particles");
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut hashes = Vec::new();
        for second in 1..=SECONDS {
            playback.seek(second as f64);
            let st = state_of(playback.world(), id).expect("its state");
            let hash = hash_of(st);
            hashes.push(hash);
            lines += &format!("{name}\t{second}\t{}\t{hash:016x}\n", st.specks.len());
        }
        assert!(state_of(playback.world(), id).is_some_and(|s| !s.specks.is_empty()), "{name}: no particles");
        // Seeking around replays them exactly.
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in [5.0, 2.0, SECONDS as f64] {
            jumpy.seek(t);
        }
        assert_eq!(hash_of(state_of(jumpy.world(), id).unwrap()), *hashes.last().unwrap(), "{name}: seeking");
        // And the balls hit exactly as they do without the particles.
        let mut alone = Playback::new(&scene, &main_part(&scene, &resolved, &kit.sims), kit.sims.clone());
        alone.seek(SECONDS as f64);
        assert_eq!(alone.log().hash(), playback.log().hash(), "{name}: the particles moved the balls");
    }
    let file = if cfg!(debug_assertions) {
        "tests/golden-particles-motion-debug.txt"
    } else {
        "tests/golden-particles-motion.txt"
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &lines).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(expected, lines, "the particles changed");
}
