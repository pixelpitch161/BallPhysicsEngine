//! Do: Change motion.

use bpe_core::{BlockSection, Choice, Params, Settings, Who};
use bpe_runtime::{Action, BlockSim, Firing, MotionChange, PrepareCx, StageCx, ThingState, state};
use glam::Vec2;

use super::amount::Amount;
use super::block_kind;
use super::who::{place_of, targets};

/// What Change motion does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MotionMode {
    /// Adds speed along the way it is going.
    #[default]
    AddSpeed,
    SetSpeed,
    MultiplySpeed,
    /// Pushes it a way.
    Push,
    Stop,
    /// Turns it toward a place, keeping its speed.
    AimAt,
    /// Holds it still for a while.
    Freeze,
    /// Moves it to a place at once.
    Teleport,
}

/// Which way Push pushes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Way {
    #[default]
    Up,
    /// Straight off what it hit.
    AwayFromTheHit,
    TowardAThing,
    /// An angle: 0° up, turning clockwise.
    Angle,
}

/// The place Aim at and Teleport use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PlaceFrom {
    /// Where the firing happened (where the hit was).
    #[default]
    TheFiringsPoint,
    /// Where a thing is.
    AThingsPlace,
}

pub struct ChangeMotion;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ChangeMotionSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Do", simple, group = "Rule")]
    pub mode: MotionMode,
    /// Pixels a second; for Multiply the factor, for Freeze the seconds.
    #[setting(label = "Amount", when = "mode!=stop|aim_at|teleport", span(0, 2000), simple, group = "Rule")]
    pub amount: f32,
    #[setting(label = "Way", when = "mode=push", group = "Rule")]
    pub way: Way,
    #[setting(label = "Toward", when = "mode=push and way=toward_a_thing", group = "Rule")]
    pub toward: Who,
    #[setting(label = "Angle", when = "mode=push and way=angle", span(-180, 180), unit = "°", group = "Rule")]
    pub angle: f32,
    #[setting(label = "To", when = "mode=aim_at|teleport", group = "Rule")]
    pub to: PlaceFrom,
    #[setting(label = "Thing", when = "mode=aim_at|teleport and to=a_things_place", group = "Rule")]
    pub place: Who,
}

impl Default for ChangeMotionSettings {
    fn default() -> Self {
        ChangeMotionSettings {
            who: Who::That,
            mode: MotionMode::AddSpeed,
            amount: 300.0,
            way: Way::Up,
            toward: Who::Anything,
            angle: 0.0,
            to: PlaceFrom::TheFiringsPoint,
            place: Who::Anything,
        }
    }
}

block_kind!(
    ChangeMotion,
    "do.motion",
    "Change motion",
    Do,
    ChangeMotionSettings,
    "Speeds up, slows, pushes, stops, aims, freezes or moves balls."
);

#[derive(Clone, Debug)]
struct MotionState {
    s: ChangeMotionSettings,
    amount: Amount,
    angle: Amount,
}

/// The first place `who` names for `firing`.
fn place(cx: &StageCx, who: &Who, firing: &Firing) -> Option<Vec2> {
    targets(cx.world, who, firing).first().map(|&t| place_of(cx.world, t))
}

impl BlockSim for ChangeMotion {
    fn kind(&self) -> &'static str {
        "do.motion"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = ChangeMotionSettings::from_params(params);
        let amount = Amount::read(params, "amount", s.amount as f64);
        let angle = Amount::read(params, "angle", s.angle as f64);
        Ok(Box::new(MotionState { s, amount, angle }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let m = state::<MotionState>(st);
        let s = &m.s;
        let amount = m.amount.or_fixed(firing) as f32;
        let at = || match s.to {
            PlaceFrom::TheFiringsPoint => Some(firing.point),
            PlaceFrom::AThingsPlace => place(cx, &s.place, firing),
        };
        let toward =
            (s.mode == MotionMode::Push && s.way == Way::TowardAThing).then(|| place(cx, &s.toward, firing)).flatten();
        for (thing, row) in targets(cx.world, &s.who, firing) {
            let Some(bodies) = cx.world.bodies.get(&thing) else { continue };
            let one = |motion: MotionChange| Action::Motion { thing, row, motion };
            let motion = match s.mode {
                MotionMode::AddSpeed => MotionChange::AddSpeed(amount),
                MotionMode::SetSpeed => MotionChange::SetSpeed(amount),
                MotionMode::MultiplySpeed => MotionChange::MulSpeed(amount),
                MotionMode::Stop => MotionChange::Stop,
                MotionMode::Freeze => MotionChange::Freeze(amount),
                MotionMode::AimAt => match at() {
                    Some(p) => MotionChange::AimAt(p),
                    None => continue,
                },
                MotionMode::Teleport => match at() {
                    Some(p) => MotionChange::Teleport(p),
                    None => continue,
                },
                MotionMode::Push => {
                    let way = match s.way {
                        Way::Up => Vec2::NEG_Y,
                        Way::AwayFromTheHit => {
                            if firing.normal.length_squared() > 0.0 {
                                firing.normal.normalize()
                            } else {
                                Vec2::NEG_Y
                            }
                        }
                        Way::Angle => {
                            let a = (m.angle.or_fixed(firing) as f32).to_radians();
                            Vec2::new(a.sin(), -a.cos())
                        }
                        Way::TowardAThing => {
                            let Some(p) = toward else { continue };
                            // Each ball its own way: one action a ball.
                            let rows: Vec<u32> = match row {
                                Some(r) => vec![r],
                                None => (0..bodies.len() as u32).filter(|&r| bodies.alive[r as usize]).collect(),
                            };
                            for r in rows {
                                let d = p - bodies.position[r as usize];
                                if d.length_squared() > 0.0 {
                                    out.push(Action::Motion {
                                        thing,
                                        row: Some(r),
                                        motion: MotionChange::Push(d.normalize() * amount),
                                    });
                                }
                            }
                            continue;
                        }
                    };
                    MotionChange::Push(way * amount)
                }
            };
            out.push(one(motion));
        }
    }
}
