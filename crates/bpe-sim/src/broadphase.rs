//! Ball-to-ball collisions (the stage after balls move in each substep). Owned by the physics
//! workstream, which replaces the simple solver here.

use crate::grid::BallGrid;
use crate::{Ball, Event, Simulation};

/// State kept between steps for ball-to-ball contacts.
#[derive(Clone, Default)]
pub(crate) struct Broadphase {
    grid: BallGrid,
}

/// Pushes overlapping balls apart and bounces them off each other, when the scene asks for it.
pub(crate) fn ball_ball(sim: &mut Simulation, _time: f64, _events: &mut Vec<Event>) {
    if sim.scene.physics.ball_collisions {
        let restitution = sim.scene.physics.restitution;
        collide_balls(&mut sim.balls, restitution, &mut sim.broadphase.grid);
    }
}

/// Elastic collisions between balls, found with a uniform grid. Heavier (bigger) balls move less.
pub(crate) fn collide_balls(balls: &mut [Ball], restitution: f32, grid: &mut BallGrid) {
    let max_radius = balls.iter().map(|b| b.radius).fold(0.0f32, f32::max);
    if balls.len() < 2 || max_radius <= 0.0 {
        return;
    }
    // Removed balls stay in the list where they left. Keeping them out of the grid stops
    // thousands of them piling up in one cell and every step checking each pair of them.
    let alive = balls.iter().enumerate().filter(|(_, b)| b.alive).map(|(i, b)| (i as u32, b.position));
    grid.build(alive, max_radius * 2.0);
    grid.for_each_pair(|i, j| resolve_pair(balls, i, j, restitution));
}

pub(crate) fn resolve_pair(balls: &mut [Ball], i: usize, j: usize, restitution: f32) {
    let (left, right) = balls.split_at_mut(j);
    let (a, b) = (&mut left[i], &mut right[0]);
    if !a.alive || !b.alive {
        return;
    }
    let offset = b.position - a.position;
    let distance = offset.length();
    let overlap = a.radius + b.radius - distance;
    if overlap <= 0.0 || distance == 0.0 {
        return;
    }
    let normal = offset / distance;
    let (mass_a, mass_b) = (a.radius * a.radius, b.radius * b.radius);
    let total = mass_a + mass_b;
    a.position -= normal * overlap * (mass_b / total);
    b.position += normal * overlap * (mass_a / total);
    let closing = (a.velocity - b.velocity).dot(normal);
    if closing > 0.0 {
        let impulse = closing * (1.0 + restitution) / total;
        a.velocity -= normal * impulse * mass_b;
        b.velocity += normal * impulse * mass_a;
    }
}
