//! When each rule block fired, read from the playback's log: the "fired N times" counts, the
//! rings on blocks that just fired and the timeline's Rules lane.

use bpe_core::{BlockId, Rule, RuleId, ThingId};
use bpe_runtime::{Event, EventKind};
use std::collections::BTreeMap;

/// A block that fired this long before the playhead (or less) has a ring.
pub const RING_SECONDS: f64 = 0.15;

/// The times each block fired, worked out again when the log grows or the scene changes.
#[derive(Default)]
pub struct Fired {
    made_for: Option<(u64, usize)>,
    /// By (rule, block), in time order.
    times: BTreeMap<(RuleId, BlockId), Vec<f64>>,
}

/// The rule and block a Rule event's `b.item` names (`rule × 65536 + block`).
pub fn rule_and_block(item: u32) -> (RuleId, BlockId) {
    (RuleId(item / 65536), BlockId(item % 65536))
}

impl Fired {
    /// Reads `events` again unless they are what was read last (`revision` and their count).
    pub fn update(&mut self, revision: u64, events: &[Event], steps_per_second: u32) {
        let made_for = (revision, events.len());
        if self.made_for == Some(made_for) {
            return;
        }
        self.made_for = Some(made_for);
        self.times.clear();
        for e in events.iter().filter(|e| e.kind == EventKind::Rule) {
            let Some(b) = e.b.filter(|b| b.thing == ThingId::ROOT) else { continue };
            self.times.entry(rule_and_block(b.item)).or_default().push(e.time(steps_per_second));
        }
    }

    fn of(&self, rule: RuleId, block: BlockId) -> &[f64] {
        self.times.get(&(rule, block)).map(Vec::as_slice).unwrap_or_default()
    }

    /// Whether `block` fired within [`RING_SECONDS`] before `time` (or at it).
    pub fn recent(&self, rule: RuleId, block: BlockId, time: f64) -> bool {
        let times = self.of(rule, block);
        let upto = times.partition_point(|&t| t <= time + 1e-9);
        upto > 0 && times[upto - 1] >= time - RING_SECONDS - 1e-9
    }

    /// The moments the rule fired: when any of its end blocks (those leading nowhere) did, each
    /// moment once, in order.
    pub fn firings(&self, rule: &Rule) -> Vec<f64> {
        let mut all: Vec<f64> = rule
            .blocks
            .iter()
            .filter(|b| rule.outputs(b.id).is_empty())
            .flat_map(|b| self.of(rule.id, b.id).iter().copied())
            .collect();
        all.sort_by(f64::total_cmp);
        all.dedup();
        all
    }

    /// How many times the rule fired up to `time`.
    pub fn count(&self, rule: &Rule, time: f64) -> usize {
        let all = self.firings(rule);
        all.partition_point(|&t| t <= time + 1e-9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Block;
    use bpe_runtime::Party;
    use bpe_runtime::rules::rule_item;

    fn fired_at(step: u64, rule: u32, block: u32) -> Event {
        Event {
            step,
            kind: EventKind::Rule,
            a: Party { thing: ThingId::ROOT, item: 0 },
            b: Some(Party { thing: ThingId::ROOT, item: rule_item(RuleId(rule), BlockId(block)) }),
            point: Default::default(),
            normal: Default::default(),
            speed: 0.0,
            at: 0.0,
        }
    }

    #[test]
    fn counts_end_blocks_once_a_moment_and_rings_for_a_moment() {
        // Rule 1: When 0 → Do 1 and Do 2 (both ends).
        let rule = Rule {
            id: RuleId(1),
            name: "R".into(),
            enabled: true,
            blocks: vec![
                Block { id: BlockId(0), kind: "w".into(), ..Default::default() },
                Block { id: BlockId(1), kind: "d".into(), inputs: vec![BlockId(0)], ..Default::default() },
                Block { id: BlockId(2), kind: "d".into(), inputs: vec![BlockId(0)], ..Default::default() },
            ],
            next_block: 3,
        };
        let events = vec![
            fired_at(240, 1, 0),
            fired_at(240, 1, 1),
            fired_at(240, 1, 2),
            fired_at(480, 1, 0),
            fired_at(480, 1, 2),
            fired_at(480, 2, 1), // another rule
        ];
        let mut f = Fired::default();
        f.update(0, &events, 240);
        assert_eq!(f.firings(&rule), vec![1.0, 2.0]);
        assert_eq!(f.count(&rule, 0.5), 0);
        assert_eq!(f.count(&rule, 1.0), 1);
        assert_eq!(f.count(&rule, 5.0), 2);
        assert!(f.recent(RuleId(1), BlockId(1), 1.1));
        assert!(!f.recent(RuleId(1), BlockId(1), 1.2), "more than 0.15 s later");
        assert!(!f.recent(RuleId(1), BlockId(1), 0.99), "before it fired");
        assert!(f.recent(RuleId(1), BlockId(2), 2.0));
        // The same log and revision aren't read again; a longer log is.
        f.times.clear();
        f.update(0, &events, 240);
        assert!(f.times.is_empty());
        f.update(1, &events, 240);
        assert!(!f.times.is_empty());
        assert_eq!(rule_and_block(rule_item(RuleId(3), BlockId(7))), (RuleId(3), BlockId(7)));
    }
}
