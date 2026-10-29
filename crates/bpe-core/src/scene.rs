//! The scene: a tree of things, and the library of modules they link to.

use crate::{Address, KindId, Library, ModuleId, Params, Pulse, Registry, SlotId, ThingId, Track};
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
    /// Keyframes, by the setting they move.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<Address, Track>,
    /// Settings that pulse with the music or the hits.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pulses: BTreeMap<Address, Pulse>,
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
            tracks: BTreeMap::new(),
            pulses: BTreeMap::new(),
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

    /// Puts a scene that was edited by hand back in shape, so every later step can trust it: ids
    /// match their map keys, Scene defaults is the root with no parent, things whose parent is
    /// missing or part of a loop move under Scene defaults, and `order` lists every thing once with
    /// each thing after its parent, keeping the old order among siblings. Returns `false` when there
    /// is no Scene defaults to hang the tree on; the scene is then unusable.
    pub fn mend(&mut self) -> bool {
        for (id, t) in &mut self.things {
            t.id = *id;
        }
        for (id, m) in &mut self.library.modules {
            m.id = *id;
        }
        let Some(root) = self.things.get_mut(&ThingId::ROOT) else { return false };
        root.parent = None;
        root.kind = "scene".into();

        let ids: Vec<ThingId> = self.things.keys().copied().collect();
        for &id in &ids {
            if id != ThingId::ROOT && self.things[&id].parent.is_none_or(|p| !self.things.contains_key(&p)) {
                self.things.get_mut(&id).expect("listed above").parent = Some(ThingId::ROOT);
            }
        }

        // Anything the walk down from the root can't reach hangs off a loop. Walking up from such
        // a thing must repeat itself, and the repeated thing is on the loop: cut it there.
        loop {
            let reached = self.reachable();
            let Some(&start) = ids.iter().find(|id| !reached.contains(id)) else { break };
            let mut seen = BTreeSet::new();
            let mut at = start;
            while seen.insert(at) {
                at = self.things[&at].parent.expect("only the root has no parent now");
            }
            self.things.get_mut(&at).expect("walked through it").parent = Some(ThingId::ROOT);
        }

        let rank: BTreeMap<ThingId, usize> = self.order.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        let mut children: BTreeMap<ThingId, Vec<ThingId>> = BTreeMap::new();
        for t in self.things.values() {
            if let Some(p) = t.parent {
                children.entry(p).or_default().push(t.id);
            }
        }
        for kids in children.values_mut() {
            kids.sort_by_key(|k| (rank.get(k).copied().unwrap_or(usize::MAX), *k));
        }
        let mut order = Vec::with_capacity(self.things.len());
        let mut stack = vec![ThingId::ROOT];
        while let Some(id) = stack.pop() {
            order.push(id);
            if let Some(kids) = children.get(&id) {
                stack.extend(kids.iter().rev());
            }
        }
        self.order = order;
        true
    }

    /// Every thing the parent links lead to from the root.
    fn reachable(&self) -> BTreeSet<ThingId> {
        let mut children: BTreeMap<ThingId, Vec<ThingId>> = BTreeMap::new();
        for t in self.things.values() {
            if let Some(p) = t.parent {
                children.entry(p).or_default().push(t.id);
            }
        }
        let mut reached = BTreeSet::new();
        let mut stack = vec![ThingId::ROOT];
        while let Some(id) = stack.pop() {
            if reached.insert(id)
                && let Some(kids) = children.get(&id)
            {
                stack.extend(kids);
            }
        }
        reached
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
    fn mend_repairs_a_hand_edited_tree() {
        let mut s = Scene::empty(&testkit::registry());
        let a = testkit::push(&mut s, "folder", ThingId::ROOT);
        let b = testkit::push(&mut s, "balls", a);
        let c = testkit::push(&mut s, "balls", ThingId::ROOT);
        let d = testkit::push(&mut s, "balls", ThingId::ROOT);
        let e = testkit::push(&mut s, "balls", d);
        // Ids that disagree with their keys, an orphan, a loop (d <-> e), a child listed before
        // its parent, a duplicate and a stranger in `order`, and a missing entry.
        s.things.get_mut(&b).unwrap().id = ThingId(77);
        s.things.get_mut(&c).unwrap().parent = Some(ThingId(999));
        s.things.get_mut(&d).unwrap().parent = Some(e);
        s.order = vec![b, ThingId::ROOT, a, a, ThingId(555), c];
        assert!(s.mend());
        assert_eq!(s.things[&b].id, b);
        assert_eq!(s.things[&c].parent, Some(ThingId::ROOT));
        // The loop is cut at the thing the upward walk repeats, which keeps the rest of it.
        let (dp, ep) = (s.things[&d].parent, s.things[&e].parent);
        assert!((dp == Some(ThingId::ROOT) && ep == Some(d)) || (ep == Some(ThingId::ROOT) && dp == Some(e)));
        assert_eq!(s.order.len(), 6);
        assert_eq!(&s.order[..4], &[ThingId::ROOT, a, b, c]);
        for id in &s.order {
            let parent = s.things[id].parent;
            assert!(parent.is_none_or(|p| s.order.iter().position(|&x| x == p) < s.order.iter().position(|x| x == id)));
        }
    }

    #[test]
    fn mend_refuses_a_scene_without_defaults() {
        let mut s = Scene::default();
        assert!(!s.mend());
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
