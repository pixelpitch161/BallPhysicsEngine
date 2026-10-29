//! Undo and redo: whole scenes, because a scene is small and comparing two is cheap.

use bpe_core::Scene;
use std::time::{Duration, Instant};

/// Edits closer together than this, with no mouse button held, undo as one step.
pub const UNDO_MERGE: Duration = Duration::from_millis(600);
/// Undo steps kept.
pub const UNDO_LIMIT: usize = 200;

#[derive(Default)]
pub struct History {
    pub(crate) undo: Vec<Scene>,
    pub(crate) redo: Vec<Scene>,
    /// The latest undo step is still growing: the mouse is down or edits are coming quickly.
    pub(crate) merging: bool,
    pub(crate) last_edit: Option<Instant>,
}

impl History {
    /// Records `before` as an undo step, unless this edit continues the previous one.
    /// A `separate` edit, like deleting a thing, is always its own step.
    pub fn record(&mut self, before: Scene, separate: bool) {
        if !self.merging || separate {
            self.undo.push(before);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.merging = !separate;
        }
        self.last_edit = Some(Instant::now());
    }

    /// Whether the open step should close now: no pointer held and the last edit a while ago.
    pub fn merge_expired(&self, pointer_down: bool) -> bool {
        self.merging && !pointer_down && self.last_edit.is_none_or(|t| t.elapsed() >= UNDO_MERGE)
    }

    /// Closes the open undo step, so the next edit starts a new one.
    pub fn end_merging(&mut self) {
        self.merging = false;
    }

    /// Puts the previous scene into `current`. False when there is nothing to undo.
    pub fn undo(&mut self, current: &mut Scene) -> bool {
        let Some(scene) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(current, scene));
        self.merging = false;
        true
    }

    /// Puts the undone scene back into `current`. False when there is nothing to redo.
    pub fn redo(&mut self, current: &mut Scene) -> bool {
        let Some(scene) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(current, scene));
        self.merging = false;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Scene;

    fn scene_named(name: &str) -> Scene {
        let mut s = bpe_kit::new_scene(&bpe_kit::registry());
        s.name = name.into();
        s
    }

    #[test]
    fn quick_edits_merge_and_separate_edits_dont() {
        let mut h = History::default();
        let (a, b, c) = (scene_named("a"), scene_named("b"), scene_named("c"));
        h.record(a.clone(), false);
        h.record(b.clone(), false); // still merging: dropped
        assert_eq!(h.undo.len(), 1);
        h.record(c.clone(), true);
        assert_eq!(h.undo.len(), 2);
        let mut current = scene_named("d");
        assert!(h.undo(&mut current) && current == c);
        assert!(h.redo(&mut current) && current.name == "d");
    }

    #[test]
    fn the_undo_stack_is_capped_and_ending_a_merge_separates_the_next_edit() {
        let mut h = History::default();
        for i in 0..(UNDO_LIMIT + 5) {
            h.record(scene_named(&i.to_string()), true);
        }
        assert_eq!(h.undo.len(), UNDO_LIMIT);
        assert_eq!(h.undo[0].name, "5");
        h.record(scene_named("x"), false);
        h.end_merging();
        h.record(scene_named("y"), false);
        assert_eq!(h.undo.len(), UNDO_LIMIT);
        assert_eq!(h.undo.last().unwrap().name, "y");
        let mut current = scene_named("z");
        assert!(!History::default().undo(&mut current));
        assert!(!History::default().redo(&mut current));
    }
}
