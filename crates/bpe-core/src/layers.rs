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

fn is_empty(v: &[u32]) -> bool {
    v.is_empty()
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
    /// For a sub-layer, the drawn layer it belongs to. A sub-layer sits in the card's list
    /// straight after its layer (and that layer's earlier sub-layers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<LayerId>,
    /// The ball numbers (places in the group) this layer applies to; empty for every ball.
    /// Numbers past the group's last ball do nothing, and come back if it grows.
    #[serde(skip_serializing_if = "is_empty")]
    pub only: Vec<u32>,
    /// For a sound chain layer, which oscillator's chain it is in (0 = Osc A, 1 = Osc B); for a
    /// bus effect, which bus (0..2). Layers of a lane keep their order within it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane: Option<u8>,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            id: LayerId(0),
            kind: KindId::new(),
            name: String::new(),
            enabled: true,
            parent: None,
            only: Vec::new(),
            lane: None,
        }
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
/// kind's defaults, by plain key, and its sub-layers. It carries no keyframes or pulses.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedLayer {
    /// The id it had where it was copied from.
    pub id: LayerId,
    pub kind: KindId,
    pub name: String,
    pub enabled: bool,
    pub params: Params,
    /// Its sub-layers, in order.
    pub subs: Vec<CopiedLayer>,
    /// The balls it applies to (see [`Layer::only`]).
    pub only: Vec<u32>,
    /// The chain or bus it was in (see [`Layer::lane`]).
    pub lane: Option<u8>,
    /// For a sub-layer that names another layer (a cut's Layer), that layer's id and kind when
    /// it was copied: pasted where that id is a layer of another kind, or none, it names none.
    pub target: Option<(LayerId, KindId)>,
}

/// A whole stack as copied, bottom first, without the group layers; sub-layers ride with their layers.
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

/// At most this many sub-layers on one layer.
pub const MAX_SUBS: usize = 12;

/// Where the block of layer `index` ends: past the sub-layers that follow it.
fn block_end(card: &Slot, index: usize) -> usize {
    let id = card.layers[index].id;
    let n = card.layers[index + 1..].iter().take_while(|l| l.parent == Some(id)).count();
    index + 1 + n
}

/// The sub-layers of layer `id`, in order.
fn subs_of(card: &Slot, id: LayerId) -> Vec<LayerId> {
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

/// The section of `layer`'s kind; an unknown kind counts as drawn.
fn section_of(reg: &Registry, layer: &Layer) -> LayerSection {
    reg.layer(&layer.kind).map_or(LayerSection::Drawn, |k| k.section())
}

/// Where the drawn layers (and their sub-layers) end: the index of the first around, reaction or
/// group layer, or the length.
fn drawn_end(reg: &Registry, card: &Slot) -> usize {
    card.layers
        .iter()
        .position(|l| matches!(section_of(reg, l), LayerSection::Around | LayerSection::Reaction | LayerSection::Group))
        .unwrap_or(card.layers.len())
}

/// Where the around layers (trails and particles, and their sub-layers) end: the index of the
/// first reaction or group layer, or the length.
fn around_end(reg: &Registry, card: &Slot) -> usize {
    card.layers
        .iter()
        .position(|l| matches!(section_of(reg, l), LayerSection::Reaction | LayerSection::Group))
        .unwrap_or(card.layers.len())
}

/// Where a new layer of `section` in `lane` goes among the sound sections (chains, modulators,
/// bus effects): after the layers of the same section in a lane up to `lane`, before those of
/// later lanes and of later sections.
fn sound_end(reg: &Registry, card: &Slot, section: LayerSection, lane: Option<u8>) -> usize {
    let rank = |s: LayerSection| match s {
        LayerSection::Chain => 0,
        LayerSection::Modulator => 1,
        LayerSection::Effect => 2,
        _ => 3,
    };
    let (r, lane) = (rank(section), lane.unwrap_or(0));
    card.layers
        .iter()
        .position(|l| {
            let lr = rank(section_of(reg, l));
            lr > r || (lr == r && l.lane.unwrap_or(0) > lane)
        })
        .unwrap_or(card.layers.len())
}

/// Whether layers of `section` sit in lanes and stay in their own (see [`Layer::lane`]).
fn has_lanes(section: LayerSection) -> bool {
    matches!(section, LayerSection::Chain | LayerSection::Effect)
}

/// Where the group layers start: the index of the first one, or the length.
fn group_start(reg: &Registry, card: &Slot) -> usize {
    card.layers.iter().position(|l| section_of(reg, l) == LayerSection::Group).unwrap_or(card.layers.len())
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
    /// on top of the drawn layers (under the trails and particles), a trail or particles layer
    /// after the others of its section (under the reactions), a reaction after the reactions (under the
    /// group layers), a group layer at the end and a base layer at the bottom. A kind that doesn't
    /// repeat may be in a card only once.
    pub fn add_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        kind: &str,
    ) -> Result<LayerId, EditError> {
        self.add_layer_in(reg, thing, slot, kind, None)
    }

    /// [`Scene::add_layer`] into `lane` (see [`Layer::lane`]): a chain layer or bus effect goes at
    /// the end of its lane, a modulator after the other modulators. Layers of other sections take
    /// no lane.
    pub fn add_layer_in(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        kind: &str,
        lane: Option<u8>,
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
                LayerSection::Around => around_end(reg, card),
                LayerSection::Reaction => group_start(reg, card),
                LayerSection::Group => card.layers.len(),
                LayerSection::Sub => return Err(EditError::NotAccepted),
                s @ (LayerSection::Chain | LayerSection::Modulator | LayerSection::Effect) => {
                    sound_end(reg, card, s, lane)
                }
            }
        };
        let lane = if has_lanes(k.section()) { Some(lane.unwrap_or(0)) } else { None };
        let layer = Layer {
            id: card.next_layer_id(),
            kind: kind.into(),
            name: free_name(card, k.name()),
            lane,
            ..Layer::default()
        };
        let id = layer.id;
        self.layer_card_mut(thing, slot).layers.insert(at, layer);
        Ok(id)
    }

    /// Adds a sub-layer of `kind` to drawn layer `parent`, every setting at its default, after
    /// the layer's other sub-layers, and returns its id. Refused on the base layer, a layer that
    /// isn't drawn or is itself a sub-layer, a kind that takes no sub-layers (or not this one), a
    /// sub kind of another card, and a layer that already has [`MAX_SUBS`].
    pub fn add_sub_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        parent: LayerId,
        kind: &str,
    ) -> Result<LayerId, EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(parent).ok_or(EditError::NoLayer(parent))?;
        let sk = reg.layer(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        Self::check_sub(reg, card, index, sk)?;
        let id = card.next_layer_id();
        let layer = Layer {
            id,
            kind: sk.id().into(),
            name: free_name(card, sk.name()),
            parent: Some(parent),
            ..Layer::default()
        };
        let at = block_end(card, index);
        self.layer_card_mut(thing, slot).layers.insert(at, layer);
        Ok(id)
    }

    /// Whether the layer at `index` of `card` may take one more sub-layer of kind `sk`.
    fn check_sub(reg: &Registry, card: &Slot, index: usize, sk: &dyn LayerKind) -> Result<(), EditError> {
        let parent = &card.layers[index];
        let pk = reg.layer(&parent.kind).ok_or(EditError::NotAccepted)?;
        // A base layer (the Body) takes sub-layers only when its kind says so.
        let refused = !matches!(pk.section(), LayerSection::Drawn | LayerSection::Around)
            || !pk.takes_subs()
            || parent.parent.is_some()
            || sk.section() != LayerSection::Sub
            || sk.card() != card.kind
            || !pk.takes_sub(sk.id())
            || subs_of(card, parent.id).len() >= MAX_SUBS;
        if refused { Err(EditError::NotAccepted) } else { Ok(()) }
    }

    /// Moves sub-layer `id` to place `to` among its layer's sub-layers (counting from 0, the first
    /// to work on the layer; past the last is the last). Every other layer keeps its place.
    pub fn move_sub_layer(&mut self, thing: ThingId, slot: SlotId, id: LayerId, to: usize) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        let parent = card.layers[index].parent.ok_or(EditError::NotAccepted)?;
        let first = card.layer_index(parent).ok_or(EditError::NoLayer(parent))? + 1;
        let count = subs_of(card, parent).len();
        let to = first + to.min(count - 1);
        let card = self.layer_card_mut(thing, slot);
        let layer = card.layers.remove(index);
        card.layers.insert(to, layer);
        Ok(())
    }

    /// Adds a copied sub-layer to drawn layer `parent` as its last sub-layer, with its settings,
    /// and returns its id; refused as [`Scene::add_sub_layer`] refuses.
    pub fn paste_sub_layer(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        parent: LayerId,
        copied: &CopiedLayer,
    ) -> Result<LayerId, EditError> {
        let kind = reg.layer(&copied.kind).ok_or_else(|| EditError::UnknownKind(copied.kind.clone()))?;
        let id = self.add_sub_layer(reg, thing, slot, parent, &copied.kind)?;
        self.name_pasted(thing, slot, id, &copied.name);
        self.fill_pasted(kind, thing, slot, id, copied);
        Ok(id)
    }

    /// Gives pasted layer `id` the copied name when nobody in the card has it.
    fn name_pasted(&mut self, thing: ThingId, slot: SlotId, id: LayerId, name: &str) {
        let card = self.layer_card_mut(thing, slot);
        let name = name.trim();
        if !name.is_empty()
            && !card.layers.iter().any(|l| l.name == name)
            && let Some(l) = card.layers.iter_mut().find(|l| l.id == id)
        {
            l.name = name.to_owned();
        }
    }

    /// Takes layer `id` out of the card with its settings, keyframes and pulses, every
    /// modulation route from it or to one of its settings, and every setting's Follow sound on
    /// it. The base layer stays.
    pub fn remove_layer(&mut self, reg: &Registry, thing: ThingId, slot: SlotId, id: LayerId) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        if is_base(reg, card, index) {
            return Err(EditError::BaseLayer);
        }
        let mut gone = vec![id];
        gone.extend(subs_of(card, id));
        for &g in &gone {
            for a in self.layer_animation(thing, slot, g) {
                self.tracks.remove(&a);
                self.pulses.remove(&a);
            }
        }
        // Settings anywhere that follow a modulator going away stop following it. Modulators live
        // only on the Sound card, and layer ids are counted per card, so a Look layer that shares
        // a modulator's number takes nothing with it.
        if slot == SlotId::Sound {
            self.pulses.retain(|_, p| match p.source {
                crate::PulseSource::Sound { shape, source: crate::SoundSource::Modulator(l) } => {
                    shape != thing || !gone.contains(&l)
                }
                _ => true,
            });
        }
        let card = self.layer_card_mut(thing, slot);
        // Ids are never given out twice in a card: a setting that still names a removed layer
        // (a cut's layer, a group layer's chosen layers) must not pick up a new one by surprise.
        card.used_ids = card.next_layer_id().0;
        card.layers.retain(|l| !gone.contains(&l.id));
        card.params.0.retain(|k, _| !gone.iter().any(|&g| of_layer(k, g)));
        if let Some(crate::Value::Routes(routes)) = card.params.0.get_mut(crate::ROUTES_KEY) {
            routes.retain(|r| !gone.contains(&r.source) && !gone.iter().any(|&g| of_layer(&r.target, g)));
        }
        Ok(())
    }

    /// Moves drawn layer `id`, with its sub-layers, to `to` in the stack, counting layers from the
    /// bottom and leaving sub-layers out of the count. Drawn layers stay above the base layer and
    /// under the trails and particles; reactions and sub-layers have no order to change here (see
    /// [`Scene::move_sub_layer`]); trails and particles (around layers) move among themselves,
    /// with their sub-layers, and group layers move among the group layers only. Chain layers and bus effects move only within their own lane, and
    /// modulators among the modulators.
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
        let section = reg.layer(&card.layers[index].kind).map(|k| k.section());
        if let Some(s @ (LayerSection::Chain | LayerSection::Modulator | LayerSection::Effect)) = section {
            let lane = card.layers[index].lane;
            let same = |l: &Layer| section_of(reg, l) == s && (!has_lanes(s) || l.lane == lane);
            let lo = card.layers.iter().position(same).expect("the layer itself");
            let hi = card.layers.iter().rposition(same).expect("the layer itself");
            if to < lo || to > hi {
                return Err(EditError::NotAccepted);
            }
            let card = self.layer_card_mut(thing, slot);
            let layer = card.layers.remove(index);
            card.layers.insert(to, layer);
            return Ok(());
        }
        let group = section == Some(LayerSection::Group);
        let around = section == Some(LayerSection::Around);
        let drawn = section == Some(LayerSection::Drawn) && card.layers[index].parent.is_none();
        // Where the layers it may sit among end, and where those before them end.
        let end_of = |c: &Slot| match () {
            _ if group => c.layers.len(),
            _ if around => around_end(reg, c),
            _ => drawn_end(reg, c),
        };
        let tops =
            |c: &Slot| -> Vec<usize> { (0..end_of(c)).filter(|&i| c.layers[i].parent.is_none()).collect::<Vec<_>>() };
        let tops_before = |c: &Slot, end: usize| c.layers[..end].iter().filter(|l| l.parent.is_none()).count();
        let lowest = match () {
            _ if group => tops_before(card, group_start(reg, card)),
            _ if around => tops_before(card, drawn_end(reg, card)),
            _ => usize::from(is_base(reg, card, 0)),
        };
        let highest = tops(card).len().saturating_sub(1);
        if !(drawn || group || around) || to < lowest || to > highest {
            return Err(EditError::NotAccepted);
        }
        let end = block_end(card, index);
        let card = self.layer_card_mut(thing, slot);
        let block: Vec<Layer> = card.layers.drain(index..end).collect();
        let rest = tops(card);
        let at = rest.get(to).copied().unwrap_or_else(|| end_of(card));
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
        // A sub-layer's copy sits beside it, while its layer has room for one more.
        if let Some(parent) = layer.parent
            && subs_of(card, parent).len() >= MAX_SUBS
        {
            return Err(EditError::NotAccepted);
        }
        // The layer and its sub-layers (a sub-layer on its own copies just itself), each with a new id.
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
        let subs = if layer.parent.is_none() {
            subs_of(card, id).into_iter().filter_map(|m| self.copy_layer(thing, slot, m)).collect()
        } else {
            Vec::new()
        };
        let target = match params.get("target") {
            Some(crate::Value::Int(t)) if layer.parent.is_some() && *t >= 0 => {
                card.layer(LayerId(*t as u32)).map(|l| (l.id, l.kind.clone()))
            }
            _ => None,
        };
        Some(CopiedLayer {
            id,
            kind: layer.kind.clone(),
            name: layer.name.clone(),
            enabled: layer.enabled,
            params,
            subs,
            only: layer.only.clone(),
            lane: layer.lane,
            target,
        })
    }

    /// The card's whole stack as copied, leaving out its group layers: those belong to the
    /// group, not to how one ball looks.
    pub fn copy_look(&self, reg: &Registry, thing: ThingId, slot: SlotId) -> Option<CopiedLook> {
        let card = self.card(thing, slot)?;
        let layers = card
            .layers
            .iter()
            .filter(|l| l.parent.is_none() && section_of(reg, l) != LayerSection::Group)
            .map(|l| self.copy_layer(thing, slot, l.id))
            .collect::<Option<Vec<_>>>()?;
        Some(CopiedLook { layers })
    }

    /// Makes layer `id` apply only to the balls numbered in `only` (empty: every ball). The base
    /// layer is every ball's, and only drawn layers can be given to some balls.
    pub fn set_layer_only(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        id: LayerId,
        mut only: Vec<u32>,
    ) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        let index = card.layer_index(id).ok_or(EditError::NoLayer(id))?;
        if is_base(reg, card, index) {
            return Err(EditError::BaseLayer);
        }
        let kind = reg.layer(&card.layers[index].kind);
        if section_of(reg, &card.layers[index]) != LayerSection::Drawn || kind.is_some_and(|k| !k.per_ball()) {
            return Err(EditError::NotAccepted);
        }
        only.sort_unstable();
        only.dedup();
        self.layer_card_mut(thing, slot).layers[index].only = only;
        Ok(())
    }

    /// Adds a copied layer to the card with its settings and sub-layers and returns its id. Where
    /// the card already has a layer of a kind that may be there only once (the base layer, a
    /// reaction), that layer takes the copied settings instead, keeping its name. A sub-layer
    /// can't be pasted on its own (see [`Scene::paste_sub_layer`]).
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
                // The copy's sub-layers take the place of its own (a Body's Gives off light).
                for sub in subs_of(card, id) {
                    let _ = self.remove_layer(reg, thing, slot, sub);
                }
                for a in self.layer_animation(thing, slot, id) {
                    self.tracks.remove(&a);
                    self.pulses.remove(&a);
                }
                self.layer_card_mut(thing, slot).params.0.retain(|k, _| !of_layer(k, id));
                id
            }
            None => {
                let id = self.add_layer_in(reg, thing, slot, &copied.kind, copied.lane)?;
                self.name_pasted(thing, slot, id, &copied.name);
                id
            }
        };
        self.fill_pasted(kind, thing, slot, id, copied);
        // Its sub-layers come with it; those the layer can't carry are left.
        for sub in &copied.subs {
            let _ = self.paste_sub_layer(reg, thing, slot, id, sub);
        }
        Ok(id)
    }

    /// Sets layer `id`'s settings from a copy, shows or hides it as it was copied and gives it to
    /// the balls it was given to (the base layer is always shown and every ball's).
    fn fill_pasted(&mut self, kind: &dyn LayerKind, thing: ThingId, slot: SlotId, id: LayerId, copied: &CopiedLayer) {
        let card = self.layer_card_mut(thing, slot);
        for (k, v) in copied.params.iter() {
            if let Some(v) = fitted_in(kind, &copied.params, k, v) {
                card.params.set(&crate::layered_key(id, k), v);
            }
        }
        // A cut naming a layer the copy came with names it here only if that id is still a layer
        // of the same kind (pasted on another ball, it may be someone else, or no one).
        if let Some(crate::Value::Int(t)) = copied.params.get("target")
            && *t >= 0
        {
            let same = copied
                .target
                .as_ref()
                .is_some_and(|(at, kind)| i64::from(at.0) == *t && card.layer(*at).is_some_and(|l| &l.kind == kind));
            if !same {
                card.params.set(&crate::layered_key(id, "target"), crate::Value::Int(-1));
            }
        }
        if let Some(l) = card.layers.iter_mut().find(|l| l.id == id) {
            l.enabled = copied.enabled || kind.base();
            l.only = if kind.base() || kind.section() != LayerSection::Drawn || !kind.per_ball() {
                Vec::new()
            } else {
                copied.only.clone()
            };
        }
    }

    /// Replaces the card's whole stack but its group layers with a copied one: every other layer,
    /// its settings, keyframes and pulses go, and the copied layers (and their sub-layers) come in
    /// with the ids they had, under the kept group layers, so a cut that names another layer
    /// still names it. A copy whose id clashes, with a kept group layer or an earlier copy, takes
    /// the lowest id nobody has, and cuts naming it follow. The kept group layers keep only the
    /// chosen layers that come back as the same kind with the same id. Group layers in the copy are left out.
    pub fn paste_look(
        &mut self,
        reg: &Registry,
        thing: ThingId,
        slot: SlotId,
        copied: &CopiedLook,
    ) -> Result<(), EditError> {
        let card = self.layer_card(thing, slot)?;
        // Every layer but the group ones bottom first, each followed by its sub-layers, with the
        // layer it belongs to.
        let mut flat: Vec<(&CopiedLayer, &dyn LayerKind, Option<usize>)> = Vec::new();
        for l in &copied.layers {
            let at = flat.len();
            let kind = reg.layer(&l.kind).ok_or_else(|| EditError::UnknownKind(l.kind.clone()))?;
            if kind.section() == LayerSection::Group {
                continue;
            }
            flat.push((l, kind, None));
            for m in &l.subs {
                flat.push((m, reg.layer(&m.kind).ok_or_else(|| EditError::UnknownKind(m.kind.clone()))?, Some(at)));
            }
        }
        if flat.iter().any(|(_, k, _)| k.card() != card.kind) {
            return Err(EditError::NotAccepted);
        }
        let kept: Vec<Layer> =
            card.layers.iter().filter(|l| section_of(reg, l) == LayerSection::Group).cloned().collect();
        let taken = |id: LayerId| kept.iter().any(|l| l.id == id);
        // Each copied layer keeps its id unless a kept group layer or an earlier copy has it; then
        // it takes one nobody has, and cuts that name it follow.
        let copied_ids: std::collections::BTreeSet<LayerId> = flat.iter().map(|(l, _, _)| l.id).collect();
        let mut fresh = (0u32..).map(LayerId).filter(|id| !taken(*id) && !copied_ids.contains(id));
        let mut used: std::collections::BTreeSet<LayerId> = kept.iter().map(|l| l.id).collect();
        let mut ids: Vec<LayerId> = Vec::with_capacity(flat.len());
        for (l, _, _) in &flat {
            let id = if used.contains(&l.id) { fresh.next().expect("ids never run out") } else { l.id };
            used.insert(id);
            ids.push(id);
        }
        let new_of = |old: i64| flat.iter().position(|(l, _, _)| i64::from(l.id.0) == old).map(|i| ids[i].0 as i64);
        let goes = |key: &str| split_layered(key).is_some_and(|(id, _)| !taken(id));
        let on = |a: &Address| a.thing == thing && a.slot == Some(slot) && goes(&a.key);
        self.tracks.retain(|a, _| !on(a));
        self.pulses.retain(|a, _| !on(a));
        let card = self.layer_card_mut(thing, slot);
        card.params.0.retain(|k, _| !goes(k));
        // The kept group layers keep the layers they chose that come back as the same kind of
        // layer with the same id (a look pasted back where it was copied from); the rest are gone.
        let before: Vec<(LayerId, KindId)> = card.layers.iter().map(|l| (l.id, l.kind.clone())).collect();
        let back = |id: u32| {
            let id = LayerId(id);
            let was = before.iter().find(|(i, _)| *i == id).map(|(_, k)| k);
            let now = flat.iter().zip(&ids).find(|(_, n)| **n == id).map(|((l, _, _), _)| &l.kind);
            was.is_some() && was == now
        };
        for l in &kept {
            let key = crate::layered_key(l.id, "chosen");
            let kept_ids = match card.params.get(&key) {
                Some(crate::Value::Text(t)) => t
                    .split([',', ' '])
                    .filter_map(|w| w.trim().parse::<u32>().ok())
                    .filter(|&id| back(id))
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                _ => String::new(),
            };
            if kept_ids.is_empty() {
                card.params.0.remove(&key);
            } else {
                card.params.set(&key, crate::Value::Text(kept_ids));
            }
        }
        card.layers.clear();
        for (i, &(l, kind, parent)) in flat.iter().enumerate() {
            let id = ids[i];
            let name = if l.name.trim().is_empty() { kind.name().to_owned() } else { l.name.clone() };
            let parent = parent.map(|p| ids[p]);
            let only = if kind.base() || kind.section() != LayerSection::Drawn || !kind.per_ball() {
                Vec::new()
            } else {
                l.only.clone()
            };
            let enabled = l.enabled || kind.base();
            card.layers.push(Layer { id, kind: l.kind.clone(), name, enabled, parent, only, lane: l.lane });
            for (k, v) in l.params.iter() {
                if let Some(mut v) = fitted_in(kind, &l.params, k, v) {
                    // A cut names a layer of the copy by the id it had there.
                    if kind.section() == LayerSection::Sub
                        && k == "target"
                        && let crate::Value::Int(t) = v
                    {
                        v = crate::Value::Int(new_of(t).unwrap_or(-1));
                    }
                    card.params.set(&crate::layered_key(id, k), v);
                }
            }
        }
        card.layers.extend(kept);
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
        let look = s.copy_look(&r, b, LOOK).unwrap();
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
        let look = s.copy_look(&r, b, LOOK).unwrap();
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
    fn sub_layers_go_after_their_layer_and_only_on_drawn_layers_that_take_them() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let top = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let pop = s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let tint = s.add_layer(&r, b, LOOK, "layer.tint").unwrap();
        let m1 = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        let m2 = s.add_sub_layer(&r, b, LOOK, ring, "sub.glow").unwrap();
        let m3 = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[
                ("Base", None),
                ("Ring", None),
                ("Cut", Some(1)),
                ("Glow", Some(1)),
                ("Cut 2", Some(1)),
                ("Ring 2", None),
                ("Tint", None),
                ("Pop", None)
            ])
        );
        assert_eq!((m1, m2, m3), (LayerId(5), LayerId(6), LayerId(7)));
        let add = |s: &mut Scene, p, k| s.add_sub_layer(&r, b, LOOK, p, k);
        assert_eq!(add(&mut s, LayerId(0), "sub.cut"), Err(EditError::NotAccepted), "not on the base");
        assert_eq!(add(&mut s, tint, "sub.cut"), Err(EditError::NotAccepted), "not on a kind that says no");
        assert_eq!(add(&mut s, pop, "sub.cut"), Err(EditError::NotAccepted), "not on a reaction");
        assert_eq!(add(&mut s, m1, "sub.cut"), Err(EditError::NotAccepted), "not on a sub-layer");
        assert_eq!(add(&mut s, ring, "layer.ring"), Err(EditError::NotAccepted), "only sub kinds");
        assert_eq!(add(&mut s, ring, "sub.nope"), Err(EditError::UnknownKind("sub.nope".into())));
        assert_eq!(add(&mut s, LayerId(40), "sub.cut"), Err(EditError::NoLayer(LayerId(40))));
        assert_eq!(s.add_layer(&r, b, LOOK, "sub.cut"), Err(EditError::NotAccepted), "sub-layers need a layer");
        s.set_layer_enabled(&r, b, LOOK, m1, false).unwrap();
        assert!(!s.card(b, LOOK).unwrap().layer(m1).unwrap().enabled);
        assert!(s.card(b, LOOK).unwrap().layer(ring).unwrap().enabled, "only the sub-layer goes off");
        let _ = top;
    }

    #[test]
    fn a_layer_kind_can_refuse_one_sub_kind() {
        let (r, mut s, b) = stacked();
        let code = s.add_layer(&r, b, LOOK, "layer.code").unwrap();
        assert_eq!(s.add_sub_layer(&r, b, LOOK, code, "sub.glow"), Err(EditError::NotAccepted));
        s.add_sub_layer(&r, b, LOOK, code, "sub.cut").unwrap();
    }

    #[test]
    fn a_layer_takes_at_most_twelve_sub_layers() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        for _ in 0..MAX_SUBS {
            s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        }
        assert_eq!(s.add_sub_layer(&r, b, LOOK, ring, "sub.glow"), Err(EditError::NotAccepted));
        let copied = s.copy_layer(b, LOOK, ring).unwrap();
        let first = copied.subs[0].clone();
        assert_eq!(s.paste_sub_layer(&r, b, LOOK, ring, &first), Err(EditError::NotAccepted));
        // A pasted copy of the full layer brings all twelve.
        let pasted = s.paste_layer(&r, b, LOOK, &copied).unwrap();
        assert_eq!(subs_of(s.card(b, LOOK).unwrap(), pasted).len(), MAX_SUBS);
        let last = *subs_of(s.card(b, LOOK).unwrap(), pasted).last().unwrap();
        assert_eq!(s.duplicate_layer(&r, b, LOOK, last), Err(EditError::NotAccepted), "no room for a copy");
    }

    #[test]
    fn a_sub_layer_moves_only_among_its_siblings() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let other = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let c = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        let g = s.add_sub_layer(&r, b, LOOK, ring, "sub.glow").unwrap();
        let c2 = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        let o = s.add_sub_layer(&r, b, LOOK, other, "sub.cut").unwrap();
        let ids = |s: &Scene| s.card(b, LOOK).unwrap().layers.iter().map(|l| l.id).collect::<Vec<_>>();
        s.move_sub_layer(b, LOOK, c, 2).unwrap();
        assert_eq!(ids(&s), [LayerId(0), ring, g, c2, c, other, o]);
        s.move_sub_layer(b, LOOK, c2, 0).unwrap();
        assert_eq!(ids(&s), [LayerId(0), ring, c2, g, c, other, o]);
        s.move_sub_layer(b, LOOK, g, 99).unwrap();
        assert_eq!(ids(&s), [LayerId(0), ring, c2, c, g, other, o], "past the end is the last");
        s.move_sub_layer(b, LOOK, o, 5).unwrap();
        assert_eq!(ids(&s), [LayerId(0), ring, c2, c, g, other, o], "an only sub-layer stays");
        assert_eq!(s.move_sub_layer(b, LOOK, ring, 0), Err(EditError::NotAccepted), "layers aren't sub-layers");
    }

    #[test]
    fn a_copied_sub_layer_pastes_as_a_layers_last_sub_layer() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let other = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let c = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        s.rename_layer(&r, b, LOOK, c, "Half").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(c, "size")), Value::Number(0.25)).unwrap();
        s.add_sub_layer(&r, b, LOOK, other, "sub.glow").unwrap();
        let copied = s.copy_layer(b, LOOK, c).unwrap();
        assert!(copied.subs.is_empty());
        assert_eq!(s.paste_layer(&r, b, LOOK, &copied), Err(EditError::NotAccepted), "not on its own");
        let new = s.paste_sub_layer(&r, b, LOOK, other, &copied).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[
                ("Base", None),
                ("Ring", None),
                ("Half", Some(1)),
                ("Ring 2", None),
                ("Glow", Some(2)),
                ("Cut", Some(2))
            ])
        );
        assert_eq!(s.value_at(&r, &Address::on(b, LOOK, &layered_key(new, "size"))), Some(Value::Number(0.25)));
        let ring_copy = s.copy_layer(b, LOOK, ring).unwrap();
        assert_eq!(s.paste_sub_layer(&r, b, LOOK, other, &ring_copy), Err(EditError::NotAccepted), "a layer isn't one");
    }

    #[test]
    fn a_moved_layer_takes_its_sub_layers_and_they_dont_move_with_move_layer() {
        let (r, mut s, b) = stacked();
        let a = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let m = s.add_sub_layer(&r, b, LOOK, a, "sub.cut").unwrap();
        s.move_layer(&r, b, LOOK, a, 2).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[("Base", None), ("Ring 2", None), ("Ring", None), ("Cut", Some(1)), ("Pop", None)])
        );
        assert_eq!(s.move_layer(&r, b, LOOK, a, 3), Err(EditError::NotAccepted), "positions count layers only");
        assert_eq!(s.move_layer(&r, b, LOOK, m, 1), Err(EditError::NotAccepted), "a sub-layer stays with its layer");
        s.move_layer(&r, b, LOOK, a, 1).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[("Base", None), ("Ring", None), ("Cut", Some(1)), ("Ring 2", None), ("Pop", None)])
        );
    }

    #[test]
    fn a_duplicate_brings_copies_of_its_sub_layers() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        let size = |id| Address::on(b, LOOK, &layered_key(id, "size"));
        s.set_at(&r, &size(m), Value::Number(0.9)).unwrap();
        s.set_key(&r, &size(m), 1.0, Value::Number(0.1)).unwrap();
        let copy = s.duplicate_layer(&r, b, LOOK, ring).unwrap();
        assert_eq!(
            names_and_parents(&s, b),
            np(&[("Base", None), ("Ring", None), ("Cut", Some(1)), ("Ring copy", None), ("Cut", Some(3))])
        );
        assert_eq!(copy, LayerId(3));
        let mcopy = LayerId(4);
        assert_eq!(s.value_at(&r, &size(mcopy)), Some(Value::Number(0.9)));
        assert_eq!(s.tracks[&size(mcopy)], s.tracks[&size(m)]);
        // A sub-layer duplicated on its own stays on the same layer, just after itself.
        let m2 = s.duplicate_layer(&r, b, LOOK, m).unwrap();
        assert_eq!(s.card(b, LOOK).unwrap().layer(m2).unwrap().parent, Some(ring));
        assert_eq!(s.card(b, LOOK).unwrap().layer_index(m2), Some(3));
    }

    #[test]
    fn removing_a_layer_removes_its_sub_layers_with_their_settings_and_animation() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let keep = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        let k = s.add_sub_layer(&r, b, LOOK, keep, "sub.cut").unwrap();
        let size = |id| Address::on(b, LOOK, &layered_key(id, "size"));
        s.set_at(&r, &size(m), Value::Number(0.9)).unwrap();
        s.set_key(&r, &size(m), 1.0, Value::Number(0.1)).unwrap();
        s.set_pulse(&r, &size(m), crate::PulseSource::Hits, Value::Number(1.0), 0.2).unwrap();
        s.set_at(&r, &size(k), Value::Number(0.8)).unwrap();
        s.remove_layer(&r, b, LOOK, ring).unwrap();
        assert_eq!(names_and_parents(&s, b), np(&[("Base", None), ("Ring 2", None), ("Cut 2", Some(2))]));
        assert!(s.card(b, LOOK).unwrap().params.get(&layered_key(m, "size")).is_none());
        assert!(!s.tracks.contains_key(&size(m)) && !s.pulses.contains_key(&size(m)));
        // Removing a sub-layer leaves its layer.
        s.remove_layer(&r, b, LOOK, k).unwrap();
        assert_eq!(names_and_parents(&s, b), np(&[("Base", None), ("Ring 2", None)]));
    }

    #[test]
    fn copied_layers_and_looks_keep_their_sub_layers() {
        let (r, mut s, b) = stacked();
        let low = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        s.rename_layer(&r, b, LOOK, m, "Half").unwrap();
        s.add_sub_layer(&r, b, LOOK, ring, "sub.glow").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(m, "target")), Value::Int(low.0 as i64)).unwrap();
        let layer = s.copy_layer(b, LOOK, ring).unwrap();
        assert_eq!(layer.subs.len(), 2);
        assert_eq!(layer.subs[0].params.get("target"), Some(&Value::Int(1)));
        let look = s.copy_look(&r, b, LOOK).unwrap();
        assert_eq!(look.layers.len(), 3, "sub-layers ride inside their layer");

        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        let pasted = s.paste_layer(&r, other, LOOK, &layer).unwrap();
        assert_eq!(
            names_and_parents(&s, other),
            np(&[("Base", None), ("Ring 2", None), ("Half", Some(pasted.0)), ("Glow", Some(pasted.0))])
        );
        let mid = s.card(other, LOOK).unwrap().layers[2].id;
        assert_eq!(s.value_at(&r, &Address::on(other, LOOK, &layered_key(mid, "target"))), Some(Value::Int(1)));
        assert_eq!(s.paste_layer(&r, other, LOOK, &layer.subs[0]), Err(EditError::NotAccepted));

        s.paste_look(&r, other, LOOK, &look).unwrap();
        let card = s.card(other, LOOK).unwrap();
        let ids: Vec<(u32, Option<u32>)> = card.layers.iter().map(|l| (l.id.0, l.parent.map(|p| p.0))).collect();
        assert_eq!(ids, [(0, None), (1, None), (2, None), (3, Some(2)), (4, Some(2))], "ids kept, so targets hold");
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
    fn a_sub_layer_whose_layer_is_gone_is_dropped_on_load() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let m = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        s.card_mut(b, LOOK).unwrap().layers[2].parent = Some(LayerId(30));
        s.card_mut(b, LOOK).unwrap().params.set(&layered_key(m, "size"), Value::Number(0.3));
        let back = crate::load(&crate::save(&s), &r).unwrap();
        let card = &back.things[&b].slots[&LOOK];
        assert_eq!(card.layers.len(), 2);
        assert!(card.params.get(&layered_key(m, "size")).is_none());
    }

    #[test]
    fn group_layers_sit_after_the_reactions_and_move_only_among_themselves() {
        let (r, mut s, b) = stacked();
        let wash = s.add_layer(&r, b, LOOK, "group.wash").unwrap();
        let swirl = s.add_layer(&r, b, LOOK, "group.swirl").unwrap();
        let pop = s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Pop", "Wash", "Swirl"]);
        assert_eq!(s.add_layer(&r, b, LOOK, "group.wash"), Err(EditError::OnlyOne("Wash".into())));
        s.move_layer(&r, b, LOOK, swirl, 3).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Pop", "Swirl", "Wash"], "group layers reorder");
        assert_eq!(s.move_layer(&r, b, LOOK, wash, 2), Err(EditError::NotAccepted), "not among the reactions");
        assert_eq!(s.move_layer(&r, b, LOOK, ring, 3), Err(EditError::NotAccepted), "drawn ones stay below");
        assert_eq!(s.move_layer(&r, b, LOOK, pop, 4), Err(EditError::NotAccepted));
        let swirl2 = s.duplicate_layer(&r, b, LOOK, swirl).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Pop", "Swirl", "Swirl copy", "Wash"]);
        s.remove_layer(&r, b, LOOK, swirl2).unwrap();
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Ring 2", "Pop", "Swirl", "Wash"]);
    }

    #[test]
    fn a_whole_look_copies_and_pastes_without_touching_group_layers() {
        let (r, mut s, b) = stacked();
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let mine = s.add_layer(&r, b, LOOK, "group.wash").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(mine, "width")), Value::Number(3.0)).unwrap();
        let look = s.copy_look(&r, b, LOOK).unwrap();
        assert_eq!(look.layers.iter().map(|l| l.kind.as_str()).collect::<Vec<_>>(), ["layer.base", "layer.ring"]);
        // The target has its own group layer, keyed, with an id the pasted ones would take.
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        let theirs = s.add_layer(&r, other, LOOK, "group.swirl").unwrap();
        assert_eq!(theirs, LayerId(1));
        let width = Address::on(other, LOOK, &layered_key(theirs, "width"));
        s.set_at(&r, &width, Value::Number(8.0)).unwrap();
        s.set_key(&r, &width, 1.0, Value::Number(2.0)).unwrap();
        s.paste_look(&r, other, LOOK, &look).unwrap();
        let card = s.card(other, LOOK).unwrap();
        assert_eq!(kinds(&s, other), ["Base", "Ring", "Swirl"], "the group layer stays, on top");
        let ids: Vec<u32> = card.layers.iter().map(|l| l.id.0).collect();
        assert_eq!(ids, [0, 2, 1], "pasted ids skip the kept group layer's");
        assert_eq!(card.params.get("1/width"), Some(&Value::Number(8.0)));
        assert!(s.tracks.contains_key(&width), "its keys stay too");
    }

    #[test]
    fn a_pasted_look_renumbered_round_a_kept_group_layer_keeps_its_cuts_targets() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let code = s.add_layer(&r, b, LOOK, "layer.code").unwrap();
        let mask = s.add_sub_layer(&r, b, LOOK, code, "sub.cut").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(mask, "target")), Value::Int(ring.0 as i64)).unwrap();
        let look = s.copy_look(&r, b, LOOK).unwrap();
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        let wash = s.add_layer(&r, other, LOOK, "group.wash").unwrap();
        assert_eq!(wash, ring, "the kept group layer has the ring's id");
        s.paste_look(&r, other, LOOK, &look).unwrap();
        let card = s.card(other, LOOK).unwrap();
        let ring2 = card.layers.iter().find(|l| l.kind == "layer.ring").unwrap().id;
        assert_ne!(ring2, wash);
        let mask2 = card.layers.iter().find(|l| l.parent.is_some()).unwrap().id;
        assert_eq!(mask2, mask, "ids that don't clash stay");
        assert_eq!(card.params.get(&layered_key(mask2, "target")), Some(&Value::Int(ring2.0 as i64)));
    }

    #[test]
    fn a_pasted_cut_names_its_layer_only_where_that_layer_is_the_same_kind() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let code = s.add_layer(&r, b, LOOK, "layer.code").unwrap();
        let cut = s.add_sub_layer(&r, b, LOOK, code, "sub.cut").unwrap();
        s.set_at(&r, &Address::on(b, LOOK, &layered_key(cut, "target")), Value::Int(ring.0 as i64)).unwrap();
        let copied = s.copy_layer(b, LOOK, code).unwrap();
        let target = |s: &Scene, thing: ThingId| {
            let card = s.card(thing, LOOK).unwrap();
            let pasted_cut = card.layers.iter().rev().find(|l| l.parent.is_some()).unwrap().id;
            card.params.get(&layered_key(pasted_cut, "target")).cloned()
        };
        // On the same ball the ring is still there: the pasted cut follows it.
        s.paste_layer(&r, b, LOOK, &copied).unwrap();
        assert_eq!(target(&s, b), Some(Value::Int(ring.0 as i64)));
        // On a ball whose layer with that id is another kind, it follows nothing.
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        let not_a_ring = s.add_layer(&r, other, LOOK, "layer.code").unwrap();
        assert_eq!(not_a_ring, ring);
        s.paste_layer(&r, other, LOOK, &copied).unwrap();
        assert_eq!(target(&s, other), Some(Value::Int(-1)));
        // A single cut pasted onto a layer there behaves the same.
        let cut_only = s.copy_layer(b, LOOK, cut).unwrap();
        s.paste_sub_layer(&r, other, LOOK, not_a_ring, &cut_only).unwrap();
        assert_eq!(target(&s, other), Some(Value::Int(-1)));
    }

    #[test]
    fn a_look_pasted_back_keeps_the_group_layers_choices() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let wash = s.add_layer(&r, b, LOOK, "group.wash").unwrap();
        let chosen = layered_key(wash, "chosen");
        s.card_mut(b, LOOK).unwrap().params.set(&chosen, Value::Text(ring.0.to_string()));
        let look = s.copy_look(&r, b, LOOK).unwrap();
        s.paste_look(&r, b, LOOK, &look).unwrap();
        assert_eq!(s.card(b, LOOK).unwrap().params.get(&chosen), Some(&Value::Text(ring.0.to_string())));
        // A look whose layer with that id is another kind clears the choice.
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        s.add_layer(&r, other, LOOK, "layer.code").unwrap();
        let theirs = s.copy_look(&r, other, LOOK).unwrap();
        s.paste_look(&r, b, LOOK, &theirs).unwrap();
        assert_eq!(s.card(b, LOOK).unwrap().params.get(&chosen), None);
    }

    #[test]
    fn a_layer_can_be_given_to_some_balls_but_the_base_is_everyones() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let pop = s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let wash = s.add_layer(&r, b, LOOK, "group.wash").unwrap();
        s.set_layer_only(&r, b, LOOK, ring, vec![5, 3, 5]).unwrap();
        assert_eq!(s.card(b, LOOK).unwrap().layer(ring).unwrap().only, [3, 5], "sorted, once each");
        assert_eq!(s.set_layer_only(&r, b, LOOK, LayerId(0), vec![1]), Err(EditError::BaseLayer));
        assert_eq!(s.set_layer_only(&r, b, LOOK, pop, vec![1]), Err(EditError::NotAccepted));
        assert_eq!(s.set_layer_only(&r, b, LOOK, wash, vec![1]), Err(EditError::NotAccepted));
        // Duplicates, copies and saves keep it; resolving lists it.
        let copy = s.duplicate_layer(&r, b, LOOK, ring).unwrap();
        assert_eq!(s.card(b, LOOK).unwrap().layer(copy).unwrap().only, [3, 5]);
        assert_eq!(s.copy_layer(b, LOOK, ring).unwrap().only, [3, 5]);
        let card = s.card(b, LOOK).unwrap();
        assert_eq!(ron::from_str::<Slot>(&ron::to_string(card).unwrap()).unwrap(), *card);
        let resolved = crate::resolve(&s, &r);
        let layer = resolved.things[&b].slots[&LOOK].layers.iter().find(|l| l.id == ring).unwrap();
        assert_eq!(layer.only, [3, 5]);
        s.set_layer_only(&r, b, LOOK, ring, Vec::new()).unwrap();
        let layer = s.card(b, LOOK).unwrap().layer(ring).unwrap();
        assert!(!ron::to_string(layer).unwrap().contains("only"), "every ball's writes none");
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

    /// A folder whose Sound card holds chain layers in lanes 0, 0, 1 and one modulator.
    fn sounding() -> (Registry, Scene, ThingId, [LayerId; 4]) {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.add_slot(&r, f, SlotId::Sound, "sound.test").unwrap();
        let m = s.add_layer(&r, f, SlotId::Sound, "mod.test").unwrap();
        let b = s.add_layer_in(&r, f, SlotId::Sound, "chain.test", Some(1)).unwrap();
        let a1 = s.add_layer_in(&r, f, SlotId::Sound, "chain.test", Some(0)).unwrap();
        let a2 = s.add_layer_in(&r, f, SlotId::Sound, "chain.test", Some(0)).unwrap();
        (r, s, f, [a1, a2, b, m])
    }

    fn order(s: &Scene, f: ThingId) -> Vec<(LayerId, Option<u8>)> {
        s.card(f, SlotId::Sound).unwrap().layers.iter().map(|l| (l.id, l.lane)).collect()
    }

    #[test]
    fn chain_layers_sit_in_lanes_before_the_modulators() {
        let (_, s, f, [a1, a2, b, m]) = sounding();
        assert_eq!(order(&s, f), [(a1, Some(0)), (a2, Some(0)), (b, Some(1)), (m, None)]);
    }

    #[test]
    fn moving_a_chain_layer_stays_in_its_lane() {
        let (r, mut s, f, [a1, a2, b, m]) = sounding();
        assert_eq!(s.move_layer(&r, f, SlotId::Sound, b, 0), Err(EditError::NotAccepted));
        assert_eq!(s.move_layer(&r, f, SlotId::Sound, a1, 2), Err(EditError::NotAccepted));
        assert_eq!(s.move_layer(&r, f, SlotId::Sound, m, 2), Err(EditError::NotAccepted));
        s.move_layer(&r, f, SlotId::Sound, a1, 1).unwrap();
        assert_eq!(order(&s, f), [(a2, Some(0)), (a1, Some(0)), (b, Some(1)), (m, None)]);
    }

    #[test]
    fn a_copied_chain_layer_keeps_its_lane() {
        let (r, mut s, f, [_, _, b, _]) = sounding();
        let copied = s.copy_layer(f, SlotId::Sound, b).unwrap();
        let new = s.paste_layer(&r, f, SlotId::Sound, &copied).unwrap();
        let dup = s.duplicate_layer(&r, f, SlotId::Sound, b).unwrap();
        let card = s.card(f, SlotId::Sound).unwrap();
        assert_eq!(card.layer(new).unwrap().lane, Some(1));
        assert_eq!(card.layer(dup).unwrap().lane, Some(1));
    }

    #[test]
    fn removing_a_modulator_drops_its_routes() {
        let (r, mut s, f, [a1, _, _, m]) = sounding();
        let m2 = s.add_layer(&r, f, SlotId::Sound, "mod.test").unwrap();
        let route = |source: LayerId, target: String| crate::ModRoute {
            source,
            target,
            amount: 0.5,
            bipolar: false,
            invert: false,
        };
        let second = route(m2, layered_key(a1, "cutoff"));
        let routes = vec![route(m, "a_gain".into()), second.clone()];
        s.card_mut(f, SlotId::Sound).unwrap().params.set(crate::ROUTES_KEY, Value::Routes(routes));
        let get = |s: &Scene| s.card(f, SlotId::Sound).unwrap().params.get(crate::ROUTES_KEY).cloned();
        s.remove_layer(&r, f, SlotId::Sound, m).unwrap();
        assert_eq!(get(&s), Some(Value::Routes(vec![second])));
        s.remove_layer(&r, f, SlotId::Sound, a1).unwrap();
        assert_eq!(get(&s), Some(Value::Routes(vec![])));
    }

    /// A pulse on some setting of `on` that follows modulator `m` of shape `shape`.
    fn follow(s: &mut Scene, on: ThingId, shape: ThingId, m: LayerId) -> Address {
        let a = Address::on(on, LOOK, "follower");
        let source = crate::PulseSource::Sound { shape, source: crate::SoundSource::Modulator(m) };
        s.pulses.insert(a.clone(), crate::Pulse { source, ..crate::Pulse::default() });
        a
    }

    #[test]
    fn removing_a_look_layer_leaves_a_modulator_with_the_same_number_followed() {
        let (r, mut s, f, _) = sounding();
        // A modulator whose number a Look layer can reach (the first one is 0, the base's).
        let m = s.add_layer(&r, f, SlotId::Sound, "mod.test").unwrap();
        assert!(m.0 > 1);
        s.add_slot(&r, f, LOOK, "look.stack").unwrap();
        // The folder's Look gets layers until one has the modulator's number.
        s.add_layer(&r, f, LOOK, "layer.base").unwrap();
        let mut same = s.add_layer(&r, f, LOOK, "layer.ring").unwrap();
        while same.0 < m.0 {
            same = s.add_layer(&r, f, LOOK, "layer.ring").unwrap();
        }
        assert_eq!(same, m);
        let a = follow(&mut s, f, f, m);
        s.remove_layer(&r, f, LOOK, same).unwrap();
        assert!(s.pulses.contains_key(&a), "the modulator is still there, so is its follower");
        s.remove_layer(&r, f, SlotId::Sound, m).unwrap();
        assert!(!s.pulses.contains_key(&a), "removing the modulator itself ends the following");
    }

    #[test]
    fn a_removed_layers_id_is_never_given_out_again() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.remove_layer(&r, b, LOOK, ring).unwrap();
        let next = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        assert_ne!(next, ring, "a cut or group layer still naming the old one must not get the new one");
        let card = s.card(b, LOOK).unwrap().clone();
        let back: Slot = ron::from_str(&ron::to_string(&card).unwrap()).unwrap();
        assert_eq!(back.next_layer_id(), card.next_layer_id(), "and it stays so after saving");
        // A duplicate of the new layer doesn't take the old id either.
        let dup = s.duplicate_layer(&r, b, LOOK, next).unwrap();
        assert!(dup.0 > next.0);
    }

    #[test]
    fn replacing_a_sound_card_ends_following_its_modulators_but_not_its_level() {
        let (r, mut s, f, [_, _, _, m]) = sounding();
        let a = follow(&mut s, f, f, m);
        let level = Address::on(f, LOOK, "level");
        let source = crate::PulseSource::Sound { shape: f, source: crate::SoundSource::Level };
        s.pulses.insert(level.clone(), crate::Pulse { source, ..crate::Pulse::default() });
        s.add_slot(&r, f, SlotId::Sound, "sound.test").unwrap();
        assert!(!s.pulses.contains_key(&a), "the new card's layer with that number is someone else");
        assert!(s.pulses.contains_key(&level));
    }

    #[test]
    fn trails_and_particles_sit_between_the_drawn_layers_and_the_reactions() {
        let (r, mut s, b) = stacked();
        let pop = s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        let spark = s.add_layer(&r, b, LOOK, "around.spark").unwrap();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let spark2 = s.add_layer(&r, b, LOOK, "around.spark").unwrap();
        s.add_layer(&r, b, LOOK, "group.wash").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Spark", "Spark 2", "Pop", "Wash"]);
        // A sub-layer on a trail rides with it, and a drawn layer's sub stays with its layer.
        let glow = s.add_sub_layer(&r, b, LOOK, spark, "sub.glow").unwrap();
        let cut = s.add_sub_layer(&r, b, LOOK, ring, "sub.cut").unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Cut", "Spark", "Glow", "Spark 2", "Pop", "Wash"]);
        // A new drawn layer goes under the trails, a new trail after the others.
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_layer(&r, b, LOOK, "around.spark").unwrap();
        assert_eq!(
            kinds(&s, b),
            ["Base", "Ring", "Cut", "Ring 2", "Spark", "Glow", "Spark 2", "Spark 3", "Pop", "Wash"]
        );
        let _ = (pop, cut, glow, spark2);
    }

    #[test]
    fn trails_and_particles_move_only_among_themselves_with_their_sub_layers() {
        let (r, mut s, b) = stacked();
        let ring = s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        let a = s.add_layer(&r, b, LOOK, "around.spark").unwrap();
        let c = s.add_layer(&r, b, LOOK, "around.spark").unwrap();
        s.add_layer(&r, b, LOOK, "react.pop").unwrap();
        s.add_sub_layer(&r, b, LOOK, a, "sub.glow").unwrap();
        // Tops from the bottom: Base 0, Ring 1, Spark 2, Spark 2 3.
        assert_eq!(s.move_layer(&r, b, LOOK, a, 1), Err(EditError::NotAccepted), "not among the drawn layers");
        assert_eq!(s.move_layer(&r, b, LOOK, a, 4), Err(EditError::NotAccepted), "not past the last trail");
        assert_eq!(s.move_layer(&r, b, LOOK, ring, 2), Err(EditError::NotAccepted), "a drawn layer stays under them");
        s.move_layer(&r, b, LOOK, a, 3).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Spark 2", "Spark", "Glow", "Pop"]);
        s.move_layer(&r, b, LOOK, a, 2).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Spark", "Glow", "Spark 2", "Pop"]);
        let _ = c;
    }

    #[test]
    fn trails_take_sub_layers_and_a_whole_look_carries_them() {
        let (r, mut s, b) = stacked();
        let spark = s.add_layer(&r, b, LOOK, "around.spark").unwrap();
        s.add_layer(&r, b, LOOK, "layer.ring").unwrap();
        s.add_sub_layer(&r, b, LOOK, spark, "sub.glow").unwrap();
        s.card_mut(b, LOOK).unwrap().params.set(&layered_key(spark, "width"), Value::Number(3.0));
        let look = s.copy_look(&r, b, LOOK).unwrap();
        let other = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, other, LOOK, "look.stack").unwrap();
        s.add_layer(&r, other, LOOK, "layer.base").unwrap();
        s.paste_look(&r, other, LOOK, &look).unwrap();
        assert_eq!(kinds(&s, other), ["Base", "Ring", "Spark", "Glow"]);
        let card = s.card(other, LOOK).unwrap();
        let pasted = card.layers.iter().find(|l| l.name == "Spark").unwrap().id;
        assert_eq!(card.params.get(&layered_key(pasted, "width")), Some(&Value::Number(3.0)));
        // A pasted single trail keeps its sub-layers and goes with the trails.
        let one = s.copy_layer(b, LOOK, spark).unwrap();
        s.paste_layer(&r, other, LOOK, &one).unwrap();
        assert_eq!(kinds(&s, other), ["Base", "Ring", "Spark", "Glow", "Spark 2", "Glow 2"]);
        // A duplicate sits beside it with its own copy of the sub-layer.
        let dup = s.duplicate_layer(&r, b, LOOK, spark).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Spark", "Glow", "Spark copy", "Glow"]);
        assert_eq!(s.card(b, LOOK).unwrap().params.get(&layered_key(dup, "width")), Some(&Value::Number(3.0)));
        // Removing it takes its sub-layer.
        s.remove_layer(&r, b, LOOK, spark).unwrap();
        assert_eq!(kinds(&s, b), ["Base", "Ring", "Spark copy", "Glow"]);
    }
}
