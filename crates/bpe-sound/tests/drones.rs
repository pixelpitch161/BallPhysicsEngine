//! Plays: Always holds the slot's notes as a drone from the start, In range holds them while a
//! ball is near a Distance, and a drone follows the slot as it changes.

use bpe_core::{LayerId, NoteSlot, ROUTES_KEY, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_kit::sound::{Change, PlayWhen, VoiceSettings, Wave, starter_voice};
use bpe_kit::{Kit, kit, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Party, World};
use bpe_sound::patch::shape_voice;
use bpe_sound::{BLOCK, BlockCx, BlockHit, ShapeVoice, TempoMap, VoiceSource};
use glam::Vec2;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap()
}

/// The test scene's shape with the starter sound playing `when`, its slots `slots`, `edit`
/// done to its settings.
fn scene_with(kit: &Kit, when: PlayWhen, slots: &[&[u8]], edit: impl FnOnce(&mut VoiceSettings)) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let shape = shape_of(&scene);
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    v.a.wave = Wave::Sine;
    v.play_when = when;
    v.notes_slots = slots.iter().map(|n| NoteSlot { notes: n.to_vec(), order_clicked: vec![] }).collect();
    edit(&mut v);
    let routes = v.routes.clone();
    let layers = card.layers.clone();
    let mut params = v.to_params();
    params.set(ROUTES_KEY, Value::Routes(routes));
    params.set(&layered_key(LayerId(0), "sustain"), Value::Number(100.0));
    params.set(&layered_key(LayerId(0), "release"), Value::Number(50.0));
    // Distance 1 measures from the shape's middle: 1 within 50 px, 0 past 200 px.
    params.set(&layered_key(LayerId(2), "near"), Value::Number(50.0));
    params.set(&layered_key(LayerId(2), "far"), Value::Number(200.0));
    card.params = params;
    card.layers = layers;
    card.layers.iter_mut().find(|l| l.id == LayerId(2)).unwrap().enabled = true;
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

fn voice(scene: &Scene, kit: &Kit) -> ShapeVoice {
    let resolved = resolve_scene(scene, &kit.registry);
    let shape = shape_of(scene);
    let card = &resolved.things[&shape].slots[&SlotId::Sound];
    shape_voice(scene, shape, card, 0, 1080.0).expect("its sound is on")
}

fn world(scene: &Scene, kit: &Kit) -> World {
    let resolved = resolve_scene(scene, &kit.registry);
    World::build(scene, &resolved, &kit.sims)
}

fn put(world: &mut World, ball: ThingId, p: Vec2) {
    let b = world.bodies.get_mut(&ball).unwrap();
    b.position[0] = p;
    b.previous[0] = p;
}

fn hit(ball: ThingId, shape: ThingId) -> BlockHit {
    BlockHit {
        offset: 0,
        event: Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ball, item: 0 },
            b: Some(Party { thing: shape, item: 0 }),
            point: Vec2::new(508.0, 0.0),
            normal: Vec2::X,
            speed: 1500.0,
            at: 0.0,
        },
    }
}

/// Runs blocks `from..from + n` with `hits` (by block). Returns the loudest sample and what
/// sounded after the last block.
fn run(v: &mut ShapeVoice, w: &World, from: u64, n: u64, hits: &[(u64, BlockHit)]) -> (f32, Vec<u8>) {
    run_as(v, w, from, n, hits, true)
}

/// As [`run`], playing or (`playing` false) stopped.
fn run_as(v: &mut ShapeVoice, w: &World, from: u64, n: u64, hits: &[(u64, BlockHit)], playing: bool) -> (f32, Vec<u8>) {
    let tempo = TempoMap::default();
    let mut peak = 0.0f32;
    for b in from..from + n {
        let theirs: Vec<BlockHit> = hits.iter().filter(|(at, _)| *at == b).map(|(_, h)| h.clone()).collect();
        let cx = BlockCx {
            start_sample: b * BLOCK as u64,
            time: (b * BLOCK as u64) as f64 / 48_000.0,
            tempo: &tempo,
            world: w,
            playing,
        };
        let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
        v.on_block(&cx, &theirs, &mut l, &mut r);
        peak = l.iter().fold(peak, |m, s| m.max(s.abs()));
    }
    let mut playing = v.playing_notes();
    playing.sort();
    (peak, playing)
}

#[test]
fn always_drones_from_the_start() {
    let kit = kit();
    let scene = scene_with(&kit, PlayWhen::Always, &[&[60, 64]], |_| {});
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    let (peak, playing) = run(&mut v, &w, 0, 4, &[]);
    assert!(peak > 0.01, "it sounds with no hits: {peak}");
    assert_eq!(playing, [60, 64], "both notes held");
    // Ten seconds on (a 100 ms gate would long have closed), still held.
    let (peak, playing) = run(&mut v, &w, 4, 7500, &[]);
    assert!(peak > 0.01);
    assert_eq!(playing, [60, 64]);
    // A hit starts no notes of its own.
    let h = [(7600, hit(ball_of(&scene), shape_of(&scene)))];
    let (_, playing) = run(&mut v, &w, 7504, 200, &h);
    assert_eq!(playing, [60, 64]);
}

#[test]
fn always_with_empty_slot_is_silent_until_a_note() {
    let kit = kit();
    let empty = scene_with(&kit, PlayWhen::Always, &[&[]], |_| {});
    let (mut v, w) = (voice(&empty, &kit), world(&empty, &kit));
    let (peak, playing) = run(&mut v, &w, 0, 200, &[]);
    assert_eq!(peak, 0.0, "nothing to hold");
    assert!(playing.is_empty());
    assert_eq!(v.sounding(), 0, "no stuck voice");
    // A note added to the slot (a patch swap) starts the drone.
    let one = scene_with(&kit, PlayWhen::Always, &[&[67]], |_| {});
    let mut next = voice(&one, &kit);
    next.adopt(&mut v);
    let (peak, playing) = run(&mut next, &w, 200, 20, &[]);
    assert!(peak > 0.01);
    assert_eq!(playing, [67]);
    // Back to On hit: the drone lets go.
    let on_hit = scene_with(&kit, PlayWhen::OnHit, &[&[67]], |_| {});
    let mut last = voice(&on_hit, &kit);
    last.adopt(&mut next);
    let _ = run(&mut last, &w, 220, 400, &[]);
    assert_eq!(last.sounding(), 0, "released");
}

#[test]
fn in_range_holds_while_a_ball_is_close() {
    let kit = kit();
    let scene = scene_with(&kit, PlayWhen::InRange, &[&[60]], |_| {});
    let ball = ball_of(&scene);
    let (mut v, mut w) = (voice(&scene, &kit), world(&scene, &kit));
    put(&mut w, ball, Vec2::new(0.0, -400.0));
    let (peak, _) = run(&mut v, &w, 0, 100, &[]);
    assert_eq!(peak, 0.0, "out of range: silent");
    put(&mut w, ball, Vec2::new(0.0, -100.0));
    let (peak, playing) = run(&mut v, &w, 100, 100, &[]);
    assert!(peak > 0.01, "in range: held");
    assert_eq!(playing, [60]);
    let (_, playing) = run(&mut v, &w, 200, 3000, &[]);
    assert_eq!(playing, [60], "held as long as the ball stays");
    put(&mut w, ball, Vec2::new(0.0, -400.0));
    let _ = run(&mut v, &w, 3200, 400, &[]);
    assert_eq!(v.sounding(), 0, "released once it leaves");
}

#[test]
fn drone_follows_slot_changes() {
    let kit = kit();
    let scene = scene_with(&kit, PlayWhen::Always, &[&[60, 64], &[67]], |v| {
        v.notes_change = Change::ByHits;
        v.notes_every_hits = 1;
    });
    let (ball, shape) = (ball_of(&scene), shape_of(&scene));
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    let (_, playing) = run(&mut v, &w, 0, 10, &[]);
    assert_eq!(playing, [60, 64]);
    let (_, playing) = run(&mut v, &w, 10, 400, &[(20, hit(ball, shape))]);
    assert_eq!(playing, [67], "the next slot's notes");
    // Mono glides to the new slot's first note on the same voice.
    let mono = scene_with(&kit, PlayWhen::Always, &[&[60], &[67]], |v| {
        v.notes_change = Change::ByHits;
        v.notes_every_hits = 1;
        v.play_mode = bpe_kit::sound::PlayMode::Mono;
        v.play_glide = 100.0;
    });
    let (mut v, w) = (voice(&mono, &kit), world(&mono, &kit));
    let _ = run(&mut v, &w, 0, 10, &[]);
    let (_, playing) = run(&mut v, &w, 10, 100, &[(20, hit(ball, shape))]);
    assert_eq!(playing, [67]);
    assert_eq!(v.sounding(), 1, "one voice, gliding");
}

#[test]
fn a_drone_on_the_starter_envelope_rings_once_and_stays_held() {
    let kit = kit();
    let mut scene = scene_with(&kit, PlayWhen::Always, &[&[60]], |_| {});
    // The starter ADSR: Sustain 0, so the note dies away after its decay.
    let shape = shape_of(&scene);
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set(&layered_key(LayerId(0), "sustain"), Value::Number(0.0));
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    let (peak, _) = run(&mut v, &w, 0, 4, &[]);
    assert!(peak > 0.01, "it sounds at the start: {peak}");
    run(&mut v, &w, 4, 750, &[]);
    // From one second on it stays silent: no plucking again by itself.
    let (peak, playing) = run(&mut v, &w, 754, 1500, &[]);
    assert!(peak < 1e-4, "silent after its decay, not plucked again: {peak}");
    assert_eq!(playing, [60], "still held");
    // A hit doesn't play it again: the note is only held.
    let h = [(2300, hit(ball_of(&scene), shape))];
    let (peak, playing) = run(&mut v, &w, 2254, 100, &h);
    assert!(peak < 1e-4, "a hit doesn't pluck the held note: {peak}");
    assert_eq!(playing, [60]);
}

/// The loudest sample of each run of four blocks in `from..from + n` (four blocks hold more than
/// a whole wave of the notes played here).
fn block_peaks(v: &mut ShapeVoice, w: &World, from: u64, n: u64, hits: &[(u64, BlockHit)]) -> Vec<f32> {
    (from..from + n).step_by(4).map(|b| run(v, w, b, 4, hits).0).collect()
}

#[test]
fn hits_dont_retrigger_a_held_note() {
    let kit = kit();
    // Sustain 50 %: a retrigger would jump back up to full before falling to the sustain again.
    let mut scene = scene_with(&kit, PlayWhen::Always, &[&[60]], |_| {});
    let (ball, shape) = (ball_of(&scene), shape_of(&scene));
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set(&layered_key(LayerId(0), "sustain"), Value::Number(50.0));
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    run(&mut v, &w, 0, 1500, &[]);
    let before = block_peaks(&mut v, &w, 1500, 100, &[]);
    let hits: Vec<(u64, BlockHit)> = (0..10).map(|k| (1600 + 10 * k, hit(ball, shape))).collect();
    let after = block_peaks(&mut v, &w, 1600, 200, &hits);
    let steady = before.iter().cloned().fold(0.0f32, f32::max);
    assert!(steady > 0.01, "it sounds at its sustain: {steady}");
    for (b, p) in after.iter().enumerate() {
        assert!((p - steady).abs() < steady * 0.02, "block {b} after the hits: {p} against {steady}");
    }
    assert_eq!(v.sounding(), 1, "one note, held");
}

#[test]
fn a_drone_waits_while_stopped_and_a_clicked_key_ends() {
    let kit = kit();
    let scene = scene_with(&kit, PlayWhen::Always, &[&[60]], |_| {});
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    // Stopped: no drone.
    let (peak, playing) = run_as(&mut v, &w, 0, 200, &[], false);
    assert_eq!(peak, 0.0, "a drone doesn't play while stopped");
    assert!(playing.is_empty());
    // A key clicked while stopped plays, then lets go by itself (its sustain is 100 %).
    v.audition(64);
    let (peak, _) = run_as(&mut v, &w, 200, 10, &[], false);
    assert!(peak > 0.01, "the clicked key sounds: {peak}");
    let _ = run_as(&mut v, &w, 210, 600, &[], false);
    assert_eq!(v.sounding(), 0, "the clicked key ended instead of ringing forever");
    // Playing again: the drone holds.
    let (peak, playing) = run(&mut v, &w, 810, 50, &[]);
    assert!(peak > 0.01);
    assert_eq!(playing, [60]);
    // And a key clicked while it plays ends too, leaving the drone.
    v.audition(67);
    let _ = run(&mut v, &w, 860, 600, &[]);
    assert_eq!(v.playing_notes(), [60]);
    assert_eq!(v.sounding(), 1);
}

#[test]
fn a_drone_a_seek_started_is_never_heard_while_stopped() {
    let kit = kit();
    let scene = scene_with(&kit, PlayWhen::Always, &[&[60]], |_| {});
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    // A seek's pre-roll plays (unheard) and starts the drone...
    let (peak, _) = run(&mut v, &w, 0, 50, &[]);
    assert!(peak > 0.01);
    // ...then the editor, still stopped, renders for a clicked key: no burst of the drone.
    let (peak, playing) = run_as(&mut v, &w, 50, 1, &[], false);
    assert_eq!(peak, 0.0, "the drone stays quiet while stopped");
    assert!(playing.is_empty());
}

#[test]
fn a_clicked_key_plays_a_fixed_gate_as_set() {
    let kit = kit();
    let mut scene = scene_with(&kit, PlayWhen::OnHit, &[&[60]], |_| {});
    let shape = shape_of(&scene);
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set(&layered_key(LayerId(0), "gate_ms"), Value::Number(1000.0));
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    v.audition(64);
    // 0.6 s on, past the 0.4 s a key that waits for a hit gets, the 1 s gate still holds it.
    let (_, playing) = run_as(&mut v, &w, 0, 450, &[], false);
    assert_eq!(playing, [64]);
    let _ = run_as(&mut v, &w, 450, 600, &[], false);
    assert_eq!(v.sounding(), 0, "and it ends after its gate");
}

#[test]
fn a_mono_drone_moves_to_the_next_slot_legato() {
    let kit = kit();
    let mut scene = scene_with(&kit, PlayWhen::Always, &[&[60], &[67]], |v| {
        v.notes_change = Change::ByHits;
        v.notes_every_hits = 1;
        v.play_mode = bpe_kit::sound::PlayMode::Mono;
    });
    let (ball, shape) = (ball_of(&scene), shape_of(&scene));
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set(&layered_key(LayerId(0), "sustain"), Value::Number(50.0));
    let (mut v, w) = (voice(&scene, &kit), world(&scene, &kit));
    run(&mut v, &w, 0, 1500, &[]);
    let steady = block_peaks(&mut v, &w, 1500, 50, &[]).into_iter().fold(0.0f32, f32::max);
    let after = block_peaks(&mut v, &w, 1550, 100, &[(1560, hit(ball, shape))]);
    assert_eq!(v.playing_notes(), [67]);
    assert!(after.iter().all(|p| (p - steady).abs() < steady * 0.05), "no new attack: {after:?} against {steady}");
}

#[test]
fn a_switched_off_modulator_leaves_its_knobs_alone() {
    let kit = kit();
    // A Distance scaling Osc A's Gain all the way, then switched off: Gain should be as set,
    // not silenced by a Distance that reads nothing.
    let mut scene = scene_with(&kit, PlayWhen::OnHit, &[&[60]], |v| {
        v.routes.push(bpe_core::ModRoute {
            source: LayerId(2),
            target: "a_gain".into(),
            amount: 1.0,
            bipolar: false,
            invert: false,
        });
    });
    let (ball, shape) = (ball_of(&scene), shape_of(&scene));
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.layers.iter_mut().find(|l| l.id == LayerId(2)).unwrap().enabled = false;
    let (mut v, mut w) = (voice(&scene, &kit), world(&scene, &kit));
    // The ball far away: the Distance, were it on, would read 0.
    put(&mut w, ball, Vec2::new(0.0, -2000.0));
    let (peak, _) = run(&mut v, &w, 0, 40, &[(2, hit(ball, shape))]);
    assert!(peak > 0.01, "the hit sounds: {peak}");
}
