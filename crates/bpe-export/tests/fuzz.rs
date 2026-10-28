//! Fuzz sweep over random scenes with captions on: every seed must simulate, lay out its captions
//! and score without panicking or producing numbers that aren't finite.
//!
//! Slow, so it only runs when asked: `cargo test --release -p bpe-export --test fuzz -- --ignored`.
//! `BPE_FUZZ_SEEDS` sets how many seeds (500 by default).

use bpe_export::batch::{self, Vary};
use bpe_export::score::{self, ScoreInputs, Weights};
use bpe_render::EffectLibrary;
use bpe_render::text::{TextLayout, TextQuads, text_values};
use bpe_scene::Scene;
use bpe_sim::Playback;

#[test]
#[ignore]
fn random_scenes_with_captions_never_break() {
    let seeds: u64 = std::env::var("BPE_FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(500);
    let presets = Scene::presets();
    let library = EffectLibrary::new();
    let vary = Vary::parse("all").unwrap();
    let weights = Weights::default();
    let started = std::time::Instant::now();
    let mut layout = TextLayout::default();
    let mut quads = TextQuads::default();
    let (mut most_glyphs, mut captions_seen) = (0, 0);
    for seed in 0..seeds {
        let base = &presets[seed as usize % presets.len()].scene;
        let (mut scene, _) = batch::variation(base, &vary, seed, 1.0, &library, &[]);
        scene.video.duration = 10.0;
        let context = format!("seed {seed} on {}", base.name);
        assert!(scene.to_ron().is_ok_and(|ron| Scene::from_ron(&ron).is_ok()), "{context}: doesn't save");
        captions_seen += scene.captions.len();
        let mut playback = Playback::new(&scene);
        // Ten checks a second is enough to meet every caption animation and counter change.
        for step in 0..=100 {
            let time = step as f64 * 0.1;
            playback.seek(time);
            let values = text_values(&scene, &playback);
            layout.layout(&scene.captions, &values, 1.0, &mut quads);
            for glyph in quads.over.iter().chain(&quads.under) {
                let numbers = glyph.rect.iter().chain(&glyph.uv).chain(&glyph.color).chain(&glyph.params);
                assert!(numbers.into_iter().all(|v| v.is_finite()), "{context} at {time}s: {glyph:?}");
            }
            most_glyphs = most_glyphs.max(quads.over.len() + quads.under.len());
        }
        let stats = score::simulate(&scene);
        let parts = score::parts(&scene, &stats, &ScoreInputs::default());
        let total = weights.score(parts).total;
        assert!((0.0..=1.0).contains(&total), "{context}: score {total} from {parts:?}");
    }
    eprintln!(
        "{seeds} seeds in {:.1} s, {captions_seen} captions, at most {most_glyphs} glyphs in a frame",
        started.elapsed().as_secs_f32()
    );
    assert!(captions_seen > 0, "the sweep never made a caption");
}
