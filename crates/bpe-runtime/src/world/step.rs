//! One step: the stages in order, substeps, moving bodies, collisions and bounces.

use super::*;

/// How far into a wall a ball that touches it with its edge may begin a substep and still count
/// as coming in from its own side (its bounce then carries on for the rest of the substep), in
/// pixels: rounding, not a push from another ball.
pub(super) const TOUCHING: f32 = 1e-3;

/// The target rules, over a world's kinds and tags.
pub(super) fn matches(
    kinds: &BTreeMap<ThingId, String>,
    tags: &BTreeMap<ThingId, BTreeSet<String>>,
    target: &Target,
    me: ThingId,
    other: ThingId,
) -> bool {
    match target {
        Target::Anything => true,
        Target::Me => other == me && kinds.contains_key(&other),
        Target::Thing(t) => other == *t && kinds.contains_key(&other),
        Target::Tag(tag) => tags.get(&other).is_some_and(|t| t.contains(tag)),
        Target::Kind(kind) => kinds.get(&other).is_some_and(|k| k == kind),
    }
}

/// Whether the mover's own Bounce module calls hitting `other` special.
pub(super) fn is_special(
    own: Option<(&dyn ModuleSim, &dyn ThingState)>,
    matcher: &dyn Fn(&Target, ThingId, ThingId) -> bool,
    mover: ThingId,
    other: ThingId,
) -> bool {
    own.and_then(|(sim, state)| sim.special_target(state)).is_some_and(|t| matcher(&t, mover, other))
}

/// A prepared Bounce module.
pub(super) type Bounce<'a> = Option<&'a Slotted>;

/// Who answers a contact: the hit thing's Bounce when it takes over, else the mover's (`true`).
pub(super) fn decide<'a>(
    sims: &'a Sims,
    hit: Bounce<'a>,
    mover: Bounce<'a>,
) -> Option<(&'a dyn ModuleSim, &'a Slotted, bool)> {
    hit.and_then(|s| {
        let sim = sims.module(&s.kind)?;
        sim.takes_over(s.state.as_ref()).then_some((sim, s, false))
    })
    .or_else(|| mover.and_then(|s| Some((sims.module(&s.kind)?, s, true))))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn respond(
    decider: Option<(&dyn ModuleSim, &dyn ThingState)>,
    contact: &crate::Contact,
    bodies: &mut Bodies,
    i: usize,
    limits: &Limits,
    special: bool,
    cx: &SimCx,
) {
    match decider {
        Some((sim, state)) => sim.respond_at(state, contact, bodies, i, limits, special, cx),
        None => bodies.velocity[i] = reflect(bodies.velocity[i], contact, 1.0),
    }
}

impl World {
    /// Steps until `time` seconds have passed.
    pub fn advance_to(&mut self, time: f64, sims: &Sims, log: &mut Log) {
        let target = step_at(time, self.steps_per_second);
        while self.step < target {
            self.step(sims, log);
        }
    }

    /// Moves every live ball back to `a` of the way through the last step (0 where it was when
    /// the step began, 1 where it is now), for drawing a moment between two steps (the bake's
    /// motion blur, lighting part 3). Draw-only: [`World::restore_between`] must put them back
    /// before the world steps again, so the physics never sees it.
    pub fn set_between(&mut self, a: f32) -> Between {
        let a = a.clamp(0.0, 1.0);
        let mut saved = Vec::with_capacity(self.bodies.len());
        for (id, bodies) in self.bodies.iter_mut() {
            saved.push((*id, bodies.position.clone()));
            for i in 0..bodies.len() {
                if bodies.alive[i] {
                    let (from, to) = (bodies.previous[i], bodies.position[i]);
                    bodies.position[i] = from + (to - from) * a;
                }
            }
        }
        Between { saved }
    }

    /// Puts the balls back where [`World::set_between`] found them.
    pub fn restore_between(&mut self, between: Between) {
        for (id, positions) in between.saved {
            if let Some(bodies) = self.bodies.get_mut(&id) {
                bodies.position = positions;
            }
        }
    }

    /// One step: every stage, in order.
    pub fn step(&mut self, sims: &Sims, log: &mut Log) {
        self.step += 1;
        let first = log.len();
        // What the rules did in the last step's Rules stage happens now, before anything moves.
        self.apply_rule_actions(sims, log);
        self.walls_before.clear();
        self.walls_before.extend(self.walls.iter().map(|(id, w)| (*id, w.outline.clone())));
        self.animate(sims, log);
        // Slow motion: the physics moves by a share of the step (the video clock runs on).
        let time = self.time();
        self.pace = self.rules.as_ref().map_or(1.0, |r| r.pace_at(time));
        let limits = self.limits();
        let dt = self.sim_dt();
        // Things take their turns in tree order: parents go first, so riders see where their
        // parent moved to.
        let order = self.order.clone();

        // Removed balls at the front of each table are skipped from here on.
        for bodies in self.bodies.values_mut() {
            bodies.note_gone();
        }
        // Spawn: things add what they release this step.
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.spawn(*id, self, log);
            }
        }
        self.refresh_rows(sims, log);
        self.work_out_pulls(sims, log);
        self.work_out_air(sims);

        // Clamp speeds, remember where everything started and find the fastest ball.
        let mut fastest: f32 = 0.0;
        for bodies in self.bodies.values_mut() {
            for i in bodies.live_rows() {
                if !bodies.alive[i] {
                    continue;
                }
                let speed = bodies.velocity[i].length();
                if speed > limits.ceiling {
                    bodies.velocity[i] *= limits.ceiling / speed;
                }
                fastest = fastest.max(speed.min(limits.ceiling));
                bodies.previous[i] = bodies.position[i];
            }
        }
        let travel = (fastest + self.gravity.length() * dt) * dt;
        let substeps = ((travel / SAFE_MOVE).ceil() as u32).clamp(1, MAX_SUBSTEPS);
        let sub_dt = dt / substeps as f32;

        // Scripted: walls spin and shrink, things follow their scripts.
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.scripted(*id, self);
            }
        }

        for k in 0..substeps {
            self.substep = (k, substeps);
            let before = log.len();
            self.substep(sims, &order, sub_dt, &limits, log);
            if !self.history.is_empty() {
                let at = (self.step - 1) as f64 + (k + 1) as f64 / substeps as f64;
                self.note_corners(&log.events()[before..], at);
            }
        }

        // Events: things log what happened to them this step, in tree order.
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.after_step(*id, self, log);
            }
        }
        for (id, history) in &mut self.history {
            if let Some(bodies) = self.bodies.get(id) {
                history.record(self.step, &bodies.position);
            }
        }
        // Rules: what happened this step, and what to do about it at the next one's start.
        if let Some(mut rules) = self.rules.take() {
            if self.pace != 1.0 {
                rules.lag += self.dt() as f64 * (1.0 - self.pace as f64);
            }
            let events = log.events()[first..].to_vec();
            rules.run(self, sims, &events, log);
            self.rules = Some(rules);
        }
    }

    /// Notes in the histories where each ball hit in `events` was at the end of the substep it
    /// hit in (the moment `at`, in steps): just touching what it hit, where it turned. (A wall
    /// hit has noted the moment it touched already, in `collide`.)
    pub(super) fn note_corners(&mut self, events: &[Event], at: f64) {
        for e in events.iter().filter(|e| e.kind == EventKind::Hit) {
            for side in std::iter::once(e.a).chain(e.b) {
                let (Some(history), Some(bodies)) = (self.history.get_mut(&side.thing), self.bodies.get(&side.thing))
                else {
                    continue;
                };
                if let Some(&place) = bodies.position.get(side.item as usize) {
                    history.corner(side.item as usize, at, place);
                }
            }
        }
    }

    pub(super) fn substep(&mut self, sims: &Sims, order: &[ThingId], dt: f32, limits: &Limits, log: &mut Log) {
        // Forces from things, before the Motion modules.
        for id in order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.forces(*id, self, dt);
            }
        }
        self.move_bodies(sims, dt);

        // Settle: things relax what they hold against each other and the walls.
        for id in order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.settle(*id, self, dt);
            }
        }
        self.collide(sims, dt, limits, log);
    }

    /// Forces from the Motion modules, then Integrate, per body thing.
    pub(super) fn move_bodies(&mut self, sims: &Sims, dt: f32) {
        let time = self.sim_clock();
        let World { gravity, bodies, modules, seed, pulls, air_at, .. } = self;
        let gravity = *gravity;
        let seed = *seed;
        for (id, bodies) in bodies.iter_mut() {
            if let Some(s) = modules.get(id).and_then(|m| m.get(&SlotId::Motion))
                && let Some(sim) = sims.module(&s.kind)
            {
                let cx = SimCx { thing: *id, time, seed };
                if s.varies() {
                    for i in bodies.live_rows() {
                        if bodies.alive[i] {
                            sim.forces_one(s.state_for(i), bodies, i, gravity, dt, &cx);
                        }
                    }
                } else {
                    sim.forces_at(s.state.as_ref(), bodies, gravity, dt, &cx);
                }
            }
            // The air (Follow the air), worked out at the step's start.
            if let Some(air) = air_at.get(id) {
                for (i, (wind, grip)) in air.iter().enumerate().take(bodies.len()) {
                    if *grip > 0.0 && bodies.alive[i] && !bodies.held[i] {
                        let take = 1.0 - (-grip * dt).exp();
                        let v = bodies.velocity[i];
                        bodies.velocity[i] = v + (*wind - v) * take;
                    }
                }
            }
            // The lights' pull (Moths), worked out at the step's start.
            if let Some(pull) = pulls.get(id) {
                for (i, p) in pull.iter().enumerate().take(bodies.len()).skip(bodies.live_rows().start) {
                    if bodies.alive[i] && !bodies.held[i] {
                        bodies.velocity[i] += *p * dt;
                    }
                }
            }
            bodies.start.copy_from_slice(&bodies.position);
            // (A removed ball below `gone_below` that was held kept no speed when it went.)
            for i in bodies.live_rows() {
                if bodies.alive[i] && !bodies.held[i] {
                    let v = bodies.velocity[i];
                    bodies.position[i] += v * dt;
                } else if bodies.held[i] {
                    // A held ball gathers no speed from the forces while it waits.
                    bodies.velocity[i] = Vec2::ZERO;
                }
            }
        }
    }

    /// Collide and Respond, then the Hit events: first the walls around the balls, then the
    /// balls against each other. What each ball touched holds it up in the pair solve.
    pub(super) fn collide(&mut self, sims: &Sims, dt: f32, limits: &Limits, log: &mut Log) {
        let time = self.sim_clock();
        let World {
            step,
            steps_per_second,
            gravity,
            bodies,
            walls,
            walls_before,
            modules,
            kinds,
            tags,
            pairs,
            seed,
            substep,
            history,
            pace,
            ..
        } = self;
        let step = *step;
        let (sub_index, sub_count) = *substep;
        let seed = *seed;
        let step_dt = 1.0 / *steps_per_second as f32 * *pace;
        let matcher = |t: &Target, me: ThingId, other: ThingId| matches(kinds, tags, t, me, other);
        let press = |thing: ThingId, row: usize| press_of(modules, sims, thing, row, *gravity, step_dt);
        let mut supports: Vec<Support> = Vec::new();
        for (mover, bodies) in bodies.iter_mut() {
            let mover_bounce = modules.get(mover).and_then(|m| m.get(&SlotId::Bounce));
            let own = mover_bounce.and_then(|s| Some((sims.module(&s.kind)?, s.state.as_ref())));
            let cx = SimCx { thing: *mover, time, seed };
            let touch = own.map_or(Touch::Edge, |(sim, state)| sim.touch(state));
            // This mover's own: a ball pulled harder than the scene's gravity presses harder. A
            // Motion that varies across the balls presses each ball its own way.
            let pressed = modules.get(mover).and_then(|m| m.get(&SlotId::Motion)).is_some_and(Slotted::varies);
            let thing_limits = Limits { press: press(*mover, 0), ..*limits };
            // A wall balls pass through bounces nothing.
            for (wall_id, wall) in walls.iter().filter(|(_, w)| !w.passes) {
                let before = walls_before.get(wall_id).unwrap_or(&wall.outline);
                let wall_bounce = modules.get(wall_id).and_then(|m| m.get(&SlotId::Bounce));
                let decider = decide(sims, wall_bounce, mover_bounce);
                let special = is_special(own, &matcher, *mover, *wall_id);
                for i in bodies.live_rows() {
                    if !bodies.alive[i] || bodies.held[i] {
                        continue;
                    }
                    let row_limits;
                    let limits = if pressed {
                        row_limits = Limits { press: press(*mover, i), ..*limits };
                        &row_limits
                    } else {
                        &thing_limits
                    };
                    // The mover's own card answers with this ball's own state.
                    let decider =
                        decider.map(|(sim, s, mine)| (sim, if mine { s.state_for(i) } else { s.state.as_ref() }));
                    let own = match mover_bounce.filter(|s| s.varies()) {
                        Some(s) => own.map(|(sim, _)| (sim, s.state_for(i))),
                        None => own,
                    };
                    let velocity = bodies.velocity[i];
                    let radius = bodies.radius[i];
                    let previous = bodies.previous[i];
                    let start = bodies.start[i];
                    let at = &mut bodies.position[i];
                    let moved_to = *at;
                    let Some(contact) = keep_clear_from(at, previous, before, velocity, radius, touch, wall) else {
                        continue;
                    };
                    // The ball was moved back to just touching: the gap to there along the
                    // normal, at the start of the substep and where it had moved to.
                    let touching = *at;
                    let gap = |p: Vec2| (p - touching).dot(contact.normal);
                    // At the start against the wall where it was then: one coming at the ball
                    // has come this far nearer since.
                    let start_gap = gap(start) + contact.wall_velocity.dot(contact.normal) * dt;
                    let within = crossing(start_gap, gap(moved_to));
                    // A ball touching with its middle bounces exactly where its centre crossed
                    // the line, off the outline's own normal there.
                    let exact = match touch {
                        Touch::Middle => middle_crossing(start, moved_to, previous, before, velocity, wall),
                        Touch::Edge => None,
                    };
                    let (mut contact, within) = exact.unwrap_or((contact, within));
                    // Where its centre was as it touched, on the way it came in.
                    let crossed = start.lerp(moved_to, within);
                    let hit = contact.speed >= limits.resting_along(contact.normal);
                    // A ball that touched on its way in goes on from there for the rest of the
                    // substep. One that began the substep already in the wall (another ball
                    // pushed it there) only bounces where it was put back, so a pile settles.
                    let carries = hit
                        && match touch {
                            Touch::Middle => exact.is_some(),
                            Touch::Edge => start_gap > -TOUCHING,
                        };
                    if carries && touch == Touch::Edge {
                        // It touched there, not where it was slid back to along the wall.
                        contact.point += crossed - touching;
                    }
                    let hit_at = step_fraction(sub_index, sub_count, within);
                    supports.push(Support {
                        thing: *mover,
                        item: i as u32,
                        normal: contact.normal,
                        wall_velocity: contact.wall_velocity,
                    });
                    respond(decider, &contact, bodies, i, limits, special, &cx);
                    // And goes on from there for the rest of the substep, along its new velocity,
                    // so a bounce loses none of the step: balls that touch a hair apart in time
                    // leave a hair apart, not a step apart. A ball resting on a wall stays put.
                    if carries {
                        let mut carried = crossed + bodies.velocity[i] * ((1.0 - within) * dt);
                        match touch {
                            // Unless that would put it on the wrong side, near a corner.
                            Touch::Middle => {
                                if carried.is_finite() && on_own_side(carried, previous, before, wall) {
                                    bodies.position[i] = carried;
                                }
                            }
                            // Kept clear of the wall again where the rest of the way would reach
                            // into it (a curve or a corner); and not across any other wall's line
                            // (one already passed this substep wouldn't stop it).
                            Touch::Edge => {
                                if carried.is_finite() {
                                    let v = bodies.velocity[i];
                                    keep_clear_from(&mut carried, previous, before, v, radius, touch, wall);
                                    let crosses = |(id, w): (&ThingId, &Wall)| {
                                        let before = walls_before.get(id).unwrap_or(&w.outline);
                                        !w.passes && !on_own_side(carried, previous, before, w)
                                    };
                                    if !walls.iter().any(crosses) {
                                        bodies.position[i] = carried;
                                    }
                                }
                            }
                        }
                    }
                    if hit {
                        bodies.hits[i] += 1;
                        // Where its centre was as it touched, on the way it came in; the end of the
                        // substep notes where it went on to (`note_corners`).
                        if let Some(h) = history.get_mut(mover) {
                            // In f64, as the end-of-substep moments are, so the two stay in order.
                            let touched = (sub_index as f64 + within as f64) / sub_count.max(1) as f64;
                            h.corner(i, (step - 1) as f64 + touched, crossed);
                        }
                        log.push(Event {
                            step,
                            kind: EventKind::Hit,
                            a: Party { thing: *mover, item: i as u32 },
                            // The letter of a Text it hit; 0 for every other wall.
                            b: Some(Party { thing: *wall_id, item: contact.part }),
                            point: contact.point,
                            normal: contact.normal,
                            speed: contact.speed,
                            at: hit_at,
                        });
                        if let Some((sim, state)) = own
                            && sim.effects(state, bodies, i, contact.speed, limits)
                        {
                            pop(bodies, i, *mover, step, log);
                        }
                    }
                }
            }
        }

        // Balls against balls.
        let substep = (sub_index, sub_count);
        collide_pairs(bodies, modules, &matcher, &press, sims, pairs, &supports, step, substep, dt, limits, log);

        // A ball touching with its middle sits a hair's breadth from the line, so a nudge from
        // another ball can carry it across: put it back on its own side (no bounce, no event).
        for (mover, bodies) in bodies.iter_mut() {
            let bounce = modules.get(mover).and_then(|m| m.get(&SlotId::Bounce));
            let touch = bounce.and_then(|s| Some(sims.module(&s.kind)?.touch(s.state.as_ref())));
            if touch != Some(Touch::Middle) {
                continue;
            }
            for (wall_id, wall) in walls.iter().filter(|(_, w)| !w.passes) {
                let before = walls_before.get(wall_id).unwrap_or(&wall.outline);
                for i in bodies.live_rows() {
                    if bodies.alive[i] {
                        let (previous, velocity) = (bodies.previous[i], bodies.velocity[i]);
                        let at = &mut bodies.position[i];
                        keep_clear_from(at, previous, before, velocity, 0.0, Touch::Middle, wall);
                    }
                }
            }
        }
    }
}
