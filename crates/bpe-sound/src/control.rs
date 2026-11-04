//! Follow sound: a shape's modulators and its level, and each bus's level, worked out from the
//! physics alone, so a setting that follows sound moves the same in the editor, in a picture
//! export and in the physics, with sound playing or not.
//!
//! [`ControlSampler`] reads the same voice settings the engine plays ([`shape_specs`]) and the
//! hits the same [`HitFilter`] lets through, then works each part out in one go at the moment
//! asked about: an envelope from its note's start ([`bpe_dsp::level_at`]), an LFO from its
//! start, a Distance from where the balls are now, a Hit from its note's hit. It never renders
//! sound. What it leaves out (smoothing, chords, other modulators moving a modulator's knobs) is
//! in `docs/superpowers/plans/sound-system/decisions.md`.

use crate::hits::hit_sample;
use crate::modmatrix::Combine;
use crate::mods::distance::distance_value;
use crate::mods::hit::{HitValues, hit_value, hit_values};
use crate::mods::lfo::lfo_value;
use crate::patch::shape_specs;
use crate::voices::HitFilter;
use crate::voices::alloc::MAX_VOICES;
use crate::voices::knobs::{self as k, MAX_ENVS, Values};
use crate::voices::notes::mix;
use crate::voices::shape::{
    PLAIN_NOTE_S, SMOOTHED, ShapeSpec, dist_params, env_params_of, knob_gain, lfo_params, params_velocity,
};
use crate::{BlockHit, SAMPLE_RATE, TempoMap};
use bpe_core::{Address, LayerId, PulseSource, Resolved, Scene, Settings, SlotId, SoundSource, ThingId, Track};
use bpe_dsp::{EnvParams, db_to_gain, level_at};
use bpe_kit::SceneSettings;
use bpe_kit::sound::{BUSES, Follows, GateMode, GateUnit, LfoStart, MOD_KINDS, MixerSettings, PlayMode, PlayWhen};
use bpe_runtime::{Event, EventKind, Log, SoundAt, SoundMaker, World};
use std::sync::{Arc, Mutex};

/// How many of the latest accepted hits are looked through for the notes still sounding.
const LOOK_BACK: usize = 64;
/// How many retriggers a held (or Mono) envelope's level is worked out through.
const CHAIN: usize = 16;

/// Whether any setting of `scene` follows sound.
pub fn follows_sound(scene: &Scene) -> bool {
    scene.pulses.values().any(|p| matches!(p.source, PulseSource::Sound { .. }))
}

/// Gives every world built from the kit's sims a [`ControlSampler`] when a physics setting
/// follows sound.
pub struct FollowSound;

impl SoundMaker for FollowSound {
    fn make(&self, scene: &Scene, resolved: &Resolved) -> Arc<dyn SoundAt> {
        Arc::new(ControlSampler::new(scene, resolved))
    }

    fn fingerprint(&self, scene: &Scene) -> u64 {
        heard_fingerprint(scene)
    }
}

/// A hash of what a [`ControlSampler`] reads from `scene`: the tempo; each sounding shape's
/// playing and notes settings, routes, envelopes and modulators, its oscillators' on, Gain and
/// Level and its Volume and bus; and each bus's on, mute, solo and fader, with their keys and
/// pulses. Wave, pitch, pan, sends, layer chains, bus effects and the Master are left out.
pub fn heard_fingerprint(scene: &Scene) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut put = |address: &Address, value: &dyn std::fmt::Debug| {
        format!("{address:?}").hash(&mut h);
        format!("{value:?}").hash(&mut h);
        if let Some(track) = scene.tracks.get(address) {
            format!("{track:?}").hash(&mut h);
        }
        if let Some(pulse) = scene.pulses.get(address) {
            format!("{pulse:?}").hash(&mut h);
        }
    };
    if let Some(root) = scene.things.get(&ThingId::ROOT) {
        for key in ["tempo", "time_top", "time_bottom"] {
            put(&Address::own(ThingId::ROOT, key), &root.params.get(key));
        }
        if let Some(mixer) = root.slots.get(&SlotId::Mixer) {
            for n in 1..=BUSES as u8 {
                for part in ["on", "mute", "solo", "fader"] {
                    let key = MixerSettings::bus_key(n, part);
                    let a = Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key: key.clone() };
                    put(&a, &mixer.params.get(&key));
                }
            }
        }
    }
    for (id, thing) in &scene.things {
        let Some(card) = thing.slots.get(&SlotId::Sound) else { continue };
        let heard_layer = |layer: LayerId| {
            card.layer(layer).is_some_and(|l| l.kind == bpe_kit::sound::ADSR || MOD_KINDS.contains(&l.kind.as_str()))
        };
        for l in card.layers.iter().filter(|l| heard_layer(l.id)) {
            put(&Address::on(*id, SlotId::Sound, &l.id.to_string()), &(&l.kind, l.enabled));
        }
        for (key, value) in card.params.iter() {
            let heard = match bpe_core::split_layered(key) {
                Some((layer, _)) => heard_layer(layer),
                None => match key.split_once('_') {
                    Some(("a" | "b", rest)) => matches!(rest, "on" | "gain" | "level"),
                    Some(("out", rest)) => matches!(rest, "volume" | "bus"),
                    _ => true,
                },
            };
            if heard {
                put(&Address::on(*id, SlotId::Sound, key), value);
            }
        }
        put(&Address::on(*id, SlotId::Sound, "kind"), &card.kind);
    }
    h.finish()
}

/// A scene's sound levels from its physics. See the module notes.
pub struct ControlSampler {
    shapes: Vec<Shape>,
    tempo: TempoMap,
    buses: [Bus; BUSES],
    /// Each shape's accepted hits so far, kept up with the log.
    heard: Mutex<Vec<Heard>>,
}

/// One bus as Follow sound reads it.
struct Bus {
    plays: bool,
    fader: f32,
    keyed: Option<Track>,
}

/// One sounding shape.
struct Shape {
    spec: ShapeSpec,
    /// What each of its routes' sources reads.
    src: Vec<Src>,
    /// The envelopes that scale a level, which decide when a note is over.
    level_envs: [bool; MAX_ENVS],
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Src {
    None,
    Env(usize),
    Lfo(usize),
    Dist(usize),
    Hit(usize),
}

/// A hit that plays on a shape.
#[derive(Clone, Debug)]
struct Accepted {
    sample: u64,
    strength: f32,
    event: Event,
    seed: u64,
}

/// A shape's hits so far: the filter as the engine runs it, over the log in order.
struct Heard {
    filter: HitFilter,
    /// How many of the log's events have been read, and the last of them (a log that went back
    /// is read again from the start).
    done: usize,
    last: Option<Event>,
    hits: Vec<Accepted>,
}

/// A note sounding at the moment asked about.
struct NoteAt {
    on: u64,
    /// Its hit (none for a held note before any hit).
    hit: Option<Accepted>,
    env: [f32; MAX_ENVS],
}

/// The shared modulators at the moment asked about.
#[derive(Default)]
struct Shared {
    lfo: [f32; k::MAX_EACH],
    dist: [f32; k::MAX_EACH],
}

/// The knobs of `spec` at `time`: as set, keyed ones from their tracks.
fn knobs_at(spec: &ShapeSpec, time: f64) -> Values {
    let mut v = spec.base;
    for (i, track) in &spec.keyed {
        if let Some(x) = k::track_value(track, time) {
            v[*i] = x;
        }
    }
    v
}

fn seconds(samples: u64) -> f32 {
    (samples as f64 / SAMPLE_RATE as f64) as f32
}

impl ControlSampler {
    pub fn new(scene: &Scene, resolved: &Resolved) -> Self {
        let root = resolved.things.get(&ThingId::ROOT);
        let settings = root.map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default();
        let mixer = root
            .and_then(|r| r.slots.get(&SlotId::Mixer))
            .map(|s| MixerSettings::from_params(&s.params))
            .unwrap_or_default();
        let any_solo = (1..=BUSES as u8).any(|n| {
            let b = mixer.bus(n);
            b.on && b.solo
        });
        let buses = std::array::from_fn(|i| {
            let n = i as u8 + 1;
            let b = mixer.bus(n);
            let a =
                Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key: MixerSettings::bus_key(n, "fader") };
            Bus {
                plays: b.on && !b.mute && (!any_solo || b.solo),
                fader: b.fader,
                keyed: scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).cloned(),
            }
        });
        let shapes: Vec<Shape> = shape_specs(scene, resolved).into_iter().map(Shape::new).collect();
        let heard = shapes
            .iter()
            .map(|s| Heard {
                filter: HitFilter::new(s.spec.thing, 0.0, 0.0, s.spec.heard_from.clone()),
                done: 0,
                last: None,
                hits: Vec::new(),
            })
            .collect();
        ControlSampler { shapes, tempo: TempoMap::from_settings(&settings), buses, heard: Mutex::new(heard) }
    }

    /// The level of `source` of `shape` at `time`, with the world at that time and the log up
    /// to it.
    pub fn value(&self, world: &World, log: &Log, shape: ThingId, source: &SoundSource, time: f64) -> f32 {
        let t = (time.max(0.0) * SAMPLE_RATE as f64).round() as u64;
        let mut heard = self.heard.lock().unwrap_or_else(|e| e.into_inner());
        let v = match source {
            SoundSource::Bus(n) => {
                let Some(bus) = self.buses.get((*n as usize).wrapping_sub(1)) else { return 0.0 };
                if !bus.plays {
                    return 0.0;
                }
                let fader = bus.keyed.as_ref().and_then(|tr| k::track_value(tr, time)).unwrap_or(bus.fader);
                let mut sum = 0.0;
                for (i, s) in self.shapes.iter().enumerate() {
                    if s.spec.bus == *n {
                        s.sync(&mut heard[i], world, log);
                        sum += s.level(&self.tempo, &heard[i].hits, world, t, time);
                    }
                }
                sum * db_to_gain(fader)
            }
            _ => {
                let Some(i) = self.shapes.iter().position(|s| s.spec.thing == shape) else { return 0.0 };
                let s = &self.shapes[i];
                s.sync(&mut heard[i], world, log);
                match source {
                    SoundSource::Level => s.level(&self.tempo, &heard[i].hits, world, t, time),
                    SoundSource::Modulator(layer) => s.modulator(&self.tempo, &heard[i].hits, world, *layer, t, time),
                    SoundSource::Bus(_) => 0.0,
                }
            }
        };
        if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
    }
}

impl SoundAt for ControlSampler {
    fn level(&self, world: &World, log: &Log, shape: ThingId, source: &SoundSource, time: f64) -> f32 {
        self.value(world, log, shape, source, time)
    }
}

impl Shape {
    fn new(spec: ShapeSpec) -> Shape {
        let m = &spec.mods;
        let src: Vec<Src> = spec
            .matrix
            .sources()
            .iter()
            .map(|id| {
                if let Some(e) = spec.envs.iter().take(MAX_ENVS).position(|x| x.id == *id) {
                    Src::Env(e)
                } else if let Some(i) = m.lfos.iter().take(k::MAX_EACH).position(|x| x.id == *id) {
                    Src::Lfo(i)
                } else if let Some(i) = m.dists.iter().take(k::MAX_EACH).position(|x| x.id == *id) {
                    Src::Dist(i)
                } else if let Some(i) = m.hits.iter().take(k::MAX_EACH).position(|x| x.id == *id) {
                    Src::Hit(i)
                } else {
                    Src::None
                }
            })
            .collect();
        let mut level_envs = [false; MAX_ENVS];
        for &target in &SMOOTHED {
            for (s, src) in src.iter().enumerate() {
                if let Src::Env(e) = *src
                    && spec.matrix.route_from(s as u16, target as u16, Combine::Scale)
                {
                    level_envs[e] = true;
                }
            }
        }
        Shape { spec, src, level_envs }
    }

    /// Reads the log's new events: the hits the shape's filter lets through, as the engine does.
    fn sync(&self, h: &mut Heard, world: &World, log: &Log) {
        let events = log.events();
        let went_back = h.done > events.len() || (h.done > 0 && h.last.as_ref() != Some(&events[h.done - 1]));
        if went_back {
            h.filter.reset();
            h.hits.clear();
            h.done = 0;
        }
        let per_step = SAMPLE_RATE as f64 / world.steps_per_second.max(1) as f64;
        let thing = self.spec.thing;
        // In sample order, as the engine hears them (hits can land out of order within a step).
        let mut new: Vec<(u64, &bpe_runtime::Event)> = events[h.done..]
            .iter()
            .filter(|e| e.kind == EventKind::Hit && e.b.map(|b| b.thing) == Some(thing))
            .map(|e| (hit_sample(e, per_step), e))
            .collect();
        new.sort_by_key(|(sample, _)| *sample);
        for (sample, e) in new {
            let time = sample as f64 / SAMPLE_RATE as f64;
            let knob = |place: usize| {
                self.spec
                    .keyed
                    .iter()
                    .find(|(i, _)| *i == place)
                    .and_then(|(_, tr)| k::track_value(tr, time))
                    .unwrap_or(self.spec.base[place])
            };
            h.filter.set(knob(k::MIN_STRENGTH) / 100.0, knob(k::RETRIGGER) / 1000.0);
            let hit = BlockHit { offset: 0, event: e.clone() };
            if let Some(strength) = h.filter.accept(&hit, sample, world) {
                let ball = e.a;
                let seed =
                    mix(sample ^ mix(((ball.thing.0 as u64) << 32) | ball.item as u64) ^ ((thing.0 as u64) << 48));
                h.hits.push(Accepted { sample, strength, event: e.clone(), seed });
            }
        }
        h.done = events.len();
        h.last = events.last().cloned();
    }

    fn envs(&self) -> usize {
        self.spec.envs.len().min(MAX_ENVS)
    }

    /// Envelope `e`'s gate in seconds from its note's start (`next`: the next accepted hit's
    /// sample, for Until next hit).
    fn gate(&self, tempo: &TempoMap, kv: &Values, e: usize, on: u64, next: Option<u64>) -> f32 {
        let spec = &self.spec.envs[e];
        match (spec.gate_mode, spec.gate_unit) {
            (GateMode::UntilNextHit, _) => next.map_or(f32::INFINITY, |n| seconds(n.saturating_sub(on))),
            (GateMode::Fixed, GateUnit::Ms) => kv[k::env(e, k::GATE_MS)].max(0.0) / 1000.0,
            (GateMode::Fixed, GateUnit::Beats) => {
                tempo.seconds_of_beats(kv[k::env(e, k::GATE_BEATS)].max(0.0) as f64) as f32
            }
        }
    }

    /// Whether a note whose envelopes are `done` (`since` seconds in) is over.
    fn over(&self, done: &[bool; MAX_ENVS], since: f32) -> bool {
        let envs = self.envs();
        if (0..envs).any(|e| self.level_envs[e]) {
            (0..envs).filter(|&e| self.level_envs[e]).all(|e| done[e])
        } else if envs > 0 {
            (0..envs).all(|e| done[e])
        } else {
            since >= PLAIN_NOTE_S
        }
    }

    /// Each envelope's level after `triggers` (sample, strength), each one starting again from
    /// where the one before had got to, at sample `t`; and whether each has finished.
    fn chain(
        &self,
        tempo: &TempoMap,
        kv: &Values,
        triggers: &[(u64, f32)],
        held: bool,
        t: u64,
    ) -> ([f32; MAX_ENVS], [bool; MAX_ENVS]) {
        let mut env = [0.0; MAX_ENVS];
        let mut done = [true; MAX_ENVS];
        for e in 0..self.envs() {
            let p: EnvParams = env_params_of(kv, e);
            let vel = params_velocity(kv, e);
            let mut from = 0.0;
            for (j, &(on, strength)) in triggers.iter().enumerate() {
                let next = triggers.get(j + 1).map(|n| n.0);
                let end = next.unwrap_or(t);
                let gate = if held { f32::INFINITY } else { self.gate(tempo, kv, e, on, next) };
                let (level, d) = level_at(&p, 1.0 - vel + vel * strength, from, seconds(end.saturating_sub(on)), gate);
                from = level;
                if next.is_none() {
                    env[e] = level;
                    done[e] = d;
                }
            }
        }
        (env, done)
    }

    /// The notes sounding at sample `t`.
    fn notes(&self, tempo: &TempoMap, kv: &Values, hits: &[Accepted], world: &World, t: u64) -> Vec<NoteAt> {
        let upto = hits.partition_point(|h| h.sample <= t);
        let past = &hits[..upto];
        let spec = &self.spec;
        let mono = spec.play_mode == PlayMode::Mono;
        match spec.play_when {
            PlayWhen::OnHit if !mono => {
                let limit = (kv[k::VOICES].round() as usize).clamp(1, MAX_VOICES);
                let mut out = Vec::new();
                for j in (upto.saturating_sub(LOOK_BACK)..upto).rev() {
                    if out.len() >= limit {
                        break;
                    }
                    let h = &past[j];
                    let next = past.get(j + 1).map(|n| n.sample);
                    let (mut env, mut done) = ([0.0; MAX_ENVS], [true; MAX_ENVS]);
                    let since = seconds(t - h.sample);
                    for e in 0..self.envs() {
                        let p = env_params_of(kv, e);
                        let vel = params_velocity(kv, e);
                        let gate = self.gate(tempo, kv, e, h.sample, next);
                        (env[e], done[e]) = level_at(&p, 1.0 - vel + vel * h.strength, 0.0, since, gate);
                    }
                    if !self.over(&done, since) {
                        out.push(NoteAt { on: h.sample, hit: Some(h.clone()), env });
                    }
                }
                out
            }
            PlayWhen::OnHit => {
                let Some(last) = past.last() else { return Vec::new() };
                let triggers: Vec<(u64, f32)> =
                    past[upto.saturating_sub(CHAIN)..].iter().map(|h| (h.sample, h.strength)).collect();
                let (env, done) = self.chain(tempo, kv, &triggers, false, t);
                if self.over(&done, seconds(t - last.sample)) {
                    return Vec::new();
                }
                vec![NoteAt { on: last.sample, hit: Some(last.clone()), env }]
            }
            PlayWhen::Always | PlayWhen::InRange => {
                let has_notes = spec.slots.first().is_some_and(|s| !s.notes.is_empty());
                if !has_notes || (spec.play_when == PlayWhen::InRange && !self.in_range(kv, world)) {
                    return Vec::new();
                }
                // Held from the start at full strength; hits don't play a held note again.
                let (env, _) = self.chain(tempo, kv, &[(0, 1.0)], true, t);
                vec![NoteAt { on: 0, hit: past.last().cloned(), env }]
            }
        }
    }

    /// Whether a ball is in the In range Distance's range now (as the engine reads it).
    fn in_range(&self, kv: &Values, world: &World) -> bool {
        let Some(i) = self.spec.zone else { return false };
        let Some(d) = self.spec.mods.dists.get(i) else { return false };
        let mut p = dist_params(d, i, kv);
        if p.follows == Follows::EachNotesBall {
            p.follows = Follows::Nearest;
        }
        distance_value(&p, &d.heard, world, self.spec.thing, None).is_some_and(|v| v > 0.0)
    }

    /// The shared LFOs and Distances at sample `t`.
    fn shared(&self, tempo: &TempoMap, kv: &Values, hits: &[Accepted], world: &World, t: u64) -> Shared {
        let m = &self.spec.mods;
        let mut out = Shared::default();
        let last = hits[..hits.partition_point(|h| h.sample <= t)].last().map(|h| h.sample);
        for (i, l) in m.lfos.iter().enumerate().take(k::MAX_EACH) {
            if m.lfo_per_note(i) {
                continue;
            }
            let p = lfo_params(l, i, kv, tempo);
            let started = match (l.settings.start, last) {
                (LfoStart::Restart, Some(s)) => s,
                _ => 0,
            };
            let since = seconds(t.saturating_sub(started)) as f64;
            out.lfo[i] = lfo_value(&p, p.rate_hz.max(0.0) * since, since, l.seed);
        }
        for (i, d) in m.dists.iter().enumerate().take(k::MAX_EACH) {
            if m.dist_per_note(i) {
                continue;
            }
            let p = dist_params(d, i, kv);
            out.dist[i] = distance_value(&p, &d.heard, world, self.spec.thing, None).unwrap_or(0.0);
        }
        out
    }

    /// The seed a note's own LFOs take: from its slot's first note (Follow sound doesn't pick
    /// notes).
    fn note_seed(&self, n: &NoteAt) -> u64 {
        let note = self.spec.slots.first().and_then(|s| s.notes.first()).copied().unwrap_or(60) as u64;
        match &n.hit {
            Some(h) if self.spec.play_when == PlayWhen::OnHit => mix(h.seed ^ note),
            _ => mix(((self.spec.thing.0 as u64) << 40) ^ 0xD20E ^ note),
        }
    }

    /// What `src` reads for note `n` (none: the largest a per-note source has is its own).
    #[allow(clippy::too_many_arguments)]
    fn source(
        &self,
        tempo: &TempoMap,
        kv: &Values,
        world: &World,
        shared: &Shared,
        src: Src,
        n: &NoteAt,
        t: u64,
    ) -> f32 {
        let m = &self.spec.mods;
        match src {
            Src::None => 0.0,
            Src::Env(e) => n.env[e],
            Src::Lfo(i) if !m.lfo_per_note(i) => shared.lfo[i],
            Src::Lfo(i) => {
                let p = lfo_params(&m.lfos[i], i, kv, tempo);
                let since = seconds(t.saturating_sub(n.on)) as f64;
                lfo_value(&p, p.rate_hz.max(0.0) * since, since, m.lfos[i].seed ^ self.note_seed(n))
            }
            Src::Dist(i) if !m.dist_per_note(i) => shared.dist[i],
            Src::Dist(i) => {
                let p = dist_params(&m.dists[i], i, kv);
                let ball = n.hit.as_ref().map(|h| h.event.a);
                ball.and_then(|b| distance_value(&p, &m.dists[i].heard, world, self.spec.thing, Some(b))).unwrap_or(0.0)
            }
            Src::Hit(i) => {
                let hv = match &n.hit {
                    Some(h) => hit_values(world, self.spec.thing, &h.event, h.seed),
                    None => HitValues { strength: 1.0, ..HitValues::default() },
                };
                let at = |key: usize| kv[k::hit(i, key)];
                hit_value(m.hits[i].pick, &hv, at(k::LOW), at(k::HIGH), at(k::HIT_CURVE) / 100.0)
            }
        }
    }

    /// Modulator `layer`'s value: a shared one's own, a per-note one's largest among the notes.
    fn modulator(&self, tempo: &TempoMap, hits: &[Accepted], world: &World, layer: LayerId, t: u64, time: f64) -> f32 {
        let m = &self.spec.mods;
        let src = if let Some(e) = self.spec.envs.iter().take(MAX_ENVS).position(|x| x.id == layer) {
            Src::Env(e)
        } else if let Some(i) = m.lfos.iter().take(k::MAX_EACH).position(|x| x.id == layer) {
            Src::Lfo(i)
        } else if let Some(i) = m.dists.iter().take(k::MAX_EACH).position(|x| x.id == layer) {
            Src::Dist(i)
        } else if let Some(i) = m.hits.iter().take(k::MAX_EACH).position(|x| x.id == layer) {
            Src::Hit(i)
        } else {
            return 0.0;
        };
        let kv = knobs_at(&self.spec, time);
        let shared = self.shared(tempo, &kv, hits, world, t);
        match src {
            Src::Lfo(i) if !m.lfo_per_note(i) => shared.lfo[i],
            Src::Dist(i) if !m.dist_per_note(i) => shared.dist[i],
            _ => self
                .notes(tempo, &kv, hits, world, t)
                .iter()
                .map(|n| self.source(tempo, &kv, world, &shared, src, n, t))
                .fold(0.0, f32::max),
        }
    }

    /// How loud the shape is: the largest among its notes of its gain knobs times their scaling
    /// routes (Output's Volume, and the louder oscillator's Gain and Level).
    fn level(&self, tempo: &TempoMap, hits: &[Accepted], world: &World, t: u64, time: f64) -> f32 {
        let kv = knobs_at(&self.spec, time);
        let shared = self.shared(tempo, &kv, hits, world, t);
        let matrix = &self.spec.matrix;
        let mut vals = vec![0.0; self.src.len()];
        let mut loudest = 0.0f32;
        for n in self.notes(tempo, &kv, hits, world, t) {
            for (s, src) in self.src.iter().enumerate() {
                vals[s] = self.source(tempo, &kv, world, &shared, *src, &n, t);
            }
            let gain = |place: usize| knob_gain(kv[place]) * matrix.factor(place as u16, &vals);
            let osc = (0..2)
                .filter(|&o| self.spec.osc[o].on)
                .map(|o| gain(k::osc(o, k::GAIN)) * gain(k::osc(o, k::LEVEL)))
                .fold(0.0, f32::max);
            loudest = loudest.max(gain(k::VOLUME) * osc);
        }
        loudest.clamp(0.0, 1.0)
    }
}
