//! The plugin registry: every kind of thing and module the editor knows about.

use crate::{SettingDef, SlotId};
use std::collections::BTreeMap;

/// A kind of thing that can sit in the scene tree, such as balls, a folder or a shape.
pub trait ThingKind: Send + Sync + 'static {
    /// Unique among every thing and module kind, and written into saved scenes: `"balls"`.
    fn id(&self) -> &'static str;
    /// What the editor calls one of these: `"Balls"`.
    fn name(&self) -> &'static str;
    /// The name of the icon shown next to it in the tree.
    fn icon(&self) -> &'static str;
    /// One plain sentence for the hover help.
    fn help(&self) -> &'static str;
    /// Every setting one of these has, in the order the inspector shows them.
    fn settings(&self) -> &'static [SettingDef];
    /// Slots this kind can fill.
    fn slots(&self) -> &'static [SlotId];
    /// Slots a newly added thing of this kind shows straight away.
    fn default_slots(&self) -> &'static [SlotId];
    /// Whether one of these is drawn on the screen instead of in the world unless it says
    /// otherwise, such as a caption.
    fn sticks_to_screen(&self) -> bool {
        false
    }
    /// Whether one of these may be stuck to the screen at all. A kind that says no is always
    /// drawn in the world, whatever its own stick-to-screen choice says, such as a canvas.
    fn can_stick_to_screen(&self) -> bool {
        true
    }
    /// Whether a scene has at most one of these, which can't be deleted or duplicated.
    fn single(&self) -> bool {
        false
    }
    /// The heading the editor's "Add thing" menu lists this kind under.
    fn family(&self) -> &'static str {
        "Other"
    }
    /// What the editor calls `slot` on one of these, such as "Effects" for a shader.
    fn slot_label(&self, slot: SlotId) -> &'static str {
        slot.name()
    }
    /// Whether the thing's own setting `key` can vary across its balls, and how it is read
    /// (see [`crate::vary`]). Most can't.
    fn vary(&self, _key: &str) -> Option<crate::VaryUse> {
        None
    }
}

/// A kind of card that fills one of a thing's slots, such as a look or one effect. Each thing
/// owns its cards; the kind only describes the settings and does the work.
pub trait ModuleKind: Send + Sync + 'static {
    /// Unique among every thing and module kind, and written into saved scenes: `"look.plain"`.
    fn id(&self) -> &'static str;
    /// What the editor calls one of these: `"Plain"`.
    fn name(&self) -> &'static str;
    /// One plain sentence for the hover help.
    fn help(&self) -> &'static str;
    fn slot(&self) -> SlotId;
    /// Every setting one of these has, in the order the inspector shows them.
    fn settings(&self) -> &'static [SettingDef];
    /// Whether this kind makes sense on things of `thing_kind`, so the editor lists only the
    /// kinds that fit. Most fit every thing.
    fn fits(&self, _thing_kind: &str) -> bool {
        true
    }
    /// Whether this card's setting `key` can vary across the balls of the thing it is on, and
    /// how it is read (see [`crate::vary`]). Most can't.
    fn vary(&self, _key: &str) -> Option<crate::VaryUse> {
        None
    }
}

/// Where a layer sits in a card's stack: with the drawn layers, painted bottom to top, with
/// the trails and particles round the ball (`Around`, after the drawn layers), with
/// the reactions, which change the whole finished stack when something happens, as a sub-layer
/// of the drawn layer it changes (its `Layer::parent`), or with the group layers, which spread
/// one look across every ball of the thing (applied in their order, after the reactions and
/// sub-layers in the card's list). A sound card's layers are chain layers (in an oscillator's lane,
/// top to bottom), modulators (in tile order, after the chains), and a mixer's are bus effects
/// (in a bus's lane); see [`crate::Layer::lane`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerSection {
    Drawn,
    /// Trails and particles: drawn round the ball, not on it. They sit after the drawn layers
    /// (and their sub-layers), before the reactions, and move among themselves.
    Around,
    Reaction,
    Sub,
    Group,
    Chain,
    Modulator,
    Effect,
}

/// A kind of layer in a card that holds a stack of them (the balls' Look), such as an outline
/// or a flash on each hit. Each layer in a card owns its settings, stored in the card's params
/// under layered keys (see [`crate::layered_key`]).
pub trait LayerKind: Send + Sync + 'static {
    /// Unique among every thing, module and layer kind, and written into saved scenes:
    /// `"layer.outline"`.
    fn id(&self) -> &'static str;
    /// What the editor calls one of these, and a new one's name: `"Outline"`.
    fn name(&self) -> &'static str;
    /// One plain sentence for the hover help.
    fn help(&self) -> &'static str;
    /// Every setting one of these has, in the order the editor shows them.
    fn settings(&self) -> &'static [SettingDef];
    fn section(&self) -> LayerSection;
    /// Whether a card may hold more than one of these.
    fn repeats(&self) -> bool;
    /// The card kind this layer goes in: `"look.layers"`.
    fn card(&self) -> &'static str;
    /// Whether this is the card's base layer: always the bottom one, exactly one, never removed,
    /// moved, copied or hidden (the balls' Body).
    fn base(&self) -> bool {
        false
    }
    /// Whether layers of this kind may carry sub-layers (a drawn or around layer that isn't the
    /// base layer).
    fn takes_subs(&self) -> bool {
        true
    }
    /// Whether a layer of this kind takes a sub-layer of kind `sub` (asked only when
    /// [`LayerKind::takes_subs`] says yes).
    fn takes_sub(&self, _sub: &str) -> bool {
        true
    }
    /// Whether some balls of a group can wear this drawn layer alone (`Layer.only`). A layer
    /// that sets how the whole finished ball mixes with what is behind it can't.
    fn per_ball(&self) -> bool {
        true
    }
    /// Settings a layer of this kind has beyond `settings()` because of its own values, such as
    /// the settings named in a Shader layer's own code. `get` reads the layer's values by plain
    /// key, defaults included. Each distinct set is made once and kept for the program's life.
    fn dynamic_defs(&self, _get: &dyn Fn(&str) -> Option<crate::Value>) -> &'static [SettingDef] {
        &[]
    }
    /// Whether this layer's setting `def` can vary across the balls that wear it, and how it is
    /// read (see [`crate::vary`]). Most can't; a ball layer's numbers, pairs, colours and
    /// gradients usually can, read while the ball lives.
    fn vary(&self, _def: &SettingDef) -> Option<crate::VaryUse> {
        None
    }
    /// How a layer of this kind takes the scene's lights (its Lighting fold: Lit, Blocks light
    /// and, on a ball's Body, Roundness), when it takes them at all. Shown after its own settings.
    fn light_defs(&self) -> &'static [SettingDef] {
        &[]
    }
}

/// The description of a layer of `kind`'s setting `key`: one of its kind's settings, or one of
/// those its own values add (`LayerKind::dynamic_defs`). `get` reads the layer's values by plain
/// key; unset ones are read as their defaults.
pub fn layer_def(
    kind: &dyn LayerKind,
    get: &dyn Fn(&str) -> Option<crate::Value>,
    key: &str,
) -> Option<&'static SettingDef> {
    if let Some((base, part)) = crate::vary::split(key) {
        return crate::vary::def_for(layer_def(kind, get, base), part);
    }
    let fixed = kind.settings();
    if let Some(d) = fixed.iter().find(|d| d.key == key) {
        return Some(d);
    }
    if let Some(d) = kind.light_defs().iter().find(|d| d.key == key) {
        return Some(d);
    }
    let with_defaults = |k: &str| get(k).or_else(|| fixed.iter().find(|d| d.key == k).map(|d| d.default.clone()));
    kind.dynamic_defs(&with_defaults).iter().find(|d| d.key == key)
}

/// A fix to a scene loaded from an older format.
pub type Upgrade = fn(&mut crate::Scene);

/// Every registered kind, by id.
#[derive(Default)]
pub struct Registry {
    things: BTreeMap<&'static str, Box<dyn ThingKind>>,
    modules: BTreeMap<&'static str, Box<dyn ModuleKind>>,
    /// In the order they were registered, which is the order the editor lists them in.
    layers: Vec<Box<dyn LayerKind>>,
    /// Scene fixes for files older than a format, in the order they were added (see
    /// [`Registry::add_upgrade`]).
    upgrades: Vec<(u32, Upgrade)>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Panics if a thing or module kind with the same id is already registered.
    pub fn add_thing(&mut self, k: impl ThingKind) {
        let id = k.id();
        self.check_new(id);
        self.things.insert(id, Box::new(k));
    }

    /// Panics if a thing or module kind with the same id is already registered.
    pub fn add_module(&mut self, k: impl ModuleKind) {
        let id = k.id();
        self.check_new(id);
        self.modules.insert(id, Box::new(k));
    }

    /// Panics if a kind with the same id is already registered.
    pub fn add_layer(&mut self, k: impl LayerKind) {
        let id = k.id();
        self.check_new(id);
        self.layers.push(Box::new(k));
    }

    /// Adds a fix that [`crate::load`] runs on a scene whose file format is below `below`, after
    /// mending it and before cleaning its settings. Fixes run in the order they were added.
    pub fn add_upgrade(&mut self, below: u32, f: Upgrade) {
        self.upgrades.push((below, f));
    }

    /// The fixes a file of `format` needs, in order.
    pub fn upgrades_for(&self, format: u32) -> impl Iterator<Item = fn(&mut crate::Scene)> + '_ {
        self.upgrades.iter().filter(move |(below, _)| format < *below).map(|(_, f)| *f)
    }

    fn check_new(&self, id: &str) {
        if self.things.contains_key(id) || self.modules.contains_key(id) || self.layer(id).is_some() {
            panic!("plugin id registered twice: {id}");
        }
    }

    /// The thing kind with this id.
    pub fn thing(&self, id: &str) -> Option<&dyn ThingKind> {
        self.things.get(id).map(|k| &**k)
    }

    /// The module kind with this id.
    pub fn module(&self, id: &str) -> Option<&dyn ModuleKind> {
        self.modules.get(id).map(|k| &**k)
    }

    /// The layer kind with this id.
    pub fn layer(&self, id: &str) -> Option<&dyn LayerKind> {
        self.layers.iter().find(|k| k.id() == id).map(|k| &**k)
    }

    /// Every layer kind, in the order they were registered.
    pub fn layers(&self) -> impl Iterator<Item = &dyn LayerKind> {
        self.layers.iter().map(|k| &**k)
    }

    /// The layer kinds that go in cards of kind `card`, in the order they were registered.
    pub fn layers_for<'a>(&'a self, card: &'a str) -> impl Iterator<Item = &'a dyn LayerKind> {
        self.layers().filter(move |k| k.card() == card)
    }

    /// Every thing kind, in id order.
    pub fn things(&self) -> impl Iterator<Item = &dyn ThingKind> {
        self.things.values().map(|k| &**k)
    }

    /// Every module kind, in id order.
    pub fn modules(&self) -> impl Iterator<Item = &dyn ModuleKind> {
        self.modules.values().map(|k| &**k)
    }

    /// The module kinds that fill `slot`, in id order.
    pub fn modules_for(&self, slot: SlotId) -> impl Iterator<Item = &dyn ModuleKind> {
        self.modules().filter(move |m| m.slot() == slot)
    }

    /// The card kinds that fill `slot` on `thing`: the ones that fit it,
    /// in id order.
    pub fn kinds_fitting<'a>(
        &'a self,
        slot: SlotId,
        thing: &'a dyn ThingKind,
    ) -> impl Iterator<Item = &'a dyn ModuleKind> {
        let has = thing.slots().contains(&slot);
        self.modules().filter(move |m| has && m.slot() == slot && m.fits(thing.id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    #[test]
    fn registry_finds_kinds_by_id_and_slot() {
        let r = testkit::registry();
        assert_eq!(
            r.thing("balls").unwrap().default_slots(),
            &[SlotId::Look, SlotId::Trail, SlotId::Motion, SlotId::Bounce]
        );
        assert_eq!(r.modules_for(SlotId::Look).map(|m| m.id()).collect::<Vec<_>>(), ["look.plain", "look.stack"]);
        assert!(r.thing("magnet").is_none());
    }

    #[test]
    #[should_panic(expected = "plugin id registered twice: balls")]
    fn duplicate_ids_panic() {
        let mut r = testkit::registry();
        r.add_thing(testkit::BallsKind);
    }
}
