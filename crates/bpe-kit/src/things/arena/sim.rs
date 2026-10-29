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
            ArenaShape::Box => Outline::Box {
                center,
                half: glam::Vec2::new(settings.width, settings.height) * 0.5,
                angle: rotation,
                thickness,
            },
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
