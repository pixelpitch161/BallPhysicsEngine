//! The sound path never allocates. Alone in its own test binary, since the counting allocator
//! sees every thread.

use bpe_core::{Settings, SlotId};
use bpe_kit::sound::{SubWave, VoiceSettings, starter_voice};
use bpe_kit::{kit, new_scene, resolve_scene};
use bpe_sound::{BLOCK, Engine};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Counts allocations while switched on, on any thread.
struct Counting;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn render_does_not_allocate() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().tags.insert("test-tone".into());
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..200 {
        e.render(&mut block);
    }
    // The physics steps between blocks (it may allocate as it grows its log); the sound path
    // itself never does.
    let mut heard = false;
    for _ in 0..1_000 {
        e.step_physics();
        ALLOCATIONS.store(0, Ordering::SeqCst);
        COUNTING.store(true, Ordering::SeqCst);
        e.render_prepared(&mut block);
        COUNTING.store(false, Ordering::SeqCst);
        assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0, "a block allocated");
        heard |= block.iter().any(|&v| v != 0.0);
    }
    assert!(heard, "hits were heard while counting");

    // The shape voice (both oscillators at full unison), playing and stopped. In the same test:
    // the counter is the whole program's.
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    v.a.unison = 16;
    v.b.on = true;
    v.b.unison = 16;
    v.a.sub = SubWave::Square;
    // Chords strummed over three slots that change every beat, and Slot keys.
    v.notes_slots = (0..3)
        .map(|i| bpe_core::NoteSlot { notes: vec![60 + i, 64 + i, 67 + i, 71 + i], order_clicked: vec![] })
        .collect();
    v.notes_plays = bpe_kit::sound::Plays::AllAtOnce;
    v.notes_strum = 150.0;
    v.notes_octave_range = 1;
    v.notes_every_unit = bpe_kit::sound::BeatsOrBars::Beats;
    // Every kind of modulator on, shared and per note, moving knobs and each other.
    let (lfo1, lfo2, dist) = (bpe_core::LayerId(4), bpe_core::LayerId(5), bpe_core::LayerId(2));
    let hit = bpe_core::LayerId(6);
    for l in &mut card.layers {
        l.enabled = true;
    }
    card.layers.push(bpe_core::Layer {
        id: hit,
        kind: bpe_kit::sound::HIT.into(),
        name: "Hit 1".into(),
        ..Default::default()
    });
    let route = |source, target: &str| bpe_core::ModRoute {
        source,
        target: target.into(),
        amount: 0.3,
        bipolar: false,
        invert: false,
    };
    v.routes.extend([
        route(lfo1, "a_cents"),
        route(lfo2, "4/rate_hz"),
        route(lfo1, "5/rate_hz"),
        route(dist, "b_gain"),
        route(hit, "out_pan"),
        route(bpe_core::LayerId(1), "2/far"),
    ]);
    card.params = v.to_params();
    card.params.set("5/start", bpe_core::Value::Choice("per_note".into()));
    card.params.set("3/follows", bpe_core::Value::Choice("each_notes_ball".into()));
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, card);
    // Keys on a knob and on the Master, read every block.
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    for (a, t, v) in [
        (bpe_core::Address::on(shape, SlotId::Sound, "a_gain"), 0.0, -6.0),
        (bpe_core::Address::on(shape, SlotId::Sound, "a_gain"), 5.0, 0.0),
        (bpe_core::Address::on(shape, SlotId::Sound, "notes_strum"), 0.0, 20.0),
        (bpe_core::Address::on(shape, SlotId::Sound, "notes_strum"), 5.0, 200.0),
        (bpe_core::Address::on(bpe_core::ThingId::ROOT, SlotId::Mixer, "master_output"), 0.0, -3.0),
        (bpe_core::Address::on(bpe_core::ThingId::ROOT, SlotId::Mixer, "master_output"), 5.0, 0.0),
    ] {
        scene.set_key(&kit.registry, &a, t, bpe_core::Value::Number(v)).unwrap();
    }
    for t in [1.0, 2.5, 4.0] {
        let a = bpe_core::Address::on(shape, SlotId::Sound, "notes_jump");
        scene.set_key(&kit.registry, &a, t, bpe_core::Value::Int(3)).unwrap();
    }
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    for _ in 0..200 {
        e.render(&mut block);
    }
    let mut heard = false;
    for _ in 0..1_000 {
        e.step_physics();
        ALLOCATIONS.store(0, Ordering::SeqCst);
        COUNTING.store(true, Ordering::SeqCst);
        e.render_prepared(&mut block);
        COUNTING.store(false, Ordering::SeqCst);
        assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0, "a shape voice block allocated");
        heard |= block.iter().any(|&v| v != 0.0);
    }
    assert!(heard, "the shape voice was heard while counting");
    // By side reads the side of each hit (the outline was made when the patch was built).
    let mut by_side = scene.clone();
    let card = by_side.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set("notes_plays", bpe_core::Value::Choice("by_side".into()));
    let resolved = resolve_scene(&by_side, &kit.registry);
    let mut side = Engine::new(&by_side, &resolved, &kit);
    let mut heard = false;
    for _ in 0..1_200 {
        side.step_physics();
        ALLOCATIONS.store(0, Ordering::SeqCst);
        COUNTING.store(true, Ordering::SeqCst);
        side.render_prepared(&mut block);
        COUNTING.store(false, Ordering::SeqCst);
        assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0, "a By side block allocated");
        heard |= block.iter().any(|&v| v != 0.0);
    }
    assert!(heard, "By side was heard while counting");
    e.stop_notes();
    e.audition(shape, 64);
    ALLOCATIONS.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    for _ in 0..100 {
        e.render_paused(&mut block);
    }
    COUNTING.store(false, Ordering::SeqCst);
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0, "a block while stopped allocated");

    // Every bus effect on Bus 1, sending to Bus 2 with a hall and a compressor keyed by the
    // shape's hits, with keys on a delay time, an EQ band and the fader.
    let (root, mixer) = (bpe_core::ThingId::ROOT, SlotId::Mixer);
    let reg = &kit.registry;
    let mut layered = Vec::new();
    for kind in bpe_kit::sound::fx::FX_KINDS {
        layered.push((kind, scene.add_layer_in(reg, root, mixer, kind, Some(0)).unwrap()));
    }
    let hall = scene.add_layer_in(reg, root, mixer, "fx.reverb", Some(1)).unwrap();
    let comp = scene.add_layer_in(reg, root, mixer, "fx.compressor", Some(1)).unwrap();
    {
        let card = scene.card_mut(root, mixer).unwrap();
        let set = |card: &mut bpe_core::Slot, key: String, v: bpe_core::Value| card.params.set(&key, v);
        set(card, "bus2_on".into(), bpe_core::Value::Bool(true));
        set(card, "bus1_send2".into(), bpe_core::Value::Number(-6.0));
        set(card, bpe_core::layered_key(hall, "kind"), bpe_core::Value::Choice("hall".into()));
        set(card, bpe_core::layered_key(comp, "sidechain"), bpe_core::Value::Choice("hits".into()));
        set(card, bpe_core::layered_key(comp, "sc_shape"), bpe_core::Value::Target(bpe_core::Target::Thing(shape)));
    }
    let delay = layered.iter().find(|(k, _)| *k == "fx.delay").unwrap().1;
    let eq = layered.iter().find(|(k, _)| *k == "fx.eq").unwrap().1;
    for (key, t, v) in [
        (bpe_core::layered_key(delay, "time_ms"), 0.0, 200.0),
        (bpe_core::layered_key(delay, "time_ms"), 5.0, 400.0),
        (bpe_core::layered_key(eq, "band5_freq"), 0.0, 500.0),
        (bpe_core::layered_key(eq, "band5_freq"), 5.0, 4000.0),
        ("bus1_fader".to_owned(), 0.0, -6.0),
        ("bus1_fader".to_owned(), 5.0, 0.0),
    ] {
        scene.set_key(reg, &bpe_core::Address::on(root, mixer, &key), t, bpe_core::Value::Number(v)).unwrap();
    }
    let resolved = resolve_scene(&scene, reg);
    let mut e = Engine::new(&scene, &resolved, &kit);
    for _ in 0..200 {
        e.render(&mut block);
    }
    let mut heard = false;
    for _ in 0..1_000 {
        e.step_physics();
        ALLOCATIONS.store(0, Ordering::SeqCst);
        COUNTING.store(true, Ordering::SeqCst);
        e.render_prepared(&mut block);
        COUNTING.store(false, Ordering::SeqCst);
        assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0, "a block through the bus effects allocated");
        assert!(block.iter().all(|v| v.is_finite()));
        heard |= block.iter().any(|&v| v != 0.0);
    }
    assert!(heard, "the buses were heard while counting");
}
