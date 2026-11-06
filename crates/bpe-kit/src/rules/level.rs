//! When: Crosses a level. A ball's speed, size, light or shadow, or a shape's or a bus's sound,
//! going over or under a level.

use std::collections::BTreeMap;
use std::sync::Arc;

use bpe_core::{BlockSection, Choice, Params, Settings, SoundSource, ThingId, Who};
use bpe_runtime::{BlockSim, Firing, LightAt, Party, PrepareCx, SoundAt, StageCx, ThingState, Values, state_mut};

use super::block_kind;
use super::when::ball_values;
use super::who::matches;

/// What Crosses a level watches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LevelOf {
    /// A ball's speed, in pixels a second.
    #[default]
    Speed,
    /// A ball's size, in pixels.
    Size,
    /// How much light falls on a ball (0 to 1; needs a Lighting card).
    Light,
    /// How much of the light its shadows take from a ball (0 to 1).
    Shadow,
    /// How loud a shape or a bus is (0 to 1).
    Sound,
}

/// Whose sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SoundOf {
    /// The shape's own.
    #[default]
    TheShape,
    Bus1,
    Bus2,
    Bus3,
}

/// Which way the value has to cross the level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CrossWay {
    #[default]
    GoesOver,
    GoesUnder,
    Either,
}

impl CrossWay {
    /// Whether going from `last` to `now` crosses `level` this way: over is `last ≤ level < now`,
    /// under `last ≥ level > now`.
    pub fn crossed(self, last: f64, now: f64, level: f64) -> bool {
        let over = last <= level && now > level;
        let under = last >= level && now < level;
        match self {
            CrossWay::GoesOver => over,
            CrossWay::GoesUnder => under,
            CrossWay::Either => over || under,
        }
    }
}

/// When a value crosses a level.
pub struct CrossesLevel;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CrossesLevelSettings {
    #[setting(label = "Value", simple, group = "Rule")]
    pub value: LevelOf,
    /// The balls watched.
    #[setting(label = "Of", when = "value!=sound", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Sound of", when = "value=sound", simple, group = "Rule")]
    pub sound: SoundOf,
    #[setting(label = "Shape", when = "value=sound and sound=the_shape", simple, group = "Rule")]
    pub shape: Who,
    #[setting(label = "When it", simple, group = "Rule")]
    pub way: CrossWay,
    #[setting(label = "Level", span(0, 2000), simple, group = "Rule")]
    pub level: f64,
}

impl Default for CrossesLevelSettings {
    fn default() -> Self {
        CrossesLevelSettings {
            value: LevelOf::Speed,
            who: Who::Anything,
            sound: SoundOf::TheShape,
            shape: Who::Anything,
            way: CrossWay::GoesOver,
            level: 800.0,
        }
    }
}

block_kind!(
    CrossesLevel,
    "when.level",
    "Crosses a level",
    When,
    CrossesLevelSettings,
    "Fires when a ball's speed, size, light or shadow, or a sound, goes over or under a level."
);

#[derive(Clone)]
struct LevelState {
    s: CrossesLevelSettings,
    /// Each ball's value at the last stage (the sound's under the scene's id).
    last: BTreeMap<Party, f64>,
    sound: Option<(Arc<dyn SoundAt>, ThingId, SoundSource)>,
    light: Option<Arc<dyn LightAt>>,
}

impl CrossesLevelSettings {
    fn hears(&self) -> bool {
        self.value == LevelOf::Sound
    }
    fn sees(&self) -> bool {
        matches!(self.value, LevelOf::Light | LevelOf::Shadow)
    }
}

impl BlockSim for CrossesLevel {
    fn kind(&self) -> &'static str {
        "when.level"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = CrossesLevelSettings::from_params(params);
        let sound = match (s.hears(), &cx.sims.sound) {
            (true, Some(maker)) => {
                let (shape, source) = match s.sound {
                    SoundOf::TheShape => match s.shape.names_thing() {
                        Some(t) => (t, SoundSource::Level),
                        None => return Err("Crosses a level needs the shape whose sound it hears".into()),
                    },
                    SoundOf::Bus1 => (ThingId::ROOT, SoundSource::Bus(1)),
                    SoundOf::Bus2 => (ThingId::ROOT, SoundSource::Bus(2)),
                    SoundOf::Bus3 => (ThingId::ROOT, SoundSource::Bus(3)),
                };
                Some((maker.make(cx.scene, cx.resolved), shape, source))
            }
            _ => None,
        };
        let light = match (s.sees(), &cx.sims.light) {
            (true, Some(maker)) => Some(maker.make(cx.scene, cx.resolved)),
            _ => None,
        };
        Ok(Box::new(LevelState { s, last: BTreeMap::new(), sound, light }))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let l = state_mut::<LevelState>(st);
        let level = l.s.level;
        let way = l.s.way;
        if l.s.value == LevelOf::Sound {
            let Some((sound, shape, source)) = &l.sound else { return };
            let now = sound.level(cx.world, cx.log, *shape, source, cx.time) as f64;
            let key = Party { thing: ThingId::ROOT, item: 0 };
            if let Some(last) = l.last.insert(key, now)
                && now.is_finite()
                && way.crossed(last, now, level)
            {
                let mut values = Values::default();
                values.set("value", now);
                out.push(Firing { step: cx.step, values, ..Firing::default() });
            }
            return;
        }
        let world = cx.world;
        let mut now = BTreeMap::new();
        for (&thing, bodies) in &world.bodies {
            if cx.is_broken(thing) {
                continue;
            }
            for row in 0..bodies.len() {
                let ball = Party { thing, item: row as u32 };
                if !bodies.alive[row] || !matches(cx, &l.s.who, ball) {
                    continue;
                }
                let v = match l.s.value {
                    LevelOf::Speed => bodies.velocity[row].length() as f64,
                    LevelOf::Size => bodies.radius[row] as f64,
                    LevelOf::Light | LevelOf::Shadow => {
                        let Some(light) = &l.light else { continue };
                        let b = light.ball(world, cx.log, thing, row, cx.time);
                        (if l.s.value == LevelOf::Light { b.light } else { b.shadow }) as f64
                    }
                    LevelOf::Sound => unreachable!("handled above"),
                };
                if !v.is_finite() {
                    continue;
                }
                if let Some(&last) = l.last.get(&ball)
                    && way.crossed(last, v, level)
                {
                    let mut values = Values::default();
                    ball_values(world, ball, &mut values);
                    values.set("value", v);
                    out.push(Firing {
                        step: cx.step,
                        subject: Some(ball),
                        point: bodies.position[row],
                        values,
                        ..Firing::default()
                    });
                }
                now.insert(ball, v);
            }
        }
        // Balls gone are forgotten; a ball's first stage only notes its value.
        l.last = now;
    }
    fn hears_sound(&self, params: &Params) -> bool {
        CrossesLevelSettings::from_params(params).hears()
    }
    fn sees_light(&self, params: &Params) -> bool {
        CrossesLevelSettings::from_params(params).sees()
    }
}
