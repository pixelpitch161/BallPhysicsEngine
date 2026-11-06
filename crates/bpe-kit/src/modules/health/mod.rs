//! The Health card (rules part 4, spec `docs/superpowers/specs/2026-11-06-rules-design.md`
//! section 7): a ball, a group's balls, an emitter's balls or a shape with one keeps a health of
//! its own, which hits take away. At zero it breaks, or nothing happens and rules decide. The
//! world keeps the health (`bpe_runtime::rules::Health`); this card only says how.

use bpe_core::{Choice, ModuleKind, Params, SettingDef, Settings, SlotId, Target, ThingId, VaryUse};
use bpe_runtime::rules::{Action, HealthCard, HealthSim, Takes};
use glam::Vec2;

/// The Health card's kind id.
pub const HEALTH: &str = "health";

/// How much health a hit takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HitsTake {
    /// The same amount every hit.
    #[default]
    EachHit,
    /// More for a harder hit: its strength × a factor.
    ByStrength,
    /// Nothing: only rules change it.
    Nothing,
}

/// What happens when the health reaches 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AtZero {
    /// The ball (or the shape) breaks.
    #[default]
    Break,
    /// Nothing: rules decide.
    Nothing,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HealthSettings {
    /// How much health each ball (or the shape) starts with.
    #[setting(label = "Starting health", range(0, 1e12), span(1, 20), simple, group = "Health")]
    pub start: f64,
    /// How much a hit takes away.
    #[setting(label = "Hits take", simple, group = "Health")]
    pub takes: HitsTake,
    /// What each hit takes (a negative amount heals).
    #[setting(label = "Each hit takes", when = "takes=each_hit", span(0, 10), simple, group = "Health")]
    pub amount: f64,
    /// A hit takes its strength (px/s) × this: at 0.01 a 300 px/s hit takes 3.
    #[setting(label = "Per px/s of strength", when = "takes=by_strength", span(0, 0.1), group = "Health")]
    pub factor: f64,
    /// Hits softer than this take nothing.
    #[setting(
        label = "Least strength",
        when = "takes!=nothing",
        range(0, 1e9),
        span(0, 2000),
        unit = "px/s",
        group = "Health"
    )]
    pub least: f32,
    /// Only hits from these take health.
    #[setting(label = "Only hits from", when = "takes!=nothing", group = "Health")]
    pub from: Target,
    /// What happens when the health reaches 0.
    #[setting(label = "At zero", simple, group = "Health")]
    pub at_zero: AtZero,
}

impl Default for HealthSettings {
    fn default() -> Self {
        HealthSettings {
            start: 3.0,
            takes: HitsTake::EachHit,
            amount: 1.0,
            factor: 0.01,
            least: 0.0,
            from: Target::Anything,
            at_zero: AtZero::Break,
        }
    }
}

/// The Health card.
pub struct HealthKind;

impl ModuleKind for HealthKind {
    fn id(&self) -> &'static str {
        HEALTH
    }
    fn name(&self) -> &'static str {
        "Health"
    }
    fn help(&self) -> &'static str {
        "A health of its own that hits take away: at zero it breaks, or rules decide what happens."
    }
    fn slot(&self) -> SlotId {
        SlotId::Health
    }
    fn settings(&self) -> &'static [SettingDef] {
        HealthSettings::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        matches!(thing_kind, "balls" | "emitter" | "shape")
    }
    /// Each ball of a group can start with a health of its own, worked out once as it starts.
    fn vary(&self, key: &str) -> Option<VaryUse> {
        (key == "start").then_some(VaryUse::AtLaunch)
    }
}

/// How the world reads a Health card.
pub struct HealthCardSim;

impl HealthSim for HealthCardSim {
    fn kind(&self) -> &'static str {
        HEALTH
    }
    fn prepare(&self, params: &Params) -> HealthCard {
        let s = HealthSettings::from_params(params);
        let start_vary = bpe_core::vary::vary_of(params, "start").and_then(|v| match v.to {
            Some(bpe_core::Value::Number(to)) => Some((to, v)),
            _ => None,
        });
        HealthCard {
            start: s.start,
            start_vary,
            takes: match s.takes {
                HitsTake::EachHit => Takes::EachHit(s.amount),
                HitsTake::ByStrength => Takes::ByStrength(s.factor),
                HitsTake::Nothing => Takes::Nothing,
            },
            least: s.least,
            from: s.from,
            at_zero: match s.at_zero {
                // Aimed at the ball or the shape when it is used.
                AtZero::Break => Some(Action::Break {
                    thing: ThingId::ROOT,
                    row: None,
                    point: Vec2::ZERO,
                    normal: Vec2::ZERO,
                    strength: 0.0,
                }),
                AtZero::Nothing => None,
            },
        }
    }
}
