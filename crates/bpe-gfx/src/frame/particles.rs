//! Particles drawn as sprites, and smoke and flame fields.

use super::*;

/// One particle (or a round bit of a trail) as the GPU draws it: the CPU has worked out where it
/// is and how it looks. Matches `SpriteIn` in sprites.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Particle {
    /// Centre (world pixels), turn (radians), half size (pixels; a ring's radius).
    pub place: [f32; 4],
    /// Linear light. A negative alpha adds `|alpha|` of the light and covers nothing.
    pub color: [f32; 4],
    /// Half length along the turn (sparks, streaks, confetti, shards), shade (1 as it is),
    /// softness 0 to 1 (see [`StreakStyle`]), band 0 to 1 (0 filled, else only the outer share
    /// of its size shows).
    pub style: [f32; 4],
    /// Its shape: 0 sparks, 1 smoke, 2 confetti, 3 stars, 4 shards, 5 rings, 6 dots, 7 hearts,
    /// 8 bubbles, 9 streaks; three spare.
    pub shape: [u32; 4],
    /// Its layer's Lit, how much it blocks light and how much it bends light (its index of
    /// refraction, glass and light part 3: above 1 it is free glass), read only when the scene is
    /// lit; one spare.
    pub light: [f32; 4],
    /// The light it gives off, straight linear rgb at full cover (read only in the emission
    /// pass); w spare.
    pub emit: [f32; 4],
    /// Lighting part 4, as [`LayerGpu::more`]: UV glow, only under UV, catches beams, link number.
    pub extra: [f32; 4],
}

impl Particle {
    /// A particle of `shape` at `at`, turned `angle` radians, `size` pixels (half size), in
    /// `color`, shaded 1, crisp and filled.
    pub fn new(shape: u32, at: Vec2, angle: f32, size: f32, color: [f32; 4]) -> Self {
        Self {
            place: [at.x, at.y, angle, size],
            color,
            style: [size, 1.0, 0.0, 0.0],
            shape: [shape, 0, 0, 0],
            light: [0.0; 4],
            emit: [0.0; 4],
            extra: [0.0; 4],
        }
    }

    /// Where it is.
    pub fn at(&self) -> Vec2 {
        Vec2::new(self.place[0], self.place[1])
    }
}

/// A field's look: smoke, shaded by its density.
pub const FIELD_SMOKE: f32 = 0.0;

/// A field's look: flame, shaded through its heat colours.
pub const FIELD_FLAME: f32 = 1.0;

/// How many stops a field's heat colours are baked into.
pub const FIELD_HEAT_STOPS: usize = 32;

/// One soft blob a [`Field`] is splatted from. Matches `SplatIn` in fields.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Splat {
    /// Centre (world pixels), radius (world pixels), weight (how much it adds where it is thickest).
    pub place: [f32; 4],
    /// Its colour (straight, linear light); w spare.
    pub color: [f32; 4],
}

/// How a [`Field`] is shaded where it is drawn. Matches `FieldLook` in fields.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct FieldLook {
    /// Mode ([`FIELD_SMOKE`], [`FIELD_FLAME`]), adds (1: adds its light and covers nothing),
    /// amount (0 to 1, times what it shows), strength (smoke's Thickness, flame's Heat).
    pub kind: [f32; 4],
    /// Noise detail: amount 0 to 1, size (world pixels), drift x, drift y (world pixels a second).
    pub detail: [f32; 4],
    /// Smoke: shading, the way the light comes from (radians), dark inside, relief (world pixels
    /// it stands up where thickest: above 0, in a lit scene, the real lights shade it instead of
    /// its own light, W15). Flame: bend (world pixels), flicker 0 to 1, flicker speed (per
    /// second), brightness.
    pub shade: [f32; 4],
    /// As [`Particle::light`], [`Particle::emit`] and [`Particle::extra`] (lighting).
    pub light: [f32; 4],
    pub emit: [f32; 4],
    pub extra: [f32; 4],
    /// Flame's heat colours from cold to hot (straight, linear, alpha), evenly spaced.
    pub heat: [[f32; 4]; FIELD_HEAT_STOPS],
}

impl Default for FieldLook {
    fn default() -> Self {
        Self {
            kind: [FIELD_SMOKE, 0.0, 1.0, 1.0],
            detail: [0.0, 60.0, 0.0, 0.0],
            shade: [0.0; 4],
            light: [0.0; 4],
            emit: [0.0; 4],
            extra: [0.0; 4],
            heat: [[1.0; 4]; FIELD_HEAT_STOPS],
        }
    }
}

/// Particles drawn as one soft field (Soft smoke, Flame): the GPU splats them into a picture at
/// half size, then shades it where the field sits in its list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    /// How many sprites of the same list (`particles` for `fields`, `particles_front` for
    /// `fields_front`) are drawn before it.
    pub at: u32,
    pub splats: Vec<Splat>,
    pub look: FieldLook,
}
