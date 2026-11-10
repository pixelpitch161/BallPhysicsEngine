//! How one particle moves from the moment it was let off to the moment drawn, worked out afresh
//! every frame from its emission and its number: the same wherever the frame came from.

use bpe_runtime::{Wall, keep_clear, reflect};
use glam::Vec2;

use super::emit::{Emission, hash};
use super::{Direction, MadeOf, ParticlesLayer};

/// The step particles move by, in seconds, while they are young.
pub const STEP: f32 = 1.0 / 120.0;
/// The most steps one particle is moved in, so a long life costs no more than a second's worth:
/// older particles move in longer steps.
pub const MOST_STEPS: u32 = 120;

/// How many steps a particle `age` seconds old moves in, and how long each is.
pub fn steps_for(age: f32) -> (u32, f32) {
    let age = age.max(0.0);
    let step = STEP.max(age / MOST_STEPS as f32);
    let n = ((age / step).ceil() as u32).min(MOST_STEPS);
    (n, step)
}

/// The swirl's push at `at` (in its own size units): along the noise's curl, so it turns
/// particles round in eddies whether they are moving or not. About -1 to 1 a unit of Swirl.
pub fn curl(at: Vec2) -> Vec2 {
    const H: f32 = 0.25;
    let dx = (noise(at + Vec2::new(H, 0.0)) - noise(at - Vec2::new(H, 0.0))) / (2.0 * H);
    let dy = (noise(at + Vec2::new(0.0, H)) - noise(at - Vec2::new(0.0, H))) / (2.0 * H);
    Vec2::new(dy, -dx) * 0.5
}

/// One particle at the moment drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Live {
    pub place: Vec2,
    pub velocity: Vec2,
    /// Its turn, radians.
    pub angle: f32,
    /// Its half size, in pixels.
    pub size: f32,
    /// How much of it shows, 0 to 1: its fade and twinkle.
    pub alpha: f32,
    /// How far through its life, 0 to 1.
    pub life: f32,
}

/// What a particle moves through.
pub struct Motion<'a> {
    /// The shapes it bounces off, when it does.
    pub walls: &'a [&'a Wall],
    /// Where the ball that let it off was at a moment (seconds into the clip), for Pull back.
    pub pull_to: Option<&'a dyn Fn(f64) -> Option<Vec2>>,
}

use bpe_core::dice::Stream;

/// Random numbers from one seed.
fn stream(seed: u32) -> Stream {
    Stream::new(seed, 0x5bd1_e995)
}

/// Smooth value noise, -1 to 1, varying over one unit.
pub fn noise(p: Vec2) -> f32 {
    let (ix, iy) = (p.x.floor(), p.y.floor());
    let (fx, fy) = (p.x - ix, p.y - iy);
    let corner = |dx: f32, dy: f32| {
        let h = hash(&[(ix + dx) as i32 as u32, (iy + dy) as i32 as u32, 0x51]);
        (h >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    };
    let ease = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v) = (ease(fx), ease(fy));
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * u;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * u;
    top + (bottom - top) * v
}

/// The way particles of `layer` from `e` fly, before their spread.
fn heading(layer: &ParticlesLayer, e: &Emission, rng: &mut Stream) -> Vec2 {
    let moving = e.velocity.try_normalize();
    match layer.direction {
        Direction::OffTheBounce => e.normal.try_normalize().unwrap_or(Vec2::NEG_Y),
        Direction::AllRound => Vec2::from_angle(rng.unit() * std::f32::consts::TAU),
        Direction::AlongThePath => moving.unwrap_or(Vec2::NEG_Y),
        Direction::BehindTheBall => -moving.unwrap_or(Vec2::Y),
        Direction::Up => Vec2::NEG_Y,
    }
}

/// Particle `k` of emission `e` of `layer` at `now` (seconds into the clip), or `None` before
/// it was let off or once it is gone.
pub fn simulate(layer: &ParticlesLayer, e: &Emission, k: u32, now: f64, motion: &Motion) -> Option<Live> {
    let age = (now - e.time) as f32;
    let life = layer.life.max(1e-3);
    if !(0.0..life).contains(&age) {
        return None;
    }
    let t = age / life;
    let grow = layer.size_curve.at(t)?;
    let mut rng = stream(hash(&[e.seed, k, 0xa11ce]));
    let mut way = heading(layer, e, &mut rng);
    if layer.direction != Direction::AllRound {
        let turn = rng.signed() * 0.5 * layer.spread.clamp(0.0, 360.0).to_radians();
        way = Vec2::from_angle(turn).rotate(way);
    }
    let speed = layer.speed.max(0.0) * e.impact.max(0.0) * (0.6 + 0.8 * rng.unit());
    let v0 = way * speed + e.velocity * layer.inherit.clamp(0.0, 1.0);
    let size = layer.size.max(0.0) * (0.7 + 0.6 * rng.unit());
    let spin = layer.spin * (1.0 + layer.spin_spread.clamp(0.0, 1.0) * rng.signed());
    let spin = if rng.unit() < 0.5 { -spin } else { spin };
    let start_angle = rng.unit() * std::f32::consts::TAU;
    let twinkle_phase = rng.unit() * std::f32::consts::TAU;
    let gravity = Vec2::new(0.0, layer.gravity);
    let drag = layer.drag.max(0.0);
    let stepped = layer.swirl != 0.0 || (layer.pull > 0.0 && motion.pull_to.is_some()) || layer.bounce;
    let (place, velocity) = if !stepped {
        // Gravity and drag alone have an exact answer.
        if drag > 1e-6 {
            let fall = gravity / drag;
            let k = (-drag * age).exp();
            (e.place + fall * age + (v0 - fall) * (1.0 - k) / drag, fall + (v0 - fall) * k)
        } else {
            (e.place + v0 * age + gravity * (0.5 * age * age), v0 + gravity * age)
        }
    } else {
        let (mut p, mut v) = (e.place, v0);
        let (steps, step) = steps_for(age);
        let radius = size * grow * 0.5;
        if layer.bounce {
            p = on_its_side(p, e.from, radius, motion.walls);
        }
        for n in 0..steps {
            let dt = if n + 1 == steps { age - n as f32 * step } else { step };
            let mut a = gravity;
            if layer.swirl != 0.0 {
                a += curl(p / layer.swirl_size.max(1.0)) * layer.swirl;
            }
            if layer.pull > 0.0
                && let Some(to) = motion.pull_to
                && let Some(ball) = to(e.time + (n as f32 * step + dt) as f64)
            {
                a += (ball - p) * layer.pull * layer.pull;
            }
            v += a * dt;
            v *= (1.0 - drag * dt).max(0.0);
            let before = p;
            p += v * dt;
            if layer.bounce {
                for wall in motion.walls {
                    if let Some(c) = keep_clear(&mut p, before, v, radius, wall) {
                        v = reflect(v, &c, layer.bounciness.clamp(0.0, 1.0));
                    }
                }
            }
        }
        (p, v)
    };
    let angle = match layer.made_of {
        MadeOf::Sparks | MadeOf::Streaks => velocity.y.atan2(velocity.x),
        _ => start_angle + spin.to_radians() * age,
    };
    let twinkle = layer.twinkle.clamp(0.0, 1.0);
    let flicker =
        1.0 - twinkle * (0.5 + 0.5 * (layer.twinkle_speed * std::f32::consts::TAU * age + twinkle_phase).sin());
    Some(Live { place, velocity, angle, size: size * grow, alpha: layer.fade.at(t) * flicker, life: t })
}

/// `p` moved, where needed, to the side of each hollow wall that `from` is on: a shape that
/// has shrunk or moved since the particle was let off doesn't shut it out.
fn on_its_side(mut p: Vec2, from: Vec2, radius: f32, walls: &[&Wall]) -> Vec2 {
    for wall in walls {
        if wall.solid || wall.outline.inside(p) == wall.outline.inside(from) {
            continue;
        }
        let (q, n) = wall.outline.nearest(p);
        p = q - n * (radius + wall.outline.thickness() * 0.5 + 0.5);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_runtime::Outline;

    fn emission() -> Emission {
        Emission {
            time: 1.0,
            row: Some(0),
            by: None,
            place: Vec2::ZERO,
            normal: Vec2::NEG_Y,
            velocity: Vec2::new(100.0, 0.0),
            count: 20,
            seed: 42,
            impact: 1.0,
            from: Vec2::ZERO,
        }
    }

    const NONE: Motion = Motion { walls: &[], pull_to: None };

    fn layer() -> ParticlesLayer {
        ParticlesLayer { life: 2.0, gravity: 0.0, drag: 0.0, spread: 0.0, ..ParticlesLayer::default() }
    }

    #[test]
    fn a_particle_is_the_same_whenever_it_is_worked_out() {
        let l = layer();
        let a = simulate(&l, &emission(), 3, 1.5, &NONE);
        assert!(a.is_some());
        assert_eq!(a, simulate(&l, &emission(), 3, 1.5, &NONE));
        assert_ne!(a, simulate(&l, &emission(), 4, 1.5, &NONE), "each has its own");
        assert!(simulate(&l, &emission(), 3, 0.9, &NONE).is_none(), "not yet let off");
        assert!(simulate(&l, &emission(), 3, 3.1, &NONE).is_none(), "gone");
        // Stepped the same way however it was reached.
        let swirl = ParticlesLayer { swirl: 800.0, ..l };
        assert_eq!(simulate(&swirl, &emission(), 3, 1.7, &NONE), simulate(&swirl, &emission(), 3, 1.7, &NONE));
    }

    #[test]
    fn gravity_pulls_down_and_drag_slows() {
        let still = layer();
        let falls = ParticlesLayer { gravity: 500.0, ..still.clone() };
        let a = simulate(&still, &emission(), 0, 1.5, &NONE).unwrap();
        let b = simulate(&falls, &emission(), 0, 1.5, &NONE).unwrap();
        assert!(b.place.y > a.place.y + 50.0, "{} {}", a.place, b.place);
        let draggy = ParticlesLayer { drag: 3.0, ..still.clone() };
        let c = simulate(&draggy, &emission(), 0, 1.5, &NONE).unwrap();
        assert!(c.velocity.length() < a.velocity.length() * 0.5);
        // Off the bounce, straight: up.
        assert!(a.velocity.y < 0.0 && a.velocity.x.abs() < 1e-3);
        // Taking the ball's speed.
        let carried = ParticlesLayer { inherit: 1.0, ..still };
        let d = simulate(&carried, &emission(), 0, 1.5, &NONE).unwrap();
        assert!((d.velocity.x - 100.0).abs() < 1e-3);
    }

    #[test]
    fn swirl_turns_them_but_keeps_their_speed() {
        let l = ParticlesLayer { swirl: 1500.0, swirl_size: 60.0, ..layer() };
        let mut turned = 0;
        for k in 0..20 {
            let plain = simulate(&layer(), &emission(), k, 1.2, &NONE).unwrap();
            let s = simulate(&l, &emission(), k, 1.2, &NONE).unwrap();
            let ratio = s.velocity.length() / plain.velocity.length();
            assert!((0.7..1.3).contains(&ratio), "{ratio}");
            if s.velocity.normalize().dot(plain.velocity.normalize()) < 0.99 {
                turned += 1;
            }
        }
        assert!(turned > 10, "{turned}");
    }

    #[test]
    fn pull_back_brings_them_to_the_ball() {
        let ball = |_: f64| Some(Vec2::new(0.0, 0.0));
        let pulled = Motion { walls: &[], pull_to: Some(&ball) };
        let l = ParticlesLayer { pull: 6.0, drag: 2.0, ..layer() };
        let free = simulate(&layer(), &emission(), 0, 2.5, &NONE).unwrap();
        let p = simulate(&l, &emission(), 0, 2.5, &pulled).unwrap();
        assert!(p.place.length() < free.place.length() * 0.3, "{} {}", p.place, free.place);
    }

    #[test]
    fn bouncing_keeps_them_inside_a_box() {
        let wall = Wall {
            outline: Outline::Box { center: Vec2::ZERO, half: Vec2::splat(100.0), angle: 0.0, thickness: 2.0 },
            solid: false,
            spin: 0.0,
            growth: 0.0,
            velocity: Vec2::ZERO,
            passes: false,
            gaps: None,
        };
        let walls = [&wall];
        let m = Motion { walls: &walls, pull_to: None };
        let l = ParticlesLayer { bounce: true, bounciness: 0.8, direction: Direction::AllRound, ..layer() };
        for k in 0..30 {
            let p = simulate(&l, &emission(), k, 2.9, &m).unwrap();
            assert!(p.place.x.abs() < 101.0 && p.place.y.abs() < 101.0, "{k}: {}", p.place);
            let free = simulate(&ParticlesLayer { bounce: false, ..l.clone() }, &emission(), k, 2.9, &m).unwrap();
            assert!(free.place.length() > 150.0, "it would have flown out");
        }
    }

    #[test]
    fn size_and_fade_follow_their_curves() {
        let l = layer();
        let young = simulate(&l, &emission(), 0, 1.1, &NONE).unwrap();
        let old = simulate(&l, &emission(), 0, 2.8, &NONE).unwrap();
        assert!(old.size < young.size, "shrinking");
        assert!(old.alpha < young.alpha, "fading out");
        let pop = ParticlesLayer { size_curve: super::super::SizeOverLife::Pop, ..l };
        assert!(simulate(&pop, &emission(), 0, 2.9, &NONE).is_none(), "popped in its last tenth");
        let twinkly = ParticlesLayer { twinkle: 1.0, fade: super::super::ParticleFade::None, ..layer() };
        let alphas: Vec<f32> =
            (0..20).map(|i| simulate(&twinkly, &emission(), 0, 1.0 + i as f64 * 0.05, &NONE).unwrap().alpha).collect();
        assert!(alphas.iter().any(|a| *a < 0.3) && alphas.iter().any(|a| *a > 0.7), "{alphas:?}");
    }

    fn ring(radius: f32) -> Wall {
        let outline = Outline::Ring { center: Vec2::ZERO, radius, sides: 0, angle: 0.0, thickness: 2.0 };
        Wall { outline, solid: false, spin: 0.0, growth: 0.0, velocity: Vec2::ZERO, passes: false, gaps: None }
    }

    #[test]
    fn a_harder_hit_sends_them_faster() {
        let l = layer();
        let soft = Emission { impact: 0.6, velocity: Vec2::ZERO, ..emission() };
        let hard = Emission { impact: 1.2, ..soft };
        let a = simulate(&l, &soft, 0, 1.5, &NONE).unwrap();
        let b = simulate(&l, &hard, 0, 1.5, &NONE).unwrap();
        assert!((b.place.length() / a.place.length() - 2.0).abs() < 1e-3, "{} {}", a.place, b.place);
    }

    #[test]
    fn a_long_life_moves_in_at_most_its_steps() {
        assert_eq!(steps_for(0.5), (60, STEP));
        let (n, step) = steps_for(3.0);
        assert_eq!(n, MOST_STEPS);
        assert!((step * n as f32 - 3.0).abs() < 1e-4);
        assert_eq!(steps_for(0.0).0, 0);
    }

    #[test]
    fn swirl_turns_still_particles_too() {
        let l = ParticlesLayer { speed: 0.0, inherit: 0.0, swirl: 800.0, swirl_size: 60.0, ..layer() };
        let e = Emission { place: Vec2::new(37.0, 11.0), ..emission() };
        let p = simulate(&l, &e, 0, 1.6, &NONE).unwrap();
        assert!(p.place.distance(e.place) > 5.0, "{}", p.place);
    }

    #[test]
    fn a_particle_let_off_outside_a_shrunk_ring_bounces_inside_it() {
        // The ball hit the ring when it was 110 round; it is 100 now.
        let wall = ring(100.0);
        let walls = [&wall];
        let m = Motion { walls: &walls, pull_to: None };
        let l = ParticlesLayer { bounce: true, bounciness: 0.8, direction: Direction::AllRound, ..layer() };
        let e = Emission { place: Vec2::new(110.0, 0.0), from: Vec2::new(90.0, 0.0), ..emission() };
        for k in 0..20 {
            let p = simulate(&l, &e, k, 2.5, &m).unwrap();
            assert!(p.place.length() < 100.0, "{k}: {}", p.place);
        }
    }
}
