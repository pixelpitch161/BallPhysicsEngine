//! Graphs in the kit: which slots run on the CPU, how a module's template graph is bound to its
//! settings and told apart from an edited one, which modules fail to compile, and the world as
//! a running graph sees it.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{
    Broken, Color, Depth, Module, ModuleGraph, ModuleId, ModuleKind, Params, Registry, Resolved, Scene, SettingDef,
    SlotId, Target, ThingId, Value, ValueType, resolve_with,
};
use bpe_graph::{Graph, Param};
use bpe_runtime::{Action, Event, EventKind, Party, World};
use bpe_vm::{Act, Ball, BallInfo, Happening, Host, Program, Role, Trigger, is_setting};
use glam::Vec2;

/// The role a slot's graphs run as, if they run on the CPU.
pub fn role_of(slot: SlotId) -> Option<Role> {
    Role::of_slot(slot)
}

/// The name of the setting block's value parameter.
fn value_param(kind: &str) -> &'static str {
    match kind {
        "setting.color" => "color",
        "setting.bool" => "on",
        _ => "value",
    }
}

fn key_of(node: &bpe_graph::Node) -> Option<&str> {
    is_setting(&node.kind).then(|| node.params.get("key").and_then(Param::text)).flatten().filter(|k| !k.is_empty())
}

fn to_param(v: &Value) -> Option<Param> {
    match v {
        Value::Number(x) => Some(Param::Number(*x as f32)),
        Value::Int(x) => Some(Param::Number(*x as f32)),
        Value::Bool(b) => Some(Param::Bool(*b)),
        Value::Color(c) => Some(Param::Color(c.to_linear())),
        _ => None,
    }
}

fn to_value(p: &Param, ty: &ValueType) -> Option<Value> {
    match (p, ty) {
        (Param::Number(x), ValueType::Number) => Some(Value::Number(*x as f64)),
        (Param::Number(x), ValueType::Int) => Some(Value::Int(x.round() as i64)),
        (Param::Bool(b), ValueType::Bool) => Some(Value::Bool(*b)),
        (Param::Color(c), ValueType::Color) => Some(Value::Color(Color::from_linear(*c))),
        _ => None,
    }
}

/// A copy of a template with every setting block's value taken from `params`, where the block's
/// key names one.
pub fn bind(template: &Graph, params: &Params) -> Graph {
    let mut g = template.clone();
    for node in &mut g.nodes {
        if let Some(key) = key_of(node)
            && let Some(p) = params.get(key).and_then(to_param)
        {
            node.params.insert(value_param(&node.kind).into(), p);
        }
    }
    g
}

/// The setting keys a template's setting blocks carry.
pub fn bound_keys(template: &Graph) -> BTreeSet<String> {
    template.nodes.iter().filter_map(|n| key_of(n).map(str::to_owned)).collect()
}

/// The setting blocks' values as params, for keys that are settings in `defs`, clamped.
pub fn settings_from_graph(graph: &Graph, defs: &[SettingDef]) -> Params {
    let mut out = Params::default();
    for node in &graph.nodes {
        let Some(key) = key_of(node) else { continue };
        let Some(def) = defs.iter().find(|d| d.key == key) else { continue };
        if let Some(p) = node.params.get(value_param(&node.kind))
            && let Some(v) = to_value(p, &def.ty)
        {
            out.set(key, def.clamp(v));
        }
    }
    out
}

/// A graph with everything that doesn't change what it does stripped out.
fn shape(graph: &Graph) -> Graph {
    let mut g = Graph {
        name: String::new(),
        nodes: graph.nodes.clone(),
        links: graph.links.clone(),
        notes: Vec::new(),
        enabled: graph.enabled,
    };
    for n in &mut g.nodes {
        n.pos = [0.0; 2];
    }
    g.nodes.sort_by_key(|n| n.id);
    g.links.sort();
    g
}

/// Whether a graph differs from its bound template in anything but layout, notes and name.
pub fn is_custom(graph: &Graph, bound_template: &Graph) -> bool {
    shape(graph) != shape(bound_template)
}

/// The graph a module opens as: its own when it has been edited, else the kind's template bound
/// to its settings. `None` when the kind has no template.
pub fn module_graph(kind: &dyn ModuleKind, module: &Module) -> Option<ModuleGraph> {
    if let Some(g) = &module.graph
        && g.custom
    {
        return Some(g.clone());
    }
    let template = kind.template()?;
    let mut params = Params::default();
    for d in kind.settings() {
        params.set(d.key, d.default.clone());
    }
    params.apply(&module.params);
    let graph = bind(&template, &params);
    let simple = graph
        .nodes
        .iter()
        .filter_map(|n| {
            let key = key_of(n)?;
            let def = kind.settings().iter().find(|d| d.key == key)?;
            (def.depth == Depth::Simple).then(|| (n.id, value_param(&n.kind).to_owned()))
        })
        .collect();
    Some(ModuleGraph { graph, template: kind.id().to_owned(), custom: false, simple })
}

/// Compiles a module's graph for its slot. The error is one plain sentence, naming the block
/// where there is one.
pub fn compile_module(slot: SlotId, graph: &Graph) -> Result<Program, String> {
    let Some(role) = role_of(slot) else { return Err(format!("{} graphs come in a later stage.", slot.name())) };
    bpe_vm::compile(graph, role).map_err(|errors| {
        let first = errors.first().expect("an error list has an error");
        match first.node.and_then(|id| graph.node(id)).and_then(|n| {
            use bpe_graph::Catalog;
            bpe_vm::catalog(role).spec(&n.kind).map(|s| s.title.clone())
        }) {
            Some(title) => format!("{title}: {}", first.message),
            None => first.message.clone(),
        }
    })
}

/// Every module whose graph would run but doesn't compile: edited modules, and Rules modules.
pub fn graph_problems(scene: &Scene) -> Broken {
    let mut out = BTreeMap::new();
    for (id, m) in &scene.library.modules {
        let Some(g) = &m.graph else { continue };
        if !(g.custom || m.slot == SlotId::Rules) {
            continue;
        }
        if let Err(message) = compile_module(m.slot, &g.graph) {
            out.insert(*id, message);
        }
    }
    out
}

/// Resolves a scene, skipping modules whose graph doesn't compile.
pub fn resolve_scene(scene: &Scene, reg: &Registry) -> Resolved {
    resolve_with(scene, reg, &graph_problems(scene))
}

/// A graph's view of a world, for the rules on `me`.
pub struct WorldHost<'a> {
    pub world: &'a World,
    pub me: ThingId,
}

impl Host for WorldHost<'_> {
    fn seed(&self) -> u64 {
        self.world.seed
    }

    fn gravity(&self) -> Vec2 {
        self.world.gravity
    }

    fn ball(&self, ball: Ball) -> Option<BallInfo> {
        let bodies = self.world.bodies.get(&ball.thing)?;
        let i = ball.item as usize;
        (i < bodies.len() && bodies.alive[i]).then(|| BallInfo {
            position: bodies.position[i],
            velocity: bodies.velocity[i],
            radius: bodies.radius[i],
            hits: bodies.hits[i],
        })
    }

    fn matches(&self, ball: Ball, target: &Target) -> bool {
        self.world.matches(self.me, ball.thing, target)
    }

    fn balls(&self, target: &Target) -> Vec<Ball> {
        let mut out = Vec::new();
        for (thing, bodies) in &self.world.bodies {
            if !self.world.matches(self.me, *thing, target) {
                continue;
            }
            out.extend((0..bodies.len()).filter(|&i| bodies.alive[i]).map(|i| Ball { thing: *thing, item: i as u32 }));
        }
        out
    }

    /// Things with balls first, so "spawn into me" on a root rule adds to the balls.
    fn things_of(&self, target: &Target) -> Vec<ThingId> {
        let all = self.world.things_of(self.me, target);
        let (with, without): (Vec<ThingId>, Vec<ThingId>) =
            all.into_iter().partition(|t| self.world.bodies.contains_key(t));
        with.into_iter().chain(without).collect()
    }
}

fn ball(p: Party) -> Ball {
    Ball { thing: p.thing, item: p.item }
}

fn party(b: Ball) -> Party {
    Party { thing: b.thing, item: b.item }
}

/// An event as a rules graph sees it.
pub fn happening(e: &Event) -> Happening {
    let kind = match &e.kind {
        EventKind::Hit => Trigger::Hit,
        EventKind::Spawn => Trigger::Spawn,
        EventKind::Remove => Trigger::Remove,
        EventKind::Enter => Trigger::Enter,
        EventKind::Leave => Trigger::Leave,
        EventKind::Beat => Trigger::Beat,
        EventKind::Note => Trigger::Note,
        EventKind::Custom(name) => Trigger::Custom(name.clone()),
    };
    Happening { kind, a: ball(e.a), b: e.b.map(ball), point: e.point, normal: e.normal, speed: e.speed }
}

/// What a rule asked for, as the runtime applies it. `point` is where the rule fired, for events.
pub fn action(act: Act, point: Vec2) -> Action {
    match act {
        Act::Grow { ball, by } => Action::Grow { ball: party(ball), by },
        Act::Speed { ball, factor } => Action::Speed { ball: party(ball), factor },
        Act::Kick { ball, velocity } => Action::Kick { ball: party(ball), velocity },
        Act::Tint { ball, color } => Action::Tint { ball: party(ball), color },
        Act::Remove { ball } => Action::Remove { ball: party(ball) },
        Act::Split { ball, pieces } => Action::Split { ball: party(ball), pieces },
        Act::Spawn { thing, position, velocity, radius, color } => {
            Action::Spawn { thing, position, velocity, radius, color }
        }
        Act::Gravity(g) => Action::Gravity(g),
        Act::Set { thing, slot, key, value } => Action::Set { thing, slot, key, value: Value::Number(value as f64) },
        Act::Fire { name, ball } => Action::Fire { name, ball: party(ball), point },
        Act::Note { ball, loudness } => Action::Note { ball: party(ball), loudness },
    }
}

/// The modules in a library by id, for tests and the editor.
pub fn module_ids(scene: &Scene) -> Vec<ModuleId> {
    scene.library.modules.keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kit, new_scene, registry};
    use bpe_core::{Color, ModuleGraph, ModuleId, ProblemKind, SettingDef, Settings, SlotId, ThingId, Value};
    use bpe_graph::{Catalog, Graph, Link, Param};
    use bpe_runtime::{Event, EventKind, Party, World};
    use bpe_vm::{Act, Ball, Host, Role, catalog};
    use glam::Vec2;

    /// A bounce template with three settings: a number, a colour and a switch.
    fn template() -> Graph {
        let c = catalog(Role::Bounce);
        let mut g = Graph::new("t");
        let n = g.add(c.spec("setting").unwrap(), [0.0, 0.0]);
        g.node_mut(n).unwrap().params.insert("key".into(), Param::Text("bounciness".into()));
        g.node_mut(n).unwrap().params.insert("value".into(), Param::Number(1.0));
        let col = g.add(c.spec("setting.color").unwrap(), [0.0, 50.0]);
        g.node_mut(col).unwrap().params.insert("key".into(), Param::Text("color".into()));
        let b = g.add(c.spec("setting.bool").unwrap(), [0.0, 100.0]);
        g.node_mut(b).unwrap().params.insert("key".into(), Param::Text("takes_over".into()));
        let o = g.add(c.spec("bounce.out").unwrap(), [200.0, 0.0]);
        g.connect(Link { from: (b, 0), to: (o, 2) }, c);
        g
    }

    struct TemplateKind;
    impl bpe_core::ModuleKind for TemplateKind {
        fn id(&self) -> &'static str {
            "bounce.templated"
        }
        fn name(&self) -> &'static str {
            "Templated"
        }
        fn help(&self) -> &'static str {
            ""
        }
        fn slot(&self) -> SlotId {
            SlotId::Bounce
        }
        fn settings(&self) -> &'static [SettingDef] {
            crate::BasicBounce::defs()
        }
        fn template(&self) -> Option<Graph> {
            Some(template())
        }
    }

    #[test]
    fn binding_writes_params_into_setting_blocks_and_reads_them_back() {
        let mut params = bpe_core::Params::default();
        params.set("bounciness", Value::Number(0.5));
        params.set("color", Value::Color(Color::rgb(255, 0, 0)));
        params.set("takes_over", Value::Bool(true));
        params.set("unrelated", Value::Number(9.0));
        let bound = bind(&template(), &params);
        assert_eq!(bound.node(1).unwrap().params["value"], Param::Number(0.5));
        assert_eq!(bound.node(2).unwrap().params["color"], Param::Color(Color::rgb(255, 0, 0).to_linear()));
        assert_eq!(bound.node(3).unwrap().params["on"], Param::Bool(true));
        assert_eq!(
            bound_keys(&template()),
            ["bounciness", "color", "takes_over"].map(String::from).into_iter().collect()
        );
        let back = settings_from_graph(&bound, crate::BasicBounce::defs());
        assert_eq!(back.get("bounciness"), Some(&Value::Number(0.5)));
        assert_eq!(back.get("takes_over"), Some(&Value::Bool(true)));
        assert_eq!(back.get("color"), None, "not a Basic bounce setting");
        assert_eq!(back.get("unrelated"), None);
    }

    #[test]
    fn moving_a_block_is_not_custom_but_rewiring_is() {
        let t = template();
        let mut moved = t.clone();
        moved.node_mut(1).unwrap().pos = [500.0, 500.0];
        moved.notes.push(bpe_graph::Note { text: "hi".into(), ..Default::default() });
        moved.name = "renamed".into();
        assert!(!is_custom(&moved, &t));
        let mut rewired = t.clone();
        rewired.links.clear();
        assert!(is_custom(&rewired, &t));
        let mut changed = t.clone();
        changed.node_mut(1).unwrap().params.insert("value".into(), Param::Number(0.2));
        assert!(is_custom(&changed, &t));
        let mut exposed = t.clone();
        exposed.node_mut(1).unwrap().exposed.insert("min".into());
        assert!(is_custom(&exposed, &t));
    }

    #[test]
    fn a_module_graph_is_the_bound_template_until_edited() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let m = scene.add_module(&reg, "bounce.basic", "B").unwrap();
        scene.library.modules.get_mut(&m).unwrap().kind = "bounce.templated".into();
        scene.library.modules.get_mut(&m).unwrap().params.set("bounciness", Value::Number(0.25));
        let module = &scene.library.modules[&m];
        let g = module_graph(&TemplateKind, module).unwrap();
        assert!(!g.custom);
        assert_eq!(g.template, "bounce.templated");
        assert_eq!(g.graph.node(1).unwrap().params["value"], Param::Number(0.25));
        assert_eq!(g.simple, [(1, "value".to_string())].into_iter().collect(), "bounciness is a Simple setting");
        let mut edited = g.clone();
        edited.custom = true;
        edited.graph.links.clear();
        scene.library.modules.get_mut(&m).unwrap().graph = Some(edited.clone());
        assert_eq!(module_graph(&TemplateKind, &scene.library.modules[&m]), Some(edited));
        assert_eq!(module_graph(&crate::ClassicLookKind, &scene.library.modules[&ModuleId(0)]), None);
    }

    #[test]
    fn a_scene_with_a_broken_custom_graph_resolves_around_it() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let bounce = ModuleId(2);
        assert_eq!(scene.library.modules[&bounce].kind, "bounce.basic");
        let c = catalog(Role::Bounce);
        let mut g = Graph::new("b");
        g.add(c.spec("bounce.out").unwrap(), [0.0, 0.0]);
        g.add(c.spec("bounce.out").unwrap(), [0.0, 50.0]);
        scene.library.modules.get_mut(&bounce).unwrap().graph =
            Some(ModuleGraph { graph: g, template: "bounce.basic".into(), custom: false, simple: Default::default() });
        assert!(graph_problems(&scene).is_empty(), "an unedited graph is never checked");
        scene.library.modules.get_mut(&bounce).unwrap().graph.as_mut().unwrap().custom = true;
        let problems = graph_problems(&scene);
        assert!(problems[&bounce].contains("Only one"), "{problems:?}");
        let resolved = resolve_scene(&scene, &reg);
        assert!(resolved.problems.iter().any(|p| p.thing == ThingId::ROOT
            && p.slot == Some(SlotId::Bounce)
            && matches!(&p.kind, ProblemKind::BrokenGraph(m) if m.contains("Only one"))));
        assert!(resolved.things[&ThingId(2)].slots.get(&SlotId::Bounce).is_none());
        assert_eq!(compile_module(SlotId::Look, &Graph::new("x")).unwrap_err(), "Look graphs come in a later stage.");
    }

    #[test]
    fn the_world_host_answers_from_the_world() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ThingId(2)).unwrap().tags.insert("red".into());
        let resolved = resolve_scene(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let host = WorldHost { world: &world, me: ThingId(2) };
        let ball = Ball { thing: ThingId(2), item: 0 };
        assert_eq!(host.ball(ball).map(|b| b.radius), Some(24.0));
        assert_eq!(host.ball(Ball { thing: ThingId(2), item: 7 }), None);
        assert!(host.matches(ball, &bpe_core::Target::Tag("red".into())));
        assert!(!host.matches(Ball { thing: ThingId(1), item: 0 }, &bpe_core::Target::Me));
        assert_eq!(host.balls(&bpe_core::Target::Anything), vec![ball]);
        assert_eq!(host.things_of(&bpe_core::Target::Me), vec![ThingId(2)]);
        assert_eq!(host.things_of(&bpe_core::Target::Kind("arena".into())), vec![ThingId(1)]);
        assert_eq!(host.seed(), world.seed);
        let e = Event {
            step: 3,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(2), item: 0 },
            b: Some(Party { thing: ThingId(1), item: 0 }),
            point: Vec2::new(1.0, 2.0),
            normal: Vec2::Y,
            speed: 50.0,
        };
        let h = happening(&e);
        assert_eq!((h.kind, h.a, h.b.map(|b| b.thing), h.speed), (bpe_vm::Trigger::Hit, ball, Some(ThingId(1)), 50.0));
        assert_eq!(
            action(Act::Grow { ball, by: 2.0 }, Vec2::ZERO),
            bpe_runtime::Action::Grow { ball: Party { thing: ThingId(2), item: 0 }, by: 2.0 }
        );
        assert_eq!(
            action(Act::Fire { name: "x".into(), ball }, Vec2::new(5.0, 6.0)),
            bpe_runtime::Action::Fire {
                name: "x".into(),
                ball: Party { thing: ThingId(2), item: 0 },
                point: Vec2::new(5.0, 6.0)
            }
        );
    }
}
