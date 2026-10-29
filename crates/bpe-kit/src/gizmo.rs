//! Gizmos: the dashed outlines the editor draws for a thing that has nothing visible of its
//! own, or whose region matters, so it can be seen, picked and dragged in the preview.

use bpe_core::{ResolvedThing, Settings, Thing};
use bpe_runtime::World;
use glam::Vec2;

use crate::things::emitter::EmitterSettings;
use crate::things::field::{FieldKindChoice, FieldSettings, FieldShape};
use crate::things::liquid::sim::{region, wall_of};
use crate::things::liquid::{LiquidSettings, LiquidStart};

/// One outline, in world pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gizmo {
    Dot(Vec2),
    Circle(Vec2, f32),
    Rect {
        center: Vec2,
        half: Vec2,
        angle: f32,
    },
    /// From a point to where it points.
    Arrow(Vec2, Vec2),
    Line(Vec2, Vec2),
}

/// How far from a gizmo the preview still grabs its thing, in world pixels.
pub const GIZMO_GRAB: f32 = 14.0;
/// How long a direction arrow is.
const ARROW: f32 = 80.0;
/// How long the lines showing an emitter's spread are.
const SPREAD: f32 = 60.0;

fn dir(degrees: f32) -> Vec2 {
    let r = degrees.to_radians();
    Vec2::new(r.cos(), r.sin())
}

/// What the editor draws for `thing`: emitter (dot, direction arrow, the two edges of its
/// spread), field (its circle or box, and an arrow for the wind; nothing when everywhere),
/// liquid (a block's box, or a pool's level line), swarm (a dot). Nothing for other kinds.
/// A field everywhere shows only its middle, so it can still be grabbed.
pub fn gizmos(thing: &Thing, resolved: &ResolvedThing, world: &World) -> Vec<Gizmo> {
    let at = thing.transform.position;
    match resolved.kind.as_str() {
        "emitter" => {
            let s = EmitterSettings::from_params(&resolved.params);
            let half = s.spread.abs() * 0.5;
            vec![
                Gizmo::Dot(at),
                Gizmo::Arrow(at, at + dir(s.direction) * ARROW),
                Gizmo::Line(at, at + dir(s.direction - half) * SPREAD),
                Gizmo::Line(at, at + dir(s.direction + half) * SPREAD),
            ]
        }
        "field" => {
            let s = FieldSettings::from_params(&resolved.params);
            let mut g = match s.shape {
                FieldShape::Circle => vec![Gizmo::Circle(at, s.radius)],
                FieldShape::Box => {
                    vec![Gizmo::Rect {
                        center: at,
                        half: Vec2::new(s.width, s.height) * 0.5,
                        angle: thing.transform.rotation,
                    }]
                }
                // A field everywhere has no edge, but its middle still matters (and must be
                // something to grab): a dot, or the wind's arrow.
                FieldShape::Everywhere if s.kind != FieldKindChoice::Wind => vec![Gizmo::Dot(at)],
                FieldShape::Everywhere => Vec::new(),
            };
            if s.kind == FieldKindChoice::Wind {
                g.push(Gizmo::Arrow(at, at + dir(s.direction) * ARROW));
            }
            g
        }
        "liquid" => {
            let s = LiquidSettings::from_params(&resolved.params);
            match s.start {
                LiquidStart::Block => {
                    vec![Gizmo::Rect { center: at, half: Vec2::new(s.width, s.height) * 0.5, angle: 0.0 }]
                }
                LiquidStart::Pool => {
                    let wall = wall_of(world, at).map(|(_, w)| w.outline);
                    let r = region(&s, wall.as_ref(), at);
                    vec![Gizmo::Line(Vec2::new(r.min.x, r.min.y), Vec2::new(r.max.x, r.min.y))]
                }
            }
        }
        "swarm" => vec![Gizmo::Dot(at)],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scene_with;
    use bpe_core::Value;
    use bpe_runtime::World;
    use glam::Vec2;

    fn gizmos_of(kind: &str, position: Vec2, f: impl FnOnce(&mut bpe_core::Params)) -> Vec<Gizmo> {
        let (scene, resolved, kit, me) = scene_with(kind, "It", position, f);
        let world = World::build(&scene, &resolved, &kit.sims);
        gizmos(&scene.things[&me], &resolved.things[&me], &world)
    }

    #[test]
    fn an_emitter_gizmo_points_along_its_direction() {
        let g = gizmos_of("emitter", Vec2::new(100.0, 200.0), |_| {});
        assert_eq!(g[0], Gizmo::Dot(Vec2::new(100.0, 200.0)));
        let Gizmo::Arrow(from, to) = g[1] else { panic!("{g:?}") };
        assert_eq!(from, Vec2::new(100.0, 200.0));
        assert!((to - Vec2::new(100.0, 120.0)).length() < 1e-3, "{to}: 80 px along -90°");
        let lines = g.iter().filter(|g| matches!(g, Gizmo::Line(..))).count();
        assert_eq!(lines, 2, "{g:?}: the two edges of the spread");
    }

    #[test]
    fn a_box_field_gizmo_is_a_rect() {
        let g = gizmos_of("field", Vec2::new(-50.0, 0.0), |p| {
            p.set("shape", Value::Choice("box".into()));
            p.set("kind", Value::Choice("well".into()));
        });
        assert_eq!(g, vec![Gizmo::Rect { center: Vec2::new(-50.0, 0.0), half: Vec2::new(300.0, 200.0), angle: 0.0 }]);
        let round = gizmos_of("field", Vec2::ZERO, |p| p.set("kind", Value::Choice("wind".into())));
        assert_eq!(round[0], Gizmo::Circle(Vec2::ZERO, 300.0));
        assert!(matches!(round[1], Gizmo::Arrow(..)), "{round:?}: the wind shows its direction");
        let everywhere = gizmos_of("field", Vec2::new(1.0, 2.0), |p| {
            p.set("shape", Value::Choice("everywhere".into()));
            p.set("kind", Value::Choice("vortex".into()));
        });
        assert_eq!(everywhere, vec![Gizmo::Dot(Vec2::new(1.0, 2.0))], "no edge, but a handle");
        let wind_everywhere = gizmos_of("field", Vec2::ZERO, |p| p.set("shape", Value::Choice("everywhere".into())));
        assert_eq!(wind_everywhere, vec![Gizmo::Arrow(Vec2::ZERO, Vec2::new(80.0, 0.0))]);
    }

    #[test]
    fn a_pool_gizmo_is_its_fill_line() {
        let g = gizmos_of("liquid", Vec2::ZERO, |p| p.set("count", Value::Int(50)));
        let [Gizmo::Line(a, b)] = g[..] else { panic!("{g:?}") };
        assert_eq!(a.y, b.y, "level");
        assert!(a.x < b.x && a.y > 0.0, "{a} {b}: below the middle of the arena");
        let block = gizmos_of("liquid", Vec2::new(10.0, 20.0), |p| {
            p.set("count", Value::Int(50));
            p.set("start", Value::Choice("block".into()));
        });
        assert_eq!(
            block,
            vec![Gizmo::Rect { center: Vec2::new(10.0, 20.0), half: Vec2::new(150.0, 150.0), angle: 0.0 }]
        );
        assert_eq!(gizmos_of("swarm", Vec2::new(3.0, 4.0), |_| {}), vec![Gizmo::Dot(Vec2::new(3.0, 4.0))]);
        assert!(gizmos_of("balls", Vec2::ZERO, |_| {}).is_empty());
    }
}
