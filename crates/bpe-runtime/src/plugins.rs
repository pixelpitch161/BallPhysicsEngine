//! What a plugin gives the runtime: how to build a thing into the world, and how a module
//! moves and bounces the things that use it.

use std::any::Any;
use std::collections::BTreeMap;

use bpe_core::{Params, ResolvedThing, Target, Thing, ThingId};
use glam::Vec2;

use crate::{Bodies, Contact, Log, World, reflect};

/// Anything a plugin keeps per thing or per module while the world runs. Implemented for every
/// `Clone + Send + Sync + 'static` type.
pub trait ThingState: Any + Send + Sync {
    fn clone_box(&self) -> Box<dyn ThingState>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Any + Clone + Send + Sync> ThingState for T {
    fn clone_box(&self) -> Box<dyn ThingState> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Clone for Box<dyn ThingState> {
    fn clone(&self) -> Self {
        // Through the reference, not the box: the box is itself a `ThingState` by the blanket
        // impl, and its `clone_box` would come straight back here.
        (**self).clone_box()
    }
}

/// The state a module prepared from its settings.
pub type ModuleState = dyn ThingState;

/// The state as its concrete type. Panics when a plugin asks for a type it didn't store.
pub fn state<T: 'static>(s: &dyn ThingState) -> &T {
    s.as_any().downcast_ref().unwrap_or_else(|| panic!("plugin state is not a {}", std::any::type_name::<T>()))
}

pub fn state_mut<T: 'static>(s: &mut dyn ThingState) -> &mut T {
    s.as_any_mut().downcast_mut().unwrap_or_else(|| panic!("plugin state is not a {}", std::any::type_name::<T>()))
}

/// Limits the simulation puts on every ball.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// The fastest anything may move, in pixels per second: 16 px × 64 substeps × the step rate.
    pub ceiling: f32,
    /// Touches slower than this are resting, not hits: `|gravity| × dt × 4 + 1`.
    pub resting: f32,
}

/// What a Bounce module says about hitting other balls.
#[derive(Clone, Debug, PartialEq)]
pub struct PairRule {
    /// Only balls on things this points at are bumped into; both sides have to agree.
    pub target: Target,
    /// How much sideways speed rubs off, 0 to 1.
    pub friction: f32,
}

/// How a thing kind takes part in the simulation.
pub trait ThingSim: Send + Sync + 'static {
    /// The thing kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// Puts the thing into the world: its bodies, its wall, its state.
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World);
    /// Whether `build` reads the walls other things put into the world. Such things are built
    /// after everything else, whatever their place in the tree.
    fn needs_walls(&self) -> bool {
        false
    }
    /// The Spawn stage, first in every step: emitters add bodies (to any thing) and log
    /// `Spawn` events for them.
    fn spawn(&self, _thing: ThingId, _world: &mut World, _log: &mut Log) {}
    /// Runs once a step before the physics: scripted motion such as a wall's spin.
    fn scripted(&self, _thing: ThingId, _world: &mut World) {}
    /// The Forces stage of every substep, before the Motion modules: fields push bodies.
    fn forces(&self, _thing: ThingId, _world: &mut World, _dt: f32) {}
    /// After Integrate and before Collide in every substep: a liquid relaxes its particles.
    fn settle(&self, _thing: ThingId, _world: &mut World, _dt: f32) {}
    /// The Events stage, last in every step, with the log: zone entries and leaves, a soft
    /// body's impacts, the scene's beats.
    fn after_step(&self, _thing: ThingId, _world: &mut World, _log: &mut Log) {}
    /// A keyframe or pulse moved one of the thing's settings while the world runs: re-read
    /// whatever can change without rebuilding. `resolved` holds the settings as they are now.
    fn settings_changed(&self, _thing: ThingId, _resolved: &ResolvedThing, _world: &mut World) {}
    /// Whether changing this setting changes the simulation. Visual settings say no, so the
    /// editor keeps playing through them.
    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
}

/// How a Motion or Bounce module moves and bounces the things that use it.
pub trait ModuleSim: Send + Sync + 'static {
    /// The module kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// Reads the settings once, into whatever the module wants to keep.
    fn prepare(&self, params: &Params) -> Box<dyn ThingState>;
    /// Changes velocities before the move: gravity, drag, wind.
    fn forces(&self, _state: &dyn ThingState, _bodies: &mut Bodies, _gravity: Vec2, _dt: f32) {}
    /// Answers a contact for one item. The default bounce keeps all the speed.
    fn respond(&self, _state: &dyn ThingState, contact: &Contact, bodies: &mut Bodies, item: usize, _limits: &Limits) {
        bodies.velocity[item] = reflect(bodies.velocity[item], contact, 1.0);
    }
    /// Whether this module, on the thing that was hit, decides the bounce instead of the mover's.
    fn takes_over(&self, _state: &dyn ThingState) -> bool {
        false
    }
    /// How much speed a ball keeps when it hits another ball: 1 keeps it all.
    fn bounciness(&self, _state: &dyn ThingState) -> f32 {
        1.0
    }
    /// `Some` when the balls using this module bump into other balls, and which ones.
    fn pairs(&self, _state: &dyn ThingState) -> Option<PairRule> {
        None
    }
    /// The things against which [`ModuleSim::respond_against`] is told the hit is special.
    fn special_target(&self, _state: &dyn ThingState) -> Option<Target> {
        None
    }
    /// Answers a contact for one item against a wall or solid; `special` says the thing hit
    /// matches [`ModuleSim::special_target`]. The default ignores that and calls `respond`.
    fn respond_against(
        &self,
        state: &dyn ThingState,
        contact: &Contact,
        bodies: &mut Bodies,
        item: usize,
        limits: &Limits,
        _special: bool,
    ) {
        self.respond(state, contact, bodies, item, limits);
    }
    /// Finishes a hit against another ball, after the two have swapped momentum: speed gain,
    /// growing, anything `respond` does beyond the bounce itself.
    fn respond_pair(&self, _state: &dyn ThingState, _bodies: &mut Bodies, _item: usize, _speed: f32, _limits: &Limits) {
    }
    /// What a real hit at `speed` does to the ball itself afterwards, against anything: counting
    /// down to popping. Returns `true` when the ball should be removed.
    fn effects(
        &self,
        _state: &dyn ThingState,
        _bodies: &mut Bodies,
        _item: usize,
        _speed: f32,
        _limits: &Limits,
    ) -> bool {
        false
    }
}

/// Every registered simulation part, by kind id.
#[derive(Default)]
pub struct Sims {
    things: BTreeMap<&'static str, Box<dyn ThingSim>>,
    modules: BTreeMap<&'static str, Box<dyn ModuleSim>>,
}

impl Sims {
    pub fn add_thing(&mut self, s: impl ThingSim) {
        let id = s.kind();
        assert!(!self.things.contains_key(id), "sim registered twice: {id}");
        self.things.insert(id, Box::new(s));
    }

    pub fn add_module(&mut self, s: impl ModuleSim) {
        let id = s.kind();
        assert!(!self.modules.contains_key(id), "sim registered twice: {id}");
        self.modules.insert(id, Box::new(s));
    }

    pub fn thing(&self, kind: &str) -> Option<&dyn ThingSim> {
        self.things.get(kind).map(|s| s.as_ref())
    }

    pub fn module(&self, kind: &str) -> Option<&dyn ModuleSim> {
        self.modules.get(kind).map(|s| s.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_state_clones_its_contents() {
        let a: Box<dyn ThingState> = Box::new(7u32);
        let b = a.clone();
        assert_eq!(*state::<u32>(b.as_ref()), 7);
    }

    #[test]
    #[should_panic(expected = "plugin state is not a f32")]
    fn asking_for_the_wrong_type_names_it() {
        let a: Box<dyn ThingState> = Box::new(7u32);
        state::<f32>(a.as_ref());
    }
}
