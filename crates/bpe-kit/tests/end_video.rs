//! "End the video": a rule asks, the log notes the end, a rewind forgets it and play finds it again.

use bpe_core::resolve;
use bpe_kit::{ending::scene_that_ends, kit};
use bpe_runtime::{Log, Playback, World};
use std::sync::Arc;

#[test]
fn a_rule_ends_the_video() {
    let kit = kit();
    let scene = scene_that_ends(&kit.registry, 1.0, 2.0);
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(5.0, &kit.sims, &mut log);
    // Asked at the first step at or past 1 s, so within a step of 3 s.
    let end = log.end().expect("a rule ended the video");
    assert!((240 * 3..=240 * 3 + 2).contains(&end), "{end}");
    assert_eq!(world.end_step, Some(end));
}

#[test]
fn the_end_is_forgotten_on_rewind_and_found_again() {
    let kit = kit();
    let scene = scene_that_ends(&kit.registry, 1.0, 2.0);
    let resolved = resolve(&scene, &kit.registry);
    let mut playback = Playback::new(&scene, &resolved, Arc::clone(&kit.sims));
    playback.seek(5.0);
    let found = playback.log().end();
    assert!(found.is_some_and(|e| (240 * 3..=240 * 3 + 2).contains(&e)), "{found:?}");
    playback.seek(0.5);
    assert_eq!(playback.log().end(), None, "rewound to before the rule asked");
    assert_eq!(playback.world().end_step, None);
    playback.seek(5.0);
    assert_eq!(playback.log().end(), found);
    // A rewind to after the rule asked keeps it.
    playback.seek(2.0);
    assert_eq!(playback.log().end(), found);
}
