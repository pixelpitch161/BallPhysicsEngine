//! Counting: When Count reached, If Counter is, Do Count. Counters are named numbers the whole
//! scene shares (they start at 0); a Count block's change applies at the next step's start.

use std::collections::BTreeMap;

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::rules::WHOLE;
use bpe_runtime::{
    Action, BlockSim, CountHow, EventKind, Firing, Party, PrepareCx, StageCx, ThingState, Values, World, state,
    state_mut,
};

use super::amount::Amount;
use super::block_kind;
use super::checks::Test;
use super::who::matches;

/// What Count reached counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CountOf {
    /// How many hits a thing has taken part in (a shape hit, a ball hitting).
    #[default]
    HitsSoFar,
    /// How many of a thing's balls are there.
    BallCount,
    /// A named counter (Do: Count).
    ACounter,
}

/// Which way the count has to go past the number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Reach {
    /// From under the number to it or over.
    #[default]
    RisesTo,
    /// From over the number to it or under.
    FallsTo,
}

impl Reach {
    /// Whether going from `last` to `now` reaches `n` this way.
    pub fn crossed(self, last: f64, now: f64, n: f64) -> bool {
        match self {
            Reach::RisesTo => last < n && now >= n,
            Reach::FallsTo => last > n && now <= n,
        }
    }
}

/// When a count reaches a number.
pub struct CountReached;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CountReachedSettings {
    #[setting(label = "Count", simple, group = "Rule")]
    pub of: CountOf,
    /// Whose hits or balls (each thing counts on its own).
    #[setting(label = "Of", when = "of!=a_counter", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Counter", when = "of=a_counter", simple, group = "Rule")]
    pub counter: String,
    #[setting(label = "When it", simple, group = "Rule")]
    pub reach: Reach,
    #[setting(label = "Number", span(0, 100), simple, group = "Rule")]
    pub amount: f64,
}

impl Default for CountReachedSettings {
    fn default() -> Self {
        CountReachedSettings {
            of: CountOf::HitsSoFar,
            who: Who::Anything,
            counter: "score".into(),
            reach: Reach::RisesTo,
            amount: 10.0,
        }
    }
}

block_kind!(
    CountReached,
    "when.count",
    "Count reached",
    When,
    CountReachedSettings,
    "Fires when a thing's hits, its number of balls, or a counter reaches a number."
);

#[derive(Clone, Debug)]
struct CountState {
    s: CountReachedSettings,
    /// Each thing's hits so far.
    hits: BTreeMap<ThingId, u64>,
    /// Each thing's count (or the counter's, under the scene) at the last stage.
    last: BTreeMap<ThingId, f64>,
}

/// Every thing's live balls in `world`, where it has balls.
fn ball_counts(world: &World) -> BTreeMap<ThingId, f64> {
    world.bodies.iter().map(|(&t, b)| (t, b.alive_count() as f64)).collect()
}

/// `who` in a When block: That, Other and Group name nothing there, so they are Anything.
fn as_when(who: &Who) -> Who {
    match who {
        Who::That | Who::Other | Who::Group => Who::Anything,
        w => w.clone(),
    }
}

impl BlockSim for CountReached {
    fn kind(&self) -> &'static str {
        "when.count"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let mut s = CountReachedSettings::from_params(params);
        s.who = as_when(&s.who);
        s.counter = s.counter.trim().to_owned();
        if s.of == CountOf::ACounter && s.counter.is_empty() {
            return Err("Count reached needs a counter name".into());
        }
        let last = match s.of {
            CountOf::BallCount => ball_counts(cx.world),
            CountOf::HitsSoFar | CountOf::ACounter => BTreeMap::new(),
        };
        Ok(Box::new(CountState { s, hits: BTreeMap::new(), last }))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let c = state_mut::<CountState>(st);
        let n = c.s.amount;
        let fire = |thing: Option<ThingId>, count: f64, out: &mut Vec<Firing>| {
            let mut values = Values::default();
            values.set("count", count);
            let subject = thing.map(|thing| Party { thing, item: WHOLE });
            let point = thing.map_or(glam::Vec2::ZERO, |t| super::who::place_of(cx.world, (t, None)));
            out.push(Firing { step: cx.step, subject, point, values, ..Firing::default() });
        };
        let now: BTreeMap<ThingId, f64> = match c.s.of {
            CountOf::ACounter => {
                let now = cx.counter(&c.s.counter);
                let last = c.last.get(&ThingId::ROOT).copied().unwrap_or(0.0);
                if c.s.reach.crossed(last, now, n) {
                    fire(None, now, out);
                }
                c.last.insert(ThingId::ROOT, now);
                return;
            }
            CountOf::HitsSoFar => {
                let mut changed = BTreeMap::new();
                for e in cx.events.iter().filter(|e| e.kind == EventKind::Hit) {
                    let a = e.a.thing;
                    let b = e.b.map(|b| b.thing).filter(|&b| b != a);
                    for t in std::iter::once(a).chain(b) {
                        let h = c.hits.entry(t).or_default();
                        *h += 1;
                        changed.insert(t, *h as f64);
                    }
                }
                changed
            }
            CountOf::BallCount => ball_counts(cx.world),
        };
        for (thing, count) in now {
            let last = c.last.insert(thing, count).unwrap_or(0.0);
            if cx.is_broken(thing) || !matches(cx, &c.s.who, Party { thing, item: WHOLE }) {
                continue;
            }
            if c.s.reach.crossed(last, count, n) {
                fire(Some(thing), count, out);
            }
        }
    }
}

/// Checks a counter.
pub struct CounterIs;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CounterIsSettings {
    #[setting(label = "Counter", simple, group = "Rule")]
    pub counter: String,
    #[setting(label = "Is", simple, group = "Rule")]
    pub test: Test,
    #[setting(label = "Than", span(0, 100), simple, group = "Rule")]
    pub amount: f64,
    #[setting(label = "And", when = "test=between", span(0, 100), group = "Rule")]
    pub to: f64,
}

impl Default for CounterIsSettings {
    fn default() -> Self {
        CounterIsSettings { counter: "score".into(), test: Test::Over, amount: 10.0, to: 20.0 }
    }
}

block_kind!(CounterIs, "if.counter", "Counter is", If, CounterIsSettings, "Goes on only when a counter passes a test.");

#[derive(Clone, Debug)]
struct CounterIsState {
    counter: String,
    test: Test,
    amount: Amount,
    to: Amount,
}

impl BlockSim for CounterIs {
    fn kind(&self) -> &'static str {
        "if.counter"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = CounterIsSettings::from_params(params);
        let counter = s.counter.trim().to_owned();
        if counter.is_empty() {
            return Err("Counter is needs a counter name".into());
        }
        Ok(Box::new(CounterIsState {
            counter,
            test: s.test,
            amount: Amount::read(params, "amount", s.amount),
            to: Amount::read(params, "to", s.to),
        }))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx) -> bool {
        let s = state::<CounterIsState>(st);
        let v = cx.counter(&s.counter);
        let Some(a) = s.amount.of(firing) else { return false };
        match s.test {
            Test::Over => v > a,
            Test::Under => v < a,
            Test::Equal => (v - a).abs() <= 1e-6,
            Test::Between => {
                let Some(b) = s.to.of(firing) else { return false };
                v >= a.min(b) && v <= a.max(b)
            }
        }
    }
}

/// How Count changes its counter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CountMode {
    /// Adds the number (take one away with a negative number).
    #[default]
    Add,
    /// Sets it to the number.
    Set,
    /// Back to 0.
    Reset,
}

/// Changes a counter.
pub struct Count;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CountSettings {
    #[setting(label = "Counter", simple, group = "Rule")]
    pub counter: String,
    #[setting(label = "Do", simple, group = "Rule")]
    pub how: CountMode,
    #[setting(label = "Number", when = "how!=reset", span(-10, 10), simple, group = "Rule")]
    pub amount: f64,
}

impl Default for CountSettings {
    fn default() -> Self {
        CountSettings { counter: "score".into(), how: CountMode::Add, amount: 1.0 }
    }
}

block_kind!(Count, "do.count", "Count", Do, CountSettings, "Adds to, sets or resets a counter the whole scene shares.");

#[derive(Clone, Debug)]
struct CountDo {
    counter: String,
    how: CountMode,
    amount: Amount,
}

impl BlockSim for Count {
    fn kind(&self) -> &'static str {
        "do.count"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = CountSettings::from_params(params);
        let counter = s.counter.trim().to_owned();
        if counter.is_empty() {
            return Err("Count needs a counter name".into());
        }
        Ok(Box::new(CountDo { counter, how: s.how, amount: Amount::read(params, "amount", s.amount) }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, _cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<CountDo>(st);
        let how = match s.how {
            CountMode::Add => CountHow::Add(s.amount.or_fixed(firing)),
            CountMode::Set => CountHow::Set(s.amount.or_fixed(firing)),
            CountMode::Reset => CountHow::Set(0.0),
        };
        out.push(Action::Count { name: s.counter.clone(), how });
    }
}
