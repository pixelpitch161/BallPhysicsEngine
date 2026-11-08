//! Tests of a Text's experimental effects (Text part 5): what each makes of the Text's instances.

use bpe_core::{LayerId, Scene, SlotId, ThingId, Value, layered_key, resolve};
use bpe_gfx::{Frame, PathChannel, PathInstance, build_frame};
use bpe_runtime::{Event, EventKind, Log, Party, World};
use glam::Vec2;

use super::KIND;
use super::fx::{MeltNow, melt_contours};
use crate::modules::look::shape::effects::{ECHO, GLITCH, MELT, MIRROR, MeltEffect, SPLIT};

fn scene_with(words: &str) -> (Scene, ThingId) {
    let reg = crate::registry();
    let mut scene = Scene::empty(&reg);
    let id = crate::add_thing(&mut scene, &reg, KIND, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().params.set("words", Value::Text(words.into()));
    (scene, id)
}

fn add(scene: &mut Scene, id: ThingId, kind: &str, params: &[(&str, Value)]) -> LayerId {
    let reg = crate::registry();
    let layer = scene.add_layer(&reg, id, SlotId::Look, kind).unwrap();
    let card = scene.card_mut(id, SlotId::Look).unwrap();
    for (k, v) in params {
        card.params.set(&layered_key(layer, k), v.clone());
    }
    layer
}

fn frame_with(scene: &Scene, time: f64, hit_at: Option<f64>) -> Frame {
    let kit = crate::kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    if let Some(t) = hit_at {
        let step = (t * world.steps_per_second as f64).round() as u64;
        log.push(Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(999), item: 0 },
            b: None,
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed: 500.0,
            at: 0.0,
        });
    }
    let mut frame = Frame::default();
    build_frame(scene, &resolved, &world, &log, time, &kit.draws, &mut frame);
    frame
}

/// The screen Text's letters (in the overlay).
fn letters(frame: &Frame) -> (Vec<PathInstance>, Vec<[f32; 4]>) {
    let o = frame.overlay.as_ref().expect("a screen text draws into the overlay");
    (o.paths.clone(), o.path_points.clone())
}

#[test]
fn a_text_without_effects_or_with_them_switched_off_draws_as_before() {
    let (mut scene, id) = scene_with("HI");
    let plain = letters(&frame_with(&scene, 1.0, None));
    let layer = add(&mut scene, id, MIRROR, &[]);
    scene.set_layer_enabled(&crate::registry(), id, SlotId::Look, layer, false).unwrap();
    add(&mut scene, id, SPLIT, &[("amount", Value::Number(0.0))]);
    let off = letters(&frame_with(&scene, 1.0, None));
    assert_eq!(plain.0, off.0);
    assert_eq!(plain.1, off.1);
}

#[test]
fn echo_stacks_smaller_copies_behind_the_letters() {
    let (mut scene, id) = scene_with("HI");
    add(&mut scene, id, ECHO, &[("copies", Value::Int(4)), ("hue", Value::Number(90.0))]);
    let (ps, _) = letters(&frame_with(&scene, 0.0, None));
    assert_eq!(ps.len(), 2 * 5, "two letters and four copies of each");
    // The furthest (smallest) copies first, the letters last at full size.
    let sizes: Vec<f32> = ps.iter().map(|p| p.a[3]).collect();
    assert!(sizes.windows(2).all(|w| w[0] <= w[1] + 1e-6), "{sizes:?}");
    assert_eq!(&sizes[8..], &[1.0, 1.0]);
    assert!((sizes[0] - 0.5).abs() < 1e-5, "the last copy is half size: {}", sizes[0]);
    // Each copy toward the point above, turned further round the wheel and fainter.
    assert!(ps[0].a[1] < ps[8].a[1] - 100.0, "the last copies rise toward the point");
    assert!((ps[0].style().hue - 0.0).abs() < 1e-3, "four quarter turns are a whole turn");
    assert!((ps[2].style().hue - 0.75).abs() < 1e-3, "{}", ps[2].style().hue);
    assert!(ps[0].clip[2] > ps[6].clip[2] && ps[8].clip[2] == 0.0);
    // The points are the letters' own.
    assert_eq!(ps[0].b, ps[8].b);
}

#[test]
fn mirror_turns_copies_round_the_middle_and_flips_every_other() {
    let (mut scene, id) = scene_with("HI");
    scene.things.get_mut(&id).unwrap().transform.position = Vec2::new(200.0, 0.0);
    add(
        &mut scene,
        id,
        MIRROR,
        &[
            ("ways", Value::Int(4)),
            ("round", Value::Choice("the_middle_of_the_picture".into())),
            ("mirrored", Value::Bool(true)),
        ],
    );
    let (ps, points) = letters(&frame_with(&scene, 0.0, None));
    assert_eq!(ps.len(), 2 * 4);
    // Every copy as far from the middle as its letter, a quarter turn on each time.
    let centre = |p: &PathInstance| Vec2::new(p.a[0], p.a[1]);
    for k in 0..4 {
        let (letter, copy) = (centre(&ps[0]), centre(&ps[k]));
        assert!((letter.length() - copy.length()).abs() < 1e-3);
    }
    let turned = centre(&ps[2]);
    assert!((turned - Vec2::new(-200.0, 0.0)).length() < 1e-2, "half way round: {turned}");
    // The odd copies' points are the letter's with y the other way.
    let (a, b) = (ps[0], ps[1]);
    assert_ne!(a.b[2], b.b[2], "a flipped copy has points of its own");
    for k in 0..a.b[3] as usize {
        let (p, q) = (points[a.b[2] as usize + k], points[b.b[2] as usize + k]);
        assert_eq!((p[0], -p[1], p[2], p[3]), (q[0], q[1], q[2], q[3]));
    }
}

#[test]
fn wedges_keep_each_copy_inside_its_own() {
    let (mut scene, id) = scene_with("WOW");
    scene.things.get_mut(&id).unwrap().transform.position = Vec2::new(150.0, 0.0);
    add(
        &mut scene,
        id,
        MIRROR,
        &[
            ("ways", Value::Int(6)),
            ("round", Value::Choice("the_middle_of_the_picture".into())),
            ("mirrored", Value::Bool(true)),
            ("wedges", Value::Bool(true)),
        ],
    );
    let (ps, points) = letters(&frame_with(&scene, 0.0, None));
    assert!(!ps.is_empty());
    let half = std::f32::consts::PI / 6.0;
    for p in &ps {
        let corners = &points[p.clip[0] as usize..(p.clip[0] + p.clip[1]) as usize];
        // Where its own middle direction lies: the wedge its corners fall in.
        let world: Vec<Vec2> = corners
            .iter()
            .map(|c| Vec2::new(p.a[0], p.a[1]) + Vec2::from_angle(p.a[2]).rotate(Vec2::new(c[0], c[1]) * p.a[3]))
            .collect();
        let mid = world.iter().copied().sum::<Vec2>() / world.len() as f32;
        let k = (mid.to_angle() / (2.0 * half)).round();
        let beta = k * 2.0 * half;
        for w in world {
            if w.length() < 1e-2 {
                continue;
            }
            let off = (Vec2::from_angle(-beta).rotate(w)).to_angle();
            assert!(off.abs() <= half + 1e-3, "a corner {off} rad off its wedge's middle");
        }
    }
}

#[test]
fn colour_split_makes_coverings_then_red_and_blue_pulled_apart() {
    let (mut scene, id) = scene_with("HI");
    add(&mut scene, id, SPLIT, &[("distance", Value::Number(10.0))]);
    let (ps, _) = letters(&frame_with(&scene, 0.0, None));
    assert_eq!(ps.len(), 6);
    let channels: Vec<PathChannel> = ps.iter().map(|p| p.style().channel).collect();
    assert!(channels[..2].iter().all(|c| *c == PathChannel::GreenCovering), "{channels:?}");
    assert!(channels[2..].iter().all(|c| c.adds_light()));
    let red = ps.iter().find(|p| p.style().channel == PathChannel::RedLight).unwrap();
    assert!((red.a[0] - ps[0].a[0] - 10.0).abs() < 1e-3, "red 10 px to the right");
    let blue = ps.iter().find(|p| p.style().channel == PathChannel::BlueLight).unwrap();
    assert!((blue.a[0] - ps[0].a[0] + 10.0).abs() < 1e-3);
}

#[test]
fn a_hit_splits_further_and_tears_more() {
    let (mut scene, id) = scene_with("HI");
    add(&mut scene, id, SPLIT, &[("distance", Value::Number(10.0)), ("harder", Value::Number(1.0))]);
    let red_of = |frame: &Frame| {
        let (ps, _) = letters(frame);
        let red = ps.iter().find(|p| p.style().channel == PathChannel::RedLight).copied().unwrap();
        red.a[0] - ps[0].a[0]
    };
    let calm = red_of(&frame_with(&scene, 1.0, None));
    let kicked = red_of(&frame_with(&scene, 1.0, Some(1.0)));
    let later = red_of(&frame_with(&scene, 3.0, Some(1.0)));
    assert!((calm - 10.0).abs() < 1e-3);
    assert!((kicked - 40.0).abs() < 1e-2, "four times as far right after the hit: {kicked}");
    assert!((later - 10.0).abs() < 0.5, "settled two seconds on: {later}");
    // Glitch tears nothing at Amount 0 but every band right after a hit.
    let (mut scene, id) = scene_with("HI");
    add(&mut scene, id, GLITCH, &[("amount", Value::Number(0.0)), ("harder", Value::Number(1.0))]);
    assert_eq!(letters(&frame_with(&scene, 1.0, None)).0.len(), 2);
    assert!(letters(&frame_with(&scene, 1.0, Some(1.0))).0.len() > 20);
}

/// The area of a convex polygon.
fn area(ps: &[[f32; 4]]) -> f32 {
    let n = ps.len();
    0.5 * (0..n).map(|k| ps[k][0] * ps[(k + 1) % n][1] - ps[(k + 1) % n][0] * ps[k][1]).sum::<f32>().abs()
}

#[test]
fn glitch_cuts_the_letters_into_bands_that_tile_them() {
    let (mut scene, id) = scene_with("I");
    let plain = letters(&frame_with(&scene, 0.0, None));
    let whole = &plain.1[plain.0[0].clip[0] as usize..][..plain.0[0].clip[1] as usize];
    add(
        &mut scene,
        id,
        GLITCH,
        &[("amount", Value::Number(0.5)), ("tear_split", Value::Number(0.0)), ("band", Value::Number(10.0))],
    );
    let (ps, points) = letters(&frame_with(&scene, 0.0, None));
    assert!(ps.len() > 2, "torn into pieces: {}", ps.len());
    let pieces: f32 = ps.iter().map(|p| area(&points[p.clip[0] as usize..][..p.clip[1] as usize])).sum();
    assert!((pieces - area(whole)).abs() < 0.5, "the pieces cover the letter once: {pieces} vs {}", area(whole));
    assert!(ps.iter().any(|p| (p.a[0] - plain.0[0].a[0]).abs() > 0.5), "some band moved sideways");
    assert!(ps.iter().all(|p| p.a[1] == plain.0[0].a[1]), "only sideways");
}

#[test]
fn an_effect_worn_by_some_letters_leaves_the_rest() {
    let (mut scene, id) = scene_with("ABC");
    let layer = add(&mut scene, id, SPLIT, &[]);
    scene.set_layer_only(&crate::registry(), id, SlotId::Look, layer, vec![1]).unwrap();
    let (ps, _) = letters(&frame_with(&scene, 0.0, None));
    assert_eq!(ps.len(), 3 + 2);
    assert_eq!(ps.iter().filter(|p| p.style().channel == PathChannel::All).count(), 2);
}

#[test]
fn melt_runs_the_bottoms_down_further_than_the_tops() {
    let m = MeltEffect { start: 1.0, speed: 0.5, drip: 100.0, ..MeltEffect::default() };
    assert_eq!(MeltNow::of(&m, 0.5), None, "not before it starts");
    let now = MeltNow::of(&m, 3.0).unwrap();
    assert!((now.length - 100.0).abs() < 1e-3, "all melted two seconds on");
    let half = MeltNow::of(&m, 2.0).unwrap();
    assert!((half.length - 25.0).abs() < 1e-3, "a quarter of the way at half the time (it speeds up)");
    // A square 100 across, its top at 0 and its bottom at 100.
    let square = vec![vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0), Vec2::new(0.0, 100.0)]];
    let melted = melt_contours(&square, &now, Vec2::Y);
    assert!(melted[0].len() > 40, "cut finer so the bottom can drip: {}", melted[0].len());
    // The top edge's points sag at most a quarter of a drip; the bottom edge's drip.
    let top: Vec<Vec2> = melted[0].iter().copied().filter(|p| p.y <= 25.0 + 1e-3).collect();
    let bottom: Vec<Vec2> = melted[0].iter().copied().filter(|p| p.y > 100.0 - 1e-3).collect();
    assert!(top.len() > 10 && bottom.len() > 10);
    assert!(bottom.iter().map(|p| p.y - 100.0).fold(0.0f32, f32::max) > 30.0, "the bottom drips");
    let drips: Vec<f32> = bottom.iter().filter(|p| p.y > 100.0).map(|p| p.y).collect();
    let spread = drips.iter().copied().fold(f32::MIN, f32::max) - drips.iter().copied().fold(f32::MAX, f32::min);
    assert!(spread > 10.0, "columns drip by different amounts: {spread}");
    assert!(melted[0].iter().all(|p| p.x >= 0.0 && p.x <= 100.0), "drips run straight down");
}

#[test]
fn melt_starts_again_and_fades_before_it_does() {
    let m = MeltEffect { speed: 1.0, every: 5.0, ..MeltEffect::default() };
    let at = |t: f64| MeltNow::of(&m, t).unwrap();
    assert!(at(2.0).length > 100.0 && at(2.0).opacity == 1.0);
    assert!(at(4.8).opacity < 1.0 && at(4.8).opacity > 0.0, "fading out at the end of a round");
    assert_eq!(at(5.0).length, 0.0, "back to the start");
    assert_eq!(at(5.0).opacity, 1.0);
}

#[test]
fn a_melting_text_draws_its_letters_moved_down() {
    let (mut scene, id) = scene_with("HI");
    let plain = letters(&frame_with(&scene, 2.0, None));
    add(&mut scene, id, MELT, &[("speed", Value::Number(1.0))]);
    let (ps, points) = letters(&frame_with(&scene, 2.0, None));
    assert_eq!(ps.len(), 2);
    let lowest = |ps: &[PathInstance], points: &[[f32; 4]]| {
        ps.iter()
            .flat_map(|p| points[p.b[2] as usize..][..p.b[3] as usize].iter().map(|q| q[1]))
            .fold(f32::MIN, f32::max)
    };
    assert!(lowest(&ps, &points) > lowest(&plain.0, &plain.1) + 30.0);
}

#[test]
fn glitch_past_its_most_bands_keeps_the_rest_of_the_letter_whole() {
    let (mut scene, id) = scene_with("I");
    let plain = letters(&frame_with(&scene, 0.0, None));
    let whole = &plain.1[plain.0[0].clip[0] as usize..][..plain.0[0].clip[1] as usize];
    // Every band torn, far thinner than the letter: only the first 64 can be cut apart.
    add(
        &mut scene,
        id,
        GLITCH,
        &[("amount", Value::Number(1.0)), ("tear_split", Value::Number(0.0)), ("band", Value::Number(1.0))],
    );
    let (ps, points) = letters(&frame_with(&scene, 0.0, None));
    assert_eq!(ps.len(), 64 + 1, "64 torn bands and the rest whole");
    let pieces: f32 = ps.iter().map(|p| area(&points[p.clip[0] as usize..][..p.clip[1] as usize])).sum();
    assert!((pieces - area(whole)).abs() < 0.5, "the pieces cover the letter once: {pieces} vs {}", area(whole));
    let rest = ps.iter().filter(|p| p.a[0] == plain.0[0].a[0]).count();
    assert!(rest >= 1, "the rest stays where it was");
}

#[test]
fn a_mirror_at_no_amount_leaves_the_word_as_it_is() {
    let (mut scene, id) = scene_with("HI");
    let plain = letters(&frame_with(&scene, 1.0, None));
    add(
        &mut scene,
        id,
        MIRROR,
        &[("amount", Value::Number(0.0)), ("turn", Value::Number(30.0)), ("wedges", Value::Bool(true))],
    );
    let (ps, _) = letters(&frame_with(&scene, 1.0, None));
    assert_eq!(ps, plain.0);
}

#[test]
fn smaller_echo_copies_grow_their_boxes_to_hold_the_layers() {
    let (mut scene, id) = scene_with("I");
    let plain = letters(&frame_with(&scene, 0.0, None));
    let box_of = |p: &PathInstance, points: &[[f32; 4]]| {
        let c = &points[p.clip[0] as usize..][..p.clip[1] as usize];
        c.iter().fold(f32::MIN, |m, q| m.max(q[0])) - c.iter().fold(f32::MAX, |m, q| m.min(q[0]))
    };
    let letter = box_of(&plain.0[0], &plain.1);
    add(&mut scene, id, ECHO, &[("copies", Value::Int(1)), ("last_size", Value::Number(0.5))]);
    let (ps, points) = letters(&frame_with(&scene, 0.0, None));
    assert_eq!(ps.len(), 2);
    let (copy, own) = (&ps[0], &ps[1]);
    assert!((copy.a[3] - 0.5 * own.a[3]).abs() < 1e-6);
    let margin = own.c[2] + own.b[0] + 2.0;
    // On screen the copy's margin round the letter is as many px as the letter's own.
    let shown = box_of(copy, &points) * copy.a[3];
    let expected = (letter - 2.0 * margin) * 0.5 + 2.0 * margin;
    assert!((shown - expected).abs() < 0.05, "{shown} vs {expected}");
}
