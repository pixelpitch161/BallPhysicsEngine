//! Follow…: wiring an input block into a knob, and taking it out again.

use crate::catalog::{catalog, followable};
use crate::inputs::inputs_of;
use bpe_graph::{Catalog, Graph, Link, Param};
use std::collections::BTreeSet;

/// What a knob follows: an input block kind, which of its inputs, and for Channel level, which
/// channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowInput {
    /// "hit_input", "live_input" or "scene_input".
    pub kind: String,
    pub input: String,
    pub channel: String,
}

/// The pin a followable knob is wired through.
fn knob_pin(graph: &Graph, node: u32, param: &str) -> Option<(u32, u8)> {
    let n = graph.node(node)?;
    let spec = catalog().spec(&n.kind)?;
    let list = followable(&n.kind);
    let i = list.iter().position(|k| *k == param)?;
    Some((node, (spec.inputs.len() - list.len() + i) as u8))
}

/// `graph` with `param` of `node` following `with`: an input block into a Scale and offset
/// (the depth) into the knob's pin. Replaces whatever the knob followed. `None` when the knob
/// can't follow anything.
pub fn follow(graph: &Graph, node: u32, param: &str, with: &FollowInput) -> Option<Graph> {
    let pin = knob_pin(graph, node, param)?;
    let cat = catalog();
    let input_spec = cat.spec(&with.kind)?;
    let def = inputs_of(&with.kind).iter().find(|d| d.name == with.input)?;
    let mut g = unfollow(graph, node, param);
    let target = g.node(node)?;
    let spec = cat.spec(&target.kind)?;
    let p = spec.param_spec(param)?;
    let value = target.number(spec, param);
    let (min, max) = p.range;
    let depth = if value <= (min + max) / 2.0 { max - value } else { min - value };
    let at = target.pos;
    let input = g.add(input_spec, [at[0] - 380.0, at[1] + 160.0]);
    let n = g.node_mut(input)?;
    n.params.insert("input".into(), Param::Choice(def.name.into()));
    n.params.insert("low".into(), Param::Number(def.low));
    n.params.insert("high".into(), Param::Number(def.high));
    if !with.channel.is_empty() {
        n.params.insert("channel".into(), Param::Text(with.channel.clone()));
    }
    let scale = g.add(cat.spec("scale")?, [at[0] - 200.0, at[1] + 160.0]);
    g.node_mut(scale)?.params.insert("scale".into(), Param::Number(depth));
    g.connect(Link { from: (input, 0), to: (scale, 0) }, cat);
    g.connect(Link { from: (scale, 0), to: pin }, cat);
    Some(g)
}

/// `graph` with nothing wired into `param`'s pin, and every block that then feeds nothing
/// removed.
pub fn unfollow(graph: &Graph, node: u32, param: &str) -> Graph {
    let mut g = graph.clone();
    let Some(pin) = knob_pin(graph, node, param) else { return g };
    let feeding: Vec<u32> = g.links.iter().filter(|l| l.to == pin).map(|l| l.from.0).collect();
    g.links.retain(|l| l.to != pin);
    let mut check = feeding;
    while let Some(id) = check.pop() {
        let feeds_something = g.links.iter().any(|l| l.from.0 == id);
        let is_end = g.node(id).is_none_or(|n| matches!(n.kind.as_str(), "fx_in" | "fx_out" | "voice_out"));
        if feeds_something || is_end {
            continue;
        }
        check.extend(g.links.iter().filter(|l| l.to.0 == id).map(|l| l.from.0));
        g.remove(&BTreeSet::from([id]));
    }
    g
}

/// What `param` of `node` follows, as the input's label ("Height in the frame", or "Channel
/// level (Kick)"), when an input block feeds its pin.
pub fn followed(graph: &Graph, node: u32, param: &str) -> Option<String> {
    let pin = knob_pin(graph, node, param)?;
    let mut at = graph.links.iter().find(|l| l.to == pin)?.from.0;
    // Back through the blocks between, to the input block.
    for _ in 0..graph.nodes.len() {
        let n = graph.node(at)?;
        let table = inputs_of(&n.kind);
        if !table.is_empty() {
            let name = match n.params.get("input") {
                Some(Param::Choice(c)) | Some(Param::Text(c)) => c.as_str(),
                _ => table[0].name,
            };
            let def = table.iter().find(|d| d.name == name).unwrap_or(&table[0]);
            return Some(match n.params.get("channel") {
                Some(Param::Text(c)) if def.name == "channel" && !c.is_empty() => format!("{} ({c})", def.label),
                _ => def.label.to_string(),
            });
        }
        at = graph.links.iter().find(|l| l.to.0 == at)?.from.0;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blank_patch;

    fn live(input: &str) -> FollowInput {
        FollowInput { kind: "live_input".into(), input: input.into(), channel: String::new() }
    }

    #[test]
    fn follow_wires_and_unfollow_cleans_up() {
        let g = blank_patch();
        let filter = g.nodes.iter().find(|n| n.kind == "filter").unwrap().id;
        let f = follow(&g, filter, "cutoff", &live("height")).unwrap();
        let inputs: Vec<_> = f.nodes.iter().filter(|n| n.kind == "live_input").collect();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].params.get("input"), Some(&Param::Choice("height".into())));
        assert_eq!(inputs[0].params.get("low"), Some(&Param::Number(0.0)));
        assert_eq!(inputs[0].params.get("high"), Some(&Param::Number(1.0)));
        let scale = f.nodes.iter().find(|n| n.kind == "scale").unwrap();
        assert_eq!(scale.params.get("scale"), Some(&Param::Number(18_000.0)));
        let pin = knob_pin(&f, filter, "cutoff").unwrap();
        assert!(f.links.contains(&Link { from: (scale.id, 0), to: pin }));
        assert!(crate::compile(&f).is_ok());
        assert_eq!(followed(&f, filter, "cutoff").as_deref(), Some("Height in the frame"));

        let hit = FollowInput { kind: "hit_input".into(), input: "speed".into(), channel: String::new() };
        let again = follow(&f, filter, "cutoff", &hit).unwrap();
        assert_eq!(again.nodes.iter().filter(|n| !crate::inputs_of(&n.kind).is_empty()).count(), 1);
        assert_eq!(followed(&again, filter, "cutoff").as_deref(), Some("Hit speed"));

        let back = unfollow(&f, filter, "cutoff");
        assert_eq!(back.nodes, g.nodes);
        assert_eq!(back.links, g.links);
        assert!(follow(&g, filter, "mode", &live("height")).is_none());
        assert_eq!(followed(&g, filter, "cutoff"), None);
    }

    #[test]
    fn channel_level_names_its_channel() {
        let g = blank_patch();
        let amp = g.nodes.iter().find(|n| n.kind == "amp").unwrap().id;
        let with = FollowInput { kind: "scene_input".into(), input: "channel".into(), channel: "Kick".into() };
        let f = follow(&g, amp, "gain", &with).unwrap();
        assert_eq!(followed(&f, amp, "gain").as_deref(), Some("Channel level (Kick)"));
        assert_eq!(crate::uses(&f).channels, ["Kick".to_string()].into());
    }
}
