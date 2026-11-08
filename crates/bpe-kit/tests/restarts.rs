//! Every setting of every thing kind, changed one at a time: when the change moves the balls,
//! the physics fingerprint must change too (else the editor keeps playing a stale preview), and
//! when it changes how a thing stepped beside the physics runs, the beside fingerprint must
//! change (else the particle playback goes stale). The other way round is allowed: a fingerprint
//! that moves for a setting that changes nothing only costs a restart.
//!
//! Each kind sits in the test start scene (a hollow circle and one ball) as `add_thing` makes it,
//! with its starting cards and layers. Values that can't be changed simply (paths, gradients,
//! curves, routes...) are skipped.

use bpe_core::{Color, Registry, Resolved, Scene, SettingDef, ThingId, Value, ValueType, layered_key};
use bpe_kit::things::particles::state::ParticlesState;
use bpe_runtime::{Playback, Sims, beside_fingerprint, fingerprint, main_part};
use std::sync::Arc;

/// How long each run plays.
const SECONDS: f64 = 1.5;

/// A value of `def`'s type that differs from `now`; `None` when it can't be changed simply.
fn changed(def: &SettingDef, now: &Value) -> Option<Value> {
    let width = def.range.or(def.span).map_or(1.0, |(lo, hi)| (hi - lo).min(100.0));
    let moved = |x: f64| {
        let step = (x.abs() * 0.5).max(width * 0.1).max(1e-3);
        let up = def.clamp(Value::Number(x + step));
        if up != Value::Number(x) { up } else { def.clamp(Value::Number(x - step)) }
    };
    let v = match (def.ty, now) {
        (ValueType::Number, Value::Number(x)) => moved(*x),
        (ValueType::Int, Value::Int(x)) => {
            let up = def.clamp(Value::Int(x + 1 + x.abs() / 2));
            if up != Value::Int(*x) { up } else { def.clamp(Value::Int(x - 1)) }
        }
        (ValueType::Bool, Value::Bool(b)) => Value::Bool(!b),
        (ValueType::Choice(names), Value::Choice(c)) => {
            let at = names.iter().position(|n| n == c)?;
            Value::Choice(names[(at + 1) % names.len()].to_string())
        }
        (ValueType::Vec2, Value::Vec2([x, y])) => Value::Vec2([x + 13.0, y - 7.0]),
        (ValueType::Color, Value::Color(c)) => {
            let other = if c.r > 128 { Color::rgb(20, 200, 90) } else { Color::rgb(240, 60, 30) };
            Value::Color(other)
        }
        (ValueType::Text, Value::Text(t)) => Value::Text(if t == "Hi" { "Yo".into() } else { "Hi".into() }),
        _ => return None,
    };
    (v != *now).then_some(v)
}

/// Where a setting lives: the thing's own settings, or a card's (by slot), plain or layered.
#[derive(Clone, Copy, Debug)]
enum Place {
    Own,
    Card(bpe_core::SlotId),
}

/// Every setting thing `id` of `scene` holds, with where it is and its full key.
fn settings_of(scene: &Scene, reg: &Registry, id: ThingId) -> Vec<(Place, String, &'static SettingDef)> {
    let thing = &scene.things[&id];
    let mut out = Vec::new();
    if let Some(kind) = reg.thing(&thing.kind) {
        out.extend(kind.settings().iter().map(|d| (Place::Own, d.key.to_string(), d)));
    }
    for (slot, card) in &thing.slots {
        if let Some(kind) = reg.module(&card.kind) {
            out.extend(kind.settings().iter().map(|d| (Place::Card(*slot), d.key.to_string(), d)));
        }
        for layer in &card.layers {
            if let Some(kind) = reg.layer(&layer.kind) {
                out.extend(kind.settings().iter().map(|d| (Place::Card(*slot), layered_key(layer.id, d.key), d)));
            }
        }
    }
    out
}

/// What a run comes to: the balls (log and every position) and each beside thing's particles.
#[derive(PartialEq, Debug)]
struct Outcome {
    balls: u64,
    beside: Vec<(ThingId, u64)>,
}

fn outcome(scene: &Scene, resolved: &Resolved, sims: &Arc<Sims>) -> Outcome {
    let main = main_part(scene, resolved, sims);
    let mut physics = Playback::new(scene, &main, sims.clone());
    physics.seek(SECONDS);
    let mut h = physics.log().hash();
    for (id, bodies) in &physics.world().bodies {
        h = h.rotate_left(7) ^ id.0 as u64;
        for p in &bodies.position {
            h = h.rotate_left(5) ^ (p.x.to_bits() as u64) ^ ((p.y.to_bits() as u64) << 32);
        }
    }
    let mut beside = Vec::new();
    if bpe_runtime::has_beside(scene, resolved, sims) {
        let mut whole = Playback::new(scene, resolved, sims.clone());
        whole.seek(SECONDS);
        let world = whole.world();
        for id in &world.beside {
            if let Some(p) = world.states.get(id).and_then(|s| (**s).as_any().downcast_ref::<ParticlesState>()) {
                beside.push((*id, bpe_kit::things::particles::step::hash_of(p)));
            }
        }
    }
    Outcome { balls: h, beside }
}

/// The setting at `place`/`key` of thing `id`, or its default.
fn value_at(scene: &Scene, id: ThingId, place: Place, key: &str, def: &SettingDef) -> Value {
    let thing = &scene.things[&id];
    let params = match place {
        Place::Own => &thing.params,
        Place::Card(slot) => &thing.slots[&slot].params,
    };
    params.get(key).cloned().unwrap_or_else(|| def.default.clone())
}

/// `scene` with the setting at `place`/`key` of thing `id` set to `v`.
fn with(scene: &Scene, id: ThingId, place: Place, key: &str, v: Value) -> Scene {
    let mut scene = scene.clone();
    let thing = scene.things.get_mut(&id).expect("the thing");
    match place {
        Place::Own => thing.params.set(key, v),
        Place::Card(slot) => thing.slots.get_mut(&slot).expect("the card").params.set(key, v),
    }
    scene
}

#[derive(Default)]
struct Tally {
    checked: usize,
    moved_balls: usize,
    moved_beside: usize,
    stale: Vec<String>,
}

/// Changes every setting of thing `id` in `base` one at a time and checks the fingerprints.
fn check(base: &Scene, id: ThingId, label: &str, reg: &Registry, sims: &Arc<Sims>, tally: &mut Tally) {
    let resolved = bpe_kit::resolve_scene(base, reg);
    let before = outcome(base, &resolved, sims);
    let (print, beside_print) = (fingerprint(base, &resolved, sims), beside_fingerprint(base, &resolved, sims));
    for (place, key, def) in settings_of(base, reg, id) {
        let now = value_at(base, id, place, &key, def);
        let Some(new) = changed(def, &now) else { continue };
        let scene = with(base, id, place, &key, new.clone());
        let resolved = bpe_kit::resolve_scene(&scene, reg);
        let after = outcome(&scene, &resolved, sims);
        tally.checked += 1;
        let what = || format!("{label}: {place:?} {key}: {now:?} -> {new:?}");
        if after.balls != before.balls {
            tally.moved_balls += 1;
            if fingerprint(&scene, &resolved, sims) == print {
                tally.stale.push(format!("{} moves the balls but keeps the physics fingerprint", what()));
            }
        }
        if after.beside != before.beside {
            tally.moved_beside += 1;
            if beside_fingerprint(&scene, &resolved, sims) == beside_print {
                tally.stale.push(format!("{} changes the particles but keeps the beside fingerprint", what()));
            }
        }
    }
}

#[test]
fn every_setting_that_changes_a_run_restarts_it() {
    let kit = bpe_kit::kit();
    let (reg, sims) = (&kit.registry, &kit.sims);
    // (scene, thing whose settings change, what it is)
    let mut jobs: Vec<(Scene, ThingId, String)> = Vec::new();
    for kind in reg.things().map(|k| k.id()) {
        let mut start = bpe_kit::new_scene(reg);
        let id = match start.things.iter().find(|(_, t)| t.kind == kind) {
            Some((id, _)) if kind == "scene" || kind == "camera" => *id,
            _ => bpe_kit::add_thing(&mut start, reg, kind, ThingId::ROOT).unwrap_or_else(|e| panic!("{kind}: {e:?}")),
        };
        // Many settings only do something once a switch or a choice is set (a group's count, a
        // light the balls are drawn to): the same again with each switch and choice of every
        // thing in the scene changed in turn.
        let switches: Vec<(ThingId, Place, String, Value)> = start
            .order
            .iter()
            .flat_map(|t| settings_of(&start, reg, *t).into_iter().map(move |(place, key, def)| (*t, place, key, def)))
            .filter(|(_, _, _, def)| matches!(def.ty, ValueType::Bool | ValueType::Choice(_)))
            .filter_map(|(t, place, key, def)| {
                changed(def, &value_at(&start, t, place, &key, def)).map(|v| (t, place, key, v))
            })
            .collect();
        for (t, place, key, v) in switches {
            let label = format!("{kind} (with {} {place:?} {key} = {v:?})", start.things[&t].kind);
            jobs.push((with(&start, t, place, &key, v), id, label));
        }
        jobs.push((start, id, kind.to_string()));
    }
    // Spread over a few threads: the whole sweep is some 40,000 short runs.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    let tallies: Vec<Tally> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut tally = Tally::default();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((scene, id, label)) = jobs.get(i) else { return tally };
                        check(scene, *id, label, reg, sims, &mut tally);
                    }
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().expect("a worker panicked")).collect()
    });
    let mut all = Tally::default();
    for t in tallies {
        all.checked += t.checked;
        all.moved_balls += t.moved_balls;
        all.moved_beside += t.moved_beside;
        all.stale.extend(t.stale);
    }
    let Tally { checked, moved_balls, moved_beside, mut stale } = all;
    stale.sort();
    eprintln!("changed {checked} settings: {moved_balls} moved the balls, {moved_beside} the particles");
    assert!(checked > 1000 && moved_balls > 100 && moved_beside > 20, "too little was checked");
    assert!(stale.is_empty(), "{} edits would leave a stale preview:\n{}", stale.len(), stale.join("\n"));
}
