//! Rendering notes channel by channel: each sound module is a channel, played through its
//! instrument. Built-in instruments place cached one-shot notes; patches run voices. Channels
//! render side by side on all cores and are added up in channel order, so the mix is the same
//! every time.

use crate::{Instrument, MixSettings, Note, NoteCache, NoteInstrument, SAMPLE_RATE, mix_notes};
use bpe_core::{Choice, InstrumentId, InstrumentSource, VoiceSettings};
use bpe_synth::{PlayNote, Program, global_steals, render_channel_on};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The most voices sounding at once across the whole mix.
pub const VOICE_CAP: usize = 256;

/// An instrument ready to play.
#[derive(Clone, Debug, PartialEq)]
pub enum InstrumentChoice {
    BuiltIn { which: Instrument, decay: f32, brightness: f32 },
    Patch { name: String, graph: bpe_graph::Graph, voices: VoiceSettings },
}

impl InstrumentChoice {
    /// The built-in pluck with its usual knobs, which anything falls back to.
    pub fn pluck() -> Self {
        InstrumentChoice::BuiltIn { which: Instrument::Pluck, decay: 0.9, brightness: 0.5 }
    }

    /// A library instrument as it plays, with a module's own `macros` applied to a patch.
    pub fn from_library(i: &bpe_core::Instrument, macros: &BTreeMap<String, f32>) -> Self {
        match &i.source {
            InstrumentSource::BuiltIn { which, knobs } => InstrumentChoice::BuiltIn {
                which: Instrument::from_name(which).unwrap_or_default(),
                decay: knobs.get("decay").copied().unwrap_or(0.9),
                brightness: knobs.get("brightness").copied().unwrap_or(0.5),
            },
            InstrumentSource::Patch { graph } => InstrumentChoice::Patch {
                name: i.name.clone(),
                graph: bpe_synth::with_macros(graph, macros),
                voices: i.voices,
            },
        }
    }
}

/// The instrument a module pointing at `id` plays: that one if it's in the library, else Scene
/// defaults' one, else the built-in pluck.
pub fn instrument_for(settings: &MixSettings, id: Option<InstrumentId>, channel: u32) -> InstrumentChoice {
    let found = id
        .and_then(|id| settings.instruments.get(&id))
        .or_else(|| settings.scene_instrument.and_then(|id| settings.instruments.get(&id)));
    let empty = BTreeMap::new();
    let macros = settings.macros.get(&channel).unwrap_or(&empty);
    found.map_or_else(InstrumentChoice::pluck, |i| InstrumentChoice::from_library(i, macros))
}

/// What a group of notes plays through.
fn resolve(settings: &MixSettings, n: &Note) -> InstrumentChoice {
    match n.instrument {
        NoteInstrument::BuiltIn(which) => InstrumentChoice::BuiltIn { which, decay: 0.9, brightness: 0.5 },
        NoteInstrument::Library(id) => instrument_for(settings, id, n.channel),
    }
}

/// A channel and the instrument some of its notes play through.
pub(crate) type GroupKey = (u32, NoteInstrument);
/// A compiled patch, how it plays notes, and the notes it plays.
type PatchJob = (Program, VoiceSettings, Vec<PlayNote>);

/// The notes of one channel that share an instrument.
struct Group<'a> {
    choice: InstrumentChoice,
    /// The instrument's own effects and its name, when it has any.
    fx: Option<(String, &'a bpe_graph::Graph)>,
    notes: Vec<&'a Note>,
}

/// Each channel after its instruments' effects and its inserts, with a fingerprint of what it
/// was made from, and plain sentences about anything that went wrong.
pub(crate) struct Rendered {
    pub channels: BTreeMap<u32, (Arc<Vec<f32>>, u64)>,
    pub problems: Vec<String>,
    /// How many channels were rendered rather than taken from the cache.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fresh: usize,
}

/// Each channel's last render, kept between mixes so an edit re-renders only the channels it
/// changed. A channel is keyed by its module and instrument, and the render is kept with a
/// fingerprint of everything it was made from: its notes (after the mix-wide voice cap), its
/// instrument with the module's macros, the voice settings, the tempo and the length.
/// The same goes for each channel after its inserts, and for each return bus.
#[derive(Default)]
pub struct ChannelCache {
    groups: BTreeMap<GroupKey, CachedChannel>,
    pub(crate) strips: BTreeMap<u32, CachedChannel>,
    pub(crate) buses: BTreeMap<bpe_core::BusId, CachedChannel>,
}

pub(crate) struct CachedChannel {
    pub fingerprint: u64,
    pub samples: Arc<Vec<f32>>,
    /// A voice made a bad sample.
    pub bad_samples: bool,
    /// Effects made a bad sample.
    pub fx_bad: bool,
}

impl ChannelCache {
    fn take(&self, key: &GroupKey, fingerprint: u64) -> Option<(Arc<Vec<f32>>, bool, bool)> {
        self.groups
            .get(key)
            .filter(|c| c.fingerprint == fingerprint)
            .map(|c| (c.samples.clone(), c.bad_samples, c.fx_bad))
    }
}

/// Hashes a fingerprint together with an effects graph's sound.
pub(crate) fn with_fx(fingerprint: u64, fx: Option<&bpe_graph::Graph>) -> u64 {
    let mut h = DefaultHasher::new();
    fingerprint.hash(&mut h);
    fx.map(bpe_synth::sound_key).hash(&mut h);
    h.finish()
}

/// The instrument effects a note plays through, with the instrument's name.
fn instrument_fx<'a>(settings: &'a MixSettings, n: &Note) -> Option<(String, &'a bpe_graph::Graph)> {
    let NoteInstrument::Library(id) = n.instrument else { return None };
    let found = id
        .and_then(|id| settings.instruments.get(&id))
        .or_else(|| settings.scene_instrument.and_then(|id| settings.instruments.get(&id)))?;
    (!found.fx.nodes.is_empty()).then(|| (found.name.clone(), &found.fx))
}

fn hash_note(h: &mut DefaultHasher, frame: usize, key: u8, velocity: f32, left: f32, right: f32) {
    (frame, key, velocity.to_bits(), left.to_bits(), right.to_bits()).hash(h);
}

/// What a built-in channel's render depends on.
fn built_in_fingerprint(choice: &InstrumentChoice, notes: &[&Note], frames: usize) -> u64 {
    let mut h = DefaultHasher::new();
    format!("{choice:?}").hash(&mut h);
    frames.hash(&mut h);
    for n in notes {
        hash_note(&mut h, n.frame, n.key, n.velocity, n.left, n.right);
    }
    h.finish()
}

/// What a patch channel's render depends on; the graph already carries the module's macros.
fn patch_fingerprint(choice: &InstrumentChoice, notes: &[PlayNote], spb: f32, frames: usize) -> u64 {
    let mut h = DefaultHasher::new();
    // Only what sounds: moving a block or renaming the patch keeps the render.
    if let InstrumentChoice::Patch { graph, voices, .. } = choice {
        bpe_synth::sound_key(graph).hash(&mut h);
        format!("{voices:?}").hash(&mut h);
    }
    (spb.to_bits(), frames).hash(&mut h);
    for n in notes {
        hash_note(&mut h, n.frame, n.key, n.velocity, n.left, n.right);
        n.steal_at.hash(&mut h);
    }
    h.finish()
}

/// Plays every note through its channel's instrument into `frames` frames of interleaved stereo,
/// taking channels that haven't changed from `cache` and keeping the new ones there.
pub(crate) fn render(settings: &MixSettings, notes: &[Note], frames: usize, cache: &mut ChannelCache) -> Rendered {
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for n in notes.iter().filter(|n| n.frame < frames) {
        groups
            .entry((n.channel, n.instrument))
            .or_insert_with(|| Group {
                choice: resolve(settings, n),
                fx: instrument_fx(settings, n),
                notes: Vec::new(),
            })
            .notes
            .push(n);
    }
    let mut problems = BTreeSet::new();
    let spb = settings.seconds_per_beat();

    // Compile the patches, and pick the voices the mix-wide cap steals.
    let mut programs: BTreeMap<GroupKey, PatchJob> = BTreeMap::new();
    let mut capped: Vec<(usize, PlayNote)> = Vec::new();
    let mut gates: Vec<usize> = Vec::new();
    for (key, g) in &groups {
        let InstrumentChoice::Patch { name, graph, voices } = &g.choice else { continue };
        match bpe_synth::compile(graph) {
            Ok(program) => {
                let index = gates.len();
                gates.push(voices.note_length.frames(spb, SAMPLE_RATE as f32));
                capped.extend(g.notes.iter().map(|n| (index, play_note(n))));
                programs.insert(*key, (program, *voices, Vec::new()));
            }
            Err(diagnostics) => {
                let first = diagnostics.first().map_or("", |d| d.message.as_str());
                problems.insert(format!("{name}: the patch doesn't compile ({first})"));
            }
        }
    }
    global_steals(&mut capped, |i| gates[i], VOICE_CAP);
    let keys: Vec<GroupKey> = programs.keys().copied().collect();
    for (index, note) in capped {
        if let Some((_, _, notes)) = programs.get_mut(&keys[index]) {
            notes.push(note);
        }
    }

    // What each channel is made from; the ones the cache already holds aren't rendered again.
    let mut fingerprints: BTreeMap<GroupKey, u64> = BTreeMap::new();
    for (key, g) in &groups {
        let fingerprint = match (&g.choice, programs.get(key)) {
            (InstrumentChoice::BuiltIn { .. }, _) => built_in_fingerprint(&g.choice, &g.notes, frames),
            (InstrumentChoice::Patch { .. }, Some((_, _, notes))) => patch_fingerprint(&g.choice, notes, spb, frames),
            (InstrumentChoice::Patch { .. }, None) => continue,
        };
        fingerprints.insert(*key, with_fx(fingerprint, g.fx.as_ref().map(|(_, fx)| *fx)));
    }
    let mut channels: BTreeMap<GroupKey, Arc<Vec<f32>>> = BTreeMap::new();
    let mut bad: BTreeSet<GroupKey> = BTreeSet::new();
    let mut fx_bad: BTreeSet<GroupKey> = BTreeSet::new();
    for (key, &fingerprint) in &fingerprints {
        if let Some((samples, bad_samples, bad_fx)) = cache.take(key, fingerprint) {
            channels.insert(*key, samples);
            if bad_samples {
                bad.insert(*key);
            }
            if bad_fx {
                fx_bad.insert(*key);
            }
        }
    }
    let fresh_before = channels.len();

    // Built-in channels: one-shot notes from the note cache.
    let stale = |key: &GroupKey| !channels.contains_key(key);
    let mut notes_cache = NoteCache::default();
    notes_cache.prepare(groups.iter().filter(|(k, _)| stale(k)).flat_map(|(_, g)| match g.choice {
        InstrumentChoice::BuiltIn { which, decay, brightness } => {
            g.notes.iter().map(|n| (which, n.key, n.velocity, decay, brightness)).collect()
        }
        InstrumentChoice::Patch { .. } => Vec::new(),
    }));
    let mut rendered: Vec<(GroupKey, Vec<f32>, bool)> = Vec::new();
    for (key, g) in groups.iter().filter(|(k, _)| stale(k)) {
        let InstrumentChoice::BuiltIn { which, decay, brightness } = g.choice else { continue };
        let placed: Vec<(&Note, Arc<Vec<f32>>)> =
            g.notes.iter().map(|n| (*n, notes_cache.get(which, n.key, n.velocity, decay, brightness))).collect();
        let mut samples = vec![0.0; frames * 2];
        mix_notes(&mut samples, &placed, |(n, wave)| (n.frame, wave.as_slice(), n.left, n.right, 0));
        rendered.push((*key, samples, false));
    }

    // Patch channels, side by side on all cores.
    let jobs: Vec<(&GroupKey, &PatchJob)> = programs.iter().filter(|(k, _)| stale(k)).collect();
    let next = AtomicUsize::new(0);
    let done = std::sync::Mutex::new(Vec::new());
    // Cores left over when there are fewer channels than cores render one channel's voices
    // side by side, so re-rendering the one channel an edit changed uses the whole machine.
    let cores = crate::level::threads();
    let per_channel = (cores / jobs.len().max(1)).max(1);
    let work = || {
        while let Some(&(key, (program, voices, notes))) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
            let audio = render_channel_on(program, notes, voices, spb, SAMPLE_RATE as f32, frames, per_channel);
            done.lock().unwrap().push((*key, audio));
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..crate::level::threads().min(jobs.len()) {
            scope.spawn(work);
        }
        work();
    });
    rendered.extend(done.into_inner().unwrap().into_iter().map(|(key, audio)| (key, audio.samples, audio.bad_samples)));
    let fresh = rendered.len();
    debug_assert_eq!(fresh + fresh_before, fingerprints.len());
    // The instruments' own effects, over the fresh renders side by side.
    let cx = crate::buses::fx_ctx(settings);
    let mut fx_jobs: Vec<(Vec<f32>, Option<&bpe_graph::Graph>, bool)> = rendered
        .iter_mut()
        .map(|(key, samples, _)| (std::mem::take(samples), groups[key].fx.as_ref().map(|(_, g)| *g), false))
        .collect();
    crate::buses::par_each(&mut fx_jobs, |(samples, fx, bad)| {
        if let Some(fx) = fx {
            *bad = crate::buses::run_fx(fx, samples, cx);
        }
    });
    for ((key, samples, bad_samples), (fx_samples, _, bad_fx)) in rendered.into_iter().zip(fx_jobs) {
        let _ = samples;
        let samples = Arc::new(fx_samples);
        if bad_samples {
            bad.insert(key);
        }
        if bad_fx {
            fx_bad.insert(key);
        }
        cache.groups.insert(
            key,
            CachedChannel { fingerprint: fingerprints[&key], samples: samples.clone(), bad_samples, fx_bad: bad_fx },
        );
        channels.insert(key, samples);
    }
    // Channels gone from the mix leave the cache.
    cache.groups.retain(|key, _| fingerprints.contains_key(key));
    for key in &bad {
        if let InstrumentChoice::Patch { name, .. } = &groups[key].choice {
            problems.insert(format!("{name} made a bad sample and was stopped"));
        }
    }
    for (key, g) in &groups {
        if let Some((name, fx)) = &g.fx {
            let place = format!("{name} effects");
            problems.extend(crate::buses::fx_problem(fx, &place));
            if fx_bad.contains(key) {
                problems.insert(format!("{place} made bad samples"));
            }
        }
    }

    for g in groups.values() {
        if let InstrumentChoice::Patch { name, graph, .. } = &g.choice {
            missing_samples(name, graph, &mut problems);
        }
    }
    let strips = crate::buses::strips(settings, &channels, &fingerprints, frames, cache, &mut problems);
    Rendered { channels: strips, problems: problems.into_iter().collect(), fresh }
}

#[cfg(test)]
impl Rendered {
    /// The channels added up.
    pub fn mixed(&self) -> Vec<f32> {
        let len = self.channels.values().map(|(s, _)| s.len()).max().unwrap_or(0);
        let mut out = vec![0.0f32; len];
        for (s, _) in self.channels.values() {
            out.iter_mut().zip(s.iter()).for_each(|(o, v)| *o += v);
        }
        out
    }
}

fn play_note(n: &Note) -> PlayNote {
    PlayNote { frame: n.frame, key: n.key, velocity: n.velocity, left: n.left, right: n.right, steal_at: None }
}

/// Adds a line for each sample file a patch's samplers couldn't read.
fn missing_samples(name: &str, graph: &bpe_graph::Graph, problems: &mut BTreeSet<String>) {
    let missing = bpe_synth::missing_samples();
    for node in graph.nodes.iter().filter(|n| n.kind == "sampler") {
        if let Some(bpe_graph::Param::Text(file)) = node.params.get("file")
            && !file.is_empty()
            && missing.iter().any(|m| m == file)
        {
            problems.insert(format!("{name}: can't read the sample {file}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::settings;
    use bpe_core::{Instrument as LibraryInstrument, InstrumentId, NoteLength};

    fn note(frame: usize, key: u8, channel: u32, instrument: NoteInstrument) -> Note {
        Note { frame, instrument, key, velocity: 0.8, left: 0.7, right: 0.5, channel }
    }

    fn patch(name: &str, graph: bpe_graph::Graph) -> LibraryInstrument {
        LibraryInstrument {
            name: name.into(),
            source: InstrumentSource::Patch { graph },
            voices: VoiceSettings { note_length: NoteLength::Seconds(0.2), ..VoiceSettings::default() },
            fx: bpe_graph::Graph::default(),
        }
    }

    /// A pluck channel and a patch channel.
    fn two_channels() -> (MixSettings, Vec<Note>) {
        let mut s = settings();
        s.instruments.insert(InstrumentId(0), patch("Saw", bpe_synth::blank_patch()));
        let pluck = NoteInstrument::BuiltIn(Instrument::Pluck);
        let saw = NoteInstrument::Library(Some(InstrumentId(0)));
        let notes = (0..20)
            .flat_map(|i| [note(i * 4_800, 60 + (i % 5) as u8, 3, pluck), note(i * 4_800 + 100, 48, 7, saw)])
            .collect();
        (s, notes)
    }

    #[test]
    fn changing_one_channel_re_renders_only_that_channel() {
        let (mut s, mut notes) = two_channels();
        let mut cache = ChannelCache::default();
        assert_eq!(render(&s, &notes, 96_000, &mut cache).fresh, 2);
        assert_eq!(render(&s, &notes, 96_000, &mut cache).fresh, 0, "nothing changed");
        // A louder pluck note touches only the pluck channel.
        notes[0].left *= 0.5;
        assert_eq!(render(&s, &notes, 96_000, &mut cache).fresh, 1);
        // A macro on the patch module re-renders the patch channel only.
        let knob = bpe_synth::macros(&bpe_synth::starters()[0].1);
        s.instruments.insert(InstrumentId(0), patch("Saw", bpe_synth::starters()[0].1.clone()));
        assert_eq!(render(&s, &notes, 96_000, &mut cache).fresh, 1);
        s.macros.insert(7, [(knob[0].key(), knob[0].min)].into());
        assert_eq!(render(&s, &notes, 96_000, &mut cache).fresh, 1);
        // So do its voice settings.
        if let Some(i) = s.instruments.get_mut(&InstrumentId(0)) {
            i.voices.limit = 2;
        }
        assert_eq!(render(&s, &notes, 96_000, &mut cache).fresh, 1);
        // A channel that leaves the mix leaves the cache.
        notes.retain(|n| n.channel == 3);
        render(&s, &notes, 96_000, &mut cache);
        assert_eq!(cache.groups.len(), 1);
    }

    #[test]
    fn moving_a_block_keeps_the_cached_channel() {
        let (mut s, notes) = two_channels();
        let mut cache = ChannelCache::default();
        render(&s, &notes, 96_000, &mut cache);
        if let Some(LibraryInstrument { source: InstrumentSource::Patch { graph }, name, .. }) =
            s.instruments.get_mut(&InstrumentId(0))
        {
            graph.nodes[0].pos = [500.0, 500.0];
            graph.notes.push(bpe_graph::Note { text: "hi".into(), ..Default::default() });
            *name = "Renamed".into();
        }
        assert_eq!(render(&s, &notes, 96_000, &mut cache).fresh, 0);
    }

    #[test]
    fn cached_and_uncached_mixes_match() {
        let (s, mut notes) = two_channels();
        let mut cache = ChannelCache::default();
        render(&s, &notes, 96_000, &mut cache);
        notes[1].velocity = 0.3;
        let cached = render(&s, &notes, 96_000, &mut cache);
        let fresh = render(&s, &notes, 96_000, &mut ChannelCache::default());
        assert_eq!(cached.fresh, 1);
        assert!(cached.mixed() == fresh.mixed(), "bit for bit");
        assert_eq!(cached.problems, fresh.problems);
    }

    /// Run by hand in release: `cargo test -p bpe-mix --release -- --ignored a_busy_minute`.
    #[test]
    #[ignore]
    fn a_busy_minute_renders_quickly() {
        use bpe_graph::Catalog;
        use std::time::Instant;
        // Three unison saws through a filter, with one envelope on the amp and one on the cutoff.
        let cat = bpe_synth::catalog();
        let mut graph = bpe_synth::blank_patch();
        let id = |g: &bpe_graph::Graph, kind: &str| g.nodes.iter().find(|n| n.kind == kind).unwrap().id;
        let (osc, filter) = (id(&graph, "osc"), id(&graph, "filter"));
        graph.node_mut(osc).unwrap().params.insert("unison".into(), bpe_graph::Param::Number(3.0));
        graph.node_mut(filter).unwrap().exposed.insert("cutoff".into());
        let sweep = graph.add(cat.spec("adsr").unwrap(), [360.0, 320.0]);
        graph.connect(bpe_graph::Link { from: (sweep, 0), to: (filter, 1) }, cat);
        let mut s = settings();
        s.duration = 60.0;
        let mut i = patch("Busy", graph);
        i.voices.note_length = NoteLength::Seconds(0.4);
        s.instruments.insert(InstrumentId(0), i);
        let frames = 60 * SAMPLE_RATE as usize;
        let played = NoteInstrument::Library(Some(InstrumentId(0)));
        let notes: Vec<Note> = (0..2_000)
            .map(|i| note(i * (frames - 48_000) / 2_000, 40 + (i * 7 % 36) as u8, [3, 7][i % 2], played))
            .collect();
        // Each channel has a reverb and a delay insert, and sends to both starter buses.
        let fx = |kind: &str| (kind.to_owned(), Default::default(), true);
        let inserts = bpe_synth::chain_graph(&[fx("reverb"), fx("delay")]);
        let (reverb, delay) = (bpe_core::BusId(0), bpe_core::BusId(1));
        s.buses = vec![
            bpe_core::Bus {
                id: reverb,
                name: "Reverb".into(),
                volume: 0.0,
                fx: bpe_synth::chain_graph(&[fx("reverb")]),
            },
            bpe_core::Bus { id: delay, name: "Delay".into(), volume: 0.0, fx: bpe_synth::chain_graph(&[fx("delay")]) },
        ];
        for channel in [3, 7] {
            let sends = [(reverb, -12.0), (delay, -12.0)].into();
            s.strips.insert(channel, bpe_core::ChannelStrip { inserts: inserts.clone(), sends });
        }
        let mut cache = ChannelCache::default();
        let full = |s: &MixSettings, cache: &mut ChannelCache| {
            let (mut mix, _, _) = crate::buses::premaster_from_notes(s, &notes, frames, cache);
            crate::master(&mut mix, &s.master);
        };
        let started = Instant::now();
        full(&s, &mut cache);
        let took = started.elapsed().as_secs_f32();
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f32;
        let budget = 4.0 * 8.0 / cores;
        println!("a busy minute with effects: {took:.2} s on {cores} cores (budget {budget:.2} s)");
        assert!(took < budget, "{took:.2} s");
        // One macro on one of the two channels re-renders only that one; the buses and the
        // master run again.
        s.macros.insert(7, [(format!("{filter}.cutoff"), 900.0)].into());
        let started = Instant::now();
        let again = render(&s, &notes, frames, &mut cache);
        assert_eq!(again.fresh, 1);
        s.macros.insert(7, [(format!("{filter}.cutoff"), 1_200.0)].into());
        let started_full = Instant::now();
        full(&s, &mut cache);
        let took = started_full.elapsed().as_secs_f32();
        println!("one macro changed: {took:.2} s (channel only {:.2} s)", started.elapsed().as_secs_f32() - took);
        assert!(took < 1.0 * 8.0 / cores.min(8.0), "{took:.2} s");
    }

    #[test]
    fn each_module_renders_as_its_own_channel_and_sums_in_order() {
        let (s, notes) = two_channels();
        let both = render(&s, &notes, 96_000, &mut ChannelCache::default());
        let only = |c: u32| {
            let mine: Vec<Note> = notes.iter().filter(|n| n.channel == c).cloned().collect();
            render(&s, &mine, 96_000, &mut ChannelCache::default()).mixed()
        };
        let (a, b) = (only(3), only(7));
        assert!(a.iter().any(|v| v.abs() > 0.01) && b.iter().any(|v| v.abs() > 0.01));
        for i in 0..both.mixed().len() {
            assert!((both.mixed()[i] - (a[i] + b[i])).abs() < 1e-6, "sample {i}");
        }
    }

    #[test]
    fn a_patch_channel_sounds_and_a_broken_one_is_silent_with_a_problem() {
        let mut s = settings();
        let mut broken = bpe_synth::blank_patch();
        broken.nodes.retain(|n| n.kind != "voice_out");
        s.instruments.insert(InstrumentId(0), patch("Good", bpe_synth::blank_patch()));
        s.instruments.insert(InstrumentId(1), patch("Broken", broken));
        let good = [note(0, 60, 1, NoteInstrument::Library(Some(InstrumentId(0))))];
        let bad = [note(0, 60, 2, NoteInstrument::Library(Some(InstrumentId(1))))];
        let out = render(&s, &good, 48_000, &mut ChannelCache::default());
        assert!(out.mixed().iter().any(|v| v.abs() > 0.01) && out.problems.is_empty());
        let out = render(&s, &bad, 48_000, &mut ChannelCache::default());
        assert!(out.mixed().iter().all(|v| *v == 0.0));
        assert_eq!(out.problems, ["Broken: the patch doesn't compile (Add a Voice out block)"]);
    }

    #[test]
    fn a_missing_instrument_plays_scene_defaults() {
        let mut s = settings();
        assert_eq!(instrument_for(&s, Some(InstrumentId(9)), 1), InstrumentChoice::pluck());
        s.instruments.insert(InstrumentId(2), patch("Scene's", bpe_synth::blank_patch()));
        s.scene_instrument = Some(InstrumentId(2));
        let InstrumentChoice::Patch { name, .. } = instrument_for(&s, Some(InstrumentId(9)), 1) else { panic!() };
        assert_eq!(name, "Scene's");
        let InstrumentChoice::Patch { name, .. } = instrument_for(&s, None, 1) else { panic!() };
        assert_eq!(name, "Scene's");
    }

    #[test]
    fn module_macros_change_the_patch() {
        let mut s = settings();
        let mut graph = bpe_synth::blank_patch();
        let filter = graph.nodes.iter().find(|n| n.kind == "filter").unwrap().id;
        graph.node_mut(filter).unwrap().exposed.insert("cutoff".into());
        s.instruments.insert(InstrumentId(0), patch("Saw", graph));
        s.macros.insert(4, [(format!("{filter}.cutoff"), 300.0)].into());
        let InstrumentChoice::Patch { graph, .. } = instrument_for(&s, Some(InstrumentId(0)), 4) else { panic!() };
        assert_eq!(bpe_synth::macros(&graph)[0].value, 300.0);
        let InstrumentChoice::Patch { graph, .. } = instrument_for(&s, Some(InstrumentId(0)), 5) else { panic!() };
        assert_eq!(bpe_synth::macros(&graph)[0].value, 2_000.0);
    }

    #[test]
    fn built_in_channels_render_as_before() {
        let s = settings();
        let notes: Vec<Note> = (0..30)
            .map(|i| note(i * 3_000, 50 + (i % 12) as u8, (i % 3) as u32, NoteInstrument::BuiltIn(Instrument::Marimba)))
            .collect();
        let new = render(&s, &notes, 96_000, &mut ChannelCache::default()).mixed();
        // The old way: every note into one track.
        let mut cache = NoteCache::default();
        let placed: Vec<(&Note, Arc<Vec<f32>>)> =
            notes.iter().map(|n| (n, cache.get(Instrument::Marimba, n.key, n.velocity, 0.9, 0.5))).collect();
        let mut old = vec![0.0; 96_000 * 2];
        mix_notes(&mut old, &placed, |(n, wave)| (n.frame, wave.as_slice(), n.left, n.right, 0));
        assert!(new.iter().zip(&old).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn the_mix_is_the_same_twice() {
        let (s, notes) = two_channels();
        let a = render(&s, &notes, 96_000, &mut ChannelCache::default()).mixed();
        let b = render(&s, &notes, 96_000, &mut ChannelCache::default()).mixed();
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    }
}
