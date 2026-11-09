//! Keyframes, pulses and rules that change the physics as it runs: worked out before each step.

use super::*;

/// The scene's tracks and pulses that change how it simulates: those on Motion and Bounce
/// modules, and those on a thing's own settings that its sim says affect the physics. `None`
/// when there are none.
pub fn physics_animation(scene: &Scene, resolved: &Resolved, sims: &Sims) -> Option<Animation> {
    let mut animation = Animation::new(scene, resolved);
    animation.retain(|thing, slot, key| match slot {
        Some(SlotId::Motion) | Some(SlotId::Bounce) => true,
        // Read by the things stepped beside the physics (the air): animated with it, never in the
        // physics fingerprint.
        Some(slot) if BESIDE_SLOTS.contains(&slot) => true,
        Some(_) => false,
        None => match (resolved.things.get(&thing).and_then(|r| sims.thing(&r.kind)), scene.things.get(&thing)) {
            (Some(s), Some(t)) => s.key_restarts_physics_here(scene, t, bpe_core::vary::base_key(key)),
            (Some(s), None) => s.key_restarts_physics(bpe_core::vary::base_key(key)),
            (None, _) => false,
        },
    });
    (!animation.is_empty()).then_some(animation)
}

/// `animation` without the keys on the cards only the things stepped beside the physics read
/// ([`BESIDE_SLOTS`]): what of it the physics itself reads. `None` when nothing is left.
pub(super) fn main_animation(animation: &Animation) -> Option<Animation> {
    let mut main = animation.clone();
    main.retain(|_, slot, _| !slot.is_some_and(|s| BESIDE_SLOTS.contains(&s)));
    (!main.is_empty()).then_some(main)
}

impl World {
    /// Writes this step's keyframed physics settings and hands them to the sims: Motion and
    /// Bounce modules are prepared again, things hear `settings_changed`. Hits never move the
    /// physics; a setting that follows sound reads it as it was at the end of the step before
    /// (the world and `log` as they are now), so the physics never waits on itself.
    pub(super) fn animate(&mut self, sims: &Sims, log: &Log) {
        let Some(mut animated) = self.animated.take() else { return };
        let time = self.time();
        // What the rules changed: the settings worked out again this step, as they were before.
        let (rule_keys, dirty) = self.rule_keys(sims, time);
        let before: Vec<Option<Value>> = rule_keys
            .iter()
            .map(|k| crate::rules::params_of(&animated.resolved, k.thing, k.slot).and_then(|p| p.get(&k.key)).cloned())
            .collect();
        if let Some(pristine) = animated.pristine.clone() {
            for k in &rule_keys {
                let was = crate::rules::params_of(&pristine, k.thing, k.slot).and_then(|p| p.get(&k.key)).cloned();
                if let (Some(was), Some(params)) =
                    (was, crate::rules::params_mut(&mut animated.resolved, k.thing, k.slot))
                {
                    params.set(&k.key, was);
                }
            }
        }
        {
            let levels = self.sound.as_deref().map(|sound| crate::LevelsAt { sound, world: self, log });
            let lit = self.light.as_deref().map(|light| crate::LightLevelsAt { light, world: self, log });
            let inputs = Inputs {
                sound: levels.as_ref().map(|l| l as &dyn bpe_core::SoundLevels),
                light: lit.as_ref().map(|l| l as &dyn bpe_core::LightLevels),
                // Health as the last Rules stage, and the actions applied since, left it.
                health: self.health_levels(),
                ..Inputs::NONE
            };
            // Decoration's pulses on hits move (a Particles thing's Rate); the physics' never do.
            let kinds = &self.kinds;
            let pulsed = |t: ThingId| kinds.get(&t).is_some_and(|k| sims.thing(k).is_some_and(|s| s.pulses_on_hits()));
            if !self.decoration.iter().any(|t| pulsed(*t)) {
                animated.animation.apply(&mut animated.resolved, time, &inputs);
            } else {
                let hits = Inputs { hits: crate::hit_strength(log, time, self.steps_per_second), ..inputs };
                animated.animation.apply_where(&mut animated.resolved, time, &inputs, |t| !pulsed(t));
                animated.animation.apply_where(&mut animated.resolved, time, &hits, pulsed);
            }
        }
        let mut destinations: Vec<(ThingId, Option<SlotId>)> =
            animated.animation.destinations().into_iter().map(|(thing, slot, _)| (thing, slot)).collect();
        if let Some(rules) = &self.rules {
            let keep = |k: &crate::rules::ChangeKey| self.is_physics(sims, k.thing, k.slot, &k.key);
            rules.apply_whole(&mut animated.resolved, time, keep);
            for (k, was) in rule_keys.iter().zip(&before) {
                let now = crate::rules::params_of(&animated.resolved, k.thing, k.slot).and_then(|p| p.get(&k.key));
                if now != was.as_ref() || dirty.contains(k) {
                    destinations.push((k.thing, k.slot));
                }
            }
            destinations.sort();
        }
        let mut done = BTreeSet::new();
        let mut prepared_afresh = BTreeSet::new();
        for (thing, slot) in destinations {
            if !done.insert((thing, slot)) {
                continue;
            }
            if self.broken(thing) {
                continue;
            }
            let Some(r) = animated.resolved.things.get(&thing) else { continue };
            match slot {
                // A rule changed the Health card: the thing's card is read again (each ball keeps
                // the health it has).
                Some(SlotId::Health) => {
                    if let Some(s) = r.slots.get(&SlotId::Health)
                        && let Some(sim) = sims.health(&s.kind)
                        && let Some(rules) = self.rules.as_mut()
                    {
                        rules.health.set_card(thing, sim.prepare(&s.params));
                    }
                }
                Some(slot) => {
                    if let Some(s) = r.slots.get(&slot)
                        && let Some(msim) = sims.module(&s.kind)
                    {
                        let prepared = self.modules.entry(thing).or_default();
                        // A card whose settings didn't move this step keeps what it has: a card
                        // varied across thousands of balls isn't prepared afresh for nothing.
                        let same = prepared.get(&slot).is_some_and(|p| p.kind == s.kind && p.params == s.params);
                        if !same {
                            prepared.insert(slot, Slotted::new(msim, &s.kind, &s.params));
                            prepared_afresh.insert((thing, Some(slot)));
                        }
                    } else if let Some(sim) = sims.thing(&r.kind) {
                        // A card no module sim steps is the thing's own to read (a Particles
                        // thing's Motion and Meets things cards, with their layers).
                        sim.settings_changed(thing, r, self);
                    }
                }
                None => {
                    if let Some(sim) = sims.thing(&r.kind) {
                        sim.settings_changed(thing, r, self);
                    }
                }
            }
        }
        if self.rules.is_some() {
            self.refresh_ball_changes(sims, &animated.resolved, &dirty, &prepared_afresh, time);
        }
        self.animated = Some(animated);
    }

    /// The whole-thing physics settings the rules change now or changed until now (to be worked
    /// out again this step), after dropping the changes that are over; and the settings that
    /// gained or lost a change since the last step.
    pub(super) fn rule_keys(
        &mut self,
        sims: &Sims,
        time: f64,
    ) -> (Vec<crate::rules::ChangeKey>, BTreeSet<crate::rules::ChangeKey>) {
        let Some(rules) = &mut self.rules else { return (Vec::new(), BTreeSet::new()) };
        rules.prune(time);
        let dirty = rules.take_dirty();
        let rules = self.rules.as_ref().expect("taken above");
        let mut keys: BTreeSet<crate::rules::ChangeKey> = rules.whole_keys().cloned().collect();
        keys.extend(dirty.iter().filter(|k| k.row.is_none()).cloned());
        let keys = keys.into_iter().filter(|k| self.is_physics(sims, k.thing, k.slot, &k.key)).collect();
        (keys, dirty)
    }

    /// Whether a thing's setting (its own, or on its card in `slot`) changes how it simulates:
    /// every Motion and Bounce setting, and a thing's own settings its sim says do.
    pub fn is_physics(&self, sims: &Sims, thing: ThingId, slot: Option<SlotId>, key: &str) -> bool {
        match slot {
            // The air and health a rule changes move the physics too (W13).
            Some(slot) => SIM_SLOTS.contains(&slot) || BESIDE_SLOTS.contains(&slot) || slot == SlotId::Health,
            None => self
                .kinds
                .get(&thing)
                .and_then(|k| sims.thing(k))
                .is_some_and(|s| s.key_restarts_physics(bpe_core::vary::base_key(key))),
        }
    }

    /// Works out again what a rule changed for single balls: each such ball's own Motion and Bounce
    /// state (for cards whose sims take a state per ball), and the things whose own settings a
    /// rule changed per ball hear `settings_changed`. Only balls whose changes came, went or move
    /// now, or all of a card's when the card was prepared afresh.
    pub(super) fn refresh_ball_changes(
        &mut self,
        sims: &Sims,
        resolved: &Resolved,
        dirty: &BTreeSet<crate::rules::ChangeKey>,
        afresh: &BTreeSet<(ThingId, Option<SlotId>)>,
        time: f64,
    ) {
        let Some(rules) = self.rules.as_ref() else { return };
        // The previous step's time: a change that moved since then is worked out again.
        let since = time - self.dt() as f64;
        // (thing, slot) -> rows to work out again.
        let mut todo: BTreeMap<(ThingId, Option<SlotId>), BTreeSet<u32>> = BTreeMap::new();
        for k in dirty.iter().filter(|k| k.row.is_some()) {
            todo.entry((k.thing, k.slot)).or_default().insert(k.row.expect("filtered"));
        }
        let mut moving: Vec<(ThingId, Option<SlotId>, u32)> = Vec::new();
        for (thing, prepared) in &self.modules {
            for (slot, s) in prepared {
                let rows = rules.ball_rows(*thing, Some(*slot));
                if rows.is_empty() && s.own.is_empty() {
                    continue;
                }
                let all = afresh.contains(&(*thing, Some(*slot))) || s.rows.as_ref().is_some_and(|r| r.changes());
                for row in rows {
                    let moves = rules.ball_keys(*thing, Some(*slot), row).iter().any(|k| rules.moving(k, since, time));
                    if all || moves {
                        moving.push((*thing, Some(*slot), row));
                    }
                }
            }
        }
        for (thing, slot, row) in moving {
            todo.entry((thing, slot)).or_default().insert(row);
        }
        // Things' own settings changed per ball.
        let mut things = BTreeSet::new();
        for ((thing, slot), _) in todo.iter().filter(|((_, s), _)| s.is_none()) {
            things.insert((*thing, *slot));
        }
        for thing in self.kinds.keys() {
            let rows = rules.ball_rows(*thing, None);
            if rows.iter().any(|&row| rules.ball_keys(*thing, None, row).iter().any(|k| rules.moving(k, since, time))) {
                things.insert((*thing, None));
            }
        }
        for ((thing, slot), rows) in todo {
            let Some(slot) = slot else { continue };
            let Some(s) = self.modules.get(&thing).and_then(|m| m.get(&slot)) else { continue };
            let Some(sim) = sims.module(&s.kind) else { continue };
            if !sim.per_ball() {
                continue;
            }
            let mut made = Vec::new();
            for row in rows {
                if !self.rules.as_ref().is_some_and(|r| !r.ball_keys(thing, Some(slot), row).is_empty()) {
                    made.push((row, None));
                    continue;
                }
                let mut params = s.params.clone();
                if let Some(r) = &s.rows {
                    let spread = self.ball_spread(sims, thing, row as usize);
                    params = r.params_at(&params, &spread);
                }
                if let Some(rules) = &self.rules {
                    rules.apply_ball(thing, Some(slot), row, &mut params, time);
                }
                made.push((row, Some(std::sync::Arc::from(sim.prepare(&params)))));
            }
            let s = self.modules.get_mut(&thing).and_then(|m| m.get_mut(&slot)).expect("found above");
            for (row, state) in made {
                match state {
                    Some(state) => {
                        s.own.insert(row, state);
                    }
                    None => {
                        s.own.remove(&row);
                    }
                }
            }
        }
        for (thing, _) in things {
            if self.broken(thing) {
                continue;
            }
            let Some(r) = resolved.things.get(&thing) else { continue };
            if let Some(sim) = sims.thing(&r.kind) {
                sim.settings_changed(thing, r, self);
            }
        }
    }

    /// Gives the balls of every card that varies across them the states for where they sit in its
    /// spreads: balls new since the last time, and every ball when a spread follows speed or hits.
    pub(super) fn refresh_rows(&mut self, sims: &Sims, log: &Log) {
        let mut todo = Vec::new();
        for (id, prepared) in &self.modules {
            let count = self.bodies.get(id).map_or(0, Bodies::len);
            for (slot, s) in prepared {
                if let Some(rows) = &s.rows
                    && let Some(from) = rows.stale_from(count)
                {
                    todo.push((*id, *slot, from, count, rows.needs_light()));
                }
            }
        }
        let time = self.time();
        for (id, slot, from, count, lit) in todo {
            let spreads: Vec<bpe_core::BallSpread> = (from..count)
                .map(|row| {
                    let mut spread = self.ball_spread(sims, id, row);
                    // A spread by Light or Shadow asks the probe, as the world is now.
                    if let (true, Some(light)) = (lit, &self.light) {
                        let b = light.ball(self, log, id, row, time);
                        (spread.light, spread.shadow) = (b.light, b.shadow);
                    }
                    spread
                })
                .collect();
            let Some(s) = self.modules.get_mut(&id).and_then(|m| m.get_mut(&slot)) else { continue };
            let Some(sim) = sims.module(&s.kind) else { continue };
            let Slotted { params, rows, .. } = s;
            if let Some(rows) = rows {
                rows.fill(sim, params, from, &spreads);
            }
        }
    }

    /// Works out every ball's pull from the lights for this step (Moths), from the world as it is
    /// now: once a step, so a crowd of moths costs one probe reading a ball a step.
    pub(super) fn work_out_pulls(&mut self, sims: &Sims, log: &Log) {
        self.pulls.clear();
        let Some(light) = self.light.clone() else { return };
        let time = self.time();
        let mut pulls = BTreeMap::new();
        for (id, prepared) in &self.modules {
            let Some(s) = prepared.get(&SlotId::Motion) else { continue };
            let Some(sim) = sims.module(&s.kind) else { continue };
            if sim.light_pull(s.state.as_ref()).is_none() {
                continue;
            }
            let Some(bodies) = self.bodies.get(id) else { continue };
            let gone = bodies.live_rows().start;
            let each = (0..bodies.len())
                .map(|row| match (row >= gone).then(|| sim.light_pull(s.state_for(row))).flatten() {
                    Some(p) if bodies.alive[row] && !bodies.held[row] => light.pull(self, log, *id, row, time, &p),
                    _ => Vec2::ZERO,
                })
                .collect();
            pulls.insert(*id, each);
        }
        self.pulls = pulls;
    }

    /// Where body row `row` of `thing` sits in the spreads of its varied settings: as its sim
    /// says, else its number of all the rows; with its speed and hits as they are now.
    pub fn ball_spread(&self, sims: &Sims, thing: ThingId, row: usize) -> bpe_core::BallSpread {
        let Some(bodies) = self.bodies.get(&thing) else { return bpe_core::BallSpread::default() };
        let own = self.kinds.get(&thing).and_then(|k| sims.thing(k)).and_then(|s| s.ball_spread(thing, self, row));
        let mut spread = own.unwrap_or_else(|| bpe_core::BallSpread {
            number: bodies.number.get(row).copied().unwrap_or(row as u32),
            count: bodies.len() as u32,
            seed: self.seed as u32,
            ..bpe_core::BallSpread::default()
        });
        spread.speed = bodies.velocity.get(row).map_or(0.0, |v| v.length());
        spread.hits = bodies.hits.get(row).copied().unwrap_or(0);
        spread.hurt = self.hurt(thing, row);
        spread
    }
}
