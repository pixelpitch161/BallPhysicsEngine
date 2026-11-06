//! The Health blocks (rules part 4): When Health, If Health is and Do Change health. Health
//! itself is kept by the world (`bpe_runtime::rules::Health`), from each thing's Health card.

use bpe_core::{BlockSection, Choice, Params, Settings, SlotId, ThingId, Who};
use bpe_runtime::rules::{HealthChange, HealthMoment, Hp};
use bpe_runtime::{Action, BlockSim, Firing, Party, PrepareCx, StageCx, ThingState, Values, state};

use super::amount::Amount;
use super::block_kind;
use super::when::ball_values;
use super::who::{matches, targets};

/// A party's health, by its ball's row when it is a ball of a thing with balls.
pub fn health_of(cx: &StageCx, party: Party) -> Option<Hp> {
    let row = cx.world.bodies.contains_key(&party.thing).then_some(party.item);
    cx.health.of(party.thing, row)
}

/// Adds `party`'s `health`, `share` (of its starting health, 0 to 1) and `start` to `values`,
/// when it has health.
pub fn health_values(cx: &StageCx, party: Party, values: &mut Values) {
    if let Some(hp) = health_of(cx, party) {
        values.set("health", hp.now);
        values.set("share", hp.share() as f64);
        values.set("start", hp.start);
    }
}

/// A fixed `who` naming a thing without a Health card (or the scene settings): a problem.
fn named_without_health(cx: &PrepareCx, who: &Who) -> Result<(), String> {
    let Some(id) = who.names_thing() else { return Ok(()) };
    if id == ThingId::ROOT {
        return Err("The scene settings have no health".into());
    }
    let has = cx.resolved.things.get(&id).is_some_and(|r| r.slots.contains_key(&SlotId::Health));
    let name = cx.scene.things.get(&id).map_or("That thing", |t| t.name.as_str());
    if has { Ok(()) } else { Err(format!("{name} has no Health card")) }
}

/// What change of health When Health fires on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HealthEvent {
    /// It went down.
    #[default]
    Drops,
    /// It reached 0 just now.
    ReachesZero,
    /// It went up.
    GoesUp,
    /// Any change.
    Changes,
}

impl HealthEvent {
    fn fits(self, m: &HealthMoment) -> bool {
        match self {
            HealthEvent::Drops => m.after < m.before,
            HealthEvent::ReachesZero => m.before > 0.0 && m.after <= 0.0,
            HealthEvent::GoesUp => m.after > m.before,
            HealthEvent::Changes => m.after != m.before,
        }
    }
}

/// When a ball's or a thing's health changes.
pub struct WhenHealth;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WhenHealthSettings {
    /// Whose health: a ball, the balls of a thing, a tag, a kind or anything.
    #[setting(label = "Whose", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "When it", simple, group = "Rule")]
    pub event: HealthEvent,
}

impl Default for WhenHealthSettings {
    fn default() -> Self {
        WhenHealthSettings { who: Who::Anything, event: HealthEvent::Drops }
    }
}

block_kind!(
    WhenHealth,
    "when.health",
    "Health",
    When,
    WhenHealthSettings,
    "Fires when a ball's or a thing's health drops, reaches 0 or goes up."
);

impl BlockSim for WhenHealth {
    fn kind(&self) -> &'static str {
        "when.health"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = WhenHealthSettings::from_params(params);
        named_without_health(cx, &s.who)?;
        Ok(Box::new(s))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let s = state::<WhenHealthSettings>(st);
        for m in cx.health.moments().iter().filter(|m| s.event.fits(m)) {
            let subject = Party { thing: m.thing, item: m.row.unwrap_or(0) };
            if !matches(cx.world, &s.who, subject) {
                continue;
            }
            let mut values = Values::default();
            values.set("health", m.after);
            values.set("damage", m.before - m.after);
            values.set("start", m.start);
            let share = if m.start > 0.0 { (m.after / m.start).clamp(0.0, 1.0) } else { 0.0 };
            values.set("share", share);
            values.set("strength", m.strength as f64);
            if m.row.is_some() {
                ball_values(cx.world, subject, &mut values);
            }
            out.push(Firing {
                step: cx.step,
                subject: Some(subject),
                other: m.other,
                point: m.point,
                normal: m.normal,
                values,
            });
        }
    }
}

/// Whose health Health is reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HealthOf {
    #[default]
    ThatBall,
    TheThingItHit,
    /// A thing (or one of its balls) you pick.
    AThing,
}

/// How Health is tests it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HealthTest {
    #[default]
    Below,
    Above,
    Between,
    /// None left.
    AtZero,
    /// At (or over) its starting health.
    Full,
}

/// What Health is compares: health points, or the share of the starting health in %.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Measure {
    #[default]
    Points,
    Percent,
}

/// Goes on only when a ball's or a thing's health passes a test.
pub struct HealthIs;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HealthIsSettings {
    #[setting(label = "Of", simple, group = "Rule")]
    pub of: HealthOf,
    #[setting(label = "Thing", when = "of=a_thing", group = "Rule")]
    pub who: Who,
    #[setting(label = "Is", simple, group = "Rule")]
    pub test: HealthTest,
    #[setting(label = "In", when = "test!=at_zero and test!=full", group = "Rule")]
    pub measure: Measure,
    #[setting(label = "Than", when = "test!=at_zero and test!=full", span(0, 20), simple, group = "Rule")]
    pub amount: f64,
    #[setting(label = "And", when = "test=between", span(0, 20), group = "Rule")]
    pub to: f64,
}

impl Default for HealthIsSettings {
    fn default() -> Self {
        HealthIsSettings {
            of: HealthOf::ThatBall,
            who: Who::That,
            test: HealthTest::Below,
            measure: Measure::Points,
            amount: 2.0,
            to: 5.0,
        }
    }
}

block_kind!(
    HealthIs,
    "if.health",
    "Health is",
    If,
    HealthIsSettings,
    "Goes on only when a ball's or a thing's health is below, above or between amounts, at zero or full."
);

#[derive(Clone, Debug)]
struct HealthIsState {
    s: HealthIsSettings,
    amount: Amount,
    to: Amount,
}

impl BlockSim for HealthIs {
    fn kind(&self) -> &'static str {
        "if.health"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = HealthIsSettings::from_params(params);
        if s.of == HealthOf::AThing {
            named_without_health(cx, &s.who)?;
        }
        let amount = Amount::read(params, "amount", s.amount);
        let to = Amount::read(params, "to", s.to);
        Ok(Box::new(HealthIsState { s, amount, to }))
    }
    fn pass(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx) -> bool {
        let s = state::<HealthIsState>(st);
        let hp = match s.s.of {
            HealthOf::ThatBall => firing.subject.and_then(|p| health_of(cx, p)),
            HealthOf::TheThingItHit => firing.other.and_then(|p| health_of(cx, p)),
            HealthOf::AThing => targets(cx, &s.s.who, firing).into_iter().find_map(|(thing, row)| {
                // A whole thing with balls reads its first live ball.
                let row = row.or_else(|| {
                    cx.world.bodies.get(&thing).and_then(|b| b.alive.iter().position(|&a| a)).map(|r| r as u32)
                });
                cx.health.of(thing, row)
            }),
        };
        let Some(hp) = hp else { return false };
        let v = match s.s.measure {
            Measure::Points => hp.now,
            Measure::Percent => hp.share() as f64 * 100.0,
        };
        match s.s.test {
            HealthTest::AtZero => hp.now <= 0.0,
            HealthTest::Full => hp.now >= hp.start,
            HealthTest::Below => s.amount.of(firing).is_some_and(|a| v < a),
            HealthTest::Above => s.amount.of(firing).is_some_and(|a| v > a),
            HealthTest::Between => {
                let (Some(a), Some(b)) = (s.amount.of(firing), s.to.of(firing)) else { return false };
                v >= a.min(b) && v <= a.max(b)
            }
        }
    }
}

/// How Change health changes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HealthHow {
    /// Takes the amount away.
    #[default]
    Damage,
    /// Gives the amount back (past the starting health too).
    Heal,
    /// Sets it to the amount.
    Set,
    /// Back to its starting health.
    Refill,
}

/// Changes a ball's or a thing's health.
pub struct ChangeHealth;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ChangeHealthSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "How", simple, group = "Rule")]
    pub how: HealthHow,
    #[setting(label = "Amount", when = "how!=refill", span(0, 10), simple, group = "Rule")]
    pub amount: f64,
}

impl Default for ChangeHealthSettings {
    fn default() -> Self {
        ChangeHealthSettings { who: Who::That, how: HealthHow::Damage, amount: 1.0 }
    }
}

block_kind!(
    ChangeHealth,
    "do.health",
    "Change health",
    Do,
    ChangeHealthSettings,
    "Takes health away from a ball or a thing, gives it back, sets it or fills it up again."
);

#[derive(Clone, Debug)]
struct ChangeHealthState {
    s: ChangeHealthSettings,
    amount: Amount,
}

impl BlockSim for ChangeHealth {
    fn kind(&self) -> &'static str {
        "do.health"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = ChangeHealthSettings::from_params(params);
        named_without_health(cx, &s.who)?;
        let amount = Amount::read(params, "amount", s.amount);
        Ok(Box::new(ChangeHealthState { s, amount }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<ChangeHealthState>(st);
        let Some(x) = s.amount.of(firing) else { return };
        let change = match s.s.how {
            HealthHow::Damage => HealthChange::Damage(x),
            HealthHow::Heal => HealthChange::Heal(x),
            HealthHow::Set => HealthChange::Set(x),
            HealthHow::Refill => HealthChange::Refill,
        };
        for (thing, row) in targets(cx, &s.s.who, firing) {
            // Only a thing with a Health card has health; a whole thing with balls changes each
            // live ball's.
            if cx.health.has(thing) {
                out.push(Action::Health { thing, row, change });
            }
        }
    }
}
