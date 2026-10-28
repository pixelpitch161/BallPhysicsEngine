//! Old scenes must sound exactly as they did before the instrument library, melodies and
//! songs were added. Each preset's and each scene file's soundtrack is hashed sample by sample
//! and compared with the hash the engine produced before those features existed.
//!
//! To print fresh hashes (only after a deliberate change to how old scenes sound), run
//! `BPE_PRINT_SOUND_HASHES=1 cargo test -p bpe-audio --test old_scenes -- --nocapture`.

use bpe_scene::Scene;

/// Hashes recorded on main at 67cf041, before any of the new sound features.
const GOLDEN: &[(&str, u64)] = &[
    ("Reference reel", 0xdcee90b73c18a4ad),
    ("Growing ball", 0x4e22ede5124f46bc),
    ("Rainbow trails", 0x1a45cda3308f2f39),
    ("Box arena", 0xf3d946e3b86eeb99),
    ("One big ball", 0xb597a65af5efaa8e),
    ("Escape rings", 0x907bf8c324787352),
    ("Escape and multiply", 0x0721ea1f2df9d8aa),
    ("Spinning hexagon", 0xf60c7d3330490f90),
    ("Multiplying balls", 0x965cee97e41b9522),
    ("Shrinking ring", 0xb3533e3c0273c482),
    ("String art", 0xe2843aac87a5f8ed),
    ("Plinko", 0x25786636c35c9c93),
    ("Pulsing ring", 0x4c33fcb16d2ae494),
    ("reference-reel.ron", 0xdcee90b73c18a4ad),
    ("growing-ball.ron", 0x4e22ede5124f46bc),
];

fn hash(samples: &[f32]) -> u64 {
    // FNV-1a over the exact bits, so any change at all shows up.
    samples.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, s| (h ^ s.to_bits() as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

fn scenes() -> Vec<(String, Scene)> {
    let mut all: Vec<(String, Scene)> = Scene::presets().into_iter().map(|p| (p.name.to_string(), p.scene)).collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes");
    for name in ["reference-reel.ron", "growing-ball.ron"] {
        all.push((name.to_string(), Scene::load(dir.join(name)).unwrap()));
    }
    all
}

#[test]
fn old_scenes_sound_identical() {
    let print = std::env::var_os("BPE_PRINT_SOUND_HASHES").is_some();
    let mut failures = Vec::new();
    for (name, scene) in scenes() {
        let hits = bpe_sim::collect_wall_hits(&scene);
        let track = bpe_audio::render_soundtrack(&scene, &hits, None).unwrap();
        let got = hash(&track.samples);
        if print {
            println!("    ({name:?}, 0x{got:016x}),");
            continue;
        }
        match GOLDEN.iter().find(|(n, _)| *n == name) {
            Some((_, want)) if *want == got => {}
            Some((_, want)) => failures.push(format!("{name}: 0x{got:016x}, expected 0x{want:016x}")),
            None => failures.push(format!("{name}: no golden hash")),
        }
    }
    assert!(failures.is_empty(), "soundtracks changed:\n{}", failures.join("\n"));
}
