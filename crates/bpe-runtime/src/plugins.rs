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
    /// Touches slower than this are resting, not hits: `|gravity| × dt × 4 + 1`. A ball whose
    /// Motion pulls harder (Gravity ×, Wind) rests at more along its pull: see
    /// [`Limits::resting_along`].
    pub resting: f32,
    /// The ball's own pull, four steps' worth (`pull × dt × 4`): zero for the scene's limits.
    pub press: glam::Vec2,
}

impl Limits {
    /// How slow a touch along `normal` is to be resting: [`Limits::resting`], or more where the
    /// ball's own pull presses it that way harder than the scene's gravity would (a ball lying
    /// on a floor under Gravity × 5, or pinned to a side wall by Wind). Across its pull it is
    /// the scene's, so a pull one way never hides real bounces the other way.
    pub fn resting_along(&self, normal: glam::Vec2) -> f32 {
        self.resting.max(self.press.dot(normal).abs() + 1.0)
    }
}

/// What a Bounce module says about hitting other balls.
#[derive(Clone, Debug, PartialEq)]
pub struct PairRule {
    /// Only balls on things this points at are bumped into; both sides have to agree.
    pub target: Target,
    /// How much sideways speed rubs off, 0 to 1.
    pub friction: f32,
}

/// What a module's step knows about the world around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimCx {
    /// The thing whose module is running.
    pub thing: ThingId,
    /// Seconds since the start.
    pub time: f32,
    /// The scene's seed, for anything random.
    pub seed: u64,
}

/// How a thing kind takes part in the simulation.
pub trait ThingSim: Send + Sync + 'static {
    /// The thing kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// Puts the thing into the world: its bodies, its wall, its state.
    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World);
    /// Runs once every thing is built, in the world's order, for what depends on the whole
    /// scene.
    fn after_build(&self, _thing: ThingId, _world: &mut World) {}
    /// Whether `build` reads the walls other things put into the world. Such things are built
    /// after everything else, whatever their place in the tree.
    fn needs_walls(&self) -> bool {
        false
    }
    /// The Spawn stage, first in every step: a thing may add bodies (to any thing).
    fn spawn(&self, _thing: ThingId, _world: &mut World, _log: &mut Log) {}
    /// Runs once a step before the physics: scripted motion such as a wall's spin.
    fn scripted(&self, _thing: ThingId, _world: &mut World) {}
    /// The Forces stage of every substep, before the Motion modules.
    fn forces(&self, _thing: ThingId, _world: &mut World, _dt: f32) {}
    /// After Integrate and before Collide in every substep.
    fn settle(&self, _thing: ThingId, _world: &mut World, _dt: f32) {}
    /// The Events stage, last in every step, with the log.
    fn after_step(&self, _thing: ThingId, _world: &mut World, _log: &mut Log) {}
    /// A keyframe or pulse moved one of the thing's settings while the world runs, or a setting of
    /// one of its Motion or Bounce cards that no module sim steps: re-read whatever can change
    /// without rebuilding. `resolved` holds the settings (and cards) as they are now.
    fn settings_changed(&self, _thing: ThingId, _resolved: &ResolvedThing, _world: &mut World) {}
    /// Whether the thing steps with the physics. A thing that says no is decoration stepped beside
    /// it (a Particles thing that never pushes back): it reads the balls and walls but changes
    /// nothing any other thing reads, so a world built without it ([`crate::main_part`]) steps every
    /// ball the same. The fingerprints leave it out, and the editor steps it on a playback of its own.
    fn in_main(&self, _resolved: &ResolvedThing) -> bool {
        true
    }
    /// Whether changing this setting changes the simulation. Visual settings say no, so the
    /// editor keeps playing through them.
    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
    /// How much of its balls' past the thing wants kept, for its trails and particles; `None`
    /// keeps none. Asked when the world is built, and hashed into the world's fingerprint.
    fn history(&self, _resolved: &ResolvedThing) -> Option<crate::HistoryWant> {
        None
    }
    /// Where body row `row` of the thing sits in the spreads its varied settings follow (see
    /// [`bpe_core::vary`]): its number in the group and how many there are, its round, launch
    /// angle and place. `None` leaves the runtime's own: the row's number of all the rows, the rest
    /// 0. The runtime fills in the speed and hits itself.
    fn ball_spread(&self, _thing: ThingId, _world: &World, _row: usize) -> Option<bpe_core::BallSpread> {
        None
    }
    /// Whether the thing needs the scene's physics fingerprint while it runs
    /// ([`World::fingerprint`]): a thing that caches work it can do again by it. Worked out once,
    /// in [`World::build`], only when some thing asks.
    fn needs_fingerprint(&self, _resolved: &ResolvedThing) -> bool {
        false
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
    /// Whether a card of this kind can vary across its balls: the runtime then prepares a state
    /// per ball (see [`bpe_core::vary`]), answers each ball's contacts with its own, and moves
    /// each ball with [`ModuleSim::forces_one`] instead of [`ModuleSim::forces_at`].
    fn per_ball(&self) -> bool {
        false
    }
    /// [`ModuleSim::forces`] on body row `item` alone, for a card that varies across its balls.
    fn forces_one(
        &self,
        _state: &dyn ThingState,
        _bodies: &mut Bodies,
        _item: usize,
        _gravity: Vec2,
        _dt: f32,
        _cx: &SimCx,
    ) {
    }
    /// How this Motion card's balls are drawn to the lights or pushed from them (Moths and light
    /// pressure); `None` when they aren't. Read from each ball's own state when it varies.
    fn light_pull(&self, _state: &dyn ThingState) -> Option<crate::LightPull> {
        None
    }
    /// The steady pull, in pixels per second squared, this Motion module puts on its balls
    /// under the scene's `gravity`: what a ball lying on something presses into it with, so a
    /// resting touch is told from a hit. `None` for a module that doesn't pull steadily.
    fn pull(&self, _state: &dyn ThingState, _gravity: Vec2) -> Option<Vec2> {
        None
    }
    /// Like [`ModuleSim::forces`], also told which thing, what time it is and the scene's seed,
    /// for forces that change over time or use random numbers. The default calls `forces`.
    fn forces_at(&self, state: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32, _cx: &SimCx) {
        self.forces(state, bodies, gravity, dt)
    }
    /// Answers a contact for one item. The default bounce keeps all the speed.
    fn respond(&self, _state: &dyn ThingState, contact: &Contact, bodies: &mut Bodies, item: usize, _limits: &Limits) {
        bodies.velocity[item] = reflect(bodies.velocity[item], contact, 1.0);
    }
    /// Like [`ModuleSim::respond_against`], also told which thing moved, what time it is and
    /// the scene's seed. The default calls `respond_against`.
    #[allow(clippy::too_many_arguments)]
    fn respond_at(
        &self,
        state: &dyn ThingState,
        contact: &Contact,
        bodies: &mut Bodies,
        item: usize,
        limits: &Limits,
        special: bool,
        _cx: &SimCx,
    ) {
        self.respond_against(state, contact, bodies, item, limits, special)
    }
    /// How the balls using this module meet walls: by their edge, or by their middle.
    fn touch(&self, _state: &dyn ThingState) -> crate::Touch {
        crate::Touch::Edge
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
    /// Answers a contact for one item against a wall; `special` says the thing hit
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
    /// Works out a scene's sound for physics settings that follow it (none: they stay put).
    pub sound: Option<std::sync::Arc<dyn crate::SoundMaker>>,
    /// Works out a scene's light for physics that follows it (none: it stays put).
    pub light: Option<std::sync::Arc<dyn crate::LightMaker>>,
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
