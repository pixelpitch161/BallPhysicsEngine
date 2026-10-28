//! Balls bumping into each other: a sequential-impulse contact solver with restitution,
//! friction, optional spin, warm starting (so stacks hold still) and sleeping for balls that
//! have come to rest.
//!
//! The classic mode (`BallContact::quality` 0) is the original one-pass push-apart in the
//! simulation itself, kept so older scenes play exactly as before.

use crate::Ball;
use crate::grid::BallGrid;
use bpe_scene::{BallContact, Vec2};

/// Seconds a group of touching balls must stay slow before it goes to sleep.
const SLEEP_SECONDS: f32 = 0.5;
/// Overlap allowed before positions are pushed apart, in pixels. A little overlap keeps resting
/// contacts touching from one step to the next, which is what stops stacks jittering.
const SLOP: f32 = 0.05;
/// Share of the remaining overlap fixed per position pass.
const PUSH: f32 = 0.6;

/// One pair of touching balls, `a < b`, with `normal` pointing from a to b.
#[derive(Clone, Copy, Debug)]
struct Contact {
    a: u32,
    b: u32,
    normal: Vec2,
    /// Inverse of the effective mass along the normal and along the surface.
    mass_n: f32,
    mass_t: f32,
    /// Speed apart the bounce aims for, along the normal.
    target: f32,
    /// Impulse so far along the normal and along the surface.
    jn: f32,
    jt: f32,
}

/// A ball pressed against a wall, peg or shape, which the solver treats as a ball that can't
/// move. Without these, piles leaning on a wall would be pushed into it by the solver and out
/// again by the wall every substep, and never settle.
#[derive(Clone, Copy, Debug)]
struct WallContact {
    ball: u32,
    /// Pointing away from the wall, into the ball.
    normal: Vec2,
    jn: f32,
    jt: f32,
}

/// How close to a wall a ball counts as touching it, in pixels.
pub const WALL_MARGIN: f32 = 0.5;

/// Contact state that lives from one substep to the next.
#[derive(Clone, Default)]
pub struct Contacts {
    grid: BallGrid,
    contacts: Vec<Contact>,
    /// Last substep's contacts as (a, b, jn, jt), sorted, for warm starting and for telling new
    /// touches (which make a sound) from ongoing ones.
    previous: Vec<(u32, u32, f32, f32)>,
    walls: Vec<WallContact>,
    /// Last substep's wall contacts as (ball, jn, jt), sorted, for warm starting.
    previous_walls: Vec<(u32, f32, f32)>,
    /// Per ball: angular velocity (radians per second, clockwise) and the angle it has turned.
    pub spin: Vec<f32>,
    pub angle: Vec<f32>,
    /// Per ball: seconds it has been slow, and whether it is asleep.
    slow_for: Vec<f32>,
    pub asleep: Vec<bool>,
    /// Union-find parents for grouping touching balls.
    group: Vec<u32>,
    /// Inverse masses this substep (0 for sleeping balls, which act like fixed pegs).
    inverse: Vec<f32>,
    /// Scratch for sleeping: per group, the shortest time any of its balls has been slow, and
    /// per ball, whether it touches another ball.
    least: Vec<f32>,
    touched: Vec<bool>,
}

/// A ball-to-ball touch worth a sound: the first moment two balls meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touch {
    pub a: u32,
    pub b: u32,
    pub position: Vec2,
    pub speed: f32,
}

impl Contacts {
    /// Makes room for every ball, new ones starting awake and still.
    pub fn fit(&mut self, count: usize) {
        self.spin.resize(count, 0.0);
        self.angle.resize(count, 0.0);
        self.slow_for.resize(count, 0.0);
        self.asleep.resize(count, false);
    }

    pub fn is_asleep(&self, ball: usize) -> bool {
        self.asleep.get(ball).copied().unwrap_or(false)
    }

    /// Wakes one ball, for when something outside the solver pushes it.
    pub fn wake(&mut self, ball: usize) {
        if let Some(asleep) = self.asleep.get_mut(ball) {
            *asleep = false;
            self.slow_for[ball] = 0.0;
        }
    }

    /// Wakes every ball, for when the world around them changes.
    pub fn wake_all(&mut self) {
        self.asleep.iter_mut().for_each(|a| *a = false);
        self.slow_for.iter_mut().for_each(|t| *t = 0.0);
    }

    /// Solves one substep of contacts between `balls`, after they have moved. `resting` is the
    /// closing speed below which a touch is a ball resting on another, not a bounce. New touches
    /// at or above it are appended to `touches`. `walls` lists balls touching a wall as (ball,
    /// direction away from the wall), sorted by ball; the walls themselves still bounce balls
    /// afterwards, this only keeps the solver from pushing balls into them.
    #[allow(clippy::too_many_arguments)]
    pub fn solve(
        &mut self,
        balls: &mut [Ball],
        settings: &BallContact,
        restitution: f32,
        dt: f32,
        resting: f32,
        allow_sleep: bool,
        walls: &[(u32, Vec2)],
        touches: &mut Vec<Touch>,
    ) {
        self.fit(balls.len());
        if !allow_sleep {
            self.wake_all();
        }
        let max_radius = balls.iter().map(|b| b.radius).fold(0.0f32, f32::max);
        if balls.is_empty() || max_radius <= 0.0 || !max_radius.is_finite() {
            self.previous.clear();
            self.previous_walls.clear();
            self.integrate_spin(dt, settings);
            return;
        }
        self.grid.build(balls.iter().map(|b| b.position), max_radius * 2.0);

        // Find touching pairs.
        self.contacts.clear();
        // Sleeping pairs are kept too: they cost nothing to solve (both act fixed) and they
        // keep their group together for sleeping.
        let contacts = &mut self.contacts;
        self.grid.for_each_pair(|i, j| {
            let (a, b) = (&balls[i], &balls[j]);
            let reach = a.radius + b.radius;
            if a.alive && b.alive && a.position.distance_squared(b.position) < reach * reach {
                contacts.push(Contact {
                    a: i as u32,
                    b: j as u32,
                    normal: Vec2::ZERO,
                    mass_n: 0.0,
                    mass_t: 0.0,
                    target: 0.0,
                    jn: 0.0,
                    jt: 0.0,
                });
            }
        });
        self.contacts.sort_unstable_by_key(|c| (c.a, c.b));

        // A sleeping ball hit hard enough wakes up. The rest act like fixed pegs this substep.
        let wake_speed = resting.max(1.0) * 2.0;
        for c in &self.contacts {
            let (a, b) = (c.a as usize, c.b as usize);
            if self.asleep[a] != self.asleep[b] {
                let closing = (balls[a].velocity - balls[b].velocity).length();
                if closing > wake_speed {
                    self.asleep[a] = false;
                    self.asleep[b] = false;
                    self.slow_for[a] = 0.0;
                    self.slow_for[b] = 0.0;
                }
            }
        }
        self.inverse.clear();
        self.inverse.extend(
            balls.iter().zip(&self.asleep).map(|(b, &s)| if s { 0.0 } else { 1.0 / (b.radius * b.radius).max(1e-6) }),
        );

        let spin_factor = if settings.spin { 3.0 } else { 1.0 };
        let mut previous = self.previous.iter().peekable();
        for c in &mut self.contacts {
            let (a, b) = (&balls[c.a as usize], &balls[c.b as usize]);
            let offset = b.position - a.position;
            c.normal = offset.try_normalize().unwrap_or(Vec2::NEG_Y);
            let (ia, ib) = (self.inverse[c.a as usize], self.inverse[c.b as usize]);
            let k = ia + ib;
            c.mass_n = if k > 0.0 { 1.0 / k } else { 0.0 };
            c.mass_t = if k > 0.0 { 1.0 / (k * spin_factor) } else { 0.0 };
            let closing = (b.velocity - a.velocity).dot(c.normal);
            while previous.peek().is_some_and(|p| (p.0, p.1) < (c.a, c.b)) {
                previous.next();
            }
            let known = previous.peek().filter(|p| (p.0, p.1) == (c.a, c.b)).copied();
            // Only real bounces spring back; gentle touches just stop, so stacks settle. Never
            // more than a perfect bounce: in a jammed pile, a springier one would gain speed
            // every substep until it blew apart.
            c.target = if -closing > resting { -closing * restitution.min(1.0) } else { 0.0 };
            match known {
                Some(&(_, _, jn, jt)) => {
                    // Warm start from last substep's impulses, so resting stacks don't sag.
                    c.jn = jn;
                    c.jt = jt;
                }
                None if -closing >= resting => {
                    touches.push(Touch { a: c.a, b: c.b, position: a.position + c.normal * a.radius, speed: -closing })
                }
                None => {}
            }
        }
        self.walls.clear();
        let previous_walls = std::mem::take(&mut self.previous_walls);
        let mut known = previous_walls.iter().peekable();
        for &(ball, normal) in walls {
            // A ball coming in fast bounces off the wall itself, with a sound.
            if self.inverse[ball as usize] <= 0.0 || -balls[ball as usize].velocity.dot(normal) > resting {
                continue;
            }
            while known.peek().is_some_and(|p| p.0 < ball) {
                known.next();
            }
            let (jn, jt) = known.peek().filter(|p| p.0 == ball).map_or((0.0, 0.0), |p| (p.1, p.2));
            let w = WallContact { ball, normal, jn, jt };
            self.push_wall(balls, &w, jn, jt, settings.spin);
            self.walls.push(w);
        }
        self.previous_walls = previous_walls;
        // Pairs' warm start goes on after the walls are chosen, which look at how fast each ball
        // arrives on its own: a resting stack's weight mustn't make its bottom ball look as if
        // it were crashing into the floor.
        for i in 0..self.contacts.len() {
            let c = self.contacts[i];
            self.apply(balls, &c, c.jn, c.jt, settings.spin);
        }

        let iterations = settings.quality.clamp(1, 32);
        let friction = settings.friction.max(0.0);
        for _ in 0..iterations {
            for i in 0..self.contacts.len() {
                let mut c = self.contacts[i];
                let (a, b) = (c.a as usize, c.b as usize);
                let tangent = c.normal.perp();
                let relative = balls[b].velocity - balls[a].velocity;
                // Along the normal: push apart until they separate at the target speed.
                let vn = relative.dot(c.normal);
                let jn = (c.jn + c.mass_n * (c.target - vn)).max(0.0);
                let dn = jn - c.jn;
                c.jn = jn;
                // Along the surface: friction, never more than `friction` times the push.
                let mut vt = relative.dot(tangent);
                if settings.spin {
                    vt -= self.spin[a] * balls[a].radius + self.spin[b] * balls[b].radius;
                }
                let limit = friction * c.jn;
                let jt = (c.jt - c.mass_t * vt).clamp(-limit, limit);
                let dt_ = jt - c.jt;
                c.jt = jt;
                self.apply(balls, &c, dn, dt_, settings.spin);
                self.contacts[i] = c;
            }
            for i in 0..self.walls.len() {
                let mut w = self.walls[i];
                let ball = w.ball as usize;
                let inverse = self.inverse[ball];
                // Only stop the ball going into the wall: bounces are the wall's own job.
                let vn = balls[ball].velocity.dot(w.normal);
                let jn = (w.jn - vn / inverse).max(0.0);
                let dn = jn - w.jn;
                w.jn = jn;
                let tangent = w.normal.perp();
                let mut vt = balls[ball].velocity.dot(tangent);
                if settings.spin {
                    vt -= self.spin[ball] * balls[ball].radius;
                }
                let limit = friction * w.jn;
                let jt = (w.jt - vt / (inverse * spin_factor)).clamp(-limit, limit);
                let dt_ = jt - w.jt;
                w.jt = jt;
                self.push_wall(balls, &w, dn, dt_, settings.spin);
                self.walls[i] = w;
            }
        }

        // Push overlapping balls apart, which doesn't change their speed.
        let passes = (iterations / 2).clamp(1, 8);
        for _ in 0..passes {
            for c in &self.contacts {
                let (a, b) = (c.a as usize, c.b as usize);
                let (ia, ib) = (self.inverse[a], self.inverse[b]);
                if ia + ib <= 0.0 {
                    continue;
                }
                let offset = balls[b].position - balls[a].position;
                let distance = offset.length();
                let overlap = balls[a].radius + balls[b].radius - distance - SLOP;
                if overlap <= 0.0 {
                    continue;
                }
                let normal = if distance > 1e-6 { offset / distance } else { c.normal };
                let push = normal * (overlap * PUSH / (ia + ib));
                balls[a].position -= push * ia;
                balls[b].position += push * ib;
            }
        }

        self.previous.clear();
        self.previous.extend(self.contacts.iter().map(|c| (c.a, c.b, c.jn, c.jt)));
        self.previous_walls.clear();
        self.previous_walls.extend(self.walls.iter().map(|w| (w.ball, w.jn, w.jt)));
        self.integrate_spin(dt, settings);
        if allow_sleep && settings.sleep {
            self.update_sleep(balls, dt, resting, walls);
        }
    }

    /// Applies impulse `jn` along the contact normal and `jt` along its surface, from a onto b.
    fn apply(&mut self, balls: &mut [Ball], c: &Contact, jn: f32, jt: f32, spin: bool) {
        let (a, b) = (c.a as usize, c.b as usize);
        let impulse = c.normal * jn + c.normal.perp() * jt;
        let (ia, ib) = (self.inverse[a], self.inverse[b]);
        balls[a].velocity -= impulse * ia;
        balls[b].velocity += impulse * ib;
        if spin {
            // A solid disc's moment of inertia is half its mass times radius squared.
            let (ra, rb) = (balls[a].radius, balls[b].radius);
            self.spin[a] -= jt * 2.0 * ia / ra.max(1e-3);
            self.spin[b] -= jt * 2.0 * ib / rb.max(1e-3);
        }
    }

    /// Applies impulse `jn` away from a wall and `jt` along it to the ball touching it.
    fn push_wall(&mut self, balls: &mut [Ball], w: &WallContact, jn: f32, jt: f32, spin: bool) {
        let ball = w.ball as usize;
        let inverse = self.inverse[ball];
        balls[ball].velocity += (w.normal * jn + w.normal.perp() * jt) * inverse;
        if spin {
            self.spin[ball] -= jt * 2.0 * inverse / balls[ball].radius.max(1e-3);
        }
    }

    fn integrate_spin(&mut self, dt: f32, settings: &BallContact) {
        if !settings.spin {
            return;
        }
        let keep = (1.0 - settings.spin_drag.max(0.0) * dt).max(0.0);
        for (w, angle) in self.spin.iter_mut().zip(&mut self.angle) {
            *w *= keep;
            *angle = (*angle + *w * dt).rem_euclid(std::f32::consts::TAU);
        }
    }

    /// Groups touching balls; a group whose balls have all been slow for a while goes to
    /// sleep, and a sleeping group that something moving touches wakes up.
    fn update_sleep(&mut self, balls: &mut [Ball], dt: f32, resting: f32, walls: &[(u32, Vec2)]) {
        let n = balls.len();
        let slow_speed = resting.max(1.0);
        self.group.clear();
        self.group.extend(0..n as u32);
        fn root(group: &mut [u32], mut i: u32) -> u32 {
            while group[i as usize] != i {
                group[i as usize] = group[group[i as usize] as usize];
                i = group[i as usize];
            }
            i
        }
        for c in &self.contacts {
            let (ra, rb) = (root(&mut self.group, c.a), root(&mut self.group, c.b));
            if ra != rb {
                // The smaller index leads, so grouping never depends on visiting order.
                let (lo, hi) = (ra.min(rb), ra.max(rb));
                self.group[hi as usize] = lo;
            }
        }
        for (i, ball) in balls.iter().enumerate() {
            let slow = ball.velocity.length() < slow_speed && self.spin[i].abs() * ball.radius < slow_speed;
            self.slow_for[i] = if slow { self.slow_for[i] + dt } else { 0.0 };
        }
        // Each group sleeps only as long as its least-rested ball allows.
        self.least.clear();
        self.least.resize(n, f32::MAX);
        self.touched.clear();
        self.touched.resize(n, false);
        for i in 0..n {
            let r = root(&mut self.group, i as u32) as usize;
            let rest = if self.asleep[i] { f32::MAX } else { self.slow_for[i] };
            self.least[r] = self.least[r].min(rest);
        }
        for c in &self.contacts {
            self.touched[c.a as usize] = true;
            self.touched[c.b as usize] = true;
        }
        // Every ball against a wall, asleep ones included (the solver leaves those out).
        for &(ball, _) in walls {
            self.touched[ball as usize] = true;
        }
        for (i, ball) in balls.iter_mut().enumerate() {
            let r = root(&mut self.group, i as u32) as usize;
            if self.least[r] >= SLEEP_SECONDS && self.touched[i] {
                if !self.asleep[i] {
                    self.asleep[i] = true;
                    ball.velocity = Vec2::ZERO;
                    self.spin[i] = 0.0;
                }
            } else if self.asleep[i] {
                // Woken by a moving neighbour, or left with nothing to rest on.
                self.asleep[i] = false;
                self.slow_for[i] = 0.0;
            }
        }
    }
}
