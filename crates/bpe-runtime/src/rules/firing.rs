//! A firing: what a When block makes when it fires, flowing along its rule's lane.

use glam::Vec2;

use crate::Party;

/// Named numbers a firing carries: `strength`, `speed`, `size`, `hits`, `ball`, `time`, `count`,
/// `bar`, `beat` and whatever later blocks add.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values(pub Vec<(&'static str, f64)>);

impl Values {
    pub fn get(&self, name: &str) -> Option<f64> {
        self.0.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }

    /// Sets `name`, replacing it when it is there.
    pub fn set(&mut self, name: &'static str, v: f64) {
        match self.0.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = v,
            None => self.0.push((name, v)),
        }
    }

    /// Adds every value of `other` whose name this doesn't have yet.
    pub fn merge_missing(&mut self, other: &Values) {
        for (n, v) in &other.0 {
            if self.get(n).is_none() {
                self.0.push((n, *v));
            }
        }
    }
}

/// Who a firing is about and what it knows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Firing {
    /// The step it was made in.
    pub step: u64,
    /// "That ball" (or that thing): a ball is its thing and body row.
    pub subject: Option<Party>,
    /// "The thing it hit": a shape (item 0) or the other ball.
    pub other: Option<Party>,
    /// Where it happened, in scene pixels.
    pub point: Vec2,
    /// The hit's normal, pointing away from what was hit; zero otherwise.
    pub normal: Vec2,
    pub values: Values,
}

/// A named signal a Send signal block sent, with the firing that reached it: When Signal blocks
/// hear it in the stage's next round.
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub name: String,
    pub firing: Firing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_set_get_and_merge() {
        let mut a = Values::default();
        a.set("speed", 3.0);
        a.set("speed", 4.0);
        assert_eq!(a.get("speed"), Some(4.0));
        let mut b = Values::default();
        b.set("speed", 9.0);
        b.set("size", 2.0);
        a.merge_missing(&b);
        assert_eq!((a.get("speed"), a.get("size"), a.get("hits")), (Some(4.0), Some(2.0), None));
    }
}
