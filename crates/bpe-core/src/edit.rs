//! Edits to the scene tree and to each thing's own cards of settings. Each edit either succeeds
//! whole or changes nothing.

use crate::{Address, Attach, KindId, Params, Registry, Scene, Slot, SlotId, SlotStash, Thing, ThingId};
use glam::Vec2;
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    UnknownKind(KindId),
    Missing(ThingId),
    /// Scene settings can't be removed or moved.
    Root,
    WouldLoop,
    SlotNotSupported(SlotId),
    /// The card kind is made for a different slot.
    WrongSlot,
    /// The thing has no card in that slot.
    NoSlot(SlotId),
    WrongType,
    UnknownSetting(String),
    /// A scene has only one of these.
    OnlyOne(KindId),
    /// That kind of card doesn't fit this kind of thing.
    NotAccepted,
    /// The card has no layer with that id.
    NoLayer(crate::LayerId),
    /// The base layer (the balls' Body) always stays at the bottom and shown.
    BaseLayer,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::UnknownKind(k) => write!(f, "This version of Ball Studio doesn't know \"{k}\""),
            EditError::Missing(_) => write!(f, "That thing isn't in the scene any more"),
            EditError::Root => write!(f, "Scene settings can't be moved or removed"),
            EditError::WouldLoop => write!(f, "A thing can't ride on itself or on something inside it"),
            EditError::SlotNotSupported(s) => write!(f, "This kind of thing has no {} card", s.name()),
            EditError::WrongSlot => write!(f, "That kind of card is made for a different place"),
            EditError::NoSlot(s) => write!(f, "This thing has no {} card", s.name()),
            EditError::WrongType => write!(f, "That kind of value doesn't fit this setting"),
            EditError::UnknownSetting(k) => write!(f, "There is no setting called \"{k}\" here"),
            EditError::OnlyOne(k) => write!(f, "A scene has only one {k}"),
            EditError::NotAccepted => write!(f, "That kind of card can't go there"),
            EditError::NoLayer(_) => write!(f, "That layer isn't there any more"),
            EditError::BaseLayer => write!(f, "The bottom layer can't be removed, moved, copied or hidden"),
        }
    }
}

impl std::error::Error for EditError {}

impl Scene {
    /// Adds a thing of `kind` as the last child of `parent`, with no cards yet (`bpe_kit`'s
    /// `add_thing` gives it its kind's starting cards).
    pub fn add_thing(&mut self, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError> {
        let k = reg.thing(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        if k.single() && self.things.values().any(|t| t.kind == kind) {
            return Err(EditError::OnlyOne(kind.into()));
        }
        if !self.things.contains_key(&parent) {
            return Err(EditError::Missing(parent));
        }
        let id = ThingId(self.next_thing);
        self.next_thing += 1;
        let colour = Some(self.new_colour());
        let thing = Thing {
            id,
            name: self.free_name(k.name()),
            kind: kind.into(),
            parent: Some(parent),
            colour,
            ..Thing::default()
        };
        self.things.insert(id, thing);
        let at = self.subtree(parent).end;
        self.order.insert(at, id);
        Ok(id)
    }

    /// `base`, or `base` with the next number after the highest one in use ("Balls 2").
    pub fn free_name(&self, base: &str) -> String {
        let prefix = format!("{base} ");
        let highest = self
            .things
            .values()
            .filter_map(|t| if t.name == base { Some(1) } else { t.name.strip_prefix(&prefix)?.parse::<u64>().ok() })
            .max();
        match highest {
            None => base.to_owned(),
            Some(n) => format!("{base} {}", n.saturating_add(1)),
        }
    }

    /// Where `id` and everything under it sit in `order`. Empty when `id` isn't there.
    fn subtree(&self, id: ThingId) -> Range<usize> {
        let Some(start) = self.order.iter().position(|&t| t == id) else { return 0..0 };
        let mut inside = BTreeSet::from([id]);
        let mut end = start + 1;
        while let Some(t) = self.order.get(end) {
            match self.things.get(t).and_then(|t| t.parent) {
                Some(p) if inside.contains(&p) => {
                    inside.insert(*t);
                    end += 1;
                }
                _ => break,
            }
        }
        start..end
    }

    fn check_movable(&self, id: ThingId) -> Result<(), EditError> {
        if id == ThingId::ROOT {
            return Err(EditError::Root);
        }
        if !self.things.contains_key(&id) {
            return Err(EditError::Missing(id));
        }
        Ok(())
    }

    /// Moves `id` and everything under it to the end of `parent`'s children.
    pub fn set_parent(&mut self, id: ThingId, parent: ThingId, attach: Attach) -> Result<(), EditError> {
        self.check_movable(id)?;
        if !self.things.contains_key(&parent) {
            return Err(EditError::Missing(parent));
        }
        if parent == id || self.ancestors(parent).contains(&id) {
            return Err(EditError::WouldLoop);
        }
        let block: Vec<ThingId> = self.order.drain(self.subtree(id)).collect();
        let thing = self.things.get_mut(&id).expect("checked above");
        thing.parent = Some(parent);
        thing.attach = attach;
        let at = self.subtree(parent).end;
        self.order.splice(at..at, block);
        Ok(())
    }

    /// Changes how `id` hangs off its parent without moving it among its siblings.
    pub fn set_attach(&mut self, id: ThingId, attach: Attach) -> Result<(), EditError> {
        self.check_movable(id)?;
        self.things.get_mut(&id).expect("checked above").attach = attach;
        Ok(())
    }

    /// Moves `id` and everything under it to just before `sibling`, which must share its parent.
    pub fn move_before(&mut self, id: ThingId, sibling: ThingId) -> Result<(), EditError> {
        self.check_movable(id)?;
        let parent = self.things[&id].parent;
        if self.things.get(&sibling).is_none_or(|s| s.parent != parent) || sibling == id {
            return Err(EditError::Missing(sibling));
        }
        let block: Vec<ThingId> = self.order.drain(self.subtree(id)).collect();
        let at = self.subtree(sibling).start;
        self.order.splice(at..at, block);
        Ok(())
    }

    /// Removes `id` and everything under it, returning every removed id in tree order.
    pub fn remove_thing(&mut self, id: ThingId) -> Result<Vec<ThingId>, EditError> {
        self.check_movable(id)?;
        let removed: Vec<ThingId> = self.order.drain(self.subtree(id)).collect();
        for t in &removed {
            self.things.remove(t);
        }
        // Their keys and pulses go with them (a saved file would otherwise keep them forever).
        self.tracks.retain(|a, _| !removed.contains(&a.thing));
        self.pulses.retain(|a, _| !removed.contains(&a.thing));
        // So do settings elsewhere that followed one of their sounds (a bus is no one's).
        self.pulses.retain(|_, p| match p.source {
            crate::PulseSource::Sound { shape, source } => {
                matches!(source, crate::SoundSource::Bus(_)) || !removed.contains(&shape)
            }
            crate::PulseSource::Hits => true,
        });
        Ok(removed)
    }

    /// Gives the thing a card of `kind` in `slot`, every setting at its default. A card already
    /// there is replaced, and its keys and pulses go.
    pub fn add_slot(&mut self, reg: &Registry, id: ThingId, slot: SlotId, kind: &str) -> Result<(), EditError> {
        let thing = self.things.get(&id).ok_or(EditError::Missing(id))?;
        let tkind = reg.thing(&thing.kind).ok_or_else(|| EditError::UnknownKind(thing.kind.clone()))?;
        if !tkind.slots().contains(&slot) {
            return Err(EditError::SlotNotSupported(slot));
        }
        let card = reg.module(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        if card.slot() != slot {
            return Err(EditError::WrongSlot);
        }
        if !card.fits(tkind.id()) {
            return Err(EditError::NotAccepted);
        }
        self.drop_slot_animation(id, slot);
        self.things.get_mut(&id).expect("checked above").slots.insert(slot, Slot::new(kind));
        Ok(())
    }

    /// Takes the card in `slot` off the thing, with its keys and pulses. True when there was one.
    pub fn remove_slot(&mut self, id: ThingId, slot: SlotId) -> bool {
        let Some(thing) = self.things.get_mut(&id) else { return false };
        if thing.slots.remove(&slot).is_none() {
            return false;
        }
        self.drop_slot_animation(id, slot);
        true
    }

    fn drop_slot_animation(&mut self, id: ThingId, slot: SlotId) {
        let on = |a: &Address| a.thing == id && a.slot == Some(slot);
        self.tracks.retain(|a, _| !on(a));
        self.pulses.retain(|a, _| !on(a));
        // A Sound card going (or being replaced, as Paste sound does) takes its modulators, so
        // settings anywhere that followed one stop following it rather than pick up whatever
        // layer gets its number in the new card. Its Level and the buses stay.
        if slot == SlotId::Sound {
            self.pulses.retain(|_, p| {
                !matches!(p.source, crate::PulseSource::Sound { shape, source: crate::SoundSource::Modulator(_) } if shape == id)
            });
        }
    }

    /// Turns the thing's card in `slot` into a card of `kind` (of the same slot) in place.
    /// Settings, keyframes and pulses the new kind also has (with the same type) stay; the rest are
    /// set aside in the card's stash under the old kind, and anything stashed earlier under `kind`
    /// comes back wherever the key is still free. Nothing changes if the edit is refused.
    pub fn set_slot_kind(&mut self, reg: &Registry, id: ThingId, slot: SlotId, kind: &str) -> Result<(), EditError> {
        let thing = self.things.get(&id).ok_or(EditError::Missing(id))?;
        let card = thing.slots.get(&slot).ok_or(EditError::NoSlot(slot))?;
        let new = reg.module(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        if new.slot() != slot {
            return Err(EditError::WrongSlot);
        }
        if card.kind == kind {
            return Ok(());
        }
        if !new.fits(&thing.kind) {
            return Err(EditError::NotAccepted);
        }
        let old_kind = card.kind.clone();
        let def = |key: &str| new.settings().iter().find(|d| d.key == key);
        let accepts = |key: &str, v: &crate::Value| def(key).is_some_and(|d| d.ty.accepts(v));
        let mut out = SlotStash::default();

        // 1. The card's own settings.
        let card = self.things.get_mut(&id).expect("checked above").slots.get_mut(&slot).expect("checked above");
        let mut kept = Params::default();
        for (k, v) in std::mem::take(&mut card.params).0 {
            if let Some(d) = def(&k).filter(|d| d.ty.accepts(&v)) {
                // Fitted to the new kind: inside its range, and left out when it is its default.
                let v = d.clamp(v);
                if v != d.default {
                    kept.set(&k, v);
                }
            } else {
                out.params.set(&k, v);
            }
        }
        card.params = kept;
        let back = card.stash.remove(kind).unwrap_or_default();

        // 2. Keyframes and pulses on the card.
        let on = |a: &Address| a.thing == id && a.slot == Some(slot);
        let gone: Vec<Address> = self
            .tracks
            .iter()
            .filter(|(a, t)| {
                let keyable = def(&a.key).is_some_and(|d| d.flags.keyframe);
                on(a) && !(keyable && t.keys.first().is_some_and(|k| accepts(&a.key, &k.value)))
            })
            .map(|(a, _)| a.clone())
            .collect();
        for a in gone {
            let track = self.tracks.remove(&a).expect("listed above");
            out.tracks.insert(a.key, track);
        }
        let gone: Vec<Address> = self
            .pulses
            .iter()
            .filter(|(a, p)| {
                let pulses = def(&a.key).is_some_and(|d| d.flags.pulse);
                on(a) && !(pulses && accepts(&a.key, &p.amount))
            })
            .map(|(a, _)| a.clone())
            .collect();
        for a in gone {
            let pulse = self.pulses.remove(&a).expect("listed above");
            out.pulses.insert(a.key, pulse);
        }

        // 3. Put back what an earlier stay as `kind` set aside, where nothing holds the key now.
        let at = |key: String| Address { thing: id, slot: Some(slot), key };
        for (k, v) in back.tracks {
            self.tracks.entry(at(k)).or_insert(v);
        }
        for (k, v) in back.pulses {
            self.pulses.entry(at(k)).or_insert(v);
        }
        let card = self.things.get_mut(&id).expect("checked above").slots.get_mut(&slot).expect("checked above");
        for (k, v) in back.params.0 {
            if card.params.get(&k).is_none() {
                card.params.set(&k, v);
            }
        }

        // 4. A setting the new kind has is current now, so an older copy set aside under any other
        // kind is stale: switching back must not bring it over a newer value (or a newer default).
        for stash in card.stash.values_mut() {
            stash.params.0.retain(|k, _| def(k).is_none());
            stash.tracks.retain(|k, _| def(k).is_none());
            stash.pulses.retain(|k, _| def(k).is_none());
        }
        card.stash.retain(|_, s| *s != SlotStash::default());

        // 5. The kind.
        if out != SlotStash::default() {
            card.stash.insert(old_kind, out);
        }
        card.kind = kind.into();
        Ok(())
    }

    /// Sets a thing's name exactly as given (as it is typed).
    pub fn set_thing_name(&mut self, id: ThingId, name: &str) {
        if let Some(thing) = self.things.get_mut(&id) {
            thing.name = name.to_owned();
        }
    }

    /// Renames a thing. A blank or unchanged name changes nothing and returns false.
    pub fn rename_thing(&mut self, id: ThingId, name: &str) -> bool {
        let name = name.trim();
        let Some(thing) = self.things.get(&id) else { return false };
        if name.is_empty() || thing.name == name {
            return false;
        }
        self.set_thing_name(id, name);
        true
    }

    /// Puts a new folder where the first of `ids` was and moves them all into it, in their old
    /// order. The root and anything already inside another listed thing are left where they are.
    pub fn make_folder(&mut self, reg: &Registry, ids: &[ThingId]) -> Result<ThingId, EditError> {
        let listed: BTreeSet<ThingId> = ids.iter().copied().collect();
        let moving: Vec<ThingId> = self
            .order
            .iter()
            .copied()
            .filter(|id| {
                *id != ThingId::ROOT
                    && listed.contains(id)
                    && !self.ancestors(*id).iter().any(|a| listed.contains(a) && *a != ThingId::ROOT)
            })
            .collect();
        let Some(&first) = moving.first() else { return Err(EditError::Root) };
        let parent = self.things[&first].parent.unwrap_or(ThingId::ROOT);
        let folder = self.add_thing(reg, "folder", parent)?;
        self.move_before(folder, first)?;
        // Things that rode the old parent keep riding: the folder rides it in their place (on the
        // one ball they all rode, if they agreed on one) and they ride the folder as before.
        let riders: Vec<Attach> = moving
            .iter()
            .filter(|id| self.things[id].parent == Some(parent))
            .map(|id| self.things[id].attach)
            .filter(|a| matches!(a, Attach::Ride { .. }))
            .collect();
        if let Some(Attach::Ride { item, .. }) = riders.first().copied()
            && parent != ThingId::ROOT
        {
            let shared = riders.iter().all(|a| matches!(a, Attach::Ride { item: i, .. } if *i == item));
            let item = if shared { item } else { None };
            self.set_attach(folder, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item })?;
            for id in &moving {
                if let Attach::Ride { item: Some(_), .. } = self.things[id].attach
                    && shared
                {
                    let t = self.things.get_mut(id).expect("gathered above");
                    if let Attach::Ride { item, .. } = &mut t.attach {
                        *item = None;
                    }
                }
            }
        }
        for id in moving {
            let attach = match self.things[&id].attach {
                ride @ Attach::Ride { .. } if self.things[&id].parent == Some(parent) => ride,
                _ => Attach::Group,
            };
            self.set_parent(id, folder, attach)?;
        }
        Ok(folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Value, testkit};

    #[test]
    fn add_thing_names_from_the_kind_and_starts_with_no_cards() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!((s.things[&a].name.as_str(), s.things[&b].name.as_str()), ("Balls", "Balls 2"));
        assert!(s.things[&a].slots.is_empty());
        assert!(matches!(s.add_thing(&r, "magnet", ThingId::ROOT), Err(EditError::UnknownKind(_))));
    }

    #[test]
    fn removing_a_thing_drops_its_keys_and_pulses() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, a, SlotId::Look, "look.plain").unwrap();
        let glow = Address { thing: a, slot: Some(SlotId::Look), key: "glow".into() };
        s.set_key(&r, &glow, 1.0, Value::Number(1.0)).unwrap();
        s.set_pulse(&r, &glow, crate::PulseSource::Hits, Value::Number(0.5), 0.25).unwrap();
        s.remove_thing(a).unwrap();
        assert!(s.tracks.is_empty() && s.pulses.is_empty());
    }

    #[test]
    fn removing_a_thing_stops_settings_following_its_sound() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let shape = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.plain").unwrap();
        let glow = Address { thing: b, slot: Some(SlotId::Look), key: "glow".into() };
        let follows = |source| crate::PulseSource::Sound { shape, source };
        s.set_pulse(&r, &glow, follows(crate::SoundSource::Level), Value::Number(0.5), 0.25).unwrap();
        let other = Address { thing: b, slot: Some(SlotId::Look), key: "size".into() };
        let bus = s.set_pulse(&r, &other, follows(crate::SoundSource::Bus(1)), Value::Number(0.5), 0.25).is_ok();
        s.remove_thing(shape).unwrap();
        assert!(!s.pulses.contains_key(&glow), "the setting stops following the deleted shape");
        assert_eq!(s.pulses.contains_key(&other), bus, "a bus is no one shape's: it keeps following it");
    }

    #[test]
    fn slots_the_kind_lacks_and_wrong_kinds_are_refused() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!(s.add_slot(&r, b, SlotId::Shader, "look.plain"), Err(EditError::SlotNotSupported(SlotId::Shader)));
        assert_eq!(s.add_slot(&r, b, SlotId::Trail, "look.plain"), Err(EditError::WrongSlot));
        assert_eq!(
            s.add_slot(&r, b, SlotId::Look, "look.hologram"),
            Err(EditError::UnknownKind("look.hologram".into()))
        );
        assert_eq!(s.add_slot(&r, ThingId(99), SlotId::Look, "look.plain"), Err(EditError::Missing(ThingId(99))));
        assert!(s.things[&b].slots.is_empty());
        s.add_slot(&r, b, SlotId::Trail, "trail.streak").unwrap();
        assert_eq!(s.things[&b].slots[&SlotId::Trail], Slot::new("trail.streak"));
    }

    #[test]
    fn removing_a_card_drops_its_keys_only() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.plain").unwrap();
        let glow = Address { thing: b, slot: Some(SlotId::Look), key: "glow".into() };
        let size = Address { thing: b, slot: None, key: "radius".into() };
        s.set_key(&r, &glow, 1.0, Value::Number(1.0)).unwrap();
        s.set_key(&r, &size, 1.0, Value::Number(9.0)).unwrap();
        assert!(s.remove_slot(b, SlotId::Look));
        assert!(!s.remove_slot(b, SlotId::Look), "already gone");
        assert!(!s.things[&b].slots.contains_key(&SlotId::Look));
        assert_eq!(s.tracks.keys().collect::<Vec<_>>(), [&size]);
    }

    #[test]
    fn two_things_never_share_a_card() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        for t in [a, b] {
            s.add_slot(&r, t, SlotId::Look, "look.plain").unwrap();
        }
        let glow = |t| Address { thing: t, slot: Some(SlotId::Look), key: "glow".into() };
        s.set_at(&r, &glow(a), Value::Number(1.5)).unwrap();
        assert_eq!(s.value_at(&r, &glow(a)), Some(Value::Number(1.5)));
        assert_eq!(s.value_at(&r, &glow(b)), Some(Value::Number(0.0)));
    }

    #[test]
    fn add_thing_copes_with_odd_names_and_parents() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.things.get_mut(&a).unwrap().name = "Balls 4294967295".into();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!(s.things[&b].name, "Balls 4294967296");
        assert!(matches!(s.add_thing(&r, "balls", ThingId(999)), Err(EditError::Missing(ThingId(999)))));
    }

    #[test]
    fn set_attach_keeps_the_place_among_siblings() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let ride = Attach::Ride { offset: glam::Vec2::new(1.0, 2.0), turn_with_parent: true, item: Some(3) };
        s.set_attach(a, ride).unwrap();
        assert_eq!((s.things[&a].attach, s.order.clone()), (ride, vec![ThingId::ROOT, a, b]));
        assert!(matches!(s.set_attach(ThingId::ROOT, ride), Err(EditError::Root)));
    }

    #[test]
    fn riding_on_a_descendant_is_refused() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        assert!(matches!(s.set_parent(f, b, Attach::Group), Err(EditError::WouldLoop)));
        assert!(matches!(s.set_parent(ThingId::ROOT, f, Attach::Group), Err(EditError::Root)));
    }

    #[test]
    fn set_parent_moves_the_whole_subtree() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let g = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.set_parent(f, g, Attach::Group).unwrap();
        assert_eq!(s.order, vec![ThingId::ROOT, g, f, b]);
    }

    #[test]
    fn move_before_reorders_siblings_with_their_children() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let g = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.move_before(g, f).unwrap();
        assert_eq!(s.order, vec![ThingId::ROOT, g, f, b]);
        assert!(matches!(s.move_before(b, g), Err(EditError::Missing(_))));
    }

    #[test]
    fn remove_thing_takes_the_subtree() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!(s.remove_thing(f).unwrap(), vec![f, b]);
        assert_eq!(s.order, vec![ThingId::ROOT, c]);
        assert!(matches!(s.remove_thing(ThingId::ROOT), Err(EditError::Root)));
    }

    #[test]
    fn ids_are_not_reused_after_reload() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.remove_thing(a).unwrap();
        let mut back = crate::save::load(&crate::save::save(&s), &r).unwrap();
        assert_ne!(back.add_thing(&r, "balls", ThingId::ROOT).unwrap(), a);
    }

    #[test]
    fn make_folder_gathers_the_selection_in_place() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let d = s.add_thing(&r, "balls", c).unwrap();
        let f = s.make_folder(&r, &[b, c, ThingId::ROOT, d]).unwrap();
        assert_eq!(s.things[&f].kind, "folder");
        assert_eq!(s.order, vec![ThingId::ROOT, a, f, b, c, d]);
        assert_eq!((s.things[&b].parent, s.things[&d].parent), (Some(f), Some(c)));
        assert!(matches!(s.make_folder(&r, &[ThingId::ROOT]), Err(EditError::Root)));
        assert!(matches!(s.make_folder(&r, &[]), Err(EditError::Root)));
    }

    #[test]
    fn make_folder_keeps_riders_riding_through_the_folder() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let arena = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let ride = |offset: f32, item| Attach::Ride { offset: Vec2::new(offset, 0.0), turn_with_parent: true, item };
        s.set_parent(b, arena, ride(10.0, Some(3))).unwrap();
        s.set_parent(c, arena, ride(20.0, Some(3))).unwrap();
        let f = s.make_folder(&r, &[b, c]).unwrap();
        // Both rode ball 3 of the arena: the folder rides that ball and they ride the folder.
        assert_eq!(s.things[&f].parent, Some(arena));
        assert_eq!(s.things[&f].attach, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: Some(3) });
        assert_eq!((s.things[&b].parent, s.things[&b].attach), (Some(f), ride(10.0, None)));
        assert_eq!((s.things[&c].parent, s.things[&c].attach), (Some(f), ride(20.0, None)));
        // Riders of different balls keep their own items; the folder rides the parent's place.
        let d = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let e = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.set_parent(d, arena, ride(1.0, Some(1))).unwrap();
        s.set_parent(e, arena, ride(2.0, Some(2))).unwrap();
        let g = s.make_folder(&r, &[d, e]).unwrap();
        assert_eq!(s.things[&g].attach, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: None });
        assert_eq!((s.things[&d].attach, s.things[&e].attach), (ride(1.0, Some(1)), ride(2.0, Some(2))));
        // Grouped things stay grouped, and a folder at the top level never rides.
        let h = s.make_folder(&r, &[arena]).unwrap();
        assert_eq!((s.things[&h].attach, s.things[&arena].attach), (Attach::Group, Attach::Group));
    }

    // Three look kinds that share `glow`: A also has `ring`; B and C have only `glow`.
    #[derive(crate::Settings)]
    struct LookASettings {
        #[setting(label = "Glow", range(0, 9), keyframe, pulse)]
        glow: f32,
        #[setting(label = "Ring", range(0, 9), keyframe, pulse)]
        ring: f32,
    }
    impl Default for LookASettings {
        fn default() -> Self {
            Self { glow: 0.0, ring: 0.0 }
        }
    }
    #[derive(crate::Settings)]
    struct LookBSettings {
        #[setting(label = "Glow", range(0, 9), keyframe, pulse)]
        glow: f32,
    }
    impl Default for LookBSettings {
        fn default() -> Self {
            Self { glow: 0.0 }
        }
    }
    struct LookA;
    struct LookB;
    struct LookC;
    macro_rules! look_kind {
        ($t:ident, $id:literal, $s:ty) => {
            impl crate::ModuleKind for $t {
                fn id(&self) -> &'static str {
                    $id
                }
                fn name(&self) -> &'static str {
                    $id
                }
                fn help(&self) -> &'static str {
                    "A test look."
                }
                fn slot(&self) -> SlotId {
                    SlotId::Look
                }
                fn settings(&self) -> &'static [crate::SettingDef] {
                    <$s as crate::Settings>::defs()
                }
            }
        };
    }
    look_kind!(LookA, "look.a", LookASettings);
    look_kind!(LookB, "look.b", LookBSettings);
    look_kind!(LookC, "look.c", LookBSettings);
    // A look whose `glow` is a yes/no rather than a number, and one that fits no balls.
    #[derive(Default, crate::Settings)]
    struct LookDSettings {
        #[setting(label = "Glow")]
        glow: bool,
    }
    struct LookD;
    look_kind!(LookD, "look.d", LookDSettings);

    /// A ball with a card of kind A with `glow` 1 and `ring` 2.
    fn switchable() -> (Registry, Scene, ThingId) {
        let mut r = testkit::registry();
        r.add_module(LookA);
        r.add_module(LookB);
        r.add_module(LookC);
        r.add_module(LookD);
        let mut s = Scene::empty(&r);
        let t = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, t, SlotId::Look, "look.a").unwrap();
        s.things.get_mut(&t).unwrap().slots.get_mut(&SlotId::Look).unwrap().params =
            params_of(&[("glow", 1.0), ("ring", 2.0)]);
        (r, s, t)
    }

    fn params_of(pairs: &[(&str, f64)]) -> Params {
        let mut p = Params::default();
        for (k, v) in pairs {
            p.set(k, Value::Number(*v));
        }
        p
    }

    fn look_addr(t: ThingId, key: &str) -> Address {
        Address { thing: t, slot: Some(SlotId::Look), key: key.into() }
    }

    fn track_at(v: f64) -> crate::Track {
        crate::Track {
            keys: vec![crate::animation::Key { time: 1.0, value: Value::Number(v), ease: Default::default() }],
            range: None,
        }
    }

    fn card(s: &Scene, t: ThingId) -> &Slot {
        &s.things[&t].slots[&SlotId::Look]
    }

    #[test]
    fn a_kind_switch_keeps_shared_keys_and_stashes_the_rest() {
        let (r, mut s, t) = switchable();
        s.set_slot_kind(&r, t, SlotId::Look, "look.b").unwrap();
        assert_eq!(card(&s, t).kind, "look.b");
        assert_eq!(card(&s, t).params, params_of(&[("glow", 1.0)]));
        assert_eq!(card(&s, t).stash["look.a"].params, params_of(&[("ring", 2.0)]));
    }

    #[test]
    fn switching_back_restores_the_stash_but_newer_shared_values_win() {
        let (r, mut s, t) = switchable();
        s.set_slot_kind(&r, t, SlotId::Look, "look.b").unwrap();
        s.set_at(&r, &look_addr(t, "glow"), Value::Number(3.0)).unwrap();
        s.set_slot_kind(&r, t, SlotId::Look, "look.a").unwrap();
        assert_eq!(card(&s, t).params, params_of(&[("glow", 3.0), ("ring", 2.0)]));
        assert!(card(&s, t).stash.is_empty());
    }

    #[test]
    fn keyframes_and_pulses_on_dropped_keys_are_stashed_and_come_back() {
        let (r, mut s, t) = switchable();
        s.tracks.insert(look_addr(t, "ring"), track_at(1.0));
        s.tracks.insert(look_addr(t, "glow"), track_at(2.0));
        let pulse = crate::Pulse { amount: Value::Number(1.0), ..Default::default() };
        s.pulses.insert(look_addr(t, "ring"), pulse);
        s.set_slot_kind(&r, t, SlotId::Look, "look.b").unwrap();
        assert_eq!(s.tracks.keys().collect::<Vec<_>>(), [&look_addr(t, "glow")]);
        assert!(s.pulses.is_empty());
        let stash = &card(&s, t).stash["look.a"];
        assert!(stash.tracks.contains_key("ring") && stash.pulses.contains_key("ring"));
        s.set_slot_kind(&r, t, SlotId::Look, "look.a").unwrap();
        assert!(s.tracks.contains_key(&look_addr(t, "ring")) && s.pulses.contains_key(&look_addr(t, "ring")));
    }

    #[test]
    fn a_stash_survives_a_switch_to_a_third_kind() {
        let (r, mut s, t) = switchable();
        s.set_slot_kind(&r, t, SlotId::Look, "look.b").unwrap();
        s.set_slot_kind(&r, t, SlotId::Look, "look.c").unwrap();
        s.set_slot_kind(&r, t, SlotId::Look, "look.a").unwrap();
        assert_eq!(card(&s, t).params, params_of(&[("glow", 1.0), ("ring", 2.0)]));
    }

    #[test]
    fn a_stashed_value_never_comes_back_over_a_newer_default() {
        let (r, mut s, t) = switchable();
        // D sets both aside (its glow is another type), B has glow again, which is set back to
        // its default there: returning to A keeps that default rather than the old 1.
        s.set_slot_kind(&r, t, SlotId::Look, "look.d").unwrap();
        s.set_slot_kind(&r, t, SlotId::Look, "look.b").unwrap();
        s.set_at(&r, &look_addr(t, "glow"), Value::Number(0.0)).unwrap();
        s.set_slot_kind(&r, t, SlotId::Look, "look.a").unwrap();
        assert_eq!(card(&s, t).params, params_of(&[("ring", 2.0)]));
    }

    #[test]
    fn a_key_both_kinds_have_with_another_type_is_stashed_not_carried() {
        let (r, mut s, t) = switchable();
        s.set_slot_kind(&r, t, SlotId::Look, "look.d").unwrap();
        assert!(card(&s, t).params.is_empty());
        assert_eq!(card(&s, t).stash["look.a"].params, params_of(&[("glow", 1.0), ("ring", 2.0)]));
    }

    #[test]
    fn a_kind_switch_is_refused_without_a_card_or_into_another_slot() {
        let (r, mut s, t) = switchable();
        assert_eq!(s.set_slot_kind(&r, t, SlotId::Look, "trail.streak"), Err(EditError::WrongSlot));
        assert_eq!(s.set_slot_kind(&r, t, SlotId::Trail, "trail.streak"), Err(EditError::NoSlot(SlotId::Trail)));
        assert_eq!(card(&s, t).kind, "look.a");
    }

    #[test]
    fn a_stash_round_trips_through_ron() {
        let (r, mut s, t) = switchable();
        s.set_slot_kind(&r, t, SlotId::Look, "look.b").unwrap();
        let back = crate::save::load(&crate::save::save(&s), &r).unwrap();
        assert_eq!(card(&back, t), card(&s, t));
    }
}
