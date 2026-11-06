//! Do: Change setting. Changes any setting of a thing, for the whole thing or (where the setting
//! can differ from ball to ball) for one ball, on top of what its keyframes give.

use std::collections::BTreeMap;

use bpe_core::{
    Address, BlockSection, Choice, Color, Ease, Gradient, Params, Settings, SlotId, ThingId, Value, ValueType, VaryUse,
    Who,
};
use bpe_runtime::{Action, BlockSim, Change, ChangeKey, Firing, How, PrepareCx, StageCx, ThingState, state};
use glam::Vec2;

use super::amount::Amount;
use super::block_kind;
use super::who::targets;

/// Which of a thing's settings a change is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CardChoice {
    /// The thing's own settings.
    #[default]
    Own,
    Look,
    Motion,
    Bounce,
    /// The scene's Effects.
    Shader,
    /// The scene's Lighting card.
    Lighting,
}

impl CardChoice {
    pub fn slot(self) -> Option<SlotId> {
        match self {
            CardChoice::Own => None,
            CardChoice::Look => Some(SlotId::Look),
            CardChoice::Motion => Some(SlotId::Motion),
            CardChoice::Bounce => Some(SlotId::Bounce),
            CardChoice::Shader => Some(SlotId::Shader),
            CardChoice::Lighting => Some(SlotId::Lighting),
        }
    }
}

/// How the change combines with the setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HowChoice {
    #[default]
    Set,
    Add,
    Multiply,
}

/// How a change glides there and back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GlideEase {
    #[default]
    Smooth,
    Linear,
    EaseIn,
    EaseOut,
    Overshoot,
    Bounce,
}

impl GlideEase {
    pub fn ease(self) -> Ease {
        match self {
            GlideEase::Smooth => Ease::Smooth,
            GlideEase::Linear => Ease::Linear,
            GlideEase::EaseIn => Ease::EaseIn,
            GlideEase::EaseOut => Ease::EaseOut,
            GlideEase::Overshoot => Ease::Overshoot,
            GlideEase::Bounce => Ease::Bounce,
        }
    }
}

/// How long a change lasts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Lasts {
    #[default]
    ForGood,
    /// A while, then it glides back.
    ForATime,
}

pub struct ChangeSetting;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ChangeSettingSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Card", simple, group = "Rule")]
    pub card: CardChoice,
    /// The setting's key (a layer's keeps its layer: "3/colour").
    #[setting(label = "Setting", simple, group = "Rule")]
    pub setting: String,
    #[setting(label = "How", simple, group = "Rule")]
    pub how: HowChoice,
    #[setting(label = "To", span(0, 2000), group = "Rule")]
    pub to_number: f32,
    #[setting(label = "To", group = "Rule")]
    pub to_pair: Vec2,
    #[setting(label = "To", group = "Rule")]
    pub to_colour: Color,
    #[setting(label = "To", group = "Rule")]
    pub to_gradient: Gradient,
    #[setting(label = "To", group = "Rule")]
    pub to_switch: bool,
    #[setting(label = "To", group = "Rule")]
    pub to_choice: String,
    #[setting(label = "Times", when = "how=multiply", span(0, 4), group = "Rule")]
    pub times: f32,
    /// Seconds to glide there, and back when it ends.
    #[setting(label = "Glide", range(0, 1e9), span(0, 5), unit = "s", group = "Rule")]
    pub glide: f64,
    #[setting(label = "Ease", when = "glide", group = "Rule")]
    pub ease: GlideEase,
    #[setting(label = "Lasts", group = "Rule")]
    pub lasts: Lasts,
    #[setting(label = "For", when = "lasts=for_a_time", range(0, 1e9), span(0, 10), unit = "s", group = "Rule")]
    pub seconds: f64,
}

impl Default for ChangeSettingSettings {
    fn default() -> Self {
        ChangeSettingSettings {
            who: Who::That,
            card: CardChoice::Own,
            setting: String::new(),
            how: HowChoice::Set,
            to_number: 0.0,
            to_pair: Vec2::ZERO,
            to_colour: Color::WHITE,
            to_gradient: Gradient::default(),
            to_switch: true,
            to_choice: String::new(),
            times: 2.0,
            glide: 0.0,
            ease: GlideEase::Smooth,
            lasts: Lasts::ForGood,
            seconds: 1.0,
        }
    }
}

block_kind!(
    ChangeSetting,
    "do.change",
    "Change setting",
    Do,
    ChangeSettingSettings,
    "Changes any setting, for good or for a while, for a whole thing or one ball.",
    values = [],
    from = ["to_number", "times"]
);

/// What a thing's setting is, as the change needs to know.
#[derive(Clone, Debug, PartialEq)]
pub struct SettingInfo {
    pub ty: ValueType,
    pub range: Option<(f64, f64)>,
    /// One ball can have it its own way.
    pub per_ball: bool,
}

#[derive(Clone, Debug)]
struct ChangeState {
    s: ChangeSettingSettings,
    slot: Option<SlotId>,
    /// Every thing that has the setting.
    things: BTreeMap<ThingId, SettingInfo>,
    to_number: Amount,
    times: Amount,
}

/// The setting `key` on `slot` of every thing in the scene that has it.
pub fn setting_infos(cx: &PrepareCx, slot: Option<SlotId>, key: &str) -> BTreeMap<ThingId, SettingInfo> {
    let reg = super::kit_registry();
    let mut out = BTreeMap::new();
    for id in cx.scene.things.keys() {
        let Some(resolved) = cx.resolved.things.get(id) else { continue };
        let a = Address { thing: *id, slot, key: key.to_owned() };
        let Some(def) = cx.scene.def_at(reg, &a) else { continue };
        let changeable = matches!(
            def.ty,
            ValueType::Number
                | ValueType::Int
                | ValueType::Vec2
                | ValueType::Bool
                | ValueType::Color
                | ValueType::Gradient
                | ValueType::Choice(_)
        );
        if !changeable {
            continue;
        }
        let live = cx.scene.vary_use(reg, &a) == Some(VaryUse::Live);
        let per_ball = match slot {
            None | Some(SlotId::Look) => live,
            Some(SlotId::Motion | SlotId::Bounce) => {
                live && resolved
                    .slots
                    .get(&slot.expect("a card"))
                    .and_then(|s| cx.sims.module(&s.kind))
                    .is_some_and(|m| m.per_ball())
            }
            Some(_) => false,
        };
        out.insert(*id, SettingInfo { ty: def.ty, range: def.range, per_ball });
    }
    out
}

impl BlockSim for ChangeSetting {
    fn kind(&self) -> &'static str {
        "do.change"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = ChangeSettingSettings::from_params(params);
        let key = s.setting.trim().to_owned();
        if key.is_empty() {
            return Err("Change setting needs a setting to change".into());
        }
        let slot = s.card.slot();
        let things = setting_infos(cx, slot, &key);
        if let Some(t) = s.who.names_thing()
            && !things.contains_key(&t)
        {
            let name = cx.scene.things.get(&t).map(|t| t.name.as_str()).unwrap_or("That thing");
            return Err(format!("{name} has no setting \"{key}\" there"));
        }
        if things.is_empty() {
            return Err(format!("Nothing in the scene has the setting \"{key}\" there"));
        }
        let to_number = Amount::read(params, "to_number", s.to_number as f64);
        let times = Amount::read(params, "times", s.times as f64);
        Ok(Box::new(ChangeState { s: ChangeSettingSettings { setting: key, ..s }, slot, things, to_number, times }))
    }

    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let c = state::<ChangeState>(st);
        let s = &c.s;
        for (thing, row) in targets(cx, &s.who, firing) {
            let Some(info) = c.things.get(&thing) else { continue };
            let numeric = matches!(info.ty, ValueType::Number | ValueType::Int | ValueType::Vec2);
            let how = match s.how {
                HowChoice::Set => How::Set,
                HowChoice::Add if numeric => How::Add,
                HowChoice::Multiply if numeric => How::Multiply,
                _ => How::Set,
            };
            let value = match (how, info.ty) {
                (How::Multiply, _) => match c.times.of(firing) {
                    Some(m) => Value::Number(m),
                    None => continue,
                },
                (_, ValueType::Number) => match c.to_number.of(firing) {
                    Some(x) => Value::Number(x),
                    None => continue,
                },
                (_, ValueType::Int) => match c.to_number.of(firing) {
                    Some(x) => Value::Int(x.round() as i64),
                    None => continue,
                },
                (_, ValueType::Vec2) => Value::Vec2(s.to_pair.into()),
                (_, ValueType::Bool) => Value::Bool(s.to_switch),
                (_, ValueType::Color) => Value::Color(s.to_colour),
                (_, ValueType::Gradient) => Value::Gradient(s.to_gradient.clone()),
                (_, ValueType::Choice(names)) => {
                    if !names.contains(&s.to_choice.as_str()) {
                        continue;
                    }
                    Value::Choice(s.to_choice.clone())
                }
                _ => continue,
            };
            out.push(Action::Change {
                key: ChangeKey { thing, slot: c.slot, row: row.filter(|_| info.per_ball), key: s.setting.clone() },
                change: Change {
                    how,
                    value,
                    start: 0.0,
                    glide: s.glide.max(0.0),
                    ease: s.ease.ease(),
                    lasts: (s.lasts == Lasts::ForATime).then_some(s.seconds.max(0.0)),
                    range: info.range,
                },
            });
        }
    }

    fn changes_settings(&self, _state: &dyn ThingState) -> bool {
        true
    }
}
