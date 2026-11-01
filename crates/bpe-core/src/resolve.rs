//! Working out every thing's effective settings: its own changes, then its linked module, then
//! whatever the folders above it use. The scene settings' own slots are never passed down.

use crate::{KindId, Module, ModuleId, Params, Registry, Scene, SettingDef, SlotId, SlotLink, Target, ThingId, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The module that fills one slot, with every setting's value.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSlot {
    /// The module kind's plugin id.
    pub kind: KindId,
    pub module: ModuleId,
    /// The thing whose link supplied the module.
    pub from: ThingId,
    /// Every setting the module kind has.
    pub params: Params,
}

/// One thing with every setting worked out.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedThing {
    pub id: ThingId,
    pub kind: KindId,
    /// Every setting the thing kind has.
    pub params: Params,
    /// Only the slots that something fills.
    pub slots: BTreeMap<SlotId, ResolvedSlot>,
    /// Drawn on the screen rather than in the world, whatever the camera does.
    pub screen: bool,
}

/// Something in the scene that couldn't be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    UnknownKind(KindId),
    MissingModule(ModuleId),
    /// A module linked into a slot it isn't made for.
    WrongSlot(ModuleId),
    UnknownModuleKind(KindId),
    /// A setting points at a thing that isn't in the scene.
    MissingTarget(ThingId),
    /// The module's graph doesn't compile; the message says why. The link is skipped.
    BrokenGraph(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub thing: ThingId,
    pub slot: Option<SlotId>,
    pub kind: ProblemKind,
}

/// The whole scene worked out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    /// Every enabled thing of a known kind.
    pub things: BTreeMap<ThingId, ResolvedThing>,
    /// In thing order, then slot order.
    pub problems: Vec<Problem>,
}

/// A link that fills a slot: the thing that owns it, the module and the thing's changes to it.
#[derive(Clone, Copy)]
pub(crate) struct Source<'a> {
    pub from: ThingId,
    pub module: ModuleId,
    pub kind: &'a Module,
    pub overrides: &'a Params,
}

/// What one thing's link says about one slot.
enum Step<'a> {
    /// Stop here: nothing fills the slot.
    Off,
    /// This link fills it.
    Use(Source<'a>),
    /// The link can't be used; look further up.
    Bad(ProblemKind),
    /// Nothing here; look further up.
    Pass,
}

/// Modules whose graph doesn't compile, with the compiler's message; `resolve_with` skips their
/// links.
pub type Broken = BTreeMap<ModuleId, String>;

/// Links in slots the thing's kind doesn't offer are ignored, like `link` refuses to make them.
fn step<'a>(scene: &'a Scene, reg: &Registry, id: ThingId, slot: SlotId, broken: &Broken) -> Step<'a> {
    let Some(thing) = scene.things.get(&id) else { return Step::Pass };
    if reg.thing(&thing.kind).is_none_or(|k| !k.slots().contains(&slot)) {
        return Step::Pass;
    }
    match thing.slots.get(&slot) {
        Some(SlotLink::Off) => Step::Off,
        Some(SlotLink::Linked { module, overrides }) => match scene.library.modules.get(module) {
            None => Step::Bad(ProblemKind::MissingModule(*module)),
            Some(_) if broken.contains_key(module) => Step::Bad(ProblemKind::BrokenGraph(broken[module].clone())),
            Some(m) if m.slot != slot => Step::Bad(ProblemKind::WrongSlot(*module)),
            Some(m) if reg.module(&m.kind).is_none() => Step::Bad(ProblemKind::UnknownModuleKind(m.kind.clone())),
            Some(m) => Step::Use(Source { from: id, module: *module, kind: m, overrides }),
        },
        Some(SlotLink::Inherit) | None => Step::Pass,
    }
}

/// Every def's default.
pub(crate) fn defaults(defs: &[SettingDef]) -> Params {
    Params(defs.iter().map(|d| (d.key.to_owned(), d.default.clone())).collect())
}

fn build(reg: &Registry, source: Source<'_>) -> ResolvedSlot {
    let kind = reg.module(&source.kind.kind).expect("step checks the module kind");
    let mut params = defaults(kind.settings());
    params.apply(&source.kind.params);
    params.apply(source.overrides);
    ResolvedSlot { kind: source.kind.kind.clone(), module: source.module, from: source.from, params }
}

/// The link that fills one slot of one thing, walking up the tree. Problems with links on the way
/// are added to `problems`.
pub(crate) fn find_slot<'a>(
    scene: &'a Scene,
    reg: &Registry,
    id: ThingId,
    slot: SlotId,
    problems: &mut Vec<Problem>,
) -> Option<Source<'a>> {
    // An ancestor whose kind keeps its slots to itself is skipped; the thing itself never is.
    let passes = |at: &ThingId| scene.things.get(at).and_then(|t| reg.thing(&t.kind)).is_none_or(|k| k.passes_down());
    for at in std::iter::once(id).chain(scene.ancestors(id).into_iter().filter(passes)) {
        match step(scene, reg, at, slot, &Broken::new()) {
            Step::Off => return None,
            Step::Use(source) => return Some(source),
            Step::Bad(kind) => problems.push(Problem { thing: at, slot: Some(slot), kind }),
            Step::Pass => {}
        }
    }
    None
}

/// One slot of one thing with every setting worked out, walking up the tree. Problems with links
/// on the way are added to `problems`, nearest first. `resolve` does this for every thing at once.
pub fn resolve_slot(
    scene: &Scene,
    reg: &Registry,
    id: ThingId,
    slot: SlotId,
    problems: &mut Vec<Problem>,
) -> Option<ResolvedSlot> {
    find_slot(scene, reg, id, slot, problems).map(|source| build(reg, source))
}

/// Works out every enabled thing's settings and lists what couldn't be used.
pub fn resolve(scene: &Scene, reg: &Registry) -> Resolved {
    resolve_with(scene, reg, &Broken::new())
}

/// Like [`resolve`], treating the `broken` modules (whose graphs don't compile) as unusable:
/// a link to one is reported and the slot falls back to the parent's.
pub fn resolve_with(scene: &Scene, reg: &Registry, broken: &Broken) -> Resolved {
    let mut out = Resolved::default();
    let mut left_out = BTreeSet::new();
    // For each thing and slot, the thing whose link fills it for the thing's children: its own
    // link when its kind passes slots down, else whatever it was handed itself. Parents come
    // before children in `order`, so each thing only looks one step up.
    let mut source: BTreeMap<ThingId, [Option<ThingId>; 8]> = BTreeMap::new();

    for &id in &scene.order {
        let Some(thing) = scene.things.get(&id) else { continue };
        if thing.parent.is_some_and(|p| left_out.contains(&p)) || !thing.enabled {
            left_out.insert(id);
            continue;
        }
        let Some(kind) = reg.thing(&thing.kind) else {
            out.problems.push(Problem { thing: id, slot: None, kind: ProblemKind::UnknownKind(thing.kind.clone()) });
            left_out.insert(id);
            continue;
        };

        let above = thing.parent.and_then(|p| source.get(&p)).copied().unwrap_or([None; 8]);
        let mut here = [None; 8];
        for slot in SlotId::ALL {
            let i = slot as usize;
            here[i] = match step(scene, reg, id, slot, broken) {
                Step::Off => None,
                Step::Use(..) => Some(id),
                Step::Bad(kind) => {
                    out.problems.push(Problem { thing: id, slot: Some(slot), kind });
                    above[i]
                }
                Step::Pass => above[i],
            };
        }
        source.insert(id, if kind.passes_down() { here } else { above });

        let mut params = defaults(kind.settings());
        params.apply(&thing.params);
        let screen = thing.sticks_to_screen(kind);
        let mut resolved = ResolvedThing { id, kind: thing.kind.clone(), params, slots: BTreeMap::new(), screen };
        for &slot in kind.slots() {
            if let Some(from) = here[slot as usize]
                && let Step::Use(source) = step(scene, reg, from, slot, broken)
            {
                resolved.slots.insert(slot, build(reg, source));
            }
        }

        for (slot, params) in
            std::iter::once((None, &resolved.params)).chain(resolved.slots.iter().map(|(s, r)| (Some(*s), &r.params)))
        {
            for v in params.0.values() {
                if let Value::Target(Target::Thing(t)) = v
                    && !scene.things.contains_key(t)
                {
                    out.problems.push(Problem { thing: id, slot, kind: ProblemKind::MissingTarget(*t) });
                }
            }
        }
        out.things.insert(id, resolved);
    }

    // Problems are found while walking the tree, which is thing order already; sort slots within
    // each thing (thing-level problems first).
    let position: BTreeMap<ThingId, usize> = scene.order.iter().enumerate().map(|(i, &t)| (t, i)).collect();
    out.problems.sort_by_key(|p| (position.get(&p.thing).copied(), p.slot));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, testkit};

    #[test]
    fn a_broken_module_falls_back_to_the_parent() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let balls = testkit::push(&mut scene, "balls", ThingId::ROOT);
        let root_look = scene.add_module(&reg, "look.plain", "Root").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Look, root_look).unwrap();
        let own = scene.add_module(&reg, "look.plain", "Own").unwrap();
        scene.link(&reg, balls, SlotId::Look, own).unwrap();
        let broken = BTreeMap::from([(own, "A loop".to_string())]);
        let r = resolve_with(&scene, &reg, &broken);
        assert_eq!(r.things[&balls].slots[&SlotId::Look].module, root_look);
        assert_eq!(r.things[&balls].slots[&SlotId::Look].from, ThingId::ROOT);
        assert_eq!(
            r.problems,
            vec![Problem { thing: balls, slot: Some(SlotId::Look), kind: ProblemKind::BrokenGraph("A loop".into()) }]
        );
        let plain = resolve(&scene, &reg);
        assert!(plain.problems.is_empty());
        assert_eq!(plain.things[&balls].slots[&SlotId::Look].module, own);
    }

    #[test]
    fn a_kind_that_does_not_pass_down_is_skipped_by_children() {
        // ROOT(look A) > sealed(look B) > balls: the balls get A, the sealed thing gets B.
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let sealed = scene.add_thing(&reg, "sealed", ThingId::ROOT).unwrap();
        let balls = scene.add_thing(&reg, "balls", sealed).unwrap();
        let a = scene.add_module(&reg, "look.plain", "A").unwrap();
        let b = scene.add_module(&reg, "look.plain", "B").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Look, a).unwrap();
        scene.link(&reg, sealed, SlotId::Look, b).unwrap();
        let r = resolve(&scene, &reg);
        assert_eq!(r.things[&sealed].slots[&SlotId::Look].module, b);
        assert_eq!(r.things[&balls].slots[&SlotId::Look].module, a);
        let mut problems = Vec::new();
        assert_eq!(resolve_slot(&scene, &reg, balls, SlotId::Look, &mut problems).unwrap().module, a);
        // A sealed thing in the middle with nothing above it: the balls get nothing.
        scene.set_slot_inherit(ThingId::ROOT, SlotId::Look);
        let r = resolve(&scene, &reg);
        assert!(!r.things[&balls].slots.contains_key(&SlotId::Look));
        assert!(resolve_slot(&scene, &reg, balls, SlotId::Look, &mut problems).is_none());
    }

    #[test]
    fn slot_ids_index_their_list() {
        for (i, slot) in SlotId::ALL.into_iter().enumerate() {
            assert_eq!(slot as usize, i);
        }
    }

    #[test]
    fn child_link_beats_parent_and_overrides_win() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.add_slot(&r, f, SlotId::Look).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let dim = s.add_module(&r, "look.plain", "Dim").unwrap();
        let neon = s.add_module(&r, "look.plain", "Neon").unwrap();
        s.library.modules.get_mut(&neon).unwrap().params.set("glow", Value::Number(1.0));
        s.link(&r, f, SlotId::Look, dim).unwrap();
        let res = resolve(&s, &r);
        assert_eq!(res.things[&b].slots[&SlotId::Look].module, dim);
        assert_eq!(res.things[&b].slots[&SlotId::Look].from, f);
        s.link(&r, b, SlotId::Look, neon).unwrap();
        // override on b
        if let Some(SlotLink::Linked { overrides, .. }) = s.things.get_mut(&b).unwrap().slots.get_mut(&SlotId::Look) {
            overrides.set("glow", Value::Number(0.3));
        }
        let look = &resolve(&s, &r).things[&b].slots[&SlotId::Look];
        assert_eq!((look.module, look.params.get("glow")), (neon, Some(&Value::Number(0.3))));
        assert_eq!(look.params.get("color"), Some(&Value::Color(Color::WHITE)));
    }

    #[test]
    fn off_stops_inheritance() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let m = s.add_module(&r, "look.plain", "Plain").unwrap();
        s.link(&r, ThingId::ROOT, SlotId::Look, m).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert!(resolve(&s, &r).things[&b].slots.contains_key(&SlotId::Look));
        s.set_slot_off(b, SlotId::Look);
        assert!(!resolve(&s, &r).things[&b].slots.contains_key(&SlotId::Look));
    }

    #[test]
    fn wrong_slot_module_is_reported_and_skipped() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let plain = s.add_module(&r, "look.plain", "Plain").unwrap();
        let pluck = s.add_module(&r, "sound.pluck", "Pluck").unwrap();
        s.link(&r, ThingId::ROOT, SlotId::Look, plain).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.things
            .get_mut(&b)
            .unwrap()
            .slots
            .insert(SlotId::Look, SlotLink::Linked { module: pluck, overrides: Params::default() });
        let res = resolve(&s, &r);
        assert_eq!(res.things[&b].slots[&SlotId::Look].module, plain);
        assert_eq!(res.problems[0].kind, ProblemKind::WrongSlot(pluck));
    }

    #[test]
    fn missing_and_unknown_modules_are_reported_against_their_owner() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let plain = s.add_module(&r, "look.plain", "Plain").unwrap();
        s.link(&r, ThingId::ROOT, SlotId::Look, plain).unwrap();
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let g = s.add_thing(&r, "folder", f).unwrap();
        let b = s.add_thing(&r, "balls", g).unwrap();
        let gone = ModuleId(99);
        s.things
            .get_mut(&f)
            .unwrap()
            .slots
            .insert(SlotId::Look, SlotLink::Linked { module: gone, overrides: Params::default() });
        let odd = s.add_module(&r, "look.plain", "Odd").unwrap();
        s.library.modules.get_mut(&odd).unwrap().kind = "look.hologram".into();
        s.link(&r, g, SlotId::Look, odd).unwrap();
        let res = resolve(&s, &r);
        assert_eq!(res.things[&b].slots[&SlotId::Look].module, plain);
        assert_eq!(
            res.problems,
            vec![
                Problem { thing: f, slot: Some(SlotId::Look), kind: ProblemKind::MissingModule(gone) },
                Problem {
                    thing: g,
                    slot: Some(SlotId::Look),
                    kind: ProblemKind::UnknownModuleKind("look.hologram".into())
                },
            ]
        );
        // The one-slot walk blames the same owners, meeting the nearest first.
        let mut problems = Vec::new();
        assert_eq!(resolve_slot(&s, &r, b, SlotId::Look, &mut problems).unwrap().from, ThingId::ROOT);
        problems.reverse();
        assert_eq!(problems, res.problems);
    }

    #[test]
    fn links_in_slots_the_kind_lacks_are_ignored() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "look.plain", "Plain").unwrap();
        // A hand edit: balls have no Shader slot, and a Look module can't fill one anyway.
        s.things
            .get_mut(&b)
            .unwrap()
            .slots
            .insert(SlotId::Shader, SlotLink::Linked { module: m, overrides: Params::default() });
        let res = resolve(&s, &r);
        assert!(res.problems.is_empty());
        assert!(!res.things[&b].slots.contains_key(&SlotId::Shader));
    }

    #[test]
    fn removing_a_subtree_reports_targets_into_it() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let inner = s.add_thing(&r, "balls", f).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "sound.pluck", "Pluck").unwrap();
        s.library.modules.get_mut(&m).unwrap().params.set("target", Value::Target(Target::Thing(inner)));
        s.link(&r, b, SlotId::Sound, m).unwrap();
        s.remove_thing(f).unwrap();
        let res = resolve(&s, &r);
        assert!(res.problems.iter().any(|p| p.thing == b && p.kind == ProblemKind::MissingTarget(inner)));
    }

    #[test]
    fn resolve_handles_a_10000_deep_chain() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let m = s.add_module(&r, "look.plain", "Plain").unwrap();
        s.link(&r, ThingId::ROOT, SlotId::Look, m).unwrap();
        let mut parent = ThingId::ROOT;
        for _ in 0..10_000 {
            parent = s.add_thing(&r, "folder", parent).unwrap();
        }
        let leaf = s.add_thing(&r, "balls", parent).unwrap();
        assert_eq!(resolve(&s, &r).things[&leaf].slots[&SlotId::Look].module, m);
        // The single-slot walk used by addresses copes too.
        assert_eq!(resolve_slot(&s, &r, leaf, SlotId::Look, &mut Vec::new()).unwrap().module, m);
    }

    #[test]
    fn disabled_and_unknown_things_are_left_out_with_their_children() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let odd = testkit::push(&mut s, "magnet", ThingId::ROOT);
        s.things.get_mut(&f).unwrap().enabled = false;
        let res = resolve(&s, &r);
        assert!(!res.things.contains_key(&f) && !res.things.contains_key(&b) && !res.things.contains_key(&odd));
        assert_eq!(
            res.problems,
            vec![Problem { thing: odd, slot: None, kind: ProblemKind::UnknownKind("magnet".into()) }]
        );
    }
}
