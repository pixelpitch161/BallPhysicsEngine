//! Where a rule's blocks sit in its lane: worked out from the links, never placed by hand. A
//! block's column is the longest path to it from a block nothing leads into; its row is its place
//! among the blocks of its column, in the order they were added.

use bpe_core::{BlockId, Rule};
use std::collections::BTreeMap;

/// One block's place in its lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub block: BlockId,
    pub column: usize,
    pub row: usize,
}

/// A line from one block into another: `input` is its place among the target's inputs (a Logic
/// block's lines meet its left edge spread out in stacked order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub from: BlockId,
    pub to: BlockId,
    pub input: usize,
}

/// A whole lane's layout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaneLayout {
    /// In block order.
    pub places: Vec<Place>,
    pub columns: usize,
    /// The most rows any column has (at least one, so an empty lane still has room).
    pub rows: usize,
    pub links: Vec<Link>,
}

impl LaneLayout {
    pub fn place(&self, block: BlockId) -> Option<Place> {
        self.places.iter().copied().find(|p| p.block == block)
    }
}

/// Lays `rule` out. Inputs that name no block of the rule are left out; links that go round in a
/// circle still give finite columns (at most the number of blocks).
pub fn lay_out(rule: &Rule) -> LaneLayout {
    let n = rule.blocks.len();
    let index: BTreeMap<BlockId, usize> = rule.blocks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let inputs: Vec<Vec<usize>> =
        rule.blocks.iter().map(|b| b.inputs.iter().filter_map(|i| index.get(i).copied()).collect()).collect();
    // The longest path, relaxed until it settles (or `n` times, for a circle).
    let mut column = vec![0usize; n];
    for _ in 0..n {
        let mut moved = false;
        for (b, ins) in inputs.iter().enumerate() {
            let want = ins.iter().map(|&i| column[i] + 1).max().unwrap_or(0).min(n);
            if want != column[b] {
                column[b] = want;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    let mut filled: BTreeMap<usize, usize> = BTreeMap::new();
    let places: Vec<Place> = rule
        .blocks
        .iter()
        .zip(&column)
        .map(|(b, &c)| {
            let row = filled.entry(c).or_default();
            let place = Place { block: b.id, column: c, row: *row };
            *row += 1;
            place
        })
        .collect();
    let links = rule
        .blocks
        .iter()
        .flat_map(|b| {
            b.inputs.iter().filter(|i| index.contains_key(i)).enumerate().map(move |(k, &from)| Link {
                from,
                to: b.id,
                input: k,
            })
        })
        .collect();
    LaneLayout {
        places,
        columns: column.iter().max().map_or(0, |c| c + 1),
        rows: filled.values().copied().max().unwrap_or(0).max(1),
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Block, RuleId};

    fn rule(blocks: &[(u32, &[u32])]) -> Rule {
        Rule {
            id: RuleId(0),
            name: "R".into(),
            enabled: true,
            blocks: blocks
                .iter()
                .map(|(id, ins)| Block {
                    id: BlockId(*id),
                    kind: "x".into(),
                    inputs: ins.iter().map(|i| BlockId(*i)).collect(),
                    params: Default::default(),
                })
                .collect(),
            next_block: 10,
        }
    }

    fn at(l: &LaneLayout, b: u32) -> (usize, usize) {
        let p = l.place(BlockId(b)).unwrap();
        (p.column, p.row)
    }

    #[test]
    fn a_chain_goes_left_to_right() {
        let l = lay_out(&rule(&[(0, &[]), (1, &[0]), (2, &[1])]));
        assert_eq!((at(&l, 0), at(&l, 1), at(&l, 2)), ((0, 0), (1, 0), (2, 0)));
        assert_eq!((l.columns, l.rows), (3, 1));
        assert_eq!(l.links.len(), 2);
    }

    #[test]
    fn a_branch_takes_a_second_row() {
        let l = lay_out(&rule(&[(0, &[]), (1, &[0]), (2, &[0])]));
        assert_eq!((at(&l, 1), at(&l, 2)), ((1, 0), (1, 1)));
        assert_eq!(l.rows, 2);
    }

    #[test]
    fn two_whens_join_a_logic_block_after_the_longest_path() {
        // When 0 → If 1 → Logic 3; When 2 → Logic 3; Do 4 after the Logic.
        let l = lay_out(&rule(&[(0, &[]), (1, &[0]), (2, &[]), (3, &[1, 2]), (4, &[3])]));
        assert_eq!((at(&l, 0), at(&l, 2)), ((0, 0), (0, 1)));
        assert_eq!(at(&l, 3), (2, 0), "the longest path in is two long");
        assert_eq!(at(&l, 4), (3, 0));
        let into: Vec<(u32, usize)> =
            l.links.iter().filter(|k| k.to == BlockId(3)).map(|k| (k.from.0, k.input)).collect();
        assert_eq!(into, vec![(1, 0), (2, 1)], "in stacked order");
    }

    #[test]
    fn a_circle_stays_finite_and_a_missing_input_is_left_out() {
        let l = lay_out(&rule(&[(0, &[1]), (1, &[0]), (2, &[9])]));
        assert!(l.places.iter().all(|p| p.column <= 3));
        assert_eq!(at(&l, 2), (0, 0), "its only input isn't there");
        assert!(l.links.iter().all(|k| k.from != BlockId(9)));
        assert_eq!(lay_out(&rule(&[])).rows, 1);
    }
}
