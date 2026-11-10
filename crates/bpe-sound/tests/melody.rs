//! Plays Melody (each accepted hit plays the next step of a written melody) and Pitch follows size.

use bpe_core::{NoteSlot, Params, Scene, Settings, SlotId, ThingId};
use bpe_kit::sound::{MelodyEnd, Plays, VoiceSettings, starter_voice};
use bpe_kit::things::balls::BallsSettings;
use bpe_kit::{Kit, kit, new_scene, resolve_scene};
use bpe_runtime::{Event, EventKind, Party, Playback, World};
use bpe_sound::patch::shape_voice;
use bpe_sound::voices::pick::melody_step;
use bpe_sound::{BLOCK, BlockCx, BlockHit, ShapeVoice, TempoMap, VoiceSource};
use glam::Vec2;

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

fn hit(scene: &Scene) -> BlockHit {
    BlockHit {
        offset: 0,
        event: Event {
            step: 1,
            kind: EventKind::Hit,
            a: Party { thing: ball_of(scene), item: 0 },
            b: Some(Party { thing: shape_of(scene), item: 0 }),
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed: 1500.0,
            at: 0.0,
        },
    }
}

fn block(v: &mut ShapeVoice, w: &World, b: u64, hits: &[BlockHit]) {
    let tempo = TempoMap::default();
    let cx = BlockCx {
        start_sample: b * BLOCK as u64,
        time: (b * BLOCK as u64) as f64 / 48_000.0,
        tempo: &tempo,
        world: w,
        log: &bpe_runtime::Log::default(),
        playing: true,
    };
    let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
    v.on_block(&cx, hits, &mut l, &mut r);
}

fn steps(notes: &[&[u8]]) -> Vec<NoteSlot> {
    notes.iter().map(|n| NoteSlot { notes: n.to_vec(), order_clicked: vec![] }).collect()
}

fn melody(v: &mut VoiceSettings) {
    v.notes_plays = Plays::Melody;
    v.notes_melody = steps(&[&[60], &[62], &[64, 67]]);
    v.play_retrigger = 0.0;
}

#[test]
fn melody_step_wraps_and_holds() {
    let wrap: Vec<_> = (0..7).map(|h| melody_step(3, h, 1, MelodyEnd::Wrap).unwrap()).collect();
    assert_eq!(wrap, [0, 1, 2, 0, 1, 2, 0]);
    let hold: Vec<_> = (0..5).map(|h| melody_step(3, h, 1, MelodyEnd::Hold).unwrap()).collect();
    assert_eq!(hold, [0, 1, 2, 2, 2]);
    let twice: Vec<_> = (0..6).map(|h| melody_step(2, h, 2, MelodyEnd::Wrap).unwrap()).collect();
    assert_eq!(twice, [0, 0, 1, 1, 0, 0], "Repeat each holds a step for that many hits");
    assert_eq!(melody_step(0, 3, 1, MelodyEnd::Wrap), None);
    assert_eq!(melody_step(4, u64::MAX, 0, MelodyEnd::Hold), Some(3), "never past the end");
}

#[test]
fn each_hit_plays_the_next_melody_step() {
    let kit = kit();
    let scene = voiced(&kit, melody);
    let p = world(&scene, &kit);
    let mut v = voice(&scene, &kit);
    let mut played = Vec::new();
    for k in 0..5 {
        block(&mut v, p.world(), k * 3000, &[hit(&scene)]);
        played.push(v.playing_notes());
        for b in k * 3000 + 1..(k + 1) * 3000 {
            block(&mut v, p.world(), b, &[]);
        }
    }
    let last = played;
    assert!(last[0].contains(&60), "{last:?}");
    assert!(last[1].contains(&62), "{last:?}");
    assert!(last[2].contains(&64) && last[2].contains(&67), "the third step is a chord: {last:?}");
    assert!(last[3].contains(&60), "and round again: {last:?}");
    assert!(last[4].contains(&62), "{last:?}");
}

#[test]
fn a_seek_keeps_the_melodys_place() {
    // A voice caught up over two hits plays the third step on the next one.
    let kit = kit();
    let scene = voiced(&kit, melody);
    let p = world(&scene, &kit);
    let tempo = TempoMap::default();
    let mut seeked = voice(&scene, &kit);
    seeked.reset();
    let earlier: Vec<(u64, BlockHit)> = (0..2).map(|k| (k * 1000, hit(&scene))).collect();
    seeked.catch_up(&earlier, 48_000, &tempo, p.world());
    block(&mut seeked, p.world(), 48_000 / BLOCK as u64, &[hit(&scene)]);
    let now = seeked.playing_notes();
    assert!(now.contains(&64) && now.contains(&67), "{now:?}");
}

#[test]
fn a_bigger_ball_plays_lower() {
    let kit = kit();
    let pitch_at = |radius: f32, amount: f32| -> f32 {
        let mut scene = voiced(&kit, |v| {
            v.notes_plays = Plays::Up;
            v.notes_slots = steps(&[&[60]]);
            v.play_size_pitch = amount;
            v.play_size_at = 20.0;
        });
        let ball = ball_of(&scene);
        let mut s = BallsSettings::from_params(&scene.things[&ball].params);
        s.radius = radius;
        scene.things.get_mut(&ball).unwrap().params = s.to_params();
        let p = world(&scene, &kit);
        let mut v = voice(&scene, &kit);
        block(&mut v, p.world(), 0, &[hit(&scene)]);
        v.playing_pitches()[0]
    };
    assert_eq!(pitch_at(80.0, 0.0), 60.0, "off by default");
    assert!((pitch_at(20.0, 12.0) - 60.0).abs() < 1e-4, "the written note at the size for it");
    assert!((pitch_at(40.0, 12.0) - 48.0).abs() < 1e-3, "an octave down at twice the size");
    assert!((pitch_at(10.0, 12.0) - 72.0).abs() < 1e-3, "an octave up at half the size");
    assert!((pitch_at(40.0, -7.0) - 67.0).abs() < 1e-3, "a minus amount goes up as it grows");
}
