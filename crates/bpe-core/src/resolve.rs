//! Working out every thing's effective settings: its kind's defaults with its own changes on
//! top, for the thing itself and for each of its own cards. Nothing is passed down the tree.

use crate::{KindId, Params, Registry, Scene, SettingDef, SlotId, Target, ThingId, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One of a thing's cards, with every setting's value.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSlot {
    /// The card kind's plugin id.
    pub kind: KindId,
    /// Every setting the card kind has.
    pub params: Params,
}

/// One thing with every setting worked out.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedThing {
    pub id: ThingId,
    pub kind: KindId,
    /// Every setting the thing kind has.
    pub params: Params,
    /// Only the slots the thing has a usable card in.
    pub slots: BTreeMap<SlotId, ResolvedSlot>,
    /// Drawn on the screen rather than in the world, whatever the camera does.
    pub screen: bool,
}

/// Something in the scene that couldn't be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    UnknownKind(KindId),
    /// A card of a kind this version doesn't know, or of a kind made for another slot.
    UnknownCardKind(KindId),
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

/// Every def's default.
pub(crate) fn defaults(defs: &[SettingDef]) -> Params {
    Params(defs.iter().map(|d| (d.key.to_owned(), d.default.clone())).collect())
}

/// Works out every enabled thing's settings and lists what couldn't be used. Cards in slots the
/// thing's kind doesn't offer are ignored.
pub fn resolve(scene: &Scene, reg: &Registry) -> Resolved {
    let mut out = Resolved::default();
    let mut left_out = BTreeSet::new();
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
        let mut params = defaults(kind.settings());
        params.apply(&thing.params);
        let screen = thing.sticks_to_screen(kind);
        let mut resolved = ResolvedThing { id, kind: thing.kind.clone(), params, slots: BTreeMap::new(), screen };
        for (&slot, card) in thing.slots.iter().filter(|(s, _)| kind.slots().contains(s)) {
            match reg.module(&card.kind).filter(|k| k.slot() == slot) {
                Some(ckind) => {
                    let mut params = defaults(ckind.settings());
                    params.apply(&card.params);
                    resolved.slots.insert(slot, ResolvedSlot { kind: card.kind.clone(), params });
                }
                None => out.problems.push(Problem {
                    thing: id,
                    slot: Some(slot),
                    kind: ProblemKind::UnknownCardKind(card.kind.clone()),
                }),
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Slot, testkit};

    #[test]
    fn slot_ids_index_their_list() {
        for (i, slot) in SlotId::ALL.into_iter().enumerate() {
            assert_eq!(slot as usize, i);
        }
        assert_eq!(SlotId::ALL[SlotId::Shader as usize], SlotId::Shader);
    }

    #[test]
    fn a_card_starts_from_its_kinds_defaults_with_the_things_changes_on_top() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.plain").unwrap();
        s.things.get_mut(&b).unwrap().slots.get_mut(&SlotId::Look).unwrap().params.set("glow", Value::Number(0.3));
        let look = &resolve(&s, &r).things[&b].slots[&SlotId::Look];
        assert_eq!((look.kind.as_str(), look.params.get("glow")), ("look.plain", Some(&Value::Number(0.3))));
        assert_eq!(look.params.get("color"), Some(&Value::Color(Color::WHITE)));
    }

    #[test]
    fn a_folders_card_is_not_passed_down() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.add_slot(&r, f, SlotId::Look, "look.plain").unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let res = resolve(&s, &r);
        assert!(res.things[&f].slots.contains_key(&SlotId::Look));
        assert!(res.things[&b].slots.is_empty());
    }

    #[test]
    fn unknown_and_wrong_slot_cards_are_reported_and_skipped() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let slots = &mut s.things.get_mut(&b).unwrap().slots;
        slots.insert(SlotId::Look, Slot::new("trail.streak"));
        slots.insert(SlotId::Trail, Slot::new("trail.hologram"));
        // Balls have no Shader slot: a card there is ignored without a word.
        slots.insert(SlotId::Shader, Slot::new("look.plain"));
        let res = resolve(&s, &r);
        assert!(res.things[&b].slots.is_empty());
        assert_eq!(
            res.problems,
            vec![
                Problem {
                    thing: b,
                    slot: Some(SlotId::Look),
                    kind: ProblemKind::UnknownCardKind("trail.streak".into())
                },
                Problem {
                    thing: b,
                    slot: Some(SlotId::Trail),
                    kind: ProblemKind::UnknownCardKind("trail.hologram".into())
                },
            ]
        );
    }

    #[test]
    fn removing_a_subtree_reports_targets_into_it() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let inner = s.add_thing(&r, "balls", f).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Trail, "trail.streak").unwrap();
        let card = s.things.get_mut(&b).unwrap().slots.get_mut(&SlotId::Trail).unwrap();
        card.params.set("target", Value::Target(Target::Thing(inner)));
        s.remove_thing(f).unwrap();
        let res = resolve(&s, &r);
        assert!(res.problems.iter().any(|p| p.thing == b && p.kind == ProblemKind::MissingTarget(inner)));
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
