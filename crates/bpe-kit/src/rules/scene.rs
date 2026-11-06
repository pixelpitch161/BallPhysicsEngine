//! Do: Lighting scene (rules part 6). Shows one of the Lighting card's scenes, crossfading to it,
//! until a later rule picks another or hands back to the card's own Show.

use bpe_core::{BlockSection, Choice, LIGHT_SCENES_KEY, Params, Settings, SlotId, ThingId, Value};
use bpe_runtime::{Action, BlockSim, Firing, PrepareCx, SceneOp, StageCx, ThingState, state};

use super::amount::Amount;
use super::block_kind;

/// Which scene it shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShowWhich {
    /// The scene numbered.
    #[default]
    AScene,
    /// The one after the scene a rule showed last (the first after none), round again.
    NextScene,
    /// The one before it.
    PreviousScene,
    /// Any scene but the one showing.
    RandomScene,
    /// The lights' own settings, no scene.
    TheLightsOwn,
    /// The Lighting card's own Show takes over again.
    BackToTheCard,
}

/// Where the crossfade's time comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FadeFrom {
    /// The Lighting card's Crossfade.
    #[default]
    TheCards,
    ItsOwn,
}

pub struct ShowScene;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShowSceneSettings {
    #[setting(label = "Show", simple, group = "Rule")]
    pub show: ShowWhich,
    /// The scene's number, from 1.
    #[setting(label = "Scene", when = "show=a_scene", range(1, 1e9), span(1, 16), simple, group = "Rule")]
    pub scene: u32,
    #[setting(label = "Crossfade", group = "Rule")]
    pub crossfade: FadeFrom,
    #[setting(label = "Fade", when = "crossfade=its_own", range(0, 1e9), span(0, 5), unit = "s", group = "Rule")]
    pub fade: f32,
}

impl Default for ShowSceneSettings {
    fn default() -> Self {
        ShowSceneSettings { show: ShowWhich::AScene, scene: 1, crossfade: FadeFrom::TheCards, fade: 1.0 }
    }
}

block_kind!(
    ShowScene,
    "do.light_scene",
    "Lighting scene",
    Do,
    ShowSceneSettings,
    "Shows one of the Lighting card's scenes, crossfading to it."
);

#[derive(Clone, Debug)]
struct ShowState {
    s: ShowSceneSettings,
    scene: Amount,
    /// How many scenes the Lighting card has.
    count: u32,
}

/// How many lighting scenes the scene's Lighting card holds (0 without one).
fn scene_count(scene: &bpe_core::Scene) -> u32 {
    let card = scene.things.get(&ThingId::ROOT).and_then(|r| r.slots.get(&SlotId::Lighting));
    match card.and_then(|c| c.params.get(LIGHT_SCENES_KEY)) {
        Some(Value::LightScenes(list)) => list.len() as u32,
        _ => 0,
    }
}

impl BlockSim for ShowScene {
    fn kind(&self) -> &'static str {
        "do.light_scene"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = ShowSceneSettings::from_params(params);
        let scene = Amount::read(params, "scene", s.scene as f64);
        Ok(Box::new(ShowState { s, scene, count: scene_count(cx.scene) }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, _cx: &StageCx, out: &mut Vec<Action>) {
        let m = state::<ShowState>(st);
        let op = match m.s.show {
            ShowWhich::AScene => {
                let n = m.scene.or_fixed(firing).round();
                if !(n >= 1.0 && n <= m.count as f64) {
                    return;
                }
                SceneOp::Scene(n as u32)
            }
            ShowWhich::NextScene => SceneOp::Next,
            ShowWhich::PreviousScene => SceneOp::Previous,
            ShowWhich::RandomScene => SceneOp::Random,
            ShowWhich::TheLightsOwn => SceneOp::LightsOwn,
            ShowWhich::BackToTheCard => SceneOp::Card,
        };
        let fade = (m.s.crossfade == FadeFrom::ItsOwn).then_some(m.s.fade.max(0.0));
        out.push(Action::LightScene { op, count: m.count, fade });
    }
}
