//! The arena's wall, turning and shrinking.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Outline, Pose, ThingSim, Wall, World, state_mut};
use glam::Vec2;

use super::{ArenaSettings, ArenaShape};

pub struct ArenaSim;

/// What the arena keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct ArenaState {
    pub settings: ArenaSettings,
    /// The outline at the start, before any turning or shrinking.
    pub base: Outline,
    /// How far it has turned, in radians clockwise.
    pub angle: f32,
    /// Its size as a fraction of the start.
    pub scale: f32,
    /// Where its parent put it at the start, when it rides one.
    pub anchor: Pose,
}

impl ArenaState {
    /// The outline `base` turned by `angle` and scaled by `scale`, carried along by however far
    /// its anchor has moved and turned since the start.
    fn outline(&self, anchor: &Pose) -> Outline {
        let center = anchor.position;
        let turn = self.angle + (anchor.angle - self.anchor.angle);
        match self.base {
            Outline::Ring { radius, sides, angle, thickness, .. } => {
                Outline::Ring { center, radius: radius * self.scale, sides, angle: angle + turn, thickness }
            }
            Outline::Box { half, angle, thickness, .. } => {
                Outline::Box { center, half: half * self.scale, angle: angle + turn, thickness }
            }
            Outline::Rounded { half, corner, angle, thickness, .. } => Outline::Rounded {
                center,
                half: half * self.scale,
                corner: corner * self.scale,
                angle: angle + turn,
                thickness,
            },
        }
    }

    /// The smallest `scale` the settings allow.
    fn floor(&self) -> f32 {
        let size = match self.base {
            Outline::Ring { radius, .. } => radius,
            Outline::Box { half, .. } | Outline::Rounded { half, .. } => half.min_element(),
        };
        (self.settings.min_size / size.max(1e-3)).min(1.0)
    }
}

/// The outline the settings describe at full size, around `center`, turned by `angle` (a
/// polygon's angle already includes the quarter turn that puts a corner at the top).
fn base_outline(settings: &ArenaSettings, center: Vec2, angle: f32) -> Outline {
    let thickness = settings.thickness;
    match settings.shape {
        ArenaShape::Circle => Outline::Ring { center, radius: settings.radius, sides: 0, angle, thickness },
        ArenaShape::Polygon => {
            Outline::Ring { center, radius: settings.radius, sides: settings.sides.max(3), angle, thickness }
        }
        ArenaShape::Box if settings.corner <= 0.0 => {
            Outline::Box { center, half: Vec2::new(settings.width, settings.height) * 0.5, angle, thickness }
        }
        ArenaShape::Box | ArenaShape::Capsule => {
            let half = Vec2::new(settings.width, settings.height) * 0.5;
            let corner = match settings.shape {
                ArenaShape::Capsule => half.min_element(),
                _ => settings.corner.min(half.min_element()),
            };
            Outline::Rounded { center, half, corner, angle, thickness }
        }
    }
}

impl ThingSim for ArenaSim {
    fn kind(&self) -> &'static str {
        "arena"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = ArenaSettings::from_params(&resolved.params);
        let anchor = world.anchor(thing.id);
        let rotation = anchor.angle;
        let angle = if settings.shape == ArenaShape::Polygon { rotation + FRAC_PI_2 } else { rotation };
        let base = base_outline(&settings, anchor.position, angle);
        let st = ArenaState { settings, base, angle: 0.0, scale: 1.0, anchor };
        let spin = st.settings.spin.to_radians();
        world.walls.insert(thing.id, Wall { outline: base, spin, growth: 0.0, velocity: anchor.velocity });
        world.poses.insert(thing.id, Pose { spin: anchor.spin + spin, ..anchor });
        world.states.insert(thing.id, Box::new(st));
    }

    /// A keyframe moved the radius, thickness, spin or shrink: the base outline takes the new
    /// size around the same middle, and the wall follows at once.
    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = ArenaSettings::from_params(&resolved.params);
        let growth = world.walls.get(&thing).map_or(0.0, |w| w.growth);
        let anchor = world.anchor(thing);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<ArenaState>(st.as_mut());
        let (center, angle) = match st.base {
            Outline::Ring { center, angle, .. }
            | Outline::Box { center, angle, .. }
            | Outline::Rounded { center, angle, .. } => (center, angle),
        };
        st.base = base_outline(&settings, center, angle);
        st.settings = settings;
        let spin = st.settings.spin.to_radians();
        let wall = Wall { outline: st.outline(&anchor), spin, growth, velocity: anchor.velocity };
        world.walls.insert(thing, wall);
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        let dt = world.dt();
        let anchor = world.anchor(thing);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<ArenaState>(st.as_mut());
        let spin = st.settings.spin.to_radians();
        let shrink = st.settings.shrink;
        st.angle += spin * dt;
        let floor = st.floor();
        let shrinking = shrink > 0.0 && st.scale > floor;
        if shrinking {
            st.scale = (st.scale * (1.0 - shrink * dt)).max(floor);
        }
        let outline = st.outline(&anchor);
        let wall = Wall { outline, spin, growth: if shrinking { -shrink } else { 0.0 }, velocity: anchor.velocity };
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

    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::ARENA;
    use crate::{kit, new_scene};
    use bpe_core::{Address, Owner, Scene, Value, resolve};
    use bpe_runtime::{Log, Outline, World};
    use glam::Vec2;

    #[test]
    fn a_radius_key_resizes_the_wall_while_running() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let radius = Address { owner: Owner::Thing(ARENA), slot: None, key: "radius".into() };
        scene.set_key(&kit.registry, &radius, 0.0, Value::Number(508.0)).unwrap();
        scene.set_key(&kit.registry, &radius, 1.0, Value::Number(254.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(0.0, &kit.sims, &mut log);
        let ring = |w: &World| match w.walls[&ARENA].outline {
            Outline::Ring { radius, center, .. } => (radius, center),
            other => panic!("{other:?}"),
        };
        assert_eq!(ring(&w).0, 508.0);
        w.advance_to(0.5, &kit.sims, &mut log);
        let (half_way, center) = ring(&w);
        assert!(half_way < 500.0 && half_way > 260.0, "{half_way}");
        assert_eq!(center, scene.things[&ARENA].transform.position);
        w.advance_to(1.5, &kit.sims, &mut log);
        assert!((ring(&w).0 - 254.0).abs() < 1e-3, "{}", ring(&w).0);
    }

    #[test]
    fn corner_and_capsule_make_rounded_outlines_and_zero_corner_stays_a_box() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let arena = scene.things.get_mut(&ARENA).unwrap();
        arena.params.set("shape", Value::Choice("box".into()));
        arena.params.set("width", Value::Number(200.0));
        arena.params.set("height", Value::Number(400.0));
        let world = |scene: &Scene| World::build(scene, &resolve(scene, &kit.registry), &kit.sims);
        assert!(matches!(world(&scene).walls[&ARENA].outline, Outline::Box { .. }));
        scene.things.get_mut(&ARENA).unwrap().params.set("corner", Value::Number(500.0));
        assert!(matches!(world(&scene).walls[&ARENA].outline, Outline::Rounded { corner, .. } if corner == 100.0));
        scene.things.get_mut(&ARENA).unwrap().params.set("corner", Value::Number(30.0));
        assert!(matches!(world(&scene).walls[&ARENA].outline, Outline::Rounded { corner, .. } if corner == 30.0));
        scene.things.get_mut(&ARENA).unwrap().params.set("shape", Value::Choice("capsule".into()));
        scene.things.get_mut(&ARENA).unwrap().params.remove("corner");
        assert!(matches!(
            world(&scene).walls[&ARENA].outline,
            Outline::Rounded { corner, half, .. } if corner == 100.0 && half == Vec2::new(100.0, 200.0)
        ));
        // Shrinking keeps the corners in proportion.
        scene.things.get_mut(&ARENA).unwrap().params.set("shrink", Value::Number(0.5));
        scene.things.get_mut(&ARENA).unwrap().params.set("min_size", Value::Number(10.0));
        let mut w = world(&scene);
        w.advance_to(1.0, &kit.sims, &mut bpe_runtime::Log::default());
        assert!(
            matches!(w.walls[&ARENA].outline, Outline::Rounded { corner, half, .. } if corner < 100.0 && corner == half.min_element())
        );
    }
}
