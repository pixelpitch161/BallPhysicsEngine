//! Copying, pasting and duplicating things (editor workflow, W5): the editor's clipboard and
//! the pure scene edits behind Ctrl+C, Ctrl+V, Ctrl+D and the tree's Copy, Paste and Duplicate.
//! The copying itself is `bpe_core`'s ([`Scene::copy_things`], [`Scene::paste_things`]).

use bpe_core::{Address, Key, Pasted, Registry, Scene, ThingClip, ThingId};

/// Keys copied from the timeline: each with its setting and how long after the first it
/// comes.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyClip {
    pub keys: Vec<(Address, f64, Key)>,
}

impl KeyClip {
    /// The one thing every copied key belongs to, if they all belong to one.
    pub fn thing(&self) -> Option<ThingId> {
        let first = self.keys.first()?.0.thing;
        self.keys.iter().all(|(a, ..)| a.thing == first).then_some(first)
    }
}

/// What was copied last: things or timeline keys. It lasts until the editor closes, through New
/// and Open, so things can be pasted into another scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Clipboard {
    #[default]
    Empty,
    Things(ThingClip),
    Keys(KeyClip),
}

impl Clipboard {
    /// The copied things' names in a few words for a menu ("Balls", "3 things"), with things
    /// copied.
    pub fn things_words(&self) -> Option<String> {
        let Clipboard::Things(clip) = self else { return None };
        Some(match clip.top_names().as_slice() {
            [one] => (*one).to_owned(),
            names => format!("{} things", names.len()),
        })
    }
}

/// The selected things that aren't inside another selected thing, in tree order.
pub fn tops(scene: &Scene, ids: &[ThingId]) -> Vec<ThingId> {
    scene
        .order
        .iter()
        .copied()
        .filter(|id| ids.contains(id) && *id != ThingId::ROOT)
        .filter(|id| !scene.ancestors(*id).iter().any(|a| ids.contains(a)))
        .collect()
}

/// Where a paste goes with `primary` selected: into a selected folder (last in it), beside
/// another selected thing (right after it), or under the scene settings.
pub fn paste_place(scene: &Scene, primary: Option<ThingId>) -> (ThingId, Option<ThingId>) {
    match primary.and_then(|p| scene.things.get(&p)) {
        Some(t) if t.id == ThingId::ROOT => (ThingId::ROOT, None),
        Some(t) if t.kind == "folder" => (t.id, None),
        Some(t) => (t.parent.unwrap_or(ThingId::ROOT), Some(t.id)),
        None => (ThingId::ROOT, None),
    }
}

/// Duplicates every selected thing (the tops of `ids`) with everything under it, each copy
/// right after its original under the same parent. Returns the copies, in order.
pub fn duplicate(scene: &mut Scene, reg: &Registry, ids: &[ThingId]) -> Vec<ThingId> {
    let mut copies = Vec::new();
    for top in tops(scene, ids) {
        let Some(parent) = scene.things.get(&top).and_then(|t| t.parent) else { continue };
        let Some(clip) = scene.copy_things(reg, &[top]) else { continue };
        if let Ok(pasted) = scene.paste_things(reg, &clip, parent, Some(top)) {
            copies.extend(pasted.tops);
        }
    }
    copies
}

/// What the status line says after a paste of `clip` made `pasted`.
pub fn pasted_words(clip: &ThingClip, pasted: &Pasted) -> String {
    let what = match (clip.top_names().as_slice(), pasted.tops.len()) {
        ([one], 1) => (*one).to_owned(),
        (_, n) => format!("{n} things"),
    };
    let mut words = format!("Pasted {what}.");
    match pasted.unlinked {
        0 => {}
        1 => words.push_str(" 1 setting pointed at a thing that isn't in this scene and now points at anything."),
        n => words
            .push_str(&format!(" {n} settings pointed at things that aren't in this scene and now point at anything.")),
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::SlotId;

    fn named(scene: &Scene, name: &str) -> ThingId {
        scene.things.values().find(|t| t.name == name).map(|t| t.id).unwrap_or_else(|| panic!("no {name}"))
    }

    #[test]
    fn duplicate_copies_each_selected_thing_with_what_is_under_it() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = Scene::empty(reg);
        bpe_kit::add_thing(&mut scene, reg, "camera", ThingId::ROOT).unwrap();
        let folder = bpe_kit::add_thing(&mut scene, reg, "folder", ThingId::ROOT).unwrap();
        let balls = bpe_kit::add_thing(&mut scene, reg, "balls", folder).unwrap();
        let shape = bpe_kit::add_thing(&mut scene, reg, "shape", ThingId::ROOT).unwrap();
        let camera = named(&scene, "Camera");
        // The folder and the ball inside it: the ball comes once, with the folder.
        let copies = duplicate(&mut scene, reg, &[balls, folder, shape, camera]);
        assert_eq!(copies.len(), 2, "the camera isn't duplicated");
        let folder_copy = copies[0];
        assert_eq!(scene.things[&folder_copy].name, "Folder copy");
        assert_eq!(scene.children(folder_copy).len(), 1, "the ball came with its folder");
        assert_eq!(scene.children(ThingId::ROOT), [camera, folder, folder_copy, shape, copies[1]]);
        assert!(scene.card(scene.children(folder_copy)[0], SlotId::Look).is_some(), "with its cards");
    }

    #[test]
    fn paste_goes_into_a_folder_beside_a_thing_or_under_the_scene() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = Scene::empty(reg);
        let folder = bpe_kit::add_thing(&mut scene, reg, "folder", ThingId::ROOT).unwrap();
        let balls = bpe_kit::add_thing(&mut scene, reg, "balls", folder).unwrap();
        assert_eq!(paste_place(&scene, Some(folder)), (folder, None));
        assert_eq!(paste_place(&scene, Some(balls)), (folder, Some(balls)));
        assert_eq!(paste_place(&scene, None), (ThingId::ROOT, None));
        assert_eq!(paste_place(&scene, Some(ThingId::ROOT)), (ThingId::ROOT, None));
    }

    #[test]
    fn the_status_line_names_what_was_pasted_and_what_lost_its_link() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = Scene::empty(reg);
        let balls = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
        let clip = scene.copy_things(reg, &[balls]).unwrap();
        let mut pasted = Pasted { tops: vec![ThingId(9)], ..Pasted::default() };
        assert_eq!(pasted_words(&clip, &pasted), "Pasted Balls.");
        pasted.unlinked = 2;
        assert!(pasted_words(&clip, &pasted).contains("2 settings pointed"));
        assert_eq!(Clipboard::Things(clip).things_words().as_deref(), Some("Balls"));
        assert_eq!(Clipboard::Empty.things_words(), None);
    }
}
