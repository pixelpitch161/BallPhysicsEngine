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
    /// For a mask, the drawn layer it limits. A mask sits in the card's list straight after its
    /// layer (and that layer's earlier masks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<LayerId>,
}

impl Default for Layer {
    fn default() -> Self {
        Self { id: LayerId(0), kind: KindId::new(), name: String::new(), enabled: true, parent: None }
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

/// A layer as copied: its kind, name, whether it is shown, the values that differ from its
/// kind's defaults, by plain key, and its masks. It carries no keyframes or pulses.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedLayer {
    /// The id it had where it was copied from.
    pub id: LayerId,
    pub kind: KindId,
    pub name: String,
    pub enabled: bool,
    pub params: Params,
    /// Its masks, in order.
    pub masks: Vec<CopiedLayer>,
}

/// A whole stack as copied, bottom first; masks ride with their layers.
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

/// Where the block of layer `index` ends: past the masks that follow it.
fn block_end(card: &Slot, index: usize) -> usize {
    let id = card.layers[index].id;
    let n = card.layers[index + 1..].iter().take_while(|l| l.parent == Some(id)).count();
    index + 1 + n
}

/// The masks of layer `id`, in order.
fn masks_of(card: &Slot, id: LayerId) -> Vec<LayerId> {
    card.layers.iter().filter(|l| l.parent == Some(id)).map(|l| l.id).collect()
}

/// `v` fitted to setting `key` of a layer of `kind` whose values are `params` (plain keys).
fn fitted_in(kind: &dyn LayerKind, params: &Params, key: &str, v: &crate::Value) -> Option<crate::Value> {
    let get = |k: &str| params.get(k).cloned();
    let def = crate::layer_def(kind, &get, key)?;
    if !def.ty.accepts(v) {
        return None;
    }
    let v = def.clamp(v.clone());
    (v != def.default).then_some(v)
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
                LayerSection::Mask => return Err(EditError::NotAccepted),
            }
        };
        let layer = Layer {
            id: card.next_layer_id(),
            kind: kind.into(),
            name: free_name(card, k.name()),
            enabled: true,
            parent: None,
        };
        let id = layer.id;
        self.layer_card_mut(thing, slot).layers.insert(at, layer);
        Ok(id)
    }

    /// Adds a mask to drawn layer `parent` (not the base layer, nor a kind that can't be masked),
    /// every setting at its default, after the layer's other masks, and returns its id.
    pub fn add_mask(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        parent: LayerId,
    ) -> Result<LayerId, EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(parent).ok_or(EditError::NoLayer(parent))?;
        let pk = reg.layer(&card.layers[index].kind).ok_or(EditError::NotAccepted)?;
        if pk.section() != LayerSection::Drawn || pk.base() || !pk.maskable() || card.layers[index].parent.is_some() {
            return Err(EditError::NotAccepted);
        }
        let mk = reg
            .layers_for(&card.kind)
            .find(|k| k.section() == LayerSection::Mask)
            .ok_or_else(|| EditError::UnknownKind("mask".into()))?;
        let id = card.next_layer_id();
        let layer =
            Layer { id, kind: mk.id().into(), name: free_name(card, mk.name()), enabled: true, parent: Some(parent) };
        let at = block_end(card, index);
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
        let mut gone = vec![id];
        gone.extend(masks_of(card, id));
        for &g in &gone {
            for a in self.layer_animation(thing, slot, g) {
                self.tracks.remove(&a);
                self.pulses.remove(&a);
            }
        }
        let card = self.layer_card_mut(thing, slot);
        card.layers.retain(|l| !gone.contains(&l.id));
        card.params.0.retain(|k, _| !gone.iter().any(|&g| of_layer(k, g)));
        Ok(())
    }

    /// Moves drawn layer `id`, with its masks, to `to` in the stack, counting drawn layers from the
    /// bottom and leaving masks out of the count. Drawn layers stay above the base layer and under
    /// the reactions; reactions and masks have no order to change.
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
        let drawn = reg.layer(&card.layers[index].kind).is_some_and(|k| k.section() == LayerSection::Drawn)
            && card.layers[index].parent.is_none();
        let lowest = usize::from(is_base(reg, card, 0));
        let tops = |c: &Slot| -> Vec<usize> {
            (0..drawn_end(reg, c)).filter(|&i| c.layers[i].parent.is_none()).collect::<Vec<_>>()
        };
        let highest = tops(card).len().saturating_sub(1);
        if !drawn || to < lowest || to > highest {
            return Err(EditError::NotAccepted);
        }
        let end = block_end(card, index);
        let card = self.layer_card_mut(thing, slot);
        let block: Vec<Layer> = card.layers.drain(index..end).collect();
        let rest = tops(card);
        let at = rest.get(to).copied().unwrap_or_else(|| drawn_end(reg, card));
        card.layers.splice(at..at, block);
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
        // The layer and its masks (a mask on its own copies just itself), each with a new id.
        let end = if layer.parent.is_some() { index + 1 } else { block_end(card, index) };
        let first = card.next_layer_id().0;
        let fresh: Vec<(LayerId, LayerId)> =
            card.layers[index..end].iter().enumerate().map(|(i, l)| (l.id, LayerId(first + i as u32))).collect();
        let new_of = |old: LayerId| fresh.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
        let rekey = |k: &str| split_layered(k).and_then(|(l, rest)| Some(crate::layered_key(new_of(l)?, rest)));
        let params: Vec<(String, crate::Value)> =
            card.params.iter().filter_map(|(k, v)| Some((rekey(k)?, v.clone()))).collect();
        let copies: Vec<Layer> = card.layers[index..end]
            .iter()
            .map(|l| Layer {
                id: new_of(l.id).expect("listed"),
                name: if l.id == id { format!("{} copy", l.name) } else { l.name.clone() },
                parent: if l.id == id { l.parent } else { l.parent.and_then(new_of) },
                ..l.clone()
            })
            .collect();
        for &(old, _) in &fresh {
            for a in self.layer_animation(thing, slot, old) {
                let to = Address { key: rekey(&a.key).expect("listed as this layer's"), ..a.clone() };
                if let Some(t) = self.tracks.get(&a).cloned() {
                    self.tracks.insert(to.clone(), t);
                }
                if let Some(p) = self.pulses.get(&a).cloned() {
                    self.pulses.insert(to, p);
                }
            }
        }
        let new = LayerId(first);
        let card = self.layer_card_mut(thing, slot);
        for (k, v) in params {
            card.params.set(&k, v);
        }
        card.layers.splice(end..end, copies);
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
        let masks = if layer.parent.is_none() {
            masks_of(card, id).into_iter().filter_map(|m| self.copy_layer(thing, slot, m)).collect()
        } else {
            Vec::new()
        };
        Some(CopiedLayer {
            id,
            kind: layer.kind.clone(),
            name: layer.name.clone(),
            enabled: layer.enabled,
            params,
            masks,
        })
    }

    /// The card's whole stack as copied.
    pub fn copy_look(&self, thing: ThingId, slot: SlotId) -> Option<CopiedLook> {
        let card = self.card(thing, slot)?;
        let layers = card
            .layers
            .iter()
            .filter(|l| l.parent.is_none())
            .map(|l| self.copy_layer(thing, slot, l.id))
            .collect::<Option<Vec<_>>>()?;
        Some(CopiedLook { layers })
    }

    /// Adds a copied layer to the card with its settings and masks and returns its id. Where the
    /// card already has a layer of a kind that may be there only once (the base layer, a
    /// reaction), that layer takes the copied settings instead, keeping its name. A mask can't be
    /// pasted on its own.
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
        self.fill_pasted(kind, thing, slot, id, copied);
        // Its masks come with it; a kind that can't carry masks leaves them.
        if kind.section() == LayerSection::Drawn && kind.maskable() && !kind.base() {
            for m in &copied.masks {
                let Some(mk) = reg.layer(&m.kind) else { continue };
                let Ok(mid) = self.add_mask(reg, thing, slot, id) else { continue };
                let card = self.layer_card_mut(thing, slot);
                let name = m.name.trim();
                if !name.is_empty()
                    && !card.layers.iter().any(|l| l.name == name)
                    && let Some(l) = card.layers.iter_mut().find(|l| l.id == mid)
                {
                    l.name = name.to_owned();
                }
                self.fill_pasted(mk, thing, slot, mid, m);
            }
        }
        Ok(id)
    }

    /// Sets layer `id`'s settings from a copy and shows or hides it as it was copied (the base
    /// layer is always shown).
    fn fill_pasted(&mut self, kind: &dyn LayerKind, thing: ThingId, slot: SlotId, id: LayerId, copied: &CopiedLayer) {
        let card = self.layer_card_mut(thing, slot);
        for (k, v) in copied.params.iter() {
            if let Some(v) = fitted_in(kind, &copied.params, k, v) {
                card.params.set(&crate::layered_key(id, k), v);
            }
        }
        if let Some(l) = card.layers.iter_mut().find(|l| l.id == id) {
            l.enabled = copied.enabled || kind.base();
        }
    }

    /// Replaces the card's whole stack with a copied one: every layer, its settings, keyframes and
    /// pulses go, and the copied layers (and their masks) come in with the ids they had, so a
    /// mask that names another layer still names it. Copies whose ids clash are numbered from 0.
    pub fn paste_look(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        copied: &CopiedLook,
    ) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        // Every layer bottom first, each followed by its masks, with the layer it limits.
        let mut flat: Vec<(&CopiedLayer, &dyn LayerKind, Option<usize>)> = Vec::new();
        for l in &copied.layers {
            let at = flat.len();
            flat.push((l, reg.layer(&l.kind).ok_or_else(|| EditError::UnknownKind(l.kind.clone()))?, None));
            for m in &l.masks {
                flat.push((m, reg.layer(&m.kind).ok_or_else(|| EditError::UnknownKind(m.kind.clone()))?, Some(at)));
            }
        }
        if flat.iter().any(|(_, k, _)| k.card() != card.kind) {
            return Err(EditError::NotAccepted);
        }
        let mut seen = std::collections::BTreeSet::new();
        let keep_ids = flat.iter().all(|(l, _, _)| seen.insert(l.id));
        let ids: Vec<LayerId> =
            (0..flat.len()).map(|i| if keep_ids { flat[i].0.id } else { LayerId(i as u32) }).collect();
        let on = |a: &Address| a.thing == thing && a.slot == Some(slot) && split_layered(&a.key).is_some();
        self.tracks.retain(|a, _| !on(a));
        self.pulses.retain(|a, _| !on(a));
        let card = self.layer_card_mut(thing, slot);
        card.params.0.retain(|k, _| split_layered(k).is_none());
        card.layers.clear();
        for (i, &(l, kind, parent)) in flat.iter().enumerate() {
            let id = ids[i];
            let name = if l.name.trim().is_empty() { kind.name().to_owned() } else { l.name.clone() };
            let parent = parent.map(|p| ids[p]);
            card.layers.push(Layer { id, kind: l.kind.clone(), name, enabled: l.enabled || kind.base(), parent });
            for (k, v) in l.params.iter() {
                if let Some(v) = fitted_in(kind, &l.params, k, v) {
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

    fn names_and_parents(s: &Scene, b: ThingId) -> Vec<(String, Option<u32>)> {
        s.card(b, LOOK).unwrap().layers.iter().map(|l| (l.name.clone(), l.parent.map(|p| p.0))).collect()
    }

    fn np(list: &[(&str, Option<u32>)]) -> Vec<(String, Option<u32>)> {
        list.iter().map(|(n, p)| ((*n).to_owned(), *p)).collect()
    }

    #[test]
    fn masks_go_after_their_layer_and_only_on_maskable_drawn_layers() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let top = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let pop = s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let tint = s.add_layer(&r, b, LOOK, "layer.tint").unwrap();
        let m1 = s.add_mask(&r, b, LOOK, ring).unwrap();
        let m2 = s.add_mask(&r, b, LOOK, ring).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[
                ("Base", None),
                ("Ring", None),
                ("Mask", Some(1)),
                ("Mask 2", Some(1)),
                ("Ring 2", None),
                ("Tint", None),
                ("Pop", None)
            ])
        );
        assert_eq!((m1, m2), (LayerId(5), LayerId(6)));
        assert_eq!(s.add_mask(&r, b, LOOK, LayerId(0)), Err(EditError::NotAccepted), "not on the base");
        assert_eq!(s.add_mask(&r, b, LOOK, tint), Err(EditError::NotAccepted), "not on a kind that says no");
        assert_eq!(s.add_mask(&r, b, LOOK, pop), Err(EditError::NotAccepted), "not on a reaction");
        assert_eq!(s.add_mask(&r, b, LOOK, m1), Err(EditError::NotAccepted), "not on a mask");
        assert_eq!(s.add_mask(&r, b, LOOK, LayerId(40)), Err(EditError::NoLayer(LayerId(40))));
        assert_eq!(s.add_layer(&r, b, LOOK, "layer.cut"), Err(EditError::NotAccepted), "masks need a layer");
        // A layer added later still goes on top of the drawn layers, above the masks.
        s.set_layer_enabled(&r, b, LOOK, m1, false).unwrap();
        assert!(!s.card(b, LOOK).unwrap().layer(m1).unwrap().enabled);
        assert!(s.card(b, LOOK).unwrap().layer(ring).unwrap().enabled, "only the mask goes off");
        let _ = top;
    }

    #[test]
    fn a_moved_layer_takes_its_masks_and_masks_dont_move() {
        let (r, mut s, b) = stacked();
        let a = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let m = s.add_mask(&r, b, LOOK, a).unwrap();
        s.move_layer(&r, b, LOOK, a, 2).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[("Base", None), ("Ring 2", None), ("Ring", None), ("Mask", Some(1)), ("Pop", None)])
        );
        assert_eq!(s.move_layer(&r, b, LOOK, a, 3), Err(EditError::NotAccepted), "positions count layers only");
        assert_eq!(s.move_layer(&r, b, LOOK, m, 1), Err(EditError::NotAccepted), "a mask stays with its layer");
        s.move_layer(&r, b, LOOK, a, 1).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[("Base", None), ("Ring", None), ("Mask", Some(1)), ("Ring 2", None), ("Pop", None)])
        );
    }

    #[test]
    fn a_duplicate_brings_copies_of_its_masks() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_mask(&r, b, LOOK, ring).unwrap();
        let size = |id| Address::on(b, LOOK, &layered_key(id, "size"));
        s.set_at(&r, &size(m), Value::Number(0.9)).unwrap();
        s.set_key(&r, &size(m), 1.0, Value::Number(0.1)).unwrap();
        let copy = s.duplicate_layer(&r, b, LOOK, ring).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[("Base", None), ("Ring", None), ("Mask", Some(1)), ("Ring copy", None), ("Mask", Some(3))])
        );
        assert_eq!(copy, LayerId(3));
        let mcopy = LayerId(4);
        assert_eq!(s.value_at(&r, &size(mcopy)), Some(Value::Number(0.9)));
        assert_eq!(s.tracks[&size(mcopy)], s.tracks[&size(m)]);
        // A mask duplicated on its own stays on the same layer.
        let m2 = s.duplicate_layer(&r, b, LOOK, m).unwrap();
        assert_eq!(s.card(b, LOOK).unwrap().layer(m2).unwrap().parent, Some(ring));
        assert_eq!(s.card(b, LOOK).unwrap().layer_index(m2), Some(3));
    }

    #[test]
    fn removing_a_layer_removes_its_masks_with_their_settings_and_animation() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let keep = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_mask(&r, b, LOOK, ring).unwrap();
        let k = s.add_mask(&r, b, LOOK, keep).unwrap();
        let size = |id| Address::on(b, LOOK, &layered_key(id, "size"));
        s.set_at(&r, &size(m), Value::Number(0.9)).unwrap();
        s.set_key(&r, &size(m), 1.0, Value::Number(0.1)).unwrap();
        s.set_pulse(&r, &size(m), crate::PulseSource::Hits, Value::Number(1.0), 0.2).unwrap();
        s.set_at(&r, &size(k), Value::Number(0.8)).unwrap();
        s.remove_layer(&r, b, LOOK, ring).unwrap();
        assert_eq!(names_and_parents(&s, b), np(&[("Base", None), ("Ring 2", None), ("Mask 2", Some(2))]));
        assert!(s.card(b, LOOK).unwrap().params.get(&layered_key(m, "size")).is_none());
        assert!(!s.tracks.contains_key(&size(m)) && !s.pulses.contains_key(&size(m)));
        // Removing a mask leaves its layer.
        s.remove_layer(&r, b, LOOK, k).unwrap();
        assert_eq!(names_and_parents(&s, b), np(&[("Base", None), ("Ring 2", None)]));
    }

    #[test]
    fn copied_layers_and_looks_keep_their_masks() {
        let (r, mut s, b) = stacked();
        let low = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_mask(&r, b, LOOK, ring).unwrap();
        s.rename_layer(&r, b, LOOK, m, "Cut").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(m, "target")), Value::Int(low.0 as i64)).unwrap();
        let layer = s.copy_layer(b, LOOK, ring).unwrap();
        assert_eq!(layer.masks.len(), 1);
        assert_eq!(layer.masks[0].params.get("target"), Some(&Value::Int(1)));
        let look = s.copy_look(b, LOOK).unwrap();
        assert_eq!(look.layers.len(), 3, "masks ride inside their layer");

        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        let pasted = s.paste_layer(&r, other, LOOK, &layer).unwrap();
        assert_eq!(names_and_parents(&s, other), np(&[("Base", None), ("Ring 2", None), ("Cut", Some(pasted.0))]));
        let mid = s.card(other, LOOK).unwrap().layers[2].id;
        assert_eq!(s.value_at(&r, &Address::on(other, LOOK, &layered_key(mid, "target"))), Some(Value::Int(1)));
        assert_eq!(s.paste_layer(&r, other, LOOK, &layer.masks[0]), Err(EditError::NotAccepted));

        s.paste_look(&r, other, LOOK, &look).unwrap();
        let card = s.card(other, LOOK).unwrap();
        let ids: Vec<(u32, Option<u32>)> = card.layers.iter().map(|l| (l.id.0, l.parent.map(|p| p.0))).collect();
        assert_eq!(ids, [(0, None), (1, None), (2, None), (3, Some(2))], "ids kept, so targets still point right");
        assert_eq!(card.params.get(&layered_key(m, "target")), Some(&Value::Int(1)));
    }

    #[test]
    fn a_layers_own_values_can_give_it_more_settings() {
        let (r, mut s, b) = stacked();
        let code = s.add_layer(&r, b, LOOK, "layer.code").unwrap();
        let extra = Address::on(b, LOOK, &layered_key(code, "extra"));
        assert!(s.def_at(&r, &extra).is_none());
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(code, "code")), Value::Text("extra".into())).unwrap();
        assert_eq!(s.def_at(&r, &extra).map(|d| d.label), Some("Extra"));
        s.set_at(&r, &extra, Value::Number(0.25)).unwrap();
        assert_eq!(s.value_at(&r, &extra), Some(Value::Number(0.25)));
        let look = crate::resolve(&s, &r);
        let card = &look.things[&b].slots[&LOOK];
        assert_eq!(card.params.get(&layered_key(code, "extra")), Some(&Value::Number(0.25)));
        let back = crate::load(&crate::save(&s), &r).unwrap();
        assert_eq!(back.things[&b].slots[&LOOK].params.get(&layered_key(code, "extra")), Some(&Value::Number(0.25)));
    }

    #[test]
    fn a_mask_whose_layer_is_gone_is_dropped_on_load() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_mask(&r, b, LOOK, ring).unwrap();
        s.card_mut(b, LOOK).unwrap().layers[2].parent = Some(LayerId(30));
        s.card_mut(b, LOOK).unwrap().params.set(&layered_key(m, "size"), Value::Number(0.3));
        let back = crate::load(&crate::save(&s), &r).unwrap();
        let card = &back.things[&b].slots[&LOOK];
        assert_eq!(card.layers.len(), 2);
        assert!(card.params.get(&layered_key(m, "size")).is_none());
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
