//! Releasing items into the target, step by step.

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Event, EventKind, Log, Party, ThingSim, World, state, state_mut};
use glam::Vec2;

use super::{EmitterSettings, Trigger};
use crate::rng::Rng;
use crate::things::balls::sim::BallsState;
use crate::things::liquid::sim::LiquidState;

pub struct EmitterSim;

/// What the emitter keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterState {
    pub settings: EmitterSettings,
    pub position: Vec2,
    pub rng: Rng,
    /// Items released so far.
    pub released: u32,
    /// Which of the matching targets gets the next item.
    pub next_target: usize,
}

/// The size of one item of `thing`, when it is a kind that has one.
pub fn item_radius(world: &World, thing: ThingId) -> Option<f32> {
    let st = world.states.get(&thing)?;
    if let Some(balls) = st.as_any().downcast_ref::<BallsState>() {
        return Some(balls.settings.radius);
    }
    if let Some(liquid) = st.as_any().downcast_ref::<LiquidState>() {
        return Some(liquid.radius());
    }
    None
}

fn dir(degrees: f32) -> Vec2 {
    let r = degrees.to_radians();
    Vec2::new(r.cos(), r.sin())
}

impl ThingSim for EmitterSim {
    fn kind(&self) -> &'static str {
        "emitter"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = EmitterSettings::from_params(&resolved.params);
        let rng = Rng::new(settings.seed, thing.id.0);
        world.states.insert(
            thing.id,
            Box::new(EmitterState { settings, position: thing.transform.position, rng, released: 0, next_target: 0 }),
        );
    }

    fn spawn(&self, me: ThingId, world: &mut World, log: &mut Log) {
        let time = world.time() as f32;
        let (settings, released) = {
            let st = state::<EmitterState>(world.states[&me].as_ref());
            (st.settings.clone(), st.released)
        };
        if released >= settings.limit || time < settings.start || (settings.stop > 0.0 && time > settings.stop) {
            return;
        }
        let due = match settings.on {
            Trigger::Time => {
                let steady = ((time - settings.start) * settings.rate).floor().max(0.0) as u32;
                (steady + settings.burst).min(settings.limit)
            }
            Trigger::Hit => {
                let listened = world.matching(&settings.listen, me);
                let hits = log
                    .since(world.step.saturating_sub(1))
                    .iter()
                    .filter(|e| e.kind == EventKind::Hit && listened.contains(&e.a.thing))
                    .count() as u32;
                (released + hits).min(settings.limit)
            }
        };
        if due <= released {
            return;
        }
        let targets: Vec<ThingId> =
            world.matching(&settings.target, me).into_iter().filter(|t| world.bodies.contains_key(t)).collect();
        if targets.is_empty() {
            return;
        }
        let across = dir(settings.direction).perp();
        for _ in released..due {
            let (target, position, velocity, radius) = {
                let st = state_mut::<EmitterState>(world.states.get_mut(&me).unwrap().as_mut());
                let target = targets[st.next_target % targets.len()];
                st.next_target += 1;
                st.released += 1;
                let along = st.rng.range(-0.5, 0.5) * settings.width;
                let turn = st.rng.range(-0.5, 0.5) * settings.spread;
                (target, st.position + across * along, dir(settings.direction + turn) * settings.speed, 0.0)
            };
            let radius = item_radius(world, target).map_or(12.0, |r| r * settings.size_scale).max(radius);
            let step = world.step;
            let bodies = world.bodies.get_mut(&target).expect("filtered to things with bodies");
            let row = bodies.push(position, velocity, radius);
            log.push(Event {
                step,
                kind: EventKind::Spawn,
                a: Party { thing: target, item: row },
                b: Some(Party { thing: me, item: 0 }),
                point: position,
                normal: velocity.normalize_or_zero(),
                speed: settings.speed,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{BALLS, run, scene_with};
    use bpe_core::{Target, ThingId, Value};
    use bpe_runtime::{EventKind, Party};
    use glam::Vec2;

    fn emitter(f: impl FnOnce(&mut bpe_core::Params)) -> (bpe_core::Scene, bpe_core::Resolved, crate::Kit, ThingId) {
        scene_with("emitter", "Spout", Vec2::new(0.0, 300.0), |p| {
            p.set("target", Value::Target(Target::Thing(BALLS)));
            f(p);
        })
    }

    #[test]
    fn an_emitter_releases_at_its_rate_into_its_target_and_logs_spawns() {
        let (scene, resolved, kit, me) = emitter(|p| {
            p.set("rate", Value::Number(10.0));
            p.set("spread", Value::Number(0.0));
        });
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let (w, log) = run(&scene, &resolved, &kit, 2.0);
        assert_eq!(w.bodies[&BALLS].len(), 1 + 20);
        let spawns: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Spawn).collect();
        assert_eq!(spawns.len(), 20);
        assert_eq!(spawns[0].a, Party { thing: BALLS, item: 1 });
        assert_eq!(spawns[0].b, Some(Party { thing: me, item: 0 }));
        assert!(spawns[0].normal.y < -0.9, "aims up: {:?}", spawns[0].normal);
        assert_eq!(spawns[0].point, Vec2::new(0.0, 300.0));
        assert_eq!(spawns[0].speed, 500.0);
        assert_eq!(w.bodies[&BALLS].radius[1], 24.0, "takes the target's size");
        assert_eq!(spawns[0].step, 24, "the first drop comes when a tenth of a second is up");
        assert!(w.bodies[&BALLS].position.iter().all(|p| p.is_finite()));
    }

    #[test]
    fn a_burst_comes_all_at_once_and_the_limit_holds() {
        let (scene, resolved, kit, _) = emitter(|p| {
            p.set("rate", Value::Number(0.0));
            p.set("burst", Value::Int(30));
            p.set("limit", Value::Int(25));
        });
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        assert_eq!(w.bodies[&BALLS].len(), 1 + 25);
        let spawns: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Spawn).collect();
        assert_eq!(spawns.len(), 25);
        assert!(spawns.iter().all(|e| e.step == 1), "all in the first step");
    }

    #[test]
    fn start_and_stop_bound_the_release() {
        let (scene, resolved, kit, _) = emitter(|p| {
            p.set("rate", Value::Number(10.0));
            p.set("start", Value::Number(1.0));
            p.set("stop", Value::Number(2.0));
        });
        let (w, log) = run(&scene, &resolved, &kit, 3.0);
        assert_eq!(w.bodies[&BALLS].len(), 1 + 10);
        let steps: Vec<u64> = log.events().iter().filter(|e| e.kind == EventKind::Spawn).map(|e| e.step).collect();
        assert!(steps.iter().all(|&s| s > 240 && s <= 480), "{steps:?}");
    }

    #[test]
    fn hit_mode_releases_one_per_listened_hit() {
        let (scene, resolved, kit, _) = emitter(|p| {
            p.set("on", Value::Choice("hit".into()));
            p.set("listen", Value::Target(Target::Thing(BALLS)));
            p.set("limit", Value::Int(3));
        });
        let (w, log) = run(&scene, &resolved, &kit, 5.0);
        assert_eq!(w.bodies[&BALLS].len(), 1 + 3);
        let spawns: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Spawn).collect();
        assert_eq!(spawns.len(), 3);
        for s in &spawns {
            assert!(
                log.events().iter().any(|e| e.kind == EventKind::Hit && e.a.thing == BALLS && e.step + 1 == s.step),
                "a drop one step after a hit: {s:?}"
            );
        }
    }

    #[test]
    fn an_emitter_without_a_live_target_releases_nothing() {
        for target in [Target::Thing(ThingId(99)), Target::Tag("none".into()), Target::Kind("arena".into())] {
            let (scene, resolved, kit, _) = emitter(|p| p.set("target", Value::Target(target.clone())));
            let (w, log) = run(&scene, &resolved, &kit, 1.0);
            assert_eq!(w.bodies[&BALLS].len(), 1, "{target:?}");
            assert!(log.events().iter().all(|e| e.kind != EventKind::Spawn), "{target:?}");
        }
    }

    #[test]
    fn two_runs_of_a_spraying_emitter_match_and_spread_out() {
        let (scene, resolved, kit, _) = emitter(|p| {
            p.set("spread", Value::Number(90.0));
            p.set("width", Value::Number(200.0));
            p.set("rate", Value::Number(50.0));
        });
        let (w1, log1) = run(&scene, &resolved, &kit, 1.0);
        let (_, log2) = run(&scene, &resolved, &kit, 1.0);
        assert_eq!(log1, log2);
        let xs: Vec<f32> = log1.events().iter().filter(|e| e.kind == EventKind::Spawn).map(|e| e.point.x).collect();
        assert!(xs.len() >= 49, "{}", xs.len());
        assert!(xs.iter().any(|&x| x > 30.0) && xs.iter().any(|&x| x < -30.0), "spread along the width: {xs:?}");
        assert!(xs.iter().all(|&x| x.abs() <= 100.0));
        let normals: Vec<Vec2> =
            log1.events().iter().filter(|e| e.kind == EventKind::Spawn).map(|e| e.normal).collect();
        assert!(normals.iter().any(|n| n.x > 0.3) && normals.iter().any(|n| n.x < -0.3), "spread of aim");
        assert!(w1.bodies[&BALLS].position.iter().all(|p| p.is_finite()));
    }

    #[test]
    fn emitter_targets_round_robin() {
        // Two Balls things tagged "cup": drops alternate between them.
        let (mut scene, _, kit, me) = emitter(|p| {
            p.set("target", Value::Target(Target::Tag("cup".into())));
            p.set("rate", Value::Number(10.0));
        });
        let second = scene.add_thing(&kit.registry, "balls", ThingId::ROOT).unwrap();
        for id in [BALLS, second] {
            scene.things.get_mut(&id).unwrap().tags.insert("cup".into());
        }
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        assert_eq!(w.bodies[&BALLS].len(), 1 + 5);
        assert_eq!(w.bodies[&second].len(), 1 + 5);
        let first = log.events().iter().find(|e| e.kind == EventKind::Spawn).unwrap();
        assert_eq!((first.a.thing, first.b.unwrap().thing), (BALLS, me));
    }
}
