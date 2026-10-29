//! Pushing the targets every substep, and noticing what comes and goes.

use std::collections::BTreeMap;

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Event, EventKind, Log, Party, ThingSim, World, state, state_mut};
use glam::Vec2;

use super::{Falloff, FieldKindChoice, FieldSettings, FieldShape};

pub struct FieldSim;

/// What the field keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldState {
    pub settings: FieldSettings,
    pub center: Vec2,
    /// Radians clockwise.
    pub rotation: f32,
    /// Per target thing, which of its bodies were inside at the end of the last step.
    pub inside: BTreeMap<ThingId, Vec<bool>>,
}

fn dir(radians: f32) -> Vec2 {
    Vec2::new(radians.cos(), radians.sin())
}

/// How much of the field's strength reaches `p`: 0 outside, 1 at full strength.
pub fn weight(s: &FieldSettings, center: Vec2, rotation: f32, p: Vec2) -> f32 {
    let d = match s.shape {
        FieldShape::Everywhere => return 1.0,
        FieldShape::Circle => (p - center).length() / s.radius.max(1e-3),
        FieldShape::Box => {
            let offset = p - center;
            let (x, y) = (dir(rotation), dir(rotation).perp());
            let local = Vec2::new(offset.dot(x), offset.dot(y));
            (local.x.abs() / (s.width * 0.5).max(1e-3)).max(local.y.abs() / (s.height * 0.5).max(1e-3))
        }
    };
    if d > 1.0 {
        return 0.0;
    }
    match s.falloff {
        Falloff::Flat => 1.0,
        Falloff::Linear => 1.0 - d,
        Falloff::Smooth => 1.0 - d * d * (3.0 - 2.0 * d),
    }
}

/// The acceleration at `p`, before the weight.
fn push(s: &FieldSettings, center: Vec2, p: Vec2) -> Vec2 {
    let r = (p - center).normalize_or_zero();
    match s.kind {
        FieldKindChoice::Wind => dir(s.direction.to_radians()) * s.strength,
        FieldKindChoice::Well => -r * s.strength,
        FieldKindChoice::Repel => r * s.strength,
        FieldKindChoice::Vortex => (r.perp() - r * s.pull) * s.strength,
    }
}

impl ThingSim for FieldSim {
    fn kind(&self) -> &'static str {
        "field"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = FieldSettings::from_params(&resolved.params);
        let st = FieldState {
            settings,
            center: thing.transform.position,
            rotation: thing.transform.rotation,
            inside: BTreeMap::new(),
        };
        world.states.insert(thing.id, Box::new(st));
    }

    fn settings_changed(&self, me: ThingId, resolved: &ResolvedThing, world: &mut World) {
        // A keyframe or pulse moved a setting: the push follows it, the region's memory of who
        // is inside stays.
        let settings = FieldSettings::from_params(&resolved.params);
        if let Some(st) = world.states.get_mut(&me) {
            state_mut::<FieldState>(st.as_mut()).settings = settings;
        }
    }

    fn forces(&self, me: ThingId, world: &mut World, dt: f32) {
        let st = state::<FieldState>(world.states[&me].as_ref());
        let (settings, center, rotation) = (st.settings.clone(), st.center, st.rotation);
        if settings.strength == 0.0 {
            return;
        }
        for target in world.matching(&settings.targets, me) {
            let Some(bodies) = world.bodies.get_mut(&target) else { continue };
            for i in 0..bodies.len() {
                if !bodies.alive[i] {
                    continue;
                }
                let p = bodies.position[i];
                let w = weight(&settings, center, rotation, p);
                if w > 0.0 {
                    bodies.velocity[i] += push(&settings, center, p) * (w * dt);
                }
            }
        }
    }

    fn after_step(&self, me: ThingId, world: &mut World, log: &mut Log) {
        let step = world.step;
        let (settings, center, rotation) = {
            let st = state::<FieldState>(world.states[&me].as_ref());
            (st.settings.clone(), st.center, st.rotation)
        };
        if !settings.log_events {
            return;
        }
        let targets = world.matching(&settings.targets, me);
        let mut events = Vec::new();
        {
            let World { bodies, states, .. } = world;
            let st = state_mut::<FieldState>(states.get_mut(&me).unwrap().as_mut());
            for target in targets {
                let Some(bodies) = bodies.get(&target) else { continue };
                if bodies.silent {
                    continue;
                }
                let inside = st.inside.entry(target).or_default();
                inside.resize(bodies.len(), false);
                for (i, was) in inside.iter_mut().enumerate() {
                    let p = bodies.position[i];
                    let now = bodies.alive[i] && weight(&settings, center, rotation, p) > 0.0;
                    if now != *was {
                        *was = now;
                        events.push(Event {
                            step,
                            kind: if now { EventKind::Enter } else { EventKind::Leave },
                            a: Party { thing: target, item: i as u32 },
                            b: Some(Party { thing: me, item: 0 }),
                            point: p,
                            normal: Vec2::ZERO,
                            speed: 0.0,
                        });
                    }
                }
            }
        }
        for e in events {
            log.push(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{BALLS, run, scene_with};
    use bpe_core::{Target, ThingId, Value};
    use bpe_runtime::{EventKind, Party};
    use glam::Vec2;

    /// A field at the origin over a scene with no gravity and a still ball at `ball`.
    fn field(
        ball: Vec2,
        f: impl FnOnce(&mut bpe_core::Params),
    ) -> (bpe_core::Scene, bpe_core::Resolved, crate::Kit, ThingId) {
        let (mut scene, _, kit, id) = scene_with("field", "Zone", Vec2::ZERO, f);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let b = scene.things.get_mut(&BALLS).unwrap();
        b.transform.position = ball;
        b.params.set("velocity", Value::Vec2([0.0, 0.0]));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        (scene, resolved, kit, id)
    }

    #[test]
    fn a_keyframed_strength_pushes_while_running() {
        // A key on Strength changes the physics while the world runs (stage 7); the field must
        // read the new strength without a rebuild. A wind keyed from 0 to 1000 over a second
        // gives a still ball about 500 px/s (the area under a smooth ramp).
        let (mut scene, _, kit, me) = field(Vec2::new(-100.0, 0.0), |p| {
            p.set("kind", Value::Choice("wind".into()));
            p.set("strength", Value::Number(0.0));
            p.set("shape", Value::Choice("everywhere".into()));
        });
        let strength = bpe_core::Address { owner: bpe_core::Owner::Thing(me), slot: None, key: "strength".into() };
        scene.set_key(&kit.registry, &strength, 0.0, Value::Number(0.0)).unwrap();
        scene.set_key(&kit.registry, &strength, 1.0, Value::Number(1000.0)).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let (w, _) = run(&scene, &resolved, &kit, 1.0);
        let v = w.bodies[&BALLS].velocity[0];
        assert!((v.x - 500.0).abs() < 25.0 && v.y.abs() < 1e-3, "the keyed wind should push: {v:?}");
    }

    #[test]
    fn wind_pushes_only_its_tagged_targets() {
        let (mut scene, _, kit, _) = field(Vec2::new(-100.0, 0.0), |p| {
            p.set("kind", Value::Choice("wind".into()));
            p.set("strength", Value::Number(1000.0));
            p.set("shape", Value::Choice("everywhere".into()));
            p.set("targets", Value::Target(Target::Tag("light".into())));
        });
        let other = scene.add_thing(&kit.registry, "balls", ThingId::ROOT).unwrap();
        scene.things.get_mut(&other).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        scene.things.get_mut(&other).unwrap().transform.position = Vec2::new(100.0, 0.0);
        scene.things.get_mut(&BALLS).unwrap().tags.insert("light".into());
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let (w, _) = run(&scene, &resolved, &kit, 0.25);
        let light = w.bodies[&BALLS].velocity[0];
        assert!((light.x - 250.0).abs() < 1.0 && light.y.abs() < 1e-3, "{light:?}");
        assert_eq!(w.bodies[&other].velocity[0], Vec2::ZERO);
    }

    #[test]
    fn a_well_pulls_in_and_a_repeller_pushes_out() {
        for (kind, sign) in [("well", -1.0), ("repel", 1.0)] {
            let (scene, resolved, kit, _) = field(Vec2::new(200.0, 0.0), |p| {
                p.set("kind", Value::Choice(kind.into()));
                p.set("falloff", Value::Choice("flat".into()));
                p.set("radius", Value::Number(500.0));
            });
            let (w, _) = run(&scene, &resolved, &kit, 0.1);
            let v = w.bodies[&BALLS].velocity[0];
            assert!((v.x - sign * 60.0).abs() < 0.5 && v.y.abs() < 1e-3, "{kind}: {v:?}");
        }
    }

    #[test]
    fn a_vortex_swirls_clockwise_with_a_little_pull() {
        let (scene, resolved, kit, _) = field(Vec2::new(200.0, 0.0), |p| {
            p.set("kind", Value::Choice("vortex".into()));
            p.set("falloff", Value::Choice("flat".into()));
            p.set("radius", Value::Number(500.0));
        });
        let (w, _) = run(&scene, &resolved, &kit, 1.0 / 240.0);
        let v = w.bodies[&BALLS].velocity[0];
        assert!(v.y > 0.0, "clockwise on a y-down screen: {v:?}");
        assert!(v.x < 0.0 && (v.x.abs() - 0.3 * v.y).abs() < 0.01, "pulled in by a third: {v:?}");
    }

    #[test]
    fn entering_and_leaving_are_logged_once_each() {
        let (mut scene, _, kit, me) = field(Vec2::new(-400.0, 0.0), |p| {
            p.set("strength", Value::Number(0.0));
            p.set("radius", Value::Number(100.0));
        });
        scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([400.0, 0.0]));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 2.0);
        let zone: Vec<_> = log.events().iter().filter(|e| e.b == Some(Party { thing: me, item: 0 })).collect();
        assert_eq!(zone.len(), 2, "{zone:?}");
        assert_eq!(zone[0].kind, EventKind::Enter);
        assert_eq!(zone[1].kind, EventKind::Leave);
        assert_eq!(zone[0].a, Party { thing: BALLS, item: 0 });
        assert!((zone[0].point.x + 100.0).abs() < 5.0, "entered at the edge: {:?}", zone[0].point);
        assert!((zone[1].point.x - 100.0).abs() < 5.0, "left at the other edge: {:?}", zone[1].point);
        assert!(zone[0].step < zone[1].step);
    }

    #[test]
    fn anything_moves_liquid_too_but_logs_no_liquid_entries() {
        let (mut scene, _, kit, me) = field(Vec2::new(0.0, 0.0), |p| {
            p.set("kind", Value::Choice("wind".into()));
            p.set("strength", Value::Number(1000.0));
            p.set("shape", Value::Choice("everywhere".into()));
        });
        // A second set of balls, made silent once built, stands in for a liquid.
        let drops = scene.add_thing(&kit.registry, "balls", ThingId::ROOT).unwrap();
        scene.things.get_mut(&drops).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        world.bodies.get_mut(&drops).unwrap().silent = true;
        let mut log = bpe_runtime::Log::default();
        world.advance_to(0.1, &kit.sims, &mut log);
        assert!(world.bodies[&drops].velocity[0].x > 90.0);
        assert!(world.bodies[&BALLS].velocity[0].x > 90.0);
        let entries: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Enter).collect();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!((entries[0].a.thing, entries[0].b.unwrap().thing), (BALLS, me));
    }

    #[test]
    fn falloff_weights_follow_their_shapes() {
        let circle =
            |falloff| FieldSettings { shape: FieldShape::Circle, radius: 100.0, falloff, ..FieldSettings::default() };
        assert_eq!(weight(&circle(Falloff::Flat), Vec2::ZERO, 0.0, Vec2::new(99.0, 0.0)), 1.0);
        assert_eq!(weight(&circle(Falloff::Flat), Vec2::ZERO, 0.0, Vec2::new(101.0, 0.0)), 0.0);
        assert!((weight(&circle(Falloff::Linear), Vec2::ZERO, 0.0, Vec2::new(0.0, 50.0)) - 0.5).abs() < 1e-5);
        assert!((weight(&circle(Falloff::Smooth), Vec2::ZERO, 0.0, Vec2::new(0.0, 50.0)) - 0.5).abs() < 1e-5);
        assert!(weight(&circle(Falloff::Smooth), Vec2::ZERO, 0.0, Vec2::new(0.0, 90.0)) < 0.03);
        let boxed = FieldSettings {
            shape: FieldShape::Box,
            width: 200.0,
            height: 100.0,
            falloff: Falloff::Linear,
            ..FieldSettings::default()
        };
        assert!((weight(&boxed, Vec2::new(10.0, 10.0), 0.0, Vec2::new(10.0, 10.0)) - 1.0).abs() < 1e-5);
        assert!((weight(&boxed, Vec2::ZERO, 0.0, Vec2::new(50.0, 0.0)) - 0.5).abs() < 1e-5);
        assert_eq!(weight(&boxed, Vec2::ZERO, 0.0, Vec2::new(0.0, 60.0)), 0.0);
        // Turned a quarter, the box's long side runs up and down.
        assert!(weight(&boxed, Vec2::ZERO, std::f32::consts::FRAC_PI_2, Vec2::new(0.0, 90.0)) > 0.0);
        let everywhere = FieldSettings { shape: FieldShape::Everywhere, ..FieldSettings::default() };
        assert_eq!(weight(&everywhere, Vec2::ZERO, 0.0, Vec2::new(9999.0, 9999.0)), 1.0);
    }
}
