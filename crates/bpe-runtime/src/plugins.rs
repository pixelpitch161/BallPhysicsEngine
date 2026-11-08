//! What a plugin gives the runtime: how to build a thing into the world, and how a module
//! moves and bounces the things that use it.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::sync::Arc;

use bpe_core::{Params, ResolvedThing, Target, Thing, ThingId};
use glam::Vec2;

use crate::{Bodies, Contact, Log, World, reflect};

/// Anything a plugin keeps per thing or per module while the world runs. Implemented for every
/// `Clone + Send + Sync + 'static` type.
///
/// That includes `Box<dyn ThingState>` and `Arc<dyn ThingState>` themselves (both are `Clone`),
/// so with this trait in scope `boxed.as_any()` reaches the box's impl before the state's, and
/// `&boxed` passed as a `&dyn ThingState` makes the box the trait object. The impl therefore
/// forwards a pointer to the state inside it: every way of asking reaches the state.
pub trait ThingState: Any + Send + Sync {
    fn clone_box(&self) -> Box<dyn ThingState>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// The state `s` points at, when `T` is a pointer to a state rather than a state. The `TypeId`
/// tests are constant for each `T`, so a real state pays nothing.
fn pointee<T: Any>(s: &T) -> Option<&dyn ThingState> {
    if TypeId::of::<T>() == TypeId::of::<Box<dyn ThingState>>() {
        return (s as &dyn Any).downcast_ref::<Box<dyn ThingState>>().map(|b| &**b);
    }
    if TypeId::of::<T>() == TypeId::of::<Arc<dyn ThingState>>() {
        return (s as &dyn Any).downcast_ref::<Arc<dyn ThingState>>().map(|a| &**a);
    }
    None
}

impl<T: Any + Clone + Send + Sync> ThingState for T {
    fn clone_box(&self) -> Box<dyn ThingState> {
        match pointee(self) {
            Some(inner) => inner.clone_box(),
            None => Box::new(self.clone()),
        }
    }
    fn as_any(&self) -> &dyn Any {
        match pointee(self) {
            Some(inner) => inner.as_any(),
            None => self,
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        if TypeId::of::<T>() == TypeId::of::<Box<dyn ThingState>>() {
            let b = (self as &mut dyn Any).downcast_mut::<Box<dyn ThingState>>().unwrap();
            return (**b).as_any_mut();
        }
        if TypeId::of::<T>() == TypeId::of::<Arc<dyn ThingState>>() {
            let a = (self as &mut dyn Any).downcast_mut::<Arc<dyn ThingState>>().unwrap();
            let inner = Arc::get_mut(a).expect("a shared plugin state can't be changed");
            return inner.as_any_mut();
        }
        self
    }
}

impl Clone for Box<dyn ThingState> {
    fn clone(&self) -> Self {
        // Straight to the state: the box is itself a `ThingState` by the blanket impl, whose
        // `clone_box` would only forward here again.
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
    /// For a thing stepped beside the physics: a hash of whatever else its stepping reads from its
    /// settings beyond its own and its Motion and Bounce cards (a Particles thing's Look asking it
    /// to keep each particle's trail), so a change of it rebuilds the playback it is stepped on.
    /// 0 when nothing else is read.
    fn more_fingerprint(&self, _resolved: &ResolvedThing) -> u64 {
        0
    }
    /// For a thing stepped beside the physics: whether the sound hears what it logs (a Particles
    /// thing whose sound is on logs its moments), with a hash of what of its settings changes what
    /// it logs. The sound engine's world keeps a heard thing ([`crate::heard_part`]), and
    /// [`crate::world::sound_fingerprint`] counts it.
    fn heard(&self, _resolved: &ResolvedThing) -> Option<u64> {
        None
    }
    /// Whether the thing reads its own Motion and Bounce cards (they have no module sims): when it
    /// steps with the physics, the fingerprint then hashes them whole, layers and all.
    fn own_cards(&self) -> bool {
        false
    }
    /// Whether the thing's pulses on hits move while the world runs. The physics' never do (the
    /// world passes them no hits); decoration stepped beside it may (a Particles thing's Rate).
    fn pulses_on_hits(&self) -> bool {
        false
    }
    /// Whether changing this setting changes the simulation. Visual settings say no, so the
    /// editor keeps playing through them.
    fn affects_physics(&self, _key: &str) -> bool {
        true
    }
    /// Whether changing this setting changes the things stepped beside the physics although it
    /// isn't physics (the scene's tempo, which particles born on the beat read): the editor's
    /// particle playback starts over when it changes ([`crate::beside_fingerprint`]).
    fn affects_beside(&self, _key: &str) -> bool {
        false
    }
    /// For a thing stepped beside the physics: whether setting `key` of its card in `slot` (not
    /// Motion or Bounce, which always count) changes how it steps, so the editor's particle
    /// playback starts over when it changes (a Particles thing's Look Engine).
    fn beside_card_key(&self, _slot: bpe_core::SlotId, _key: &str) -> bool {
        false
    }
    /// Whether setting `key` of this thing, where it sits in `scene`, changes the simulation: what
    /// the physics animates and the fingerprint hashes. [`ThingSim::affects_physics`] unless the
    /// thing's place says otherwise (a Text on the screen has no wall, so its words aren't physics).
    fn affects_physics_in(&self, _scene: &bpe_core::Scene, _thing: &Thing, key: &str) -> bool {
        self.affects_physics(key)
    }
    /// What else of the saved scene changes this thing's physics, beyond its settings: hashed into
    /// the physics fingerprint when not 0 (a Text's Stuck to the screen, the font it bounces in).
    fn fingerprint_more(&self, _scene: &bpe_core::Scene, _thing: &Thing, _resolved: &ResolvedThing) -> u64 {
        0
    }

    /// Whether this thing, with its settings `resolved`, reads the saved scene while it builds or
    /// steps: the world then keeps a copy (`World::tree`).
    fn reads_tree(&self, _resolved: &ResolvedThing) -> bool {
        false
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
    /// A rule adds `ball` to the thing (Split, Spawn balls): its body row is pushed and whatever
    /// the thing keeps per row is kept in step. The row, or `None` when the thing has no balls to
    /// add to.
    fn add_ball(&self, _thing: ThingId, _world: &mut World, _ball: &NewBall) -> Option<u32> {
        None
    }
    /// A rule asks the thing to fire now (an emitter's round, in this step's Spawn stage); false
    /// when it can't.
    fn fire_now(&self, _thing: ThingId, _world: &mut World) -> bool {
        false
    }
}

/// A ball a rule adds to a thing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NewBall {
    pub position: Vec2,
    pub velocity: Vec2,
    /// Its size; `None`: the thing's own, as a new ball of it would be.
    pub radius: Option<f32>,
    /// The body row it is split from: it takes that ball's number and facts.
    pub like: Option<u32>,
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
    blocks: BTreeMap<&'static str, Box<dyn crate::rules::BlockSim>>,
    healths: BTreeMap<&'static str, Box<dyn crate::rules::HealthSim>>,
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

    pub fn add_block(&mut self, s: impl crate::rules::BlockSim) {
        let id = s.kind();
        assert!(!self.blocks.contains_key(id), "sim registered twice: {id}");
        self.blocks.insert(id, Box::new(s));
    }

    pub fn block(&self, kind: &str) -> Option<&dyn crate::rules::BlockSim> {
        self.blocks.get(kind).map(|s| s.as_ref())
    }

    /// A Health card kind's sim (rules part 4).
    pub fn add_health(&mut self, s: impl crate::rules::HealthSim) {
        let id = s.kind();
        assert!(!self.healths.contains_key(id), "sim registered twice: {id}");
        self.healths.insert(id, Box::new(s));
    }

    pub fn health(&self, kind: &str) -> Option<&dyn crate::rules::HealthSim> {
        self.healths.get(kind).map(|s| s.as_ref())
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

    /// A `Box` or `Arc` of a state is itself a `ThingState` (the blanket impl), so with the
    /// trait in scope, as here, a call on the pointer reaches the pointer's impl first. It must
    /// still reach the state inside.
    #[test]
    fn a_pointer_to_a_state_is_read_as_the_state() {
        let mut states: BTreeMap<u32, Box<dyn ThingState>> = BTreeMap::new();
        states.insert(1, Box::new(7u32));
        assert_eq!(states.get(&1).and_then(|s| s.as_any().downcast_ref::<u32>()), Some(&7));
        assert_eq!(states.get_mut(&1).and_then(|s| s.as_any_mut().downcast_mut::<u32>()), Some(&mut 7));
        let boxed = states.get(&1).unwrap();
        assert_eq!(*state::<u32>(boxed), 7, "&Box coerced to &dyn");
        // `as_any` forwards through any boxes, so the clone's own type is checked without it.
        assert_eq!(Any::type_id(&*boxed.clone_box()), TypeId::of::<u32>(), "a clone is the state, not a box");
        *state_mut::<u32>(states.get_mut(&1).unwrap()) = 8;
        assert_eq!(*state::<u32>(states[&1].as_ref()), 8);

        let mut shared: Arc<dyn ThingState> = Arc::new(9u32);
        assert_eq!(shared.as_any().downcast_ref::<u32>(), Some(&9));
        assert_eq!(*state::<u32>(&shared), 9, "&Arc coerced to &dyn");
        assert_eq!(Any::type_id(&*shared.clone_box()), TypeId::of::<u32>(), "a clone is the state, not an Arc");
        *state_mut::<u32>(&mut shared) = 10;
        assert_eq!(*state::<u32>(shared.as_ref()), 10);
    }

    #[test]
    #[should_panic(expected = "a shared plugin state can't be changed")]
    fn changing_a_shared_state_through_its_arc_panics() {
        let mut shared: Arc<dyn ThingState> = Arc::new(9u32);
        let _other = shared.clone();
        shared.as_any_mut();
    }

    #[test]
    #[should_panic(expected = "plugin state is not a f32")]
    fn asking_for_the_wrong_type_names_it() {
        let a: Box<dyn ThingState> = Box::new(7u32);
        state::<f32>(a.as_ref());
    }
}
