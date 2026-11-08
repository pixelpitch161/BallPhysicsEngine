//! Rules in the running world (spec `docs/superpowers/specs/2026-11-06-rules-design.md`, section
//! 4): the prepared rules, the Rules stage that runs after every step, the queue of actions it
//! leaves for the next step's start, and what the rules have changed so far (settings, broken
//! things, frozen balls). All of it is part of the world, so snapshots and replays keep it.

mod actions;
mod changes;
mod firing;
mod health;
mod remains;
mod shows;
mod sim;

pub use actions::{Action, CountHow, MotionChange, TextOp, WHOLE};
pub use changes::{Change, ChangeKey, How, apply_to, fold};
pub use firing::{Firing, Signal, Values};
pub use health::{Health, HealthCard, HealthChange, HealthMoment, HealthSim, Hp, Takes, aimed};
pub use remains::{BallAt, BreakShow, Remains};
pub use shows::{AS_IF_HIT, FULL_FLASH_SPEED, Kick, MOST_KICKS, SceneOp, ShownScene, next_scene};
pub use sim::{BlockSim, PrepareCx, StageCx};

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{BlockId, BlockSection, Params, Resolved, Rule, RuleId, Scene, SlotId, ThingId, Value};
use glam::Vec2;

use crate::{Event, EventKind, Log, Party, Sims, ThingState, World};

/// The most actions one rule may queue in one step; past it the rule stops for that step.
pub const MOST_ACTIONS: usize = 4096;
/// The most live balls a thing may have: splits and spawns past it are dropped.
pub const MAX_BALLS: usize = 4096;
/// The most body rows a thing may have (rows are never removed, so balls that keep splitting and
/// breaking stop splitting here).
pub const MAX_ROWS: usize = 16 * MAX_BALLS;
/// How many rounds signals go round in one stage after the first; signals sent in the last round
/// are heard at the next step's stage.
pub const MOST_ROUNDS: usize = 8;
/// The most subjects a block remembers on its own (per ball counters, Logic memories); the
/// oldest is forgotten first.
pub const MOST_REMEMBERED: usize = 4096;

/// The most changes one setting keeps at a time (see [`RulesState::add_change`]).
pub const MOST_CHANGES: usize = 64;

/// Seconds a change hidden under a Set fully in for good is kept: longer than the sound engine's
/// physics runs ahead of the block it plays (a step and a few blocks), so the sound folds the same
/// changes the physics did at its time.
const HIDDEN_KEPT: f64 = 0.1;

/// The slowest and fastest the simulation can run (Slow motion's pace).
pub const MIN_PACE: f32 = 0.01;
pub const MAX_PACE: f32 = 8.0;

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
    health: &'a Health,
    sims: &'a Sims,
    events: &'a [Event],
    step: u64,
    time: f64,
    rule: RuleId,
    broken: &'a BTreeSet<ThingId>,
    log: &'a Log,
    counters: &'a BTreeMap<String, f64>,
    ball_tags: &'a BTreeMap<(ThingId, u32), BTreeSet<String>>,
    /// Blocks that fired this step (by index), with the first firing's subject and point.
    noted: BTreeMap<usize, (Option<Party>, Vec2)>,
    actions: &'a mut Vec<Action>,
    /// Signals sent, for the next round.
    signals: &'a mut Vec<Signal>,
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
            broken: self.broken,
            log: self.log,
            counters: self.counters,
            ball_tags: self.ball_tags,
            health: self.health,
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
                        match a {
                            Action::Signal { name, firing } => self.signals.push(Signal { name, firing }),
                            a => self.actions.push(a),
                        }
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
    /// What is left of broken balls (by thing and body row) and things ([`WHOLE`]) while their
    /// Break look plays out: drawn only.
    pub(crate) remains: BTreeMap<(ThingId, u32), Remains>,
    /// Whether some block can change settings.
    changes_settings: bool,
    /// The named counters (Count), by name.
    pub(crate) counters: BTreeMap<String, f64>,
    /// The tags rules gave single balls, by thing and body row.
    pub(crate) ball_tags: BTreeMap<(ThingId, u32), BTreeSet<String>>,
    /// Signals sent in the last round of the last stage, heard in this one's first.
    carried: Vec<Signal>,
    /// Every thing's health, and this step's changes of it (rules part 4).
    pub(crate) health: Health,
    /// The camera shakes and zoom punches still going, oldest first.
    pub(crate) kicks: Vec<shows::Kick>,
    /// How fast the simulation runs (Slow motion): changes on a pace of 1, in the order made.
    pace: Vec<Change>,
    /// Seconds the simulation's own clock is behind the video's (slow motion so far).
    pub(crate) lag: f64,
    /// How many times a rule flashed each ball (thing, body row) or thing (thing, 0).
    pub(crate) flashes: BTreeMap<(ThingId, u32), u32>,
    /// The lighting scene a rule shows, when one does.
    pub(crate) scene: Option<shows::ShownScene>,
    /// What rules did to Texts' words (Set text), by Text: drawn only.
    pub(crate) texts: BTreeMap<ThingId, RuleText>,
}

/// What Set text rules did to one Text's words (Text part 2). Drawn only: nothing physical reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuleText {
    /// Words in place of the Text's own (Replace; Clear leaves them empty).
    pub words: Option<String>,
    /// Lines added after them (Add a line), in order.
    pub added: Vec<String>,
    /// The video time of each Show the next line, in order: while there are any, the Text shows line
    /// `nexts.len()` alone, from the last of them.
    pub nexts: Vec<f64>,
}

impl RuleText {
    /// The words to draw in place of `own` (the Text's own words).
    pub fn words_for(&self, own: &str) -> String {
        let mut words = self.words.clone().unwrap_or_else(|| own.to_string());
        for line in &self.added {
            if !words.is_empty() {
                words.push('\n');
            }
            words.push_str(line);
        }
        words
    }
}

impl RulesState {
    /// The scene's enabled rules prepared, and its things' health, or `None` when it has neither.
    pub fn build(scene: &Scene, resolved: &Resolved, sims: &Sims, world: &World) -> Option<RulesState> {
        let enabled: Vec<&Rule> = scene.rules.list.iter().filter(|r| r.enabled).collect();
        let health = Health::build(resolved, world, sims);
        if enabled.is_empty() && health.is_empty() {
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
                    let cx = PrepareCx { scene, world, resolved, sims, rule: rule.id, block: b.id };
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
            remains: BTreeMap::new(),
            changes_settings,
            counters: BTreeMap::new(),
            ball_tags: BTreeMap::new(),
            carried: Vec::new(),
            health,
            kicks: Vec::new(),
            pace: Vec::new(),
            lag: 0.0,
            flashes: BTreeMap::new(),
            scene: None,
            texts: BTreeMap::new(),
        })
    }

    /// A test flight's copy (`World::lone`): no rules, health held as it is now. `None` without
    /// health.
    pub(crate) fn lone(&self) -> Option<RulesState> {
        if self.health.is_empty() {
            return None;
        }
        Some(RulesState {
            rules: Vec::new(),
            pending: Vec::new(),
            changes: BTreeMap::new(),
            dirty: BTreeSet::new(),
            broken: self.broken.clone(),
            frozen: Vec::new(),
            changes_settings: false,
            counters: BTreeMap::new(),
            ball_tags: BTreeMap::new(),
            carried: Vec::new(),
            health: self.health.held(),
            remains: BTreeMap::new(),
            kicks: Vec::new(),
            pace: Vec::new(),
            lag: 0.0,
            flashes: BTreeMap::new(),
            scene: None,
            texts: BTreeMap::new(),
        })
    }

    /// The Rules stage: first what this step's hits take from things' health, then every enabled
    /// rule without a problem, in list order, reads this step's `events` (and changes of health)
    /// and queues its actions; signals sent go round up to [`MOST_ROUNDS`] more times
    /// (the last round's wait for the next step); what fired is logged as `Rule` events.
    pub(crate) fn run(&mut self, world: &World, sims: &Sims, events: &[Event], log: &mut Log) {
        let step = world.step;
        let time = world.time();
        // A ball gone (caught, faded, removed) keeps no tags.
        let gone =
            |&(t, r): &(ThingId, u32)| !world.bodies.get(&t).is_some_and(|b| b.alive.get(r as usize) == Some(&true));
        if self.ball_tags.keys().any(gone) {
            self.ball_tags.retain(|k, _| !gone(k));
        }
        // Read, not changed, while the rules run (their changes are actions): lent out, not copied.
        let broken = std::mem::take(&mut self.broken);
        let counters = std::mem::take(&mut self.counters);
        let ball_tags = std::mem::take(&mut self.ball_tags);
        self.health.fill(world, sims);
        self.health.damage(world, events, &broken, &mut self.pending);
        let count = self.rules.len();
        let mut noted: Vec<BTreeMap<usize, (Option<Party>, Vec2)>> = vec![BTreeMap::new(); count];
        let mut budget = vec![MOST_ACTIONS; count];
        let mut busy = vec![false; count];
        let mut heard = std::mem::take(&mut self.carried);
        let seen: &Log = log;
        for round in 0..=MOST_ROUNDS {
            if round > 0 && heard.is_empty() {
                break;
            }
            let mut sent = Vec::new();
            for (ri, r) in self.rules.iter_mut().enumerate() {
                if r.problem.is_some() || busy[ri] {
                    continue;
                }
                let mut run = Run {
                    world,
                    sims,
                    events,
                    step,
                    time,
                    rule: r.id,
                    broken: &broken,
                    log: seen,
                    counters: &counters,
                    ball_tags: &ball_tags,
                    health: &self.health,
                    noted: std::mem::take(&mut noted[ri]),
                    actions: &mut self.pending,
                    signals: &mut sent,
                    budget: budget[ri],
                    busy: false,
                };
                for pass in 0..3 {
                    // 0: Logic ticks, 1: When blocks fire (both once a step, in round 0), 2: When
                    // blocks hear the signals sent in the round before.
                    if pass < 2 && round > 0 {
                        continue;
                    }
                    if pass == 2 && heard.is_empty() {
                        continue;
                    }
                    let section = if pass == 0 { BlockSection::Logic } else { BlockSection::When };
                    for i in 0..r.blocks.len() {
                        if r.blocks[i].section != section || run.busy {
                            continue;
                        }
                        let Some(sim) = sims.block(r.blocks[i].kind) else { continue };
                        let mut out = Vec::new();
                        let cx = run.cx(r.blocks[i].id);
                        let inputs = r.blocks[i].inputs;
                        match pass {
                            0 => sim.tick(r.blocks[i].state.as_mut(), inputs, &cx, &mut out),
                            1 => sim.fire(r.blocks[i].state.as_mut(), &cx, &mut out),
                            _ => sim.hear(r.blocks[i].state.as_mut(), &heard, &cx, &mut out),
                        }
                        for f in out {
                            run.note(i, &f);
                            run.deliver(&mut r.blocks, &r.outputs, i, &f);
                        }
                    }
                }
                if run.busy {
                    r.busy_at = Some(step);
                    busy[ri] = true;
                }
                budget[ri] = run.budget;
                noted[ri] = std::mem::take(&mut run.noted);
            }
            heard = sent;
        }
        (self.broken, self.counters, self.ball_tags) = (broken, counters, ball_tags);
        // Signals sent in the last round wait for the next step.
        self.carried = heard;
        let mut fired = Vec::new();
        for (ri, r) in self.rules.iter().enumerate() {
            for (i, (subject, point)) in std::mem::take(&mut noted[ri]) {
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
        // This stage's changes of health were read.
        self.health.moments.clear();
    }

    /// Every thing's health.
    pub fn health(&self) -> &Health {
        &self.health
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

    /// What is left of body row `row` of `thing` (`None`: the whole thing) while its Break look
    /// plays out.
    pub fn remains_of(&self, thing: ThingId, row: Option<u32>) -> Option<&Remains> {
        self.remains.get(&(thing, row.unwrap_or(WHOLE)))
    }

    /// Everything still showing its Break look, by thing and body row ([`WHOLE`] for a thing).
    pub fn remains(&self) -> impl Iterator<Item = (ThingId, u32, &Remains)> {
        self.remains.iter().map(|((t, r), m)| (*t, *r, m))
    }

    /// Whether anything of `thing` (itself, or a ball of it) still shows its Break look at `time`.
    pub fn showing(&self, thing: ThingId, time: f64) -> bool {
        self.remains.range((thing, 0)..=(thing, WHOLE)).any(|(_, r)| r.shows_at(time))
    }

    /// Forgets the remains whose look is over at `time`.
    pub(crate) fn prune_remains(&mut self, time: f64) {
        self.remains.retain(|_, r| r.shows_at(time));
    }

    /// The actions queued for the next step.
    pub fn pending(&self) -> &[Action] {
        &self.pending
    }

    // --- Spectacle (part 6) ---------------------------------------------------------------------

    /// The camera shakes and zoom punches still going, oldest first.
    pub fn kicks(&self) -> &[shows::Kick] {
        &self.kicks
    }

    /// How fast the simulation runs at `time` (video seconds): 1 unless a rule slowed it.
    pub fn pace_at(&self, time: f64) -> f32 {
        if self.pace.is_empty() {
            return 1.0;
        }
        match fold(&self.pace, &Value::Number(1.0), time) {
            Value::Number(x) if x.is_finite() => (x as f32).clamp(MIN_PACE, MAX_PACE),
            _ => 1.0,
        }
    }

    /// Seconds the simulation's clock is behind the video's.
    pub fn lag(&self) -> f64 {
        self.lag
    }

    /// How many times a rule flashed body row `item` of `thing` (item 0 for a thing without
    /// balls).
    pub fn flashes(&self, thing: ThingId, item: u32) -> u32 {
        self.flashes.get(&(thing, item)).copied().unwrap_or(0)
    }

    /// Whether a rule ever flashed something of `thing`.
    pub fn flashed(&self, thing: ThingId) -> bool {
        self.flashes.range((thing, 0)..=(thing, u32::MAX)).next().is_some()
    }

    /// The lighting scene a rule shows, when one does.
    pub fn light_scene(&self) -> Option<&shows::ShownScene> {
        self.scene.as_ref()
    }

    /// Adds a pace change; a Set with no glide that lasts for good replaces what was there.
    pub(crate) fn add_pace(&mut self, change: Change) {
        if change.covers() && change.glide <= 0.0 {
            self.pace.clear();
        }
        self.pace.push(change);
        if self.pace.len() > MOST_CHANGES {
            self.pace.remove(0);
        }
    }

    /// Drops the kicks and pace changes that are over at step `step` (`time` seconds).
    pub(crate) fn prune_shows(&mut self, step: u64, rate: u32, time: f64) {
        let _ = step;
        self.kicks.retain(|k| k.age(time, rate).is_some());
        self.pace.retain(|c| !c.done(time));
        if let Some(last) = self.pace.iter().rposition(|c| c.covers() && time >= c.start + c.glide.max(0.0)) {
            self.pace.drain(..last);
        }
    }

    // --- Changes ------------------------------------------------------------------------------

    /// Adds a running change. A Set with no glide that lasts for good hides what was there from
    /// its start on; [`RulesState::prune`] drops what it hides a little later, as the sound engine,
    /// which reads the changes just behind the physics, may still be before it (W13).
    pub(crate) fn add_change(&mut self, key: ChangeKey, change: Change) {
        let now = change.start;
        let list = self.changes.entry(key.clone()).or_default();
        list.push(change);
        // A setting keeps at most MOST_CHANGES: changes fully in for good that add (or multiply)
        // one after another become one, else the oldest goes.
        while list.len() > MOST_CHANGES {
            let pair = (0..list.len() - 1).find(|&i| {
                let (a, b) = (&list[i], &list[i + 1]);
                a.how == b.how && a.how != How::Set && a.settled(now) && b.settled(now) && a.merge(b).is_some()
            });
            match pair {
                Some(i) => {
                    let merged = list[i].merge(&list[i + 1]).expect("checked");
                    list[i] = merged;
                    list.remove(i + 1);
                }
                None => {
                    list.remove(0);
                }
            }
        }
        self.dirty.insert(key);
    }

    /// Drops the changes that are over at `time` (noting their settings), and those hidden for
    /// good under a Set that is fully in.
    pub(crate) fn prune(&mut self, time: f64) {
        let mut ended = Vec::new();
        for (key, list) in self.changes.iter_mut() {
            let before = list.len();
            list.retain(|c| !c.done(time));
            // A Set fully in for good gives its value whatever came before it (a weight of 1), so
            // keeping what it hides changes no setting; it goes once the sound has passed it too.
            let hidden = |c: &Change| c.covers() && time >= c.start + c.glide.max(0.0) + HIDDEN_KEPT;
            if let Some(last) = list.iter().rposition(hidden) {
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

    /// Forgets every change of body row `row` of `thing` alone (the ball broke).
    pub(crate) fn forget_row(&mut self, thing: ThingId, row: u32) {
        let keys: Vec<ChangeKey> = self.row_changes_any_slot(thing, row);
        for k in keys {
            self.changes.remove(&k);
            self.dirty.insert(k);
        }
        self.ball_tags.remove(&(thing, row));
    }

    /// Gives body row `to` of `thing` every change, tag and health row `from` has of its own (a ball split
    /// off it).
    pub(crate) fn copy_row(&mut self, thing: ThingId, from: u32, to: u32) {
        let copies: Vec<(ChangeKey, Vec<Change>)> = self
            .changes
            .iter()
            .filter(|(k, _)| k.thing == thing && k.row == Some(from))
            .map(|(k, l)| (ChangeKey { row: Some(to), ..k.clone() }, l.clone()))
            .collect();
        for (k, l) in copies {
            self.dirty.insert(k.clone());
            self.changes.insert(k, l);
        }
        if let Some(tags) = self.ball_tags.get(&(thing, from)).cloned() {
            self.ball_tags.insert((thing, to), tags);
        }
        self.health.copy_row(thing, from, to);
    }

    /// A counter's value (0 for one never set).
    pub fn counter(&self, name: &str) -> f64 {
        self.counters.get(name).copied().unwrap_or(0.0)
    }

    /// What Set text rules did to Text `thing`'s words so far, when any did.
    pub fn text(&self, thing: ThingId) -> Option<&RuleText> {
        self.texts.get(&thing)
    }

    /// Every counter set so far, by name (what the Rules panel lists).
    pub fn counters(&self) -> &BTreeMap<String, f64> {
        &self.counters
    }

    /// The tags rules gave body row `row` of `thing` of its own.
    pub fn ball_tags(&self, thing: ThingId, row: u32) -> Option<&BTreeSet<String>> {
        self.ball_tags.get(&(thing, row))
    }

    fn row_changes_any_slot(&self, thing: ThingId, row: u32) -> Vec<ChangeKey> {
        self.changes.keys().filter(|k| k.thing == thing && k.row == Some(row)).cloned().collect()
    }

    /// Forgets every change on `thing` (it broke).
    pub(crate) fn forget(&mut self, thing: ThingId) {
        self.changes.retain(|k, _| k.thing != thing);
        self.dirty.retain(|k| k.thing != thing);
        self.ball_tags.retain(|(t, _), _| *t != thing);
    }

    /// The settings with a change for the whole thing, in key order.
    pub fn whole_keys(&self) -> impl Iterator<Item = &ChangeKey> {
        self.changes.keys().filter(|k| k.row.is_none())
    }

    /// Whether a change on `key` moved between `since` and `time` (see [`Change::moving`]).
    pub fn moving(&self, key: &ChangeKey, since: f64, time: f64) -> bool {
        self.changes.get(key).is_some_and(|l| l.iter().any(|c| c.moving(since, time)))
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
        let from = ChangeKey { thing, slot, row: Some(0), key: String::new() };
        self.changes
            .range(from..)
            .take_while(|(k, _)| k.thing == thing && k.slot == slot)
            .filter_map(|(k, _)| k.row)
            .collect()
    }

    /// Body row `row`'s own changes on `slot`, by setting (keys sort by thing, slot, row, key).
    fn row_changes(
        &self,
        thing: ThingId,
        slot: Option<SlotId>,
        row: u32,
    ) -> impl Iterator<Item = (&ChangeKey, &Vec<Change>)> {
        let from = ChangeKey { thing, slot, row: Some(row), key: String::new() };
        self.changes.range(from..).take_while(move |(k, _)| k.thing == thing && k.slot == slot && k.row == Some(row))
    }

    /// The settings body row `row` of `thing` has changes of its own on, on `slot`.
    pub fn ball_keys(&self, thing: ThingId, slot: Option<SlotId>, row: u32) -> Vec<ChangeKey> {
        self.row_changes(thing, slot, row).map(|(k, _)| k.clone()).collect()
    }

    /// Whether some ball of `thing` has a change of its own on `slot`.
    pub fn any_ball_changes(&self, thing: ThingId, slot: Option<SlotId>) -> bool {
        let from = ChangeKey { thing, slot, row: Some(0), key: String::new() };
        self.changes.range(from..).next().is_some_and(|(k, _)| k.thing == thing && k.slot == slot)
    }

    /// Body row `row`'s own changes on `slot` written into `params` at `time`.
    pub fn apply_ball(&self, thing: ThingId, slot: Option<SlotId>, row: u32, params: &mut Params, time: f64) {
        let keyed = self.row_changes(thing, slot, row).map(|(k, l)| (k.key.as_str(), l.as_slice()));
        apply_to(params, keyed, time);
    }

    /// Body row `row`'s own changes on `slot` as text, the same for two balls changed alike
    /// (what the draw shares a ball's own stack by).
    pub fn ball_words(&self, thing: ThingId, slot: Option<SlotId>, row: u32, time: f64) -> String {
        let mut out = String::new();
        for (k, l) in self.row_changes(thing, slot, row) {
            let v: Vec<String> = l.iter().map(|c| format!("{:?}{:?}{}", c.how, c.value, c.weight(time))).collect();
            out += &format!("{}={};", k.key, v.join(","));
        }
        out
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

    /// `thing`'s changes for the whole thing on `slot`, by setting key, in key order (the sound
    /// engine folds those on its Sound and Mixer cards once a block). Never allocates.
    pub fn whole_changes(&self, thing: ThingId, slot: Option<SlotId>) -> impl Iterator<Item = (&str, &[Change])> {
        let from = ChangeKey { thing, slot, row: None, key: String::new() };
        self.changes
            .range(from..)
            .take_while(move |(k, _)| k.thing == thing && k.slot == slot && k.row.is_none())
            .map(|(k, l)| (k.key.as_str(), l.as_slice()))
    }

    /// Whether `thing` has a change for the whole thing on `slot`.
    pub fn has_whole_changes(&self, thing: ThingId, slot: Option<SlotId>) -> bool {
        self.whole_changes(thing, slot).next().is_some()
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
