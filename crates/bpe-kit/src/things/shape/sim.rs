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
    /// Its outline in shape units, moulded or not (`ShapeSettings::outline_path`): the corners
    /// and sides a hit's Place and Side are read against.
    pub path: bpe_core::ShapePath,
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
            &Outline::Ellipse { half, angle, thickness, .. } => {
                Outline::Ellipse { center, half: half * self.scale, angle: angle + turn, thickness }
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
    pub fn floor(&self) -> f32 {
        (self.settings.min_size / self.settings.unit().max(1e-3)).min(1.0)
    }

    fn wall(&self, anchor: &Pose, growth: f32) -> Wall {
        Wall {
            outline: self.outline(anchor),
            solid: self.settings.fill == ShapeFill::Filled,
            // Its own spin on top of its parent's, when it turns with what it rides.
            spin: anchor.spin + self.settings.spin.to_radians(),
            growth,
            velocity: anchor.velocity,
            passes: self.settings.passes,
        }
    }
}

/// The outline the settings describe at full size, around `center`, turned by `turn`. Circles,
/// even polygons and rectangles lined up with the shape take the exact Ring and Box forms (an
/// even polygon is a Ring turned a further quarter, which puts its corners where
/// `ShapePath::even` has them); an ellipse is exact (a Ring when it is round), turned by its Turn
/// on top; anything else is a free outline. A filled shape has no thickness.
pub fn base_outline(settings: &ShapeSettings, center: Vec2, turn: f32) -> Outline {
    let thickness = if settings.fill == ShapeFill::Filled { 0.0 } else { settings.thickness };
    let size = settings.size;
    match (settings.start, settings.moulded()) {
        (ShapeStart::Ellipse, _) => {
            let half = Vec2::new(settings.width, settings.height) * 0.5;
            let angle = turn + settings.turn.to_radians();
            if half.x == half.y {
                Outline::Ring { center, radius: half.x, sides: 0, angle, thickness }
            } else {
                Outline::Ellipse { center, half, angle, thickness }
            }
        }
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
        let path = settings.outline_path();
        let st = ShapeState { settings, base, rotation, angle: 0.0, scale: 1.0, anchor, path };
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
        let s = &st.settings;
        // An ellipse's path follows its Width, Height and Turn, but nothing reads it while it is an
        // ellipse (`side_on` and `place_on_edge` read the wall), so keys on them don't remake it
        // (which would allocate every step).
        if (s.start, s.sides, &s.path) != (settings.start, settings.sides, &settings.path) {
            st.path = settings.outline_path();
        }
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

/// Which side of `shape` the point `point` (in scene pixels, on or near its outline) is on, 0
/// first, in the order of the shape's corners (a circle counts as its 8 curved sides, an ellipse
/// as one side). 0 when `shape` is not a shape in `world`.
pub fn side_of(world: &World, shape: ThingId, point: Vec2) -> u32 {
    let Some(st) = world.states.get(&shape).and_then(|s| s.as_any().downcast_ref::<ShapeState>()) else {
        return 0;
    };
    side_on(world, shape, &st.path, point)
}

/// [`side_of`] with the shape's outline in shape units (`ShapeSettings::outline_path`) made
/// beforehand, so it never allocates (the sound engine asks on every hit).
pub fn side_on(world: &World, shape: ThingId, path: &bpe_core::ShapePath, point: Vec2) -> u32 {
    let Some(st) = world.states.get(&shape).and_then(|s| s.as_any().downcast_ref::<ShapeState>()) else {
        return 0;
    };
    if st.settings.start == ShapeStart::Ellipse {
        return 0;
    }
    path.nearest_side(in_shape_units(world, shape, st, point)).map_or(0, |(side, _, _)| side as u32)
}

/// `point` (in scene pixels) in shape `shape`'s own units, as it stands in `world` now.
fn in_shape_units(world: &World, shape: ThingId, st: &ShapeState, point: Vec2) -> Vec2 {
    let anchor = world.anchor(shape);
    let turn = st.rotation + st.angle + (anchor.angle - st.anchor.angle);
    let size = (st.settings.unit() * st.scale).max(1e-3);
    Vec2::from_angle(-turn).rotate(point - anchor.position) / size
}

/// Where on shape `shape`'s outline (as it is in `world` now) the point nearest `point` is: the
/// way round clockwise from its first corner as a fraction (0..1) of the whole way, the side
/// (as [`side_on`] has it, so side k is By side's slot k), and how many sides it has (a circle
/// has its 8; an ellipse is one side, its places measured round from its own top by the angle
/// it is drawn with). `None` when the world has no such shape. Never allocates.
pub fn place_on_edge(world: &World, shape: ThingId, point: Vec2) -> Option<(f32, usize, usize)> {
    let st = world.states.get(&shape)?.as_any().downcast_ref::<ShapeState>()?;
    if st.settings.start == ShapeStart::Ellipse {
        let outline = &world.walls.get(&shape)?.outline;
        let (center, half, angle) = match *outline {
            Outline::Ellipse { center, half, angle, .. } => (center, half, angle),
            Outline::Ring { center, radius, angle, .. } => (center, Vec2::splat(radius), angle),
            _ => return Some((0.0, 0, 1)),
        };
        let own = Vec2::from_angle(-angle).rotate(point - center);
        let q = bpe_runtime::nearest_on_ellipse(half, own) / half.max(Vec2::splat(1e-6));
        let place = (q.x.atan2(-q.y) / std::f32::consts::TAU).rem_euclid(1.0);
        return Some((place, 0, 1));
    }
    let path = &st.path;
    let n = path.side_count();
    let (side, t, _) = path.nearest_side(in_shape_units(world, shape, st, point))?;
    let (mut total, mut at) = (0.0f32, 0.0f32);
    for s in 0..n {
        if s == side {
            at = total + side_length(path, s, t);
        }
        total += side_length(path, s, 1.0);
    }
    let place = if total > 0.0 { (at / total).clamp(0.0, 1.0) } else { 0.0 };
    Some((place, side, n))
}

/// How long side `side` of `path` is from its corner to `t` along it: a curve measured over
/// [`bpe_core::shape_path::FLATTEN`] straight pieces.
fn side_length(path: &bpe_core::ShapePath, side: usize, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if path.bends[side].is_none() {
        let (a, b) = path.side_ends(side);
        return (b - a).length() * t;
    }
    let pieces = bpe_core::shape_path::FLATTEN;
    let mut length = 0.0;
    let mut last = path.side_point(side, 0.0);
    for k in 1..=pieces {
        let next = path.side_point(side, t * k as f32 / pieces as f32);
        length += (next - last).length();
        last = next;
    }
    length
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
    fn side_of_follows_the_corners_and_the_turn() {
        let kit = kit();
        let square = ShapeSettings { sides: 4, ..ShapeSettings::default() };
        let scene = with(square.clone());
        let w = world(&scene);
        let mid = scene.things[&SHAPE].transform.position;
        let path = square.outline_path();
        let size = square.size;
        // The middle of each side, out in the scene, is on that side.
        for side in 0..4 {
            let p = mid + path.side_point(side, 0.5) * size;
            assert_eq!(side_of(&w, SHAPE, p), side as u32, "side {side}");
        }
        assert_eq!(side_of(&w, crate::testing::BALLS, mid), 0, "not a shape");
        // Turned a quarter: each side's middle is where the next one's was.
        let turned = with(ShapeSettings { sides: 4, ..ShapeSettings::default() });
        let mut turned = turned;
        turned.things.get_mut(&SHAPE).unwrap().transform.rotation = FRAC_PI_2;
        let w = World::build(&turned, &resolve(&turned, &kit.registry), &kit.sims);
        let p0 = mid + Vec2::from_angle(FRAC_PI_2).rotate(path.side_point(0, 0.5) * size);
        assert_eq!(side_of(&w, SHAPE, p0), 0);
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

    /// A key at 0 s is the shape's size from the very start, before the first step.
    #[test]
    fn a_size_key_at_the_start_holds_from_the_first_frame() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let size = Address { thing: SHAPE, slot: None, key: "size".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(250.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(250.0)).unwrap();
        let w = world(&scene);
        assert!(matches!(w.walls[&SHAPE].outline, Outline::Ring { radius: 250.0, .. }), "{:?}", w.walls[&SHAPE]);
    }

    /// A shape riding a turning shape, turning with it, turns at both spins, and its wall moves
    /// that fast under the balls.
    #[test]
    fn a_shape_turning_with_its_parent_spins_with_it() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&SHAPE).unwrap().params.set("spin", Value::Number(90.0));
        let inner = crate::add_thing(&mut scene, &kit.registry, "shape", bpe_core::ThingId::ROOT).unwrap();
        let ride = bpe_core::Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: None };
        scene.set_parent(inner, SHAPE, ride).unwrap();
        scene.things.get_mut(&inner).unwrap().params.set("spin", Value::Number(30.0));
        let mut w = world(&scene);
        let want = 120f32.to_radians();
        assert!((w.walls[&inner].spin - want).abs() < 1e-5, "{}", w.walls[&inner].spin);
        w.advance_to(0.5, &kit.sims, &mut Log::default());
        assert!((w.walls[&inner].spin - want).abs() < 1e-5, "{}", w.walls[&inner].spin);
        assert!((w.walls[&SHAPE].spin - 90f32.to_radians()).abs() < 1e-5);
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
        let oval = kind(ShapeSettings::ellipse(900.0, 1300.0, 3.0));
        assert!(
            matches!(oval.outline, Outline::Ellipse { half, angle: 0.0, thickness: 3.0, .. } if half == Vec2::new(450.0, 650.0)),
            "{oval:?}"
        );
        let round = kind(ShapeSettings::ellipse(600.0, 600.0, 3.0));
        assert!(matches!(round.outline, Outline::Ring { radius: 300.0, sides: 0, .. }), "{round:?}");
        let mut turned = ShapeSettings::ellipse(900.0, 1300.0, 3.0);
        turned.turn = 30.0;
        let mut scene = with(turned);
        scene.things.get_mut(&SHAPE).unwrap().transform.rotation = 0.25;
        let angle = world(&scene).walls[&SHAPE].outline.angle();
        assert!((angle - (0.25 + 30f32.to_radians())).abs() < 1e-6, "{angle}");
    }

    #[test]
    fn width_keys_resize_the_ellipse_and_move_its_foci() {
        let kit = kit();
        let mut scene = with(ShapeSettings::ellipse(900.0, 1300.0, 3.0));
        let width = Address { thing: SHAPE, slot: None, key: "width".into() };
        scene.set_key(&kit.registry, &width, 0.0, Value::Number(900.0)).unwrap();
        scene.set_key(&kit.registry, &width, 1.0, Value::Number(500.0)).unwrap();
        let mut w = world(&scene);
        let half = |w: &World| match w.walls[&SHAPE].outline {
            Outline::Ellipse { half, .. } => half,
            ref other => panic!("{other:?}"),
        };
        let gap = |w: &World| {
            let [a, b] = crate::things::shape::foci(w, SHAPE).unwrap();
            (b - a).length()
        };
        assert_eq!(half(&w), Vec2::new(450.0, 650.0));
        assert!((gap(&w) - 2.0 * (650f32.powi(2) - 450f32.powi(2)).sqrt()).abs() < 1e-2);
        w.advance_to(1.5, &kit.sims, &mut Log::default());
        assert!((half(&w) - Vec2::new(250.0, 650.0)).length() < 1e-3, "{}", half(&w));
        assert!((gap(&w) - 2.0 * (650f32.powi(2) - 250f32.powi(2)).sqrt()).abs() < 1e-2);
    }

    #[test]
    fn an_ellipse_spins_and_shrinks_like_a_ring() {
        let mut s = ShapeSettings::ellipse(900.0, 1300.0, 3.0);
        s.spin = 90.0;
        s.shrink = 0.5;
        s.min_size = 325.0;
        let mut w = world(&with(s));
        w.advance_to(10.0, &kit().sims, &mut Log::default());
        match w.walls[&SHAPE].outline {
            Outline::Ellipse { half, angle, .. } => {
                assert!((half - Vec2::new(225.0, 325.0)).length() < 1e-2, "stops at half its size: {half}");
                let turned = (angle - 900f32.to_radians()).rem_euclid(std::f32::consts::TAU);
                assert!(!(1e-2..=std::f32::consts::TAU - 1e-2).contains(&turned), "{angle}");
            }
            ref other => panic!("{other:?}"),
        }
    }

    #[test]
    fn side_on_and_place_treat_an_ellipse_as_one_side() {
        let s = ShapeSettings::ellipse(900.0, 1300.0, 3.0);
        let scene = with(s.clone());
        let w = world(&scene);
        let mid = scene.things[&SHAPE].transform.position;
        let path = s.outline_path();
        for (at, want) in [
            (Vec2::new(0.0, -650.0), 0.0),
            (Vec2::new(450.0, 0.0), 0.25),
            (Vec2::new(0.0, 650.0), 0.5),
            (Vec2::new(-450.0, 0.0), 0.75),
        ] {
            assert_eq!(side_on(&w, SHAPE, &path, mid + at), 0);
            let (place, side, sides) = place_on_edge(&w, SHAPE, mid + at * 1.01).unwrap();
            assert_eq!((side, sides), (0, 1));
            let off = (place - want).abs();
            assert!(off < 1e-3 || (want == 0.0 && off > 1.0 - 1e-3), "{at}: {place} not {want}");
        }
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
