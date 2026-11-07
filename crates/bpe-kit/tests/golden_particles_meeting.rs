//! Golden particles meeting things (particles part 4): fed children, Pop, Stick, Settle, piles
//! and two-way must keep making exactly the same particles and piles. Once a second, the hash of
//! every Particles thing's particles and piles is written down with the balls' hit log; a seek
//! back and forth must give the same, and the scenes that aren't two-way must leave the balls
//! hitting exactly as they do without the particles.
//!
//! When a change is meant to move them, run with `BLESS_GOLDEN=1` (once with `--release` and
//! once without) and say why in the pull request.

use bpe_core::{Scene, ThingId};
use bpe_kit::things::particles::sim::state_of;
use bpe_kit::things::particles::step::hash_of;
use bpe_kit::{kit, scenes};
use bpe_runtime::{Playback, World, main_part};

const SECONDS: u32 = 6;

fn fnv(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= *b as u64;
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

/// Every Particles thing's particles and piles, in tree order: (count, hash).
fn particles(scene: &Scene, world: &World) -> (usize, u64) {
    let (mut count, mut h) = (0, 0xcbf2_9ce4_8422_2325u64);
    let ids: Vec<ThingId> = scene.order.iter().copied().filter(|t| scene.things[t].kind == "particles").collect();
    assert!(!ids.is_empty());
    for id in ids {
        let st = state_of(world, id).expect("its state");
        count += st.specks.len();
        fnv(&mut h, &hash_of(st).to_le_bytes());
        fnv(&mut h, format!("{:?}", st.piles).as_bytes());
    }
    (count, h)
}

#[test]
fn every_meeting_scene_keeps_its_particles_and_piles() {
    let kit = kit();
    let mut lines = String::from("# scene\tsecond\tparticles\thash\thits\n");
    for (name, scene, two_way) in scenes::particle_scenes_meeting(&kit.registry) {
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut last = (0, 0);
        for second in 1..=SECONDS {
            playback.seek(second as f64);
            let (count, hash) = particles(&scene, playback.world());
            last = (count, hash);
            lines += &format!("{name}\t{second}\t{count}\t{hash:016x}\t{:016x}\n", playback.log().hash());
        }
        assert!(last.0 > 0, "{name}: no particles");
        // Seeking around replays them exactly.
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        for t in [4.0, 2.0, SECONDS as f64] {
            jumpy.seek(t);
        }
        assert_eq!(particles(&scene, jumpy.world()), last, "{name}: seeking");
        assert_eq!(jumpy.log().hash(), playback.log().hash(), "{name}: seeking moved the balls");
        if !two_way {
            let mut alone = Playback::new(&scene, &main_part(&scene, &resolved, &kit.sims), kit.sims.clone());
            alone.seek(SECONDS as f64);
            assert_eq!(alone.log().hash(), playback.log().hash(), "{name}: the particles moved the balls");
        }
    }
    let file = if cfg!(debug_assertions) {
        "tests/golden-particles-meeting-debug.txt"
    } else {
        "tests/golden-particles-meeting.txt"
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        std::fs::write(&path, &lines).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).expect("the golden file is missing; run with BLESS_GOLDEN=1");
    assert_eq!(expected, lines, "the particles changed");
}
