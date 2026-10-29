//! The ball table: one column per property, one row per ball, in spawn order.

use glam::Vec2;

/// Every ball in the world. Rows are never removed; a removed ball is `alive == false`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bodies {
    pub position: Vec<Vec2>,
    pub velocity: Vec<Vec2>,
    pub radius: Vec<f32>,
    /// Where the ball was at the start of the step, for trails.
    pub previous: Vec<Vec2>,
    /// Where the ball was before the current substep moved it, so a hit on a solid can sweep the
    /// path it really took, whatever a wall did to it since.
    pub start: Vec<Vec2>,
    pub alive: Vec<bool>,
    /// How many times the ball has hit something.
    pub hits: Vec<u32>,
    /// A colour laid over the look's, linear RGBA; the alpha is how strongly. Zero alpha means
    /// the ball shows its look's colour.
    pub tint: Vec<[f32; 4]>,
}

impl Bodies {
    /// Adds a ball and returns its row.
    pub fn push(&mut self, position: Vec2, velocity: Vec2, radius: f32) -> u32 {
        let row = self.position.len() as u32;
        self.position.push(position);
        self.velocity.push(velocity);
        self.radius.push(radius);
        self.previous.push(position);
        self.start.push(position);
        self.alive.push(true);
        self.hits.push(0);
        self.tint.push([0.0; 4]);
        row
    }

    pub fn len(&self) -> usize {
        self.position.len()
    }

    pub fn is_empty(&self) -> bool {
        self.position.is_empty()
    }

    pub fn alive_count(&self) -> usize {
        self.alive.iter().filter(|&&a| a).count()
    }

    /// Bytes the table takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.position.capacity() * std::mem::size_of::<Vec2>() * 4
            + self.radius.capacity() * std::mem::size_of::<f32>()
            + self.alive.capacity()
            + self.hits.capacity() * std::mem::size_of::<u32>()
            + self.tint.capacity() * std::mem::size_of::<[f32; 4]>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_fills_every_column() {
        let mut bodies = Bodies::default();
        let i = bodies.push(Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0), 5.0);
        let j = bodies.push(Vec2::ZERO, Vec2::ZERO, 1.0);
        assert_eq!((i, j), (0, 1));
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies.previous[0], Vec2::new(1.0, 2.0));
        assert_eq!(bodies.radius[0], 5.0);
        assert!(bodies.alive[1]);
        assert_eq!(bodies.hits[1], 0);
        assert_eq!(bodies.tint[1], [0.0; 4]);
        bodies.alive[1] = false;
        assert_eq!(bodies.alive_count(), 1);
        assert!(bodies.memory() >= 2 * (4 * 8 + 4 + 1 + 4 + 16));
    }
}
