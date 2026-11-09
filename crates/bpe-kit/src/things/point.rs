//! Where a thing is, as one point: what a light aims at (and the camera's follow reads alike).

use bpe_core::ThingId;
use bpe_runtime::World;
use glam::Vec2;

/// The point of thing `id` in `world`: for a thing with balls, ball number `ball` (the first is
/// number one) while it is alive, else (0, or a ball that is gone) the middle of its live balls;
/// for a wall (a shape, a Text of letters) its outline's middle; else its pose. `None` when it has
/// no live balls or no place.
pub fn point_of(world: &World, id: ThingId, ball: u32) -> Option<Vec2> {
    if let Some(b) = world.bodies.get(&id) {
        if ball >= 1
            && let Some(row) = b.row_of(ball - 1).filter(|&r| b.alive[r])
        {
            return Some(b.position[row]);
        }
        let alive: Vec<usize> = (0..b.len()).filter(|&r| b.alive[r]).collect();
        if alive.is_empty() {
            return None;
        }
        return Some(alive.iter().map(|&r| b.position[r]).sum::<Vec2>() / alive.len() as f32);
    }
    if let Some(wall) = world.walls.get(&id) {
        return Some(crate::outline::centre(&wall.outline));
    }
    world.poses.get(&id).map(|p| p.position)
}
