//! The modular Ball Studio's sound: instruments, chords and the mixer that turns the event log
//! into a soundtrack.
//!
//! Each Sound module turns the events of the things that use it into [`Note`]s; the mixer
//! renders every note once, adds them up in a fixed order, sets the level and limits the peaks.
//! The same log and settings give the same samples every time.

pub mod chords;
pub mod instruments;
pub mod level;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bpe_core::{Params, Resolved, SlotId, Target, ThingId};
use bpe_runtime::{Event, Log};

pub use chords::{Chords, pitch_classes};
pub use instruments::{Instrument, NoteCache};
pub use level::{limit_peaks, mix_notes, mix64, normalize_loudness};

pub const SAMPLE_RATE: u32 = 48_000;

/// Interleaved stereo audio at [`SAMPLE_RATE`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Soundtrack {
    pub samples: Vec<f32>,
}

impl Soundtrack {
    pub fn silent(seconds: f32) -> Self {
        Self { samples: vec![0.0; (seconds.max(0.0) * SAMPLE_RATE as f32) as usize * 2] }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn write_wav(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path.as_ref(), spec)?;
        for &s in &self.samples {
            writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        writer.finalize()?;
        Ok(())
    }
}

/// One note to play.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// The frame it starts on.
    pub frame: usize,
    pub instrument: Instrument,
    /// MIDI key.
    pub key: u8,
    /// How hard it is played, 0 to 1.
    pub velocity: f32,
    /// Gains per channel.
    pub left: f32,
    pub right: f32,
    /// The pluck's decay in seconds and brightness; other instruments ignore them.
    pub decay: f32,
    pub brightness: f32,
}

/// What the whole mix needs from the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct MixSettings {
    pub duration: f32,
    pub steps_per_second: u32,
    /// The chord text, parsed by each module with its own note range.
    pub chords: String,
    /// Target loudness in dB.
    pub loudness: f32,
    pub seed: u64,
    /// Half the video's width, for panning.
    pub half_width: f32,
    /// Every thing's tags, for sounds that only play against some things.
    pub tags: BTreeMap<ThingId, BTreeSet<String>>,
}

/// What a Sound module can read while turning events into notes.
pub struct MixCx<'a> {
    pub settings: &'a MixSettings,
    /// How many frames the track has.
    pub frames: usize,
    /// An event's position in the log, for repeatable note picks.
    pub hit_index: &'a dyn Fn(&Event) -> u64,
    /// Whether a thing is one a target points at, seen from another: `(target, me, other)`.
    pub matches: &'a dyn Fn(&Target, ThingId, ThingId) -> bool,
}

/// How a Sound module kind turns events into notes.
pub trait ModuleMix: Send + Sync + 'static {
    /// The module kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// Adds the notes for `events`, which are the ones where `thing` is either party: the one
    /// that hit (`a`) or the one that was hit (`b`).
    fn notes(&self, params: &Params, thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>);
}

/// The target rules over a resolved scene's kinds and the settings' tags: the same rules as
/// `Scene::target_matches`.
pub fn target_matches(
    resolved: &Resolved,
    settings: &MixSettings,
    target: &Target,
    me: ThingId,
    other: ThingId,
) -> bool {
    match target {
        Target::Anything => true,
        Target::Me => other == me && resolved.things.contains_key(&other),
        Target::Thing(t) => other == *t && resolved.things.contains_key(&other),
        Target::Tag(tag) => settings.tags.get(&other).is_some_and(|t| t.contains(tag)),
        Target::Kind(kind) => resolved.things.get(&other).is_some_and(|r| r.kind == *kind),
    }
}

/// Every registered sound part, by kind id.
#[derive(Default)]
pub struct Mixes {
    modules: BTreeMap<&'static str, Box<dyn ModuleMix>>,
}

impl Mixes {
    pub fn add(&mut self, m: impl ModuleMix) {
        let id = m.kind();
        assert!(!self.modules.contains_key(id), "mix registered twice: {id}");
        self.modules.insert(id, Box::new(m));
    }

    pub fn module(&self, kind: &str) -> Option<&dyn ModuleMix> {
        self.modules.get(kind).map(|m| m.as_ref())
    }
}

/// The soundtrack of a run: every thing's Sound module turns its events into notes, in thing
/// order; the notes are rendered once each, added up, levelled and limited.
pub fn mix(settings: &MixSettings, resolved: &Resolved, log: &Log, mixes: &Mixes) -> Soundtrack {
    let mut track = Soundtrack::silent(settings.duration);
    let frames = track.frames();
    let hit_index = |e: &Event| -> u64 {
        let all = log.events();
        let start = all.partition_point(|x| x.step < e.step);
        let within = all[start..].iter().position(|x| x == e).unwrap_or(0);
        (start + within) as u64
    };
    let matches = |t: &Target, me: ThingId, other: ThingId| target_matches(resolved, settings, t, me, other);
    let cx = MixCx { settings, frames, hit_index: &hit_index, matches: &matches };
    let mut notes = Vec::new();
    for (id, r) in &resolved.things {
        let Some(slot) = r.slots.get(&SlotId::Sound) else { continue };
        let Some(m) = mixes.module(&slot.kind) else { continue };
        let party = |e: &Event| e.a.thing == *id || e.b.is_some_and(|b| b.thing == *id);
        let events: Vec<Event> = log.events().iter().filter(|e| party(e)).cloned().collect();
        if events.is_empty() {
            continue;
        }
        m.notes(&slot.params, *id, &events, &cx, &mut notes);
    }
    notes.retain(|n| n.frame < frames);
    if notes.is_empty() {
        return track;
    }
    let mut cache = NoteCache::default();
    cache.prepare(&notes);
    let placed: Vec<(Note, std::sync::Arc<Vec<f32>>)> = notes
        .into_iter()
        .map(|n| {
            let wave = cache.get(n.instrument, n.key, n.velocity, n.decay, n.brightness);
            (n, wave)
        })
        .collect();
    mix_notes(&mut track.samples, &placed, |(n, wave)| (n.frame, wave.as_slice(), n.left, n.right, 0));
    normalize_loudness(&mut track.samples, settings.loudness);
    limit_peaks(&mut track.samples);
    track
}
