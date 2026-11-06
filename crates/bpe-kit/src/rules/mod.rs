//! Rules (spec `docs/superpowers/specs/2026-11-06-rules-design.md`, plan
//! `docs/superpowers/plans/rules/part1.md`): every rule block kind. Each is one struct that is
//! both the core's [`RuleBlockKind`] (its name and settings) and the runtime's [`BlockSim`]
//! (what it does in the Rules stage).
//!
//! - When: [`when::Hit`], [`when::Time`], [`when::Beat`].
//! - If: [`checks::Compare`], [`checks::Is`], [`checks::Chance`], [`checks::EveryNth`],
//!   [`checks::Cooldown`], [`checks::OnlyN`], [`checks::BetweenTimes`].
//! - Logic: [`logic::Logic`].
//! - Do: [`motion::ChangeMotion`], [`change::ChangeSetting`], [`breaks::Break`].
//!
//! [`RuleBlockKind`]: bpe_core::RuleBlockKind
//! [`BlockSim`]: bpe_runtime::BlockSim

pub mod amount;
pub mod breaks;
pub mod change;
pub mod checks;
pub mod logic;
pub mod memory;
pub mod motion;
pub mod when;
pub mod who;

#[cfg(test)]
mod tests;

use bpe_core::Registry;
use bpe_runtime::Sims;

/// Gives a block kind's struct its [`bpe_core::RuleBlockKind`] side from its settings struct.
macro_rules! block_kind {
    ($kind:ty, $id:literal, $name:literal, $section:ident, $settings:ty, $help:literal) => {
        impl bpe_core::RuleBlockKind for $kind {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> &'static str {
                $name
            }
            fn help(&self) -> &'static str {
                $help
            }
            fn section(&self) -> bpe_core::BlockSection {
                bpe_core::BlockSection::$section
            }
            fn settings(&self) -> &'static [bpe_core::SettingDef] {
                <$settings as bpe_core::Settings>::defs()
            }
        }
    };
}
pub(crate) use block_kind;

/// Puts every block kind in the registry.
pub fn register(r: &mut Registry) {
    r.add_block(when::Hit);
    r.add_block(when::Time);
    r.add_block(when::Beat);
    r.add_block(checks::Compare);
    r.add_block(checks::Is);
    r.add_block(checks::Chance);
    r.add_block(checks::EveryNth);
    r.add_block(checks::Cooldown);
    r.add_block(checks::OnlyN);
    r.add_block(checks::BetweenTimes);
    r.add_block(logic::Logic);
    r.add_block(motion::ChangeMotion);
    r.add_block(change::ChangeSetting);
    r.add_block(breaks::Break);
}

/// Puts every block kind's simulation in the sims.
pub fn add_sims(s: &mut Sims) {
    s.add_block(when::Hit);
    s.add_block(when::Time);
    s.add_block(when::Beat);
    s.add_block(checks::Compare);
    s.add_block(checks::Is);
    s.add_block(checks::Chance);
    s.add_block(checks::EveryNth);
    s.add_block(checks::Cooldown);
    s.add_block(checks::OnlyN);
    s.add_block(checks::BetweenTimes);
    s.add_block(logic::Logic);
    s.add_block(motion::ChangeMotion);
    s.add_block(change::ChangeSetting);
    s.add_block(breaks::Break);
}

/// The kit's registry, made once for the process: what Change setting reads the scene's setting
/// descriptions with, since the sims aren't handed a registry.
pub fn kit_registry() -> &'static Registry {
    static REG: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    REG.get_or_init(crate::registry)
}
