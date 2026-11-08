//! Copying things (editor workflow, W5): [`Scene::copy_things`] lifts things out of a scene with
//! everything under them, their keys and pulses and the fonts and pictures they use, and
//! [`Scene::paste_things`] puts such a copy into a scene (the same one or another) with new ids,
//! pointing what pointed between the copied things at the copies.

use std::collections::{BTreeMap, BTreeSet};

use crate::scene_pictures::{PICTURE_KEY, ScenePicture, is_key};
use crate::{
    Address, Attach, EditError, KindId, Pulse, PulseSource, Registry, Scene, SceneFont, SoundSource, Target, Thing,
    ThingId, Track, Value, Who,
};

/// The setting key that names a font (a Text thing's Font).
const FONT_KEY: &str = "font";

/// Things copied out of a scene, ready to paste into any scene.
#[derive(Clone, Debug, PartialEq)]
pub struct ThingClip {
    /// The copied things, each before the things under it, with the ids they had.
    pub things: Vec<Thing>,
    /// The copied things that hang off something not copied, in tree order.
    pub tops: Vec<ThingId>,
    /// The copied things' keys and pulses.
    pub tracks: BTreeMap<Address, Track>,
    pub pulses: BTreeMap<Address, Pulse>,
    /// The fonts and pictures their settings name.
    pub fonts: BTreeMap<String, SceneFont>,
    pub pictures: BTreeMap<String, ScenePicture>,
    /// Each thing not copied that a copied setting points at: its kind and name, so a paste
    /// keeps the link only where it finds the same thing.
    pub outside: BTreeMap<ThingId, (KindId, String)>,
}

impl ThingClip {
    /// The names of the copied things that hang off something not copied.
    pub fn top_names(&self) -> Vec<&str> {
        self.tops.iter().filter_map(|t| self.things.iter().find(|x| x.id == *t)).map(|t| t.name.as_str()).collect()
    }
}

/// What a paste made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pasted {
    /// The pasted things that hang off the parent pasted into, in order.
    pub tops: Vec<ThingId>,
    /// Every pasted thing, its old id and its new one.
    pub ids: BTreeMap<ThingId, ThingId>,
    /// Settings and pulses that pointed at a thing this scene doesn't have: now pointing at
    /// anything, or no longer following.
    pub unlinked: usize,
}

/// Where a reference to a thing goes in a paste.
enum Link {
    To(ThingId),
    Cut,
}

/// Every thing id `v` points at.
fn ids_in(v: &Value, out: &mut BTreeSet<ThingId>) {
    match v {
        Value::Target(Target::Thing(t)) => _ = out.insert(*t),
        Value::Who(Who::Thing { thing, .. }) => _ = out.insert(*thing),
        Value::LightScenes(scenes) => out.extend(scenes.iter().flat_map(|s| s.lights.iter().map(|l| l.thing))),
        _ => {}
    }
}

/// Points `v`'s thing ids where `link` says; true when a link was cut.
fn relink(v: &mut Value, link: &dyn Fn(ThingId) -> Link) -> usize {
    match v {
        Value::Target(t @ Target::Thing(_)) => {
            let Target::Thing(id) = *t else { unreachable!() };
            match link(id) {
                Link::To(new) => *t = Target::Thing(new),
                Link::Cut => {
                    *t = Target::Anything;
                    return 1;
                }
            }
        }
        Value::Who(w @ Who::Thing { .. }) => {
            let Who::Thing { thing, ball } = *w else { unreachable!() };
            match link(thing) {
                Link::To(new) => *w = Who::Thing { thing: new, ball },
                Link::Cut => {
                    *w = Who::Anything;
                    return 1;
                }
            }
        }
        Value::LightScenes(scenes) => {
            let mut cut = 0;
            for s in scenes {
                s.lights.retain_mut(|l| match link(l.thing) {
                    Link::To(new) => {
                        l.thing = new;
                        true
                    }
                    Link::Cut => {
                        cut += 1;
                        false
                    }
                });
            }
            return cut;
        }
        _ => {}
    }
    0
}

/// The thing a pulse follows, if any (a bus is no one's).
fn followed(source: &PulseSource) -> Option<ThingId> {
    match *source {
        PulseSource::Sound { source: SoundSource::Bus(_), .. } | PulseSource::Hits => None,
        PulseSource::Sound { shape, .. } => Some(shape),
        PulseSource::Light { thing, .. } => Some(thing),
        PulseSource::Health { thing } => Some(thing),
    }
}

fn refollow(source: &mut PulseSource, new: ThingId) {
    match source {
        PulseSource::Sound { shape, .. } => *shape = new,
        PulseSource::Light { thing, .. } | PulseSource::Health { thing } => *thing = new,
        PulseSource::Hits => {}
    }
}

/// Every value a thing holds, with its key: own settings, its cards and what they keep for other
/// kinds.
fn values_mut(thing: &mut Thing) -> impl Iterator<Item = (&String, &mut Value)> {
    let own = thing.params.0.iter_mut();
    let cards = thing.slots.values_mut().flat_map(|s| {
        let stash = s.stash.values_mut().flat_map(|st| st.params.0.iter_mut());
        s.params.0.iter_mut().chain(stash)
    });
    own.chain(cards)
}

fn values(thing: &Thing) -> impl Iterator<Item = (&String, &Value)> {
    let own = thing.params.0.iter();
    let cards = thing.slots.values().flat_map(|s| {
        let stash = s.stash.values().flat_map(|st| st.params.0.iter());
        s.params.0.iter().chain(stash)
    });
    own.chain(cards)
}

impl Scene {
    /// Copies `ids` with everything under them. The scene settings, things inside another listed
    /// thing (they come with it) and one-per-scene things such as the camera (with what is under
    /// them) are left out. `None` when nothing is left to copy.
    pub fn copy_things(&self, reg: &Registry, ids: &[ThingId]) -> Option<ThingClip> {
        let single = |id: &ThingId| {
            self.things.get(id).and_then(|t| reg.thing(&t.kind)).is_some_and(|k| k.single()) || *id == ThingId::ROOT
        };
        let listed: BTreeSet<ThingId> = ids.iter().copied().filter(|id| self.things.contains_key(id)).collect();
        let tops: Vec<ThingId> = self
            .order
            .iter()
            .copied()
            .filter(|id| listed.contains(id) && !single(id))
            .filter(|id| !self.ancestors(*id).iter().any(|a| listed.contains(a)))
            .collect();
        if tops.is_empty() {
            return None;
        }
        let mut taken: BTreeSet<ThingId> = BTreeSet::new();
        for &top in &tops {
            taken.insert(top);
            let inside = self.descendants(top);
            // Under a one-per-scene thing nothing comes along (it isn't copied either).
            let blocked: BTreeSet<ThingId> = inside.iter().copied().filter(single).collect();
            for id in inside {
                if !blocked.contains(&id) && !self.ancestors(id).iter().any(|a| blocked.contains(a)) {
                    taken.insert(id);
                }
            }
        }
        let things: Vec<Thing> =
            self.order.iter().filter(|id| taken.contains(id)).map(|id| self.things[id].clone()).collect();
        let tracks: BTreeMap<Address, Track> =
            self.tracks.iter().filter(|(a, _)| taken.contains(&a.thing)).map(|(a, t)| (a.clone(), t.clone())).collect();
        let pulses: BTreeMap<Address, Pulse> =
            self.pulses.iter().filter(|(a, _)| taken.contains(&a.thing)).map(|(a, p)| (a.clone(), p.clone())).collect();
        let mut pointed = BTreeSet::new();
        let mut texts: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        let mut text = |key: &str, v: &Value| {
            for name in [FONT_KEY, PICTURE_KEY] {
                if is_key(key, name)
                    && let Value::Text(t) = v
                {
                    texts.entry(name).or_default().insert(t.clone());
                }
            }
        };
        for thing in &things {
            for (k, v) in values(thing) {
                ids_in(v, &mut pointed);
                text(k, v);
            }
        }
        for (a, track) in &tracks {
            for key in &track.keys {
                ids_in(&key.value, &mut pointed);
                text(&a.key, &key.value);
            }
        }
        pointed.extend(pulses.values().filter_map(|p| followed(&p.source)));
        let outside = pointed
            .into_iter()
            .filter(|id| !taken.contains(id))
            .filter_map(|id| self.things.get(&id).map(|t| (id, (t.kind.clone(), t.name.clone()))))
            .collect();
        let named = |name: &str| texts.get(name).cloned().unwrap_or_default();
        let fonts =
            named(FONT_KEY).into_iter().filter_map(|n| Some((n.clone(), self.fonts.get(&n)?.clone()))).collect();
        let pictures =
            named(PICTURE_KEY).into_iter().filter_map(|n| Some((n.clone(), self.pictures.get(&n)?.clone()))).collect();
        Some(ThingClip { things, tops, tracks, pulses, fonts, pictures, outside })
    }

    /// Puts a copy of `clip` under `parent`, right after `after` when that is one of `parent`'s
    /// children, else last. Each pasted thing gets a new id and colour, and keeps its name unless
    /// another thing here has it ("Balls copy"). What pointed between the copied things points
    /// between the copies; what pointed at a thing outside the copy keeps pointing at it when this
    /// scene has that very thing (same id, kind and name), and otherwise points at anything (a
    /// pulse following it is dropped), counted in [`Pasted::unlinked`]. A one-per-scene thing
    /// this scene already has is left out, with what is under it.
    pub fn paste_things(
        &mut self,
        reg: &Registry,
        clip: &ThingClip,
        parent: ThingId,
        after: Option<ThingId>,
    ) -> Result<Pasted, EditError> {
        if !self.things.contains_key(&parent) {
            return Err(EditError::Missing(parent));
        }
        // Which copied things go in, with their new ids.
        let mut ids: BTreeMap<ThingId, ThingId> = BTreeMap::new();
        let mut skipped: BTreeSet<ThingId> = BTreeSet::new();
        let mut next = self.next_thing;
        for thing in &clip.things {
            let under_skipped = thing.parent.is_some_and(|p| skipped.contains(&p));
            let kind = reg.thing(&thing.kind);
            let one_here = kind.is_some_and(|k| k.single()) && self.things.values().any(|t| t.kind == thing.kind);
            if under_skipped || one_here {
                skipped.insert(thing.id);
                continue;
            }
            ids.insert(thing.id, ThingId(next));
            next += 1;
        }
        if ids.is_empty() {
            return Ok(Pasted::default());
        }
        // Fonts and pictures: a name taken by other bytes comes in under a new one.
        let mut renamed: BTreeMap<(&str, String), String> = BTreeMap::new();
        for (name, font) in &clip.fonts {
            if self.fonts.get(name) != Some(font) {
                let used = self.add_font(name, font.bytes.clone());
                if used != *name {
                    renamed.insert((FONT_KEY, name.clone()), used);
                }
            }
        }
        for (name, picture) in &clip.pictures {
            if self.pictures.get(name) != Some(picture) {
                let used = self.add_picture(name, picture.bytes.clone());
                if used != *name {
                    renamed.insert((PICTURE_KEY, name.clone()), used);
                }
            }
        }
        let here = |id: ThingId| -> bool {
            clip.outside
                .get(&id)
                .is_some_and(|(kind, name)| self.things.get(&id).is_some_and(|t| t.kind == *kind && t.name == *name))
        };
        let kept: BTreeSet<ThingId> = clip.outside.keys().copied().filter(|&id| here(id)).collect();
        let link = |id: ThingId| match ids.get(&id) {
            Some(&new) => Link::To(new),
            None if kept.contains(&id) => Link::To(id),
            None => Link::Cut,
        };
        let rename = |key: &str, v: &mut Value| {
            for name in [FONT_KEY, PICTURE_KEY] {
                if is_key(key, name)
                    && let Value::Text(t) = v
                    && let Some(new) = renamed.get(&(name, t.clone()))
                {
                    *t = new.clone();
                }
            }
        };
        let names_here: BTreeSet<String> = self.things.values().map(|t| t.name.clone()).collect();
        let mut unlinked = 0;
        let mut tops = Vec::new();
        let mut placed: Vec<Thing> = Vec::new();
        for source in &clip.things {
            let Some(&id) = ids.get(&source.id) else { continue };
            let mut thing = source.clone();
            thing.id = id;
            let top = !source.parent.is_some_and(|p| ids.contains_key(&p));
            if top {
                // Riding is kept only on the parent it rode before.
                if source.parent != Some(parent) {
                    thing.attach = Attach::Group;
                }
                thing.parent = Some(parent);
                tops.push(id);
            } else {
                thing.parent = source.parent.and_then(|p| ids.get(&p).copied());
            }
            if names_here.contains(&thing.name) {
                thing.name = self.free_name(&format!("{} copy", source.name));
            }
            for (k, v) in values_mut(&mut thing) {
                unlinked += relink(v, &link);
                rename(k, v);
            }
            placed.push(thing);
        }
        // Each gets a colour of its own, picked as the things go in, so they differ from each
        // other too.
        self.next_thing = next;
        for mut thing in placed {
            thing.colour = Some(self.new_colour());
            self.things.insert(thing.id, thing);
        }
        for (a, track) in &clip.tracks {
            let Some(&thing) = ids.get(&a.thing) else { continue };
            let mut track = track.clone();
            for key in &mut track.keys {
                unlinked += relink(&mut key.value, &link);
                rename(&a.key, &mut key.value);
            }
            self.tracks.insert(Address { thing, ..a.clone() }, track);
        }
        for (a, pulse) in &clip.pulses {
            let Some(&thing) = ids.get(&a.thing) else { continue };
            let mut pulse = pulse.clone();
            if let Some(from) = followed(&pulse.source) {
                match link(from) {
                    Link::To(new) => refollow(&mut pulse.source, new),
                    Link::Cut => {
                        unlinked += 1;
                        continue;
                    }
                }
            }
            self.pulses.insert(Address { thing, ..a.clone() }, pulse);
        }
        // Into the tree: the pasted block in clip order (parents before children) where it goes.
        let block: Vec<ThingId> = clip.things.iter().filter_map(|t| ids.get(&t.id).copied()).collect();
        let at = match after.filter(|a| self.things.get(a).is_some_and(|t| t.parent == Some(parent))) {
            Some(a) => self.subtree_end(a),
            None => self.subtree_end(parent),
        };
        self.order.splice(at..at, block);
        Ok(Pasted { tops, ids, unlinked })
    }

    /// Where the things under `id` end in `order`.
    fn subtree_end(&self, id: ThingId) -> usize {
        let Some(start) = self.order.iter().position(|&t| t == id) else { return self.order.len() };
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
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, SlotId, testkit};
    use std::sync::Arc;

    fn named(scene: &Scene, name: &str) -> ThingId {
        scene.things.values().find(|t| t.name == name).map(|t| t.id).unwrap_or_else(|| panic!("no {name}"))
    }

    /// A scene with a folder holding a ball group and a second group pointing at the first, and a
    /// third group outside the folder that the inner one points at.
    fn scene() -> (Registry, Scene, [ThingId; 4]) {
        let reg = testkit::registry();
        let mut s = Scene::empty(&reg);
        let folder = s.add_thing(&reg, "folder", ThingId::ROOT).unwrap();
        let a = s.add_thing(&reg, "balls", folder).unwrap();
        let b = s.add_thing(&reg, "balls", folder).unwrap();
        let outside = s.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&reg, b, SlotId::Trail, "trail.streak").unwrap();
        s.card_mut(b, SlotId::Trail).unwrap().params.set("target", Value::Target(Target::Thing(a)));
        s.add_slot(&reg, a, SlotId::Trail, "trail.streak").unwrap();
        s.card_mut(a, SlotId::Trail).unwrap().params.set("target", Value::Target(Target::Thing(outside)));
        let size = Address::own(a, "radius");
        s.tracks.entry(size).or_default().keys.push(Key {
            time: 1.0,
            value: Value::Number(9.0),
            ease: Default::default(),
        });
        let glow = Address::on(b, SlotId::Look, "glow");
        s.pulses.insert(glow, Pulse { source: PulseSource::Health { thing: a }, ..Pulse::default() });
        (reg, s, [folder, a, b, outside])
    }

    fn target(s: &Scene, id: ThingId) -> Value {
        s.card(id, SlotId::Trail).unwrap().params.get("target").cloned().unwrap()
    }

    #[test]
    fn copy_brings_what_is_under_and_its_keys_and_pulses() {
        let (reg, s, [folder, a, b, outside]) = scene();
        // The folder and one of its children listed: the child comes with the folder once.
        let clip = s.copy_things(&reg, &[a, folder]).unwrap();
        assert_eq!(clip.tops, [folder]);
        assert_eq!(clip.things.iter().map(|t| t.id).collect::<Vec<_>>(), [folder, a, b]);
        assert_eq!(clip.tracks.len(), 1);
        assert_eq!(clip.pulses.len(), 1);
        assert_eq!(clip.outside.keys().copied().collect::<Vec<_>>(), [outside]);
        assert!(s.copy_things(&reg, &[ThingId::ROOT]).is_none(), "the scene settings aren't copied");
    }

    #[test]
    fn pasting_into_the_same_scene_points_inside_at_the_copies_and_keeps_outside_links() {
        let (reg, mut s, [folder, a, b, outside]) = scene();
        let clip = s.copy_things(&reg, &[folder]).unwrap();
        let pasted = s.paste_things(&reg, &clip, ThingId::ROOT, Some(folder)).unwrap();
        assert_eq!(pasted.unlinked, 0);
        let [new_folder] = pasted.tops[..] else { panic!("{pasted:?}") };
        let (na, nb) = (pasted.ids[&a], pasted.ids[&b]);
        assert_eq!(s.things[&new_folder].name, "Folder copy");
        assert_eq!(s.things[&na].parent, Some(new_folder));
        assert_eq!(target(&s, nb), Value::Target(Target::Thing(na)), "between copies");
        assert_eq!(target(&s, na), Value::Target(Target::Thing(outside)), "to the same outside thing");
        assert_eq!(target(&s, b), Value::Target(Target::Thing(a)), "the original is untouched");
        assert!(s.tracks.contains_key(&Address::own(na, "radius")));
        let pulse = &s.pulses[&Address::on(nb, SlotId::Look, "glow")];
        assert_eq!(pulse.source, PulseSource::Health { thing: na });
        // Right after the folder, before the outside group.
        let at = |id| s.order.iter().position(|&t| t == id).unwrap();
        assert!(at(b) < at(new_folder) && at(nb) < at(outside), "{:?}", s.order);
        assert_ne!(s.things[&na].colour, s.things[&a].colour);
        assert!(s.mend());
    }

    #[test]
    fn pasting_into_another_scene_points_lost_links_at_anything() {
        let (reg, s, [_, a, _, _]) = scene();
        let clip = s.copy_things(&reg, &[a]).unwrap();
        let mut other = Scene::empty(&reg);
        let pasted = other.paste_things(&reg, &clip, ThingId::ROOT, None).unwrap();
        assert_eq!(pasted.unlinked, 1);
        let na = pasted.tops[0];
        assert_eq!(other.things[&na].name, s.things[&a].name, "a free name is kept");
        assert_eq!(target(&other, na), Value::Target(Target::Anything));
        // A pulse following a thing that isn't there is dropped.
        let (reg, s, [_, _, b, _]) = scene();
        let clip = s.copy_things(&reg, &[b]).unwrap();
        let mut other = Scene::empty(&reg);
        let pasted = other.paste_things(&reg, &clip, ThingId::ROOT, None).unwrap();
        assert_eq!(pasted.unlinked, 2, "the trail's target and the pulse");
        assert!(other.pulses.is_empty());
    }

    #[test]
    fn a_thing_of_the_same_id_but_another_name_is_not_linked() {
        let (reg, s, [_, a, _, outside]) = scene();
        let clip = s.copy_things(&reg, &[a]).unwrap();
        let mut other = s.clone();
        other.things.get_mut(&outside).unwrap().name = "Someone else".into();
        let pasted = other.paste_things(&reg, &clip, ThingId::ROOT, None).unwrap();
        assert_eq!(pasted.unlinked, 1);
    }

    #[test]
    fn fonts_and_pictures_come_along_and_are_renamed_on_a_clash() {
        let (reg, mut s, [_, a, _, _]) = scene();
        let font = s.add_font("Mine", Arc::from(&b"font one"[..]));
        let pic = s.add_picture("Logo", Arc::from(&b"picture one"[..]));
        s.things.get_mut(&a).unwrap().params.set("font", Value::Text(font));
        s.things.get_mut(&a).unwrap().params.set("3/picture", Value::Text(pic));
        let clip = s.copy_things(&reg, &[a]).unwrap();
        assert_eq!(clip.fonts.len(), 1);
        assert_eq!(clip.pictures.len(), 1);
        let mut other = Scene::empty(&reg);
        other.add_font("Mine", Arc::from(&b"another font"[..]));
        let pasted = other.paste_things(&reg, &clip, ThingId::ROOT, None).unwrap();
        let t = &other.things[&pasted.tops[0]];
        assert_eq!(t.params.get("font"), Some(&Value::Text("Mine (2)".into())));
        assert_eq!(t.params.get("3/picture"), Some(&Value::Text("Logo".into())));
        assert_eq!(&*other.pictures["Logo"].bytes, b"picture one");
        // Into the scene it came from: nothing new is added.
        let fonts = s.fonts.len();
        s.paste_things(&reg, &clip, ThingId::ROOT, None).unwrap();
        assert_eq!(s.fonts.len(), fonts);
        assert_eq!(named(&s, "Balls copy"), *s.order.last().unwrap());
    }

    #[test]
    fn riding_is_kept_only_on_the_parent_it_rode() {
        let (reg, mut s, [folder, a, _, outside]) = scene();
        let rider = s.add_thing(&reg, "balls", outside).unwrap();
        let ride = Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None };
        s.set_attach(rider, ride).unwrap();
        let clip = s.copy_things(&reg, &[rider]).unwrap();
        let same = s.paste_things(&reg, &clip, outside, None).unwrap();
        assert_eq!(s.things[&same.tops[0]].attach, ride);
        let moved = s.paste_things(&reg, &clip, folder, Some(a)).unwrap();
        assert_eq!(s.things[&moved.tops[0]].attach, Attach::Group);
        assert_eq!(s.children(folder)[1], moved.tops[0], "right after the thing it was pasted beside");
    }
}
