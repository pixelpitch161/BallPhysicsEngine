//! Signals: Do Send signal and When Signal. A signal sent in one round of the Rules stage is
//! heard in its next round (at most `MOST_ROUNDS` more a step; the last round's at the next step),
//! carrying the sender's firing: who it was about, where, and its values.

use bpe_core::{BlockSection, Params, Settings};
use bpe_runtime::{Action, BlockSim, Firing, PrepareCx, Signal, StageCx, ThingState, state};

use super::block_kind;

/// Sends a signal.
pub struct SendSignal;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SignalSettings {
    /// The signal's name: When Signal blocks with the same name hear it.
    #[setting(label = "Signal", simple, group = "Rule")]
    pub name: String,
}

impl Default for SignalSettings {
    fn default() -> Self {
        SignalSettings { name: "go".into() }
    }
}

block_kind!(
    SendSignal,
    "do.signal",
    "Send signal",
    Do,
    SignalSettings,
    "Sends a signal that starts every rule with a Signal block of the same name, right away."
);

/// The signal's name, trimmed; an empty one is a problem.
fn name_of(params: &Params) -> Result<String, String> {
    let name = SignalSettings::from_params(params).name.trim().to_owned();
    if name.is_empty() { Err("A signal needs a name".into()) } else { Ok(name) }
}

impl BlockSim for SendSignal {
    fn kind(&self) -> &'static str {
        "do.signal"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(name_of(params)?))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, _cx: &StageCx, out: &mut Vec<Action>) {
        out.push(Action::Signal { name: state::<String>(st).clone(), firing: firing.clone() });
    }
}

/// When a signal comes.
pub struct HearSignal;

block_kind!(
    HearSignal,
    "when.signal",
    "Signal",
    When,
    SignalSettings,
    "Fires when another rule (or this one) sends this signal, with what that rule knew."
);

impl BlockSim for HearSignal {
    fn kind(&self) -> &'static str {
        "when.signal"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(name_of(params)?))
    }
    fn hear(&self, st: &mut dyn ThingState, signals: &[Signal], cx: &StageCx, out: &mut Vec<Firing>) {
        let name = state::<String>(st);
        for s in signals.iter().filter(|s| s.name == *name) {
            out.push(Firing { step: cx.step, ..s.firing.clone() });
        }
    }
}
