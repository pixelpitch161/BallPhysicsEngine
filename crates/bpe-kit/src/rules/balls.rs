//! Do blocks that make balls: Split, Spawn balls and Fire a round. The balls are added at the next
//! step's start through the thing's own sim (`ThingSim::add_ball`), at most
//! `bpe_runtime::rules::MAX_BALLS` live balls a thing.

use std::f32::consts::TAU;

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::rules::MAX_BALLS;
use bpe_runtime::{Action, BlockSim, Firing, PrepareCx, StageCx, ThingState, state};
use glam::Vec2;

use super::amount::Amount;
use super::block_kind;
use super::who::{Target, place_of, targets};

/// How a split shares the ball's size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SplitSizes {
    /// The balls together are as big (in area) as the one they came from.
    #[default]
    SharedByArea,
    /// Each is as big as the one they came from.
    KeepSize,
}

/// Splits a ball into several.
pub struct Split;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SplitSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    /// How many balls it becomes.
    #[setting(label = "Into", range(2, 1024), span(2, 8), simple, group = "Rule")]
    pub into: u32,
    /// How wide the fan they go off in is.
    #[setting(label = "Spread", range(0, 360), span(0, 180), unit = "°", simple, group = "Rule")]
    pub spread: f32,
    /// Their speed as a share of the ball's.
    #[setting(label = "Speed", range(0, 1e6), span(0, 3), unit = "×", group = "Rule")]
    pub speed: f32,
    #[setting(label = "Sizes", group = "Rule")]
    pub sizes: SplitSizes,
}

impl Default for SplitSettings {
    fn default() -> Self {
        SplitSettings { who: Who::That, into: 2, spread: 60.0, speed: 1.0, sizes: SplitSizes::SharedByArea }
    }
}

block_kind!(
    Split,
    "do.split",
    "Split",
    Do,
    SplitSettings,
    "Splits a ball into several that fan out, keeping its look and its cards."
);

#[derive(Clone, Debug)]
struct SplitState {
    who: Who,
    into: Amount,
    spread: Amount,
    speed: Amount,
    share: bool,
}

impl BlockSim for Split {
    fn kind(&self) -> &'static str {
        "do.split"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = SplitSettings::from_params(params);
        Ok(Box::new(SplitState {
            who: s.who,
            into: Amount::read(params, "into", s.into as f64),
            spread: Amount::read(params, "spread", s.spread as f64),
            speed: Amount::read(params, "speed", s.speed as f64),
            share: s.sizes == SplitSizes::SharedByArea,
        }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<SplitState>(st);
        let into = s.into.or_fixed(firing).round().clamp(2.0, 1024.0) as u32;
        let spread = (s.spread.or_fixed(firing) as f32).clamp(0.0, 360.0).to_radians();
        let speed = (s.speed.or_fixed(firing) as f32).max(0.0);
        for (thing, row) in targets(cx, &s.who, firing) {
            let Some(bodies) = cx.world.bodies.get(&thing) else { continue };
            // A whole thing splits each of its balls.
            let rows: Vec<u32> = match row {
                Some(r) => vec![r],
                None => (0..bodies.len() as u32).filter(|&r| bodies.alive[r as usize]).collect(),
            };
            for row in rows {
                out.push(Action::Split { thing, row, into, spread, speed, share: s.share });
            }
        }
    }
}

/// Where Spawn balls puts its balls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SpawnAt {
    /// Where the firing happened (where the hit was).
    #[default]
    TheFiringsPoint,
    /// Where a thing is.
    AThingsPlace,
}

/// Which way the spawned balls go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SpawnWay {
    #[default]
    Up,
    /// An angle: 0° up, turning clockwise.
    Angle,
    /// Straight off what the firing's ball hit.
    AwayFromTheHit,
    /// The way the firing's ball is going.
    LikeThatBall,
    /// Spread evenly all the way round, starting at the Angle.
    AllRound,
    /// Each its own random way (the same on every play).
    Random,
}

/// How big the spawned balls are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SpawnSize {
    /// The thing's own Size.
    #[default]
    ItsOwnSize,
    SetSize,
}

/// Adds balls to a Balls thing or an emitter.
pub struct Spawn;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SpawnSettings {
    /// The Balls thing (or emitter) the balls join: they wear its look and use its cards.
    #[setting(label = "Into", simple, group = "Rule")]
    pub into: Who,
    #[setting(label = "How many", range(0, 4096), span(1, 20), simple, group = "Rule")]
    pub count: u32,
    #[setting(label = "At", simple, group = "Rule")]
    pub at: SpawnAt,
    #[setting(label = "Thing", when = "at=a_things_place", group = "Rule")]
    pub place: Who,
    #[setting(label = "Going", simple, group = "Rule")]
    pub way: SpawnWay,
    #[setting(label = "Angle", when = "way=angle|all_round", span(-180, 180), unit = "°", group = "Rule")]
    pub angle: f32,
    /// How wide a fan they go off in.
    #[setting(
        label = "Spread",
        when = "way!=all_round|random",
        range(0, 360),
        span(0, 180),
        unit = "°",
        group = "Rule"
    )]
    pub spread: f32,
    #[setting(label = "Speed", range(0, 1e9), span(0, 2000), unit = "px/s", simple, group = "Rule")]
    pub speed: f32,
    #[setting(label = "Size", group = "Rule")]
    pub size: SpawnSize,
    #[setting(label = "Size", when = "size=set_size", range(0.5, 1e6), span(2, 200), unit = "px", group = "Rule")]
    pub radius: f32,
}

impl Default for SpawnSettings {
    fn default() -> Self {
        SpawnSettings {
            into: Who::Group,
            count: 1,
            at: SpawnAt::TheFiringsPoint,
            place: Who::Anything,
            way: SpawnWay::Up,
            angle: 0.0,
            spread: 0.0,
            speed: 300.0,
            size: SpawnSize::ItsOwnSize,
            radius: 20.0,
        }
    }
}

block_kind!(
    Spawn,
    "do.spawn",
    "Spawn balls",
    Do,
    SpawnSettings,
    "Adds balls to a Balls thing or an emitter, where the firing happened or at a thing."
);

#[derive(Clone, Debug)]
struct SpawnState {
    s: SpawnSettings,
    count: Amount,
    angle: Amount,
    speed: Amount,
    radius: Amount,
}

/// An angle (0° up, clockwise, in degrees) as a way.
fn way_of(degrees: f32) -> Vec2 {
    let a = degrees.to_radians();
    Vec2::new(a.sin(), -a.cos())
}

impl BlockSim for Spawn {
    fn kind(&self) -> &'static str {
        "do.spawn"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = SpawnSettings::from_params(params);
        Ok(Box::new(SpawnState {
            count: Amount::read(params, "count", s.count as f64),
            angle: Amount::read(params, "angle", s.angle as f64),
            speed: Amount::read(params, "speed", s.speed as f64),
            radius: Amount::read(params, "radius", s.radius as f64),
            s,
        }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let sp = state::<SpawnState>(st);
        let s = &sp.s;
        let n = sp.count.or_fixed(firing).round().clamp(0.0, MAX_BALLS as f64) as usize;
        if n == 0 {
            return;
        }
        let point = match s.at {
            SpawnAt::TheFiringsPoint => firing.point,
            SpawnAt::AThingsPlace => match targets(cx, &s.place, firing).first() {
                Some(&t) => place_of(cx.world, t),
                None => return,
            },
        };
        let angle = sp.angle.or_fixed(firing) as f32;
        let speed = (sp.speed.or_fixed(firing) as f32).max(0.0);
        let size = match s.size {
            SpawnSize::ItsOwnSize => None,
            SpawnSize::SetSize => Some((sp.radius.or_fixed(firing) as f32).max(0.5)),
        };
        let base = match s.way {
            SpawnWay::Up | SpawnWay::Random => Vec2::NEG_Y,
            SpawnWay::Angle | SpawnWay::AllRound => way_of(angle),
            SpawnWay::AwayFromTheHit => firing.normal.try_normalize().unwrap_or(Vec2::NEG_Y),
            SpawnWay::LikeThatBall => firing
                .subject
                .and_then(|p| cx.world.bodies.get(&p.thing)?.velocity.get(p.item as usize)?.try_normalize())
                .unwrap_or(Vec2::NEG_Y),
        };
        let spread = s.spread.clamp(0.0, 360.0).to_radians();
        let ways: Vec<Vec2> = (0..n)
            .map(|k| match s.way {
                SpawnWay::AllRound => Vec2::from_angle(TAU * k as f32 / n as f32).rotate(base),
                SpawnWay::Random => Vec2::from_angle(TAU * cx.dice(firing.subject, k as u32)).rotate(base),
                _ if n > 1 => Vec2::from_angle(spread * (k as f32 / (n - 1) as f32 - 0.5)).rotate(base),
                _ => base,
            })
            .collect();
        let mut seen: Vec<ThingId> = Vec::new();
        for (thing, _) in targets(cx, &s.into, firing) {
            if seen.contains(&thing) {
                continue;
            }
            seen.push(thing);
            // Several fan out from the point, each a ball's size along its way.
            let reach = match (n, size) {
                (1, _) => 0.0,
                (_, Some(r)) => r,
                (_, None) => own_radius(cx, (thing, None)),
            };
            let balls = ways.iter().map(|&w| (point + w * reach, w * speed, size)).collect();
            out.push(Action::Spawn { thing, balls });
        }
    }
}

/// About how big a thing's balls are: its first live ball's size, else 20 px.
fn own_radius(cx: &StageCx, (thing, _): Target) -> f32 {
    cx.world.bodies.get(&thing).and_then(|b| b.alive.iter().position(|&a| a).map(|r| b.radius[r])).unwrap_or(20.0)
}

/// Makes an emitter fire a round now.
pub struct FireRound;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FireRoundSettings {
    /// The emitters that fire (anything else is left be).
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
}

impl Default for FireRoundSettings {
    fn default() -> Self {
        FireRoundSettings { who: Who::Anything }
    }
}

block_kind!(
    FireRound,
    "do.fire",
    "Fire a round",
    Do,
    FireRoundSettings,
    "Makes an emitter fire a round now (set its Fire when to By a rule to fire only then)."
);

impl BlockSim for FireRound {
    fn kind(&self) -> &'static str {
        "do.fire"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(FireRoundSettings::from_params(params)))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<FireRoundSettings>(st);
        let mut seen: Vec<ThingId> = Vec::new();
        for (thing, _) in targets(cx, &s.who, firing) {
            if cx.world.kind(thing) == Some(crate::things::emitter::KIND) && !seen.contains(&thing) {
                seen.push(thing);
                out.push(Action::Fire { thing });
            }
        }
    }
}
