//! The Logic block: joins two or more lanes (spec section 4).

use std::collections::VecDeque;

use bpe_core::{BlockSection, Choice, Params, Settings};
use bpe_runtime::rules::MOST_REMEMBERED;
use bpe_runtime::{BlockSim, Firing, Party, PrepareCx, StageCx, ThingState, state_mut};

use super::block_kind;
use super::memory::Remembered;

/// How a Logic block joins its inputs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LogicMode {
    /// Every input fired within a short time of each other.
    #[default]
    AndWithin,
    /// Every input has fired since the block last went on.
    AndSinceLast,
    /// The inputs fired in order, each within the time of the one before.
    Then,
    /// Any input.
    Or,
    /// The first input fired and none of the others did within the time.
    Not,
    /// At least a number of the inputs fired within the time.
    AnyN,
}

pub struct Logic;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LogicSettings {
    #[setting(label = "Mode", simple, group = "Rule")]
    pub mode: LogicMode,
    #[setting(
        label = "Within",
        when = "mode=and_within|then|not|any_n",
        range(0, 1e9),
        span(0, 5),
        unit = "s",
        simple,
        group = "Rule"
    )]
    pub within: f64,
    #[setting(label = "At least", when = "mode=any_n", range(1, 64), span(1, 8), group = "Rule")]
    pub n: u32,
    /// Only joins firings about the same ball.
    #[setting(label = "Same ball", group = "Rule")]
    pub same_ball: bool,
}

impl Default for LogicSettings {
    fn default() -> Self {
        LogicSettings { mode: LogicMode::AndWithin, within: 0.5, n: 2, same_ball: false }
    }
}

block_kind!(
    Logic,
    "logic",
    "Logic",
    Logic,
    LogicSettings,
    "Joins two or more lanes: And, Then, Or, Not or Any of them."
);

/// What a Logic block remembers per key.
#[derive(Clone, Debug, Default)]
struct Memory {
    /// Each input's latest firing and its step.
    last: Vec<Option<(u64, Firing)>>,
    /// Then: how many inputs have come in order, and the step of the latest.
    progress: usize,
    then_at: u64,
    /// Not: the step any other input last fired at (+1, 0 for never).
    others_at: u64,
}

#[derive(Clone, Debug)]
struct LogicState {
    mode: LogicMode,
    /// Within, in seconds.
    within: f64,
    n: usize,
    same_ball: bool,
    memory: Remembered<Memory>,
    /// Not: firings of the first input waiting out the time, with their key.
    waiting: VecDeque<(Option<Party>, Firing)>,
}

/// `firing` with the values of the other inputs' latest firings where its names are free.
fn joined(firing: &Firing, last: &[Option<(u64, Firing)>]) -> Firing {
    let mut out = firing.clone();
    for (_, f) in last.iter().flatten() {
        out.values.merge_missing(&f.values);
    }
    out
}

impl BlockSim for Logic {
    fn kind(&self) -> &'static str {
        "logic"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Logic
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = LogicSettings::from_params(params);
        Ok(Box::new(LogicState {
            mode: s.mode,
            within: s.within.max(0.0),
            n: s.n.max(1) as usize,
            same_ball: s.same_ball,
            memory: Remembered::default(),
            waiting: VecDeque::new(),
        }))
    }

    fn join(
        &self,
        st: &mut dyn ThingState,
        input: usize,
        inputs: usize,
        firing: &Firing,
        cx: &StageCx,
        out: &mut Vec<Firing>,
    ) {
        let s = state_mut::<LogicState>(st);
        let now = cx.step;
        let key = if s.same_ball { firing.subject } else { None };
        let window = steps(s.within, cx);
        let (mode, n) = (s.mode, s.n.min(inputs));
        if mode == LogicMode::Or {
            out.push(firing.clone());
            return;
        }
        if mode == LogicMode::Not {
            if input == 0 {
                let m = s.memory.get(key);
                // Another input already fired this step: closed before it opened.
                if m.others_at != now + 1 {
                    if s.waiting.len() >= MOST_REMEMBERED {
                        s.waiting.pop_front();
                    }
                    s.waiting.push_back((key, firing.clone()));
                }
            } else {
                s.memory.get(key).others_at = now + 1;
                s.waiting.retain(|(k, _)| *k != key);
            }
            return;
        }
        let m = s.memory.get(key);
        m.last.resize(inputs, None);
        match mode {
            LogicMode::AndWithin | LogicMode::AndSinceLast | LogicMode::AnyN => {
                m.last[input] = Some((now, firing.clone()));
                let fresh = |l: &Option<(u64, Firing)>| {
                    l.as_ref().is_some_and(|(at, _)| mode == LogicMode::AndSinceLast || now - at <= window)
                };
                let count = m.last.iter().filter(|l| fresh(l)).count();
                let need = if mode == LogicMode::AnyN { n } else { inputs };
                if count >= need {
                    let kept: Vec<Option<(u64, Firing)>> =
                        m.last.iter().map(|l| l.clone().filter(|_| fresh(l))).collect();
                    out.push(joined(firing, &kept));
                    m.last.iter_mut().for_each(|l| *l = None);
                }
            }
            LogicMode::Then => {
                let late = m.progress > 0 && now - m.then_at > window;
                if late {
                    m.progress = 0;
                }
                if input == m.progress {
                    m.last[input] = Some((now, firing.clone()));
                    m.progress += 1;
                    m.then_at = now;
                    if m.progress == inputs {
                        out.push(joined(firing, &m.last));
                        m.progress = 0;
                        m.last.iter_mut().for_each(|l| *l = None);
                    }
                } else if input == 0 {
                    // The first input again starts over.
                    m.last.iter_mut().for_each(|l| *l = None);
                    m.last[0] = Some((now, firing.clone()));
                    m.progress = 1;
                    m.then_at = now;
                }
            }
            LogicMode::Or | LogicMode::Not => unreachable!(),
        }
    }

    fn tick(&self, st: &mut dyn ThingState, _inputs: usize, cx: &StageCx, out: &mut Vec<Firing>) {
        let s = state_mut::<LogicState>(st);
        if s.mode != LogicMode::Not {
            return;
        }
        let (now, window) = (cx.step, steps(s.within, cx));
        while let Some((_, f)) = s.waiting.front() {
            if now.saturating_sub(f.step) <= window {
                break;
            }
            let (_, f) = s.waiting.pop_front().expect("just looked");
            out.push(f);
        }
    }
}

/// `seconds` in whole steps (a millionth of a step under counts as the step).
fn steps(seconds: f64, cx: &StageCx) -> u64 {
    (seconds * cx.world.steps_per_second as f64 + 1e-6).floor() as u64
}
