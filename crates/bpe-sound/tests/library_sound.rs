//! Every built-in scene with sound sounds: its first seconds aren't silent and never clip
//! (spec `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md`, "Checks").

use bpe_kit::kit;
use bpe_kit::library::{self, SoundLevel};
use bpe_sound::{has_sound, render_samples};

#[test]
fn every_scene_with_sound_sounds() {
    let kit = kit();
    for e in library::all() {
        let id = e.info.id;
        let scene = e.scene(&kit.registry);
        assert_eq!(has_sound(&scene), e.info.sound != SoundLevel::None, "{id}: has sound as its info says");
        if e.info.sound == SoundLevel::None {
            continue;
        }
        let seconds = e.info.seconds.min(6.0);
        let samples = render_samples(&scene, &kit, seconds);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.001, "{id}: silent for its first {seconds} s");
        assert!(peak < 1.0, "{id}: peaks at {peak}, over full scale");
    }
}
