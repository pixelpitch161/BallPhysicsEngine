//! The Scenes window's preview clips: every frame is drawn at the same size, and a stop ends it at once.

use bpe_cli::preview_clip;
use bpe_gfx::test_device;
use bpe_kit::{kit, library};
use std::sync::atomic::AtomicBool;

#[test]
fn preview_clip_draws_every_frame_and_stops() {
    let Ok((device, queue)) = test_device() else { return };
    let kit = kit();
    let scene = library::find("first-bounce").expect("a built-in scene").scene(&kit.registry);
    let mut seen = Vec::new();
    let stop = AtomicBool::new(false);
    let drawn = preview_clip(&device, &queue, &scene, &kit, 0.5, 10, 0.1, &stop, |index, rgba, size| {
        assert_eq!(rgba.len(), (size.0 * size.1 * 4) as usize);
        seen.push((index, size));
    })
    .expect("a clip");
    assert_eq!(drawn, 5);
    assert_eq!(seen.iter().map(|s| s.0).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
    assert!(seen.iter().all(|s| s.1 == seen[0].1), "every frame the same size");

    let stop = AtomicBool::new(true);
    let mut calls = 0;
    let result = preview_clip(&device, &queue, &scene, &kit, 0.5, 10, 0.1, &stop, |_, _, _| calls += 1);
    assert!(result.is_err());
    assert_eq!(calls, 0);
}
