//! The plugin registry: every kind of thing and module the editor knows about.

use crate::{SettingDef, SlotId};
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
