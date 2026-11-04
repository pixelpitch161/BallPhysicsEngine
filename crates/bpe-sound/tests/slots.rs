//! Slot changes: by tempo on the bar lines, by time, by hits, by keyframes, in each order.

use bpe_core::{NoteSlot, Params, Scene, Settings, SlotId, ThingId, Track, Value};
use bpe_kit::sound::{BeatsOrBars, Change, Plays, SlotOrder, VoiceSettings, starter_voice};
use bpe_kit::{Kit, kit, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Party, Playback, World};
use bpe_sound::patch::shape_voice;
use bpe_sound::voices::{NotesSpec, SlotClock};
use bpe_sound::{BLOCK, BlockCx, BlockHit, ShapeVoice, TempoMap, VoiceSource};
use glam::Vec2;

const BAR: u64 = 96_000;

fn spec(f: impl FnOnce(&mut VoiceSettings)) -> NotesSpec {
    let mut v = VoiceSettings::default();
    f(&mut v);
    NotesSpec::from_settings(&v, 4, [None, None, None])
}

fn jumps(keys: &[(f64, i64)]) -> Track {
    let mut t = Track::default();
    for &(time, slot) in keys {
        t.set_key(time, Value::Int(slot));
    }
    t
}

#[test]
fn by_tempo_changes_on_bar_lines() {
    let s = spec(|_| {});
    let tempo = TempoMap::new(120.0, 4, 4);
    let c = SlotClock::changes_until(&s, 4, 1, &tempo, 4 * BAR + 1, &[]);
    assert_eq!(c, [(BAR, 1), (2 * BAR, 2), (3 * BAR, 3), (4 * BAR, 0)]);
    // Every 2 beats, 6/8: a beat is an eighth (0.25 s), so a change every 0.5 s.
    let s = spec(|v| {
        v.notes_every_unit = BeatsOrBars::Beats;
        v.notes_every_beats = 2;
    });
    let c = SlotClock::changes_until(&s, 3, 1, &TempoMap::new(120.0, 6, 8), 48_000 + 1, &[]);
    assert_eq!(c, [(24_000, 1), (48_000, 2)]);
    // By time, every 1.5 s.
    let s = spec(|v| {
        v.notes_change = Change::ByTime;
        v.notes_every_seconds = 1.5;
    });
    let c = SlotClock::changes_until(&s, 2, 1, &tempo, 3 * 48_000 + 1, &[]);
    assert_eq!(c, [(72_000, 1), (144_000, 0)]);
    // Only on keyframes: nothing moves on its own.
    let s = spec(|v| v.notes_change = Change::OnlyKeyframes);
    assert!(SlotClock::changes_until(&s, 4, 1, &tempo, 10 * BAR, &[]).is_empty());
}

#[test]
fn by_hits_marks_follow_the_hits() {
    let s = spec(|v| {
        v.notes_change = Change::ByHits;
        v.notes_every_hits = 2;
    });
    let c = SlotClock::changes_until(&s, 3, 1, &TempoMap::default(), 48_000, &[100, 200, 300, 400, 500]);
    assert_eq!(c, [(200, 1), (400, 2)], "after every second hit, on its sample");
}

#[test]
fn orders() {
    let tempo = TempoMap::default();
    let seq = |order: SlotOrder, n: usize| -> Vec<usize> {
        let s = spec(|v| v.notes_order = order);
        SlotClock::changes_until(&s, n, 7, &tempo, 8 * BAR + 1, &[]).into_iter().map(|(_, slot)| slot).collect()
    };
    assert_eq!(seq(SlotOrder::Forward, 3), [1, 2, 0, 1, 2, 0, 1, 2]);
    assert_eq!(seq(SlotOrder::Reverse, 3), [2, 1, 0, 2, 1, 0, 2, 1]);
    // ping_pong_order: there and back, the ends once.
    assert_eq!(seq(SlotOrder::PingPong, 3), [1, 2, 1, 0, 1, 2, 1, 0]);
    assert!(seq(SlotOrder::PingPong, 1).is_empty(), "one slot never changes");
    let random = seq(SlotOrder::Random, 4);
    assert!(std::iter::once(0).chain(random.iter().copied()).collect::<Vec<_>>().windows(2).all(|w| w[0] != w[1]));
    assert_eq!(random, seq(SlotOrder::Random, 4), "the same every time");
    let shuffle = seq(SlotOrder::Shuffle, 4);
    for round in shuffle.chunks(4) {
        let mut r = round.to_vec();
        r.sort();
        assert_eq!(r, [0, 1, 2, 3], "{shuffle:?}");
    }
}

#[test]
fn jump_key_overrides_the_mode() {
    let tempo = TempoMap::default();
    let mut s = spec(|_| {});
    s.jump = Some(jumps(&[(0.5, 3), (5.0, 1)]));
    let c = SlotClock::changes_until(&s, 4, 1, &tempo, 3 * BAR + 1, &[]);
    // Jumps to slot 3 half a second in, then carries on from there on the bar lines; at 5 s
    // back to slot 1.
    assert_eq!(c, [(24_000, 2), (BAR, 3), (2 * BAR, 0), (5 * 48_000, 0), (3 * BAR, 1)]);
    // Only on keyframes moves only there; a key past the last slot plays the last.
    let mut s = spec(|v| v.notes_change = Change::OnlyKeyframes);
    s.jump = Some(jumps(&[(1.0, 2), (2.0, 9)]));
    assert_eq!(SlotClock::changes_until(&s, 3, 1, &tempo, 3 * 48_000, &[]), [(48_000, 1), (96_000, 2)]);
}

// The voice: the slot plays.

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap()
}

fn voiced(kit: &Kit, edit: impl FnOnce(&mut VoiceSettings)) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    edit(&mut v);
    let mut params: Params = v.to_params();
    for (k, val) in card.params.iter() {
        if params.get(k).is_none() {
            params.set(k, val.clone());
        }
    }
    card.params = params;
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

fn voice(scene: &Scene, kit: &Kit) -> ShapeVoice {
    let resolved = resolve_scene(scene, &kit.registry);
    let shape = shape_of(scene);
    shape_voice(scene, shape, &resolved.things[&shape].slots[&SlotId::Sound], 0, 1080.0).unwrap()
}

fn world(scene: &Scene, kit: &Kit) -> Playback {
    Playback::new(scene, &resolve_scene(scene, &kit.registry), kit.sims.clone())
}

fn hit(scene: &Scene, offset: u32, speed: f32, on_shape: bool) -> BlockHit {
    let other = ThingId(999);
    BlockHit {
        offset,
        event: Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ball_of(scene), item: 0 },
            b: Some(Party { thing: if on_shape { shape_of(scene) } else { other }, item: 0 }),
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed,
            at: 0.0,
        },
    }
}

/// Runs block `b` of `v` with `hits`.
fn block(v: &mut ShapeVoice, w: &World, b: u64, hits: &[BlockHit]) {
    let tempo = TempoMap::default();
    let cx = BlockCx {
        start_sample: b * BLOCK as u64,
        time: (b * BLOCK as u64) as f64 / 48_000.0,
        tempo: &tempo,
        world: w,
        playing: true,
    };
    let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
    v.on_block(&cx, hits, &mut l, &mut r);
}

fn two_slots(v: &mut VoiceSettings) {
    v.notes_slots =
        vec![NoteSlot { notes: vec![60], order_clicked: vec![] }, NoteSlot { notes: vec![64], order_clicked: vec![] }];
}

#[test]
fn a_change_and_a_hit_on_one_sample_uses_the_new_slot() {
    let kit = kit();
    let scene = voiced(&kit, two_slots);
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    // The bar line at 96000 is the first sample of block 1500.
    for b in 0..1500 {
        block(&mut v, p.world(), b, &[]);
    }
    assert_eq!(v.slot(), 0);
    block(&mut v, p.world(), 1500, &[hit(&scene, 0, 1500.0, true)]);
    assert_eq!(v.slot(), 1);
    assert!(v.slot_changed());
    assert_eq!(v.playing_notes(), [64], "the hit on the bar line plays the new slot");
}

#[test]
fn by_hits_counts_accepted_hits_only() {
    let kit = kit();
    let scene = voiced(&kit, |v| {
        two_slots(v);
        v.notes_change = Change::ByHits;
        v.notes_every_hits = 2;
        v.play_min_strength = 50.0;
        v.play_retrigger = 0.0;
    });
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    let soft = hit(&scene, 0, 100.0, true);
    let elsewhere = hit(&scene, 0, 1500.0, false);
    let hard = hit(&scene, 0, 1500.0, true);
    block(&mut v, p.world(), 0, &[soft.clone(), elsewhere.clone(), hard.clone()]);
    assert_eq!(v.slot(), 0, "one accepted hit of two");
    block(&mut v, p.world(), 10, &[soft, elsewhere]);
    assert_eq!(v.slot(), 0, "soft and other shapes' hits don't count");
    block(&mut v, p.world(), 20, std::slice::from_ref(&hard));
    assert_eq!(v.slot(), 1, "the second accepted hit moves it on");
    assert!(v.playing_notes().contains(&60), "that hit still played slot 1's note");
    block(&mut v, p.world(), 30, &[hard]);
    assert!(v.playing_notes().contains(&64));
}

#[test]
fn empty_slot_plays_nothing() {
    let kit = kit();
    let scene = voiced(&kit, |v| {
        v.notes_slots = vec![NoteSlot { notes: vec![60], order_clicked: vec![] }, NoteSlot::default()];
        v.notes_plays = Plays::Up;
    });
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    for b in 0..1500 {
        block(&mut v, p.world(), b, &[]);
    }
    block(&mut v, p.world(), 1500, &[hit(&scene, 10, 1500.0, true)]);
    assert_eq!(v.slot(), 1);
    assert!(v.playing_notes().is_empty(), "no fallback to another slot");
}

#[test]
fn two_shapes_change_together_after_a_seek() {
    // Played from the start, and started at second 5 after a seek (caught up over the hits
    // before it): the same slot on every block from there, changing on the bar lines.
    let kit = kit();
    let scene = voiced(&kit, |v| {
        v.notes_slots = (0..3).map(|i| NoteSlot { notes: vec![60 + i], order_clicked: vec![] }).collect();
        v.notes_order = SlotOrder::Shuffle;
    });
    let p = world(&scene, &kit);
    let tempo = TempoMap::default();
    let mut from_start = voice(&scene, &kit);
    let mut seeked = voice(&scene, &kit);
    let seek_block = 5 * 48_000 / BLOCK as u64;
    for b in 0..seek_block {
        block(&mut from_start, p.world(), b, &[]);
    }
    seeked.reset();
    seeked.catch_up(&[], seek_block * BLOCK as u64, &tempo, p.world());
    let mut changes = 0;
    for b in seek_block..seek_block + 10 * 750 {
        block(&mut from_start, p.world(), b, &[]);
        block(&mut seeked, p.world(), b, &[]);
        assert_eq!(from_start.slot(), seeked.slot(), "block {b}");
        assert_eq!(from_start.slot_changed(), seeked.slot_changed());
        if seeked.slot_changed() {
            assert_eq!((b * BLOCK as u64) % BAR, 0, "on a bar line");
            changes += 1;
        }
    }
    assert!(changes >= 4);
}

#[test]
fn up_order_after_a_seek_matches_from_the_start() {
    // The real engine: Up order with real hits, sought to 6 s (past the 4 s the seek renders
    // again), hears the notes it would have.
    let kit = kit();
    // The hits on the shape before 2 s (where the seek's re-render starts): the note count is
    // picked so they don't come out even, so a voice that forgot them would be off.
    let plain = voiced(&kit, |_| {});
    let mut p = world(&plain, &kit);
    p.seek(2.0);
    let shape = shape_of(&plain);
    let early =
        p.log().events().iter().filter(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == shape)).count();
    let count = [3usize, 5, 7].into_iter().find(|n| early % n != 0).expect("some count doesn't divide");
    assert!(early > 0, "the ball hits the shape in the first 2 s");
    let scene = voiced(&kit, |v| {
        v.notes_slots = vec![NoteSlot { notes: (0..count as u8).map(|i| 60 + 2 * i).collect(), order_clicked: vec![] }];
        v.notes_plays = Plays::Up;
        v.play_retrigger = 0.0;
    });
    let resolved = resolve_scene(&scene, &kit.registry);
    let render = |seek: Option<f64>| -> Vec<f32> {
        let mut e = bpe_sound::Engine::new(&scene, &resolved, &kit);
        if let Some(t) = seek {
            e.seek(t);
        }
        let mut out = Vec::new();
        let mut buf = [0.0f32; BLOCK * 2];
        while out.len() < 2 * 48_000 * 2 {
            e.render(&mut buf);
            out.extend_from_slice(&buf);
        }
        out
    };
    let mut whole = bpe_sound::Engine::new(&scene, &resolved, &kit);
    let mut all = Vec::new();
    let mut buf = [0.0f32; BLOCK * 2];
    while all.len() < 8 * 48_000 * 2 {
        whole.render(&mut buf);
        all.extend_from_slice(&buf);
    }
    let later = render(Some(6.0));
    let from = 6 * 48_000 * 2;
    let n = later.len().min(all.len() - from);
    let diff = (0..n).map(|i| (all[from + i] - later[i]).abs()).fold(0.0f32, f32::max);
    assert!(all[from..from + n].iter().any(|x| x.abs() > 0.01), "hits play after 6 s");
    assert!(diff < 1e-3, "the same sound after the seek ({diff})");
    let _ = render(None);
}

#[test]
fn a_strummed_note_after_a_hit_still_ends_at_the_next_hit() {
    let kit = kit();
    let mut scene = voiced(&kit, |v| {
        v.notes_slots = vec![NoteSlot { notes: vec![60, 64, 67], order_clicked: vec![] }];
        v.notes_plays = Plays::AllAtOnce;
        v.notes_strum = 100.0;
        v.play_voices = 16;
        v.play_retrigger = 0.0;
    });
    // Every envelope holds at full level until the next hit.
    let shape = shape_of(&scene);
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    for id in card.layers.iter().filter(|l| l.kind == "mod.adsr").map(|l| l.id).collect::<Vec<_>>() {
        card.params.set(&bpe_core::layered_key(id, "gate_mode"), Value::Choice("until_next_hit".into()));
        card.params.set(&bpe_core::layered_key(id, "sustain"), Value::Number(100.0));
    }
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    // Hit A strums C E G 50 ms apart: E starts on sample 2400 (block 37, offset 32). Hit B
    // lands in that block before E has started.
    block(&mut v, p.world(), 0, &[hit(&scene, 0, 1500.0, true)]);
    for b in 1..37 {
        block(&mut v, p.world(), b, &[]);
    }
    block(&mut v, p.world(), 37, &[hit(&scene, 0, 1500.0, true)]);
    for b in 38..400 {
        block(&mut v, p.world(), b, &[]);
    }
    // Hit C closes every gate still open; after the release only C's own chord is left.
    block(&mut v, p.world(), 400, &[hit(&scene, 0, 1500.0, true)]);
    for b in 401..1200 {
        block(&mut v, p.world(), b, &[]);
    }
    assert_eq!(v.playing_notes(), [60, 64, 67], "no note of A's strum is left holding");
}

/// `scene` with Jump to slot keys at `keys` (seconds, slot).
fn with_jumps(mut scene: Scene, kit: &Kit, keys: &[(f64, i64)]) -> Scene {
    let a = bpe_core::Address::on(shape_of(&scene), SlotId::Sound, "notes_jump");
    scene.tracks.remove(&a);
    for &(time, slot) in keys {
        scene.set_key(&kit.registry, &a, time, Value::Int(slot)).unwrap();
    }
    scene
}

fn three_slots(v: &mut VoiceSettings) {
    v.notes_slots = (0..3).map(|i| NoteSlot { notes: vec![60 + i], order_clicked: vec![] }).collect();
    v.notes_change = Change::OnlyKeyframes;
}

#[test]
fn editing_jump_keys_while_playing_keeps_the_next_jump() {
    let kit = kit();
    let base = voiced(&kit, three_slots);
    let p = world(&base, &kit);
    // 0.5 s is block 375; 1 s is block 750.
    let run = |v: &mut ShapeVoice, from: u64, to: u64| {
        for b in from..to {
            block(v, p.world(), b, &[]);
        }
    };
    // A past key is deleted: the next one still comes.
    let before = with_jumps(base.clone(), &kit, &[(0.2, 2), (1.0, 3)]);
    let mut old = voice(&before, &kit);
    run(&mut old, 0, 375);
    assert_eq!(old.slot(), 1);
    let after = with_jumps(base.clone(), &kit, &[(1.0, 3)]);
    let mut new = voice(&after, &kit);
    new.adopt(&mut old);
    run(&mut new, 375, 760);
    assert_eq!(new.slot(), 2, "the key at 1 s still jumps to slot 3");
    // A key added before the playhead does not fire at once.
    let before = with_jumps(base.clone(), &kit, &[(1.0, 3)]);
    let mut old = voice(&before, &kit);
    run(&mut old, 0, 375);
    let after = with_jumps(base.clone(), &kit, &[(0.2, 2), (1.0, 3)]);
    let mut new = voice(&after, &kit);
    new.adopt(&mut old);
    run(&mut new, 375, 376);
    assert_eq!(new.slot(), 0, "a key behind the playhead waits for the next pass");
    run(&mut new, 376, 760);
    assert_eq!(new.slot(), 2);
}
