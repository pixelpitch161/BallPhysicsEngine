//! Undo and redo: whole scenes, because a scene is small and comparing two is cheap. Steps are
//! kept up to a count and up to a size, so a scene with big brush strokes or paths can't fill the
//! memory with copies of itself.

use bpe_core::Scene;
use std::time::{Duration, Instant};

/// Edits closer together than this, with no mouse button held, undo as one step.
pub const UNDO_MERGE: Duration = Duration::from_millis(600);
/// Undo steps kept.
pub const UNDO_LIMIT: usize = 200;
/// The most the undo and redo steps may take together, as scene text (about what they hold in
/// memory); past it the oldest undo steps go. The newest step always stays.
pub const UNDO_BYTES: usize = 256 * 1024 * 1024;

/// One undo or redo step: the scene, and its size as scene text.
pub struct Step {
    pub scene: Scene,
    pub bytes: usize,
}

impl Step {
    pub fn of(scene: Scene) -> Self {
        let bytes = scene_bytes(&scene);
        Self { scene, bytes }
    }
}

/// How long `scene` is as scene text, counted without writing the text out.
pub fn scene_bytes(scene: &Scene) -> usize {
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0 += s.len();
            Ok(())
        }
    }
    let mut count = Count(0);
    // A scene that can't be written (never, in practice) counts as small.
    let _ = ron::ser::to_writer(&mut count, scene);
    count.0
}

#[derive(Default)]
pub struct History {
    pub(crate) undo: Vec<Step>,
    pub(crate) redo: Vec<Step>,
    /// The latest undo step is still growing: the mouse is down or edits are coming quickly.
    pub(crate) merging: bool,
    pub(crate) last_edit: Option<Instant>,
}

impl History {
    /// Records `before` as an undo step, unless this edit continues the previous one.
    /// A `separate` edit, like deleting a thing, is always its own step.
    pub fn record(&mut self, before: Scene, separate: bool) {
        if !self.merging || separate {
            self.undo.push(Step::of(before));
            self.redo.clear();
            self.trim();
            self.merging = !separate;
        }
        self.last_edit = Some(Instant::now());
    }

    /// Lets the oldest undo steps go past [`UNDO_LIMIT`] steps or [`UNDO_BYTES`] in all, never
    /// the newest.
    fn trim(&mut self) {
        let mut bytes: usize = self.undo.iter().chain(&self.redo).map(|s| s.bytes).sum();
        let mut gone = 0;
        let newest = self.undo.len().saturating_sub(1);
        while gone < newest && (self.undo.len() - gone > UNDO_LIMIT || bytes > UNDO_BYTES) {
            bytes -= self.undo[gone].bytes;
            gone += 1;
        }
        self.undo.drain(..gone);
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
        let Some(step) = self.undo.pop() else { return false };
        self.redo.push(Step::of(std::mem::replace(current, step.scene)));
        self.merging = false;
        true
    }

    /// Puts the undone scene back into `current`. False when there is nothing to redo.
    pub fn redo(&mut self, current: &mut Scene) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        self.undo.push(Step::of(std::mem::replace(current, step.scene)));
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
        assert_eq!(h.undo[0].scene.name, "5");
        h.record(scene_named("x"), false);
        h.end_merging();
        h.record(scene_named("y"), false);
        assert_eq!(h.undo.len(), UNDO_LIMIT);
        assert_eq!(h.undo.last().unwrap().scene.name, "y");
        let mut current = scene_named("z");
        assert!(!History::default().undo(&mut current));
        assert!(!History::default().redo(&mut current));
    }

    #[test]
    fn the_undo_steps_are_capped_by_size_too_and_the_newest_always_stays() {
        let mut h = History::default();
        let one = scene_bytes(&scene_named("0"));
        assert!(one > 100, "a scene counts as its text: {one} bytes");
        // Steps as big as a quarter of the cap each: four fit, the fifth pushes the oldest out.
        for i in 0..6 {
            h.record(scene_named(&i.to_string()), true);
            h.undo.last_mut().unwrap().bytes = UNDO_BYTES / 4;
        }
        // Each record trims before its own size is set above, so five are checked against the cap.
        h.record(scene_named("6"), true);
        assert!(h.undo.iter().map(|s| s.bytes).sum::<usize>() <= UNDO_BYTES + one);
        assert_eq!(h.undo.last().unwrap().scene.name, "6");
        assert!(h.undo.len() < 7, "the oldest went: {} kept", h.undo.len());
        // One step bigger than the cap on its own still stays.
        let mut h = History::default();
        h.record(scene_named("big"), true);
        h.undo[0].bytes = UNDO_BYTES * 2;
        h.record(scene_named("next"), true);
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0].scene.name, "next");
        h.undo[0].bytes = UNDO_BYTES * 2;
        h.trim();
        assert_eq!(h.undo.len(), 1, "the newest stays");
    }
}
