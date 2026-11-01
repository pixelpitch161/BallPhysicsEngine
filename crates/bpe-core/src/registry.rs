//! The plugin registry: every kind of thing and module the editor knows about.

use crate::{Params, SettingDef, SlotId};
use std::collections::BTreeMap;

/// A kind of thing that can sit in the scene tree, such as balls, a folder or an arena.
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
    /// Whether a scene has at most one of these, which can't be deleted or duplicated.
    fn single(&self) -> bool {
        false
    }
    /// The heading the editor's "Add thing" menu lists this kind under.
    fn family(&self) -> &'static str {
        "Other"
    }
    /// Whether the things under one of these use its modules for slots they don't fill
    /// themselves. A kind that says no keeps its slots to itself, like Scene settings.
    fn passes_down(&self) -> bool {
        true
    }
    /// What the editor calls `slot` on one of these, such as "Effects" for a shader.
    fn slot_label(&self, slot: SlotId) -> &'static str {
        slot.name()
    }
    /// Whether a module of `module_kind` may fill `slot` on one of these.
    fn accepts(&self, _slot: SlotId, _module_kind: &str) -> bool {
        true
    }
}

/// A kind of module that fills one slot, such as a plain look or a pluck sound.
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
    /// The node graph this module opens as at Nodes depth, if it has one.
    fn template(&self) -> Option<bpe_graph::Graph> {
        None
    }
    /// Whether this kind makes sense on things of `thing_kind`, so the editor lists only the
    /// kinds that fit. Most fit every thing.
    fn fits(&self, _thing_kind: &str) -> bool {
        true
    }
}

/// Every registered kind, by id.
#[derive(Default)]
pub struct Registry {
    things: BTreeMap<&'static str, Box<dyn ThingKind>>,
    modules: BTreeMap<&'static str, Box<dyn ModuleKind>>,
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

    fn check_new(&self, id: &str) {
        if self.things.contains_key(id) || self.modules.contains_key(id) {
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

    /// The module kinds that fill `slot` on `thing`: ones the thing accepts and that fit it,
    /// in id order.
    pub fn kinds_fitting<'a>(
        &'a self,
        slot: SlotId,
        thing: &'a dyn ThingKind,
    ) -> impl Iterator<Item = &'a dyn ModuleKind> {
        self.modules().filter(move |m| m.slot() == slot && thing.accepts(slot, m.id()) && m.fits(thing.id()))
    }

    /// The values of `params` that module kind `to` also has, with the same type: what a module
    /// keeps when it becomes another kind, such as a look's colour and glow. An unknown kind
    /// keeps nothing.
    pub fn carry_over(&self, params: &Params, to: &str) -> Params {
        let Some(kind) = self.module(to) else {
            return Params::default();
        };
        let mut kept = Params::default();
        for def in kind.settings() {
            if let Some(value) = params.get(def.key).filter(|v| def.ty.accepts(v)) {
                kept.set(def.key, value.clone());
            }
        }
        kept
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
            &[SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Sound]
        );
        assert_eq!(r.modules_for(SlotId::Look).map(|m| m.id()).collect::<Vec<_>>(), ["look.plain"]);
        assert!(r.thing("magnet").is_none());
    }

    #[test]
    #[should_panic(expected = "plugin id registered twice: balls")]
    fn duplicate_ids_panic() {
        let mut r = testkit::registry();
        r.add_thing(testkit::BallsKind);
    }
}
