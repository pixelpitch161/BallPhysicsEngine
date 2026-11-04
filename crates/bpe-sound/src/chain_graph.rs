//! Which chain layer reads which, and the order they render in. Each layer takes the sound of
//! the layer above it in its oscillator's lane (the oscillator itself at the top); a
//! Modulation layer also taps a source: an oscillator as it reaches its first layer, or the
//! sound just after any layer of either lane. A tap on a layer rendered later in the same sample
//! (the two lanes feeding each other, or a layer feeding itself) is a loop: it reads that
//! layer's sound one sample late. Pure: the editor uses it for the loop mark.

use bpe_core::{Layer, LayerId, Slot, Value, layered_key};
use bpe_kit::sound::{CHAIN_KINDS, MAX_CHAIN, MODULATION, Source};

/// Where a layer's sound comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    /// Oscillator 0 (A) or 1 (B) as it reaches its first layer (after its Gain).
    Raw(u8),
    /// After the layer at this place in [`ChainGraph::layers`].
    Layer(usize),
}

/// One chain layer as the graph sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainLayer {
    pub id: LayerId,
    pub lane: u8,
    pub kind: String,
    pub enabled: bool,
    pub solo: bool,
    /// A Modulation layer's source (others have none).
    pub source: Option<Source>,
}

/// Who reads whom, for one sound card.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainGraph {
    /// The layers that run: enabled, of a chain kind, the first [`MAX_CHAIN`] of the card, in
    /// card order (each lane top to bottom).
    pub layers: Vec<ChainLayer>,
    /// Each layer's input: the nearest running layer above it in its lane, or its oscillator.
    pub input: Vec<Tap>,
    /// Each Modulation layer's source, and whether it is a loop (read one sample late).
    pub source: Vec<Option<(Tap, bool)>>,
    /// The render order (places in `layers`): every tap that is not a loop comes earlier.
    pub order: Vec<usize>,
    /// What each lane plays: its topmost soloed layer, else its last layer, else the
    /// oscillator itself.
    pub out: [Tap; 2],
    /// The layers whose source loops back.
    pub loops: Vec<LayerId>,
}

impl ChainGraph {
    /// The graph of `all`, the card's chain layers in card order (any others, and disabled
    /// ones, may be among them: they are passed over). A tap on a disabled layer reads what
    /// reaches it; a tap on a layer that isn't there falls back to the tapping layer's own
    /// oscillator.
    pub fn build(all: &[ChainLayer]) -> ChainGraph {
        let chain: Vec<&ChainLayer> = all.iter().filter(|l| CHAIN_KINDS.contains(&l.kind.as_str())).collect();
        // The running layers, and for every chain layer (running or not) the tap that stands
        // for "after it".
        let mut layers: Vec<ChainLayer> = Vec::new();
        let mut after: Vec<(LayerId, Tap)> = Vec::new();
        let mut input = Vec::new();
        let mut last = [Tap::Raw(0), Tap::Raw(1)];
        for (n, l) in chain.iter().enumerate() {
            let lane = l.lane.min(1);
            let runs = l.enabled && n < MAX_CHAIN;
            if runs {
                input.push(last[lane as usize]);
                last[lane as usize] = Tap::Layer(layers.len());
                layers.push(ChainLayer { lane, ..(*l).clone() });
            }
            after.push((l.id, last[lane as usize]));
        }
        let tap_of = |id: LayerId| after.iter().find(|(l, _)| *l == id).map(|(_, t)| *t);
        let resolved: Vec<Option<Tap>> = layers
            .iter()
            .map(|l| {
                let s = l.source?.in_lane(l.lane);
                Some(match s {
                    Source::Raw(o) => Tap::Raw(o.min(1)),
                    Source::After(id) => tap_of(id).unwrap_or(Tap::Raw(l.lane)),
                    Source::OtherRaw => Tap::Raw(1 - l.lane),
                })
            })
            .collect();
        // Each layer renders after its input. Sources are taken in card order, each after what
        // it taps unless that would close a circle: then it is a loop and reads one sample late.
        let n = layers.len();
        let mut before: Vec<Vec<usize>> = (0..n)
            .map(|i| match input[i] {
                Tap::Layer(j) => vec![j],
                Tap::Raw(_) => Vec::new(),
            })
            .collect();
        // Whether `a` already waits for `b` (through any number of steps).
        let waits = |before: &Vec<Vec<usize>>, a: usize, b: usize| {
            let mut seen = vec![false; n];
            let mut todo = vec![a];
            while let Some(x) = todo.pop() {
                if x == b {
                    return true;
                }
                if !std::mem::replace(&mut seen[x], true) {
                    todo.extend(before[x].iter().copied());
                }
            }
            false
        };
        let mut is_loop = vec![false; n];
        for i in 0..n {
            if let Some(Tap::Layer(j)) = resolved[i] {
                if j == i || waits(&before, j, i) {
                    is_loop[i] = true;
                } else {
                    before[i].push(j);
                }
            }
        }
        // The first layer (in card order) whose waits are over goes next.
        let mut order = Vec::with_capacity(n);
        let mut placed = vec![false; n];
        while order.len() < n {
            let next =
                (0..n).find(|&i| !placed[i] && before[i].iter().all(|&j| placed[j])).expect("no circles are left");
            placed[next] = true;
            order.push(next);
        }
        let source: Vec<Option<(Tap, bool)>> =
            resolved.iter().enumerate().map(|(i, t)| t.map(|t| (t, is_loop[i]))).collect();
        let out = std::array::from_fn(|lane| {
            layers.iter().position(|l| l.solo && l.lane as usize == lane).map(Tap::Layer).unwrap_or(last[lane])
        });
        let loops = layers.iter().zip(&is_loop).filter(|(_, l)| **l).map(|(l, _)| l.id).collect();
        ChainGraph { layers, input, source, order, out, loops }
    }

    /// Which layers must run, and which oscillators must play, for the lanes in `heard` to be
    /// heard: what each heard lane plays, and everything that feeds it.
    pub fn needed(&self, heard: [bool; 2]) -> (Vec<bool>, [bool; 2]) {
        let mut need = vec![false; self.layers.len()];
        let mut raw = [false; 2];
        let mut todo: Vec<Tap> = (0..2).filter(|&o| heard[o]).map(|o| self.out[o]).collect();
        while let Some(t) = todo.pop() {
            match t {
                Tap::Raw(o) => raw[o as usize] = true,
                Tap::Layer(i) => {
                    if need[i] {
                        continue;
                    }
                    need[i] = true;
                    todo.push(self.input[i]);
                    if let Some((s, _)) = self.source[i] {
                        todo.push(s);
                    }
                }
            }
        }
        (need, raw)
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

/// The card's chain layers as the graph reads them, values from `get` (a layered key's value).
pub fn chain_layers(layers: &[Layer], get: &dyn Fn(&str) -> Option<Value>) -> Vec<ChainLayer> {
    layers
        .iter()
        .filter(|l| CHAIN_KINDS.contains(&l.kind.as_str()))
        .map(|l| {
            let solo = matches!(get(&layered_key(l.id, "solo")), Some(Value::Bool(true)));
            let source = (l.kind == MODULATION).then(|| match get(&layered_key(l.id, "source")) {
                Some(Value::Text(t)) => Source::parse(&t),
                _ => Source::OtherRaw,
            });
            ChainLayer { id: l.id, lane: l.lane.unwrap_or(0), kind: l.kind.clone(), enabled: l.enabled, solo, source }
        })
        .collect()
}

/// The layers of a sound card whose source loops back (the editor's loop mark).
pub fn loops_of(card: &Slot) -> Vec<LayerId> {
    ChainGraph::build(&chain_layers(&card.layers, &|k| card.params.get(k).cloned())).loops
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::sound::{DISTORTION, FILTER};

    fn layer(id: u32, lane: u8, kind: &str, source: Option<Source>) -> ChainLayer {
        ChainLayer { id: LayerId(id), lane, kind: kind.into(), enabled: true, solo: false, source }
    }

    #[test]
    fn a_lane_runs_top_to_bottom() {
        let g =
            ChainGraph::build(&[layer(1, 0, FILTER, None), layer(2, 0, DISTORTION, None), layer(3, 1, FILTER, None)]);
        assert_eq!(g.input, vec![Tap::Raw(0), Tap::Layer(0), Tap::Raw(1)]);
        assert_eq!(g.out, [Tap::Layer(1), Tap::Layer(2)]);
        assert_eq!(g.order, vec![0, 1, 2]);
        assert!(g.loops.is_empty());
    }

    #[test]
    fn a_source_on_the_other_lane_renders_first() {
        // A's FM taps B after B's filter: B's filter renders before it.
        let g =
            ChainGraph::build(&[layer(1, 0, MODULATION, Some(Source::After(LayerId(2)))), layer(2, 1, FILTER, None)]);
        assert_eq!(g.source[0], Some((Tap::Layer(1), false)));
        assert_eq!(g.order, vec![1, 0]);
        assert!(g.loops.is_empty());
    }

    #[test]
    fn lanes_feeding_each_other_are_a_loop() {
        let g = ChainGraph::build(&[
            layer(1, 0, MODULATION, Some(Source::After(LayerId(3)))),
            layer(2, 1, FILTER, None),
            layer(3, 1, MODULATION, Some(Source::After(LayerId(1)))),
        ]);
        // The first in card order (A's FM) hears its source on time; B's FM, which would close
        // the circle, hears A's one sample late.
        assert_eq!(g.loops, vec![LayerId(3)]);
        let pos = |i: usize| g.order.iter().position(|&o| o == i).unwrap();
        assert!(pos(2) < pos(0), "A's FM reads B's FM this sample");
        // A layer tapping a layer under it in its own lane.
        let down =
            ChainGraph::build(&[layer(1, 0, MODULATION, Some(Source::After(LayerId(2)))), layer(2, 0, FILTER, None)]);
        assert_eq!(down.loops, vec![LayerId(1)]);
        assert_eq!(down.order, vec![0, 1]);
        // A layer tapping itself loops too.
        let me = ChainGraph::build(&[layer(5, 0, MODULATION, Some(Source::After(LayerId(5))))]);
        assert_eq!(me.loops, vec![LayerId(5)]);
    }

    #[test]
    fn taps_on_missing_or_disabled_layers_fall_back() {
        let mut off = layer(2, 1, FILTER, None);
        off.enabled = false;
        let g = ChainGraph::build(&[
            layer(1, 1, FILTER, None),
            off,
            layer(3, 0, MODULATION, Some(Source::After(LayerId(2)))),
            layer(4, 0, MODULATION, Some(Source::After(LayerId(99)))),
        ]);
        assert_eq!(g.layers.len(), 3);
        assert_eq!(g.source[1], Some((Tap::Layer(0), false)), "after a switched-off layer: what reaches it");
        assert_eq!(g.source[2], Some((Tap::Raw(0), false)), "a gone layer: its own oscillator");
    }

    #[test]
    fn solo_and_needs() {
        let mut solo = layer(2, 0, FILTER, None);
        solo.solo = true;
        let g = ChainGraph::build(&[
            layer(1, 0, FILTER, None),
            solo,
            layer(3, 0, DISTORTION, None),
            layer(4, 1, MODULATION, Some(Source::Raw(0))),
        ]);
        assert_eq!(g.out[0], Tap::Layer(1));
        let (need, raw) = g.needed([true, false]);
        assert_eq!(need, vec![true, true, false, false]);
        assert_eq!(raw, [true, false]);
        let (need, raw) = g.needed([false, true]);
        assert_eq!(need, vec![false, false, false, true]);
        assert_eq!(raw, [true, true], "B's FM needs A playing though A isn't heard");
    }
}
