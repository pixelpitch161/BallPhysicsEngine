//! Rolling new values for groups of settings, for the dice buttons and the seed explorer.
//!
//! Every roll stays inside ranges that make a watchable video: balls start inside the arena,
//! the arena fits the frame, and colours come from one palette. `amount` says how far to move
//! from the current values: 1 rolls completely new ones, smaller numbers nudge the current
//! values part of the way towards a roll, and yes/no choices flip only some of the time.

use crate::{ChordStep, Color, Container, Instrument, Scene, Spawner, Vec2};
use std::f32::consts::{PI, TAU};

/// A small repeatable random number generator (SplitMix64), so a seed always rolls the same values.
#[derive(Clone, Debug)]
pub struct Dice {
    state: u64,
}

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self { state: seed ^ 0x5851_f42d_4c95_7f2d }
    }

    /// A dice for one part of a roll, mixed from a shared seed and a number unique to that part,
    /// so each part rolls its own values.
    pub fn mixed(seed: u64, salt: u64) -> Self {
        let mut dice = Self::new(seed.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        dice.next_u64();
        dice
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut x = self.state;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^ (x >> 31)
    }

    /// From 0 up to 1.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit()
    }

    /// Spread evenly on a log scale, for values where 100 to 200 matters as much as 1000 to 2000.
    pub fn log_range(&mut self, min: f32, max: f32) -> f32 {
        (min.ln() + (max.ln() - min.ln()) * self.unit()).exp()
    }

    /// A whole number from `min` to `max`, both included.
    pub fn int(&mut self, min: u32, max: u32) -> u32 {
        if max <= min { min } else { min + (self.next_u64() % (max - min + 1) as u64) as u32 }
    }

    pub fn chance(&mut self, probability: f32) -> bool {
        self.unit() < probability
    }

    pub fn sign(&mut self) -> f32 {
        if self.chance(0.5) { 1.0 } else { -1.0 }
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.int(0, items.len() as u32 - 1) as usize]
    }
}

/// A group of settings that can be rolled together. Each matches a heading in the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// One group of balls, by its index in `Scene::spawners`.
    Balls(usize),
    Shape,
    Movement,
    World,
    Bounces,
    NewBalls,
    Background,
    Trails,
    Glow,
    Strings,
    Chords,
    Notes,
}

impl Group {
    /// The group's name, as shown in the editor.
    pub fn name(self) -> &'static str {
        match self {
            Group::Balls(_) => "ball",
            Group::Shape => "arena shape",
            Group::Movement => "arena movement",
            Group::World => "world",
            Group::Bounces => "bounce",
            Group::NewBalls => "new ball",
            Group::Background => "background",
            Group::Trails => "trail",
            Group::Glow => "glow",
            Group::Strings => "string art",
            Group::Chords => "chord",
            Group::Notes => "note",
        }
    }

    /// A number unique to the group, so each group rolls its own values from a shared seed and
    /// turning one group on or off doesn't change what the others roll.
    fn salt(self) -> u64 {
        match self {
            Group::Balls(index) => 100 + index as u64,
            Group::Shape => 1,
            Group::Movement => 2,
            Group::World => 3,
            Group::Bounces => 4,
            Group::NewBalls => 5,
            Group::Background => 6,
            Group::Trails => 7,
            Group::Glow => 8,
            Group::Strings => 9,
            Group::Chords => 10,
            Group::Notes => 11,
        }
    }
}

/// Furthest the arena reaches from the centre of the frame, so it stays in a 1080 wide video.
const MAX_ARENA_RADIUS: f32 = 520.0;
/// Balls start within this fraction of the space inside the arena, clear of the wall.
const INSIDE: f32 = 0.9;
const ROOTS: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

/// The open space inside the arena where balls can start.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Room {
    Circle { center: Vec2, radius: f32 },
    Box { center: Vec2, half: Vec2 },
}

impl Room {
    /// A random point in the middle part of the room.
    fn point(self, dice: &mut Dice) -> Vec2 {
        match self {
            Room::Circle { center, radius } => {
                let angle = dice.range(0.0, TAU);
                center + Vec2::from_angle(angle) * radius * 0.55 * dice.unit().sqrt()
            }
            Room::Box { center, half } => {
                center + half * 0.55 * Vec2::new(dice.range(-1.0, 1.0), dice.range(-1.0, 1.0))
            }
        }
    }

    /// How much something with half-size `half` must shrink to fit, as a scale from 0 to 1.
    fn fit_scale(self, half: Vec2) -> f32 {
        match self {
            Room::Circle { radius, .. } => (radius * INSIDE / half.length().max(1e-3)).min(1.0),
            Room::Box { half: room, .. } => {
                (room.x * INSIDE / half.x.max(1e-3)).min(room.y * INSIDE / half.y.max(1e-3)).min(1.0)
            }
        }
    }

    /// Moves `position` so something with half-size `half` around it sits inside the room.
    fn clamp(self, position: Vec2, half: Vec2) -> Vec2 {
        match self {
            Room::Circle { center, radius } => {
                let reach = (radius * INSIDE - half.length()).max(0.0);
                let offset = position - center;
                if offset.length() > reach { center + offset.normalize_or_zero() * reach } else { position }
            }
            Room::Box { center, half: room } => {
                let reach = (room * INSIDE - half).max(Vec2::ZERO);
                center + (position - center).clamp(-reach, reach)
            }
        }
    }
}

fn mix(old: f32, rolled: f32, amount: f32) -> f32 {
    old + (rolled - old) * amount
}

fn set(value: &mut f32, rolled: f32, amount: f32) {
    *value = mix(*value, rolled, amount);
}

fn set_whole(value: &mut u32, rolled: u32, amount: f32) {
    *value = mix(*value as f32, rolled as f32, amount).round().max(0.0) as u32;
}

fn set_vec(value: &mut Vec2, rolled: Vec2, amount: f32) {
    *value = value.lerp(rolled, amount);
}

/// A value that is often exactly zero (`zero_chance`), otherwise rolled by `roll`.
fn maybe_zero(dice: &mut Dice, zero_chance: f32, roll: impl FnOnce(&mut Dice) -> f32) -> f32 {
    let zero = dice.chance(zero_chance);
    let value = roll(dice);
    if zero { 0.0 } else { value }
}

/// A launch velocity: any direction, with a speed in the range.
fn velocity(dice: &mut Dice, min_speed: f32, max_speed: f32) -> Vec2 {
    let angle = dice.range(0.0, TAU);
    Vec2::from_angle(angle) * dice.range(min_speed, max_speed)
}

/// A few bright colours that go together: neighbours on the colour wheel, opposites, or a rainbow.
pub fn palette(dice: &mut Dice, count: usize) -> Vec<Color> {
    let base = dice.unit();
    let step = match dice.int(0, 3) {
        0 => dice.range(0.04, 0.1),
        1 => dice.range(0.1, 0.2),
        2 => 0.5 / (count.max(2) - 1) as f32,
        _ => dice.range(0.6, 0.9) / count.max(1) as f32,
    } * dice.sign();
    let saturation = dice.range(0.55, 0.9);
    (0..count)
        .map(|i| {
            let value = dice.range(0.85, 1.0);
            Color::from_hsv(base + step * i as f32, saturation + dice.range(-0.08, 0.08), value)
        })
        .collect()
}

/// Rolls a list of colours. A full roll picks a new palette; a partial one turns the current
/// colours around the colour wheel together, so they still go together.
fn roll_colors(colors: &mut Vec<Color>, dice: &mut Dice, amount: f32, min: u32, max: u32) {
    let count = dice.int(min, max) as usize;
    let fresh = palette(dice, count);
    let turn = dice.range(-0.5, 0.5) * amount;
    if amount >= 0.999 || colors.is_empty() {
        *colors = fresh;
    } else {
        colors.iter_mut().for_each(|c| *c = c.shift_hue(turn));
    }
}

fn roll_color(color: &mut Color, dice: &mut Dice, amount: f32) {
    let mut colors = vec![*color];
    roll_colors(&mut colors, dice, amount, 1, 1);
    *color = colors[0];
}

/// A light line colour: mostly near white, sometimes tinted.
fn line_color(dice: &mut Dice) -> Color {
    let hue = dice.unit();
    if dice.chance(0.55) {
        Color::from_hsv(hue, dice.range(0.0, 0.1), dice.range(0.85, 1.0))
    } else {
        Color::from_hsv(hue, dice.range(0.25, 0.6), 1.0)
    }
}

impl Scene {
    /// How many balls the scene starts with or drops in, before any split off.
    pub fn ball_count(&self) -> u32 {
        self.spawners
            .iter()
            .map(|s| match s {
                Spawner::Grid { columns, rows, .. } => columns * rows,
                Spawner::Single { .. } => 1,
                Spawner::Stream { count, .. } => *count,
            })
            .sum()
    }

    fn room(&self) -> Room {
        let wall = self.container.thickness() * 0.5;
        match self.container {
            Container::Circle { center, radius, .. } => Room::Circle { center, radius: (radius - wall).max(1.0) },
            Container::Polygon { center, radius, sides, .. } => {
                let inner = radius * (PI / sides.max(3) as f32).cos();
                Room::Circle { center, radius: (inner - wall).max(1.0) }
            }
            Container::Rings { center, inner_radius, sides, .. } => {
                let inner = if sides >= 3 { inner_radius * (PI / sides as f32).cos() } else { inner_radius };
                Room::Circle { center, radius: (inner - wall).max(1.0) }
            }
            Container::Rect { center, size, .. } => Room::Box { center, half: (size * 0.5 - wall).max(Vec2::ONE) },
            Container::Custom { center, .. } => {
                Room::Circle { center, radius: (self.container.size() - wall).max(1.0) }
            }
        }
    }

    /// How far pegs and walls reach from the arena's centre: the furthest distance, and the
    /// furthest left/right and up/down.
    fn obstacle_reach(&self) -> (f32, Vec2) {
        let center = self.container.center();
        let mut far = 0.0f32;
        let mut half = Vec2::ZERO;
        let mut reach = |point: Vec2, pad: f32| {
            let offset = (point - center).abs() + pad;
            far = far.max((point - center).length() + pad);
            half = half.max(offset);
        };
        for part in self.obstacle_parts() {
            match part {
                crate::ObstaclePart::Disc { center, radius, .. } => reach(center, radius),
                crate::ObstaclePart::Segment { from, to, thickness, .. } => {
                    reach(from, thickness * 0.5);
                    reach(to, thickness * 0.5);
                }
            }
        }
        (far, half)
    }

    /// Rolls new values for one group of settings. See the module docs for `amount`.
    pub fn roll(&mut self, group: Group, dice: &mut Dice, amount: f32) {
        let amount = amount.clamp(0.0, 1.0);
        match group {
            Group::Balls(index) => self.roll_balls(index, dice, amount),
            Group::Shape => self.roll_shape(dice, amount),
            Group::Movement => self.roll_movement(dice, amount),
            Group::World => self.roll_world(dice, amount),
            Group::Bounces => self.roll_bounces(dice, amount),
            Group::NewBalls => self.roll_new_balls(dice, amount),
            Group::Background => {
                let look = &mut self.look;
                let hue = dice.unit();
                let top = Color::from_hsv(hue, dice.range(0.45, 0.9), dice.range(0.05, 0.16));
                let bottom =
                    Color::from_hsv(hue + dice.range(-0.08, 0.08), dice.range(0.5, 1.0), dice.range(0.0, 0.05));
                if amount >= 0.999 {
                    look.background_top = top;
                    look.background_bottom = bottom;
                } else {
                    let turn = dice.range(-0.5, 0.5) * amount;
                    look.background_top = look.background_top.shift_hue(turn);
                    look.background_bottom = look.background_bottom.shift_hue(turn);
                }
            }
            Group::Trails => {
                let look = &mut self.look;
                set(&mut look.trail_seconds, maybe_zero(dice, 0.15, |d| d.range(0.1, 1.2)), amount);
                set(&mut look.trail_width, dice.range(0.15, 1.0), amount);
                set(&mut look.trail_opacity, dice.range(0.2, 0.9), amount);
            }
            Group::Glow => {
                let look = &mut self.look;
                set(&mut look.ball_glow, dice.range(0.0, 0.5), amount);
                set(&mut look.flash_strength, dice.range(0.0, 1.5), amount);
                set(&mut look.bloom_strength, dice.range(0.05, 0.7), amount);
            }
            Group::Strings => {
                // Each line is drawn every frame, so fewer per ball when there are many balls.
                let most = (3000 / self.ball_count().max(1)).clamp(3, 400);
                let look = &mut self.look;
                let lines = if dice.chance(0.4) { 0 } else { dice.int(3, most) };
                set_whole(&mut look.string_lines, lines, amount);
                set(&mut look.string_opacity, dice.range(0.1, 0.6), amount);
                set(&mut look.string_width, dice.range(0.6, 3.0), amount);
            }
            Group::Chords => {
                let progression = chords(dice);
                if dice.chance(amount) || self.sound.progression.is_empty() {
                    self.sound.progression = progression;
                }
            }
            Group::Notes => {
                let sound = &mut self.sound;
                let low = dice.int(36, 62);
                let high = (low + dice.int(12, 30)).min(96);
                sound.low_note = mix(sound.low_note as f32, low as f32, amount).round() as u8;
                sound.high_note = (mix(sound.high_note as f32, high as f32, amount).round() as u8).max(sound.low_note);
                let (decay, brightness) = (dice.range(0.3, 2.5), dice.range(0.2, 0.9));
                if let Instrument::Pluck { decay: d, brightness: b } = &mut sound.instrument {
                    set(d, decay, amount);
                    set(b, brightness, amount);
                }
                set(&mut sound.stereo_width, dice.range(0.3, 1.0), amount);
                let seed = dice.int(0, 999_999) as u64;
                if dice.chance(amount) {
                    sound.seed = seed;
                }
            }
        }
        if matches!(group, Group::Balls(_) | Group::Shape) {
            self.keep_balls_inside();
        }
    }

    fn roll_balls(&mut self, index: usize, dice: &mut Dice, amount: f32) {
        let room = self.room();
        let room_size = match room {
            Room::Circle { radius, .. } => radius,
            Room::Box { half, .. } => half.min_element(),
        };
        let start = room.point(dice);
        let Some(spawner) = self.spawners.get_mut(index) else { return };
        match spawner {
            Spawner::Grid { center, columns, rows, spacing, radius, velocity: v, gradient, .. } => {
                set_vec(center, start, amount);
                set_whole(columns, dice.int(4, 32), amount);
                set_whole(rows, dice.int(4, 32), amount);
                (*columns, *rows) = ((*columns).max(1), (*rows).max(1));
                set(spacing, dice.range(10.0, 26.0), amount);
                let size = dice.range(0.15, 0.42);
                set(radius, *spacing * size, amount);
                set_vec(v, velocity(dice, 0.0, 700.0), amount);
                roll_colors(gradient, dice, amount, 2, 5);
            }
            Spawner::Single { position, velocity: v, radius, color, .. } => {
                set_vec(position, start, amount);
                set(radius, dice.log_range(8.0, (room_size * 0.15).clamp(9.0, 70.0)), amount);
                set_vec(v, velocity(dice, 250.0, 1100.0), amount);
                roll_color(color, dice, amount);
            }
            Spawner::Stream { position, velocity: v, radius, count, interval, jitter, gradient, .. } => {
                set_vec(position, start, amount);
                set(radius, dice.range(4.0, 18.0), amount);
                set_whole(count, dice.log_range(10.0, 300.0).round() as u32, amount);
                *count = (*count).max(1);
                set(interval, dice.range(0.04, 0.5), amount);
                set(jitter, dice.range(0.0, 30.0), amount);
                set_vec(v, velocity(dice, 0.0, 600.0), amount);
                roll_colors(gradient, dice, amount, 2, 5);
            }
        }
    }

    fn roll_shape(&mut self, dice: &mut Dice, amount: f32) {
        let (far, half) = self.obstacle_reach();
        let line = line_color(dice);
        let thickness = dice.range(1.5, 8.0);
        match &mut self.container {
            Container::Circle { radius, thickness: t, color, .. } => {
                set(radius, dice.range(280.0, MAX_ARENA_RADIUS), amount);
                *radius = radius.max(far + 20.0);
                set(t, thickness, amount);
                set_color(color, line, dice, amount);
            }
            Container::Rect { size, thickness: t, color, .. } => {
                let width = dice.range(500.0, 2.0 * MAX_ARENA_RADIUS);
                let height = dice.range(width * 0.8, 1800.0);
                set_vec(size, Vec2::new(width, height), amount);
                *size = size.max(half * 2.0 + 40.0);
                set(t, thickness, amount);
                set_color(color, line, dice, amount);
            }
            Container::Polygon { radius, sides, rotation, thickness: t, color, .. } => {
                let new_sides = dice.pick(&[3, 4, 5, 6, 6, 7, 8]);
                if dice.chance(amount) {
                    *sides = new_sides;
                }
                set(radius, dice.range(300.0, MAX_ARENA_RADIUS), amount);
                *radius = radius.max((far + 20.0) / (PI / (*sides).max(3) as f32).cos());
                let turn = if dice.chance(0.5) { 0.0 } else { dice.range(-180.0, 180.0) };
                set(rotation, turn, amount);
                set(t, thickness, amount);
                set_color(color, line, dice, amount);
            }
            Container::Rings {
                count,
                inner_radius,
                spacing,
                sides,
                gap,
                spin,
                spin_step,
                gap_offset,
                thickness: t,
                gradient,
                ..
            } => {
                set_whole(count, dice.int(4, 16), amount);
                *count = (*count).max(1);
                set(inner_radius, dice.range(70.0, 180.0), amount);
                set(spacing, dice.range(22.0, 55.0), amount);
                let new_sides = dice.pick(&[0, 0, 0, 3, 4, 5, 6, 8]);
                if dice.chance(amount * 0.5) {
                    *sides = new_sides;
                }
                set(gap, dice.range(25.0, 80.0), amount);
                set(spin, dice.sign() * dice.range(40.0, 140.0), amount);
                set(spin_step, dice.range(-15.0, 15.0), amount);
                set(gap_offset, dice.range(-40.0, 40.0), amount);
                set(t, dice.range(3.0, 9.0), amount);
                roll_colors(gradient, dice, amount, 2, 4);
                // Keep the biggest ring inside the frame.
                if *count > 1 && crate::ring_radius(*inner_radius, *spacing, *count - 1) > MAX_ARENA_RADIUS {
                    *spacing = ((MAX_ARENA_RADIUS - *inner_radius) / (*count - 1) as f32).max(8.0);
                    let fits = ((MAX_ARENA_RADIUS - *inner_radius) / *spacing).floor().max(0.0) as u32 + 1;
                    *count = (*count).min(fits).max(1);
                }
            }
            Container::Custom { thickness: t, color, .. } => {
                set(t, thickness, amount);
                set_color(color, line, dice, amount);
            }
        }
    }

    fn roll_movement(&mut self, dice: &mut Dice, amount: f32) {
        // With many balls the wall is hit hundreds of times a second, so shrinking per hit must be tiny.
        let per_hit_max = (40.0 / self.ball_count().max(1) as f32).clamp(0.02, 4.0);
        let size = self.container.size();
        let motion = &mut self.arena_motion;
        set(&mut motion.spin, maybe_zero(dice, 0.55, |d| d.sign() * d.range(10.0, 120.0)), amount);
        set(&mut motion.shrink_speed, maybe_zero(dice, 0.6, |d| d.range(3.0, 25.0)), amount);
        set(&mut motion.shrink_per_hit, maybe_zero(dice, 0.7, |d| d.range(per_hit_max * 0.1, per_hit_max)), amount);
        set(&mut motion.min_size, dice.range(60.0, (size * 0.5).max(61.0)), amount);
    }

    fn roll_world(&mut self, dice: &mut Dice, amount: f32) {
        let few_balls = self.ball_count() <= 300;
        let physics = &mut self.physics;
        set(&mut physics.gravity.y, maybe_zero(dice, 0.15, |d| d.log_range(60.0, 1200.0)), amount);
        set(&mut physics.restitution, dice.range(0.9, 1.02), amount);
        set(&mut physics.air_drag, maybe_zero(dice, 0.7, |d| d.range(0.02, 0.4)), amount);
        let flip = dice.chance(amount * 0.3);
        if flip && (few_balls || physics.ball_collisions) {
            physics.ball_collisions = !physics.ball_collisions;
        }
    }

    fn roll_bounces(&mut self, dice: &mut Dice, amount: f32) {
        let many_balls = self.ball_count() > 20;
        let rules = &mut self.rules;
        let gain = match dice.int(0, 9) {
            0..=3 => 1.0,
            4 => 1.0 - dice.range(0.005, 0.03),
            _ => 1.0 + dice.range(0.002, 0.04),
        };
        set(&mut rules.speed_gain, gain, amount);
        set(&mut rules.max_speed, dice.log_range(1500.0, 6000.0), amount);
        set(&mut rules.grow, maybe_zero(dice, 0.6, |d| d.range(0.3, if many_balls { 0.8 } else { 4.0 })), amount);
        let biggest = if many_balls { dice.range(8.0, 30.0) } else { dice.range(20.0, 250.0) };
        set(&mut rules.max_radius, biggest, amount);
        let recolor = dice.chance(0.45);
        let mut colors = rules.recolor.clone();
        roll_colors(&mut colors, dice, amount, 2, 6);
        if amount >= 0.999 {
            rules.recolor = if recolor { colors } else { Vec::new() };
        } else if !rules.recolor.is_empty() || (recolor && dice.chance(amount)) {
            rules.recolor = colors;
        }
    }

    fn roll_new_balls(&mut self, dice: &mut Dice, amount: f32) {
        let starting = self.ball_count();
        let rings = matches!(self.container, Container::Rings { .. });
        let rules = &mut self.rules;
        set_whole(&mut rules.spawn_per_hit, if dice.chance(0.65) { 0 } else { dice.int(1, 2) }, amount);
        let per_escape = dice.int(0, 3);
        if rings {
            set_whole(&mut rules.spawn_per_escape, per_escape, amount);
        }
        let most = dice.log_range(50.0, 800.0).round() as u32;
        set_whole(&mut rules.max_balls, most.max(starting), amount);
        set(&mut rules.spawn_spread, dice.range(10.0, 90.0), amount);
    }

    /// Moves and if needed shrinks each group of balls so it starts inside the arena.
    pub fn keep_balls_inside(&mut self) {
        let room = self.room();
        for spawner in &mut self.spawners {
            match spawner {
                Spawner::Grid { center, columns, rows, spacing, radius, .. } => {
                    let span = Vec2::new(columns.saturating_sub(1) as f32, rows.saturating_sub(1) as f32);
                    let half = span * *spacing * 0.5 + *radius;
                    let scale = room.fit_scale(half);
                    if scale < 1.0 {
                        *spacing *= scale;
                        *radius = (*radius * scale).max(1.0);
                    }
                    let half = span * *spacing * 0.5 + *radius;
                    *center = room.clamp(*center, half);
                }
                Spawner::Single { position, radius, .. } => {
                    *radius *= room.fit_scale(Vec2::splat(*radius * 2.0));
                    *position = room.clamp(*position, Vec2::splat(*radius));
                }
                Spawner::Stream { position, radius, jitter, .. } => {
                    *radius *= room.fit_scale(Vec2::splat(*radius * 2.0));
                    let half = Vec2::new(*jitter + *radius, *radius);
                    let scale = room.fit_scale(half);
                    *jitter *= scale;
                    *position = room.clamp(*position, Vec2::new(*jitter + *radius, *radius));
                }
            }
        }
    }

    /// A copy of the scene with `groups` rolled from `seed`. The same scene, groups, seed and
    /// amount always give the same result.
    pub fn variation(&self, groups: &[Group], seed: u64, amount: f32) -> Scene {
        let mut scene = self.clone();
        for &group in groups {
            scene.roll(group, &mut Dice::mixed(seed, group.salt()), amount);
        }
        scene
    }
}

fn set_color(color: &mut Color, rolled: Color, dice: &mut Dice, amount: f32) {
    if amount >= 0.999 {
        *color = rolled;
    } else {
        roll_color(color, dice, amount);
    }
}

/// Four chords that sound good in a loop, in a random key.
fn chords(dice: &mut Dice) -> Vec<ChordStep> {
    // Scale steps of the major key: semitones up from the key note, and whether the chord is minor.
    const STEPS: [(u32, bool); 6] = [(0, false), (2, true), (4, true), (5, false), (7, false), (9, true)];
    const LOOPS: [[usize; 4]; 7] =
        [[0, 4, 5, 3], [5, 3, 0, 4], [0, 5, 3, 4], [0, 3, 5, 4], [3, 0, 4, 5], [0, 2, 3, 4], [5, 4, 3, 4]];
    let key = dice.int(0, 11);
    let pattern = dice.pick(&LOOPS);
    let seconds = dice.pick(&[2.0, 2.5, 3.0, 4.0]);
    let colorful = dice.chance(0.5);
    pattern
        .iter()
        .map(|&step| {
            let (offset, minor) = STEPS[step];
            let root = ROOTS[((key + offset) % 12) as usize];
            let flavour = if colorful && dice.chance(0.4) {
                if minor { dice.pick(&["m7", "m7", "madd9"]) } else { dice.pick(&["maj7", "add9", "sus2", "sus4"]) }
            } else if minor {
                "m"
            } else {
                ""
            };
            ChordStep { chord: format!("{root}{flavour}"), seconds }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Group; 11] = [
        Group::Shape,
        Group::Movement,
        Group::World,
        Group::Bounces,
        Group::NewBalls,
        Group::Background,
        Group::Trails,
        Group::Glow,
        Group::Strings,
        Group::Chords,
        Group::Notes,
    ];

    fn every_group(scene: &Scene) -> Vec<Group> {
        (0..scene.spawners.len()).map(Group::Balls).chain(ALL).collect()
    }

    /// Whether every ball starts inside the arena.
    fn balls_inside(scene: &Scene) -> bool {
        let room = scene.room();
        let mut inside = true;
        for spawner in &scene.spawners {
            let (points, radius): (Vec<Vec2>, f32) = match *spawner {
                Spawner::Grid { center, columns, rows, spacing, radius, .. } => {
                    let size = Vec2::new(columns.saturating_sub(1) as f32, rows.saturating_sub(1) as f32) * spacing;
                    let origin = center - size * 0.5;
                    let corners = [Vec2::ZERO, Vec2::new(size.x, 0.0), Vec2::new(0.0, size.y), size];
                    (corners.iter().map(|c| origin + *c).collect(), radius)
                }
                Spawner::Single { position, radius, .. } => (vec![position], radius),
                Spawner::Stream { position, radius, jitter, .. } => {
                    (vec![position - Vec2::X * jitter, position + Vec2::X * jitter], radius)
                }
            };
            for point in points {
                inside &= match room {
                    Room::Circle { center, radius: r } => (point - center).length() + radius <= r + 0.01,
                    Room::Box { center, half } => {
                        let offset = (point - center).abs() + radius;
                        offset.x <= half.x + 0.01 && offset.y <= half.y + 0.01
                    }
                };
            }
        }
        inside
    }

    fn fits_frame(scene: &Scene) -> bool {
        match scene.container {
            Container::Rect { size, .. } => size.x <= 2.0 * MAX_ARENA_RADIUS + 0.1 && size.y <= 1800.1,
            _ => scene.container.size() <= MAX_ARENA_RADIUS + 0.1,
        }
    }

    #[test]
    fn dice_repeat_and_stay_in_range() {
        let (mut a, mut b) = (Dice::new(42), Dice::new(42));
        for _ in 0..1000 {
            let x = a.range(2.0, 5.0);
            assert_eq!(x, b.range(2.0, 5.0));
            assert!((2.0..5.0).contains(&x));
            let n = a.int(3, 7);
            b.int(3, 7);
            assert!((3..=7).contains(&n));
            let l = a.log_range(10.0, 1000.0);
            b.log_range(10.0, 1000.0);
            assert!((9.999..=1000.01).contains(&l));
        }
        assert_ne!(Dice::new(1).next_u64(), Dice::new(2).next_u64());
    }

    #[test]
    fn variations_repeat_for_the_same_seed() {
        let scene = Scene::default();
        let groups = every_group(&scene);
        assert_eq!(scene.variation(&groups, 7, 1.0), scene.variation(&groups, 7, 1.0));
        assert_ne!(scene.variation(&groups, 7, 1.0), scene.variation(&groups, 8, 1.0));
        assert_eq!(scene.variation(&groups, 7, 0.0).look, scene.look);
    }

    #[test]
    fn groups_roll_independently() {
        // Turning a group off leaves what the others roll unchanged.
        let scene = Scene::default();
        let both = scene.variation(&[Group::Trails, Group::Glow], 3, 1.0);
        let one = scene.variation(&[Group::Glow], 3, 1.0);
        assert_eq!(both.look.ball_glow, one.look.ball_glow);
        assert_eq!(one.look.trail_seconds, scene.look.trail_seconds);
    }

    #[test]
    fn rolls_only_touch_their_group() {
        let scene = Scene::default();
        let rolled = scene.variation(&[Group::Trails], 5, 1.0);
        assert_eq!(Scene { look: scene.look.clone(), ..rolled.clone() }, scene);
        let rolled = scene.variation(&[Group::Chords], 5, 1.0);
        assert_eq!(rolled.sound.low_note, scene.sound.low_note);
        assert_ne!(rolled.sound.progression, scene.sound.progression);
    }

    #[test]
    fn every_preset_stays_sensible_after_many_rolls() {
        for preset in Scene::presets() {
            let groups = every_group(&preset.scene);
            for seed in 0..60 {
                for amount in [0.2, 0.6, 1.0] {
                    let scene = preset.scene.variation(&groups, seed, amount);
                    let name = format!("{} seed {seed} amount {amount}", preset.name);
                    assert!(balls_inside(&scene), "balls outside the arena: {name}");
                    assert!(fits_frame(&scene) || !fits_frame(&preset.scene), "arena too big: {name}");
                    assert!(scene.ball_count() >= 1, "{name}");
                    assert!(scene.sound.low_note <= scene.sound.high_note, "{name}");
                    assert!(scene.rules.max_balls >= 1, "{name}");
                    for step in &scene.sound.progression {
                        assert!(step.seconds > 0.0, "{name}");
                    }
                    let values = [
                        scene.look.trail_seconds,
                        scene.look.trail_width,
                        scene.look.ball_glow,
                        scene.rules.speed_gain,
                        scene.rules.max_speed,
                        scene.physics.gravity.y,
                        scene.physics.restitution,
                    ];
                    assert!(values.iter().all(|v| v.is_finite() && *v >= 0.0), "{name}: {values:?}");
                    // Round-trips through a scene file.
                    assert_eq!(Scene::from_ron(&scene.to_ron().unwrap()).unwrap().spawners.len(), scene.spawners.len());
                }
            }
        }
    }

    #[test]
    fn rolled_chords_are_real_chords() {
        let mut dice = Dice::new(9);
        for _ in 0..200 {
            for step in chords(&mut dice) {
                let (root, quality) = step.chord.split_at(if step.chord[1..].starts_with(['#', 'b']) { 2 } else { 1 });
                assert!(ROOTS.contains(&root), "{}", step.chord);
                assert!(["", "m", "m7", "madd9", "maj7", "add9", "sus2", "sus4"].contains(&quality), "{}", step.chord);
            }
        }
    }

    #[test]
    fn big_blocks_shrink_to_fit_small_arenas() {
        let container = Container::Circle { center: Vec2::ZERO, radius: 100.0, thickness: 4.0, color: Color::WHITE };
        let mut scene = Scene { container, ..Scene::default() };
        scene.keep_balls_inside();
        assert!(balls_inside(&scene));
        let Spawner::Grid { spacing, .. } = scene.spawners[0] else { panic!() };
        assert!(spacing < 19.0);
    }
}
