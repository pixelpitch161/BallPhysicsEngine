//! Working out every thing's effective settings: its kind's defaults with its own changes on
//! top, for the thing itself and for each of its own cards. Nothing is passed down the tree.

use crate::{KindId, LayerId, Params, Registry, Scene, SettingDef, SlotId, Target, ThingId, Value, layered_key};
use std::collections::{BTreeMap, BTreeSet};

/// One shown layer of a card, of a known kind.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedLayer {
    pub id: LayerId,
    /// The layer kind's plugin id.
    pub kind: KindId,
    pub name: String,
    /// For a mask, the layer it limits.
    pub parent: Option<LayerId>,
}

/// One of a thing's cards, with every setting's value.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSlot {
    /// The card kind's plugin id.
    pub kind: KindId,
    /// Every setting the card kind has, then every setting of each of `layers` under its
    /// layered key.
    pub params: Params,
    /// The card's shown layers of known kinds, bottom first, then its reactions.
    pub layers: Vec<ResolvedLayer>,
}

impl ResolvedSlot {
    /// Layer `id`'s settings under their plain keys.
    pub fn layer_params(&self, id: LayerId) -> Params {
        let prefix = format!("{id}/");
        Params(
            self.params
                .iter()
                .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|plain| (plain.to_owned(), v.clone())))
                .collect(),
        )
    }

    /// The first shown layer of `kind`.
    pub fn first_layer(&self, kind: &str) -> Option<&ResolvedLayer> {
        self.layers.iter().find(|l| l.kind == kind)
    }
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
    /// A layer of a kind this version doesn't know, or of a kind made for another card.
    UnknownLayerKind(KindId),
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
                    let mut layers = Vec::new();
                    for layer in &card.layers {
                        let Some(lkind) = reg.layer(&layer.kind).filter(|k| k.card() == card.kind) else {
                            out.problems.push(Problem {
                                thing: id,
                                slot: Some(slot),
                                kind: ProblemKind::UnknownLayerKind(layer.kind.clone()),
                            });
                            continue;
                        };
                        if !layer.enabled {
                            continue;
                        }
                        let get = |k: &str| card.params.get(&layered_key(layer.id, k)).cloned();
                        let with_defaults = |k: &str| {
                            get(k).or_else(|| lkind.settings().iter().find(|d| d.key == k).map(|d| d.default.clone()))
                        };
                        for d in lkind.settings().iter().chain(lkind.dynamic_defs(&with_defaults)) {
                            let key = layered_key(layer.id, d.key);
                            let v = card.params.get(&key).unwrap_or(&d.default).clone();
                            params.set(&key, v);
                        }
                        layers.push(ResolvedLayer {
                            id: layer.id,
                            kind: layer.kind.clone(),
                            name: layer.name.clone(),
                            parent: layer.parent,
                        });
                    }
                    for (k, v) in card.params.iter().filter(|(k, _)| crate::split_layered(k).is_none()) {
                        params.set(k, v.clone());
                    }
                    resolved.slots.insert(slot, ResolvedSlot { kind: card.kind.clone(), params, layers });
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
    fn shown_layers_resolve_their_defaults_under_layered_keys() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        let ring = s.add_layer(&r, b, SlotId::Look, "layer.ring").unwrap();
        let hidden = s.add_layer(&r, b, SlotId::Look, "layer.ring").unwrap();
        s.set_layer_enabled(&r, b, SlotId::Look, hidden, false).unwrap();
        let card = s.card_mut(b, SlotId::Look).unwrap();
        card.params.set("1/width", Value::Number(9.0));
        card.layers.push(crate::Layer { id: LayerId(5), kind: "layer.hologram".into(), ..Default::default() });
        card.params.set("5/width", Value::Number(2.0));
        let res = resolve(&s, &r);
        let look = &res.things[&b].slots[&SlotId::Look];
        let ids: Vec<LayerId> = look.layers.iter().map(|l| l.id).collect();
        assert_eq!(ids, [LayerId(0), ring], "the hidden and the unknown layers are left out");
        assert_eq!(look.params.get("0/size"), Some(&Value::Number(1.0)));
        assert_eq!(look.params.get("1/width"), Some(&Value::Number(9.0)));
        assert_eq!(look.params.get("2/width"), None, "a hidden layer writes nothing");
        assert_eq!(look.layer_params(ring).get("width"), Some(&Value::Number(9.0)));
        assert_eq!(look.layer_params(ring).0.len(), 1);
        assert_eq!(look.first_layer("layer.ring").map(|l| l.name.as_str()), Some("Ring"));
        assert_eq!(
            res.problems,
            vec![Problem {
                thing: b,
                slot: Some(SlotId::Look),
                kind: ProblemKind::UnknownLayerKind("layer.hologram".into())
            }]
        );
    }

    #[test]
    fn a_keyframe_on_a_layer_setting_moves_it() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        let ring = s.add_layer(&r, b, SlotId::Look, "layer.ring").unwrap();
        let width = crate::Address::on(b, SlotId::Look, &layered_key(ring, "width"));
        let first = s.set_key(&r, &width, 0.0, Value::Number(0.0)).unwrap();
        s.set_key(&r, &width, 2.0, Value::Number(20.0)).unwrap();
        s.set_ease(&width, first, crate::Ease::Linear);
        let mut res = resolve(&s, &r);
        crate::Animation::new(&s, &res).apply(&mut res, 1.0, &crate::Inputs::default());
        let Some(Value::Number(w)) = res.things[&b].slots[&SlotId::Look].params.get("1/width").cloned() else {
            panic!()
        };
        assert!((w - 10.0).abs() < 1e-6, "{w}");
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
