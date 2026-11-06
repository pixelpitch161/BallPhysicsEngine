//! Rules in the running world (spec `docs/superpowers/specs/2026-11-06-rules-design.md`, section
//! 4): the prepared rules, the Rules stage that runs after every step, the queue of actions it
//! leaves for the next step's start, and what the rules have changed so far (settings, broken
//! things, frozen balls). All of it is part of the world, so snapshots and replays keep it.

mod actions;
mod changes;
mod firing;
mod sim;

pub use actions::{Action, MotionChange, WHOLE};
pub use changes::{Change, ChangeKey, How, apply_to, fold};
pub use firing::{Firing, Values};
pub use sim::{BlockSim, PrepareCx, StageCx};

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{BlockId, BlockSection, Params, Resolved, Rule, RuleId, Scene, SlotId, ThingId, Value};
use glam::Vec2;

use crate::{Event, EventKind, Log, Party, Sims, ThingState, World};

/// The most actions one rule may queue in one step; past it the rule stops for that step.
pub const MOST_ACTIONS: usize = 4096;
/// The most balls a thing may have (splits and spawns past it are dropped, from part 5).
pub const MAX_BALLS: usize = 4096;
/// The most subjects a block remembers on its own (per ball counters, Logic memories); the
/// oldest is forgotten first.
pub const MOST_REMEMBERED: usize = 4096;

/// A number from 0 to 1 for a block, a step and a subject: a hash of the scene's seed with them,
/// never a shared random stream, so adding a rule never changes another rule's dice.
pub fn dice(seed: u64, rule: RuleId, block: BlockId, step: u64, subject: Option<Party>, salt: u32) -> f32 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut feed = |x: u64| {
        for b in x.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    feed(rule.0 as u64);
    feed(block.0 as u64);
    feed(step);
    let (t, i) = subject.map_or((u64::MAX, u64::MAX), |p| (p.thing.0 as u64, p.item as u64));
    feed(t);
    feed(i);
    feed(salt as u64);
    // A final mix so close inputs land far apart.
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// One prepared block.
#[derive(Clone)]
struct RunBlock {
    id: BlockId,
    kind: &'static str,
    section: BlockSection,
    inputs: usize,
    state: Box<dyn ThingState>,
}

/// One prepared rule.
#[derive(Clone)]
struct RunRule {
    id: RuleId,
    blocks: Vec<RunBlock>,
    /// Per block: the blocks it leads into (index) and on which of their inputs, in block order.
    outputs: Vec<Vec<(usize, usize)>>,
    /// What is wrong with it, when something is: it is then skipped.
    problem: Option<String>,
    /// The last step it queued as many actions as it may.
    busy_at: Option<u64>,
}

/// A rule's per-step run: what fired and what it queued.
struct Run<'a> {
    world: &'a World,
    sims: &'a Sims,
    events: &'a [Event],
    step: u64,
    time: f64,
    rule: RuleId,
    /// Blocks that fired this step (by index), with the first firing's subject and point.
    noted: BTreeMap<usize, (Option<Party>, Vec2)>,
    actions: &'a mut Vec<Action>,
    budget: usize,
    busy: bool,
}

impl Run<'_> {
    fn cx(&self, block: BlockId) -> StageCx<'_> {
        StageCx {
            world: self.world,
            sims: self.sims,
            events: self.events,
            step: self.step,
            time: self.time,
            rule: self.rule,
            block,
        }
    }

    fn note(&mut self, block: usize, f: &Firing) {
        self.noted.entry(block).or_insert((f.subject, f.point));
    }

    /// Sends `firing` from block `from` on to every block it leads into, depth first.
    fn deliver(&mut self, blocks: &mut [RunBlock], outputs: &[Vec<(usize, usize)>], from: usize, firing: &Firing) {
        for &(next, input) in &outputs[from] {
            if self.busy {
                return;
            }
            let Some(sim) = self.sims.block(blocks[next].kind) else { continue };
            let (id, section, inputs) = (blocks[next].id, blocks[next].section, blocks[next].inputs);
            match section {
                BlockSection::When => {}
                BlockSection::If => {
                    let cx = self.cx(id);
                    if sim.pass(blocks[next].state.as_mut(), firing, &cx) {
                        self.note(next, firing);
                        self.deliver(blocks, outputs, next, firing);
                    }
                }
                BlockSection::Logic => {
                    let mut out = Vec::new();
                    let cx = self.cx(id);
                    sim.join(blocks[next].state.as_mut(), input, inputs, firing, &cx, &mut out);
                    for f in out {
                        self.note(next, &f);
                        self.deliver(blocks, outputs, next, &f);
                    }
                }
                BlockSection::Do => {
                    let mut out = Vec::new();
                    let cx = self.cx(id);
                    sim.act(blocks[next].state.as_mut(), firing, &cx, &mut out);
                    self.note(next, firing);
                    for a in out {
                        if self.budget == 0 {
                            self.busy = true;
                            return;
                        }
                        self.budget -= 1;
                        self.actions.push(a);
                    }
                }
            }
        }
    }
}

/// Everything about the scene's rules the world keeps while it runs.
#[derive(Clone)]
pub struct RulesState {
    rules: Vec<RunRule>,
    /// Actions queued in the last Rules stage, for the next step's start.
    pub(crate) pending: Vec<Action>,
    /// Every running setting change, in the order made per setting.
    changes: BTreeMap<ChangeKey, Vec<Change>>,
    /// Settings that gained or lost a change since the world last worked them out.
    dirty: BTreeSet<ChangeKey>,
    /// Things a rule broke.
    pub(crate) broken: BTreeSet<ThingId>,
    /// Balls a rule holds still.
    pub(crate) frozen: Vec<actions::Frozen>,
    /// Whether some block can change settings.
    changes_settings: bool,
}

impl RulesState {
    /// The scene's enabled rules prepared, or `None` when it has none.
    pub fn build(scene: &Scene, resolved: &Resolved, sims: &Sims) -> Option<RulesState> {
        let enabled: Vec<&Rule> = scene.rules.list.iter().filter(|r| r.enabled).collect();
        if enabled.is_empty() {
            return None;
        }
        let mut rules = Vec::with_capacity(enabled.len());
        let mut changes_settings = false;
        for rule in enabled {
            let mut run = RunRule {
                id: rule.id,
                blocks: Vec::new(),
                outputs: Vec::new(),
                problem: structure_problem(scene, rule, sims),
                busy_at: None,
            };
            if run.problem.is_none() {
                for b in &rule.blocks {
                    let sim = sims.block(&b.kind).expect("checked by structure_problem");
                    let cx = PrepareCx { scene, resolved, sims, rule: rule.id, block: b.id };
                    match sim.prepare(&b.params, &cx) {
                        Ok(state) => {
                            changes_settings |= sim.changes_settings(state.as_ref());
                            run.blocks.push(RunBlock {
                                id: b.id,
                                kind: sim.kind(),
                                section: sim.section(),
                                inputs: b.inputs.len(),
                                state,
                            });
                        }
                        Err(why) => {
                            run.problem = Some(why);
                            break;
                        }
                    }
                }
            }
            if run.problem.is_some() {
                run.blocks.clear();
            } else {
                let index: BTreeMap<BlockId, usize> = rule.blocks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
                run.outputs = vec![Vec::new(); rule.blocks.len()];
                for (to, b) in rule.blocks.iter().enumerate() {
                    for (input, from) in b.inputs.iter().enumerate() {
                        run.outputs[index[from]].push((to, input));
                    }
                }
            }
            rules.push(run);
        }
        Some(RulesState {
            rules,
            pending: Vec::new(),
            changes: BTreeMap::new(),
            dirty: BTreeSet::new(),
            broken: BTreeSet::new(),
            frozen: Vec::new(),
            changes_settings,
        })
    }

    /// The Rules stage: every enabled rule without a problem, in list order, reads this step's
    /// `events` and queues its actions; what fired is logged as `Rule` events.
    pub(crate) fn run(&mut self, world: &World, sims: &Sims, events: &[Event], log: &mut Log) {
        let step = world.step;
        let time = world.time();
        let mut fired = Vec::new();
        for r in &mut self.rules {
            if r.problem.is_some() {
                continue;
            }
            let mut run = Run {
                world,
                sims,
                events,
                step,
                time,
                rule: r.id,
                noted: BTreeMap::new(),
                actions: &mut self.pending,
                budget: MOST_ACTIONS,
                busy: false,
            };
            for section in [BlockSection::Logic, BlockSection::When] {
                for i in 0..r.blocks.len() {
                    if r.blocks[i].section != section || run.busy {
                        continue;
                    }
                    let Some(sim) = sims.block(r.blocks[i].kind) else { continue };
                    let mut out = Vec::new();
                    let cx = run.cx(r.blocks[i].id);
                    let inputs = r.blocks[i].inputs;
                    match section {
                        BlockSection::Logic => sim.tick(r.blocks[i].state.as_mut(), inputs, &cx, &mut out),
                        _ => sim.fire(r.blocks[i].state.as_mut(), &cx, &mut out),
                    }
                    for f in out {
                        run.note(i, &f);
                        run.deliver(&mut r.blocks, &r.outputs, i, &f);
                    }
                }
            }
            if run.busy {
                r.busy_at = Some(step);
            }
            for (i, (subject, point)) in std::mem::take(&mut run.noted) {
                let block = r.blocks[i].id;
                fired.push(Event {
                    step,
                    kind: EventKind::Rule,
                    a: subject.unwrap_or(Party { thing: ThingId::ROOT, item: 0 }),
                    b: Some(Party { thing: ThingId::ROOT, item: rule_item(r.id, block) }),
                    point,
                    normal: Vec2::ZERO,
                    speed: 0.0,
                    at: 0.0,
                });
            }
        }
        for e in fired {
            log.push(e);
        }
    }

    /// What is wrong with rule `id`, when it is skipped for it.
    pub fn problem(&self, id: RuleId) -> Option<&str> {
        self.rules.iter().find(|r| r.id == id)?.problem.as_deref()
    }

    /// The last step rule `id` queued as many actions as it may ("too busy").
    pub fn busy_at(&self, id: RuleId) -> Option<u64> {
        self.rules.iter().find(|r| r.id == id)?.busy_at
    }

    /// Whether some block can change settings.
    pub fn changes_settings(&self) -> bool {
        self.changes_settings
    }

    /// Whether a rule broke `thing`.
    pub fn is_broken(&self, thing: ThingId) -> bool {
        self.broken.contains(&thing)
    }

    /// Every thing a rule broke, in id order.
    pub fn broken(&self) -> impl Iterator<Item = ThingId> + '_ {
        self.broken.iter().copied()
    }

    /// Whether the rules have changed anything the picture shows: a setting or a broken thing.
    pub fn changes_picture(&self) -> bool {
        !self.changes.is_empty() || !self.broken.is_empty()
    }

    /// The actions queued for the next step.
    pub fn pending(&self) -> &[Action] {
        &self.pending
    }

    // --- Changes ------------------------------------------------------------------------------

    /// Adds a running change; a Set with no glide that lasts for good replaces what was there.
    pub(crate) fn add_change(&mut self, key: ChangeKey, change: Change) {
        let list = self.changes.entry(key.clone()).or_default();
        if change.covers() && change.glide <= 0.0 {
            list.clear();
        }
        list.push(change);
        self.dirty.insert(key);
    }

    /// Drops the changes that are over at `time` (noting their settings), and those hidden for
    /// good under a Set that is fully in.
    pub(crate) fn prune(&mut self, time: f64) {
        let mut ended = Vec::new();
        for (key, list) in self.changes.iter_mut() {
            let before = list.len();
            list.retain(|c| !c.done(time));
            if let Some(last) = list.iter().rposition(|c| c.covers() && time >= c.start + c.glide.max(0.0) as f64) {
                list.drain(..last);
            }
            if list.len() != before && list.is_empty() {
                ended.push(key.clone());
            }
        }
        for key in ended {
            self.changes.remove(&key);
            self.dirty.insert(key);
        }
    }

    /// Takes the settings that gained or lost a change since the last time.
    pub(crate) fn take_dirty(&mut self) -> BTreeSet<ChangeKey> {
        std::mem::take(&mut self.dirty)
    }

    /// Forgets every change on `thing` (it broke).
    pub(crate) fn forget(&mut self, thing: ThingId) {
        self.changes.retain(|k, _| k.thing != thing);
        self.dirty.retain(|k| k.thing != thing);
    }

    /// The settings with a change for the whole thing, in key order.
    pub fn whole_keys(&self) -> impl Iterator<Item = &ChangeKey> {
        self.changes.keys().filter(|k| k.row.is_none())
    }

    /// Whether a change on `key` moves at `time` (gliding in or back).
    pub fn moving(&self, key: &ChangeKey, time: f64) -> bool {
        self.changes.get(key).is_some_and(|l| l.iter().any(|c| c.moving(time)))
    }

    /// Writes every whole-thing change `keep` says yes to into `resolved` at `time`, on top of
    /// what is there.
    pub fn apply_whole(&self, resolved: &mut Resolved, time: f64, keep: impl Fn(&ChangeKey) -> bool) {
        for (key, list) in self.changes.iter().filter(|(k, _)| k.row.is_none() && keep(k)) {
            let Some(params) = params_mut(resolved, key.thing, key.slot) else { continue };
            let Some(base) = params.get(&key.key) else { continue };
            let v = fold(list, base, time);
            params.set(&key.key, v);
        }
    }

    /// The body rows of `thing` with a change of their own on `slot` (the thing's own settings
    /// for `None`), in row order.
    pub fn ball_rows(&self, thing: ThingId, slot: Option<SlotId>) -> BTreeSet<u32> {
        self.changes.keys().filter(|k| k.thing == thing && k.slot == slot).filter_map(|k| k.row).collect()
    }

    /// The settings body row `row` of `thing` has changes of its own on, on `slot`.
    pub fn ball_keys(&self, thing: ThingId, slot: Option<SlotId>, row: u32) -> Vec<ChangeKey> {
        self.changes.keys().filter(|k| k.thing == thing && k.slot == slot && k.row == Some(row)).cloned().collect()
    }

    /// Whether any ball of any thing has a change of its own on `slot`.
    pub fn any_ball_changes(&self, thing: ThingId, slot: Option<SlotId>) -> bool {
        self.changes.keys().any(|k| k.thing == thing && k.slot == slot && k.row.is_some())
    }

    /// Body row `row`'s own changes on `slot` written into `params` at `time`.
    pub fn apply_ball(&self, thing: ThingId, slot: Option<SlotId>, row: u32, params: &mut Params, time: f64) {
        let keyed = self
            .changes
            .iter()
            .filter(|(k, _)| k.thing == thing && k.slot == slot && k.row == Some(row))
            .map(|(k, l)| (k.key.as_str(), l.as_slice()));
        apply_to(params, keyed, time);
    }

    /// The balls' own changes on one setting, for a sim to fold over each ball's own value.
    pub fn ruled(&self, thing: ThingId, slot: Option<SlotId>, key: &str, time: f64) -> Option<Ruled> {
        let rows: BTreeMap<u32, Vec<Change>> = self
            .changes
            .iter()
            .filter(|(k, _)| k.thing == thing && k.slot == slot && k.key == key)
            .filter_map(|(k, l)| Some((k.row?, l.clone())))
            .collect();
        (!rows.is_empty()).then_some(Ruled { time, rows })
    }

    /// Every change on `key` (for tests and the editor).
    pub fn changes_on(&self, key: &ChangeKey) -> &[Change] {
        self.changes.get(key).map_or(&[], Vec::as_slice)
    }
}

/// One setting's changes per ball, at one moment: see [`RulesState::ruled`].
#[derive(Clone, Debug, PartialEq)]
pub struct Ruled {
    pub time: f64,
    pub rows: BTreeMap<u32, Vec<Change>>,
}

impl Ruled {
    /// Body row `row`'s value of the setting, its own `base` with its changes on top.
    pub fn on(&self, row: usize, base: Value) -> Value {
        match self.rows.get(&(row as u32)) {
            Some(list) => fold(list, &base, self.time),
            None => base,
        }
    }

    /// [`Ruled::on`] for a number.
    pub fn number(&self, row: usize, base: f32) -> f32 {
        match self.on(row, Value::Number(base as f64)) {
            Value::Number(x) => x as f32,
            _ => base,
        }
    }
}

/// The params of `thing`'s own settings (`slot` none) or of its card in `slot`.
pub(crate) fn params_mut(resolved: &mut Resolved, thing: ThingId, slot: Option<SlotId>) -> Option<&mut Params> {
    let r = resolved.things.get_mut(&thing)?;
    match slot {
        None => Some(&mut r.params),
        Some(slot) => r.slots.get_mut(&slot).map(|s| &mut s.params),
    }
}

/// The params of `thing`'s own settings or of its card in `slot`.
pub(crate) fn params_of(resolved: &Resolved, thing: ThingId, slot: Option<SlotId>) -> Option<&Params> {
    let r = resolved.things.get(&thing)?;
    match slot {
        None => Some(&r.params),
        Some(slot) => r.slots.get(&slot).map(|s| &s.params),
    }
}

/// A `Rule` event's item for block `block` of rule `rule`: `rule × 65536 + block`.
pub fn rule_item(rule: RuleId, block: BlockId) -> u32 {
    rule.0.wrapping_mul(65536).wrapping_add(block.0 & 0xffff)
}

/// What is wrong with how a rule's blocks fit together, as the sims know their kinds: a kind
/// without a sim, a wrong number of inputs, a link from a block that isn't there, a circle, or a
/// thing named that isn't in the scene.
fn structure_problem(scene: &Scene, rule: &Rule, sims: &Sims) -> Option<String> {
    let ids: BTreeSet<BlockId> = rule.blocks.iter().map(|b| b.id).collect();
    for b in &rule.blocks {
        let Some(sim) = sims.block(&b.kind) else {
            return Some(format!("This version of Ball Studio doesn't know the block \"{}\"", b.kind));
        };
        if b.inputs.iter().any(|i| !ids.contains(i)) {
            return Some("A link leads from a block that isn't there any more".into());
        }
        let n = b.inputs.len();
        let fits = match sim.section() {
            BlockSection::When => n == 0,
            BlockSection::If | BlockSection::Do => n == 1,
            BlockSection::Logic => n >= 2,
        };
        if !fits {
            return Some(match sim.section() {
                BlockSection::When => "A When block can't have anything leading into it".into(),
                BlockSection::Logic => "A Logic block needs at least two blocks leading into it".into(),
                _ => "A block here needs one block leading into it".into(),
            });
        }
        for (_, v) in b.params.iter() {
            if let Value::Who(who) = v
                && let Some(t) = who.names_thing()
                && !scene.things.contains_key(&t)
            {
                return Some("A block points at a thing that isn't in the scene any more".into());
            }
        }
    }
    if rule.in_flow_order().is_none() {
        return Some("The links go round in a circle".into());
    }
    None
}

#[cfg(test)]
mod tests;
