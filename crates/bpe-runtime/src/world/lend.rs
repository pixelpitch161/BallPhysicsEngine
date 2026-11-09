//! Lone copies of a world and lending states and cards between worlds.

use super::*;

impl World {
    /// A copy of the world for test-firing a ball: only the things `keep` names are stepped (their
    /// states, walls, cards and keys kept; a card varied across the balls with its own values),
    /// with no history. Every other thing is left out: not
    /// stepped, its balls gone, its keys dropped, its pose left where it is now (a thing riding it
    /// stays put). Kinds, names and tags stay whole, so targets still name what they named.
    /// Decoration ([`World::decoration`]: things stepped beside the physics, or kept in it by a
    /// rule that reads them) is always left out.
    pub fn lone(&self, keep: &dyn Fn(ThingId) -> bool) -> World {
        // Decoration (things whose sim steps beside the physics) never changes a flight, even when
        // a rule keeps it in this world.
        let keep = |id: ThingId| keep(id) && !self.decoration.contains(&id);
        let keep = &keep;
        let kept = |id: &ThingId| keep(*id);
        let animated = self.animated.as_ref().and_then(|a| {
            let mut animation = a.animation.clone();
            animation.retain(|thing, _, _| keep(thing));
            // Only the kept things' resolved settings: a flight is cloned many times.
            let resolved = Resolved {
                things: a.resolved.things.iter().filter(|(id, _)| kept(id)).map(|(id, r)| (*id, r.clone())).collect(),
                problems: Vec::new(),
            };
            // What the rules did so far stays; nothing more is changed, so nothing is put back.
            (!animation.is_empty()).then_some(Animated { animation, resolved, pristine: None })
        });
        // A thing riding one ball of a thing left out stays where it is now: its parent's balls
        // are gone, so it would fall back to its own place.
        let attach = self
            .attach
            .iter()
            .map(|(id, entry)| {
                let (parent, attach, _) = entry;
                if matches!(attach, Attach::Ride { item: Some(_), .. }) && !keep(*parent) {
                    let pose = self.anchor(*id);
                    let transform = Transform { position: pose.position, rotation: pose.angle };
                    (*id, (*parent, Attach::Group, transform))
                } else {
                    (*id, *entry)
                }
            })
            .collect();
        World {
            step: self.step,
            steps_per_second: self.steps_per_second,
            gravity: self.gravity,
            bodies: self.bodies.iter().filter(|(id, _)| kept(id)).map(|(id, b)| (*id, b.clone())).collect(),
            walls: self.walls.iter().filter(|(id, _)| kept(id)).map(|(id, w)| (*id, w.clone())).collect(),
            // The air goes along while balls follow it (it lives under the scene's id).
            states: self
                .states
                .iter()
                .filter(|(id, _)| kept(id) || (self.air_felt && **id == ThingId::ROOT))
                .map(|(id, s)| (*id, s.clone()))
                .collect(),
            poses: self.poses.clone(),
            names: self.names.clone(),
            tags: self.tags.clone(),
            history: BTreeMap::new(),
            seed: self.seed,
            camera: self.camera,
            video: self.video,
            beat_seconds: self.beat_seconds,
            beats_in_bar: self.beats_in_bar,
            first_beat: self.first_beat,
            state_memory: BTreeMap::new(),
            gpu_memory: BTreeMap::new(),
            kinds: self.kinds.clone(),
            order: self.order.iter().copied().filter(kept).collect(),
            pairs: Pairs::default(),
            walls_before: BTreeMap::new(),
            attach,
            // A card varied across the balls answers with its own values: the lone ball is none of them.
            modules: self
                .modules
                .iter()
                .filter(|(id, _)| kept(id))
                .map(|(id, m)| (*id, m.iter().map(|(slot, s)| (*slot, s.plain())).collect()))
                .collect(),
            animated,
            substep: (0, 1),
            sound: self.sound.clone(),
            light: self.light.clone(),
            pulls: BTreeMap::new(),
            air_felt: self.air_felt,
            air_at: BTreeMap::new(),
            fingerprint: self.fingerprint,
            // A test flight finds its ways as if no rule acted from now on, health staying as it is.
            rules: self.rules.as_ref().and_then(crate::rules::RulesState::lone),
            pace: 1.0,
            run: RunId::fresh(),
            beside: BTreeSet::new(),
            decoration: BTreeSet::new(),
            parts: self.parts.iter().filter(|(id, _)| kept(id)).map(|(id, p)| (*id, p.clone())).collect(),
            tree: self.tree.clone(),
        }
    }

    /// Puts `states` in for drawing, by thing (the editor's particle playback lends its Particles
    /// things' states to the main world, which never built them). Draw-only: [`World::take_back`]
    /// must put back what they replaced before the world steps again. A state of a thing the world
    /// didn't build is never stepped.
    pub fn lend_states(&mut self, states: Vec<(ThingId, Box<dyn ThingState>)>) -> Lent {
        let replaced = states.into_iter().map(|(id, s)| (id, self.states.insert(id, s))).collect();
        Lent { replaced }
    }

    /// Puts back what [`World::lend_states`] replaced.
    pub fn take_back(&mut self, lent: Lent) {
        for (id, old) in lent.replaced {
            match old {
                Some(s) => self.states.insert(id, s),
                None => self.states.remove(&id),
            };
        }
    }

    /// Gives `to` the Motion and Bounce cards `owner` has in `from` (as prepared now, the card's
    /// own values when it varies across the balls), in place of its own: a lone copy firing
    /// another thing's ball. Keys on `owner`'s cards aren't carried over.
    pub fn lend_cards(&mut self, from: &World, owner: ThingId, to: ThingId) {
        let lent: Prepared = from
            .modules
            .get(&owner)
            .map(|m| m.iter().map(|(slot, s)| (*slot, s.plain())).collect())
            .unwrap_or_default();
        if let Some(a) = &mut self.animated {
            a.animation.retain(|thing, slot, _| thing != to || slot.is_none());
        }
        if lent.is_empty() {
            self.modules.remove(&to);
        } else {
            self.modules.insert(to, lent);
        }
    }
}
