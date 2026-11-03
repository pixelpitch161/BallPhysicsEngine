//! The scene: a tree of things, each with its own cards of settings.

use crate::{Address, KindId, Layer, LayerId, Params, Pulse, Registry, SlotId, ThingId, Track};
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

/// The colour that marks a thing in the editor (its tree tag, timeline group and selection), like
/// a track colour in a DAW. Only the editor reads it; it never changes the video.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackColour {
    Pink,
    Cyan,
    Lime,
    Orange,
    Violet,
    Yellow,
    Red,
    Teal,
}

impl TrackColour {
    /// Every colour, in the order new things take them.
    pub const ALL: [TrackColour; 8] = [
        TrackColour::Pink,
        TrackColour::Cyan,
        TrackColour::Lime,
        TrackColour::Orange,
        TrackColour::Violet,
        TrackColour::Yellow,
        TrackColour::Red,
        TrackColour::Teal,
    ];

    /// What the editor calls it: "Pink".
    pub fn name(self) -> &'static str {
        match self {
            TrackColour::Pink => "Pink",
            TrackColour::Cyan => "Cyan",
            TrackColour::Lime => "Lime",
            TrackColour::Orange => "Orange",
            TrackColour::Violet => "Violet",
            TrackColour::Yellow => "Yellow",
            TrackColour::Red => "Red",
            TrackColour::Teal => "Teal",
        }
    }
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

/// One of a thing's own cards of settings, such as its Look or its Trail: which kind of card it
/// is and the values that differ from that kind's defaults. Nothing else shares it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    /// The card kind's plugin id, such as `"look.basic"`.
    pub kind: KindId,
    /// Values that differ from the kind's defaults.
    #[serde(skip_serializing_if = "Params::is_empty")]
    pub params: Params,
    /// What earlier kinds of this card left behind, by kind, to restore if it switches back.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stash: BTreeMap<KindId, SlotStash>,
    /// The card's stack of layers, bottom first, then its reactions, for a card kind that holds
    /// layers (the balls' Look). Each layer's settings sit in `params` under layered keys.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<Layer>,
}

impl Slot {
    /// A card of `kind` with every setting at its default.
    pub fn new(kind: impl Into<KindId>) -> Slot {
        Slot { kind: kind.into(), ..Slot::default() }
    }

    /// The layer with this id.
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// Where the layer with this id sits in the stack.
    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// The id a new layer gets: one more than the largest in the card, 0 for the first.
    pub fn next_layer_id(&self) -> LayerId {
        LayerId(self.layers.iter().map(|l| l.id.0 + 1).max().unwrap_or(0))
    }
}

/// What a card set aside when it changed kind: the settings, keyframes and pulses its new kind has
/// no use for, by setting key. Switching back to the old kind puts them back.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotStash {
    pub params: Params,
    pub tracks: BTreeMap<String, Track>,
    pub pulses: BTreeMap<String, Pulse>,
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
    /// `None` only for the scene settings.
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
    /// The thing's own cards of settings. A slot left out isn't there.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<SlotId, Slot>,
    /// Drawn on the screen instead of in the world, whatever the camera does. `None` is the
    /// kind's own choice ([`crate::ThingKind::sticks_to_screen`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<bool>,
    /// The thing's colour in the editor. `None` only for the scene settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<TrackColour>,
}

impl Thing {
    /// Whether this thing sits on its parent instead of keeping its own place.
    pub fn rides(&self) -> bool {
        matches!(self.attach, Attach::Ride { .. })
    }

    /// Whether it is drawn on the screen rather than in the world; never for a kind that
    /// [can't stick to the screen](crate::ThingKind::can_stick_to_screen).
    pub fn sticks_to_screen(&self, kind: &dyn crate::ThingKind) -> bool {
        kind.can_stick_to_screen() && self.screen.unwrap_or_else(|| kind.sticks_to_screen())
    }
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
            screen: None,
            colour: None,
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
    /// The id the next added thing gets.
    pub next_thing: u32,
    /// Keyframes, by the setting they move.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<Address, Track>,
    /// Settings that pulse with the hits.
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
            next_thing: 1,
            tracks: BTreeMap::new(),
            pulses: BTreeMap::new(),
        }
    }
}

impl Scene {
    /// The save format this version writes.
    pub const FORMAT: u32 = 5;

    /// A scene holding only the scene settings. Panics if `reg` has no `"scene"` kind.
    pub fn empty(reg: &Registry) -> Scene {
        let kind = reg.thing("scene").expect("the registry needs a \"scene\" thing kind");
        let root = Thing { id: ThingId::ROOT, name: kind.name().into(), kind: "scene".into(), ..Thing::default() };
        Scene { things: BTreeMap::from([(ThingId::ROOT, root)]), order: vec![ThingId::ROOT], ..Scene::default() }
    }

    /// The colour a new thing takes: the first nobody uses yet, or once all are taken, the one
    /// the fewest things use (earliest first on a tie).
    pub fn next_colour(&self) -> TrackColour {
        let used = |c: TrackColour| self.things.values().filter(|t| t.colour == Some(c)).count();
        TrackColour::ALL.into_iter().min_by_key(|&c| used(c)).expect("there are eight colours")
    }

    /// The colour a new thing of `kind` takes: the kind's own when nobody has it yet, else the
    /// next free one.
    pub fn colour_for(&self, kind: Option<&dyn crate::ThingKind>) -> TrackColour {
        match kind.and_then(|k| k.track_colour()) {
            Some(own) if !self.things.values().any(|t| t.colour == Some(own)) => own,
            _ => self.next_colour(),
        }
    }

    /// Gives every thing but the scene settings a colour if it has none, in tree order (scenes
    /// saved before colours existed), and takes the scene settings' away.
    pub fn give_colours(&mut self, reg: &Registry) {
        if let Some(root) = self.things.get_mut(&ThingId::ROOT) {
            root.colour = None;
        }
        for id in self.order.clone() {
            let Some(thing) = self.things.get(&id) else { continue };
            if id == ThingId::ROOT || thing.colour.is_some() {
                continue;
            }
            let colour = self.colour_for(reg.thing(&thing.kind));
            self.things.get_mut(&id).expect("looked up above").colour = Some(colour);
        }
    }

    /// The thing's card in `slot`, if it has one.
    pub fn card(&self, id: ThingId, slot: SlotId) -> Option<&Slot> {
        self.things.get(&id)?.slots.get(&slot)
    }

    /// The thing's card in `slot`, for changing, if it has one.
    pub fn card_mut(&mut self, id: ThingId, slot: SlotId) -> Option<&mut Slot> {
        self.things.get_mut(&id)?.slots.get_mut(&slot)
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

    /// Everything under `id` in tree order, not `id` itself.
    pub fn descendants(&self, id: ThingId) -> Vec<ThingId> {
        let Some(start) = self.order.iter().position(|&t| t == id) else { return Vec::new() };
        let mut inside = BTreeSet::from([id]);
        let mut out = Vec::new();
        for &t in &self.order[start + 1..] {
            match self.things.get(&t).and_then(|t| t.parent) {
                Some(p) if inside.contains(&p) => {
                    inside.insert(t);
                    out.push(t);
                }
                _ => break,
            }
        }
        out
    }

    /// The parent of the first listed thing that has one.
    pub fn common_parent(&self, ids: &[ThingId]) -> Option<ThingId> {
        ids.iter().find_map(|id| self.things.get(id).and_then(|t| t.parent))
    }

    /// Whether `other` is one of the things `target` points at, seen from `me`. A thing that is
    /// not in the scene matches only `Anything`.
    pub fn target_matches(&self, target: &Target, me: ThingId, other: ThingId) -> bool {
        match target {
            Target::Anything => true,
            Target::Me => other == me && self.things.contains_key(&other),
            Target::Thing(t) => other == *t && self.things.contains_key(&other),
            Target::Tag(tag) => self.things.get(&other).is_some_and(|t| t.tags.contains(tag)),
            Target::Kind(kind) => self.things.get(&other).is_some_and(|t| t.kind == *kind),
        }
    }

    /// Puts a scene that was edited by hand back in shape, so every later step can trust it: ids
    /// match their map keys, the scene settings are the root with no parent, things whose parent is
    /// missing or part of a loop move under the scene settings, and `order` lists every thing once with
    /// each thing after its parent, keeping the old order among siblings. Returns `false` when there
    /// are no scene settings to hang the tree on; the scene is then unusable.
    pub fn mend(&mut self) -> bool {
        for (id, t) in &mut self.things {
            t.id = *id;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    /// Balls that may never be stuck to the screen, standing in for a canvas.
    struct Unstuck;
    impl crate::ThingKind for Unstuck {
        fn id(&self) -> &'static str {
            "unstuck"
        }
        fn name(&self) -> &'static str {
            "Unstuck"
        }
        fn icon(&self) -> &'static str {
            "ball"
        }
        fn help(&self) -> &'static str {
            ""
        }
        fn settings(&self) -> &'static [crate::SettingDef] {
            &[]
        }
        fn slots(&self) -> &'static [SlotId] {
            &[]
        }
        fn default_slots(&self) -> &'static [SlotId] {
            &[]
        }
        fn can_stick_to_screen(&self) -> bool {
            false
        }
    }

    #[test]
    fn a_kind_that_cannot_stick_to_the_screen_never_does() {
        let reg = testkit::registry();
        let balls = reg.thing("balls").unwrap();
        let mut scene = Scene::empty(&reg);
        let id = testkit::push(&mut scene, "balls", ThingId::ROOT);
        let thing = scene.things.get_mut(&id).unwrap();
        thing.screen = Some(true);
        assert!(thing.sticks_to_screen(balls), "an ordinary kind follows the flag");
        assert!(!thing.sticks_to_screen(&Unstuck), "the flag is ignored");
        thing.screen = None;
        assert!(!thing.sticks_to_screen(&Unstuck));
    }

    #[test]
    fn empty_scene_has_only_the_root() {
        let s = Scene::empty(&testkit::registry());
        assert_eq!(s.order, vec![ThingId::ROOT]);
        assert_eq!(s.things[&ThingId::ROOT].name, "Scene settings");
        assert_eq!(s.format, 5);
        assert_eq!(s.next_thing, 1);
    }

    #[test]
    fn targets_match_by_thing_tag_kind_and_self() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.things.get_mut(&b).unwrap().tags.insert("drums".into());
        assert!(s.target_matches(&Target::Anything, a, b));
        assert!(s.target_matches(&Target::Me, a, a) && !s.target_matches(&Target::Me, a, b));
        assert!(s.target_matches(&Target::Thing(b), a, b) && !s.target_matches(&Target::Thing(b), a, a));
        let drums = Target::Tag("drums".into());
        assert!(s.target_matches(&drums, a, b) && !s.target_matches(&drums, b, a));
        assert!(s.target_matches(&Target::Kind("folder".into()), a, b));
        assert!(!s.target_matches(&Target::Kind("balls".into()), a, b));
        assert!(!s.target_matches(&Target::Thing(ThingId(99)), a, ThingId(99)));
        assert!(s.target_matches(&Target::Anything, a, ThingId(99)));
    }

    #[test]
    fn descendants_are_in_tree_order_without_the_root_of_the_walk() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let a = s.add_thing(&r, "balls", f).unwrap();
        let g = s.add_thing(&r, "folder", f).unwrap();
        let b = s.add_thing(&r, "balls", g).unwrap();
        let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!(s.descendants(f), vec![a, g, b]);
        assert_eq!(s.descendants(c), Vec::<ThingId>::new());
        assert_eq!(s.descendants(ThingId::ROOT), vec![f, a, g, b, c]);
        assert!(!s.things[&a].rides());
        assert_eq!(s.common_parent(&[ThingId::ROOT, a, c]), Some(f));
        assert_eq!(s.common_parent(&[ThingId::ROOT]), None);
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
