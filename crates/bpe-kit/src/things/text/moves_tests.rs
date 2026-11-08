use super::*;
use crate::modules::look::shape as look;
use crate::things::text::KIND;
use bpe_core::{Pulse, Resolved, Scene, SoundSource, ThingId, resolve};
use bpe_runtime::{Event, Log, Party, World};

/// A Text "ABCD" set in the world, each move of `moves` added with its settings.
fn text_with(moves: &[(&str, Vec<(&str, Value)>)]) -> (Scene, ThingId, Vec<LayerId>) {
    let reg = crate::registry();
    let mut scene = Scene::empty(&reg);
    let id = crate::add_thing(&mut scene, &reg, KIND, ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.params.set("words", Value::Text("ABCD".into()));
    t.screen = Some(false);
    let mut ids = Vec::new();
    for (kind, params) in moves {
        let layer = scene.add_layer(&reg, id, SlotId::Look, kind).unwrap();
        let card = scene.card_mut(id, SlotId::Look).unwrap();
        for (k, v) in params {
            card.params.set(&layered_key(layer, k), v.clone());
        }
        ids.push(layer);
    }
    (scene, id, ids)
}

/// The Text's first copy at `time` (run up to then), read with `log` added to the world's own
/// and `sound`.
fn now_at(scene: &Scene, id: ThingId, time: f64, extra: &[Event], sound: Option<&dyn SoundAt>) -> TextNow {
    let kit = crate::kit();
    let resolved: Resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    for e in extra {
        log.push(e.clone());
    }
    let r = &resolved.things[&id];
    let at = TextAt {
        scene: Some(&resolved),
        log: Some(&log),
        sound,
        look: r.slots.get(&SlotId::Look),
        ..TextAt::new(scene, &r.params, &world, id, time)
    };
    super::super::draw::text_now(&at).unwrap()
}

use bpe_runtime::SoundAt;

fn moved(now: &TextNow, k: usize) -> LetterMove {
    now.shown[k].expect("shown")
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

#[test]
fn a_text_without_moves_shows_as_laid_out() {
    let (scene, id, _) = text_with(&[]);
    let now = now_at(&scene, id, 1.0, &[], None);
    assert!(now.shown.iter().all(|m| *m == Some(LetterMove::WHOLE)));
}

#[test]
fn a_wave_runs_along_the_letters() {
    let (scene, id, _) =
        text_with(&[(look::moves::WAVE, vec![("height", n(30.0)), ("letters", n(8.0)), ("speed", n(0.0))])]);
    let now = now_at(&scene, id, 0.0, &[], None);
    assert!(moved(&now, 0).offset.length() < 1e-4);
    // Letter 2 is a quarter wave along: at the bottom of the wave.
    assert!((moved(&now, 2).offset.y - 30.0).abs() < 1e-3, "{:?}", moved(&now, 2));
    assert!(moved(&now, 1).offset.y > 0.0 && moved(&now, 1).offset.y < 30.0);
}

#[test]
fn a_beat_jump_peaks_half_way_through_takes() {
    let (scene, id, _) =
        text_with(&[(look::moves::BEAT, vec![("height", n(40.0)), ("takes", n(0.3)), ("later", n(0.0))])]);
    // 120 beats a minute: a beat at 0.5 s.
    let now = now_at(&scene, id, 0.65, &[], None);
    assert!((moved(&now, 0).offset.y + 40.0).abs() < 0.5, "{:?}", moved(&now, 0));
    assert!(moved(&now, 0).stretch.y > 1.0 && moved(&now, 0).stretch.x < 1.0, "squashed tall");
    let later = now_at(&scene, id, 0.9, &[], None);
    assert!(moved(&later, 0).offset.length() < 1e-4, "landed");
}

fn hit_on(id: ThingId, item: u32, step: u64) -> Event {
    Event {
        step,
        kind: EventKind::Hit,
        a: Party { thing: ThingId(999), item: 0 },
        b: Some(Party { thing: id, item }),
        point: Vec2::ZERO,
        normal: Vec2::Y,
        speed: 500.0,
        at: 0.0,
    }
}

#[test]
fn a_hit_jumps_the_hit_letter_or_every_letter() {
    let (mut scene, id, layers) = text_with(&[(look::moves::HIT, vec![("takes", n(0.4)), ("height", n(50.0))])]);
    // A walled Text: its letters are its parts.
    scene.things.get_mut(&id).unwrap().params.set("balls", Value::Choice("bounce_off".into()));
    let rate = 240;
    let now = now_at(&scene, id, 1.2, &[hit_on(id, 2, rate)], None);
    assert!((moved(&now, 2).offset.y + 50.0).abs() < 0.5, "{:?}", moved(&now, 2));
    assert!(moved(&now, 0).offset.length() < 1e-4, "only the hit letter");
    // Every letter jumps for any hit.
    let card = scene.card_mut(id, SlotId::Look).unwrap();
    card.params.set(&layered_key(layers[0], "which"), Value::Choice("every_letter".into()));
    card.params.set(&layered_key(layers[0], "later"), n(0.0));
    let now = now_at(&scene, id, 1.2, &[hit_on(id, 2, rate)], None);
    assert!((0..4).all(|k| (moved(&now, k).offset.y + 50.0).abs() < 0.5));
}

#[test]
fn flicker_dims_some_letters() {
    let (scene, id, _) = text_with(&[(look::moves::FLICKER, vec![("amount", n(1.0)), ("speed", n(20.0))])]);
    let dimmed = (0..40).any(|i| {
        let now = now_at(&scene, id, i as f64 * 0.0371, &[], None);
        (0..4).any(|k| moved(&now, k).opacity < 0.9)
    });
    assert!(dimmed);
}

/// A sound whose level is its time in seconds (up to 1).
struct Ramp;

impl SoundAt for Ramp {
    fn level(&self, _: &World, _: &Log, _: ThingId, _: &SoundSource, time: f64) -> f32 {
        time.clamp(0.0, 1.0) as f32
    }
}

#[test]
fn follow_sound_reads_each_letter_a_little_later() {
    let (mut scene, id, layers) = text_with(&[(
        look::moves::SOUND,
        vec![("level", n(0.6)), ("later", n(0.1)), ("own", n(0.0)), ("jump", n(100.0)), ("stretch", n(0.0))],
    )]);
    let address = Address { thing: id, slot: Some(SlotId::Look), key: layered_key(layers[0], "level") };
    let pulse = Pulse {
        source: PulseSource::Sound { shape: ThingId::ROOT, source: SoundSource::Bus(1) },
        amount: n(1.0),
        ..Default::default()
    };
    scene.pulses.insert(address, pulse);
    // No reader: the Level as set (resolved), the same for every letter.
    let plain = now_at(&scene, id, 0.6, &[], None);
    let ys: Vec<f32> = (0..4).map(|k| moved(&plain, k).offset.y).collect();
    assert!(ys.iter().all(|y| (*y - ys[0]).abs() < 1e-3), "{ys:?}");
    // With one, letter k hears the sound k × 0.1 s ago.
    let now = now_at(&scene, id, 0.6, &[], Some(&Ramp));
    let level = |k: usize| -moved(&now, k).offset.y / 100.0;
    let first = level(0);
    for k in 1..4 {
        assert!((first - level(k) - 0.1 * k as f32).abs() < 0.02, "letter {k}: {} vs {first}", level(k));
    }
}

#[test]
fn shake_is_seeded_and_smooth() {
    let at = |seed: i64, t: f64| {
        let (scene, id, _) = text_with(&[(look::moves::SHAKE, vec![("seed", Value::Int(seed))])]);
        moved(&now_at(&scene, id, t, &[], None), 1)
    };
    let (a, b) = (at(1, 1.0), at(1, 1.0001));
    assert!(a.offset.distance(b.offset) < 0.5, "smooth: {a:?} {b:?}");
    assert_ne!(at(1, 1.0).offset, at(2, 1.0).offset, "seeded");
    assert!(a.offset.length() > 0.0);
}

#[test]
fn spin_turns_each_letter() {
    let (scene, id, _) = text_with(&[(look::moves::SPIN, vec![("speed", n(90.0)), ("later", n(10.0))])]);
    let now = now_at(&scene, id, 1.0, &[], None);
    assert!((moved(&now, 0).turn - 90f32.to_radians()).abs() < 1e-3);
    assert!((moved(&now, 2).turn - 110f32.to_radians()).abs() < 1e-3);
    // A turned letter's contours are turned about its middle.
    let c = now.contours(0);
    let mid = now.middle(0);
    let placed = now.contours_placed(0);
    let (p, q) = (placed[0][0] - mid, c[0][0] - mid);
    assert!((p.length() - q.length()).abs() < 1e-2);
    assert!((Vec2::new(-p.y, p.x) - q).length() < 1e-2, "a quarter turn: {p} {q}");
}

#[test]
fn scatter_flies_out_and_comes_back() {
    let (scene, id, _) = text_with(&[(
        look::moves::SCATTER,
        vec![("start", n(0.5)), ("out", n(0.5)), ("stay", n(0.5)), ("back", n(0.5)), ("distance", n(400.0))],
    )]);
    let before = now_at(&scene, id, 0.2, &[], None);
    assert!(moved(&before, 0).offset.length() < 1e-4);
    let out = now_at(&scene, id, 1.2, &[], None);
    assert!(moved(&out, 0).offset.length() > 150.0, "{:?}", moved(&out, 0));
    let home = now_at(&scene, id, 2.2, &[], None);
    assert!(moved(&home, 0).offset.length() < 1e-3);
}

#[test]
fn draw_on_traces_letter_after_letter() {
    let (mut scene, id, layers) = text_with(&[(
        look::moves::DRAW_ON,
        vec![("start", n(0.0)), ("takes", n(4.0)), ("then", Value::Choice("lines_and_fill".into())), ("fill", n(0.5))],
    )]);
    let half = now_at(&scene, id, 2.0, &[], None);
    let traced: Vec<Option<f32>> = half.shown.iter().map(|m| m.and_then(|m| m.trace).map(|t| t.share)).collect();
    // The first letters done, the last not yet started (hidden).
    assert_eq!(traced[0], Some(1.0));
    assert!(half.shown[3].is_none(), "{traced:?}");
    let tr = half.shown[0].unwrap().trace.unwrap();
    assert_eq!((tr.band, tr.band_shown), (3.0, 1.0));
    assert!(tr.fill > 0.0, "filling in once traced");
    // The pen is on the word while it traces, and gone after.
    let kit = crate::kit();
    let resolved = resolve(&scene, &kit.registry);
    let world = World::build(&scene, &resolved, &kit.sims);
    let r = &resolved.things[&id];
    let pen_at = |t: f64| {
        let at = TextAt { look: r.slots.get(&SlotId::Look), ..TextAt::new(&scene, &r.params, &world, id, t) };
        pen(&at)
    };
    let (lo, hi) = half.bounds().unwrap();
    let p = pen_at(2.0).expect("a pen");
    assert!(p.x >= lo.x - 1.0 && p.x <= hi.x + 1.0 && p.y >= lo.y - 1.0 && p.y <= hi.y + 1.0, "{p}");
    assert!(pen_at(5.0).is_none() && pen_at(-1.0).is_none());
    // Fill only: done and filled, the letters are whole again.
    let card = scene.card_mut(id, SlotId::Look).unwrap();
    card.params.set(&layered_key(layers[0], "then"), Value::Choice("fill_only".into()));
    let done = now_at(&scene, id, 6.0, &[], None);
    assert!(done.shown.iter().all(|m| *m == Some(LetterMove::WHOLE)));
}

#[test]
fn a_traced_piece_goes_out_and_back() {
    let square = vec![vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]];
    let (pieces, pen) = traced(&square, 0.375);
    assert_eq!(pieces.len(), 1);
    assert_eq!(pen, Some(Vec2::new(10.0, 5.0)));
    assert_eq!(pieces[0], [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 5.0), Vec2::new(10.0, 0.0)]);
    let (whole, pen) = traced(&square, 1.0);
    assert_eq!((whole, pen), (square, None));
}

#[test]
fn a_move_worn_by_some_letters_moves_only_those_and_a_hidden_one_nothing() {
    let reg = crate::registry();
    let (mut scene, id, layers) = text_with(&[(look::moves::SPIN, vec![("speed", n(90.0))])]);
    scene.set_layer_only(&reg, id, SlotId::Look, layers[0], vec![1, 3]).unwrap();
    let now = now_at(&scene, id, 1.0, &[], None);
    assert_eq!(moved(&now, 0).turn, 0.0);
    assert!(moved(&now, 1).turn > 1.0 && moved(&now, 3).turn > 1.0);
    scene.set_layer_enabled(&reg, id, SlotId::Look, layers[0], false).unwrap();
    let now = now_at(&scene, id, 1.0, &[], None);
    assert!(now.shown.iter().all(|m| *m == Some(LetterMove::WHOLE)));
}

#[test]
fn moves_fit_only_a_text() {
    let reg = crate::registry();
    let mut scene = Scene::empty(&reg);
    let shape = crate::add_thing(&mut scene, &reg, crate::things::shape::KIND, ThingId::ROOT).unwrap();
    assert_eq!(scene.add_layer(&reg, shape, SlotId::Look, look::moves::WAVE), Err(bpe_core::EditError::NotAccepted));
    let paint = scene.add_layer(&reg, shape, SlotId::Look, look::PAINT).unwrap();
    assert_eq!(scene.set_layer_only(&reg, shape, SlotId::Look, paint, vec![1]), Err(bpe_core::EditError::NotAccepted));
    let (mut scene, id, _) = text_with(&[]);
    let paint = scene.add_layer(&reg, id, SlotId::Look, look::PAINT).unwrap();
    scene.set_layer_only(&reg, id, SlotId::Look, paint, vec![2, 0]).unwrap();
    assert_eq!(scene.things[&id].slots[&SlotId::Look].layers.iter().find(|l| l.id == paint).unwrap().only, [0, 2]);
}
