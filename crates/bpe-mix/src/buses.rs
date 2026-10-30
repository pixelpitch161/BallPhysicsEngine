//! After the channels: sends into the return buses, each bus's effects, and the master
//! effects over the whole mix. What comes out goes to the master stage.

use crate::channels::{CachedChannel, ChannelCache, GroupKey, with_fx};
use crate::{MixSettings, Note, PhysicsInputs};
use bpe_synth::{Ctx, SceneSignals, Sources, effects_or_through};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

/// What effects know while they run: the tempo and the seed, from the top of the track.
pub(crate) fn fx_ctx(settings: &MixSettings) -> Ctx {
    Ctx { seconds_per_beat: settings.seconds_per_beat(), seed: settings.seed, ..Ctx::default() }
}

/// Runs `f` on every item, side by side on all cores.
pub(crate) fn par_each<T: Send>(items: &mut [T], f: impl Fn(&mut T) + Sync) {
    if items.len() < 2 {
        items.iter_mut().for_each(&f);
        return;
    }
    let per = items.len().div_ceil(crate::level::threads());
    std::thread::scope(|scope| {
        for chunk in items.chunks_mut(per) {
            let f = &f;
            scope.spawn(move || chunk.iter_mut().for_each(f));
        }
    });
}

/// What stops an effects graph working, as a line for the status bar, when something does.
pub(crate) fn fx_problem(graph: &bpe_graph::Graph, place: &str) -> Option<String> {
    let (_, problem) = effects_or_through(graph);
    problem.map(|p| format!("{place}: the effects don't work ({p}), so they're bypassed"))
}

/// Runs an effects graph over `samples` in place; one that doesn't compile leaves them be.
/// Samples that aren't numbers become silence, and the answer is whether there were any.
pub(crate) fn run_fx(graph: &bpe_graph::Graph, samples: &mut [f32], cx: Ctx, scene: &Arc<SceneSignals>) -> bool {
    let (Some(program), _) = effects_or_through(graph) else { return false };
    program.run_with(samples, cx, &Sources { hit: None, scene: Some(scene.clone()) });
    let mut bad = false;
    for v in samples.iter_mut().filter(|v| !v.is_finite()) {
        *v = 0.0;
        bad = true;
    }
    bad
}

fn add(into: &mut [f32], from: &[f32], gain: f32) {
    for (o, v) in into.iter_mut().zip(from) {
        *o += v * gain;
    }
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Each channel: its instrument groups added up in order, through its inserts. Unchanged
/// channels come from `cache`.
pub(crate) fn strips(
    settings: &MixSettings,
    groups: &BTreeMap<GroupKey, Arc<Vec<f32>>>,
    fingerprints: &BTreeMap<GroupKey, u64>,
    frames: usize,
    cache: &mut ChannelCache,
    problems: &mut BTreeSet<String>,
    signals: &dyn Fn(u32) -> Arc<SceneSignals>,
) -> BTreeMap<u32, (Arc<Vec<f32>>, u64)> {
    let mut members: BTreeMap<u32, Vec<&GroupKey>> = BTreeMap::new();
    for key in groups.keys() {
        members.entry(key.0).or_default().push(key);
    }
    let mut out = BTreeMap::new();
    let mut jobs = Vec::new();
    for (&channel, keys) in &members {
        let inserts = settings.strips.get(&channel).map(|s| &s.inserts).filter(|g| !g.nodes.is_empty());
        let mut h = DefaultHasher::new();
        (channel, frames).hash(&mut h);
        for k in keys {
            fingerprints.get(k).hash(&mut h);
        }
        let scene = signals(channel);
        let fingerprint = with_fx(h.finish(), inserts, &fx_ctx(settings), &scene);
        let name = settings.channel_names.get(&channel).cloned().unwrap_or_else(|| format!("Channel {channel}"));
        let place = format!("{name} inserts");
        if let Some(fx) = inserts {
            problems.extend(fx_problem(fx, &place));
        }
        match cache.strips.get(&channel).filter(|c| c.fingerprint == fingerprint) {
            Some(c) => {
                if c.fx_bad {
                    problems.insert(format!("{place} made bad samples"));
                }
                out.insert(channel, (c.samples.clone(), fingerprint));
            }
            None if inserts.is_none() && keys.len() == 1 => {
                out.insert(channel, (groups[keys[0]].clone(), fingerprint));
            }
            None => {
                let mut sum = vec![0.0f32; frames * 2];
                for k in keys {
                    add(&mut sum, &groups[*k], 1.0);
                }
                jobs.push((channel, fingerprint, sum, inserts, place, false, scene));
            }
        }
    }
    let cx = fx_ctx(settings);
    par_each(&mut jobs, |(_, _, samples, fx, _, bad, scene)| {
        if let Some(fx) = fx {
            *bad = run_fx(fx, samples, cx, scene);
        }
    });
    for (channel, fingerprint, samples, _, place, bad, _) in jobs {
        if bad {
            problems.insert(format!("{place} made bad samples"));
        }
        let samples = Arc::new(samples);
        cache
            .strips
            .insert(channel, CachedChannel { fingerprint, samples: samples.clone(), bad_samples: false, fx_bad: bad });
        out.insert(channel, (samples, fingerprint));
    }
    out
}

/// The mix before the master effects and the master stage.
pub(crate) struct Premaster {
    pub samples: Vec<f32>,
    pub problems: Vec<String>,
    /// How many instrument groups were rendered rather than taken from the cache.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fresh: usize,
    /// The channel levels anything reads, by name, for the master effects.
    pub levels: BTreeMap<String, Arc<Vec<f32>>>,
}

/// Plays `notes` through their channels, sends and buses: the mix before the master effects
/// and the master stage. Gives the samples, the problems, and how many instrument groups
/// were rendered rather than taken from `cache`.
pub(crate) fn premaster_from_notes(
    settings: &MixSettings,
    notes: &[Note],
    frames: usize,
    cache: &mut ChannelCache,
    physics: &PhysicsInputs,
) -> Premaster {
    let rendered = crate::channels::render_with(settings, notes, frames, cache, physics);
    let mut problems: BTreeSet<String> = rendered.problems.into_iter().collect();
    let mut mix = vec![0.0f32; frames * 2];
    for (samples, _) in rendered.channels.values() {
        add(&mut mix, samples, 1.0);
    }

    // Each bus: what the channels send it, through its effects. Kept before its volume, so
    // turning a bus up or down doesn't run its effects again.
    let cx = fx_ctx(settings);
    let scene = Arc::new(rendered.scene.clone());
    let mut jobs = Vec::new();
    let mut ready: Vec<(usize, Arc<Vec<f32>>)> = Vec::new();
    let mut live = BTreeSet::new();
    for (i, bus) in settings.buses.iter().enumerate() {
        let sends: Vec<(u32, f32)> = rendered
            .channels
            .keys()
            .filter_map(|ch| settings.strips.get(ch)?.sends.get(&bus.id).map(|&db| (*ch, db)))
            .collect();
        let fx = Some(&bus.fx).filter(|g| !g.nodes.is_empty());
        let place = format!("{} bus", bus.name);
        if let Some(fx) = fx {
            problems.extend(fx_problem(fx, &place));
        }
        if sends.is_empty() {
            continue;
        }
        live.insert(bus.id);
        let mut h = DefaultHasher::new();
        (bus.id, frames).hash(&mut h);
        for (ch, db) in &sends {
            (ch, rendered.channels[ch].1, db.to_bits()).hash(&mut h);
        }
        let fingerprint = with_fx(h.finish(), fx, &cx, &scene);
        match cache.buses.get(&bus.id).filter(|c| c.fingerprint == fingerprint) {
            Some(c) => {
                if c.fx_bad {
                    problems.insert(format!("{place} made bad samples"));
                }
                ready.push((i, c.samples.clone()));
            }
            None => {
                let mut input = vec![0.0f32; frames * 2];
                for (ch, db) in &sends {
                    add(&mut input, &rendered.channels[ch].0, db_to_gain(*db));
                }
                jobs.push((i, fingerprint, input, fx, place, false));
            }
        }
    }
    par_each(&mut jobs, |(_, _, samples, fx, _, bad)| {
        if let Some(fx) = fx {
            *bad = run_fx(fx, samples, cx, &scene);
        }
    });
    for (i, fingerprint, samples, _, place, bad) in jobs {
        if bad {
            problems.insert(format!("{place} made bad samples"));
        }
        let samples = Arc::new(samples);
        let id = settings.buses[i].id;
        cache
            .buses
            .insert(id, CachedChannel { fingerprint, samples: samples.clone(), bad_samples: false, fx_bad: bad });
        ready.push((i, samples));
    }
    cache.buses.retain(|id, _| live.contains(id));
    ready.sort_by_key(|(i, _)| *i);
    for (i, samples) in ready {
        add(&mut mix, &samples, db_to_gain(settings.buses[i].volume));
    }

    Premaster {
        samples: mix,
        problems: problems.into_iter().collect(),
        fresh: rendered.fresh,
        levels: rendered.scene.levels,
    }
}

/// Runs the master effects over the whole mix (the song in it too), before the master stage.
/// Gives what went wrong, as lines for the status bar.
pub fn run_master_fx(samples: &mut [f32], settings: &MixSettings, physics: &PhysicsInputs) -> Vec<String> {
    if settings.master_fx.nodes.is_empty() {
        return Vec::new();
    }
    let mut problems: Vec<String> = fx_problem(&settings.master_fx, "Master effects").into_iter().collect();
    let scene = Arc::new(physics.scene.clone());
    if run_fx(&settings.master_fx, samples, fx_ctx(settings), &scene) {
        problems.push("Master effects made bad samples".into());
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::settings;
    use crate::{Instrument, Note, NoteInstrument, SAMPLE_RATE};
    use bpe_core::{BusId, ChannelStrip};
    use bpe_graph::{Graph, Param};
    use std::collections::BTreeMap;

    fn plucks(channel: u32) -> Vec<Note> {
        (0..10)
            .map(|i| Note {
                frame: i * 4_800,
                instrument: NoteInstrument::BuiltIn(Instrument::Pluck),
                key: 60 + i as u8,
                velocity: 0.8,
                left: 0.7,
                right: 0.7,
                channel,
                hit: None,
            })
            .collect()
    }

    fn chain(kind: &str, knobs: &[(&str, f32)]) -> Graph {
        let knobs: BTreeMap<String, Param> = knobs.iter().map(|(k, v)| (k.to_string(), Param::Number(*v))).collect();
        bpe_synth::chain_graph(&[(kind.into(), knobs, true)])
    }

    fn with_reverb_bus(send: Option<(BusId, f32)>) -> crate::MixSettings {
        let mut s = settings();
        s.buses = vec![bpe_core::Bus {
            id: BusId(0),
            name: "Reverb".into(),
            volume: 0.0,
            fx: chain("reverb", &[("mix", 1.0), ("decay", 2.0)]),
        }];
        if let Some((bus, db)) = send {
            s.strips.insert(1, ChannelStrip { sends: [(bus, db)].into(), ..Default::default() });
        }
        s
    }

    fn premaster(s: &crate::MixSettings, cache: &mut crate::ChannelCache) -> (Vec<f32>, Vec<String>, usize) {
        let frames = crate::Soundtrack::silent(s.duration).frames();
        let p = premaster_from_notes(s, &plucks(1), frames, cache, &Default::default());
        let (mut mix, mut problems, fresh) = (p.samples, p.problems, p.fresh);
        problems.extend(run_master_fx(&mut mix, s, &Default::default()));
        (mix, problems, fresh)
    }

    fn energy(x: &[f32], from: f32, to: f32) -> f32 {
        let (a, b) = ((from * SAMPLE_RATE as f32) as usize * 2, (to * SAMPLE_RATE as f32) as usize * 2);
        x[a..b.min(x.len())].iter().map(|v| v * v).sum::<f32>()
    }

    #[test]
    fn no_effects_is_the_old_mix() {
        let s = settings();
        let frames = crate::Soundtrack::silent(s.duration).frames();
        let rendered = crate::channels::render(&s, &plucks(1), frames, &mut Default::default());
        let mut sum = vec![0.0f32; frames * 2];
        for (samples, _) in rendered.channels.values() {
            sum.iter_mut().zip(samples.iter()).for_each(|(o, v)| *o += v);
        }
        let (mixed, problems, _) = premaster(&s, &mut Default::default());
        assert!(problems.is_empty());
        assert_eq!(mixed, sum);
    }

    #[test]
    fn a_send_feeds_the_reverb_tail() {
        let (dry, ..) = premaster(&with_reverb_bus(None), &mut Default::default());
        let (wet, ..) = premaster(&with_reverb_bus(Some((BusId(0), 0.0))), &mut Default::default());
        let tail = |x: &[f32]| energy(x, 2.0, 3.0);
        assert!(10.0 * ((tail(&wet) + 1e-12) / (tail(&dry) + 1e-12)).log10() > 20.0);
    }

    #[test]
    fn a_send_to_a_removed_bus_is_ignored() {
        let (plain, ..) = premaster(&with_reverb_bus(None), &mut Default::default());
        let (dangling, problems, _) = premaster(&with_reverb_bus(Some((BusId(99), 0.0))), &mut Default::default());
        assert_eq!(plain, dangling);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_broken_insert_passes_through() {
        let plain = premaster(&settings(), &mut Default::default()).0;
        let mut s = settings();
        let mut broken = bpe_synth::chain_graph(&[]);
        broken.add(bpe_graph::Catalog::spec(bpe_synth::catalog(), "fx_out").unwrap(), [0.0, 0.0]);
        s.strips.insert(1, ChannelStrip { inserts: broken, ..Default::default() });
        s.channel_names.insert(1, "Bell sound".into());
        let (mixed, problems, _) = premaster(&s, &mut Default::default());
        assert_eq!(mixed, plain);
        assert!(problems.iter().any(|p| p.contains("Bell sound inserts") && p.contains("bypassed")), "{problems:?}");
    }

    #[test]
    fn changing_a_bus_keeps_the_channels() {
        let mut s = with_reverb_bus(Some((BusId(0), -6.0)));
        let mut cache = crate::ChannelCache::default();
        let (first, _, fresh) = premaster(&s, &mut cache);
        assert!(fresh > 0);
        s.buses[0].volume = -12.0;
        let (second, _, fresh) = premaster(&s, &mut cache);
        assert_eq!(fresh, 0);
        assert_ne!(first, second);
        // And the cached mix is the same as a fresh one.
        assert_eq!(second, premaster(&s, &mut Default::default()).0);
    }

    #[test]
    fn a_tempo_change_reruns_synced_effects() {
        let synced = || {
            let mut g = chain("delay", &[("mix", 1.0)]);
            let id = g.nodes.iter().find(|n| n.kind == "delay").unwrap().id;
            g.node_mut(id).unwrap().params.insert("time_mode".into(), Param::Choice("1/4".into()));
            g
        };
        let mut s = with_reverb_bus(Some((BusId(0), 0.0)));
        s.buses[0].fx = synced();
        s.strips.get_mut(&1).unwrap().inserts = synced();
        s.beats = (0..20).map(|i| i as f64 * 0.5).collect();
        let mut cache = crate::ChannelCache::default();
        premaster(&s, &mut cache);
        s.beats = (0..20).map(|i| i as f64 * 0.3).collect();
        let cached = premaster(&s, &mut cache).0;
        assert!(cached == premaster(&s, &mut Default::default()).0, "the echoes follow the new tempo");
    }

    #[test]
    fn a_one_second_scene_mixes() {
        let mut s = with_reverb_bus(Some((BusId(0), 0.0)));
        s.duration = 1.0;
        let (mixed, ..) = premaster(&s, &mut Default::default());
        assert_eq!(mixed.len(), 48_000 * 2);
        assert!(mixed.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_mix_is_the_same_twice() {
        let mut s = with_reverb_bus(Some((BusId(0), -3.0)));
        s.strips.get_mut(&1).unwrap().inserts = chain("chorus", &[]);
        s.master_fx = chain("compressor", &[]);
        let a = premaster(&s, &mut Default::default()).0;
        let b = premaster(&s, &mut Default::default()).0;
        assert_eq!(
            a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
}
