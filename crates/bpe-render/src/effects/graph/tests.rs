use super::*;
use crate::effects::{EffectInfo, EffectLibrary, templates};

fn info(graph: &Graph) -> EffectInfo {
    EffectInfo::from_graph(graph, false)
}

fn assert_works(graph: &Graph) {
    let info = info(graph);
    assert!(info.compiled.is_ok(), "{}: {:?}\n{}", graph.name, info.graph.as_ref().unwrap().problems, source(graph));
    let thumbs = EffectInfo::graph_thumbnails(graph);
    assert!(thumbs.compiled.is_ok(), "{} thumbnails: {:?}", graph.name, thumbs.compiled.err());
}

fn source(graph: &Graph) -> String {
    generate(graph, false).map(|g| g.source).unwrap_or_default()
}

/// A wire from the block's first output into something that accepts it, ending at Output.
fn sink(b: &mut Builder, id: u32, block: &Block) {
    let out = b.add("output");
    let Some(first) = block.outputs.first() else { return };
    match first.ty {
        Ty::Vector => {
            let pic = b.add("picture");
            b.wire(id, first.name, pic, "at").wire(pic, "out", out, "color");
        }
        _ => {
            b.wire(id, first.name, out, "color");
        }
    }
}

#[test]
fn every_block_compiles_on_its_own() {
    for block in Catalog::get().all() {
        if block.kind == "output" {
            continue;
        }
        let mut b = Builder::new(block.kind);
        let id = b.add(block.kind);
        sink(&mut b, id, block);
        assert_works(&b.finish());
    }
}

#[test]
fn every_generic_block_works_with_each_type() {
    for block in Catalog::get().all().iter().filter(|b| b.outputs.iter().any(|o| o.ty == Ty::Any)) {
        for source in ["number", "position", "colour"] {
            let mut b = Builder::new(&format!("{} {source}", block.kind));
            let src = b.add(source);
            let id = b.add(block.kind);
            let first_any = block.inputs.iter().find(|i| i.ty == Ty::Any).unwrap().name;
            b.wire(src, "out", id, first_any);
            let out = b.add("output");
            if source == "position" {
                let pic = b.add("picture");
                b.wire(id, "out", pic, "at").wire(pic, "out", out, "color");
            } else {
                b.wire(id, "out", out, "color");
            }
            assert_works(&b.finish());
        }
    }
}

#[test]
fn every_input_can_be_a_slider() {
    for block in Catalog::get().all() {
        let numbers: Vec<_> = block.inputs.iter().filter(|i| matches!(i.ty, Ty::Number | Ty::Any)).collect();
        if numbers.is_empty() || block.kind == "output" {
            continue;
        }
        let mut b = Builder::new(block.kind);
        let id = b.add(block.kind);
        for input in &numbers {
            b.expose(id, input.name, None);
        }
        sink(&mut b, id, block);
        let graph = b.finish();
        let info = info(&graph);
        assert!(info.compiled.is_ok(), "{}: {:?}", block.kind, info.compiled.err());
        assert_eq!(info.params.len(), numbers.len(), "{}", block.kind);
        for (param, input) in info.params.iter().zip(&numbers) {
            assert_eq!(param.name, input.name);
            assert_eq!((param.min, param.max), (input.min, input.max));
            let kind = match input.control {
                Control::Slider => crate::effects::ParamKind::Slider,
                Control::Whole => crate::effects::ParamKind::Whole,
                Control::Toggle => crate::effects::ParamKind::Toggle,
            };
            assert_eq!(param.kind, kind, "{}.{}", block.kind, input.name);
        }
    }
}

#[test]
fn an_output_alone_shows_the_picture() {
    let mut b = Builder::new("Plain");
    b.add("output");
    let graph = b.finish();
    assert_works(&graph);
    assert!(source(&graph).contains("return (c0);"));
}

#[test]
fn empty_graphs_explain_what_is_missing() {
    let info = info(&Builder::new("Empty").finish());
    let problems = &info.graph.as_ref().unwrap().problems;
    assert!(info.compiled.is_err());
    assert_eq!(problems.len(), 1);
    assert!(problems[0].node.is_none() && problems[0].message.contains("Output"), "{problems:?}");
}

#[test]
fn problems_point_at_the_block() {
    // A colour into a position.
    let mut b = Builder::new("Bad wire");
    let colour = b.add("colour");
    let pic = b.add("picture");
    let out = b.add("output");
    b.wire(colour, "out", pic, "at").wire(pic, "out", out, "color");
    let problems = generate(&b.finish(), false).unwrap_err();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].node, Some(pic));
    assert!(problems[0].message.starts_with("at:"), "{problems:?}");

    // A position and a colour into one Add.
    let mut b = Builder::new("Mixed");
    let pos = b.add("position");
    let colour = b.add("colour");
    let add = b.add("add");
    let out = b.add("output");
    b.wire(pos, "out", add, "a").wire(colour, "out", add, "b").wire(add, "out", out, "color");
    let problems = generate(&b.finish(), false).unwrap_err();
    assert_eq!(problems.iter().map(|p| p.node).collect::<Vec<_>>(), [Some(add)], "{problems:?}");

    // Unknown blocks and loops.
    let mut b = Builder::new("Loop");
    let a = b.add("add");
    let c = b.add("add");
    let out = b.add("output");
    b.wire(a, "out", c, "a").wire(c, "out", a, "a").wire(c, "out", out, "color");
    let mut graph = b.finish();
    graph.nodes.push(Node { id: 99, kind: "no_such_block".into(), enabled: true, ..Node::default() });
    let problems = generate(&graph, false).unwrap_err();
    let mut nodes: Vec<_> = problems.iter().filter_map(|p| p.node).collect();
    nodes.sort();
    assert_eq!(nodes, [a, c, 99], "{problems:?}");
}

#[test]
fn problems_on_unused_blocks_are_only_warnings() {
    let mut b = Builder::new("Stray");
    let colour = b.add("colour");
    let pic = b.add("picture");
    b.wire(colour, "out", pic, "at");
    b.add("output");
    let graph = b.finish();
    let info = info(&graph);
    assert!(info.compiled.is_ok());
    assert_eq!(info.graph.as_ref().unwrap().problems.len(), 1);
}

#[test]
fn two_outputs_is_a_problem_on_the_second() {
    let mut b = Builder::new("Two");
    b.add("output");
    let second = b.add("output");
    let problems = generate(&b.finish(), false).unwrap_err();
    assert_eq!(problems.iter().map(|p| p.node).collect::<Vec<_>>(), [Some(second)]);
}

#[test]
fn bad_wires_and_values_never_panic() {
    let mut b = Builder::new("Odd");
    let n = b.add("number");
    b.set(n, "value", f32::NAN);
    let out = b.add("output");
    b.wire(n, "out", out, "color");
    let mut graph = b.finish();
    graph.links.push(Link { from: (n, 7), to: (out, 0) });
    graph.links.push(Link { from: (1234, 0), to: (out, 0) });
    graph.links.push(Link { from: (n, 0), to: (out, 9) });
    let problems = generate(&graph, false).unwrap_err();
    assert!(problems.iter().all(|p| p.node == Some(out)), "{problems:?}");
    graph.links.truncate(1);
    assert_works(&graph);
    assert!(!source(&graph).contains("NaN"));
}

#[test]
fn switched_off_blocks_pass_their_input_through() {
    let mut b = Builder::new("Off");
    let pic = b.add("picture");
    let invert = b.add("invert");
    let out = b.add("output");
    b.wire(pic, "out", invert, "color").wire(invert, "out", out, "color");
    let mut graph = b.finish();
    graph.nodes[1].enabled = false;
    assert_works(&graph);
    assert!(source(&graph).contains(&format!("let n{invert}_out = n{pic}_out;")));
}

#[test]
fn slider_names_are_unique_and_limited() {
    let mut b = Builder::new("Sliders");
    let mut last = b.add("number");
    b.expose(last, "value", Some("Glow amount!"));
    for _ in 0..3 {
        let next = b.add("add");
        b.expose(next, "b", None);
        b.wire(last, "out", next, "a");
        last = next;
    }
    let out = b.add("output");
    b.wire(last, "out", out, "color");
    let graph = b.finish();
    let names: Vec<_> = info(&graph).params.into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["glow_amount", "b", "b_2", "b_3"]);

    let mut b = Builder::new("Too many");
    let mut last = b.add("number");
    for _ in 0..MAX_SLIDERS + 1 {
        let next = b.add("add");
        b.expose(next, "b", None);
        b.wire(last, "out", next, "a");
        last = next;
    }
    let out = b.add("output");
    b.wire(last, "out", out, "color");
    let problems = generate(&b.finish(), false).unwrap_err();
    assert!(problems[0].message.contains("at most"), "{problems:?}");
}

#[test]
fn unused_blocks_are_left_out_except_for_thumbnails() {
    let mut b = Builder::new("Unused");
    let noise = b.add("voronoi");
    b.add("output");
    let graph = b.finish();
    assert!(!source(&graph).contains("g_voronoi"));
    let thumbs = generate(&graph, true).unwrap();
    assert!(thumbs.source.contains("g_voronoi"));
    assert_eq!(thumbs.thumbnails, [noise]);
}

#[test]
fn shader_check_errors_map_to_blocks() {
    let mut b = Builder::new("Broken");
    let out = b.add("output");
    let graph = b.finish();
    let generated = generate(&graph, false).unwrap();
    let line = generated.source.lines().position(|l| l.contains("return")).unwrap() + 1;
    assert_eq!(generated.node_at_line(line), Some(out));
    assert_eq!(generated.node_at_line(1), None);
    assert_eq!(generated.node_at_line(0), None);
    assert_eq!(generated.node_at_line(100_000), None);
}

#[test]
fn templates_and_rebuilds_compile() {
    for graph in templates::graphs() {
        assert_works(&graph);
        let info = info(&graph);
        assert!(!info.params.is_empty(), "{} has no sliders", graph.name);
        assert!(info.graph.unwrap().problems.is_empty(), "{}", graph.name);
    }
    let library = EffectLibrary::new();
    for id in templates::REBUILT {
        let graph = templates::rebuild(id).unwrap();
        assert_works(&graph);
        let rebuilt = info(&graph);
        let original = library.builtin(id).unwrap();
        let mut ours: Vec<_> = rebuilt.params.iter().map(|p| (&p.name, p.default, p.min, p.max, p.kind)).collect();
        // "Pulse with music" is left out of the node versions; at 0 it changes nothing.
        let mut theirs: Vec<_> = original.params.iter().filter(|p| p.name != "music").map(|p| (&p.name, p.default, p.min, p.max, p.kind)).collect();
        ours.sort_by(|a, b| a.0.cmp(b.0));
        theirs.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(ours, theirs, "{id} sliders differ from the original");
    }
}

#[test]
fn library_serves_scene_graphs() {
    let library = EffectLibrary::new();
    assert!(library.get("graph:Bass tunnel").is_some_and(|i| i.is_builtin() && i.compiled.is_ok()));
    let mut b = Builder::new("Mine");
    b.add("output");
    let graph = b.finish();
    assert!(library.sync_graphs(std::slice::from_ref(&graph)));
    assert!(!library.sync_graphs(std::slice::from_ref(&graph)));
    let mine = library.get("graph:Mine").unwrap();
    assert!(mine.is_graph() && !mine.is_builtin());
    assert!(library.list().iter().any(|i| i.id == "graph:Mine"));
    // A scene graph with a built-in's name replaces it.
    let mut b = Builder::new("Bass tunnel");
    b.add("output");
    library.sync_graphs(&[b.finish()]);
    assert!(!library.get("graph:Bass tunnel").unwrap().is_builtin());
    assert!(library.get("graph:Mine").is_none());
    assert_eq!(library.list().iter().filter(|i| i.id == "graph:Bass tunnel").count(), 1);
    library.sync_graphs(&[]);
    assert!(library.get("graph:Bass tunnel").unwrap().is_builtin());
}

/// Random graphs, wired any old way, never panic and either work or say why.
#[test]
fn random_graphs_never_panic() {
    let blocks = Catalog::get().all();
    let mut seed = 0x9e37_79b9_u64;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    let mut working = 0;
    for round in 0..300 {
        let mut b = Builder::new(&format!("Random {round}"));
        let mut ids = vec![(b.add("output"), Catalog::get().block("output").unwrap())];
        for _ in 0..1 + next(12) {
            let block = &blocks[next(blocks.len())];
            ids.push((b.add(block.kind), block));
        }
        let mut graph = b.finish();
        for _ in 0..next(20) {
            let (from, from_block) = ids[next(ids.len())];
            let (to, to_block) = ids[next(ids.len())];
            if from_block.outputs.is_empty() || to_block.inputs.is_empty() {
                continue;
            }
            graph.links.push(Link {
                from: (from, next(from_block.outputs.len()) as u8),
                to: (to, next(to_block.inputs.len()) as u8),
            });
        }
        for node in &mut graph.nodes {
            if next(4) == 0 {
                node.enabled = false;
            }
            if next(3) == 0 {
                let block = Catalog::get().block(&node.kind).unwrap();
                if let Some(input) = block.inputs.get(next(block.inputs.len().max(1))) {
                    node.exposed.insert(input.name.into());
                }
            }
        }
        let normal = info(&graph);
        let thumbs = EffectInfo::graph_thumbnails(&graph);
        if normal.compiled.is_ok() {
            working += 1;
            assert!(thumbs.compiled.is_ok(), "{:?}\n{}", thumbs.compiled.err(), source(&graph));
        } else {
            let problems = &normal.graph.as_ref().unwrap().problems;
            assert!(!problems.is_empty());
            // Anything the shader check rejects is a compiler bug, not a user mistake.
            assert!(
                problems.iter().all(|p| !p.message.starts_with("The shader check failed")),
                "{problems:?}\n{}",
                source(&graph)
            );
        }
    }
    assert!(working > 20, "only {working} random graphs worked");
}

#[test]
fn generation_is_fast() {
    // About 40 blocks in a chain.
    let mut b = Builder::new("Big");
    let mut last = b.add("position");
    for i in 0..13 {
        let warp = b.add(["twirl", "ripple", "flow", "kaleidoscope", "mirror"][i % 5]);
        b.wire(last, "out", warp, "at");
        last = warp;
    }
    let pic = b.add("picture");
    b.wire(last, "out", pic, "at");
    let mut colour = pic;
    for i in 0..24 {
        let step = b.add(["hue_shift", "saturation", "vignette", "blend_screen", "brightness_contrast", "grain"][i % 6]);
        let input = if i % 6 == 3 { "base" } else { "color" };
        b.wire(colour, "out", step, input);
        colour = step;
    }
    let out = b.add("output");
    b.wire(colour, "out", out, "color");
    let graph = b.finish();
    assert!(graph.nodes.len() >= 40);
    let start = std::time::Instant::now();
    let info = info(&graph);
    let elapsed = start.elapsed();
    assert!(info.compiled.is_ok(), "{:?}", info.compiled.err());
    println!("40-block graph to checked WGSL: {elapsed:?}");
    assert!(elapsed.as_millis() < 50 || cfg!(debug_assertions), "{elapsed:?}");
}
