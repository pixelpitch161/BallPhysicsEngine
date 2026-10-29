//! Working out every thing's effective settings: its own changes, then its linked module, then
//! whatever its parents use, then Scene defaults.

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

/// What one thing's link says about one slot.
enum Step<'a> {
    /// Stop here: nothing fills the slot.
    Off,
    /// This module fills it, with these changes.
    Use(&'a Module, &'a Params),
    /// The link can't be used; look further up.
    Bad(ProblemKind),
    /// Nothing here; look further up.
    Pass,
}

fn step<'a>(scene: &'a Scene, reg: &Registry, id: ThingId, slot: SlotId) -> Step<'a> {
    match scene.things.get(&id).and_then(|t| t.slots.get(&slot)) {
        Some(SlotLink::Off) => Step::Off,
        Some(SlotLink::Linked { module, overrides }) => match scene.library.modules.get(module) {
            None => Step::Bad(ProblemKind::MissingModule(*module)),
            Some(m) if m.slot != slot => Step::Bad(ProblemKind::WrongSlot(*module)),
            Some(m) if reg.module(&m.kind).is_none() => Step::Bad(ProblemKind::UnknownModuleKind(m.kind.clone())),
            Some(m) => Step::Use(m, overrides),
        },
        Some(SlotLink::Inherit) | None => Step::Pass,
    }
}

/// Every def's default.
pub(crate) fn defaults(defs: &[SettingDef]) -> Params {
    Params(defs.iter().map(|d| (d.key.to_owned(), d.default.clone())).collect())
}

fn build(reg: &Registry, module: &Module, overrides: &Params, from: ThingId) -> ResolvedSlot {
    let kind = reg.module(&module.kind).expect("step checks the module kind");
    ResolvedSlot {
        kind: module.kind.clone(),
        module: module.id,
        from,
        params: defaults(kind.settings()).merged(&module.params).merged(overrides),
    }
}

/// One slot of one thing, walking up the tree. Problems with links on the way are added to `problems`.
pub(crate) fn resolve_slot(
    scene: &Scene,
    reg: &Registry,
    id: ThingId,
    slot: SlotId,
    problems: &mut Vec<Problem>,
) -> Option<ResolvedSlot> {
    for at in std::iter::once(id).chain(scene.ancestors(id)) {
        match step(scene, reg, at, slot) {
            Step::Off => return None,
            Step::Use(m, overrides) => return Some(build(reg, m, overrides, at)),
            Step::Bad(kind) => problems.push(Problem { thing: at, slot: Some(slot), kind }),
            Step::Pass => {}
        }
    }
    None
}

/// Works out every enabled thing's settings and lists what couldn't be used.
pub fn resolve(scene: &Scene, reg: &Registry) -> Resolved {
    let mut out = Resolved::default();
    let mut left_out = BTreeSet::new();
    // For each thing and slot, the thing whose link fills it when walking up from here. Parents
    // come before children in `order`, so each thing only looks one step up.
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
        for (i, slot) in SlotId::ALL.into_iter().enumerate() {
            here[i] = match step(scene, reg, id, slot) {
                Step::Off => None,
                Step::Use(..) => Some(id),
                Step::Bad(kind) => {
                    out.problems.push(Problem { thing: id, slot: Some(slot), kind });
                    above[i]
                }
                Step::Pass => above[i],
            };
        }
        source.insert(id, here);

        let mut resolved = ResolvedThing {
            id,
            kind: thing.kind.clone(),
            params: defaults(kind.settings()).merged(&thing.params),
            slots: BTreeMap::new(),
        };
        for &slot in kind.slots() {
            let i = SlotId::ALL.iter().position(|&s| s == slot).expect("every slot is in ALL");
            if let Some(from) = here[i]
                && let Step::Use(m, overrides) = step(scene, reg, from, slot)
            {
                resolved.slots.insert(slot, build(reg, m, overrides, from));
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
