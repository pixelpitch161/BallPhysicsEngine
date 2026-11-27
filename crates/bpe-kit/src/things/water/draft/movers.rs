//! The course's moving parts (Water mode part 5a, `docs/superpowers/plans/water-mode/
//! part5-moving-parts.md`): which Pieces and Walls move, the still field the rest make with the
//! carved rock, each moving part's shape in its own frame, and where each is at a water time.

use bpe_core::ThingId;
use bpe_water::{MOST_MOVERS, Mover, MoverShape, Movers, Pose, SolidField, Water};

use super::{SolidSetup, WaterSetup};
use crate::modules::water::{CarveMask, MovesKind};

/// What the stage says about a Driven Piece or Wall (part 5b moves them).
pub const DRIVEN_LATER: &str =
    "Driven parts arrive in the next part of Water mode: this one stays where it is for now.";
/// What the stage says about a Keyframed Piece or Wall with Cuts on.
pub const MOVING_CUTS: &str = "A moving part can't cut the course: it moves as a solid.";
/// What the stage says about a Keyframed Piece or Wall past the most the water moves.
pub const TOO_MANY_MOVERS: &str = "The water moves at most 64 parts: this one stays where it is.";

/// Which of `solids` move (their places in the list, in order, at most [`MOST_MOVERS`]): each
/// Keyframed one with something to it. What the stage says about those that can't is added to
/// `notes`.
pub(super) fn moving(solids: &[SolidSetup], notes: &mut Vec<(ThingId, &'static str)>) -> Vec<usize> {
    let mut out = Vec::new();
    for (k, s) in solids.iter().enumerate() {
        match s.motion() {
            MovesKind::Fixed => {}
            MovesKind::Driven => notes.push((s.id, DRIVEN_LATER)),
            MovesKind::Keyframed if s.moving_part().is_none() => {}
            MovesKind::Keyframed if out.len() >= MOST_MOVERS => notes.push((s.id, TOO_MANY_MOVERS)),
            MovesKind::Keyframed => {
                if s.cuts() {
                    notes.push((s.id, MOVING_CUTS));
                }
                out.push(k);
            }
        }
    }
    out
}

/// The owner number a part's faces are marked with: its place among the course's parts plus 1.
pub(super) fn owner(k: usize) -> u16 {
    u16::try_from(k + 1).unwrap_or(u16::MAX)
}

impl WaterSetup {
    /// Whether any Piece or Wall moves.
    pub fn has_movers(&self) -> bool {
        !self.movers.is_empty()
    }

    /// The course without its moving parts: the rock carved as `mask` says, as wet as the Course's
    /// Rock, with every Piece and Wall that stays where it is laid over it in order.
    pub fn still_with(&self, mask: &CarveMask) -> SolidField {
        let mut field = mask.field(self.cell).with_wetting(self.wetting());
        let least = bpe_water::drops::LEAST_WALL * self.cell;
        for (k, s) in self.solids.iter().enumerate() {
            if self.movers.contains(&k) {
                continue;
            }
            if let Some(part) = s.part() {
                field.add_part(&part, owner(k), least);
            }
        }
        field
    }

    /// The moving parts on `still` (this setup's still field), each sampled once in its own frame at
    /// the setup's cell, its grid lying on the course's at its start; `None` when nothing moves.
    pub fn movers_on(&self, still: SolidField) -> Option<Movers> {
        if self.movers.is_empty() {
            return None;
        }
        let l = still.layout;
        let start = self.poses_at(0.0);
        let movers = self
            .movers
            .iter()
            .zip(&start)
            .filter_map(|(&k, pose)| {
                let s = &self.solids[k];
                // (Thin walls as thick as the physics needs at the smallest the part is keyed to.)
                let least = bpe_water::drops::LEAST_WALL * l.cell / s.least_scale();
                let shape = MoverShape::of(&s.moving_part()?, l.cell, least, l.origin - pose.at)?;
                Some(Mover { shape: std::sync::Arc::new(shape), wetting: s.wetting(), owner: owner(k), start: *pose })
            })
            .collect();
        Some(Movers { still: std::sync::Arc::new(still), movers })
    }

    /// Where each moving part is at water time `time` (seconds), in the setup's order.
    pub fn poses_at(&self, time: f64) -> Vec<Pose> {
        self.movers.iter().map(|&k| self.solids[k].pose_at(time)).collect()
    }

    /// The course's moving parts, if any, on its still field (carved as the setup's own mask).
    pub fn movers(&self) -> Option<Movers> {
        if self.movers.is_empty() {
            return None;
        }
        self.movers_on(self.still_with(&self.mask()))
    }

    /// Gives `water` (a new body on this setup's course) its moving parts where they start.
    pub(super) fn give_movers(&self, water: &mut Water) {
        if let Some(movers) = self.movers() {
            water.set_movers(movers, &self.poses_at(0.0));
        }
    }

    /// Gives `water` (a body on this setup's course made from `state`) its moving parts as the state
    /// says.
    pub(super) fn give_movers_from(&self, water: &mut Water, state: &bpe_water::WaterState) {
        if let Some(movers) = self.movers() {
            water.resume_movers(movers, state);
        }
    }
}
