//! The order a shape's modulators run in each block: a modulator whose knobs others move runs
//! after them. Where they move each other round in a loop, the first in the card runs first and
//! hears the others as they were a block (1.3 ms) earlier.

use bpe_core::{LayerId, ModRoute, split_layered};

/// The modulators `mods` (in card order) in the order they run, and the routes that close a
/// loop (source, target modulator): those read their source's value from the block before. A
/// modulator moving its own knob is always late.
pub fn mod_order(routes: &[ModRoute], mods: &[LayerId]) -> (Vec<LayerId>, Vec<(LayerId, LayerId)>) {
    let mut edges: Vec<(LayerId, LayerId)> = Vec::new();
    for r in routes {
        let Some((target, _)) = split_layered(&r.target) else { continue };
        if mods.contains(&r.source) && mods.contains(&target) && !edges.contains(&(r.source, target)) {
            edges.push((r.source, target));
        }
    }
    let mut late: Vec<(LayerId, LayerId)> = edges.iter().copied().filter(|(a, b)| a == b).collect();
    let mut done: Vec<LayerId> = Vec::with_capacity(mods.len());
    let mut left: Vec<LayerId> = mods.to_vec();
    while !left.is_empty() {
        let ready = |m: &LayerId| edges.iter().all(|(a, b)| b != m || a == b || done.contains(a));
        let i = left.iter().position(ready).unwrap_or(0);
        let m = left.remove(i);
        for &(a, b) in &edges {
            if b == m && a != b && !done.contains(&a) {
                late.push((a, b));
            }
        }
        done.push(m);
    }
    (done, late)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(source: u32, target: &str) -> ModRoute {
        ModRoute { source: LayerId(source), target: target.into(), amount: 0.5, bipolar: false, invert: false }
    }

    #[test]
    fn a_modulator_runs_after_those_moving_it() {
        let mods = [LayerId(1), LayerId(2), LayerId(3)];
        let (order, late) = mod_order(&[r(3, "1/rate_hz"), r(1, "a_cents")], &mods);
        assert_eq!(order, [LayerId(2), LayerId(3), LayerId(1)]);
        assert!(late.is_empty());
    }

    #[test]
    fn a_loop_breaks_at_the_first_in_the_card() {
        let mods = [LayerId(4), LayerId(5)];
        let (order, late) = mod_order(&[r(5, "4/rate_hz"), r(4, "5/rate_hz"), r(4, "4/phase")], &mods);
        assert_eq!(order, [LayerId(4), LayerId(5)]);
        assert!(late.contains(&(LayerId(5), LayerId(4))));
        assert!(late.contains(&(LayerId(4), LayerId(4))), "its own knob");
        assert!(!late.contains(&(LayerId(4), LayerId(5))));
    }
}
