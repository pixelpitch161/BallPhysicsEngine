//! The firing maths, apart from the world: which way the k-th ball of a round of n goes, where
//! it starts, the order a round goes in, how big each round is and where one round lays out.

use std::f32::consts::TAU;

use glam::Vec2;

use super::{Directions, EmitterSettings, FireOrder, FireWhen, StartFrom};
use crate::rng::Rng;

/// What a random roll is for, so each gets its own stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Roll {
    Direction = 1,
    Speed = 2,
    Order = 3,
}

/// The emitter's seed with the scene's and its thing's, for a round's rolls.
fn stream(s: &EmitterSettings, scene_seed: u64, thing: u32, round: u32, roll: Roll) -> Rng {
    let seed = (scene_seed as u32) ^ (scene_seed >> 32) as u32 ^ thing.wrapping_mul(0x9e37_79b9) ^ s.seed as u32;
    Rng::new(seed, round.wrapping_mul(0x85eb_ca6b) ^ ((roll as u32) << 28))
}

/// Ball `k`'s own random number in 0 to 1 for `roll` in `round`.
fn unit(s: &EmitterSettings, scene_seed: u64, thing: u32, round: u32, k: u32, roll: Roll) -> f32 {
    let mut rng = stream(s, scene_seed, thing, round, roll);
    // Skip to the k-th of the stream cheaply: salt by k instead of stepping k times.
    rng = Rng::new(rng.next_u32(), k);
    rng.unit()
}

/// Which way (radians, clockwise from +x, before the emitter's own turn) ball `k` of a round of
/// `n` goes: All round `aim + 360°·k/n`, a Fan evenly across `aim ± fan/2` (one ball: the aim), One
/// way the aim, Random a way picked for (scene seed, thing, round, k).
pub fn direction(s: &EmitterSettings, k: u32, n: u32, round: u32, scene_seed: u64, thing: u32) -> f32 {
    let aim = s.aim_angle.to_radians();
    match s.directions {
        Directions::AllRound => aim + TAU * k as f32 / n.max(1) as f32,
        Directions::Fan => {
            if n <= 1 {
                aim
            } else {
                aim + s.fan_angle.to_radians() * (k as f32 / (n - 1) as f32 - 0.5)
            }
        }
        Directions::OneWay => aim,
        Directions::Random => TAU * unit(s, scene_seed, thing, round, k, Roll::Direction),
    }
}

/// Where ball `k` of a round of `n` starts from the emitter's middle, going the way `dir`
/// (radians): the middle; on the ring at its own direction; or evenly along a line across the aim
/// (one ball: the middle).
pub fn start_offset(s: &EmitterSettings, k: u32, n: u32, dir: f32) -> Vec2 {
    match s.start_from {
        StartFrom::Point => Vec2::ZERO,
        StartFrom::Ring => Vec2::from_angle(dir) * s.start_radius.max(0.0),
        StartFrom::Line => {
            let t = if n <= 1 { 0.0 } else { k as f32 / (n - 1) as f32 - 0.5 };
            Vec2::from_angle(s.aim_angle.to_radians()).perp() * t * s.start_length.max(0.0)
        }
    }
}

/// Ball `number`'s speed as a share of Speed: 1 without a spread, 0.5 to 1.5 at full spread.
pub fn speed_factor(s: &EmitterSettings, round: u32, k: u32, scene_seed: u64, thing: u32) -> f32 {
    if s.vary_speed == 0.0 {
        return 1.0;
    }
    1.0 + s.vary_speed.clamp(0.0, 1.0) * (unit(s, scene_seed, thing, round, k, Roll::Speed) - 0.5)
}

/// The order a round of `n` goes in, by k. Without a gap every ball goes at once in k order.
pub fn launch_order(s: &EmitterSettings, n: u32, round: u32, scene_seed: u64, thing: u32) -> Vec<u32> {
    let mut order: Vec<u32> = (0..n).collect();
    if s.gap <= 0.0 {
        return order;
    }
    match s.order {
        FireOrder::Around => {}
        FireOrder::Random => {
            let mut rng = stream(s, scene_seed, thing, round, Roll::Order);
            for i in (1..order.len()).rev() {
                let j = (rng.next_u32() as usize) % (i + 1);
                order.swap(i, j);
            }
        }
        FireOrder::MiddleOut => {
            let middle = (n.max(1) - 1) as f32 * 0.5;
            order.sort_by(|a, b| {
                let (da, db) = ((*a as f32 - middle).abs(), (*b as f32 - middle).abs());
                da.total_cmp(&db).then(a.cmp(b))
            });
        }
    }
    order
}

/// How many steps after a round fires its `j`-th ball goes, at `rate` steps a second.
pub fn slot_step(s: &EmitterSettings, j: u32, rate: u32) -> u64 {
    (j as f64 * s.gap.max(0.0) as f64 * rate as f64).round() as u64
}

/// The next round's size after one of `n`: n × Each round × plus Each round +, rounded, never
/// below 0 nor above Most balls.
pub fn next_count(s: &EmitterSettings, n: u32) -> u32 {
    let next = (n as f64 * s.grow_times.max(0.0) as f64 + s.grow_plus as f64).round();
    next.clamp(0.0, s.most_balls.max(1) as f64) as u32
}

/// The first round's size.
pub fn first_count(s: &EmitterSettings) -> u32 {
    s.count.clamp(1, s.most_balls.max(1))
}

/// Every round's size, when the rounds are known ahead (a limited number fired at the start or
/// every so often); `None` for rounds without end or rounds that wait on waves landing.
pub fn round_sizes(s: &EmitterSettings) -> Option<Vec<u32>> {
    let limit = s.round_limit()?;
    if s.fire_when == FireWhen::WhenAWaveLands {
        return None;
    }
    let mut sizes = Vec::with_capacity(limit as usize);
    let mut n = first_count(s);
    for _ in 0..limit {
        sizes.push(n);
        n = next_count(s, n);
    }
    Some(sizes)
}

/// How many balls the group counts for the group gradient's Order: every round's balls when they
/// are known ahead, at most Most balls; 0 when they aren't (then it counts those fired so far).
pub fn planned(s: &EmitterSettings) -> usize {
    round_sizes(s).map_or(0, |sizes| {
        let all: u64 = sizes.iter().map(|&n| n as u64).sum();
        all.min(s.most_balls.max(1) as u64) as usize
    })
}

/// How far along its way a ball is laid out in the group's places: a short way, so the balls
/// of a round from one point still spread apart.
pub fn layout_reach(s: &EmitterSettings) -> f32 {
    (6.0 * s.radius).max(48.0)
}

/// One round's places (the first round's, by k): each ball's start place a short way along its
/// direction, from the emitter, unturned. Random directions are those of round 1 with the scene
/// seed 0 and thing 0, so the layout is the same wherever the emitter is.
pub fn layout(s: &EmitterSettings) -> Vec<Vec2> {
    let n = first_count(s).min(s.most_balls.max(1));
    let reach = layout_reach(s);
    (0..n)
        .map(|k| {
            let dir = direction(s, k, n, 1, 0, 0);
            start_offset(s, k, n, dir) + Vec2::from_angle(dir) * reach
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::things::emitter::Earlier;

    fn deg(r: f32) -> f32 {
        let d = r.to_degrees().rem_euclid(360.0);
        if d > 180.0 { d - 360.0 } else { d }
    }

    #[test]
    fn all_round_spreads_evenly_from_the_aim() {
        let s = EmitterSettings { count: 4, ..EmitterSettings::default() };
        let ways: Vec<f32> = (0..4).map(|k| deg(direction(&s, k, 4, 1, 0, 0))).collect();
        for (got, want) in ways.iter().zip([-90.0, 0.0, 90.0, 180.0]) {
            assert!((got - want).abs() < 1e-3 || (got.abs() - 180.0).abs() < 1e-3 && want == 180.0, "{ways:?}");
        }
    }

    #[test]
    fn a_fan_spreads_across_the_aim_and_a_fan_of_one_goes_the_aim_way() {
        let s = EmitterSettings { directions: Directions::Fan, aim_angle: 0.0, fan_angle: 90.0, ..Default::default() };
        assert!((deg(direction(&s, 0, 3, 1, 0, 0)) + 45.0).abs() < 1e-3);
        assert!(deg(direction(&s, 1, 3, 1, 0, 0)).abs() < 1e-3);
        assert!((deg(direction(&s, 2, 3, 1, 0, 0)) - 45.0).abs() < 1e-3);
        assert!(deg(direction(&s, 0, 1, 1, 0, 0)).abs() < 1e-3);
        let one = EmitterSettings { directions: Directions::OneWay, aim_angle: 30.0, ..Default::default() };
        assert!((deg(direction(&one, 5, 9, 2, 0, 0)) - 30.0).abs() < 1e-3);
    }

    #[test]
    fn random_ways_are_seeded_and_stable() {
        let s = EmitterSettings { directions: Directions::Random, ..Default::default() };
        let a: Vec<f32> = (0..20).map(|k| direction(&s, k, 20, 1, 7, 3)).collect();
        let b: Vec<f32> = (0..20).map(|k| direction(&s, k, 20, 1, 7, 3)).collect();
        let c: Vec<f32> = (0..20).map(|k| direction(&s, k, 20, 2, 7, 3)).collect();
        assert_eq!(a, b);
        assert_ne!(a, c, "another round, other ways");
        assert!(a.iter().all(|d| (0.0..TAU).contains(d)));
    }

    #[test]
    fn ring_and_line_start_places() {
        let ring = EmitterSettings { start_from: StartFrom::Ring, start_radius: 50.0, ..Default::default() };
        let p = start_offset(&ring, 0, 4, 0.0);
        assert!((p - Vec2::new(50.0, 0.0)).length() < 1e-4);
        let line = EmitterSettings {
            start_from: StartFrom::Line,
            start_length: 100.0,
            aim_angle: -90.0,
            directions: Directions::OneWay,
            ..Default::default()
        };
        // Across an upward aim the line runs left to right.
        let ends = [start_offset(&line, 0, 3, 0.0), start_offset(&line, 1, 3, 0.0), start_offset(&line, 2, 3, 0.0)];
        assert!((ends[0].x.abs() - 50.0).abs() < 1e-3 && ends[0].y.abs() < 1e-3, "{ends:?}");
        assert!(ends[1].length() < 1e-4 && (ends[0] + ends[2]).length() < 1e-4);
        assert_eq!(start_offset(&line, 0, 1, 0.0), Vec2::ZERO, "one ball: the middle");
    }

    #[test]
    fn orders_with_a_gap() {
        let s = EmitterSettings { gap: 0.1, order: FireOrder::MiddleOut, ..Default::default() };
        assert_eq!(launch_order(&s, 5, 1, 0, 0), [2, 1, 3, 0, 4]);
        assert_eq!(launch_order(&s, 4, 1, 0, 0), [1, 2, 0, 3]);
        let around = EmitterSettings { gap: 0.1, ..Default::default() };
        assert_eq!(launch_order(&around, 3, 1, 0, 0), [0, 1, 2]);
        let random = EmitterSettings { gap: 0.1, order: FireOrder::Random, ..Default::default() };
        let r = launch_order(&random, 12, 1, 5, 2);
        assert_eq!(r, launch_order(&random, 12, 1, 5, 2));
        let mut sorted = r.clone();
        sorted.sort();
        assert_eq!(sorted, (0..12).collect::<Vec<u32>>());
        assert_ne!(r, sorted);
        let no_gap = EmitterSettings { order: FireOrder::Random, ..Default::default() };
        assert_eq!(launch_order(&no_gap, 3, 1, 0, 0), [0, 1, 2], "no gap: all at once in order");
        assert_eq!(slot_step(&s, 3, 240), 72);
    }

    #[test]
    fn round_sizes_grow_by_times_and_plus() {
        let s = EmitterSettings {
            fire_when: FireWhen::Every,
            rounds: 4,
            count: 10,
            grow_times: 10.0,
            grow_plus: -5,
            most_balls: 2000,
            ..Default::default()
        };
        assert_eq!(round_sizes(&s).unwrap(), [10, 95, 945, 2000]);
        assert_eq!(planned(&s), 2000, "capped at Most balls");
        let at_start = EmitterSettings { rounds: 9, ..s.clone() };
        assert_eq!(round_sizes(&EmitterSettings { fire_when: FireWhen::AtStart, ..at_start }).unwrap(), [10]);
        assert!(round_sizes(&EmitterSettings { endless: true, ..s.clone() }).is_none());
        let waves = EmitterSettings { fire_when: FireWhen::WhenAWaveLands, ..s.clone() };
        assert_eq!((round_sizes(&waves), planned(&waves)), (None, 0));
        assert_eq!(next_count(&EmitterSettings { grow_times: 0.1, ..s.clone() }, 3), 0, "never below 0");
        let _ = Earlier::Keep;
    }

    #[test]
    fn one_round_lays_out_a_short_way_along_its_directions() {
        let s = EmitterSettings { count: 4, radius: 10.0, ..Default::default() };
        let places = layout(&s);
        assert_eq!(places.len(), 4);
        assert!((places[0] - Vec2::new(0.0, -60.0)).length() < 1e-3, "{places:?}");
        assert!((places[1] - Vec2::new(60.0, 0.0)).length() < 1e-3, "{places:?}");
    }
}
