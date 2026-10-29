//! The arena's wall, turning and shrinking.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Outline, ThingSim, Wall, World, state_mut};
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
}

impl ArenaState {
    /// The outline `base` turned by `angle` and scaled by `scale`.
    fn outline(&self) -> Outline {
        match self.base {
            Outline::Ring { center, radius, sides, angle, thickness } => {
                Outline::Ring { center, radius: radius * self.scale, sides, angle: angle + self.angle, thickness }
            }
            Outline::Box { center, half, angle, thickness } => {
                Outline::Box { center, half: half * self.scale, angle: angle + self.angle, thickness }
            }
        }
    }

    /// The smallest `scale` the settings allow.
    fn floor(&self) -> f32 {
        let size = match self.base {
            Outline::Ring { radius, .. } => radius,
            Outline::Box { half, .. } => half.min_element(),
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
        ArenaShape::Box => {
            Outline::Box { center, half: Vec2::new(settings.width, settings.height) * 0.5, angle, thickness }
        }
    }
}

impl ThingSim for ArenaSim {
    fn kind(&self) -> &'static str {
        "arena"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = ArenaSettings::from_params(&resolved.params);
        let rotation = thing.transform.rotation;
        let angle = if settings.shape == ArenaShape::Polygon { rotation + FRAC_PI_2 } else { rotation };
        let base = base_outline(&settings, thing.transform.position, angle);
        let st = ArenaState { settings, base, angle: 0.0, scale: 1.0 };
        world.walls.insert(thing.id, Wall { outline: base, spin: st.settings.spin.to_radians(), growth: 0.0 });
        world.states.insert(thing.id, Box::new(st));
    }

    /// A keyframe moved the radius, thickness, spin or shrink: the base outline takes the new
    /// size around the same middle, and the wall follows at once.
    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = ArenaSettings::from_params(&resolved.params);
        let growth = world.walls.get(&thing).map_or(0.0, |w| w.growth);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<ArenaState>(st.as_mut());
        let (center, angle) = match st.base {
            Outline::Ring { center, angle, .. } | Outline::Box { center, angle, .. } => (center, angle),
        };
        st.base = base_outline(&settings, center, angle);
        st.settings = settings;
        let wall = Wall { outline: st.outline(), spin: st.settings.spin.to_radians(), growth };
        world.walls.insert(thing, wall);
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        let dt = world.dt();
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
        let wall = Wall { outline: st.outline(), spin, growth: if shrinking { -shrink } else { 0.0 } };
        world.walls.insert(thing, wall);
    }

    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::ARENA;
    use crate::{kit, new_scene};
    use bpe_core::{Address, Owner, Value, resolve};
    use bpe_runtime::{Log, Outline, World};

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
}
