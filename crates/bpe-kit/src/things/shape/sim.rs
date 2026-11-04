//! The shape's wall, turning and shrinking.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Outline, Pose, ThingSim, Wall, World, state_mut};
use glam::Vec2;

use super::{KIND, ShapeFill, ShapeSettings, ShapeStart};

pub struct ShapeSim;

/// What the shape keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeState {
    pub settings: ShapeSettings,
    /// The outline at the start, before any turning or shrinking.
    pub base: Outline,
    /// How it was turned at the start (its Rotation, and its parent's when it rides one).
    pub rotation: f32,
    /// How far it has turned since, in radians clockwise.
    pub angle: f32,
    /// Its size as a fraction of the start.
    pub scale: f32,
    /// Where its parent put it at the start, when it rides one.
    pub anchor: Pose,
}

impl ShapeState {
    /// The outline `base` turned by `angle` and scaled by `scale`, carried along by however far
    /// its anchor has moved and turned since the start.
    pub fn outline(&self, anchor: &Pose) -> Outline {
        let center = anchor.position;
        let turn = self.angle + (anchor.angle - self.anchor.angle);
        match &self.base {
            &Outline::Ring { radius, sides, angle, thickness, .. } => {
                Outline::Ring { center, radius: radius * self.scale, sides, angle: angle + turn, thickness }
            }
            &Outline::Box { half, angle, thickness, .. } => {
                Outline::Box { center, half: half * self.scale, angle: angle + turn, thickness }
            }
            Outline::Path { angle, scale, points, thickness, .. } => Outline::Path {
                center,
                angle: angle + turn,
                scale: scale * self.scale,
                points: points.clone(),
                thickness: *thickness,
            },
        }
    }

    /// The smallest `scale` the settings allow.
    fn floor(&self) -> f32 {
        (self.settings.min_size / self.settings.size.max(1e-3)).min(1.0)
    }

    fn wall(&self, anchor: &Pose, growth: f32) -> Wall {
        Wall {
            outline: self.outline(anchor),
            solid: self.settings.fill == ShapeFill::Filled,
            spin: self.settings.spin.to_radians(),
            growth,
            velocity: anchor.velocity,
        }
    }
}

/// The outline the settings describe at full size, around `center`, turned by `turn`. Circles,
/// even polygons and rectangles lined up with the shape take the exact Ring and Box forms (an
/// even polygon is a Ring turned a further quarter, which puts its corners where
/// `ShapePath::even` has them); anything else is a free outline. A filled shape has no thickness.
pub fn base_outline(settings: &ShapeSettings, center: Vec2, turn: f32) -> Outline {
    let thickness = if settings.fill == ShapeFill::Filled { 0.0 } else { settings.thickness };
    let size = settings.size;
    match (settings.start, settings.moulded()) {
        (ShapeStart::Circle, _) => Outline::Ring { center, radius: size, sides: 0, angle: turn, thickness },
        (ShapeStart::Polygon, None) => Outline::Ring {
            center,
            radius: size,
            sides: settings.sides.clamp(3, bpe_runtime::MAX_SIDES as u32),
            angle: turn + FRAC_PI_2,
            thickness,
        },
        (ShapeStart::Polygon, Some(path)) => match path.is_axis_rectangle() {
            Some(half) => Outline::Box { center, half: half * size, angle: turn, thickness },
            None => Outline::Path {
                center,
                angle: turn,
                scale: size,
                points: path.flatten(bpe_core::shape_path::FLATTEN).into(),
                thickness,
            },
        },
    }
}

impl ThingSim for ShapeSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = ShapeSettings::from_params(&resolved.params);
        let anchor = world.anchor(thing.id);
        let rotation = anchor.angle;
        let base = base_outline(&settings, anchor.position, rotation);
        let st = ShapeState { settings, base, rotation, angle: 0.0, scale: 1.0, anchor };
        let spin = st.settings.spin.to_radians();
        world.walls.insert(thing.id, st.wall(&anchor, 0.0));
        world.poses.insert(thing.id, Pose { spin: anchor.spin + spin, ..anchor });
        world.states.insert(thing.id, Box::new(st));
    }

    /// A keyframe moved the size, thickness, spin or shrink: the base outline takes the new
    /// size around the same middle, and the wall follows at once.
    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = ShapeSettings::from_params(&resolved.params);
        let growth = world.walls.get(&thing).map_or(0.0, |w| w.growth);
        let anchor = world.anchor(thing);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<ShapeState>(st.as_mut());
        st.base = base_outline(&settings, st.base.center(), st.rotation);
        st.settings = settings;
        let wall = st.wall(&anchor, growth);
        world.walls.insert(thing, wall);
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        let dt = world.dt();
        let anchor = world.anchor(thing);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<ShapeState>(st.as_mut());
        let spin = st.settings.spin.to_radians();
        let shrink = st.settings.shrink;
        st.angle += spin * dt;
        let floor = st.floor();
        let shrinking = shrink > 0.0 && st.scale > floor;
        if shrinking {
            st.scale = (st.scale * (1.0 - shrink * dt)).max(floor);
        }
        let wall = st.wall(&anchor, if shrinking { -shrink } else { 0.0 });
        world.walls.insert(thing, wall);
        world.poses.insert(
            thing,
            Pose {
                position: anchor.position,
                angle: anchor.angle + st.angle,
                velocity: anchor.velocity,
                spin: anchor.spin + spin,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::SHAPE;
    use crate::{kit, new_scene};
    use bpe_core::{Address, Scene, ShapePath, Value, resolve};
    use bpe_runtime::{Log, Outline, World};

    fn world(scene: &Scene) -> World {
        let kit = kit();
        World::build(scene, &resolve(scene, &kit.registry), &kit.sims)
    }

    fn with(settings: ShapeSettings) -> Scene {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&SHAPE).unwrap().params = settings.to_params();
        scene
    }

    #[test]
    fn a_size_key_resizes_the_wall_while_running() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let size = Address { thing: SHAPE, slot: None, key: "size".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(508.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(254.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(0.0, &kit.sims, &mut log);
        let ring = |w: &World| match w.walls[&SHAPE].outline {
            Outline::Ring { radius, center, .. } => (radius, center),
            ref other => panic!("{other:?}"),
        };
        assert_eq!(ring(&w).0, 508.0);
        w.advance_to(0.5, &kit.sims, &mut log);
        let (half_way, center) = ring(&w);
        assert!(half_way < 500.0 && half_way > 260.0, "{half_way}");
        assert_eq!(center, scene.things[&SHAPE].transform.position);
        w.advance_to(1.5, &kit.sims, &mut log);
        assert!((ring(&w).0 - 254.0).abs() < 1e-3, "{}", ring(&w).0);
    }

    #[test]
    fn a_size_key_resizes_a_moulded_outline_around_the_same_middle() {
        let kit = kit();
        let mut settings = ShapeSettings::polygon(5, 300.0, 4.0);
        let mut path = ShapePath::even(5);
        path.create_curve(1);
        settings.path = path;
        let mut scene = with(settings);
        scene.things.get_mut(&SHAPE).unwrap().transform.position = glam::Vec2::new(40.0, -60.0);
        let size = Address { thing: SHAPE, slot: None, key: "size".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(300.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(150.0)).unwrap();
        let mut w = world(&scene);
        let mut log = Log::default();
        w.advance_to(1.5, &kit.sims, &mut log);
        match &w.walls[&SHAPE].outline {
            Outline::Path { scale, center, thickness, .. } => {
                assert!((scale - 150.0).abs() < 1e-3, "{scale}");
                assert_eq!(*center, glam::Vec2::new(40.0, -60.0));
                assert_eq!(*thickness, 4.0);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn each_form_of_outline_is_picked() {
        let kind = |s: ShapeSettings| world(&with(s)).walls[&SHAPE].clone();
        let circle = kind(ShapeSettings::circle(500.0, 3.0));
        assert!(matches!(circle.outline, Outline::Ring { sides: 0, radius: 500.0, .. }) && !circle.solid);
        let hexagon = kind(ShapeSettings::polygon(6, 450.0, 3.0));
        assert!(matches!(hexagon.outline, Outline::Ring { sides: 6, radius: 450.0, angle, .. } if angle == FRAC_PI_2));
        let rect = kind(ShapeSettings::rectangle(900.0, 1500.0, 3.0));
        assert!(matches!(rect.outline, Outline::Box { half, .. } if half == Vec2::new(450.0, 750.0)), "{rect:?}");
        let mut moulded = ShapeSettings::polygon(6, 450.0, 3.0);
        moulded.path = ShapePath::even(7);
        let path = kind(moulded);
        assert!(matches!(&path.outline, Outline::Path { points, scale: 450.0, .. } if points.len() == 7));
        let mut curved = ShapeSettings::polygon(6, 450.0, 3.0);
        curved.path = ShapePath::circle();
        let flat = kind(curved);
        assert!(matches!(&flat.outline, Outline::Path { points, .. } if points.len() == 8 * 16));
    }

    #[test]
    fn a_filled_shape_is_solid_with_no_thickness() {
        let mut s = ShapeSettings::circle(300.0, 12.0);
        s.fill = ShapeFill::Filled;
        let wall = world(&with(s)).walls[&SHAPE].clone();
        assert!(wall.solid);
        assert_eq!(wall.outline.thickness(), 0.0);
    }

    #[test]
    fn every_setting_affects_the_physics() {
        for d in ShapeSettings::defs() {
            assert!(ShapeSim.affects_physics(d.key), "{}", d.key);
        }
    }

    /// The blank test scene's shape with `settings`, its Size jumping from `from` to `to` at 0.5 s
    /// (a Hold key), and the ball resting at `at` with no gravity.
    fn jumping(settings: ShapeSettings, from: f64, to: f64, at: glam::Vec2) -> Scene {
        let kit = kit();
        let mut scene = with(settings);
        scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let ball = scene.things.get_mut(&crate::testing::BALLS).unwrap();
        ball.transform.position = at;
        ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
        let size = Address { thing: SHAPE, slot: None, key: "size".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(from)).unwrap();
        scene.set_key(&kit.registry, &size, 0.5, Value::Number(to)).unwrap();
        assert!(scene.set_ease(&size, 0, bpe_core::Ease::Hold));
        scene
    }

    #[test]
    fn a_ball_inside_stays_inside_when_the_shape_jumps_smaller() {
        // A ring and a moulded outline: a ball between the old and the new size is pulled in.
        let mut moulded = ShapeSettings::polygon(6, 500.0, 4.0);
        moulded.path = ShapePath::even(7);
        for settings in [ShapeSettings::circle(500.0, 4.0), moulded] {
            let scene = jumping(settings, 500.0, 250.0, glam::Vec2::new(0.0, 380.0));
            let mut w = world(&scene);
            w.advance_to(1.0, &kit().sims, &mut Log::default());
            let p = w.bodies[&crate::testing::BALLS].position[0];
            assert!(w.walls[&SHAPE].outline.inside(p), "the ball was left outside, at {p}");
            assert!(p.length() < 250.0, "{p}");
        }
    }

    #[test]
    fn a_shrinking_moulded_shape_stops_at_its_smallest_size() {
        let mut s = ShapeSettings::polygon(6, 400.0, 3.0);
        s.path = ShapePath::even(5);
        s.shrink = 0.5;
        s.min_size = 100.0;
        let mut w = world(&with(s));
        w.advance_to(10.0, &kit().sims, &mut Log::default());
        assert!(matches!(w.walls[&SHAPE].outline, Outline::Path { scale, .. } if (scale - 100.0).abs() < 1e-2));
    }
}
