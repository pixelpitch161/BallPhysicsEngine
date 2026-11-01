//! Which things are selected: one primary thing the inspector shows, and any number of extra
//! ones that edits apply to as well.

use std::collections::BTreeSet;

use bpe_core::{Scene, ThingId};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub primary: Option<ThingId>,
    pub extra: BTreeSet<ThingId>,
}

impl Selection {
    /// Selects `id` alone.
    pub fn set(&mut self, id: ThingId) {
        self.primary = Some(id);
        self.extra.clear();
    }

    /// Ctrl-click: `id` joins the selection, or leaves it again. Without a primary it becomes
    /// the primary; taking the primary out hands its place to the first extra.
    pub fn toggle(&mut self, id: ThingId) {
        if self.primary == Some(id) {
            self.primary = self.extra.pop_first();
        } else if !self.extra.remove(&id) {
            if self.primary.is_none() {
                self.primary = Some(id);
            } else {
                self.extra.insert(id);
            }
        }
    }

    /// Shift-click: every row between the primary and `to` in `visible` joins the selection.
    /// The primary stays what it was.
    pub fn range(&mut self, to: ThingId, visible: &[ThingId]) {
        let Some(primary) = self.primary else {
            self.set(to);
            return;
        };
        let at = |id| visible.iter().position(|&v| v == id);
        let (Some(a), Some(b)) = (at(primary), at(to)) else {
            self.extra.insert(to);
            return;
        };
        for &id in &visible[a.min(b)..=a.max(b)] {
            if id != primary {
                self.extra.insert(id);
            }
        }
    }

    /// The selected things that are still in the scene: the primary first, then the extras in
    /// tree order.
    pub fn all(&self, scene: &Scene) -> Vec<ThingId> {
        let mut out: Vec<ThingId> = self.primary.filter(|p| scene.things.contains_key(p)).into_iter().collect();
        out.extend(scene.order.iter().copied().filter(|id| self.extra.contains(id) && Some(*id) != self.primary));
        out
    }

    pub fn contains(&self, id: ThingId) -> bool {
        self.primary == Some(id) || self.extra.contains(&id)
    }

    /// Forgets `ids`, for instance because they were deleted.
    pub fn remove(&mut self, ids: &[ThingId]) {
        if self.primary.is_some_and(|p| ids.contains(&p)) {
            self.primary = None;
        }
        for id in ids {
            self.extra.remove(id);
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_ranging_and_listing_a_selection() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let c = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let d = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let mut s = Selection::default();
        s.set(ThingId(1));
        s.toggle(d);
        assert_eq!(s.all(&scene), vec![ThingId(1), d]);
        assert!(s.contains(d));
        s.toggle(d);
        assert_eq!(s.all(&scene), vec![ThingId(1)]);
        s.range(c, &scene.order);
        assert_eq!(s.all(&scene), vec![ThingId(1), ThingId(2), c]);
        s.remove(&[ThingId(1)]);
        assert_eq!(s.primary, None);
        assert!(s.contains(c) && !s.contains(d));
        // Taking the primary out by toggling hands the place on; a missing thing is skipped.
        s.set(d);
        s.toggle(c);
        s.toggle(d);
        assert_eq!(s.primary, Some(c));
        scene.remove_thing(c).unwrap();
        assert!(s.all(&scene).is_empty());
        s.clear();
        assert_eq!(s, Selection::default());
    }

    #[test]
    fn a_range_without_a_primary_just_selects() {
        let mut s = Selection::default();
        s.range(ThingId(2), &[ThingId(0), ThingId(1), ThingId(2)]);
        assert_eq!(s.primary, Some(ThingId(2)));
        assert!(s.extra.is_empty());
        // A row that isn't visible still joins.
        s.range(ThingId(7), &[ThingId(0), ThingId(1), ThingId(2)]);
        assert!(s.contains(ThingId(7)));
    }
}
