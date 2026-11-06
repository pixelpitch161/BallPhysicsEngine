//! Do: Camera (rules part 6). Shakes the camera or punches its zoom.

use bpe_core::{BlockSection, Choice, KindId, Params, Settings, Who};
use bpe_runtime::{Action, BlockSim, Firing, Kick, PrepareCx, StageCx, ThingState, state};

use super::amount::Amount;
use super::block_kind;
use super::who::targets;

/// The camera thing's kind.
const CAMERA: &str = "camera";

/// What Camera does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CameraMove {
    /// Shakes the shot about.
    #[default]
    Shake,
    /// Zooms in at once, then eases back.
    ZoomPunch,
}

pub struct CameraKick;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CameraKickSettings {
    /// The camera (any camera unless one is named).
    #[setting(label = "Camera", group = "Rule")]
    pub who: Who,
    #[setting(label = "Do", simple, group = "Rule")]
    pub mode: CameraMove,
    /// How far the shake moves the shot at its start.
    #[setting(
        label = "Strength",
        when = "mode=shake",
        range(0, 1e9),
        span(0, 200),
        unit = "px",
        simple,
        group = "Rule"
    )]
    pub amount: f32,
    /// How much closer the punch goes, as a share of the zoom (below 0 pulls out).
    #[setting(label = "Amount", when = "mode=zoom_punch", range(-0.95, 20), span(-0.5, 1), simple, group = "Rule")]
    pub punch: f32,
    /// How long it takes to die away.
    #[setting(label = "Time", range(0.01, 1e9), span(0.05, 3), unit = "s", simple, group = "Rule")]
    pub seconds: f32,
    /// How often the shake changes direction.
    #[setting(label = "Wobbles", when = "mode=shake", range(0.1, 200), span(1, 60), unit = "/s", group = "Rule")]
    pub rate: f32,
}

impl Default for CameraKickSettings {
    fn default() -> Self {
        CameraKickSettings {
            who: Who::Kind(KindId::from(CAMERA)),
            mode: CameraMove::Shake,
            amount: 40.0,
            punch: 0.15,
            seconds: 0.4,
            rate: 18.0,
        }
    }
}

block_kind!(
    CameraKick,
    "do.camera",
    "Camera",
    Do,
    CameraKickSettings,
    "Shakes the camera or punches its zoom, dying away over a time."
);

#[derive(Clone, Debug)]
struct CameraKickState {
    s: CameraKickSettings,
    amount: Amount,
    punch: Amount,
    seconds: Amount,
}

impl BlockSim for CameraKick {
    fn kind(&self) -> &'static str {
        "do.camera"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = CameraKickSettings::from_params(params);
        if let Some(t) = s.who.names_thing()
            && let Some(thing) = cx.scene.things.get(&t)
            && thing.kind != CAMERA
        {
            return Err(format!("{} isn't a camera", thing.name));
        }
        let amount = Amount::read(params, "amount", s.amount as f64);
        let punch = Amount::read(params, "punch", s.punch as f64);
        let seconds = Amount::read(params, "seconds", s.seconds as f64);
        Ok(Box::new(CameraKickState { s, amount, punch, seconds }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let k = state::<CameraKickState>(st);
        let punch = k.s.mode == CameraMove::ZoomPunch;
        let amount = if punch {
            (k.punch.or_fixed(firing) as f32).clamp(-0.95, 20.0)
        } else {
            (k.amount.or_fixed(firing) as f32).max(0.0)
        };
        let seconds = (k.seconds.or_fixed(firing) as f32).max(0.01);
        for (thing, _) in targets(cx, &k.s.who, firing) {
            if cx.world.kind_of(thing) != Some(CAMERA) {
                continue;
            }
            out.push(Action::Kick(Kick { thing, start: 0, punch, amount, seconds, rate: k.s.rate.clamp(0.1, 200.0) }));
        }
    }
}
