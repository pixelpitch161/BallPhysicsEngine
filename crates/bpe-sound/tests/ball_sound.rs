//! W10: a Balls or Emitter thing's Sound card plays its own balls: what they hit (a shape, a
//! ball), an emitter firing them, a catcher taking them, a rule breaking them. Shapes keep
//! hearing exactly what they heard before.

use bpe_core::{Params, Scene, Settings, SlotId, Target, ThingId, Value};
use bpe_kit::sound::{VoiceSettings, starter_voice};
use bpe_kit::things::emitter::{Catches, Directions, EmitterSettings, StartFrom};
use bpe_kit::{Kit, add_thing, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Log, Party, Playback, World};
use bpe_sound::hits::hit_sample;
use bpe_sound::patch::shape_voice;
use bpe_sound::{BLOCK, BlockCx, BlockHit, ControlSampler, Engine, NoteOn, ShapeVoice, TempoMap, VoiceSource};
use bpe_core::SoundSource;
use glam::Vec2;

const SHAPE: ThingId = ThingId(1);
const BALLS: ThingId = ThingId(2);

/// Puts the starter sound on `id`, its settings changed by `edit` (the retrigger guard off, so
/// every hit counts).
fn give_sound(scene: &mut Scene, kit: &Kit, id: ThingId, edit: impl FnOnce(&mut VoiceSettings)) {
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    v.play_retrigger = 0.0;
    edit(&mut v);
    let mut params: Params = v.to_params();
    for (k, val) in card.params.iter() {
        if params.get(k).is_none() {
            params.set(k, val.clone());
        }
    }
    card.params = params;
    scene.things.get_mut(&id).unwrap().slots.insert(SlotId::Sound, card);
}

/// The new scene (a ball dropping into a hollow circle) plus an emitter that fires nothing,
/// with sound on `id` set by `edit`.
fn scene_with(kit: &Kit, id: ThingId, edit: impl FnOnce(&mut VoiceSettings)) -> (Scene, ThingId) {
    let mut scene = new_scene(&kit.registry);
    let emitter = add_thing(&mut scene, &kit.registry, bpe_kit::things::emitter::KIND, ThingId::ROOT).unwrap();
    let s = EmitterSettings { emits: false, ..EmitterSettings::default() };
    scene.things.get_mut(&emitter).unwrap().params = s.to_params();
    give_sound(&mut scene, kit, if id == ThingId::ROOT { emitter } else { id }, edit);
    (scene, emitter)
}

fn voice_of(scene: &Scene, kit: &Kit, id: ThingId) -> ShapeVoice {
    let resolved = resolve_scene(scene, &kit.registry);
    let card = &resolved.things[&id].slots[&SlotId::Sound];
    let mut v = shape_voice(scene, id, card, 0, 1080.0).expect("its sound is on");
    v.record_notes(true);
    v
}

fn event(kind: EventKind, a: (ThingId, u32), b: Option<(ThingId, u32)>) -> Event {
    Event {
        step: 1,
        kind,
        a: Party { thing: a.0, item: a.1 },
        b: b.map(|(thing, item)| Party { thing, item }),
        point: Vec2::ZERO,
        normal: Vec2::Y,
        speed: 1000.0,
        at: 0.0,
    }
}

/// How many notes `v` starts for each event in turn, each in a block of its own.
fn notes_for(v: &mut ShapeVoice, world: &World, events: &[Event]) -> Vec<usize> {
    let tempo = TempoMap::default();
    let log = Log::default();
    let mut out = Vec::new();
    let mut notes: Vec<NoteOn> = Vec::new();
    for (i, e) in events.iter().enumerate() {
        // Far apart, so no note is still sounding from the last.
        let start = (i as u64 + 1) * 48_000;
        let cx = BlockCx { start_sample: start, time: start as f64 / 48_000.0, tempo: &tempo, world, log: &log, playing: true };
        let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
        v.on_block(&cx, &[BlockHit { offset: 0, event: e.clone() }], &mut l, &mut r);
        notes.clear();
        v.take_notes(&mut notes);
        out.push(notes.iter().filter(|n| n.sample >= start).count());
    }
    out
}

fn world_of(scene: &Scene, kit: &Kit) -> Playback {
    let resolved = resolve_scene(scene, &kit.registry);
    Playback::new(scene, &resolved, kit.sims.clone())
}

#[test]
fn a_balls_sound_plays_its_own_balls_hits_from_either_side() {
    let kit = bpe_sound::kit();
    let (scene, emitter) = scene_with(&kit, BALLS, |_| {});
    let p = world_of(&scene, &kit);
    let mut v = voice_of(&scene, &kit, BALLS);
    let got = notes_for(
        &mut v,
        p.world(),
        &[
            event(EventKind::Hit, (BALLS, 0), Some((SHAPE, 0))),
            event(EventKind::Hit, (emitter, 4), Some((BALLS, 0))),
            event(EventKind::Hit, (BALLS, 0), Some((BALLS, 1))),
            event(EventKind::Hit, (emitter, 4), Some((SHAPE, 0))),
            event(EventKind::Moment, (SHAPE, 0), Some((BALLS, 0))),
        ],
    );
    assert_eq!(got, [1, 1, 1, 0, 0], "its ball on a shape, met by another ball, two of its own; not others'");
}

#[test]
fn hit_a_shape_and_hit_a_ball_choose_what_plays() {
    let kit = bpe_sound::kit();
    let (scene, emitter) = scene_with(&kit, BALLS, |v| v.moments_hit_shape = false);
    let p = world_of(&scene, &kit);
    let hits = [
        event(EventKind::Hit, (BALLS, 0), Some((SHAPE, 0))),
        event(EventKind::Hit, (emitter, 4), Some((BALLS, 0))),
    ];
    assert_eq!(notes_for(&mut voice_of(&scene, &kit, BALLS), p.world(), &hits), [0, 1]);
    let (scene, _) = scene_with(&kit, BALLS, |v| v.moments_hit_ball = false);
    assert_eq!(notes_for(&mut voice_of(&scene, &kit, BALLS), p.world(), &hits), [1, 0]);
    // Heard from names what its balls must hit.
    let (scene, _) = scene_with(&kit, BALLS, |v| v.play_heard_from = Target::Thing(emitter));
    assert_eq!(notes_for(&mut voice_of(&scene, &kit, BALLS), p.world(), &hits), [0, 1]);
}

#[test]
fn fired_caught_and_broken_play_only_when_switched_on() {
    let kit = bpe_sound::kit();
    let events = |me: ThingId, other: ThingId| {
        [
            event(EventKind::Spawn, (me, 0), Some((me, 0))),
            event(EventKind::Enter, (me, 0), Some((other, 0))),
            event(EventKind::Enter, (other, 2), Some((me, 0))),
            event(EventKind::Break, (me, 0), None),
            event(EventKind::Remove, (me, 0), None),
        ]
    };
    let (scene, emitter) = scene_with(&kit, ThingId::ROOT, |_| {});
    let p = world_of(&scene, &kit);
    let off = notes_for(&mut voice_of(&scene, &kit, emitter), p.world(), &events(emitter, BALLS));
    assert_eq!(off, [0, 0, 0, 0, 0], "off at first");
    let (scene, emitter) = scene_with(&kit, ThingId::ROOT, |v| {
        v.moments_fire = true;
        v.moments_catch = true;
        v.moments_break = true;
    });
    let on = notes_for(&mut voice_of(&scene, &kit, emitter), p.world(), &events(emitter, BALLS));
    assert_eq!(on, [1, 1, 1, 1, 0], "fired, its ball caught, a ball it caught, broken");
    // Another thing's moments are not its own.
    let theirs = notes_for(&mut voice_of(&scene, &kit, emitter), p.world(), &events(BALLS, SHAPE));
    assert_eq!(theirs, [0, 0, 0, 0, 0]);
}

#[test]
fn a_shape_hears_what_it_heard_before() {
    let kit = bpe_sound::kit();
    let (scene, emitter) = scene_with(&kit, SHAPE, |v| {
        v.moments_fire = true;
        v.moments_catch = true;
        v.moments_break = true;
    });
    let p = world_of(&scene, &kit);
    let got = notes_for(
        &mut voice_of(&scene, &kit, SHAPE),
        p.world(),
        &[
            event(EventKind::Hit, (BALLS, 0), Some((SHAPE, 0))),
            event(EventKind::Hit, (SHAPE, 0), Some((BALLS, 0))),
            event(EventKind::Spawn, (emitter, 0), Some((SHAPE, 0))),
            event(EventKind::Enter, (BALLS, 0), Some((SHAPE, 0))),
            event(EventKind::Break, (SHAPE, u32::MAX), None),
        ],
    );
    assert_eq!(got, [1, 0, 0, 0, 0], "a ball hitting it, nothing else");
}

/// The notes each thing plays in the first `seconds`, by thing, as start samples.
fn played(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<(ThingId, u64)> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, kit);
    engine.record_notes(true);
    let mut block = [0.0f32; BLOCK * 2];
    let end = (seconds * 48_000.0) as u64;
    for _ in 0..(end as usize).div_ceil(BLOCK) {
        engine.render(&mut block);
    }
    engine.note_log().spans(end).into_iter().map(|s| (s.0, s.3)).collect()
}

/// The samples of `log`'s events that `pick` takes, before `end`.
fn samples(log: &Log, end: u64, pick: impl Fn(&Event) -> bool) -> Vec<u64> {
    let mut out: Vec<u64> =
        log.events().iter().filter(|e| pick(e)).map(|e| hit_sample(e, 200.0)).filter(|s| *s < end).collect();
    out.sort();
    out
}

#[test]
fn a_falling_ball_plays_on_each_of_its_hits() {
    let kit = bpe_sound::kit();
    let (scene, _) = scene_with(&kit, BALLS, |_| {});
    let mut p = world_of(&scene, &kit);
    p.seek(3.0);
    let end = 3 * 48_000 - 4_800;
    let hits = samples(p.log(), end, |e| e.kind == EventKind::Hit && (e.a.thing == BALLS || e.b.is_some_and(|b| b.thing == BALLS)));
    assert!(hits.len() >= 2, "the ball bounces: {hits:?}");
    let notes: Vec<u64> = played(&scene, &kit, 3.0).into_iter().filter(|(t, s)| *t == BALLS && *s < end).map(|n| n.1).collect();
    assert_eq!(notes, hits, "a note on each hit's own sample");
}

/// An emitter firing three balls one way at 0 s at a catcher 200 px off (no other balls), the
/// shooter's sound playing its fires when `fire` and the catcher's its catches.
fn shooter_and_catcher(kit: &Kit, fire: bool) -> (Scene, ThingId, ThingId) {
    let mut scene = new_scene(&kit.registry);
    scene.remove_thing(BALLS).unwrap();
    let mut add = |at: Vec2, s: EmitterSettings| {
        let id = add_thing(&mut scene, &kit.registry, bpe_kit::things::emitter::KIND, ThingId::ROOT).unwrap();
        let t = scene.things.get_mut(&id).unwrap();
        t.params = s.to_params();
        t.transform.position = at;
        id
    };
    let shooter = add(
        Vec2::ZERO,
        EmitterSettings {
            count: 3,
            directions: Directions::OneWay,
            aim_angle: 0.0,
            start_from: StartFrom::Line,
            start_length: 40.0,
            speed: 400.0,
            ..EmitterSettings::default()
        },
    );
    let catcher = add(
        Vec2::new(200.0, 0.0),
        EmitterSettings { emits: false, catches: Catches::Remove, catch_size: 30.0, ..EmitterSettings::default() },
    );
    give_sound(&mut scene, kit, shooter, |v| {
        v.moments_hit_ball = false;
        v.moments_hit_shape = false;
        v.moments_fire = fire;
    });
    give_sound(&mut scene, kit, catcher, |v| {
        v.moments_hit_ball = false;
        v.moments_hit_shape = false;
        v.moments_catch = true;
    });
    (scene, shooter, catcher)
}

#[test]
fn an_emitter_plays_each_ball_it_fires_and_each_catch() {
    let kit = bpe_sound::kit();
    let (scene, shooter, catcher) = shooter_and_catcher(&kit, true);
    let mut p = world_of(&scene, &kit);
    // The round at 0 s is there from the first frame: no step logs it, the sound plays it on
    // sample 0.
    let at_start = bpe_sound::hits::start_fires(&scene, p.world());
    assert_eq!(at_start.len(), 3);
    assert!(at_start.iter().all(|e| e.a.thing == shooter && e.speed > 399.0));
    p.seek(2.0);
    let end = 2 * 48_000 - 4_800;
    let mut fired = samples(p.log(), end, |e| e.kind == EventKind::Spawn && e.b.is_some_and(|b| b.thing == shooter));
    fired.splice(0..0, at_start.iter().map(|_| 0));
    let caught = samples(p.log(), end, |e| e.kind == EventKind::Enter && e.b.is_some_and(|b| b.thing == catcher));
    assert_eq!((fired.len(), caught.len()), (3, 3), "three fired, three caught");
    let notes = played(&scene, &kit, 2.0);
    let of = |id: ThingId| -> Vec<u64> { notes.iter().filter(|(t, s)| *t == id && *s < end).map(|n| n.1).collect() };
    assert_eq!(of(shooter), fired, "the shooter plays each ball it fires");
    assert_eq!(of(catcher), caught, "the catcher each ball it catches");
}

#[test]
fn follow_sound_hears_a_balls_sound() {
    let kit = bpe_sound::kit();
    let (mut scene, _) = scene_with(&kit, BALLS, |_| {});
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    let resolved = resolve_scene(&scene, &kit.registry);
    let sampler = ControlSampler::new(&scene, &resolved);
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    p.seek(3.0);
    let first = samples(p.log(), u64::MAX, |e| e.kind == EventKind::Hit && e.a.thing == BALLS)[0] as f64 / 48_000.0;
    let mut level = |t: f64| {
        p.seek(t);
        sampler.value(p.world(), p.log(), BALLS, &SoundSource::Level, t)
    };
    assert_eq!(level(first - 0.01), 0.0, "silent before its first hit");
    assert!(level(first + 0.02) > 0.05, "sounding just after it");
}

#[test]
fn a_new_balls_card_has_the_new_moments_off() {
    let v = VoiceSettings::default();
    assert!(v.moments_hit_ball && v.moments_hit_shape);
    assert!(!v.moments_fire && !v.moments_catch && !v.moments_break);
    let _ = Value::Bool(true);
}

#[test]
fn the_round_at_the_start_plays_again_after_a_seek_back() {
    let kit = bpe_sound::kit();
    let loud = |scene: &Scene, seek: bool| {
        let resolved = resolve_scene(scene, &kit.registry);
        let mut engine = Engine::new(scene, &resolved, &kit);
        let mut block = [0.0f32; BLOCK * 2];
        if seek {
            for _ in 0..200 {
                engine.render(&mut block);
            }
            engine.seek(0.0);
        }
        let mut most = 0.0f32;
        // The first 0.2 s: the balls are still on their way to the catcher.
        for _ in 0..150 {
            engine.render(&mut block);
            most = block.iter().fold(most, |m, v| m.max(v.abs()));
        }
        most
    };
    let (silent, ..) = shooter_and_catcher(&kit, false);
    assert_eq!(loud(&silent, false), 0.0, "nothing plays before the catch without Fired");
    let (scene, ..) = shooter_and_catcher(&kit, true);
    assert!(loud(&scene, false) > 0.01, "the round at 0 s plays");
    assert!(loud(&scene, true) > 0.01, "and again after going back to the start");
}
