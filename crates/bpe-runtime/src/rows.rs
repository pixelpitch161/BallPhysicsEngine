//! A Motion or Bounce card whose settings vary across the balls (see [`bpe_core::vary`]): a
//! prepared state per ball. Balls share a state when their places in the spreads fall in the same
//! of [`bpe_core::vary::VARY_LEVELS`] steps, so a card is prepared at most once per step of each
//! spread, however many balls it has.

use std::collections::HashMap;
use std::sync::Arc;

use bpe_core::Params;
use bpe_core::vary::{self, BallSpread, Vary};

use crate::plugins::{ModuleSim, ThingState};

/// What a varied card keeps per ball.
#[derive(Clone)]
pub(crate) struct Rows {
    varies: Vec<Vary>,
    salts: Vec<u32>,
    /// Some setting spreads by speed or hits, so every ball's place is found again each step.
    changes: bool,
    /// Each body row's state.
    states: Vec<Arc<dyn ThingState>>,
    /// The states made so far, by each varied setting's step in its spread.
    made: HashMap<Vec<u16>, Arc<dyn ThingState>>,
}

impl Rows {
    /// The rows of a card prepared from `params` by `sim`, when it varies and the sim can take a
    /// state per ball.
    pub(crate) fn of(sim: &dyn ModuleSim, params: &Params) -> Option<Rows> {
        if !sim.per_ball() {
            return None;
        }
        // A vary without a stored To has nowhere to go.
        let varies: Vec<Vary> = vary::varies_in(params).into_iter().filter(|v| v.to.is_some()).collect();
        if varies.is_empty() {
            return None;
        }
        Some(Rows {
            salts: varies.iter().map(Vary::salt).collect(),
            changes: varies.iter().any(|v| v.by.changes()),
            varies,
            states: Vec::new(),
            made: HashMap::new(),
        })
    }

    /// The first row whose state needs working out again, of `count` rows.
    pub(crate) fn stale_from(&self, count: usize) -> Option<usize> {
        let from = if self.changes { 0 } else { self.states.len() };
        (from < count).then_some(from)
    }

    /// Gives rows `from..` the states for their places in the spreads, `spreads` one per row
    /// from `from`, preparing any state not made yet from `params`.
    pub(crate) fn fill(&mut self, sim: &dyn ModuleSim, params: &Params, from: usize, spreads: &[BallSpread]) {
        self.states.truncate(from);
        for spread in spreads {
            let key: Vec<u16> =
                self.varies.iter().zip(&self.salts).map(|(v, &salt)| vary::level(spread.t(v.by, salt))).collect();
            let state = match self.made.get(&key) {
                Some(s) => s.clone(),
                None => {
                    let ts: Vec<f32> = key.iter().map(|&q| vary::at_level(q)).collect();
                    let own = vary::with_varies(params, &self.varies, &ts, &|_| None);
                    let s: Arc<dyn ThingState> = Arc::from(sim.prepare(&own));
                    self.made.insert(key, s.clone());
                    s
                }
            };
            self.states.push(state);
        }
    }

    /// Body row `row`'s state, when it has one yet.
    pub(crate) fn state(&self, row: usize) -> Option<&dyn ThingState> {
        self.states.get(row).map(|s| s.as_ref())
    }

    /// How many states were made, for tests.
    #[cfg(test)]
    pub(crate) fn made(&self) -> usize {
        self.made.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Keeps its `pull` setting, and counts how often it is prepared.
    struct Counting(AtomicUsize);
    impl ModuleSim for Counting {
        fn kind(&self) -> &'static str {
            "motion.counting"
        }
        fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
            self.0.fetch_add(1, Ordering::Relaxed);
            let Some(Value::Number(x)) = params.get("pull") else { return Box::new(0.0f64) };
            Box::new(*x)
        }
        fn per_ball(&self) -> bool {
            true
        }
    }

    fn varied(by: &str) -> Params {
        let mut p = Params::default();
        p.set("pull", Value::Number(0.0));
        p.set("pull~to", Value::Number(1023.0));
        p.set("pull~by", Value::Choice(by.into()));
        p
    }

    fn spreads(count: u32) -> Vec<BallSpread> {
        (0..count).map(|number| BallSpread { number, count, ..BallSpread::default() }).collect()
    }

    #[test]
    fn balls_share_a_state_per_step_of_the_spread() {
        let sim = Counting(AtomicUsize::new(0));
        let params = varied("number");
        let mut rows = Rows::of(&sim, &params).unwrap();
        assert_eq!(rows.stale_from(3000), Some(0));
        rows.fill(&sim, &params, 0, &spreads(3000));
        assert!(rows.made() <= 1024, "{}", rows.made());
        assert_eq!(sim.0.load(Ordering::Relaxed), rows.made());
        let pull = |rows: &Rows, row| *crate::state::<f64>(rows.state(row).unwrap());
        assert_eq!(pull(&rows, 0), 0.0);
        assert_eq!(pull(&rows, 2999), 1023.0);
        assert!(pull(&rows, 1500) > 500.0 && pull(&rows, 1500) < 524.0);
        // Nothing to do until a ball is added.
        assert_eq!(rows.stale_from(3000), None);
        assert_eq!(rows.stale_from(3001), Some(3000));
    }

    #[test]
    fn a_spread_that_moves_finds_every_ball_again() {
        let sim = Counting(AtomicUsize::new(0));
        let params = varied("hits");
        let mut rows = Rows::of(&sim, &params).unwrap();
        rows.fill(&sim, &params, 0, &spreads(4));
        assert_eq!(rows.stale_from(4), Some(0), "hits can change any time");
        let mut hit = spreads(4);
        hit[2].hits = 10;
        rows.fill(&sim, &params, 0, &hit);
        assert_eq!(*crate::state::<f64>(rows.state(2).unwrap()), 1023.0);
        assert_eq!(*crate::state::<f64>(rows.state(1).unwrap()), 0.0);
        assert_eq!(rows.made(), 2, "the no-hits state is made once");
    }

    #[test]
    fn a_card_without_varies_or_a_sim_that_cannot_has_no_rows() {
        struct Whole;
        impl ModuleSim for Whole {
            fn kind(&self) -> &'static str {
                "motion.whole"
            }
            fn prepare(&self, _params: &Params) -> Box<dyn ThingState> {
                Box::new(())
            }
        }
        assert!(Rows::of(&Whole, &varied("number")).is_none());
        let sim = Counting(AtomicUsize::new(0));
        assert!(Rows::of(&sim, &Params::default()).is_none());
        let mut no_to = varied("number");
        no_to.remove("pull~to");
        assert!(Rows::of(&sim, &no_to).is_none());
    }
}
