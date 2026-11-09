//! Fingerprints (what an edit restarts) and the parts of a scene each run steps: the main
//! world, the sound's, and the things stepped beside it.

use super::*;

/// Whether something physical in the scene follows light, so its worlds need the light probe: a
/// physics setting following light, a Motion or Bounce card whose settings spread by Light or
/// Shadow, or a Motion card drawing its balls to the lights.
pub fn needs_light(scene: &Scene, resolved: &Resolved, sims: &Sims, animation: Option<&Animation>) -> bool {
    let follows = animation.is_some_and(|animation| {
        animation.addresses().iter().any(|a| scene.pulses.get(*a).is_some_and(|p| p.source.follows_light()))
    });
    follows
        || resolved.things.values().any(|r| {
            SIM_SLOTS.iter().any(|slot| {
                let Some(s) = r.slots.get(slot) else { return false };
                let Some(sim) = sims.module(&s.kind) else { return false };
                let spreads = bpe_core::vary::varies_in(&s.params).iter().any(|v| v.to.is_some() && v.by.reads_light());
                spreads || (*slot == SlotId::Motion && sim.light_pull(sim.prepare(&s.params).as_ref()).is_some())
            })
        })
}

/// Whether any of `animation`'s pulses follows sound.
pub(super) fn follows_sound(scene: &Scene, animation: &Animation) -> bool {
    animation
        .addresses()
        .iter()
        .any(|a| scene.pulses.get(*a).is_some_and(|p| matches!(p.source, bpe_core::PulseSource::Sound { .. })))
}

/// A hash of everything about a scene that changes how it simulates. Scenes that differ only
/// in how they look fingerprint the same, so the editor can keep playing through
/// those edits.
pub fn fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    fingerprint_of(scene, resolved, sims, true)
}

/// [`fingerprint`] without the balls' history (how much of their past trails and particles
/// keep): what the sound hears of a run. A longer trail changes the run the editor keeps, not
/// its hits, so the sound plays on through it.
/// Things stepped beside the physics that the sound hears ([`crate::ThingSim::heard`]: a Particles
/// thing whose sound is on) count too: how they run and what of their sound card changes what they
/// log, so editing their particles runs the sound again while a volume change only patches it.
pub fn sound_fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    let base = fingerprint_of(scene, resolved, sims, false);
    let heard = |r: &bpe_core::ResolvedThing| sims.thing(&r.kind).and_then(|s| s.heard(r));
    if !resolved.things.values().any(|r| heard(r).is_some()) {
        return base;
    }
    let mut h = Hasher(base);
    h.bytes(b"heard");
    beside_hash(&mut h, scene, resolved, sims, &|r| heard(r).is_some());
    // What of its sound card changes what it logs, beside the physics or in it (a rule reads it).
    for (id, r) in &resolved.things {
        if let Some(print) = heard(r) {
            h.bytes(&id.0.to_le_bytes());
            h.bytes(&print.to_le_bytes());
        }
    }
    h.0
}

/// Whether thing `id` (resolved as `r`) is stepped beside the physics: its sim says it doesn't
/// step with it ([`crate::ThingSim::in_main`] false), no enabled rule reads it
/// ([`crate::BlockSim::keeps`]: a rule that reads it changes the balls, so it steps with them), no
/// other thing reads it as it steps ([`crate::ThingSim::keeps`]: a camera following it) and no
/// setting follows its sound (W12).
pub fn is_beside(scene: &Scene, sims: &Sims, id: ThingId, r: &bpe_core::ResolvedThing) -> bool {
    let Some(sim) = sims.thing(&r.kind) else { return false };
    if sim.in_main(r) {
        return false;
    }
    // Heat moves the air the balls follow: a thing that heats it steps with them.
    if sim.heats_air(r) && crate::air_felt(scene, sims) {
        return false;
    }
    let tags = scene.things.get(&id).map_or(NO_TAGS, |t| &t.tags);
    let read_by_a_rule = scene
        .rules
        .list
        .iter()
        .filter(|rule| rule.enabled)
        .flat_map(|rule| &rule.blocks)
        .any(|b| sims.block(&b.kind).is_some_and(|s| s.keeps(&b.params, id, &r.kind, tags)));
    // Another thing reads it as it steps (a camera following it).
    let read_by_a_thing = scene
        .things
        .values()
        .filter(|t| t.enabled && t.id != id)
        .any(|t| sims.thing(&t.kind).is_some_and(|s| s.keeps(scene, t, id)));
    // A setting follows its sound (its own, or a bus while the sound hears it): its moments must
    // be in the log Follow sound reads.
    let heard = || sim.heard(r).is_some();
    // Only a setting of a thing that is on follows anything.
    let on = |a: &bpe_core::Address| scene.things.get(&a.thing).is_some_and(|t| t.enabled);
    let sound_followed = scene.pulses.iter().filter(|(a, _)| on(a)).any(|(_, p)| match &p.source {
        bpe_core::PulseSource::Sound { shape, source } => {
            *shape == id || (matches!(source, bpe_core::SoundSource::Bus(_)) && heard())
        }
        _ => false,
    });
    !(read_by_a_rule || read_by_a_thing || sound_followed)
}

/// `resolved` without the things `drop` says, borrowed when it drops none.
pub(super) fn without<'a>(
    resolved: &'a Resolved,
    drop: impl Fn(ThingId, &bpe_core::ResolvedThing) -> bool,
) -> std::borrow::Cow<'a, Resolved> {
    if !resolved.things.iter().any(|(id, r)| drop(*id, r)) {
        return std::borrow::Cow::Borrowed(resolved);
    }
    let things = resolved.things.iter().filter(|(id, r)| !drop(**id, r)).map(|(id, r)| (*id, r.clone())).collect();
    std::borrow::Cow::Owned(Resolved { things, problems: resolved.problems.clone() })
}

/// `resolved` without the things stepped beside the physics ([`is_beside`]): what the editor's
/// main playback and the aiming build from. A world built from it steps every ball exactly as one
/// built from the whole scene.
pub fn main_part<'a>(scene: &Scene, resolved: &'a Resolved, sims: &Sims) -> std::borrow::Cow<'a, Resolved> {
    without(resolved, |id, r| is_beside(scene, sims, id, r))
}

/// [`main_part`] and the things beside it whose events the sound hears ([`crate::ThingSim::heard`]):
/// what the sound engine's physics builds from.
pub fn heard_part<'a>(scene: &Scene, resolved: &'a Resolved, sims: &Sims) -> std::borrow::Cow<'a, Resolved> {
    let heard = |r: &bpe_core::ResolvedThing| sims.thing(&r.kind).is_some_and(|s| s.heard(r).is_some());
    without(resolved, |id, r| is_beside(scene, sims, id, r) && !heard(r))
}

/// Whether some thing of `resolved` is stepped beside the physics.
pub fn has_beside(scene: &Scene, resolved: &Resolved, sims: &Sims) -> bool {
    resolved.things.iter().any(|(id, r)| is_beside(scene, sims, *id, r))
}

/// A hash of everything that changes how the things stepped beside the physics run (and the
/// physics they read), 0 when there are none: the editor's particle playback starts over when it
/// changes. Their Motion and Bounce cards count whole, as nothing but the thing reads them.
pub fn beside_fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    if !has_beside(scene, resolved, sims) {
        return 0;
    }
    let mut h = Hasher(fingerprint_of(scene, resolved, sims, true));
    beside_hash(&mut h, scene, resolved, sims, &|_| true);
    h.0
}

/// Hashes into `h` the things stepped beside the physics that `pick` takes: their settings, place,
/// parent, Motion and Bounce cards whole, and their keys.
pub(super) fn beside_hash(
    h: &mut Hasher,
    scene: &Scene,
    resolved: &Resolved,
    sims: &Sims,
    pick: &dyn Fn(&bpe_core::ResolvedThing) -> bool,
) {
    let chosen = |id: ThingId, r: &bpe_core::ResolvedThing| is_beside(scene, sims, id, r) && pick(r);
    // What the main things hold that only the beside things read (the tempo).
    for (id, r) in &resolved.things {
        let Some(sim) = sims.thing(&r.kind).filter(|s| s.in_main(r)) else { continue };
        for (key, value) in r.params.iter() {
            if sim.key_restarts_beside(key) {
                h.bytes(&id.0.to_le_bytes());
                h.bytes(key.as_bytes());
                h.value(value);
            }
        }
    }
    for (id, r) in &resolved.things {
        if !chosen(*id, r) {
            continue;
        }
        let sim = sims.thing(&r.kind).expect("a beside thing has a sim");
        let Some(thing) = scene.things.get(id) else { continue };
        h.bytes(&id.0.to_le_bytes());
        h.bytes(&sim.beside_print_extra(r).to_le_bytes());
        // What else of the saved scene it reads (the Text a Particles thing spells).
        let more = sim.physics_print_extra(scene, thing, r);
        if more != 0 {
            h.bytes(&more.to_le_bytes());
        }
        h.bytes(r.kind.as_bytes());
        h.bytes(&thing.parent.map_or(u32::MAX, |p| p.0).to_le_bytes());
        h.f32(thing.transform.position.x);
        h.f32(thing.transform.position.y);
        h.f32(thing.transform.rotation);
        h.bytes(&[thing.enabled as u8]);
        for (key, value) in r.params.iter() {
            h.bytes(key.as_bytes());
            h.value(value);
        }
        for (slot, s) in r.slots.iter().filter(|(slot, _)| !SIM_SLOTS.contains(slot)) {
            for (key, value) in s.params.iter().filter(|(key, _)| sim.card_key_restarts_beside(*slot, key)) {
                h.bytes(&[*slot as u8]);
                h.bytes(key.as_bytes());
                h.value(value);
            }
        }
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot) {
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                h.bytes(format!("{:?}", s.layers).as_bytes());
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
    }
    // The scene's cards only they read (the air).
    for (id, r) in &resolved.things {
        for slot in BESIDE_SLOTS {
            if let Some(s) = r.slots.get(&slot) {
                h.bytes(&id.0.to_le_bytes());
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
    }
    // Their keys too, and those cards', with what of the sound or the light they follow.
    if let Some(animation) = physics_animation(scene, resolved, sims) {
        let theirs = |a: &bpe_core::Address| {
            a.slot.is_some_and(|s| BESIDE_SLOTS.contains(&s))
                || resolved.things.get(&a.thing).is_some_and(|r| chosen(a.thing, r))
        };
        let follows = |light: bool| {
            animation.addresses().iter().filter(|a| theirs(a)).any(|a| {
                scene.pulses.get(*a).is_some_and(|p| {
                    if light {
                        p.source.follows_light()
                    } else {
                        matches!(p.source, bpe_core::PulseSource::Sound { .. })
                    }
                })
            })
        };
        if let Some(maker) = &sims.sound
            && follows(false)
        {
            h.bytes(b"beside hear");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        if let Some(maker) = &sims.light
            && follows(true)
        {
            h.bytes(b"beside see");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        for a in animation.addresses() {
            if theirs(a) {
                h.bytes(format!("{a:?}").as_bytes());
                if let Some(track) = scene.tracks.get(a) {
                    h.bytes(format!("{track:?}").as_bytes());
                }
                if let Some(pulse) = scene.pulses.get(a) {
                    h.bytes(format!("{pulse:?}").as_bytes());
                }
            }
        }
    }
}

pub(super) fn fingerprint_of(scene: &Scene, resolved: &Resolved, sims: &Sims, history: bool) -> u64 {
    // Things stepped beside the physics never change it.
    let resolved = &*main_part(scene, resolved, sims);
    let mut h = Hasher(HASH_OFFSET);
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        h.bytes(&id.0.to_le_bytes());
        h.bytes(r.kind.as_bytes());
        for tag in &thing.tags {
            h.bytes(b"#");
            h.bytes(tag.as_bytes());
        }
        h.bytes(&thing.parent.map_or(u32::MAX, |p| p.0).to_le_bytes());
        h.f32(thing.transform.position.x);
        h.f32(thing.transform.position.y);
        h.f32(thing.transform.rotation);
        match thing.attach {
            Attach::Group => h.bytes(&[0]),
            Attach::Ride { offset, turn_with_parent, item } => {
                h.bytes(&[1, turn_with_parent as u8]);
                h.f32(offset.x);
                h.f32(offset.y);
                h.bytes(&item.map_or(u32::MAX, |i| i).to_le_bytes());
            }
        }
        for tag in &thing.tags {
            h.bytes(tag.as_bytes());
            h.bytes(&[0]);
        }
        if let Some(sim) = sims.thing(&r.kind) {
            for (key, value) in r.params.iter() {
                if sim.key_restarts_physics_here(scene, thing, bpe_core::vary::base_key(key)) {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
            let more = sim.physics_print_extra(scene, thing, r);
            if more != 0 {
                h.bytes(b"more");
                h.bytes(&more.to_le_bytes());
            }
        }
        if let Some(want) = sims.thing(&r.kind).and_then(|s| s.history(r)).filter(|_| history) {
            // Whether a history is kept and how long, not how its trails look.
            h.bytes(b"history");
            h.f32(want.rounded());
            h.bytes(&[want.forever as u8]);
        }
        for tag in &thing.tags {
            h.bytes(tag.as_bytes());
        }
        // Only modules the processor simulates count, and the cards of a thing that reads its own.
        let owns = sims.thing(&r.kind).is_some_and(|s| s.steps_own_cards());
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot)
                && (owns || sims.module(&s.kind).is_some())
            {
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                if owns {
                    // What each layer is and does; not its name.
                    for l in &s.layers {
                        h.bytes(format!("{:?}", (l.id, &l.kind, l.parent, &l.only)).as_bytes());
                    }
                }
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
        // What else its stepping keeps (two-way particles' trails and marks), so a change rebuilds it.
        if let Some(more) = sims.thing(&r.kind).map(|s| s.beside_print_extra(r)).filter(|m| *m != 0) {
            h.bytes(b"more");
            h.bytes(&more.to_le_bytes());
        }
    }
    // Health cards change the run (a ball breaks at zero, rules read it): only scenes with one.
    for (id, r) in &resolved.things {
        if let Some(s) = r.slots.get(&SlotId::Health)
            && sims.health(&s.kind).is_some()
        {
            h.bytes(b"health");
            h.bytes(&id.0.to_le_bytes());
            h.bytes(s.kind.as_bytes());
            for (key, value) in s.params.iter() {
                h.bytes(key.as_bytes());
                h.value(value);
            }
        }
    }
    // A thing in the physics that reads its own cards (two-way particles) may read the air too,
    // and balls that follow the air (or haze moving with it) do: then the air's cards and keys
    // count.
    let felt = crate::air_read(scene, resolved, sims);
    if felt || resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| s.steps_own_cards())) {
        for (id, r) in &resolved.things {
            for slot in BESIDE_SLOTS {
                if let Some(s) = r.slots.get(&slot) {
                    h.bytes(b"beside card");
                    h.bytes(&id.0.to_le_bytes());
                    h.bytes(&[slot as u8]);
                    h.bytes(s.kind.as_bytes());
                    for (key, value) in s.params.iter() {
                        h.bytes(key.as_bytes());
                        h.value(value);
                    }
                }
            }
        }
        if let Some(animation) = physics_animation(scene, resolved, sims) {
            for a in animation.addresses() {
                if a.slot.is_some_and(|s| BESIDE_SLOTS.contains(&s)) {
                    h.bytes(format!("{a:?}").as_bytes());
                    if let Some(track) = scene.tracks.get(a) {
                        h.bytes(format!("{track:?}").as_bytes());
                    }
                    if let Some(pulse) = scene.pulses.get(a) {
                        h.bytes(format!("{pulse:?}").as_bytes());
                    }
                }
            }
        }
    }
    // Keyframes and pulses on physics settings: moving a key changes the run. Keys on the cards
    // only the things beside the physics read are theirs ([`beside_fingerprint`]), unless balls
    // follow the air: then the air's keys (and the sound or light they follow) move the balls.
    let animation = physics_animation(scene, resolved, sims)
        .as_ref()
        .and_then(|a| if felt { Some(a.clone()) } else { main_animation(a) });
    if let Some(animation) = &animation {
        for a in animation.addresses() {
            h.bytes(format!("{a:?}").as_bytes());
            if let Some(track) = scene.tracks.get(a) {
                h.bytes(format!("{track:?}").as_bytes());
            }
            if let Some(pulse) = scene.pulses.get(a) {
                h.bytes(format!("{pulse:?}").as_bytes());
            }
        }
        // A physics setting that follows sound moves with what of the sound it reads.
        if let Some(maker) = &sims.sound
            && follows_sound(scene, animation)
        {
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
    }
    // Rules change the run: every enabled rule's blocks and links (not names or switched-off ones),
    // and what of the sound and the light they read.
    if scene.rules.any_enabled() {
        let blocks = || {
            scene
                .rules
                .list
                .iter()
                .filter(|r| r.enabled)
                .flat_map(|r| &r.blocks)
                .filter_map(|b| Some((sims.block(&b.kind)?, b)))
        };
        if let Some(maker) = &sims.sound
            && blocks().any(|(sim, b)| sim.hears_sound(&b.params))
        {
            h.bytes(b"rules hear");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        if let Some(maker) = &sims.light
            && blocks().any(|(sim, b)| sim.sees_light(&b.params))
        {
            h.bytes(b"rules see");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        h.bytes(b"rules");
        for rule in scene.rules.list.iter().filter(|r| r.enabled) {
            h.bytes(&rule.id.0.to_le_bytes());
            for b in &rule.blocks {
                h.bytes(&b.id.0.to_le_bytes());
                h.bytes(b.kind.as_bytes());
                for i in &b.inputs {
                    h.bytes(&i.0.to_le_bytes());
                }
                for (key, value) in b.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
                h.bytes(&[0xff]);
            }
        }
    }
    // Physics that follows light moves with what the light probe reads.
    if let Some(maker) = &sims.light
        && needs_light(scene, resolved, sims, animation.as_ref())
    {
        h.bytes(b"light");
        h.bytes(&maker.fingerprint(scene).to_le_bytes());
    }
    h.0
}

pub(super) struct Hasher(u64);

impl Hasher {
    pub(super) fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }

    pub(super) fn f32(&mut self, x: f32) {
        self.bytes(&x.to_bits().to_le_bytes());
    }

    pub(super) fn value(&mut self, v: &Value) {
        match v {
            Value::Number(x) => {
                self.bytes(&[0]);
                self.bytes(&x.to_bits().to_le_bytes());
            }
            Value::Int(x) => {
                self.bytes(&[1]);
                self.bytes(&x.to_le_bytes());
            }
            Value::Bool(b) => self.bytes(&[2, *b as u8]),
            Value::Vec2([x, y]) => {
                self.bytes(&[3]);
                self.f32(*x);
                self.f32(*y);
            }
            Value::Color(c) => self.bytes(&[4, c.r, c.g, c.b, c.a]),
            Value::Gradient(g) => {
                self.bytes(&[5]);
                for s in &g.stops {
                    self.f32(s.at);
                    self.bytes(&[s.color.r, s.color.g, s.color.b, s.color.a, s.blend as u8]);
                    self.f32(s.middle);
                }
                self.bytes(&[g.mix as u8, g.looped as u8]);
            }
            Value::Choice(s) | Value::Text(s) => {
                self.bytes(&[6]);
                self.bytes(s.as_bytes());
            }
            Value::Target(t) => {
                self.bytes(&[7]);
                self.bytes(format!("{t:?}").as_bytes());
            }
            Value::Routes(r) => {
                self.bytes(&[11]);
                self.bytes(format!("{r:?}").as_bytes());
            }
            Value::Slots(n) => {
                self.bytes(&[12]);
                self.bytes(format!("{n:?}").as_bytes());
            }
            Value::LightScenes(l) => {
                self.bytes(&[14]);
                self.bytes(format!("{l:?}").as_bytes());
            }
            Value::Curve(c) => {
                self.bytes(&[13]);
                self.bytes(&(c.len() as u64).to_le_bytes());
                for p in c {
                    self.f32(p.x);
                    self.f32(p.y);
                    self.f32(p.bend);
                }
            }
            Value::Strokes(s) => {
                self.bytes(&[8]);
                self.bytes(&s.hash64().to_le_bytes());
            }
            Value::Points(ps) => {
                self.bytes(&[9]);
                self.bytes(&(ps.len() as u64).to_le_bytes());
                for [x, y] in ps {
                    self.f32(*x);
                    self.f32(*y);
                }
            }
            Value::Who(w) => {
                self.bytes(&[15]);
                self.bytes(format!("{w:?}").as_bytes());
            }
            Value::Path(p) => {
                self.bytes(&[10]);
                self.bytes(&(p.points.len() as u64).to_le_bytes());
                for [x, y] in &p.points {
                    self.f32(*x);
                    self.f32(*y);
                }
                for b in &p.bends {
                    match b {
                        None => self.bytes(&[0]),
                        Some(hs) => {
                            self.bytes(&[1]);
                            for [x, y] in hs {
                                self.f32(*x);
                                self.f32(*y);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Every thing with a Health card at full health: what Follow health reads while the world is built.
pub(super) struct FullHealth<'a>(pub(super) &'a Resolved);

impl bpe_core::HealthLevels for FullHealth<'_> {
    fn share(&self, thing: ThingId) -> f32 {
        let has = self.0.things.get(&thing).is_some_and(|r| r.slots.contains_key(&bpe_core::SlotId::Health));
        if has { 1.0 } else { 0.0 }
    }
}
