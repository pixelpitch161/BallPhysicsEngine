//! The arena's wall, turning and shrinking.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Outline, ThingSim, Wall, World, state_mut};

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
            Outline::Rounded { center, half, corner, angle, thickness } => Outline::Rounded {
                center,
                half: half * self.scale,
                corner: corner * self.scale,
                angle: angle + self.angle,
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

impl ThingSim for ArenaSim {
    fn kind(&self) -> &'static str {
        "arena"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = ArenaSettings::from_params(&resolved.params);
        let center = thing.transform.position;
        let rotation = thing.transform.rotation;
        let thickness = settings.thickness;
        let base = match settings.shape {
            ArenaShape::Circle => {
                Outline::Ring { center, radius: settings.radius, sides: 0, angle: rotation, thickness }
            }
            ArenaShape::Polygon => Outline::Ring {
                center,
                radius: settings.radius,
                sides: settings.sides.max(3),
                angle: rotation + FRAC_PI_2,
                thickness,
            },
            ArenaShape::Box if settings.corner <= 0.0 => Outline::Box {
                center,
                half: glam::Vec2::new(settings.width, settings.height) * 0.5,
                angle: rotation,
                thickness,
            },
            ArenaShape::Box | ArenaShape::Capsule => {
                let half = glam::Vec2::new(settings.width, settings.height) * 0.5;
                let corner = match settings.shape {
                    ArenaShape::Capsule => half.min_element(),
                    _ => settings.corner.min(half.min_element()),
                };
                Outline::Rounded { center, half, corner, angle: rotation, thickness }
            }
        };
        let st = ArenaState { settings, base, angle: 0.0, scale: 1.0 };
        world.walls.insert(thing.id, Wall { outline: base, spin: st.settings.spin.to_radians(), growth: 0.0 });
        world.states.insert(thing.id, Box::new(st));
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
    use bpe_core::{Scene, Value, resolve};
    use bpe_runtime::{Outline, World};
    use glam::Vec2;

    use crate::testing::ARENA;
    use crate::{kit, new_scene};

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
