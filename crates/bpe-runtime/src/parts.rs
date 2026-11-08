//! A thing's parts (Text part 3): its items that rules can name one by one, the way they name a
//! group's balls, though they aren't balls: a Text's letters. The thing's sim keeps them; the rules
//! break and flash them, and Health keeps one health a part.

use glam::Vec2;

/// A thing's parts: whether each is still there, and its middle in the world now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parts {
    /// Part k is still there (a rule's Break takes it away for good).
    pub alive: Vec<bool>,
    /// Part k's middle, in scene pixels.
    pub at: Vec<Vec2>,
}

impl Parts {
    pub fn len(&self) -> usize {
        self.alive.len()
    }

    pub fn is_empty(&self) -> bool {
        self.alive.is_empty()
    }

    /// Whether part `k` is there.
    pub fn is_alive(&self, k: usize) -> bool {
        self.alive.get(k) == Some(&true)
    }

    /// Makes it `n` parts at `at` (its length `n`): the parts it had keep whether they are there,
    /// new ones are.
    pub fn set(&mut self, at: Vec<Vec2>) {
        self.alive.resize(at.len(), true);
        self.at = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_part_stays_broken_as_the_count_changes() {
        let mut p = Parts::default();
        p.set(vec![Vec2::ZERO; 3]);
        assert_eq!(p.alive, [true, true, true]);
        p.alive[1] = false;
        p.set(vec![Vec2::ONE; 5]);
        assert_eq!(p.alive, [true, false, true, true, true]);
        p.set(vec![Vec2::ONE; 1]);
        assert_eq!(p.alive, [true]);
        assert!(p.is_alive(0) && !p.is_alive(3));
    }
}
