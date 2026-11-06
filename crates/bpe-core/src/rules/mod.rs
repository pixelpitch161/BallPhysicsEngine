//! Rules: an optional, independent section of a scene that controls what happens in the
//! simulation (spec `docs/superpowers/specs/2026-11-06-rules-design.md`). A rule is a lane of
//! blocks: When blocks listen for something that happens, If blocks check, Logic blocks join
//! several inputs and Do blocks change the running simulation. Rules never change the saved scene.
//!
//! This module holds the saved data, the edits and the structure checks; the runtime runs the
//! rules and the kit gives the block kinds.

use crate::{EditError, KindId, Params, Registry, Scene, SettingDef, ThingId, Value, ValueType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One rule in a scene. Never reused within a scene, even after the rule is removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(pub u32);

/// One block in a rule. Never reused within its rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockId(pub u32);

fn is_true(v: &bool) -> bool {
    *v
}

/// Every rule of a scene, in the order they run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    pub list: Vec<Rule>,
    /// The id the next added rule gets.
    pub next_rule: u32,
}

/// One lane: its blocks and how they link.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rule {
    pub id: RuleId,
    pub name: String,
    #[serde(skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// In the order they were added (the editor's layout reads it).
    pub blocks: Vec<Block>,
    /// The id the next added block gets.
    pub next_block: u32,
}

impl Default for Rule {
    fn default() -> Self {
        Rule { id: RuleId(0), name: String::new(), enabled: true, blocks: Vec::new(), next_block: 0 }
    }
}

/// One block of a rule: its kind, what feeds it and its settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Block {
    pub id: BlockId,
    /// The block kind's plugin id: `"when.hit"`, `"if.compare"`, `"logic"`, `"do.change"`.
    pub kind: KindId,
    /// The blocks that feed it: none for a When, one for an If or a Do, two or more for a Logic
    /// block (in stacked order).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<BlockId>,
    /// Values that differ from the kind's defaults.
    #[serde(skip_serializing_if = "Params::is_empty")]
    pub params: Params,
}

/// Which of the four sections a block kind belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockSection {
    /// Listens for something that happens and starts a firing.
    When,
    /// Lets a firing through or stops it.
    If,
    /// Joins several inputs.
    Logic,
    /// Changes the simulation.
    Do,
}

impl BlockSection {
    pub fn name(self) -> &'static str {
        match self {
            BlockSection::When => "When",
            BlockSection::If => "If",
            BlockSection::Logic => "Logic",
            BlockSection::Do => "Do",
        }
    }
}

/// Who a block points at (spec section 3.2). In a When block it says which things count; in an
/// If or Do block it says which things to check or change, often relative to the firing.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Who {
    /// The ball (or thing) the firing is about.
    #[default]
    That,
    /// The other side of the firing: what it hit.
    Other,
    /// Every ball of the subject's thing.
    Group,
    /// One thing by id (the scene settings, the camera and lights too); for a group, optionally
    /// one ball, by its number counted from 1.
    Thing { thing: ThingId, ball: Option<u32> },
    /// Every thing with this tag.
    Tag(String),
    /// Every thing of this kind.
    Kind(KindId),
    /// Every thing.
    Anything,
}

impl Who {
    /// The one thing this names by id, which must be in the scene.
    pub fn names_thing(&self) -> Option<ThingId> {
        match self {
            Who::Thing { thing, .. } => Some(*thing),
            _ => None,
        }
    }
}

/// What a block (or a rule) can't do, so the rule is skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleProblemKind {
    /// A block of a kind this version doesn't know.
    UnknownKind(KindId),
    /// A When block with inputs.
    WhenWithInputs,
    /// An If or Do block without exactly one input.
    NeedsOneInput,
    /// A Logic block with fewer than two inputs.
    LogicNeedsTwo,
    /// An input names a block that isn't in the rule.
    MissingInput(BlockId),
    /// The links go round in a circle.
    Loop,
    /// A block points at a thing that isn't in the scene.
    MissingThing(ThingId),
    /// Anything else, in words (a setting that isn't there, from the runtime).
    Other(String),
}

/// A problem with one rule, at one block when it is about one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleProblem {
    pub block: Option<BlockId>,
    pub what: RuleProblemKind,
}

impl RuleProblem {
    /// What is wrong, in plain words for the header's hover.
    pub fn words(&self) -> String {
        match &self.what {
            RuleProblemKind::UnknownKind(k) => format!("This version of Ball Studio doesn't know the block \"{k}\""),
            RuleProblemKind::WhenWithInputs => "A When block can't have anything leading into it".into(),
            RuleProblemKind::NeedsOneInput => "A block here needs one block leading into it".into(),
            RuleProblemKind::LogicNeedsTwo => "A Logic block needs at least two blocks leading into it".into(),
            RuleProblemKind::MissingInput(_) => "A link leads from a block that isn't there any more".into(),
            RuleProblemKind::Loop => "The links go round in a circle".into(),
            RuleProblemKind::MissingThing(_) => "A block points at a thing that isn't in the scene any more".into(),
            RuleProblemKind::Other(w) => w.clone(),
        }
    }
}

/// A kind of rule block, such as "When Hit" or "Change setting": its settings and how it shows.
/// What it does in the simulation is the runtime's `BlockSim`.
pub trait RuleBlockKind: Send + Sync + 'static {
    /// Unique among every kind, and written into saved scenes: `"when.hit"`.
    fn id(&self) -> &'static str;
    /// What the editor calls one of these: `"Hit"`.
    fn name(&self) -> &'static str;
    /// One plain sentence for the hover help.
    fn help(&self) -> &'static str;
    fn section(&self) -> BlockSection;
    /// Every setting one of these has, in the order the inspector shows them.
    fn settings(&self) -> &'static [SettingDef];
    /// Settings a block of this kind has beyond `settings()` because of its own values. `get`
    /// reads the block's values by key, defaults included.
    fn dynamic_defs(&self, _get: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
        &[]
    }
    /// A few words for the block's face: "Ball 1 hits breakable".
    fn summary(&self, _get: &dyn Fn(&str) -> Option<Value>) -> String {
        self.name().to_owned()
    }
    /// The values a When block's firings carry, by name ("strength", "speed"…), for the editor's
    /// "from the firing" menus. None for the other sections.
    fn values(&self) -> &'static [&'static str] {
        &[]
    }
    /// The number settings that can come from the firing: their `"<key>~from"` is read.
    fn firing_keys(&self) -> &'static [&'static str] {
        &[]
    }
}

// --- Values from the firing -------------------------------------------------------------------

/// The suffix of a number setting's "from the firing" value name.
pub const FROM: &str = "~from";
/// The suffix of its factor.
pub const TIMES: &str = "~times";
/// The suffix of its offset.
pub const PLUS: &str = "~plus";

/// Which part of "from the firing" a key names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FromPart {
    /// The firing's value to read (Text: its name; empty for the fixed number).
    From,
    /// What it is multiplied by.
    Times,
    /// What is added after.
    Plus,
}

/// `"<key>~from"`.
pub fn from_key(key: &str) -> String {
    format!("{key}{FROM}")
}

/// `"<key>~times"`.
pub fn times_key(key: &str) -> String {
    format!("{key}{TIMES}")
}

/// `"<key>~plus"`.
pub fn plus_key(key: &str) -> String {
    format!("{key}{PLUS}")
}

/// The number setting a from-key belongs to, and which part it is.
pub fn split_from(key: &str) -> Option<(&str, FromPart)> {
    [(FROM, FromPart::From), (TIMES, FromPart::Times), (PLUS, FromPart::Plus)]
        .into_iter()
        .find_map(|(suffix, part)| key.strip_suffix(suffix).map(|base| (base, part)))
}

/// Whether `key` is a from-key.
pub fn is_from_key(key: &str) -> bool {
    split_from(key).is_some()
}

const FROM_DEF: SettingDef = SettingDef {
    key: "from",
    label: "From the firing",
    help: "Which value of the firing this number comes from; empty for the number as set.",
    ty: ValueType::Text,
    default: Value::Text(String::new()),
    range: None,
    span: None,
    unit: "",
    group: "Rule",
    depth: crate::Depth::More,
    flags: crate::Flags { keyframe: false, pulse: false, hold: false, area: crate::Area::No },
    when: "",
};

const TIMES_DEF: SettingDef = SettingDef {
    key: "times",
    label: "Times",
    help: "What the firing's value is multiplied by.",
    ty: ValueType::Number,
    default: Value::Number(1.0),
    range: None,
    span: Some((-10.0, 10.0)),
    unit: "",
    group: "Rule",
    depth: crate::Depth::More,
    flags: crate::Flags { keyframe: false, pulse: false, hold: false, area: crate::Area::No },
    when: "",
};

const PLUS_DEF: SettingDef = SettingDef {
    key: "plus",
    label: "Plus",
    help: "What is added after multiplying.",
    ty: ValueType::Number,
    default: Value::Number(0.0),
    range: None,
    span: Some((-1000.0, 1000.0)),
    unit: "",
    group: "Rule",
    depth: crate::Depth::More,
    flags: crate::Flags { keyframe: false, pulse: false, hold: false, area: crate::Area::No },
    when: "",
};

/// The description of `part` of a number setting `base` that can come from the firing.
pub fn from_def(base: Option<&'static SettingDef>, part: FromPart) -> Option<&'static SettingDef> {
    let base = base?;
    if !matches!(base.ty, ValueType::Number | ValueType::Int) {
        return None;
    }
    static DEFS: [SettingDef; 3] = [FROM_DEF, TIMES_DEF, PLUS_DEF];
    Some(match part {
        FromPart::From => &DEFS[0],
        FromPart::Times => &DEFS[1],
        FromPart::Plus => &DEFS[2],
    })
}

/// The description of a block of `kind`'s setting `key`: one of its kind's settings, one its own
/// values add, or a from-the-firing part of a number setting. `get` reads the block's values.
pub fn block_def(
    kind: &dyn RuleBlockKind,
    get: &dyn Fn(&str) -> Option<Value>,
    key: &str,
) -> Option<&'static SettingDef> {
    if let Some((base, part)) = split_from(key) {
        return from_def(block_def(kind, get, base), part);
    }
    let fixed = kind.settings();
    if let Some(d) = fixed.iter().find(|d| d.key == key) {
        return Some(d);
    }
    let with_defaults = |k: &str| get(k).or_else(|| fixed.iter().find(|d| d.key == k).map(|d| d.default.clone()));
    kind.dynamic_defs(&with_defaults).iter().find(|d| d.key == key)
}

// --- Rules and blocks -------------------------------------------------------------------------

impl Rules {
    /// Whether the scene has no rules at all.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Whether some rule is switched on.
    pub fn any_enabled(&self) -> bool {
        self.list.iter().any(|r| r.enabled)
    }

    pub fn rule(&self, id: RuleId) -> Option<&Rule> {
        self.list.iter().find(|r| r.id == id)
    }

    pub fn rule_mut(&mut self, id: RuleId) -> Option<&mut Rule> {
        self.list.iter_mut().find(|r| r.id == id)
    }

    /// The id a new rule gets: past every id given out.
    fn take_id(&mut self) -> RuleId {
        let id = self.next_rule.max(self.list.iter().map(|r| r.id.0 + 1).max().unwrap_or(0));
        self.next_rule = id + 1;
        RuleId(id)
    }
}

impl Rule {
    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn block_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    fn index(&self, id: BlockId) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// The id a new block gets: past every id given out.
    fn take_id(&mut self) -> BlockId {
        let id = self.next_block.max(self.blocks.iter().map(|b| b.id.0 + 1).max().unwrap_or(0));
        self.next_block = id + 1;
        BlockId(id)
    }

    /// The blocks `id` leads into, in block order.
    pub fn outputs(&self, id: BlockId) -> Vec<BlockId> {
        self.blocks.iter().filter(|b| b.inputs.contains(&id)).map(|b| b.id).collect()
    }

    /// The blocks in an order where every block comes after its inputs (block order among
    /// equals); `None` when the links go round in a circle. Inputs that aren't in the rule are
    /// left out.
    pub fn in_flow_order(&self) -> Option<Vec<BlockId>> {
        let ids: BTreeSet<BlockId> = self.blocks.iter().map(|b| b.id).collect();
        let mut waiting: BTreeMap<BlockId, usize> = self
            .blocks
            .iter()
            .map(|b| (b.id, b.inputs.iter().filter(|i| ids.contains(i)).collect::<BTreeSet<_>>().len()))
            .collect();
        let mut out = Vec::with_capacity(self.blocks.len());
        let mut done = BTreeSet::new();
        while out.len() < self.blocks.len() {
            let next = self.blocks.iter().find(|b| !done.contains(&b.id) && waiting[&b.id] == 0)?;
            done.insert(next.id);
            out.push(next.id);
            for b in &self.blocks {
                if b.inputs.contains(&next.id) {
                    *waiting.get_mut(&b.id).expect("every block is listed") -= 1;
                }
            }
        }
        Some(out)
    }

    /// Whether linking `from` into `to` would close a circle.
    fn would_loop(&self, from: BlockId, to: BlockId) -> bool {
        // A path from `to` to `from` already, through outputs.
        let mut todo = vec![to];
        let mut seen = BTreeSet::new();
        while let Some(at) = todo.pop() {
            if at == from {
                return true;
            }
            if seen.insert(at) {
                todo.extend(self.outputs(at));
            }
        }
        false
    }

    /// What is wrong with the rule's blocks and links, as the registry knows their kinds.
    pub fn problems(&self, reg: &Registry) -> Vec<RuleProblem> {
        let mut out = Vec::new();
        let ids: BTreeSet<BlockId> = self.blocks.iter().map(|b| b.id).collect();
        for b in &self.blocks {
            let at = |what| RuleProblem { block: Some(b.id), what };
            for i in &b.inputs {
                if !ids.contains(i) {
                    out.push(at(RuleProblemKind::MissingInput(*i)));
                }
            }
            let Some(kind) = reg.block(&b.kind) else {
                out.push(at(RuleProblemKind::UnknownKind(b.kind.clone())));
                continue;
            };
            let n = b.inputs.len();
            match kind.section() {
                BlockSection::When if n > 0 => out.push(at(RuleProblemKind::WhenWithInputs)),
                BlockSection::If | BlockSection::Do if n != 1 => out.push(at(RuleProblemKind::NeedsOneInput)),
                BlockSection::Logic if n < 2 => out.push(at(RuleProblemKind::LogicNeedsTwo)),
                _ => {}
            }
        }
        if self.in_flow_order().is_none() {
            out.push(RuleProblem { block: None, what: RuleProblemKind::Loop });
        }
        out
    }
}

/// A rule set aside by Copy rule.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedRule(pub Rule);

/// A block set aside by Copy block: its kind and settings.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedBlock {
    pub kind: KindId,
    pub params: Params,
}

impl Scene {
    /// Every problem with rule `id`: its structure, and things it names that aren't in the scene.
    pub fn rule_problems(&self, reg: &Registry, id: RuleId) -> Vec<RuleProblem> {
        let Some(rule) = self.rules.rule(id) else { return Vec::new() };
        let mut out = rule.problems(reg);
        for b in &rule.blocks {
            for (_, v) in b.params.iter() {
                if let Value::Who(who) = v
                    && let Some(t) = who.names_thing()
                    && !self.things.contains_key(&t)
                {
                    out.push(RuleProblem { block: Some(b.id), what: RuleProblemKind::MissingThing(t) });
                }
            }
        }
        out
    }

    fn rule_or_err(&mut self, id: RuleId) -> Result<&mut Rule, EditError> {
        self.rules.rule_mut(id).ok_or(EditError::NoRule(id))
    }

    /// Adds an empty rule at the end of the list.
    pub fn add_rule(&mut self, name: &str) -> RuleId {
        let id = self.rules.take_id();
        self.rules.list.push(Rule { id, name: name.to_owned(), ..Rule::default() });
        id
    }

    /// Removes a rule and every block in it.
    pub fn remove_rule(&mut self, id: RuleId) -> Result<(), EditError> {
        let at = self.rules.list.iter().position(|r| r.id == id).ok_or(EditError::NoRule(id))?;
        self.rules.list.remove(at);
        Ok(())
    }

    /// Moves a rule to `to` in the list (clamped to the list).
    pub fn move_rule(&mut self, id: RuleId, to: usize) -> Result<(), EditError> {
        let at = self.rules.list.iter().position(|r| r.id == id).ok_or(EditError::NoRule(id))?;
        let rule = self.rules.list.remove(at);
        let to = to.min(self.rules.list.len());
        self.rules.list.insert(to, rule);
        Ok(())
    }

    /// A copy of a rule, right after it, named "<name> copy".
    pub fn duplicate_rule(&mut self, id: RuleId) -> Result<RuleId, EditError> {
        let at = self.rules.list.iter().position(|r| r.id == id).ok_or(EditError::NoRule(id))?;
        let mut copy = self.rules.list[at].clone();
        copy.id = self.rules.take_id();
        copy.name = format!("{} copy", copy.name);
        let new = copy.id;
        self.rules.list.insert(at + 1, copy);
        Ok(new)
    }

    pub fn rename_rule(&mut self, id: RuleId, name: &str) -> Result<(), EditError> {
        self.rule_or_err(id)?.name = name.to_owned();
        Ok(())
    }

    pub fn set_rule_enabled(&mut self, id: RuleId, on: bool) -> Result<(), EditError> {
        self.rule_or_err(id)?.enabled = on;
        Ok(())
    }

    /// Adds a block of `kind` to rule `rule`. A When block starts the lane (stacked under the
    /// other When blocks) and ignores `after`; any other block takes `after` as its input.
    pub fn add_block(
        &mut self,
        reg: &Registry,
        rule: RuleId,
        kind: &str,
        after: Option<BlockId>,
    ) -> Result<BlockId, EditError> {
        let k = reg.block(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        let r = self.rule_or_err(rule)?;
        let inputs = match (k.section(), after) {
            (BlockSection::When, _) | (_, None) => Vec::new(),
            (_, Some(a)) => {
                r.block(a).ok_or(EditError::NoBlock(a))?;
                vec![a]
            }
        };
        let id = r.take_id();
        r.blocks.push(Block { id, kind: kind.to_owned(), inputs, params: Params::default() });
        Ok(id)
    }

    /// Adds a When block under the others ("+ When").
    pub fn add_when(&mut self, reg: &Registry, rule: RuleId, kind: &str) -> Result<BlockId, EditError> {
        if reg.block(kind).map(|k| k.section()) != Some(BlockSection::When) {
            return Err(EditError::WrongType);
        }
        self.add_block(reg, rule, kind, None)
    }

    /// Removes a block. The links close up: a block it led into that had it as its only input
    /// takes its inputs instead when that leaves the right number; a Logic block keeps its other
    /// inputs and takes this one's in its place.
    pub fn remove_block(&mut self, reg: &Registry, rule: RuleId, id: BlockId) -> Result<(), EditError> {
        let r = self.rule_or_err(rule)?;
        let at = r.index(id).ok_or(EditError::NoBlock(id))?;
        let gone = r.blocks.remove(at);
        for b in &mut r.blocks {
            let Some(place) = b.inputs.iter().position(|i| *i == id) else { continue };
            b.inputs.remove(place);
            let section = reg.block(&b.kind).map(|k| k.section());
            let fits = match section {
                Some(BlockSection::Logic) => true,
                _ => b.inputs.is_empty() && gone.inputs.len() == 1,
            };
            if fits {
                for (k, i) in gone.inputs.iter().enumerate() {
                    if !b.inputs.contains(i) {
                        b.inputs.insert((place + k).min(b.inputs.len()), *i);
                    }
                }
            }
        }
        Ok(())
    }

    /// Links `from` into `to`: onto a Logic block it adds an input (at the end); onto an If or Do
    /// block it replaces the input. A When block takes no input; a link that would go round in a
    /// circle is refused.
    pub fn link(&mut self, reg: &Registry, rule: RuleId, from: BlockId, to: BlockId) -> Result<(), EditError> {
        let r = self.rule_or_err(rule)?;
        r.block(from).ok_or(EditError::NoBlock(from))?;
        let kind = r.block(to).ok_or(EditError::NoBlock(to))?.kind.clone();
        let section = reg.block(&kind).map(|k| k.section()).ok_or(EditError::UnknownKind(kind))?;
        if section == BlockSection::When {
            return Err(EditError::WrongType);
        }
        if from == to || r.would_loop(from, to) {
            return Err(EditError::RuleLoop);
        }
        let b = r.block_mut(to).expect("found above");
        match section {
            BlockSection::When => return Err(EditError::WrongType),
            BlockSection::Logic => {
                if !b.inputs.contains(&from) {
                    b.inputs.push(from);
                }
            }
            BlockSection::If | BlockSection::Do => b.inputs = vec![from],
        }
        Ok(())
    }

    /// Takes the link from `from` into `to` away.
    pub fn unlink(&mut self, rule: RuleId, from: BlockId, to: BlockId) -> Result<(), EditError> {
        let b = self.rule_or_err(rule)?.block_mut(to).ok_or(EditError::NoBlock(to))?;
        b.inputs.retain(|i| *i != from);
        Ok(())
    }

    pub fn copy_rule(&self, id: RuleId) -> Option<CopiedRule> {
        self.rules.rule(id).cloned().map(CopiedRule)
    }

    /// Puts a copied rule at the end of the list, with an id of its own.
    pub fn paste_rule(&mut self, copied: &CopiedRule) -> RuleId {
        let mut rule = copied.0.clone();
        rule.id = self.rules.take_id();
        // Pasted beside a rule of the same name: "<name> copy".
        if self.rules.list.iter().any(|r| r.name == rule.name) {
            rule.name = format!("{} copy", rule.name);
        }
        let id = rule.id;
        self.rules.list.push(rule);
        id
    }

    pub fn copy_block(&self, rule: RuleId, id: BlockId) -> Option<CopiedBlock> {
        let b = self.rules.rule(rule)?.block(id)?;
        Some(CopiedBlock { kind: b.kind.clone(), params: b.params.clone() })
    }

    /// Puts a copied block in a rule after `after` (a When block starts the lane).
    pub fn paste_block(
        &mut self,
        reg: &Registry,
        rule: RuleId,
        copied: &CopiedBlock,
        after: Option<BlockId>,
    ) -> Result<BlockId, EditError> {
        let id = self.add_block(reg, rule, &copied.kind, after)?;
        let b = self.rules.rule_mut(rule).and_then(|r| r.block_mut(id)).expect("just added");
        b.params = copied.params.clone();
        Ok(id)
    }

    /// The description of a block's setting `key`.
    pub fn block_def_at(&self, reg: &Registry, rule: RuleId, id: BlockId, key: &str) -> Option<&'static SettingDef> {
        let b = self.rules.rule(rule)?.block(id)?;
        let kind = reg.block(&b.kind)?;
        let get = |k: &str| b.params.get(k).cloned();
        block_def(kind, &get, key)
    }

    /// Sets a block's setting, pulled into range; a value equal to the default is removed.
    pub fn set_block_param(
        &mut self,
        reg: &Registry,
        rule: RuleId,
        id: BlockId,
        key: &str,
        v: Value,
    ) -> Result<(), EditError> {
        self.rules.rule(rule).ok_or(EditError::NoRule(rule))?.block(id).ok_or(EditError::NoBlock(id))?;
        let def = self.block_def_at(reg, rule, id, key).ok_or_else(|| EditError::UnknownSetting(key.into()))?;
        if !def.ty.accepts(&v) {
            return Err(EditError::WrongType);
        }
        let v = def.clamp(v);
        let b = self.rules.rule_mut(rule).and_then(|r| r.block_mut(id)).expect("checked above");
        if def.default == v {
            b.params.remove(key);
        } else {
            b.params.set(key, v);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
