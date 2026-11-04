//! Follow sound as the runtime sees it: a setting that follows a shape's sound reads it through
//! [`SoundAt`], which the sound crate provides from the physics alone (the world now and the log
//! so far), so the picture and the physics move the same way whether sound plays or not.

use std::sync::Arc;

use bpe_core::{Resolved, Scene, SoundLevels, SoundSource, ThingId};

use crate::{Log, World};

/// A scene's sound levels, worked out from its physics.
pub trait SoundAt: Send + Sync {
    /// How far `source` of shape `shape` is up at `time` (0..1), with the world as it is at
    /// that time and the log up to it.
    fn level(&self, world: &World, log: &Log, shape: ThingId, source: &SoundSource, time: f64) -> f32;
}

/// Makes a scene's [`SoundAt`]; the kit's sims carry one so every world built from them (the
/// editor's, the export's and the sound engine's) follows sound the same way.
pub trait SoundMaker: Send + Sync {
    fn make(&self, scene: &Scene, resolved: &Resolved) -> Arc<dyn SoundAt>;
}

/// A [`SoundAt`] read at one world and log, as the pulses read it.
pub struct LevelsAt<'a> {
    pub sound: &'a dyn SoundAt,
    pub world: &'a World,
    pub log: &'a Log,
}

impl SoundLevels for LevelsAt<'_> {
    fn level(&self, shape: ThingId, source: &SoundSource, time: f64) -> f32 {
        self.sound.level(self.world, self.log, shape, source, time)
    }
}
