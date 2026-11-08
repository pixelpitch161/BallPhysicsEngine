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
    /// Where the ball first appeared, for colours that follow the start. Set once in `push`;
    /// it is not part of the physics fingerprint or the golden logs.
    pub spawn: Vec<Vec2>,
    pub alive: Vec<bool>,
    /// How many times the ball has hit something.
    pub hits: Vec<u32>,
    /// Each ball's number in its thing: its row unless the thing numbers them otherwise (a group
    /// lets its balls go in another order than their places). What riders name a ball by.
    pub number: Vec<u32>,
    /// Held still by something (a catcher waiting to send it back out): not moved, not bounced
    /// off walls or other balls, and never caught.
    pub held: Vec<bool>,
    /// Every row below this one is removed. A removed ball never comes back (nothing sets `alive`
    /// back to true), so loops over the balls may start here ([`Bodies::live_rows`]): an endless
    /// emitter whose oldest rounds go stops paying for them. Moved on by [`Bodies::note_gone`].
    pub gone_below: usize,
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
        self.spawn.push(position);
        self.alive.push(true);
        self.hits.push(0);
        self.number.push(row);
        self.held.push(false);
        row
    }

    /// The rows that may still be alive: from [`Bodies::gone_below`] on.
    pub fn live_rows(&self) -> std::ops::Range<usize> {
        self.gone_below.min(self.len())..self.len()
    }

    /// Moves [`Bodies::gone_below`] past the removed rows at its front.
    pub fn note_gone(&mut self) {
        while self.gone_below < self.len() && !self.alive[self.gone_below] {
            self.gone_below += 1;
        }
    }

    /// The row of the ball numbered `number`, if there is one.
    pub fn row_of(&self, number: u32) -> Option<usize> {
        // Balls split by a rule share their number: the first one still there.
        let mut rows = (0..self.len()).filter(|&i| self.number[i] == number);
        let first = rows.next()?;
        if self.alive[first] { Some(first) } else { rows.find(|&i| self.alive[i]).or(Some(first)) }
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
        self.position.capacity() * std::mem::size_of::<Vec2>() * 5
            + self.radius.capacity() * std::mem::size_of::<f32>()
            + self.alive.capacity()
            + self.held.capacity()
            + (self.hits.capacity() + self.number.capacity()) * std::mem::size_of::<u32>()
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
        assert_eq!(bodies.spawn[0], Vec2::new(1.0, 2.0));
        bodies.position[0] = Vec2::ZERO;
        assert_eq!(bodies.spawn[0], Vec2::new(1.0, 2.0), "spawn stays where the ball began");
        assert!(bodies.alive[1]);
        assert_eq!(bodies.hits[1], 0);
        assert_eq!(bodies.number, [0, 1], "numbered by row unless told otherwise");
        assert_eq!(bodies.held, [false, false]);
        bodies.number[0] = 7;
        assert_eq!((bodies.row_of(7), bodies.row_of(1), bodies.row_of(0)), (Some(0), Some(1), None));
        bodies.alive[1] = false;
        assert_eq!(bodies.alive_count(), 1);
        bodies.note_gone();
        assert_eq!(bodies.live_rows(), 0..2, "ball 0 is still there");
        bodies.alive[0] = false;
        bodies.note_gone();
        assert_eq!(bodies.live_rows(), 2..2);
        bodies.push(Vec2::ZERO, Vec2::ZERO, 1.0);
        assert_eq!(bodies.live_rows(), 2..3);
        assert!(bodies.memory() >= 2 * (4 * 8 + 4 + 1 + 4));
    }
}
