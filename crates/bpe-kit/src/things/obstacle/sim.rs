//! Placing an obstacle's solid pieces and moving them along their path.

use std::f32::consts::{FRAC_PI_2, TAU};

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Pose, Solid, Solids, ThingSim, World, state};
use glam::Vec2;

use super::{MovePath, ObstacleSettings, ObstacleShape};

pub struct ObstacleSim;

/// What an obstacle keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct ObstacleState {
    pub settings: ObstacleSettings,
    /// Its pieces before any turning or moving, relative to its middle.
    pub parts: Vec<Solid>,
}

/// The obstacle's pieces, relative to its middle and unturned.
pub fn parts(s: &ObstacleSettings) -> Vec<Solid> {
    let half = s.thickness * 0.5;
    match s.shape {
        ObstacleShape::Peg => vec![Solid::Disc { center: Vec2::ZERO, radius: s.radius }],
        ObstacleShape::Bar => {
            vec![Solid::Capsule {
                from: Vec2::new(-s.length * 0.5, 0.0),
                to: Vec2::new(s.length * 0.5, 0.0),
                radius: half,
            }]
        }
        ObstacleShape::PegGrid => {
            let (columns, rows) = (s.columns.max(1), s.rows.max(1));
            let size = Vec2::new((columns - 1) as f32, (rows - 1) as f32) * s.spacing;
            let origin = -size * 0.5;
            let mut out = Vec::with_capacity((columns * rows) as usize);
            for row in 0..rows {
                let shifted = s.stagger && row % 2 == 1;
                let count = if shifted { columns.saturating_sub(1).max(1) } else { columns };
                let shift = if shifted && columns > 1 { s.spacing * 0.5 } else { 0.0 };
                for column in 0..count {
                    let center = origin + Vec2::new(column as f32 * s.spacing + shift, row as f32 * s.spacing);
                    out.push(Solid::Disc { center, radius: s.radius });
                }
            }
            out
        }
        ObstacleShape::Polygon => {
            let sides = s.sides.max(3);
            let corner = |i: u32| {
                let angle = -FRAC_PI_2 + TAU * (i % sides) as f32 / sides as f32;
                Vec2::new(angle.cos(), angle.sin()) * s.radius
            };
            (0..sides).map(|i| Solid::Capsule { from: corner(i), to: corner(i + 1), radius: half }).collect()
        }
        ObstacleShape::Ring => vec![Solid::Ring { center: Vec2::ZERO, radius: s.radius, thickness: s.thickness }],
    }
}

/// Where the path has taken the obstacle at `time`, and how fast it is moving there.
pub fn path_offset(s: &ObstacleSettings, time: f64) -> (Vec2, Vec2) {
    let seconds = s.path_seconds.max(1e-3);
    let phase = TAU * (time as f32 / seconds + s.path_phase);
    let rate = TAU / seconds;
    match s.path {
        MovePath::Still => (Vec2::ZERO, Vec2::ZERO),
        MovePath::Line => (s.path_distance * phase.sin(), s.path_distance * phase.cos() * rate),
        MovePath::Circle => {
            let r = s.path_distance.x;
            (Vec2::new(phase.cos(), phase.sin()) * r, Vec2::new(-phase.sin(), phase.cos()) * r * rate)
        }
    }
}

/// A piece turned by `turn` and moved to `at`.
fn placed(part: &Solid, turn: Vec2, at: Vec2) -> Solid {
    let move_point = |p: Vec2| at + turn.rotate(p);
    match *part {
        Solid::Disc { center, radius } => Solid::Disc { center: move_point(center), radius },
        Solid::Capsule { from, to, radius } => Solid::Capsule { from: move_point(from), to: move_point(to), radius },
        Solid::Ring { center, radius, thickness } => Solid::Ring { center: move_point(center), radius, thickness },
    }
}

impl ObstacleSim {
    /// Works out where the obstacle is now and writes its pose and pieces into the world.
    fn place(&self, thing: ThingId, world: &mut World) {
        let Some(st) = world.states.get(&thing) else { return };
        let st = state::<ObstacleState>(st.as_ref());
        let time = world.time();
        let base = world.anchor(thing);
        let (offset, drift) = path_offset(&st.settings, time);
        let spin = st.settings.spin.to_radians();
        let angle = base.angle + spin * time as f32;
        let pose =
            Pose { position: base.position + offset, angle, velocity: base.velocity + drift, spin: base.spin + spin };
        let turn = Vec2::from_angle(angle);
        let parts = st.parts.iter().map(|p| placed(p, turn, pose.position)).collect();
        world.poses.insert(thing, pose);
        world.solids.insert(thing, Solids::new(pose, parts));
    }
}

impl ThingSim for ObstacleSim {
    fn kind(&self) -> &'static str {
        "obstacle"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = ObstacleSettings::from_params(&resolved.params);
        let parts = parts(&settings);
        world.states.insert(thing.id, Box::new(ObstacleState { settings, parts }));
        self.place(thing.id, world);
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        self.place(thing, world);
    }

    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;
    use std::time::{Duration, Instant};

    use bpe_core::{Attach, Value, resolve};
    use bpe_runtime::{Log, Solid, World};
    use glam::Vec2;

    use super::*;
    use crate::testing::{BALLS, OBSTACLE, peg_board, scene_with_obstacle};
    use crate::{kit, new_scene};

    #[test]
    fn parts_follow_the_shape_settings() {
        let mut s = ObstacleSettings::default();
        assert_eq!(parts(&s), vec![Solid::Disc { center: Vec2::ZERO, radius: 30.0 }]);
        s.shape = ObstacleShape::Bar;
        assert_eq!(
            parts(&s),
            vec![Solid::Capsule { from: Vec2::new(-200.0, 0.0), to: Vec2::new(200.0, 0.0), radius: 4.0 }]
        );
        s.shape = ObstacleShape::PegGrid;
        s.columns = 3;
        s.rows = 2;
        s.spacing = 10.0;
        s.stagger = true;
        let pegs = parts(&s);
        assert_eq!(pegs.len(), 5, "a staggered second row has one peg fewer");
        assert!(
            matches!(pegs[3], Solid::Disc { center, .. } if (center - Vec2::new(-5.0, 5.0)).length() < 1e-4),
            "{pegs:?}"
        );
        s.stagger = false;
        assert_eq!(parts(&s).len(), 6);
        s.shape = ObstacleShape::Polygon;
        s.sides = 4;
        let edges = parts(&s);
        assert_eq!(edges.len(), 4);
        assert!(matches!(edges[0], Solid::Capsule { from, .. } if (from - Vec2::new(0.0, -30.0)).length() < 1e-4));
        s.shape = ObstacleShape::Ring;
        assert_eq!(parts(&s), vec![Solid::Ring { center: Vec2::ZERO, radius: 30.0, thickness: 8.0 }]);
    }

    #[test]
    fn a_bar_on_a_line_path_moves_back_and_forth() {
        let s = ObstacleSettings {
            shape: ObstacleShape::Bar,
            path: MovePath::Line,
            path_distance: Vec2::new(100.0, 0.0),
            path_seconds: 4.0,
            ..Default::default()
        };
        assert!((path_offset(&s, 1.0).0 - Vec2::new(100.0, 0.0)).length() < 1e-3);
        assert!((path_offset(&s, 3.0).0 - Vec2::new(-100.0, 0.0)).length() < 1e-3);
        assert!(path_offset(&s, 0.0).1.x > 100.0, "fastest in the middle");
        let circle = ObstacleSettings { path: MovePath::Circle, ..s.clone() };
        assert!((path_offset(&circle, 0.0).0 - Vec2::new(100.0, 0.0)).length() < 1e-3);
        assert!((path_offset(&circle, 1.0).0 - Vec2::new(0.0, 100.0)).length() < 1e-3);
        assert_eq!(path_offset(&ObstacleSettings::default(), 7.0), (Vec2::ZERO, Vec2::ZERO));

        let (scene, resolved, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("bar".into()));
            p.set("spin", Value::Number(90.0));
        });
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.poses[&OBSTACLE].angle, 0.0);
        w.advance_to(1.0, &kit.sims, &mut Log::default());
        let pose = w.poses[&OBSTACLE];
        assert!((pose.angle - FRAC_PI_2).abs() < 0.01, "{pose:?}");
        assert!((pose.spin - FRAC_PI_2).abs() < 1e-4);
        assert!(
            matches!(w.solids[&OBSTACLE].parts[0], Solid::Capsule { from, .. } if from.x.abs() < 1.0 && from.y < -190.0),
            "{:?}",
            w.solids[&OBSTACLE].parts
        );
    }

    #[test]
    fn balls_bounce_off_an_obstacle_and_log_it() {
        let (scene, resolved, kit) = scene_with_obstacle(|p| p.set("radius", Value::Number(40.0)));
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(3.0, &kit.sims, &mut log);
        assert!(log.events().iter().any(|e| e.b.is_some_and(|b| b.thing == OBSTACLE)), "{:?}", log.events());
        assert!(w.bodies[&BALLS].position[0].is_finite());
    }

    #[test]
    fn a_big_peg_board_steps_a_second_in_under_a_second() {
        // 20 x 20 pegs and a 20 x 20 block of balls.
        let (scene, resolved, kit) = peg_board(20, 20, 20);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.solids[&OBSTACLE].parts.len(), 20 * 20 - 10);
        let started = Instant::now();
        let mut log = Log::default();
        w.advance_to(1.0, &kit.sims, &mut log);
        let took = started.elapsed();
        assert!(took < Duration::from_secs(if cfg!(debug_assertions) { 6 } else { 1 }), "{took:?}");
        assert!(log.events().iter().any(|e| e.b.is_some_and(|b| b.thing == OBSTACLE)), "balls hit pegs");
    }

    #[test]
    fn a_peg_riding_a_ball_follows_it() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let peg = crate::add_thing(&mut scene, &kit.registry, "obstacle", BALLS).unwrap();
        scene
            .set_attach(peg, Attach::Ride { offset: Vec2::new(0.0, 60.0), turn_with_parent: false, item: Some(0) })
            .unwrap();
        scene.things.get_mut(&peg).unwrap().params.set("radius", Value::Number(5.0));
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        w.advance_to(0.5, &kit.sims, &mut Log::default());
        // Riders are placed at the start of each step, so they trail a moving ball by at most
        // one step's travel.
        let ball = w.bodies[&BALLS].position[0];
        let lag = w.bodies[&BALLS].velocity[0].length() * w.dt();
        let off = (w.poses[&peg].position - (ball + Vec2::new(0.0, 60.0))).length();
        assert!(off <= lag + 1e-3, "{off} > {lag}");
        assert!(
            matches!(w.solids[&peg].parts[0], Solid::Disc { center, .. } if (center - w.poses[&peg].position).length() < 1e-3)
        );
    }
}
