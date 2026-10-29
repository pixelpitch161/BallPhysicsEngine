//! What rules ask the world to do, queued during a step and applied at the start of the next.

use bpe_core::{SlotId, ThingId, Value};
use glam::Vec2;

use crate::plugins::Sims;
use crate::{Event, EventKind, Log, Party, World};

/// The most balls one thing may hold, so a runaway spawn rule stays playable.
pub const MAX_BALLS: usize = 4096;
/// How far apart the pieces of a split fly, from the first to the last.
pub const SPLIT_SPREAD_DEGREES: f32 = 40.0;
/// The biggest a ball may be grown to, in pixels.
const MAX_RADIUS: f32 = 400.0;
const MIN_RADIUS: f32 = 0.5;

/// One thing a rule does. Balls that are gone and things without balls are skipped, and an
/// action with a number that isn't finite does nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Make the ball bigger by `by` pixels, or smaller when negative.
    Grow {
        ball: Party,
        by: f32,
    },
    /// Multiply the ball's speed.
    Speed {
        ball: Party,
        factor: f32,
    },
    /// Add to the ball's velocity.
    Kick {
        ball: Party,
        velocity: Vec2,
    },
    /// Lay a colour over the ball's look; the alpha is how strongly, 0 clears it.
    Tint {
        ball: Party,
        color: [f32; 4],
    },
    Remove {
        ball: Party,
    },
    /// Split the ball into pieces that share its area and fan out around its direction.
    Split {
        ball: Party,
        pieces: u32,
    },
    /// Add a ball to a thing.
    Spawn {
        thing: ThingId,
        position: Vec2,
        velocity: Vec2,
        radius: f32,
        color: [f32; 4],
    },
    /// Change the scene's gravity from now on.
    Gravity(Vec2),
    /// Change one setting of the module a thing uses in a slot, for that thing only.
    Set {
        thing: ThingId,
        slot: SlotId,
        key: String,
        value: Value,
    },
    /// Log a custom event other rules can wait for.
    Fire {
        name: String,
        ball: Party,
        point: Vec2,
    },
    /// Log a note event the thing's sound plays, as loud as a hit at `loudness` px/s.
    Note {
        ball: Party,
        loudness: f32,
    },
}

impl World {
    /// Applies queued actions in order, logging the spawns, removals, custom events and notes
    /// at the current step.
    pub fn apply(&mut self, actions: Vec<Action>, sims: &Sims, log: &mut Log) {
        for action in actions {
            self.apply_one(action, sims, log);
        }
    }

    fn apply_one(&mut self, action: Action, sims: &Sims, log: &mut Log) {
        let step = self.step;
        let event = |kind: EventKind, a: Party, point: Vec2, speed: f32| Event {
            step,
            kind,
            a,
            b: None,
            point,
            normal: Vec2::ZERO,
            speed,
        };
        match action {
            Action::Grow { ball, by } => {
                if let Some((bodies, i)) = self.live(ball)
                    && by.is_finite()
                {
                    bodies.radius[i] = (bodies.radius[i] + by).clamp(MIN_RADIUS, MAX_RADIUS);
                }
            }
            Action::Speed { ball, factor } => {
                if let Some((bodies, i)) = self.live(ball)
                    && factor.is_finite()
                {
                    bodies.velocity[i] *= factor.max(0.0);
                }
            }
            Action::Kick { ball, velocity } => {
                if let Some((bodies, i)) = self.live(ball)
                    && velocity.is_finite()
                {
                    bodies.velocity[i] += velocity;
                }
            }
            Action::Tint { ball, color } => {
                if let Some((bodies, i)) = self.live(ball)
                    && color.iter().all(|c| c.is_finite())
                {
                    bodies.tint[i] = color;
                }
            }
            Action::Remove { ball } => {
                if let Some((bodies, i)) = self.live(ball) {
                    bodies.alive[i] = false;
                    let point = bodies.position[i];
                    log.push(event(EventKind::Remove, ball, point, 0.0));
                }
            }
            Action::Split { ball, pieces } => {
                let pieces = pieces.clamp(2, 8) as usize;
                let Some((bodies, i)) = self.live(ball) else { return };
                if bodies.len() + pieces - 1 > MAX_BALLS {
                    return;
                }
                let radius = (bodies.radius[i] / (pieces as f32).sqrt()).max(MIN_RADIUS);
                let (position, velocity, tint) = (bodies.position[i], bodies.velocity[i], bodies.tint[i]);
                bodies.radius[i] = radius;
                let step_angle = SPLIT_SPREAD_DEGREES.to_radians() / (pieces - 1) as f32;
                let start = -SPLIT_SPREAD_DEGREES.to_radians() * 0.5;
                for k in 0..pieces {
                    let v = Vec2::from_angle(start + step_angle * k as f32).rotate(velocity);
                    if k == 0 {
                        bodies.velocity[i] = v;
                    } else {
                        let row = bodies.push(position, v, radius);
                        bodies.tint[row as usize] = tint;
                        log.push(event(EventKind::Spawn, Party { thing: ball.thing, item: row }, position, 0.0));
                    }
                }
            }
            Action::Spawn { thing, position, velocity, radius, color } => {
                let Some(bodies) = self.bodies.get_mut(&thing) else { return };
                if bodies.len() >= MAX_BALLS
                    || !position.is_finite()
                    || !velocity.is_finite()
                    || !radius.is_finite()
                    || color.iter().any(|c| !c.is_finite())
                {
                    return;
                }
                let row = bodies.push(position, velocity, radius.clamp(MIN_RADIUS, MAX_RADIUS));
                bodies.tint[row as usize] = color;
                log.push(event(EventKind::Spawn, Party { thing, item: row }, position, 0.0));
            }
            Action::Gravity(g) => {
                if g.is_finite() {
                    self.gravity = g;
                }
            }
            Action::Set { thing, slot, key, value } => self.set_slot_param(thing, slot, &key, value, sims),
            Action::Fire { name, ball, point } => {
                if point.is_finite() {
                    log.push(event(EventKind::Custom(name), ball, point, 0.0));
                }
            }
            Action::Note { ball, loudness } => {
                if let Some((bodies, i)) = self.live(ball)
                    && loudness.is_finite()
                {
                    let point = bodies.position[i];
                    log.push(event(EventKind::Note, ball, point, loudness.max(0.0)));
                }
            }
        }
    }

    /// The ball table and row of a ball that is still there.
    fn live(&mut self, ball: Party) -> Option<(&mut crate::Bodies, usize)> {
        let bodies = self.bodies.get_mut(&ball.thing)?;
        let i = ball.item as usize;
        (i < bodies.len() && bodies.alive[i]).then_some((bodies, i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{DOTS, dots_in_ring};
    use crate::{EventKind, Log, Party, World};
    use bpe_core::ThingId;
    use glam::Vec2;

    fn world() -> (World, crate::Sims) {
        let (scene, resolved, sims) = dots_in_ring(1);
        let mut w = World::build(&scene, &resolved, &sims);
        w.step = 7;
        (w, sims)
    }

    const B0: Party = Party { thing: DOTS, item: 0 };

    #[test]
    fn grow_speed_kick_tint_and_remove_change_one_ball() {
        let (mut w, sims) = world();
        let mut log = Log::default();
        let v = w.bodies[&DOTS].velocity[0];
        w.apply(
            vec![
                Action::Grow { ball: B0, by: 3.0 },
                Action::Speed { ball: B0, factor: 2.0 },
                Action::Kick { ball: B0, velocity: Vec2::new(0.0, -10.0) },
                Action::Tint { ball: B0, color: [1.0, 0.0, 0.0, 0.5] },
            ],
            &sims,
            &mut log,
        );
        let b = &w.bodies[&DOTS];
        assert_eq!(b.radius[0], 8.0);
        assert_eq!(b.velocity[0], v * 2.0 + Vec2::new(0.0, -10.0));
        assert_eq!(b.tint[0], [1.0, 0.0, 0.0, 0.5]);
        assert!(log.is_empty());
        w.apply(vec![Action::Remove { ball: B0 }], &sims, &mut log);
        assert!(!w.bodies[&DOTS].alive[0]);
        assert_eq!(log.events().len(), 1);
        assert_eq!((log.events()[0].kind.clone(), log.events()[0].a, log.events()[0].step), (EventKind::Remove, B0, 7));
        // Gone balls are left alone from then on.
        w.apply(vec![Action::Grow { ball: B0, by: 3.0 }], &sims, &mut log);
        assert_eq!(w.bodies[&DOTS].radius[0], 8.0);
    }

    #[test]
    fn spawn_adds_a_ball_and_logs_it() {
        let (mut w, sims) = world();
        let mut log = Log::default();
        let spawn = Action::Spawn {
            thing: DOTS,
            position: Vec2::new(3.0, 4.0),
            velocity: Vec2::new(1.0, 0.0),
            radius: 9.0,
            color: [0.0, 1.0, 0.0, 1.0],
        };
        w.apply(vec![spawn], &sims, &mut log);
        let b = &w.bodies[&DOTS];
        assert_eq!(b.len(), 2);
        assert_eq!(
            (b.position[1], b.velocity[1], b.radius[1], b.tint[1]),
            (Vec2::new(3.0, 4.0), Vec2::X, 9.0, [0.0, 1.0, 0.0, 1.0])
        );
        assert!(b.alive[1]);
        let e = &log.events()[0];
        assert_eq!(
            (e.kind.clone(), e.a, e.point),
            (EventKind::Spawn, Party { thing: DOTS, item: 1 }, Vec2::new(3.0, 4.0))
        );
    }

    #[test]
    fn split_spreads_the_pieces_and_keeps_area() {
        let (mut w, sims) = world();
        let mut log = Log::default();
        w.bodies.get_mut(&DOTS).unwrap().velocity[0] = Vec2::new(100.0, 0.0);
        w.bodies.get_mut(&DOTS).unwrap().tint[0] = [0.0, 0.0, 1.0, 1.0];
        w.apply(vec![Action::Split { ball: B0, pieces: 2 }], &sims, &mut log);
        let b = &w.bodies[&DOTS];
        assert_eq!(b.len(), 2);
        let r = 5.0 / 2f32.sqrt();
        assert!((b.radius[0] - r).abs() < 1e-5 && (b.radius[1] - r).abs() < 1e-5);
        assert!((b.velocity[0].length() - 100.0).abs() < 1e-3 && (b.velocity[1].length() - 100.0).abs() < 1e-3);
        let angle = b.velocity[0].angle_to(b.velocity[1]).abs().to_degrees();
        assert!((angle - SPLIT_SPREAD_DEGREES).abs() < 0.01, "{angle}");
        assert_eq!(b.tint[1], [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(log.events().len(), 1);
        assert_eq!(log.events()[0].a, Party { thing: DOTS, item: 1 });
    }

    #[test]
    fn spawning_stops_at_the_ball_cap() {
        let (mut w, sims) = world();
        let mut log = Log::default();
        let bodies = w.bodies.get_mut(&DOTS).unwrap();
        while bodies.len() < MAX_BALLS {
            bodies.push(Vec2::ZERO, Vec2::ZERO, 1.0);
        }
        let spawn =
            Action::Spawn { thing: DOTS, position: Vec2::ZERO, velocity: Vec2::ZERO, radius: 1.0, color: [0.0; 4] };
        w.apply(vec![spawn, Action::Split { ball: B0, pieces: 3 }], &sims, &mut log);
        assert_eq!(w.bodies[&DOTS].len(), MAX_BALLS);
        assert!(log.is_empty());
    }

    #[test]
    fn a_non_finite_action_is_ignored() {
        let (mut w, sims) = world();
        let mut log = Log::default();
        let before = w.bodies[&DOTS].clone();
        w.apply(
            vec![
                Action::Kick { ball: B0, velocity: Vec2::new(f32::NAN, 0.0) },
                Action::Grow { ball: B0, by: f32::INFINITY },
                Action::Speed { ball: B0, factor: f32::NAN },
                Action::Spawn {
                    thing: DOTS,
                    position: Vec2::new(f32::INFINITY, 0.0),
                    velocity: Vec2::ZERO,
                    radius: 1.0,
                    color: [0.0; 4],
                },
                Action::Gravity(Vec2::new(0.0, f32::NAN)),
            ],
            &sims,
            &mut log,
        );
        assert_eq!(w.bodies[&DOTS], before);
        assert!(w.gravity.is_finite());
        w.apply(vec![Action::Gravity(Vec2::new(0.0, 50.0))], &sims, &mut log);
        assert_eq!(w.gravity, Vec2::new(0.0, 50.0));
    }

    #[test]
    fn actions_on_gone_balls_are_skipped() {
        let (mut w, sims) = world();
        let mut log = Log::default();
        let before = w.bodies.clone();
        w.apply(
            vec![
                Action::Grow { ball: Party { thing: DOTS, item: 5 }, by: 1.0 },
                Action::Remove { ball: Party { thing: ThingId(99), item: 0 } },
                Action::Spawn {
                    thing: ThingId(99),
                    position: Vec2::ZERO,
                    velocity: Vec2::ZERO,
                    radius: 1.0,
                    color: [0.0; 4],
                },
            ],
            &sims,
            &mut log,
        );
        assert_eq!(w.bodies, before);
        assert!(log.is_empty());
    }

    #[test]
    fn fire_and_note_log_events_without_touching_the_ball() {
        let (mut w, sims) = world();
        let mut log = Log::default();
        w.apply(
            vec![
                Action::Fire { name: "boom".into(), ball: B0, point: Vec2::new(1.0, 1.0) },
                Action::Note { ball: B0, loudness: 400.0 },
            ],
            &sims,
            &mut log,
        );
        let e = log.events();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].kind.clone(), e[0].point), (EventKind::Custom("boom".into()), Vec2::new(1.0, 1.0)));
        assert_eq!((e[1].kind.clone(), e[1].speed, e[1].a), (EventKind::Note, 400.0, B0));
        // Grown past the biggest size, a ball stops at 400.
        w.apply(vec![Action::Grow { ball: B0, by: 1000.0 }], &sims, &mut log);
        assert_eq!(w.bodies[&DOTS].radius[0], 400.0);
    }
}
