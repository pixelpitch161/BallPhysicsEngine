//! Rendering notes channel by channel: each sound module is a channel, played through its
//! instrument. Built-in instruments place cached one-shot notes; patches run voices. Channels
//! render side by side on all cores and are added up in channel order, so the mix is the same
//! every time.

use crate::{Instrument, MixSettings, Note, NoteCache, NoteInstrument, SAMPLE_RATE, mix_notes};
use bpe_core::{Choice, InstrumentId, InstrumentSource, VoiceSettings};
use bpe_synth::{PlayNote, Program, global_steals, render_channel};
use std::collections::{BTreeMap, BTreeSet};
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
        InstrumentChoice::BuiltIn { which: Instrument::Pluck, decay: 0.3, brightness: 0.5 }
    }

    /// A library instrument as it plays, with a module's own `macros` applied to a patch.
    pub fn from_library(i: &bpe_core::Instrument, macros: &BTreeMap<String, f32>) -> Self {
        match &i.source {
            InstrumentSource::BuiltIn { which, knobs } => InstrumentChoice::BuiltIn {
                which: Instrument::from_name(which).unwrap_or_default(),
                decay: knobs.get("decay").copied().unwrap_or(0.3),
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
        NoteInstrument::BuiltIn(which) => InstrumentChoice::BuiltIn { which, decay: 0.3, brightness: 0.5 },
        NoteInstrument::Library(id) => instrument_for(settings, id, n.channel),
    }
}

/// A channel and the instrument some of its notes play through.
type GroupKey = (u32, NoteInstrument);
/// A compiled patch, how it plays notes, and the notes it plays.
type PatchJob = (Program, VoiceSettings, Vec<PlayNote>);

/// The notes of one channel that share an instrument.
struct Group<'a> {
    choice: InstrumentChoice,
    notes: Vec<&'a Note>,
}

/// Rendered channels added up, and plain sentences about anything that went wrong.
pub(crate) struct Rendered {
    pub samples: Vec<f32>,
    pub problems: Vec<String>,
}

/// Plays every note through its channel's instrument into `frames` frames of interleaved stereo.
pub(crate) fn render(settings: &MixSettings, notes: &[Note], frames: usize) -> Rendered {
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for n in notes.iter().filter(|n| n.frame < frames) {
        groups
            .entry((n.channel, n.instrument))
            .or_insert_with(|| Group { choice: resolve(settings, n), notes: Vec::new() })
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

    // Built-in channels: one-shot notes from the cache.
    let mut cache = NoteCache::default();
    cache.prepare(groups.values().flat_map(|g| match g.choice {
        InstrumentChoice::BuiltIn { which, decay, brightness } => {
            g.notes.iter().map(|n| (which, n.key, n.velocity, decay, brightness)).collect()
        }
        InstrumentChoice::Patch { .. } => Vec::new(),
    }));
    let mut channels: BTreeMap<GroupKey, Vec<f32>> = BTreeMap::new();
    for (key, g) in &groups {
        let InstrumentChoice::BuiltIn { which, decay, brightness } = g.choice else { continue };
        let placed: Vec<(&Note, Arc<Vec<f32>>)> =
            g.notes.iter().map(|n| (*n, cache.get(which, n.key, n.velocity, decay, brightness))).collect();
        let mut samples = vec![0.0; frames * 2];
        mix_notes(&mut samples, &placed, |(n, wave)| (n.frame, wave.as_slice(), n.left, n.right, 0));
        channels.insert(*key, samples);
    }

    // Patch channels, side by side on all cores.
    let jobs: Vec<(&GroupKey, &PatchJob)> = programs.iter().collect();
    let next = AtomicUsize::new(0);
    let done = std::sync::Mutex::new(Vec::new());
    let work = || {
        while let Some(&(key, (program, voices, notes))) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
            let audio = render_channel(program, notes, voices, spb, SAMPLE_RATE as f32, frames);
            done.lock().unwrap().push((*key, audio));
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..crate::level::threads().min(jobs.len()) {
            scope.spawn(work);
        }
        work();
    });
    for (key, audio) in done.into_inner().unwrap() {
        if audio.bad_samples
            && let InstrumentChoice::Patch { name, .. } = &groups[&key].choice
        {
            problems.insert(format!("{name} made a bad sample and was stopped"));
        }
        channels.insert(key, audio.samples);
    }

    for g in groups.values() {
        if let InstrumentChoice::Patch { name, graph, .. } = &g.choice {
            missing_samples(name, graph, &mut problems);
        }
    }
    let mut samples = vec![0.0; frames * 2];
    for channel in channels.values() {
        for (o, s) in samples.iter_mut().zip(channel) {
            *o += s;
        }
    }
    Rendered { samples, problems: problems.into_iter().collect() }
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
    fn each_module_renders_as_its_own_channel_and_sums_in_order() {
        let (s, notes) = two_channels();
        let both = render(&s, &notes, 96_000);
        let only = |c: u32| {
            let mine: Vec<Note> = notes.iter().filter(|n| n.channel == c).cloned().collect();
            render(&s, &mine, 96_000).samples
        };
        let (a, b) = (only(3), only(7));
        assert!(a.iter().any(|v| v.abs() > 0.01) && b.iter().any(|v| v.abs() > 0.01));
        for i in 0..both.samples.len() {
            assert!((both.samples[i] - (a[i] + b[i])).abs() < 1e-6, "sample {i}");
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
        let out = render(&s, &good, 48_000);
        assert!(out.samples.iter().any(|v| v.abs() > 0.01) && out.problems.is_empty());
        let out = render(&s, &bad, 48_000);
        assert!(out.samples.iter().all(|v| *v == 0.0));
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
        let new = render(&s, &notes, 96_000).samples;
        // The old way: every note into one track.
        let mut cache = NoteCache::default();
        let placed: Vec<(&Note, Arc<Vec<f32>>)> =
            notes.iter().map(|n| (n, cache.get(Instrument::Marimba, n.key, n.velocity, 0.3, 0.5))).collect();
        let mut old = vec![0.0; 96_000 * 2];
        mix_notes(&mut old, &placed, |(n, wave)| (n.frame, wave.as_slice(), n.left, n.right, 0));
        assert!(new.iter().zip(&old).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn the_mix_is_the_same_twice() {
        let (s, notes) = two_channels();
        let a = render(&s, &notes, 96_000).samples;
        let b = render(&s, &notes, 96_000).samples;
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    }
}
