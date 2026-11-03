//! Layers: a card kind can hold a stack of layers (the balls' Look), each of its own kind with
//! its own settings. A layer's settings live in its card's `params` under layered keys,
//! `"<id>/<key>"`, so addresses, keyframes, pulses, copy and paste and undo treat them as any
//! other card setting.

use crate::{Address, EditError, KindId, LayerKind, LayerSection, Params, Registry, Scene, Slot, SlotId, ThingId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// One layer within one card. Never reused within the card while it lives (a pasted whole look
/// replaces the list, ids and all).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u32);

impl fmt::Display for LayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn is_true(v: &bool) -> bool {
    *v
}

/// One layer of a card's stack: which kind it is, what it is called and whether it is shown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layer {
    pub id: LayerId,
    /// The layer kind's plugin id, such as `"layer.outline"`.
    pub kind: KindId,
    pub name: String,
    /// Off, the layer draws (or does) nothing; its settings stay.
    #[serde(skip_serializing_if = "is_true")]
    pub enabled: bool,
}

impl Default for Layer {
    fn default() -> Self {
        Self { id: LayerId(0), kind: KindId::new(), name: String::new(), enabled: true }
    }
}

/// The key a layer's setting `key` is stored under in its card: `"3/width"`.
pub fn layered_key(id: LayerId, key: &str) -> String {
    format!("{id}/{key}")
}

/// The layer and the plain key of a layered key; `None` for a plain key.
pub fn split_layered(key: &str) -> Option<(LayerId, &str)> {
    let (id, rest) = key.split_once('/')?;
    if id.is_empty() || rest.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((LayerId(id.parse().ok()?), rest))
}

/// A layer as copied: its kind, name, whether it is shown and the values that differ from its
/// kind's defaults, by plain key. It carries no keyframes or pulses.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedLayer {
    pub kind: KindId,
    pub name: String,
    pub enabled: bool,
    pub params: Params,
}

/// A whole stack as copied, bottom first.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedLook {
    pub layers: Vec<CopiedLayer>,
}

/// Whether `key` is a layered key of layer `id`.
fn of_layer(key: &str, id: LayerId) -> bool {
    split_layered(key).is_some_and(|(l, _)| l == id)
}

/// Whether the layer at `index` of `card` is its base layer, which stays put.
fn is_base(reg: &Registry, card: &Slot, index: usize) -> bool {
    index == 0 && card.layers.first().and_then(|l| reg.layer(&l.kind)).is_some_and(|k| k.base())
}

/// Where the drawn layers end: the index of the first reaction, or the length.
fn drawn_end(reg: &Registry, card: &Slot) -> usize {
    card.layers
        .iter()
        .position(|l| reg.layer(&l.kind).is_some_and(|k| k.section() == LayerSection::Reaction))
        .unwrap_or(card.layers.len())
}

/// `base`, or `base` with the lowest free number from 2 ("Outline 2").
fn free_name(card: &Slot, base: &str) -> String {
    let taken = |n: &str| card.layers.iter().any(|l| l.name == n);
    if !taken(base) {
        return base.to_owned();
    }
    (2u32..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).expect("a free number exists")
}

/// `v` fitted to the layer kind's setting `key`: `None` when the kind has no such setting, the
/// value is of the wrong type, or it is the default.
fn fitted(kind: &dyn LayerKind, key: &str, v: &crate::Value) -> Option<crate::Value> {
    let def = kind.settings().iter().find(|d| d.key == key)?;
    if !def.ty.accepts(v) {
        return None;
    }
    let v = def.clamp(v.clone());
    (v != def.default).then_some(v)
}

impl Scene {
    fn layer_card(&self, thing: ThingId, slot: SlotId) -> Result<&Slot, EditError> {
        self.things.get(&thing).ok_or(EditError::Missing(thing))?.slots.get(&slot).ok_or(EditError::NoSlot(slot))
    }

    fn layer_card_mut(&mut self, thing: ThingId, slot: SlotId) -> &mut Slot {
        self.card_mut(thing, slot).expect("checked by layer_card")
    }

    /// Every keyframe and pulse address on layer `id` of the card.
    fn layer_animation(&self, thing: ThingId, slot: SlotId, id: LayerId) -> Vec<Address> {
        let on = |a: &Address| a.thing == thing && a.slot == Some(slot) && of_layer(&a.key, id);
        self.tracks.keys().chain(self.pulses.keys()).filter(|a| on(a)).cloned().collect()
    }

    /// Adds a layer of `kind`, every setting at its default, and returns its id. A drawn layer goes
    /// on top of the drawn layers (under the reactions), a reaction at the end, and a base layer
    /// at the bottom. A kind that doesn't repeat may be in a card only once.
    pub fn add_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        kind: &str,
    ) -> Result<LayerId, EditError> {
        let card = self.layer_card(thing, slot)?;
        let k = reg.layer(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        if k.card() != card.kind {
            return Err(EditError::NotAccepted);
        }
        if (!k.repeats() || k.base()) && card.layers.iter().any(|l| l.kind == kind) {
            return Err(EditError::OnlyOne(k.name().into()));
        }
        let at = if k.base() {
            0
        } else {
            match k.section() {
                LayerSection::Drawn => drawn_end(reg, card),
                LayerSection::Reaction => card.layers.len(),
            }
        };
        let layer =
            Layer { id: card.next_layer_id(), kind: kind.into(), name: free_name(card, k.name()), enabled: true };
        let id = layer.id;
        self.layer_card_mut(thing, slot).layers.insert(at, layer);
        Ok(id)
    }

    /// Takes layer `id` out of the card with its settings, keyframes and pulses. The base layer
    /// stays.
    pub fn remove_layer(&mut self, reg: &Registry, thing: ThingId, slot: SlotId, id: LayerId) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        if is_base(reg, card, index) {
            return Err(EditError::BaseLayer);
        }
        for a in self.layer_animation(thing, slot, id) {
            self.tracks.remove(&a);
            self.pulses.remove(&a);
        }
        let card = self.layer_card_mut(thing, slot);
        card.layers.remove(index);
        card.params.0.retain(|k, _| !of_layer(k, id));
        Ok(())
    }

    /// Moves drawn layer `id` to `to` in the stack, counting from the bottom. Drawn layers stay
    /// above the base layer and under the reactions; reactions have no order to change.
    pub fn move_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        id: LayerId,
        to: usize,
    ) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        if is_base(reg, card, index) {
            return Err(EditError::BaseLayer);
        }
        let drawn = reg.layer(&card.layers[index].kind).is_some_and(|k| k.section() == LayerSection::Drawn);
        let lowest = usize::from(is_base(reg, card, 0));
        let highest = drawn_end(reg, card).saturating_sub(1);
        if !drawn || to < lowest || to > highest {
            return Err(EditError::NotAccepted);
        }
        let card = self.layer_card_mut(thing, slot);
        let layer = card.layers.remove(index);
        card.layers.insert(to, layer);
        Ok(())
    }

    /// Adds a copy of layer `id` (its settings, keyframes and pulses) just above it, named
    /// "<name> copy", and returns the copy's id. The base layer and kinds that don't repeat
    /// can't be copied.
    pub fn duplicate_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        id: LayerId,
    ) -> Result<LayerId, EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        let layer = card.layers[index].clone();
        let kind = reg.layer(&layer.kind).ok_or_else(|| EditError::UnknownKind(layer.kind.clone()))?;
        if is_base(reg, card, index) || kind.base() {
            return Err(EditError::BaseLayer);
        }
        if !kind.repeats() {
            return Err(EditError::OnlyOne(kind.name().into()));
        }
        let new = card.next_layer_id();
        let rekey = |k: &str| split_layered(k).filter(|(l, _)| *l == id).map(|(_, rest)| crate::layered_key(new, rest));
        let params: Vec<(String, crate::Value)> =
            card.params.iter().filter_map(|(k, v)| Some((rekey(k)?, v.clone()))).collect();
        let name = format!("{} copy", layer.name);
        for a in self.layer_animation(thing, slot, id) {
            let to = Address { key: rekey(&a.key).expect("listed as this layer's"), ..a.clone() };
            if let Some(t) = self.tracks.get(&a).cloned() {
                self.tracks.insert(to.clone(), t);
            }
            if let Some(p) = self.pulses.get(&a).cloned() {
                self.pulses.insert(to, p);
            }
        }
        let card = self.layer_card_mut(thing, slot);
        for (k, v) in params {
            card.params.set(&k, v);
        }
        card.layers.insert(index + 1, Layer { id: new, kind: layer.kind, name, enabled: layer.enabled });
        Ok(new)
    }

    /// Renames layer `id`. A blank name gives it back its kind's name.
    pub fn rename_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        id: LayerId,
        name: &str,
    ) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        let name = name.trim();
        let name = if name.is_empty() {
            reg.layer(&card.layers[index].kind).map_or(card.layers[index].kind.clone(), |k| k.name().to_owned())
        } else {
            name.to_owned()
        };
        self.layer_card_mut(thing, slot).layers[index].name = name;
        Ok(())
    }

    /// Shows or hides layer `id`. The base layer is always shown.
    pub fn set_layer_enabled(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        id: LayerId,
        on: bool,
    ) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        if !on && is_base(reg, card, index) {
            return Err(EditError::BaseLayer);
        }
        self.layer_card_mut(thing, slot).layers[index].enabled = on;
        Ok(())
    }

    /// Layer `id` as copied: its kind, name and settings by plain key.
    pub fn copy_layer(&self, thing: ThingId, slot: SlotId, id: LayerId) -> Option<CopiedLayer> {
        let card = self.card(thing, slot)?;
        let layer = card.layer(id)?;
        let params = Params(
            card.params
                .iter()
                .filter_map(|(k, v)| {
                    split_layered(k).filter(|(l, _)| *l == id).map(|(_, rest)| (rest.to_owned(), v.clone()))
                })
                .collect(),
        );
        Some(CopiedLayer { kind: layer.kind.clone(), name: layer.name.clone(), enabled: layer.enabled, params })
    }

    /// The card's whole stack as copied.
    pub fn copy_look(&self, thing: ThingId, slot: SlotId) -> Option<CopiedLook> {
        let card = self.card(thing, slot)?;
        let layers = card.layers.iter().map(|l| self.copy_layer(thing, slot, l.id)).collect::<Option<Vec<_>>>()?;
        Some(CopiedLook { layers })
    }

    /// Adds a copied layer to the card with its settings and returns its id. Where the card
    /// already has a layer of a kind that may be there only once (the base layer, a reaction),
    /// that layer takes the copied settings instead, keeping its name.
    pub fn paste_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        copied: &CopiedLayer,
    ) -> Result<LayerId, EditError> {
        let card = self.layer_card(thing, slot)?;
        let kind = reg.layer(&copied.kind).ok_or_else(|| EditError::UnknownKind(copied.kind.clone()))?;
        if kind.card() != card.kind {
            return Err(EditError::NotAccepted);
        }
        let single = !kind.repeats() || kind.base();
        let id = match card.layers.iter().find(|l| l.kind == copied.kind).filter(|_| single) {
            Some(existing) => {
                let id = existing.id;
                for a in self.layer_animation(thing, slot, id) {
                    self.tracks.remove(&a);
                    self.pulses.remove(&a);
                }
                self.layer_card_mut(thing, slot).params.0.retain(|k, _| !of_layer(k, id));
                id
            }
            None => {
                let id = self.add_layer(reg, thing, slot, &copied.kind)?;
                let card = self.layer_card_mut(thing, slot);
                let name = copied.name.trim();
                if !name.is_empty()
                    && !card.layers.iter().any(|l| l.name == name)
                    && let Some(l) = card.layers.iter_mut().find(|l| l.id == id)
                {
                    l.name = name.to_owned();
                }
                id
            }
        };
        let card = self.layer_card_mut(thing, slot);
        for (k, v) in copied.params.iter() {
            if let Some(v) = fitted(kind, k, v) {
                card.params.set(&crate::layered_key(id, k), v);
            }
        }
        // Shown or hidden as it was copied; the base layer is always shown.
        if let Some(l) = card.layers.iter_mut().find(|l| l.id == id) {
            l.enabled = copied.enabled || kind.base();
        }
        Ok(id)
    }

    /// Replaces the card's whole stack with a copied one: every layer, its settings, keyframes and
    /// pulses go, and the copied layers come in with new ids from 0, bottom first.
    pub fn paste_look(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        copied: &CopiedLook,
    ) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let mut kinds = Vec::with_capacity(copied.layers.len());
        for l in &copied.layers {
            let kind = reg.layer(&l.kind).ok_or_else(|| EditError::UnknownKind(l.kind.clone()))?;
            if kind.card() != card.kind {
                return Err(EditError::NotAccepted);
            }
            kinds.push(kind);
        }
        let on = |a: &Address| a.thing == thing && a.slot == Some(slot) && split_layered(&a.key).is_some();
        self.tracks.retain(|a, _| !on(a));
        self.pulses.retain(|a, _| !on(a));
        let card = self.layer_card_mut(thing, slot);
        card.params.0.retain(|k, _| split_layered(k).is_none());
        card.layers.clear();
        for (i, (l, kind)) in copied.layers.iter().zip(kinds).enumerate() {
            let id = LayerId(i as u32);
            let name = if l.name.trim().is_empty() { kind.name().to_owned() } else { l.name.clone() };
            card.layers.push(Layer { id, kind: l.kind.clone(), name, enabled: l.enabled || kind.base() });
            for (k, v) in l.params.iter() {
                if let Some(v) = fitted(kind, k, v) {
                    card.params.set(&crate::layered_key(id, k), v);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Value, testkit};

    /// A scene with one ball whose Look is a stack holding only its base layer.
    fn stacked() -> (Registry, Scene, ThingId) {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        (r, s, b)
    }

    fn kinds(s: &Scene, b: ThingId) -> Vec<String> {
        s.card(b, SlotId::Look).unwrap().layers.iter().map(|l| l.name.clone()).collect()
    }

    const LOOK: SlotId = SlotId::Look;

    #[test]
    fn layered_keys_split_only_when_they_start_with_a_number() {
        assert_eq!(split_layered("3/width"), Some((LayerId(3), "width")));
        assert_eq!(split_layered("12/a/b"), Some((LayerId(12), "a/b")));
        for plain in ["width", "x/width", "3/", "/width", "-1/width", ""] {
            assert_eq!(split_layered(plain), None, "{plain}");
        }
        assert_eq!(layered_key(LayerId(7), "glow"), "7/glow");
    }

    #[test]
    fn layers_go_where_their_section_says_and_take_free_names() {
        let (r, mut s, b) = stacked();
        let pop = s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let ring2 = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Ring 2", "Pop"], "drawn layers stay under the reactions");
        assert_eq!((pop, ring, ring2), (LayerId(1), LayerId(2), LayerId(3)));
        assert_eq!(s.add_layer(&r, b, LOOK, "react.pop"), Err(EditError::OnlyOne("Pop".into())));
        assert_eq!(s.add_layer(&r, b, LOOK, "layer.base"), Err(EditError::OnlyOne("Base".into())));
        assert_eq!(s.add_layer(&r, b, LOOK, "layer.nope"), Err(EditError::UnknownKind("layer.nope".into())));
        s.add_slot(&r, b, SlotId::Trail, "trail.streak").unwrap();
        assert_eq!(s.add_layer(&r, b, SlotId::Trail, "layer.ring"), Err(EditError::NotAccepted));
        assert_eq!(s.add_layer(&r, b, SlotId::Motion, "layer.ring"), Err(EditError::NoSlot(SlotId::Motion)));
        // A name freed by a rename is taken again.
        s.rename_layer(&r, b, LOOK, ring, "Thin").unwrap();
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Thin", "Ring 2", "Ring", "Pop"]);
    }

    #[test]
    fn the_base_layer_stays_put_and_shown() {
        let (r, mut s, b) = stacked();
        let base = LayerId(0);
        assert_eq!(s.remove_layer(&r, b, LOOK, base), Err(EditError::BaseLayer));
        assert_eq!(s.set_layer_enabled(&r, b, LOOK, base, false), Err(EditError::BaseLayer));
        assert_eq!(s.duplicate_layer(&r, b, LOOK, base), Err(EditError::BaseLayer));
        assert_eq!(s.move_layer(&r, b, LOOK, base, 0), Err(EditError::BaseLayer));
        s.set_layer_enabled(&r, b, LOOK, base, true).unwrap();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        assert_eq!(s.move_layer(&r, b, LOOK, ring, 0), Err(EditError::NotAccepted), "nothing goes under the base");
        assert_eq!(s.remove_layer(&r, b, LOOK, LayerId(9)), Err(EditError::NoLayer(LayerId(9))));
    }

    #[test]
    fn drawn_layers_move_between_the_base_and_the_reactions() {
        let (r, mut s, b) = stacked();
        let a = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let pop = s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        s.move_layer(&r, b, LOOK, a, 2).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring 2", "Ring", "Pop"]);
        assert_eq!(s.move_layer(&r, b, LOOK, a, 3), Err(EditError::NotAccepted), "not among the reactions");
        assert_eq!(s.move_layer(&r, b, LOOK, pop, 1), Err(EditError::NotAccepted), "reactions don't move");
        s.move_layer(&r, b, LOOK, a, 1).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Ring 2", "Pop"]);
    }

    #[test]
    fn removing_a_layer_takes_its_settings_keyframes_and_pulses() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let keep = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let width = |id| Address::on(b, LOOK, &layered_key(id, "width"));
        s.set_at(&r, &width(ring), Value::Number(9.0)).unwrap();
        s.set_at(&r, &width(keep), Value::Number(7.0)).unwrap();
        s.set_key(&r, &width(ring), 1.0, Value::Number(2.0)).unwrap();
        s.set_key(&r, &width(keep), 1.0, Value::Number(2.0)).unwrap();
        s.set_pulse(&r, &width(ring), crate::PulseSource::Hits, Value::Number(1.0), 0.2).unwrap();
        s.remove_layer(&r, b, LOOK, ring).unwrap();
        let card = s.card(b, LOOK).unwrap();
        assert_eq!(card.params.get("1/width"), None);
        assert_eq!(card.params.get("2/width"), Some(&Value::Number(7.0)));
        assert!(!s.tracks.contains_key(&width(ring)) && !s.pulses.contains_key(&width(ring)));
        assert!(s.tracks.contains_key(&width(keep)));
        assert_eq!(kinds(&s, b), ["Base", "Ring 2"]);
    }

    #[test]
    fn a_duplicate_sits_above_with_its_own_copy_of_everything() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let width = |id| Address::on(b, LOOK, &layered_key(id, "width"));
        s.set_at(&r, &width(ring), Value::Number(9.0)).unwrap();
        s.set_key(&r, &width(ring), 1.0, Value::Number(2.0)).unwrap();
        s.set_layer_enabled(&r, b, LOOK, ring, false).unwrap();
        let copy = s.duplicate_layer(&r, b, LOOK, ring).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Ring copy", "Pop"]);
        assert_eq!(s.value_at(&r, &width(copy)), Some(Value::Number(9.0)));
        assert_eq!(s.tracks[&width(copy)], s.tracks[&width(ring)]);
        assert!(!s.card(b, LOOK).unwrap().layer(copy).unwrap().enabled, "a hidden layer's copy is hidden too");
        s.set_at(&r, &width(copy), Value::Number(1.0)).unwrap();
        assert_eq!(s.value_at(&r, &width(ring)), Some(Value::Number(9.0)), "the copy is its own");
        assert_eq!(s.duplicate_layer(&r, b, LOOK, LayerId(2)), Err(EditError::OnlyOne("Pop".into())));
    }

    #[test]
    fn a_blank_rename_gives_back_the_kind_name() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.rename_layer(&r, b, LOOK, ring, "  Thick  ").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Thick"]);
        s.rename_layer(&r, b, LOOK, ring, "   ").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring"]);
    }

    #[test]
    fn copied_layers_paste_as_new_ones_or_onto_their_only_one() {
        let (r, mut s, b) = stacked();
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.rename_layer(&r, b, LOOK, ring, "Thick").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, "1/width"), Value::Number(12.0)).unwrap();
        s.set_at(&r, &Address::on(b, LOOK, "0/size"), Value::Number(2.0)).unwrap();
        let copied = s.copy_layer(b, LOOK, ring).unwrap();
        assert_eq!(copied.params.get("width"), Some(&Value::Number(12.0)));
        let pasted = s.paste_layer(&r, other, LOOK, &copied).unwrap();
        assert_eq!(kinds(&s, other), ["Base", "Thick"]);
        assert_eq!(s.value_at(&r, &Address::on(other, LOOK, &layered_key(pasted, "width"))), Some(Value::Number(12.0)));
        // A base layer's copy goes onto the base layer that is there.
        let base = s.copy_layer(b, LOOK, LayerId(0)).unwrap();
        assert_eq!(s.paste_layer(&r, other, LOOK, &base).unwrap(), LayerId(0));
        assert_eq!(s.value_at(&r, &Address::on(other, LOOK, "0/size")), Some(Value::Number(2.0)));
        assert_eq!(kinds(&s, other), ["Base", "Thick"]);
        // Pasting the same name again takes the kind's next free name.
        s.paste_layer(&r, other, LOOK, &copied).unwrap();
        assert_eq!(kinds(&s, other), ["Base", "Thick", "Ring"]);
    }

    #[test]
    fn a_pasted_look_replaces_the_whole_stack() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(ring, "width")), Value::Number(12.0)).unwrap();
        s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let look = s.copy_look(b, LOOK).unwrap();
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        let a = s.add_layer(&r, other, LOOK, "layer.ring").unwrap();
        let b2 = s.add_layer(&r, other, LOOK, "layer.ring").unwrap();
        s.remove_layer(&r, other, LOOK, a).unwrap();
        let old = Address::on(other, LOOK, &layered_key(b2, "width"));
        s.set_key(&r, &old, 0.5, Value::Number(3.0)).unwrap();
        s.paste_look(&r, other, LOOK, &look).unwrap();
        let card = s.card(other, LOOK).unwrap();
        let ids: Vec<u32> = card.layers.iter().map(|l| l.id.0).collect();
        assert_eq!(ids, [0, 1, 2]);
        assert_eq!(kinds(&s, other), ["Base", "Ring", "Pop"]);
        assert_eq!(card.params.get("1/width"), Some(&Value::Number(12.0)));
        assert!(card.params.get("2/width").is_none() && !s.tracks.contains_key(&old), "the old stack is gone");
    }

    #[test]
    fn a_hidden_layer_stays_hidden_when_copied() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.set_layer_enabled(&r, b, LOOK, ring, false).unwrap();
        let look = s.copy_look(b, LOOK).unwrap();
        let layer = s.copy_layer(b, LOOK, ring).unwrap();
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        s.paste_look(&r, other, LOOK, &look).unwrap();
        let shown: Vec<bool> = s.card(other, LOOK).unwrap().layers.iter().map(|l| l.enabled).collect();
        assert_eq!(shown, [true, false], "the pasted look shows what the copied one showed");
        let pasted = s.paste_layer(&r, other, LOOK, &layer).unwrap();
        assert!(!s.card(other, LOOK).unwrap().layer(pasted).unwrap().enabled);
    }

    #[test]
    fn a_slot_with_layers_round_trips_and_one_without_writes_none() {
        let (_, s, b) = stacked();
        let card = s.card(b, LOOK).unwrap();
        let text = ron::to_string(card).unwrap();
        assert_eq!(ron::from_str::<Slot>(&text).unwrap(), *card);
        let plain = ron::to_string(&Slot::new("look.plain")).unwrap();
        assert!(!plain.contains("layers"), "{plain}");
        let mut hidden = card.clone();
        hidden.layers[0].enabled = false;
        assert_eq!(ron::from_str::<Slot>(&ron::to_string(&hidden).unwrap()).unwrap(), hidden);
    }
}
