//! Timings for captions and pre-scoring. Run with
//! `cargo test --release -p bpe-export --test bench -- --ignored --nocapture`.

use bpe_export::batch::{self, Vary};
use bpe_export::score::{self, ScoreInputs, Weights};
use bpe_render::EffectLibrary;
use bpe_render::text::{TextLayout, TextQuads, text_values};
use bpe_scene::{Anim, Caption, CaptionStyle, React, Scene};
use bpe_sim::Playback;
use std::time::Instant;

fn six_captions() -> Vec<Caption> {
    let mut captions = vec![
        Caption::new("Can it escape?", CaptionStyle::Hook),
        Caption::new("{hits} bounces", CaptionStyle::Counter),
        Caption::new("{countdown}", CaptionStyle::Timer),
        Caption::new("Ring {escapes}/{rings}", CaptionStyle::BigCentre),
        Caption::new("Wait for the end…", CaptionStyle::Subtitle),
        Caption::new("{balls} balls", CaptionStyle::Counter),
    ];
    captions[0].anim_in = Anim::Typewriter;
    captions[1].react = React::PulseOnHit;
    captions[3].anim_in = Anim::Glitch;
    captions
}

#[test]
#[ignore]
fn caption_layout_per_frame() {
    let mut scene = Scene::presets().into_iter().find(|p| p.name == "Escape rings").unwrap().scene;
    scene.captions = six_captions();
    let mut playback = Playback::new(&scene);
    let mut layout = TextLayout::default();
    let mut quads = TextQuads::default();
    let frames = scene.frame_count().min(900);
    let fps = scene.video.fps as f64;
    let mut layout_seconds = 0.0;
    for frame in 0..frames {
        playback.seek(frame as f64 / fps);
        let started = Instant::now();
        let values = text_values(&scene, &playback);
        layout.layout(&scene.captions, &values, 1.0, &mut quads);
        layout_seconds += started.elapsed().as_secs_f64();
    }
    let per_frame = layout_seconds / frames as f64 * 1000.0;
    eprintln!("6 captions: {per_frame:.4} ms a frame over {frames} frames (target under 0.3 ms)");
    assert!(per_frame < 0.3, "{per_frame} ms");
}

#[test]
#[ignore]
fn prescoring_rate() {
    let presets = Scene::presets();
    let library = EffectLibrary::new();
    let vary = Vary::parse("balls,arena,bounces,look,captions").unwrap();
    let weights = Weights::default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per_thread = 12;
    let started = Instant::now();
    std::thread::scope(|scope| {
        for t in 0..threads {
            let (presets, library, vary) = (&presets, &library, &vary);
            scope.spawn(move || {
                for i in 0..per_thread {
                    let seed = (t * per_thread + i) as u64;
                    let mut base = presets[seed as usize % presets.len()].scene.clone();
                    base.video.duration = 30.0;
                    let (scene, _) = batch::variation(&base, vary, seed, 0.5, library, &[]);
                    let stats = score::simulate(&scene);
                    weights.score(score::parts(&scene, &stats, &ScoreInputs::default()));
                }
            });
        }
    });
    let seconds = started.elapsed().as_secs_f64();
    let count = threads * per_thread;
    let rate = count as f64 / seconds;
    eprintln!(
        "pre-scored {count} 30 s variations on {threads} cores in {seconds:.1} s: {rate:.1} a second, \
         {:.1} a second a core (8 cores: about {:.0} a second)",
        rate / threads as f64,
        rate / threads as f64 * 8.0
    );
}
