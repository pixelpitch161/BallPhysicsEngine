//! Small helpers that keep each built-in scene's code short and readable. They only call the same edits
//! the editor makes, so a built-in scene is exactly what a person could have built by hand.

use bpe_core::{
    BlockId, Color, LayerId, Params, Registry, RuleId, Scene, Settings, SlotId, ThingId, Value, layered_key,
};
use glam::Vec2;

use crate::ready_looks::{self, Colours, How};
use crate::things::text::looks::{Look, add_text};
use crate::things::text::TextSettings;
use crate::{SceneSettings, add_thing};

/// A colour from 0xRRGGBB.
pub(crate) fn hex(rgb: u32) -> Color {
    Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub(crate) fn n(x: f64) -> Value {
    Value::Number(x)
}

pub(crate) fn int(x: i64) -> Value {
    Value::Int(x)
}

pub(crate) fn choice(name: &str) -> Value {
    Value::Choice(name.into())
}

pub(crate) fn text(words: &str) -> Value {
    Value::Text(words.into())
}

/// Copies the `prefix`ed keys of `params` into their own set, without the prefix.
fn strip(params: &Params, prefix: &str) -> Params {
    let mut out = Params::default();
    for (k, v) in &params.0 {
        if let Some(rest) = k.strip_prefix(prefix) {
            out.set(rest, v.clone());
        }
    }
    out
}

/// Puts `own` back into `params` under `prefix`, dropping what was there (keys at their default stay out).
fn put_back(params: &mut Params, prefix: &str, own: Params) {
    params.0.retain(|k, _| !k.starts_with(prefix));
    for (k, v) in own.0 {
        params.set(&format!("{prefix}{k}"), v);
    }
}

/// Reads `params` as `S`, changes it and writes back: the keys `S` had away from their default are
/// replaced by the new ones; every other key stays.
fn edit_as<S: Settings>(params: &mut Params, f: impl FnOnce(&mut S)) {
    let mut s = S::from_params(params);
    for k in s.to_params().0.keys() {
        params.remove(k);
    }
    f(&mut s);
    for (k, v) in s.to_params().0 {
        params.set(&k, v);
    }
}

/// A built-in scene being made.
pub(crate) struct Lib<'a> {
    pub reg: &'a Registry,
    pub scene: Scene,
}

impl<'a> Lib<'a> {
    /// A blank project (Scene settings and Camera, as the editor starts) `seconds` long.
    pub fn new(reg: &'a Registry, seconds: f64) -> Lib<'a> {
        let mut lib = Lib { reg, scene: crate::new_studio_scene(reg) };
        lib.scene_settings(|s| s.duration = seconds as f32);
        lib
    }

    /// Changes the Scene settings.
    pub fn scene_settings(&mut self, f: impl FnOnce(&mut SceneSettings)) {
        self.params(ThingId::ROOT, f);
    }

    /// A new thing of `kind` named `name` under `parent`, with its kind's starting cards.
    pub fn add(&mut self, kind: &str, parent: ThingId, name: &str) -> ThingId {
        let id = add_thing(&mut self.scene, self.reg, kind, parent).unwrap_or_else(|e| panic!("{kind}: {e}"));
        self.scene.rename_thing(id, name);
        id
    }

    /// Puts `id` at `at`.
    pub fn at(&mut self, id: ThingId, at: Vec2) {
        self.scene.things.get_mut(&id).expect("a thing").transform.position = at;
    }

    /// Changes a thing's own settings, read as `S`. Only settings away from their default are kept, and keys
    /// `S` doesn't know stay as they are.
    pub fn params<S: Settings>(&mut self, id: ThingId, f: impl FnOnce(&mut S)) {
        let thing = self.scene.things.get_mut(&id).expect("a thing");
        edit_as(&mut thing.params, f);
    }

    /// Changes a card's own settings, read as `S` (its layers' settings and keys `S` doesn't know stay).
    pub fn card<S: Settings>(&mut self, id: ThingId, slot: SlotId, f: impl FnOnce(&mut S)) {
        let card = self.scene.card_mut(id, slot).unwrap_or_else(|| panic!("no {slot:?} card"));
        edit_as(&mut card.params, f);
    }

    /// Sets one of a card's own settings.
    pub fn card_set(&mut self, id: ThingId, slot: SlotId, key: &str, v: Value) {
        self.scene.card_mut(id, slot).unwrap_or_else(|| panic!("no {slot:?} card")).params.set(key, v);
    }

    /// Gives `id` its starting card in `slot` (for slots a kind doesn't start with, like Sound or Health).
    pub fn fill(&mut self, id: ThingId, slot: SlotId) {
        crate::fill_slot(&mut self.scene, self.reg, id, slot).unwrap_or_else(|e| panic!("{slot:?}: {e}"));
    }

    /// The first layer of `kind` on the card, if it has one.
    pub fn find_layer(&self, id: ThingId, slot: SlotId, kind: &str) -> Option<LayerId> {
        self.scene.card(id, slot)?.layers.iter().find(|l| l.kind == kind).map(|l| l.id)
    }

    /// A new layer of `kind` on the card.
    pub fn layer(&mut self, id: ThingId, slot: SlotId, kind: &str) -> LayerId {
        self.scene.add_layer(self.reg, id, slot, kind).unwrap_or_else(|e| panic!("{kind}: {e}"))
    }

    /// Changes a layer's settings, read as `S`.
    pub fn layer_params<S: Settings>(&mut self, id: ThingId, slot: SlotId, layer: LayerId, f: impl FnOnce(&mut S)) {
        let prefix = layered_key(layer, "");
        let card = self.scene.card_mut(id, slot).unwrap_or_else(|| panic!("no {slot:?} card"));
        let mut own = strip(&card.params, &prefix);
        edit_as(&mut own, f);
        put_back(&mut card.params, &prefix, own);
    }

    /// Sets one of a layer's settings.
    pub fn layer_set(&mut self, id: ThingId, slot: SlotId, layer: LayerId, key: &str, v: Value) {
        let card = self.scene.card_mut(id, slot).unwrap_or_else(|| panic!("no {slot:?} card"));
        card.params.set(&layered_key(layer, key), v);
    }

    /// Dresses a ball or shape in a ready-made look (its own colours replaced by the look's).
    pub fn look(&mut self, id: ThingId, look: &str) {
        let ready = ready_looks::find(look).unwrap_or_else(|| panic!("no ready-made look {look}"));
        ready_looks::apply(&mut self.scene, self.reg, id, ready, Colours::Look, How::Replace)
            .unwrap_or_else(|e| panic!("{look}: {e}"));
    }

    /// Dresses a ball or shape in a ready-made look in its own colours.
    pub fn look_mine(&mut self, id: ThingId, look: &str) {
        let ready = ready_looks::find(look).unwrap_or_else(|| panic!("no ready-made look {look}"));
        ready_looks::apply(&mut self.scene, self.reg, id, ready, Colours::Mine, How::Replace)
            .unwrap_or_else(|e| panic!("{look}: {e}"));
    }

    /// Gives `id` the sound "Enable sound" adds (the starter voice).
    pub fn sound(&mut self, id: ThingId) {
        let card = crate::sound::starter_voice(self.reg);
        self.scene.things.get_mut(&id).expect("a thing").slots.insert(SlotId::Sound, card);
    }

    /// A rule named `name` whose blocks follow one another.
    pub fn rule(&mut self, name: &str, blocks: &[(&str, Vec<(&str, Value)>)]) -> RuleId {
        let rule = self.scene.add_rule(name);
        let mut last: Option<BlockId> = None;
        for (kind, set) in blocks {
            let id = self.scene.add_block(self.reg, rule, kind, last).unwrap_or_else(|e| panic!("{kind}: {e}"));
            for (k, v) in set {
                self.scene
                    .set_block_param(self.reg, rule, id, k, v.clone())
                    .unwrap_or_else(|e| panic!("{name}: {kind} {k}: {e}"));
            }
            last = Some(id);
        }
        rule
    }

    /// A Text made from one of the Text's own looks (a hook title, a hit counter), named and with its words.
    pub fn text(&mut self, look: Look, name: &str, words: &str) -> ThingId {
        let id = add_text(&mut self.scene, self.reg, look, ThingId::ROOT).unwrap_or_else(|e| panic!("{name}: {e}"));
        self.scene.rename_thing(id, name);
        self.params(id, |s: &mut TextSettings| s.words = words.into());
        id
    }

    pub fn finish(self) -> Scene {
        self.scene
    }
}
