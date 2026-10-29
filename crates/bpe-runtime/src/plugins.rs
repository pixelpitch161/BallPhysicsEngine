//! What a plugin gives the runtime: how to build a thing into the world, and how a module
//! moves and bounces the things that use it.

use std::any::Any;
use std::collections::BTreeMap;

use bpe_core::{Params, ResolvedThing, Thing, ThingId};
use glam::Vec2;

use crate::{Bodies, Contact, World, reflect};

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

/// How a thing kind takes part in the simulation.
pub trait ThingSim: Send + Sync + 'static {
    /// The thing kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// Puts the thing into the world: its bodies, its wall, its state.
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World);
    /// Runs once a step before the physics: scripted motion such as a wall's spin.
    fn scripted(&self, _thing: ThingId, _world: &mut World) {}
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
