//! Do: Slow motion (rules part 6). The simulation runs at a share of its speed (or faster) for a
//! while, gliding in and out, while the video clock and keyframes run on.

use bpe_core::{BlockSection, Params, Settings, Value};
use bpe_runtime::rules::{MAX_PACE, MIN_PACE};
use bpe_runtime::{Action, BlockSim, Change, Firing, How, PrepareCx, StageCx, ThingState, state};

use super::amount::Amount;
use super::block_kind;
use super::change::{GlideEase, Lasts};

pub struct SlowMotion;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SlowMotionSettings {
    /// How fast the simulation runs: 0.25 is a quarter speed, 2 twice as fast.
    #[setting(label = "Speed", range(0.01, 8), span(0.05, 2), unit = "×", simple, group = "Rule")]
    pub speed: f32,
    /// Seconds to glide into it, and back out when it ends.
    #[setting(label = "Glide", range(0, 1e9), span(0, 2), unit = "s", group = "Rule")]
    pub glide: f64,
    #[setting(label = "Ease", when = "glide", group = "Rule")]
    pub ease: GlideEase,
    #[setting(label = "Lasts", group = "Rule")]
    pub lasts: Lasts,
    /// Seconds of the video it lasts before it glides back.
    #[setting(
        label = "For",
        when = "lasts=for_a_time",
        range(0, 1e9),
        span(0, 10),
        unit = "s",
        simple,
        group = "Rule"
    )]
    pub seconds: f64,
}

impl Default for SlowMotionSettings {
    fn default() -> Self {
        SlowMotionSettings { speed: 0.25, glide: 0.3, ease: GlideEase::Smooth, lasts: Lasts::ForATime, seconds: 1.0 }
    }
}

block_kind!(
    SlowMotion,
    "do.slow",
    "Slow motion",
    Do,
    SlowMotionSettings,
    "Runs the simulation slower (or faster) for a while; the video and its keyframes run on.",
    values = [],
    from = ["speed", "seconds"]
);

#[derive(Clone, Debug)]
struct SlowState {
    s: SlowMotionSettings,
    speed: Amount,
    seconds: Amount,
}

impl BlockSim for SlowMotion {
    fn kind(&self) -> &'static str {
        "do.slow"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = SlowMotionSettings::from_params(params);
        let speed = Amount::read(params, "speed", s.speed as f64);
        let seconds = Amount::read(params, "seconds", s.seconds);
        Ok(Box::new(SlowState { s, speed, seconds }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, _cx: &StageCx, out: &mut Vec<Action>) {
        let m = state::<SlowState>(st);
        let speed = m.speed.or_fixed(firing).clamp(MIN_PACE as f64, MAX_PACE as f64);
        let lasts = match m.s.lasts {
            Lasts::ForGood => None,
            Lasts::ForATime => Some(m.seconds.or_fixed(firing).max(0.0)),
        };
        out.push(Action::Pace(Box::new(Change {
            how: How::Set,
            value: Value::Number(speed),
            start: 0.0,
            glide: if m.s.glide.is_finite() { m.s.glide.max(0.0) } else { 0.0 },
            ease: m.s.ease.ease(),
            lasts,
            range: Some((MIN_PACE as f64, MAX_PACE as f64)),
        })));
    }
}
