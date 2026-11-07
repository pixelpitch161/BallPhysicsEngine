//! Do: Set text (Text part 2). Changes a Text's words: replaces them, adds a line, shows its next
//! line alone, or clears them, at the next step's start. The words may hold values in braces, read as
//! the video plays. Drawn only: nothing physical reads a Text's words.

use bpe_core::{BlockSection, Choice, KindId, Params, Settings, Who};
use bpe_runtime::{Action, BlockSim, Firing, PrepareCx, StageCx, TextOp, ThingState, state};

use super::block_kind;
use super::who::targets;
use crate::things::text::KIND as TEXT;

/// What Set text does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SetTextHow {
    /// These words in place of its own.
    #[default]
    Replace,
    /// A line after its words.
    AddALine,
    /// Shows its next line alone (lyrics a rule moves on).
    ShowTheNextLine,
    /// No words.
    Clear,
}

/// Changes a Text's words.
pub struct SetText;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SetTextSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Do", simple, group = "Rule")]
    pub how: SetTextHow,
    /// May hold values in braces, as a Text's words can ({hits}, {counter score} …).
    #[setting(label = "To words", when = "how=replace|add_a_line", simple, group = "Rule")]
    pub words: String,
}

impl Default for SetTextSettings {
    fn default() -> Self {
        SetTextSettings { who: Who::Kind(KindId::from(TEXT)), how: SetTextHow::Replace, words: "NICE!".into() }
    }
}

block_kind!(
    SetText,
    "do.text",
    "Set text",
    Do,
    SetTextSettings,
    "Changes a Text's words: new words, a line added, its next line shown alone, or none."
);

#[derive(Clone, Debug)]
struct SetTextState {
    who: Who,
    op: TextOp,
}

impl BlockSim for SetText {
    fn kind(&self) -> &'static str {
        "do.text"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = SetTextSettings::from_params(params);
        let op = match s.how {
            SetTextHow::Replace => TextOp::Replace(s.words),
            SetTextHow::AddALine => TextOp::AddLine(s.words),
            SetTextHow::ShowTheNextLine => TextOp::NextLine,
            SetTextHow::Clear => TextOp::Clear,
        };
        Ok(Box::new(SetTextState { who: s.who, op }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<SetTextState>(st);
        let mut done = Vec::new();
        for (thing, _) in targets(cx, &s.who, firing) {
            // Only Texts have words; one change a Text a firing.
            if cx.world.kind(thing) != Some(TEXT) || done.contains(&thing) {
                continue;
            }
            done.push(thing);
            out.push(Action::Text { thing, op: s.op.clone() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Scene, ThingId, Value, resolve};
    use bpe_runtime::{Log, World};

    /// A scene with a Text and a rule: When the time is 0.1 s, Set text as `how` with `words`.
    fn scene_with(how: &str, words: &str, times: &[f64]) -> (Scene, ThingId) {
        let kit = crate::kit();
        let reg = &kit.registry;
        let mut scene = Scene::empty(reg);
        let text = crate::add_thing(&mut scene, reg, TEXT, ThingId::ROOT).unwrap();
        for &t in times {
            let rule = scene.add_rule("Set");
            let when = scene.add_block(reg, rule, "when.time", None).unwrap();
            scene.set_block_param(reg, rule, when, "at", Value::Number(t)).unwrap();
            let set = scene.add_block(reg, rule, "do.text", Some(when)).unwrap();
            scene.set_block_param(reg, rule, set, "how", Value::Choice(how.into())).unwrap();
            if how == "replace" || how == "add_a_line" {
                scene.set_block_param(reg, rule, set, "words", Value::Text(words.into())).unwrap();
            }
        }
        (scene, text)
    }

    fn run(scene: &Scene, until: f64) -> World {
        let kit = crate::kit();
        let resolved = resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut world = World::build(scene, &resolved, &kit.sims);
        world.advance_to(until, &kit.sims, &mut Log::default());
        world
    }

    #[test]
    fn set_text_replaces_adds_and_moves_on() {
        let (scene, text) = scene_with("replace", "HI {hits}", &[0.1]);
        let world = run(&scene, 0.05);
        assert!(world.rules().unwrap().text(text).is_none(), "not yet");
        let world = run(&scene, 0.2);
        let t = world.rules().unwrap().text(text).unwrap();
        assert_eq!(t.words_for("OWN"), "HI {hits}");

        let (scene, text) = scene_with("add_a_line", "MORE", &[0.1, 0.2]);
        let world = run(&scene, 0.3);
        assert_eq!(world.rules().unwrap().text(text).unwrap().words_for("OWN"), "OWN\nMORE\nMORE");

        let (scene, text) = scene_with("show_the_next_line", "", &[0.1, 0.2]);
        let world = run(&scene, 0.3);
        let nexts = &world.rules().unwrap().text(text).unwrap().nexts;
        assert_eq!(nexts.len(), 2);
        assert!(nexts[0] > 0.1 && nexts[0] < 0.11 && nexts[1] > 0.2 && nexts[1] < 0.21, "{nexts:?}");

        let (scene, text) = scene_with("clear", "", &[0.1]);
        let world = run(&scene, 0.2);
        assert_eq!(world.rules().unwrap().text(text).unwrap().words_for("OWN"), "");
    }
}
