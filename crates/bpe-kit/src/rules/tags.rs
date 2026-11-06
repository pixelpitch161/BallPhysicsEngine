//! Do: Tag. A tag on one ball counts for rules (a Who of Tag finds it); a tag on a whole thing
//! counts for rules and for the physics too (a Bounce card's special target by tag).

use bpe_core::{BlockSection, Choice, Params, Settings, Who};
use bpe_runtime::{Action, BlockSim, Firing, PrepareCx, StageCx, ThingState, state};

use super::block_kind;
use super::who::targets;

/// Whether Tag adds or takes away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TagHow {
    #[default]
    Add,
    Remove,
}

/// Adds or removes a tag.
pub struct Tag;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct TagSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Do", simple, group = "Rule")]
    pub how: TagHow,
    #[setting(label = "Tag", simple, group = "Rule")]
    pub tag: String,
}

impl Default for TagSettings {
    fn default() -> Self {
        TagSettings { who: Who::That, how: TagHow::Add, tag: "hit".into() }
    }
}

block_kind!(
    Tag,
    "do.tag",
    "Tag",
    Do,
    TagSettings,
    "Adds a tag to a ball or a thing, or takes one away, so other rules can pick it out."
);

#[derive(Clone, Debug)]
struct TagState {
    who: Who,
    add: bool,
    tag: String,
}

impl BlockSim for Tag {
    fn kind(&self) -> &'static str {
        "do.tag"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = TagSettings::from_params(params);
        let tag = s.tag.trim().to_owned();
        if tag.is_empty() {
            return Err("Tag needs a tag".into());
        }
        Ok(Box::new(TagState { who: s.who, add: s.how == TagHow::Add, tag }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<TagState>(st);
        for (thing, row) in targets(cx, &s.who, firing) {
            if thing == bpe_core::ThingId::ROOT {
                continue;
            }
            out.push(Action::Tag { thing, row, tag: s.tag.clone(), add: s.add });
        }
    }
}
