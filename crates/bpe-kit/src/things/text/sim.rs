//! A Text in the world: its pose (where it sits, turned by its Spin) and, for a Text set in the
//! world that rides nothing, its letters as a wall (Text part 3): balls bounce off them or stay
//! inside them, or pass through while rules still see them, and each letter is a part a rule,
//! a health or a sound tells apart. A Text whose words hold standings keeps the balls' past (a
//! little), so a line moving to its new place starts from where the ranking stood.

use std::sync::Arc;

use bpe_core::{Params, ResolvedThing, Scene, Settings, Thing, ThingId};
use bpe_runtime::{Outline, Pose, ThingSim, Wall, World, state, state_mut};
use glam::Vec2;

use super::walls::{letter_walls, middles};
use super::{KIND, TextBalls, TextPath, TextSettings};
use crate::text::{Placed, TextLayout};

pub struct TextSim;

/// The letters a Text's wall is made of now.
#[derive(Clone, Debug)]
pub struct WallLetters {
    pub layout: Arc<TextLayout>,
    pub placed: Vec<Placed>,
    /// Which letters were there (not broken) when it was made.
    pub alive: Vec<bool>,
    pub walls: Arc<bpe_runtime::LetterWalls>,
    /// Each letter's middle in the Text's own frame.
    pub middles: Vec<Vec2>,
}

/// How far a Text has turned by its Spin, and how fast it turns, and its wall's letters.
#[derive(Clone, Debug, Default)]
pub struct TextState {
    /// Radians, clockwise.
    pub angle: f32,
    /// Radians a second.
    pub spin: f32,
    /// Whether its words hold standings (which read the balls' past).
    pub standings: bool,
    /// Its settings as they are now (keys applied).
    pub params: Params,
    /// Its Look card as it is now, whose letter moves (Draw on's pen among them) what rides it sees.
    pub look: Option<Arc<bpe_core::ResolvedSlot>>,
    /// Whether it is a wall at all: set in the world, riding nothing.
    pub walled: bool,
    /// Its settings changed since its letters were last laid out.
    pub dirty: bool,
    /// What its wall is made of, while it has one.
    pub letters: Option<WallLetters>,
    /// Each letter's character (as `layout.letters`), for Spell notes.
    pub chars: Arc<[char]>,
}

/// How much of the balls' past a Text with standings keeps: twice the longest Re-sort time and a
/// little more (a line glides from the ranking one grid step back).
fn standings_history() -> bpe_runtime::HistoryWant {
    bpe_runtime::HistoryWant { seconds: 2.5, forever: false }
}

/// Whether a Text with these settings shows standings.
fn has_standings(resolved: &ResolvedThing) -> bool {
    !crate::text::live::standings_in(&TextSettings::from_params(&resolved.params).words).is_empty()
}

/// Whether `thing` (built into `world`) can be a wall: set in the world, and not riding a group of
/// balls, an emitter or a shape (riding Texts are labels).
fn can_be_a_wall(thing: &Thing, world: &World) -> bool {
    let on_screen = thing.screen.unwrap_or(true);
    let rides = thing
        .parent
        .filter(|p| *p != ThingId::ROOT)
        .is_some_and(|p| world.kind_of(p).is_some_and(|k| crate::riding::RIDDEN_BY_TEXT.contains(&k)));
    !on_screen && !rides
}

/// [`can_be_a_wall`] read from the saved scene: set in the world, riding nothing.
pub fn walled_in(scene: &Scene, thing: &Thing) -> bool {
    let rides = thing
        .parent
        .filter(|p| *p != ThingId::ROOT)
        .and_then(|p| scene.things.get(&p))
        .is_some_and(|p| crate::riding::RIDDEN_BY_TEXT.contains(&p.kind.as_str()));
    !thing.screen.unwrap_or(true) && !rides
}

/// Whether the letters of a Text with settings `s` can move by themselves from step to step (their
/// values change, a rule sets its words, the wave moves, the shape it runs along moves).
fn moves_by_itself(s: &TextSettings, world: &World, id: ThingId) -> bool {
    s.words.contains('{')
        || world.rules().is_some_and(|r| r.text(id).is_some())
        || (s.path == TextPath::AlongAWave && s.wave_speed != 0.0)
        || s.path == TextPath::AlongAShape
}

/// The character of letter `part` of Text `thing` as its wall has it (`None` when it isn't a Text
/// with a wall, or has no such letter). Never allocates.
pub fn letter_char(world: &World, thing: ThingId, part: u32) -> Option<char> {
    let st = world.states.get(&thing)?.as_any().downcast_ref::<TextState>()?;
    st.chars.get(part as usize).copied()
}

/// The Text's letters laid out and placed at this moment, as the drawing has them (its first copy).
fn laid_out(world: &World, id: ThingId, params: &Params) -> Option<(Arc<TextLayout>, Vec<Placed>)> {
    let tree = world.tree.as_ref()?;
    let at = super::TextAt::new(tree, params, world, id, world.time());
    let now = super::text_now(&at)?;
    Some((now.layout, now.placed))
}

/// Which of Text `id`'s letters are there (not broken).
fn alive_of(world: &World, id: ThingId) -> &[bool] {
    world.parts.get(&id).map_or(&[], |p| p.alive.as_slice())
}

/// Makes or moves Text `id`'s wall in `world` (and its parts) as it is now: the letters are laid out
/// again only when they may have changed (`relay`, or a letter broke); otherwise the wall moves
/// with the Text's pose.
fn refresh(id: ThingId, world: &mut World, relay: bool) {
    let Some(st) = world.states.get(&id) else { return };
    let st = state::<TextState>(st.as_ref());
    if !st.walled {
        return;
    }
    let s = TextSettings::from_params(&st.params);
    let pose = world.poses.get(&id).copied().unwrap_or_else(|| world.anchor(id));
    let relay = relay || st.dirty || st.letters.is_none() || moves_by_itself(&s, world, id);
    let same_alive = |kept: &WallLetters, alive_now: &[bool]| {
        (0..kept.layout.letters.len()).all(|k| alive_now.get(k).copied().unwrap_or(true) == kept.alive[k])
    };
    // Nothing about the letters changed: the wall and the parts only move with the pose (every
    // step, so without copying the letters).
    if !relay && st.letters.as_ref().is_some_and(|l| same_alive(l, alive_of(world, id))) {
        let turn = Vec2::from_angle(pose.angle);
        let middles = &st.letters.as_ref().expect("checked").middles;
        if let Some(w) = world.walls.get_mut(&id) {
            if let Outline::Letters { center, angle, .. } = &mut w.outline {
                *center = pose.position;
                *angle = pose.angle;
            }
            w.spin = pose.spin;
            w.velocity = pose.velocity;
        }
        if let Some(parts) = world.parts.get_mut(&id)
            && parts.at.len() == middles.len()
        {
            for (at, m) in parts.at.iter_mut().zip(middles) {
                *at = pose.position + turn.rotate(*m);
            }
        }
        return;
    }
    let alive_now: Vec<bool> = alive_of(world, id).to_vec();
    let mut letters = st.letters.clone();
    let mut changed = false;
    if relay {
        if let Some((layout, placed)) = laid_out(world, id, &st.params) {
            let same = letters.as_ref().is_some_and(|l| Arc::ptr_eq(&l.layout, &layout) && l.placed == placed);
            if !same {
                letters = Some(WallLetters {
                    middles: middles(&layout, &placed),
                    layout,
                    placed,
                    alive: Vec::new(),
                    walls: Arc::new(bpe_runtime::LetterWalls::new(Vec::new())),
                });
                changed = true;
            }
        } else {
            letters = None;
            changed = true;
        }
    }
    // A letter broke: it leaves the wall.
    if let Some(l) = letters.as_mut()
        && (changed || !same_alive(l, &alive_now))
    {
        let n = l.layout.letters.len();
        l.alive = (0..n).map(|k| alive_now.get(k).copied().unwrap_or(true)).collect();
        let alive = l.alive.clone();
        l.walls = Arc::new(letter_walls(&l.layout, &l.placed, &|k| alive[k]));
        changed = true;
    }
    let turn = Vec2::from_angle(pose.angle);
    let at: Vec<Vec2> =
        letters.as_ref().map_or(Vec::new(), |l| l.middles.iter().map(|m| pose.position + turn.rotate(*m)).collect());
    let wall = letters.as_ref().filter(|l| !l.walls.list.is_empty()).map(|l| Wall {
        outline: Outline::Letters {
            center: pose.position,
            angle: pose.angle,
            letters: l.walls.clone(),
            thickness: if s.balls == TextBalls::BounceInside { s.thickness.max(0.0) } else { 0.0 },
        },
        solid: s.balls == TextBalls::BounceOff,
        spin: pose.spin,
        growth: 0.0,
        velocity: pose.velocity,
        passes: s.balls == TextBalls::PassThrough,
        gaps: None,
    });
    let chars: Option<Arc<[char]>> = changed
        .then(|| letters.as_ref().map_or_else(|| Arc::from([]), |l| l.layout.letters.iter().map(|l| l.ch).collect()));
    {
        let st = state_mut::<TextState>(world.states.get_mut(&id).expect("its state").as_mut());
        st.dirty = false;
        if changed {
            st.letters = letters;
        }
        if let Some(c) = chars {
            st.chars = c;
        }
    }
    match wall {
        Some(w) => {
            world.walls.insert(id, w);
        }
        None => {
            world.walls.remove(&id);
        }
    }
    if at.is_empty() {
        world.parts.remove(&id);
    } else {
        world.parts.entry(id).or_default().set(at);
    }
}

impl ThingSim for TextSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let spin = TextSettings::from_params(&resolved.params).spin.to_radians();
        let anchor = world.anchor(thing.id);
        world.poses.insert(thing.id, Pose { spin: anchor.spin + spin, ..anchor });
        let standings = has_standings(resolved);
        let walled = can_be_a_wall(thing, world);
        let st = TextState {
            angle: 0.0,
            spin,
            standings,
            params: resolved.params.clone(),
            look: resolved.slots.get(&bpe_core::SlotId::Look).cloned().map(Arc::new),
            walled,
            dirty: true,
            letters: None,
            chars: Arc::from([]),
        };
        world.states.insert(thing.id, Box::new(st));
        refresh(thing.id, world, true);
    }

    /// With standings, every thing with balls keeps a little of its past. A Text running along a
    /// shape built after it lays its letters out again, now the shape is there.
    fn after_build(&self, thing: ThingId, world: &mut World) {
        refresh(thing, world, true);
        let standings = world.states.get(&thing).is_some_and(|s| state::<TextState>(s.as_ref()).standings);
        if !standings {
            return;
        }
        let with_balls: Vec<(ThingId, usize)> = world.bodies.iter().map(|(id, b)| (*id, b.len())).collect();
        for (id, rows) in with_balls {
            world.keep_history(id, standings_history());
            if let Some(h) = world.history.get_mut(&id) {
                h.note(world.step, rows);
            }
        }
    }

    /// Standings ask for the balls' past; asking makes a change to them rebuild the world.
    fn history(&self, resolved: &ResolvedThing) -> Option<bpe_runtime::HistoryWant> {
        has_standings(resolved).then(standings_history)
    }

    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let spin = TextSettings::from_params(&resolved.params).spin.to_radians();
        if let Some(st) = world.states.get_mut(&thing) {
            let st = state_mut::<TextState>(st.as_mut());
            st.spin = spin;
            if st.params != resolved.params {
                st.params = resolved.params.clone();
                st.dirty = true;
            }
            let look = resolved.slots.get(&bpe_core::SlotId::Look);
            if st.look.as_deref() != look {
                st.look = look.cloned().map(Arc::new);
            }
        }
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        // Slow motion slows the spin with the balls.
        let dt = world.sim_dt();
        let anchor = world.anchor(thing);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<TextState>(st.as_mut());
        st.angle += st.spin * dt;
        let pose = Pose { angle: anchor.angle + st.angle, spin: anchor.spin + st.spin, ..anchor };
        world.poses.insert(thing, pose);
        refresh(thing, world, false);
    }

    /// What lays out and places the letters, and what balls do at them; the Show, values' Re-sort
    /// time, riding and Fast letters are drawn only.
    fn key_restarts_physics(&self, key: &str) -> bool {
        TextSettings::defs().iter().any(|d| d.key == key && matches!(d.group, "Words" | "Path" | "Balls"))
            && key != "fast"
    }

    /// Only a Text with a wall has its words, path and balls settings in the physics (a title on
    /// the screen is typed into without restarting the run); its Spin always.
    fn key_restarts_physics_here(&self, scene: &Scene, thing: &Thing, key: &str) -> bool {
        key == "spin" || (self.key_restarts_physics(key) && walled_in(scene, thing))
    }

    /// For a Text with a wall: that it has one, and a font the scene keeps (loading or removing it
    /// lays the letters out again). 0 for every other Text, as before letters were walls.
    fn physics_print_extra(&self, scene: &Scene, thing: &Thing, resolved: &ResolvedThing) -> u64 {
        if !walled_in(scene, thing) {
            return 0;
        }
        let settings = TextSettings::from_params(&resolved.params);
        let font = settings.font;
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |bytes: &[u8]| {
            for b in bytes {
                h = (h ^ *b as u64).wrapping_mul(0x100_0000_01b3);
            }
        };
        eat(&[1 + thing.screen.unwrap_or(true) as u8]);
        if let Some(f) = scene.fonts.get(&font) {
            eat(&(f.bytes.len() as u64).to_le_bytes());
            eat(&f.bytes[..f.bytes.len().min(64)]);
            eat(&f.bytes[f.bytes.len().saturating_sub(64)..]);
        }
        // Values may name things (`of <name>`): a rename changes the words the wall is made of.
        if settings.words.contains('{') {
            for t in scene.things.values() {
                eat(&t.id.0.to_le_bytes());
                eat(t.name.as_bytes());
                eat(&[0]);
            }
        }
        h
    }

    fn reads_tree(&self, _resolved: &ResolvedThing) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "sim_tests.rs"]
mod tests;
