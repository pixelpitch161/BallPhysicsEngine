//! The scene: a tree of things, and the library of modules they link to.

use crate::{KindId, Library, ModuleId, Params, Registry, SlotId, ThingId};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_true(v: &bool) -> bool {
    *v
}

/// Which things something points at: a sound that plays on hitting walls, a rule about red balls.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Target {
    /// The thing itself.
    Me,
    Thing(ThingId),
    /// Every thing with this tag.
    Tag(String),
    /// Every thing of this kind.
    Kind(KindId),
    #[default]
    Anything,
}

/// Where a thing sits and how it is turned, relative to its parent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub position: Vec2,
    pub rotation: f32,
}

/// How a thing hangs off its parent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Attach {
    /// Only grouped: it inherits settings but moves on its own.
    #[default]
    Group,
    /// It rides on the parent, moving with it.
    Ride {
        offset: Vec2,
        turn_with_parent: bool,
        /// Which of the parent's items it rides on (one ball of many), if not the whole parent.
        item: Option<u32>,
    },
}

/// What fills one slot on a thing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum SlotLink {
    /// Whatever the parent uses.
    #[default]
    Inherit,
    /// A library module, with this thing's own changes on top.
    Linked {
        module: ModuleId,
        #[serde(default, skip_serializing_if = "Params::is_empty")]
        overrides: Params,
    },
    /// Nothing, and the parent's module isn't used either.
    Off,
}

/// One thing in the tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Thing {
    pub id: ThingId,
    pub name: String,
    /// The thing kind's plugin id.
    pub kind: KindId,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub tags: BTreeSet<String>,
    /// `None` only for Scene defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ThingId>,
    #[serde(skip_serializing_if = "is_default")]
    pub attach: Attach,
    #[serde(skip_serializing_if = "is_default")]
    pub transform: Transform,
    #[serde(skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Values that differ from the kind's defaults.
    #[serde(skip_serializing_if = "Params::is_empty")]
    pub params: Params,
    /// Slots shown in the inspector. A slot left out behaves as `Inherit`.
    pub slots: BTreeMap<SlotId, SlotLink>,
}

impl Default for Thing {
    fn default() -> Self {
        Self {
            id: ThingId::ROOT,
            name: String::new(),
            kind: KindId::new(),
            tags: BTreeSet::new(),
            parent: None,
            attach: Attach::Group,
            transform: Transform::default(),
            enabled: true,
            params: Params::default(),
            slots: BTreeMap::new(),
        }
    }
}

/// A whole scene, as saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub format: u32,
    pub name: String,
    pub things: BTreeMap<ThingId, Thing>,
    /// Every thing once, root first, each thing before its children and siblings in tree order.
    pub order: Vec<ThingId>,
    pub library: Library,
    /// The id the next added thing gets.
    pub next_thing: u32,
    /// The id the next added module gets.
    pub next_module: u32,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            format: Scene::FORMAT,
            name: String::new(),
            things: BTreeMap::new(),
            order: Vec::new(),
            library: Library::default(),
            next_thing: 1,
            next_module: 0,
        }
    }
}

impl Scene {
    /// The save format this version writes.
    pub const FORMAT: u32 = 2;

    /// A scene holding only Scene defaults. Panics if `reg` has no `"scene"` kind.
    pub fn empty(reg: &Registry) -> Scene {
        let kind = reg.thing("scene").expect("the registry needs a \"scene\" thing kind");
        let root = Thing {
            id: ThingId::ROOT,
            name: "Scene defaults".into(),
            kind: "scene".into(),
            slots: kind.default_slots().iter().map(|&s| (s, SlotLink::Inherit)).collect(),
            ..Thing::default()
        };
        Scene { things: BTreeMap::from([(ThingId::ROOT, root)]), order: vec![ThingId::ROOT], ..Scene::default() }
    }

    /// The things directly under `id`, in tree order.
    pub fn children(&self, id: ThingId) -> Vec<ThingId> {
        self.order.iter().copied().filter(|c| self.things.get(c).is_some_and(|t| t.parent == Some(id))).collect()
    }

    /// The parent, its parent and so on up to the root, nearest first.
    pub fn ancestors(&self, id: ThingId) -> Vec<ThingId> {
        let mut out = Vec::new();
        let mut at = self.things.get(&id).and_then(|t| t.parent);
        while let Some(p) = at {
            // A hand-edited loop must not hang: no real chain is longer than the scene.
            if out.len() >= self.things.len() {
                break;
            }
            out.push(p);
            at = self.things.get(&p).and_then(|t| t.parent);
        }
        out
    }

    /// The things that link module `m` in any slot, in tree order.
    pub fn users_of(&self, m: ModuleId) -> Vec<ThingId> {
        self.order
            .iter()
            .copied()
            .filter(|id| {
                self.things.get(id).is_some_and(|t| {
                    t.slots.values().any(|l| matches!(l, SlotLink::Linked { module, .. } if *module == m))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    #[test]
    fn empty_scene_has_only_the_root() {
        let s = Scene::empty(&testkit::registry());
        assert_eq!(s.order, vec![ThingId::ROOT]);
        assert_eq!(s.things[&ThingId::ROOT].name, "Scene defaults");
        assert_eq!(s.format, 2);
        assert_eq!(s.next_thing, 1);
    }

    #[test]
    fn ancestors_are_nearest_first() {
        let mut s = Scene::empty(&testkit::registry());
        let a = testkit::push(&mut s, "folder", ThingId::ROOT);
        let b = testkit::push(&mut s, "balls", a);
        assert_eq!(s.ancestors(b), vec![a, ThingId::ROOT]);
        assert_eq!(s.children(ThingId::ROOT), vec![a]);
    }
}
