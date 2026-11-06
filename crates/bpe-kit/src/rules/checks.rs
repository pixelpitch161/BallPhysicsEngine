//! If blocks: checks a firing must pass to go on.

use bpe_core::{BlockSection, Choice, Params, Settings, Who};
use bpe_runtime::{BlockSim, Firing, PrepareCx, StageCx, ThingState, state, state_mut};

use super::amount::Amount;
use super::block_kind;
use super::memory::Remembered;
use super::who::{ball_number, matches};

/// How Compare tests the value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Test {
    #[default]
    Over,
    Under,
    Between,
    Equal,
}

/// Whose value Compare reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CompareOf {
    /// The firing's own values.
    #[default]
    TheFiring,
    /// A ball's (or a group's first live ball's) speed, size, hits and number, as it is now.
    AThing,
}

/// Compares one of the firing's values, or a thing's, with a number.
pub struct Compare;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CompareSettings {
    /// The firing's value: strength, speed, size, hits, ball, time, count, bar or beat.
    #[setting(label = "Value", simple, group = "Rule")]
    pub value: String,
    #[setting(label = "Of", group = "Rule")]
    pub of: CompareOf,
    #[setting(label = "Thing", when = "of=a_thing", group = "Rule")]
    pub who: Who,
    #[setting(label = "Is", simple, group = "Rule")]
    pub test: Test,
    #[setting(label = "Than", span(0, 2000), simple, group = "Rule")]
    pub amount: f32,
    #[setting(label = "And", when = "test=between", span(0, 2000), group = "Rule")]
    pub to: f32,
}

impl Default for CompareSettings {
    fn default() -> Self {
        CompareSettings {
            value: "strength".into(),
            of: CompareOf::TheFiring,
            who: Who::That,
            test: Test::Over,
            amount: 500.0,
            to: 1000.0,
        }
    }
}

block_kind!(
    Compare,
    "if.compare",
    "Compare",
    If,
    CompareSettings,
    "Goes on only when one of the firing's numbers passes a test.",
    values = [],
    from = ["amount", "to"]
);

#[derive(Clone, Debug)]
struct CompareState {
    value: String,
    of: CompareOf,
    who: Who,
    test: Test,
    amount: Amount,
    to: Amount,
}

impl BlockSim for Compare {
    fn kind(&self) -> &'static str {
        "if.compare"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = CompareSettings::from_params(params);
        if s.value.trim().is_empty() {
            return Err("Compare needs a value to look at".into());
        }
        Ok(Box::new(CompareState {
            value: s.value.trim().to_owned(),
            of: s.of,
            who: s.who,
            test: s.test,
            amount: Amount::read(params, "amount", s.amount as f64),
            to: Amount::read(params, "to", s.to as f64),
        }))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx) -> bool {
        let s = state::<CompareState>(st);
        let v = match s.of {
            CompareOf::TheFiring => firing.values.get(&s.value),
            CompareOf::AThing => thing_values(cx, &s.who, firing).get(&s.value),
        };
        let (Some(v), Some(a)) = (v, s.amount.of(firing)) else { return false };
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

/// The values of the first thing `who` names for `firing`: a ball's own (a whole thing's first
/// live ball's), and the time.
fn thing_values(cx: &StageCx, who: &Who, firing: &Firing) -> bpe_runtime::Values {
    let mut values = bpe_runtime::Values::default();
    values.set("time", cx.time);
    if let Some(&(thing, row)) = super::who::targets(cx, who, firing).first()
        && let Some(b) = cx.world.bodies.get(&thing)
        && let Some(r) = row.or_else(|| b.alive.iter().position(|&a| a).map(|r| r as u32))
    {
        super::when::ball_values(cx.world, bpe_runtime::Party { thing, item: r }, &mut values);
    }
    values
}

/// Which side of the firing Is looks at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Side {
    #[default]
    ThatBall,
    TheThingItHit,
}

/// What Is checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum IsTest {
    /// Is one of these things.
    #[default]
    Matches,
    /// Is the ball with this number.
    BallNumber,
}

/// Checks what the firing is about.
pub struct Is;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct IsSettings {
    #[setting(label = "Side", simple, group = "Rule")]
    pub side: Side,
    #[setting(label = "Test", simple, group = "Rule")]
    pub test: IsTest,
    #[setting(label = "Is", when = "test=matches", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Ball number", when = "test=ball_number", range(1, 1e9), span(1, 100), group = "Rule")]
    pub number: u32,
}

impl Default for IsSettings {
    fn default() -> Self {
        IsSettings { side: Side::ThatBall, test: IsTest::Matches, who: Who::Anything, number: 1 }
    }
}

block_kind!(
    Is,
    "if.is",
    "Is",
    If,
    IsSettings,
    "Goes on only when the ball, or what it hit, is the one you pick.",
    values = [],
    from = ["number"]
);

#[derive(Clone, Debug)]
struct IsState {
    s: IsSettings,
    number: Amount,
}

impl BlockSim for Is {
    fn kind(&self) -> &'static str {
        "if.is"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = IsSettings::from_params(params);
        let number = Amount::read(params, "number", s.number as f64);
        Ok(Box::new(IsState { s, number }))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx) -> bool {
        let s = state::<IsState>(st);
        let party = match s.s.side {
            Side::ThatBall => firing.subject,
            Side::TheThingItHit => firing.other,
        };
        let Some(party) = party else { return false };
        match s.s.test {
            IsTest::Matches => matches(cx.world, &s.s.who, party),
            IsTest::BallNumber => {
                let want = s.number.of(firing).map(|n| n.round());
                ball_number(cx.world, party).is_some_and(|n| Some(n as f64) == want)
            }
        }
    }
}

/// Goes on by chance.
pub struct Chance;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ChanceSettings {
    #[setting(label = "Chance", range(0, 100), unit = "%", simple, group = "Rule")]
    pub percent: f32,
}

impl Default for ChanceSettings {
    fn default() -> Self {
        ChanceSettings { percent: 50.0 }
    }
}

block_kind!(
    Chance,
    "if.chance",
    "Chance",
    If,
    ChanceSettings,
    "Goes on only some of the time, the same way on every play.",
    values = [],
    from = ["percent"]
);

impl BlockSim for Chance {
    fn kind(&self) -> &'static str {
        "if.chance"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = ChanceSettings::from_params(params);
        Ok(Box::new(Amount::read(params, "percent", s.percent as f64)))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx) -> bool {
        let p = state::<Amount>(st).or_fixed(firing);
        // Two firings about one ball in one step (two walls hit) roll apart: salted by where.
        let salt = firing.point.x.to_bits() ^ firing.point.y.to_bits().rotate_left(16);
        (cx.dice(firing.subject, salt) as f64) < p / 100.0
    }
}

/// Whether a counting check counts for the whole rule or for each ball on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Per {
    #[default]
    Rule,
    EachBall,
}

impl Per {
    fn key(self, firing: &Firing) -> Option<bpe_runtime::Party> {
        match self {
            Per::Rule => None,
            Per::EachBall => firing.subject,
        }
    }
}

/// Lets every Nth firing through.
pub struct EveryNth;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EveryNthSettings {
    #[setting(label = "Every", range(1, 1e9), span(1, 16), simple, group = "Rule")]
    pub n: u32,
    #[setting(label = "Counting", group = "Rule")]
    pub per: Per,
}

impl Default for EveryNthSettings {
    fn default() -> Self {
        EveryNthSettings { n: 2, per: Per::Rule }
    }
}

block_kind!(
    EveryNth,
    "if.every",
    "Every Nth",
    If,
    EveryNthSettings,
    "Goes on every second, third, … time it is reached."
);

/// A counting check's settings and what it remembers.
#[derive(Clone, Debug)]
struct Counting {
    n: u64,
    seconds: f64,
    per: Per,
    /// Per key: a count, or the step it last went on at (+1, so 0 is never).
    seen: Remembered<u64>,
}

impl BlockSim for EveryNth {
    fn kind(&self) -> &'static str {
        "if.every"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = EveryNthSettings::from_params(params);
        Ok(Box::new(Counting { n: s.n.max(1) as u64, seconds: 0.0, per: s.per, seen: Remembered::default() }))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, _cx: &StageCx) -> bool {
        let c = state_mut::<Counting>(st);
        let n = c.n;
        let count = c.seen.get(c.per.key(firing));
        *count += 1;
        (*count).is_multiple_of(n)
    }
}

/// Goes on, then waits a while before it goes on again.
pub struct Cooldown;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CooldownSettings {
    #[setting(label = "Wait", range(0, 1e9), span(0, 5), unit = "s", simple, group = "Rule")]
    pub seconds: f64,
    #[setting(label = "Counting", group = "Rule")]
    pub per: Per,
}

impl Default for CooldownSettings {
    fn default() -> Self {
        CooldownSettings { seconds: 0.5, per: Per::Rule }
    }
}

block_kind!(
    Cooldown,
    "if.cooldown",
    "Cooldown",
    If,
    CooldownSettings,
    "Goes on, then lets nothing through for a while."
);

impl BlockSim for Cooldown {
    fn kind(&self) -> &'static str {
        "if.cooldown"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = CooldownSettings::from_params(params);
        Ok(Box::new(Counting { n: 0, seconds: s.seconds.max(0.0), per: s.per, seen: Remembered::default() }))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx) -> bool {
        let c = state_mut::<Counting>(st);
        let wait = (c.seconds * cx.world.steps_per_second as f64 - 1e-6).ceil().max(0.0) as u64;
        let last = c.seen.get(c.per.key(firing));
        let free = *last == 0 || wait == 0 || cx.step >= *last - 1 + wait;
        if free {
            *last = cx.step + 1;
        }
        free
    }
}

/// Goes on only the first few times.
pub struct OnlyN;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct OnlyNSettings {
    #[setting(label = "Times", range(0, 1e9), span(1, 16), simple, group = "Rule")]
    pub n: u32,
    #[setting(label = "Counting", group = "Rule")]
    pub per: Per,
}

impl Default for OnlyNSettings {
    fn default() -> Self {
        OnlyNSettings { n: 1, per: Per::Rule }
    }
}

block_kind!(OnlyN, "if.only", "Only N times", If, OnlyNSettings, "Goes on the first few times, then never again.");

impl BlockSim for OnlyN {
    fn kind(&self) -> &'static str {
        "if.only"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = OnlyNSettings::from_params(params);
        Ok(Box::new(Counting { n: s.n as u64, seconds: 0.0, per: s.per, seen: Remembered::default() }))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, _cx: &StageCx) -> bool {
        let c = state_mut::<Counting>(st);
        let n = c.n;
        let count = c.seen.get(c.per.key(firing));
        if *count < n {
            *count += 1;
            true
        } else {
            false
        }
    }
}

/// Goes on only between two moments.
pub struct BetweenTimes;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BetweenTimesSettings {
    #[setting(label = "From", range(0, 1e9), span(0, 60), unit = "s", simple, group = "Rule")]
    pub from: f64,
    #[setting(label = "To", range(0, 1e9), span(0, 60), unit = "s", simple, group = "Rule")]
    pub to: f64,
}

impl Default for BetweenTimesSettings {
    fn default() -> Self {
        BetweenTimesSettings { from: 0.0, to: 10.0 }
    }
}

block_kind!(
    BetweenTimes,
    "if.between",
    "Between times",
    If,
    BetweenTimesSettings,
    "Goes on only between two moments of the clip."
);

impl BlockSim for BetweenTimes {
    fn kind(&self) -> &'static str {
        "if.between"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(BetweenTimesSettings::from_params(params)))
    }
    fn pass(&self, st: &mut dyn ThingState, _firing: &Firing, cx: &StageCx) -> bool {
        let s = state::<BetweenTimesSettings>(st);
        let (a, b) = (s.from.min(s.to), s.from.max(s.to));
        cx.time >= a - 1e-9 && cx.time <= b + 1e-9
    }
}
