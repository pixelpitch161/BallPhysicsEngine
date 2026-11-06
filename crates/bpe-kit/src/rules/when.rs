//! When blocks: what starts a rule.

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::{BlockSim, EventKind, Firing, Party, PrepareCx, StageCx, ThingState, Values, state, state_mut};

use super::block_kind;
use super::who::{ball_number, is_ball, matches};
use crate::SceneSettings;

/// When a ball hits something.
pub struct Hit;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HitSettings {
    /// The ball (or the balls of the thing) that hits.
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    /// What it hits: a shape or another ball.
    #[setting(label = "Hits", simple, group = "Rule")]
    pub whom: Who,
    /// Softer hits don't count.
    #[setting(label = "At least", range(0, 1e9), span(0, 2000), unit = "px/s", group = "Rule")]
    pub min_strength: f32,
}

impl Default for HitSettings {
    fn default() -> Self {
        HitSettings { who: Who::Anything, whom: Who::Anything, min_strength: 0.0 }
    }
}

block_kind!(
    Hit,
    "when.hit",
    "Hit",
    When,
    HitSettings,
    "Fires when a ball hits a shape or another ball.",
    values = ["strength", "speed", "size", "hits", "ball"],
    from = []
);

/// A ball's values for a firing about it.
pub fn ball_values(world: &bpe_runtime::World, ball: Party, values: &mut Values) {
    if let Some(b) = world.bodies.get(&ball.thing)
        && let Some(i) = Some(ball.item as usize).filter(|&i| i < b.len())
    {
        values.set("speed", b.velocity[i].length() as f64);
        values.set("size", b.radius[i] as f64);
        values.set("hits", b.hits[i] as f64);
        if let Some(n) = ball_number(world, ball) {
            values.set("ball", n as f64);
        }
    }
}

impl BlockSim for Hit {
    fn kind(&self) -> &'static str {
        "when.hit"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(HitSettings::from_params(params)))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let s = state::<HitSettings>(st);
        for e in cx.events.iter().filter(|e| e.kind == EventKind::Hit && e.speed >= s.min_strength) {
            let Some(b) = e.b else { continue };
            let mut sides = vec![(e.a, b, e.normal)];
            // A ball-on-ball hit: either ball can be the one that hits.
            if is_ball(cx.world, b) {
                sides.push((b, e.a, -e.normal));
            }
            for (subject, other, normal) in sides {
                if !matches(cx, &s.who, subject) || !matches(cx, &s.whom, other) {
                    continue;
                }
                let mut values = Values::default();
                values.set("strength", e.speed as f64);
                ball_values(cx.world, subject, &mut values);
                out.push(Firing {
                    step: cx.step,
                    subject: Some(subject),
                    other: Some(other),
                    point: e.point,
                    normal,
                    values,
                });
            }
        }
    }
}

/// At a time, or again and again.
pub struct Time;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct TimeSettings {
    /// The first moment, in seconds from the start.
    #[setting(label = "At", range(0, 1e9), span(0, 60), unit = "s", simple, group = "Rule")]
    pub at: f64,
    /// Fires again every so often after that.
    #[setting(label = "Repeat", simple, group = "Rule")]
    pub repeat: bool,
    #[setting(label = "Every", when = "repeat", range(0.004166667, 1e9), span(0.05, 10), unit = "s", group = "Rule")]
    pub every: f64,
    /// Stops repeating after a moment.
    #[setting(label = "Until", when = "repeat", group = "Rule")]
    pub until: bool,
    #[setting(label = "Until", when = "repeat and until", range(0, 1e9), span(0, 60), unit = "s", group = "Rule")]
    pub until_at: f64,
}

impl Default for TimeSettings {
    fn default() -> Self {
        TimeSettings { at: 1.0, repeat: false, every: 1.0, until: false, until_at: 10.0 }
    }
}

block_kind!(
    Time,
    "when.time",
    "Time",
    When,
    TimeSettings,
    "Fires at a moment, and again every so often if you like.",
    values = ["time", "count"],
    from = []
);

/// The moments a Time or Beat block fires at: `first + k·every` (once when `every` is none),
/// up to `until`, and which of them it has fired so far.
#[derive(Clone, Debug, PartialEq)]
pub struct Moments {
    pub first: f64,
    pub every: Option<f64>,
    pub until: Option<f64>,
    /// The next moment's index.
    pub next: u64,
}

impl Moments {
    fn at(&self, k: u64) -> f64 {
        self.first + self.every.unwrap_or(0.0) * k as f64
    }

    /// The moments falling in step `step` (step 1 takes step 0's too), with their indexes.
    pub fn due(&mut self, step: u64, rate: u32) -> Vec<(u64, f64)> {
        let mut out = Vec::new();
        loop {
            if self.every.is_none() && self.next > 0 {
                break;
            }
            let m = self.at(self.next);
            if self.until.is_some_and(|u| m > u + 1e-9) || bpe_runtime::step_at(m, rate) > step {
                break;
            }
            out.push((self.next, m));
            self.next += 1;
            // Every is at least a step long, so a few a step at most; never hang on odd values.
            if out.len() >= 64 {
                break;
            }
        }
        out
    }
}

impl BlockSim for Time {
    fn kind(&self) -> &'static str {
        "when.time"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = TimeSettings::from_params(params);
        Ok(Box::new(Moments {
            first: s.at.max(0.0),
            every: s.repeat.then_some(s.every.max(1.0 / 240.0)),
            until: (s.repeat && s.until).then_some(s.until_at),
            next: 0,
        }))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        for (k, m) in state_mut::<Moments>(st).due(cx.step, cx.world.steps_per_second) {
            let mut values = Values::default();
            values.set("time", m);
            values.set("count", (k + 1) as f64);
            out.push(Firing { step: cx.step, values, ..Firing::default() });
        }
    }
}

/// How often a Beat block fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BeatEvery {
    /// Every beat of the scene's tempo.
    #[default]
    Beat,
    /// At the start of every bar.
    Bar,
    /// Every few bars.
    Bars,
}

/// On the beat.
pub struct Beat;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BeatSettings {
    #[setting(label = "Every", simple, group = "Rule")]
    pub every: BeatEvery,
    #[setting(label = "Bars", when = "every=bars", range(1, 1e6), span(1, 16), unit = "bars", group = "Rule")]
    pub bars: u32,
    /// Starts this many beats in.
    #[setting(label = "Offset", range(0, 1e6), span(0, 16), unit = "beats", group = "Rule")]
    pub offset: f64,
}

impl Default for BeatSettings {
    fn default() -> Self {
        BeatSettings { every: BeatEvery::Beat, bars: 2, offset: 0.0 }
    }
}

block_kind!(
    Beat,
    "when.beat",
    "Beat",
    When,
    BeatSettings,
    "Fires on the beats or bars of the scene's tempo.",
    values = ["time", "count", "bar", "beat"],
    from = []
);

#[derive(Clone, Debug, PartialEq)]
struct BeatState {
    moments: Moments,
    beat: f64,
    per_bar: u32,
}

impl BlockSim for Beat {
    fn kind(&self) -> &'static str {
        "when.beat"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = BeatSettings::from_params(params);
        let scene =
            cx.resolved.things.get(&ThingId::ROOT).map(|t| SceneSettings::from_params(&t.params)).unwrap_or_default();
        let (beat, per_bar) = crate::things::rig::pattern::beat_seconds(&scene);
        let every = match s.every {
            BeatEvery::Beat => beat,
            BeatEvery::Bar => beat * per_bar as f64,
            BeatEvery::Bars => beat * per_bar as f64 * s.bars.max(1) as f64,
        };
        Ok(Box::new(BeatState {
            moments: Moments { first: s.offset.max(0.0) * beat, every: Some(every), until: None, next: 0 },
            beat,
            per_bar,
        }))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let b = state_mut::<BeatState>(st);
        let (beat, per_bar) = (b.beat, b.per_bar as u64);
        for (k, m) in b.moments.due(cx.step, cx.world.steps_per_second) {
            let index = (m / beat + 1e-6).floor().max(0.0) as u64;
            let mut values = Values::default();
            values.set("time", m);
            values.set("count", (k + 1) as f64);
            values.set("bar", (index / per_bar + 1) as f64);
            values.set("beat", (index % per_bar + 1) as f64);
            out.push(Firing { step: cx.step, values, ..Firing::default() });
        }
    }
}
