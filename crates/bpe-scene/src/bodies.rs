//! Soft things in the arena: squishy jelly balls, ropes, cloth and a small amount of liquid,
//! plus the settings for balls bumping into each other.

use crate::{Color, Vec2};
use serde::{Deserialize, Serialize};

/// Most liquid particles the CPU simulates. More than this is drawn by the graphics card's
/// liquid instead, which looks the same but can't make sounds or trigger rules.
pub const CPU_FLUID_LIMIT: u32 = 8_000;
/// Stacking quality used when "Balls bump into each other" is first turned on.
pub const GOOD_CONTACT_QUALITY: u32 = 8;

/// How balls bump into each other when `Physics::ball_collisions` is on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BallContact {
    /// Solver passes per step. More makes piles and stacks stand still. 0 is the classic
    /// one-pass push-apart that older scenes were made with.
    pub quality: u32,
    /// How much balls grip each other as they slide past, 0 for ice.
    pub friction: f32,
    /// Whether balls spin when they rub against each other.
    pub spin: bool,
    /// Fraction of spin lost per second.
    pub spin_drag: f32,
    /// Whether balls that have come to rest stop moving entirely until something hits them.
    pub sleep: bool,
    /// Holes that balls fall into, like a billiard table's pockets. A ball whose middle goes
    /// over one leaves the scene, with an escape event.
    pub pockets: Vec<Pocket>,
}

/// A hole that swallows balls.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pocket {
    pub position: Vec2,
    pub radius: f32,
}

impl Default for BallContact {
    fn default() -> Self {
        Self { quality: 0, friction: 0.2, spin: false, spin_drag: 0.5, sleep: true, pockets: Vec::new() }
    }
}

/// Everything soft in the scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bodies {
    pub jelly: Vec<Jelly>,
    pub ropes: Vec<Rope>,
    pub cloths: Vec<Cloth>,
    pub fluid: Fluid,
    /// Solver passes per step for soft bodies. More is stiffer and steadier but slower.
    pub iterations: u32,
}

impl Default for Bodies {
    fn default() -> Self {
        Self { jelly: Vec::new(), ropes: Vec::new(), cloths: Vec::new(), fluid: Fluid::default(), iterations: 8 }
    }
}

impl Bodies {
    /// Whether there is anything soft to simulate.
    pub fn is_empty(&self) -> bool {
        self.jelly.iter().all(|j| j.count() == 0)
            && self.ropes.is_empty()
            && self.cloths.is_empty()
            && (self.fluid.count == 0 || self.fluid.on_gpu())
    }
}

/// A block of squishy balls: each is a ring of points held round by the air inside it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Jelly {
    pub center: Vec2,
    pub columns: u32,
    pub rows: u32,
    /// Distance between neighbouring jelly balls' centres.
    pub spacing: f32,
    pub radius: f32,
    pub velocity: Vec2,
    /// Points around each ball's edge, 8 to 64. More is rounder and slower.
    pub points: u32,
    /// 0 is nearly solid, 1 is very wobbly.
    pub squishiness: f32,
    /// Colours from the first ball to the last, spread evenly.
    pub gradient: Vec<Color>,
}

impl Default for Jelly {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            columns: 1,
            rows: 1,
            spacing: 120.0,
            radius: 50.0,
            velocity: Vec2::ZERO,
            points: 24,
            squishiness: 0.5,
            gradient: vec![Color::rgb(0x5c, 0xe6, 0x9c)],
        }
    }
}

impl Jelly {
    pub fn count(&self) -> u32 {
        self.columns * self.rows
    }
}

/// A rope or chain hanging between two points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rope {
    pub from: Vec2,
    pub to: Vec2,
    /// Links along the rope, 2 to 200.
    pub points: u32,
    /// Rope length compared to the straight distance between its ends. Above 1 it sags.
    pub slack: f32,
    pub pin_start: bool,
    pub pin_end: bool,
    /// Width in pixels.
    pub thickness: f32,
    /// 0 is stretchy like elastic, 1 is a stiff cable.
    pub stiffness: f32,
    pub color: Color,
}

impl Default for Rope {
    fn default() -> Self {
        Self {
            from: Vec2::new(-300.0, -200.0),
            to: Vec2::new(300.0, -200.0),
            points: 40,
            slack: 1.15,
            pin_start: true,
            pin_end: true,
            thickness: 8.0,
            stiffness: 1.0,
            color: Color::rgb(0xff, 0xb8, 0x2e),
        }
    }
}

/// Which points of a cloth are held in place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClothPin {
    /// Free to fall.
    None,
    /// The whole top edge, like a curtain.
    #[default]
    TopRow,
    /// Only the two top corners, like a hammock.
    TopCorners,
}

/// A sheet of cloth: a grid of points tied to their neighbours.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cloth {
    pub center: Vec2,
    pub size: Vec2,
    /// Points across and down, 2 to 80 each.
    pub columns: u32,
    pub rows: u32,
    pub pin: ClothPin,
    /// 0 is stretchy, 1 holds its shape.
    pub stiffness: f32,
    pub color: Color,
}

impl Default for Cloth {
    fn default() -> Self {
        Self {
            center: Vec2::new(0.0, -100.0),
            size: Vec2::new(600.0, 500.0),
            columns: 30,
            rows: 25,
            pin: ClothPin::TopRow,
            stiffness: 0.9,
            color: Color::rgb(0x8a, 0x4d, 0xff),
        }
    }
}

/// Liquid poured into the arena as a block of particles that flow together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fluid {
    /// Number of particles. 0 means no liquid. Above [`CPU_FLUID_LIMIT`] the graphics card
    /// simulates it.
    pub count: u32,
    /// Middle and size of the block the liquid starts in.
    pub center: Vec2,
    pub size: Vec2,
    pub velocity: Vec2,
    /// Size of one particle in pixels. Smaller is smoother and needs more particles.
    pub radius: f32,
    /// 0 flows like water, 1 like honey.
    pub thickness: f32,
    pub color: Color,
}

impl Default for Fluid {
    fn default() -> Self {
        Self {
            count: 0,
            center: Vec2::new(0.0, -200.0),
            size: Vec2::new(500.0, 300.0),
            velocity: Vec2::ZERO,
            radius: 6.0,
            thickness: 0.2,
            color: Color::rgb(0x2e, 0x9b, 0xff),
        }
    }
}

impl Fluid {
    /// Whether this much liquid is too much for the CPU and goes to the graphics card.
    pub fn on_gpu(&self) -> bool {
        self.count > CPU_FLUID_LIMIT
    }
}
