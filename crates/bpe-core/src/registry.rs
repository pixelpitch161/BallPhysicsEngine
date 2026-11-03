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
    /// The editor track colour a new one of these starts with, when the kind has its own (the
    /// arena is orange); `None` takes the next free colour.
    fn track_colour(&self) -> Option<crate::TrackColour> {
        None
    }
    /// The heading the editor's "Add thing" menu lists this kind under.
    fn family(&self) -> &'static str {
        "Other"
    }
    /// What the editor calls `slot` on one of these, such as "Effects" for a shader.
    fn slot_label(&self, slot: SlotId) -> &'static str {
        slot.name()
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

    /// The card kinds that fill `slot` on `thing`: the ones that fit it,
    /// in id order.
    pub fn kinds_fitting<'a>(
        &'a self,
        slot: SlotId,
        thing: &'a dyn ThingKind,
    ) -> impl Iterator<Item = &'a dyn ModuleKind> {
        self.modules().filter(move |m| m.slot() == slot && m.fits(thing.id()))
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
