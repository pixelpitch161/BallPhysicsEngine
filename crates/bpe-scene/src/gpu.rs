//! Huge swarms and liquids that run on the graphics card.
//!
//! Both only look at the simulation: balls push them around and walls keep them in, but they
//! never push balls back, make sound or set off rules. That keeps the exported video exactly
//! like the preview and lets them use millions of points.

use crate::{Color, Dice};
use serde::{Deserialize, Serialize};

/// Most swarm points the editor offers.
pub const MAX_SWARM_POINTS: u32 = 1_000_000;
/// Most liquid particles the editor offers.
pub const MAX_FLUID_PARTICLES: u32 = 300_000;
/// Fewest liquid particles worth running on the graphics card. Smaller amounts run on the CPU
/// with the jelly balls and ropes, where they can make sound.
pub const MIN_GPU_FLUID_PARTICLES: u32 = 8_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuLayers {
    pub swarm: Swarm,
    pub fluid: GpuFluid,
}

impl GpuLayers {
    pub fn is_active(&self) -> bool {
        self.swarm.is_active() || self.fluid.is_active()
    }
}

/// A cloud of tiny glowing points that flow around, flock together and follow the balls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Swarm {
    pub enabled: bool,
    /// How many points, up to [`MAX_SWARM_POINTS`].
    pub count: u32,
    /// Where the points start.
    pub start: SwarmStart,
    /// Point size in pixels.
    pub size: f32,
    /// How bright each point is. Many points on top of each other add up.
    pub brightness: f32,
    /// Fastest a point moves, in pixels per second.
    pub max_speed: f32,
    /// Fraction of speed lost per second.
    pub drag: f32,
    /// Strength of the swirling flow that carries points around.
    pub flow: f32,
    /// Size of the swirls in pixels.
    pub flow_scale: f32,
    /// How quickly the swirls change, in turns per second.
    pub flow_speed: f32,
    /// How strongly points steer with their neighbours, like a flock of birds.
    pub flock: f32,
    /// How strongly points keep apart when crowded.
    pub separation: f32,
    /// Pull towards balls. Negative pushes points away from balls instead.
    pub attraction: f32,
    /// How far from a ball its pull reaches, in pixels.
    pub reach: f32,
    /// How strongly walls push points back.
    pub avoid_walls: f32,
    /// Pull in pixels per second squared, on top of everything else. Positive pulls down.
    pub gravity: f32,
    pub color: SwarmColor,
    /// Colours for [`SwarmColor::Speed`] (slow to fast) and [`SwarmColor::Gradient`].
    pub colors: Vec<Color>,
    pub seed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmStart {
    /// Spread over the whole arena.
    Fill,
    /// Bunched up in the middle.
    Center,
    /// Around the inside of the arena wall.
    Ring,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmColor {
    /// From the first colour when still to the last when fast.
    Speed,
    /// The colour of the nearest ball.
    NearestBall,
    /// Each point keeps one colour along the gradient.
    Gradient,
}

/// Liquid that fills the arena, splashing around the balls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuFluid {
    pub enabled: bool,
    /// Number of liquid particles. More gives a smoother, more detailed surface.
    pub count: u32,
    /// How much of the arena the liquid fills, 0 to 1.
    pub fill: f32,
    /// Seconds over which the liquid pours in from the top. 0 starts it already in place.
    pub pour_seconds: f32,
    pub color: Color,
    /// How see-through the liquid is, 0 (solid colour) to 1 (clear).
    pub clarity: f32,
    /// How much the liquid bends the picture behind it.
    pub refraction: f32,
    /// Brightness of the shine on the surface.
    pub highlight: f32,
    /// How thick and slow the liquid is, 0 (water) to 1 (honey).
    pub viscosity: f32,
    /// How hard the liquid resists being squashed. Higher is less bouncy but needs more care.
    pub stiffness: f32,
    /// How strongly balls push the liquid aside.
    pub ball_push: f32,
    /// Pull on the liquid in pixels per second squared. Positive pulls down.
    pub gravity: f32,
    /// Smooths the liquid's surface, in pixels.
    pub smoothing: f32,
    pub seed: u64,
}

impl Default for Swarm {
    fn default() -> Self {
        Self {
            enabled: false,
            count: 200_000,
            start: SwarmStart::Fill,
            size: 1.5,
            brightness: 0.25,
            max_speed: 260.0,
            drag: 0.6,
            flow: 1.0,
            flow_scale: 180.0,
            flow_speed: 0.05,
            flock: 0.4,
            separation: 0.3,
            attraction: 0.6,
            reach: 220.0,
            avoid_walls: 1.0,
            gravity: 0.0,
            color: SwarmColor::Speed,
            colors: vec![Color::rgb(0x20, 0x40, 0xff), Color::rgb(0x2e, 0xd8, 0xe6), Color::rgb(0xff, 0xf2, 0xc0)],
            seed: 1,
        }
    }
}

impl Default for GpuFluid {
    fn default() -> Self {
        Self {
            enabled: false,
            count: 60_000,
            fill: 0.35,
            pour_seconds: 0.0,
            color: Color::rgb(0x2e, 0x8c, 0xff),
            clarity: 0.45,
            refraction: 0.5,
            highlight: 0.8,
            viscosity: 0.2,
            stiffness: 0.5,
            ball_push: 1.0,
            gravity: 900.0,
            smoothing: 6.0,
            seed: 1,
        }
    }
}

impl Swarm {
    pub fn is_active(&self) -> bool {
        self.enabled && self.count > 0 && self.size > 0.0 && self.brightness > 0.0
    }

    /// The point count actually used, within what the editor allows.
    pub fn points(&self) -> u32 {
        self.count.min(MAX_SWARM_POINTS)
    }

    /// Rolls new movement and colours. The point count stays, since it sets how heavy the scene is.
    pub fn roll(&mut self, dice: &mut Dice, amount: f32) {
        let d = Swarm::default();
        let mix = |dice: &mut Dice, current: f32, lo: f32, hi: f32| current + (dice.range(lo, hi) - current) * amount;
        self.flow = mix(dice, self.flow, 0.0, 2.5);
        self.flow_scale = mix(dice, self.flow_scale, 60.0, 400.0);
        self.flock = mix(dice, self.flock, 0.0, 1.5);
        self.separation = mix(dice, self.separation, 0.0, 1.0);
        self.attraction = mix(dice, self.attraction, -1.5, 2.0);
        self.reach = mix(dice, self.reach, 80.0, 500.0);
        self.max_speed = mix(dice, self.max_speed, 120.0, 600.0);
        self.brightness = mix(dice, self.brightness, 0.08, 0.6);
        if dice.chance(amount) {
            self.color = dice.pick(&[SwarmColor::Speed, SwarmColor::NearestBall, SwarmColor::Gradient]);
            self.colors = crate::palette(dice, 3);
        }
        self.max_speed = self.max_speed.max(d.max_speed * 0.25);
        self.seed = dice.next_u64();
    }
}

impl GpuFluid {
    pub fn is_active(&self) -> bool {
        self.enabled && self.count > 0 && self.fill > 0.0
    }

    /// The particle count actually used, within what the editor allows.
    pub fn particles(&self) -> u32 {
        self.count.min(MAX_FLUID_PARTICLES)
    }

    pub fn roll(&mut self, dice: &mut Dice, amount: f32) {
        let mix = |dice: &mut Dice, current: f32, lo: f32, hi: f32| current + (dice.range(lo, hi) - current) * amount;
        self.fill = mix(dice, self.fill, 0.15, 0.6);
        self.viscosity = mix(dice, self.viscosity, 0.0, 0.8);
        self.clarity = mix(dice, self.clarity, 0.1, 0.8);
        self.refraction = mix(dice, self.refraction, 0.0, 1.0);
        self.highlight = mix(dice, self.highlight, 0.2, 1.5);
        if dice.chance(amount) {
            self.color = Color::from_hsv(dice.unit(), dice.range(0.5, 0.95), dice.range(0.6, 1.0));
        }
        self.seed = dice.next_u64();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_round_trips() {
        let layers = GpuLayers::default();
        assert!(!layers.is_active());
        let on = GpuLayers {
            swarm: Swarm { enabled: true, ..Swarm::default() },
            fluid: GpuFluid { enabled: true, ..GpuFluid::default() },
        };
        assert!(on.is_active());
        let text = ron::to_string(&on).unwrap();
        assert_eq!(ron::from_str::<GpuLayers>(&text).unwrap(), on);
        let partial: GpuLayers = ron::from_str("(swarm: (enabled: true))").unwrap();
        assert!(partial.swarm.is_active() && !partial.fluid.is_active());
    }

    #[test]
    fn counts_stay_within_limits() {
        let swarm = Swarm { enabled: true, count: u32::MAX, ..Swarm::default() };
        assert_eq!(swarm.points(), MAX_SWARM_POINTS);
        let fluid = GpuFluid { enabled: true, count: u32::MAX, ..GpuFluid::default() };
        assert_eq!(fluid.particles(), MAX_FLUID_PARTICLES);
        assert!(!Swarm { enabled: true, count: 0, ..Swarm::default() }.is_active());
        assert!(!GpuFluid { enabled: true, fill: 0.0, ..GpuFluid::default() }.is_active());
    }

    #[test]
    fn rolls_are_repeatable_and_finite() {
        let (mut a, mut b) = (Swarm::default(), Swarm::default());
        a.roll(&mut Dice::new(3), 1.0);
        b.roll(&mut Dice::new(3), 1.0);
        assert_eq!(a, b);
        assert!(a.max_speed > 0.0 && a.flow.is_finite());
        let mut f = GpuFluid::default();
        f.roll(&mut Dice::new(4), 1.0);
        assert!((0.0..=1.0).contains(&f.fill));
        let mut same = Swarm::default();
        same.roll(&mut Dice::new(5), 0.0);
        assert_eq!(same.flow, Swarm::default().flow);
    }
}
