//! The modular Ball Studio's sound: instruments, chords and the mixer that turns the event log
//! into a soundtrack.
//!
//! Each Sound module turns the events of the things that use it into [`Note`]s; the mixer
//! plays each module's notes as its own channel through the module's instrument (cached
//! one-shot notes for built-ins, voices for synth patches), adds the channels up in a fixed
//! order, sets the level and limits the peaks. The same log and settings give the same samples
//! every time.

pub mod audition;
mod channels;
pub mod chords;
pub mod instruments;
pub mod level;
pub mod presets;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bpe_core::{
    Animation, Inputs, InstrumentId, Library, ModuleGraph, Params, Progression, ProgressionId, ProgressionRef,
    Resolved, SlotId, Target, ThingId,
};
use bpe_runtime::{Event, Log};

pub use audition::{audition, audition_keys};
pub use channels::{ChannelCache, InstrumentChoice, VOICE_CAP, instrument_for};
pub use chords::{
    ChordClock, Function, PicksFrom, borrowed, candidates, chord_name, diatonic, function, intervals, note_name,
    numeral, parse_chord, scale, voicing,
};
pub use instruments::{Instrument, NoteCache};
pub use level::{limit_peaks, mix_notes, mix64, normalize_loudness};
pub use presets::{PRESETS, Preset, moods};

pub const SAMPLE_RATE: u32 = 48_000;

/// Interleaved stereo audio at [`SAMPLE_RATE`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Soundtrack {
    pub samples: Vec<f32>,
    /// What went wrong while playing, in plain sentences for the status line: a patch that
    /// doesn't compile, a voice that made bad samples, a sample file that couldn't be read.
    pub problems: Vec<String>,
}

impl Soundtrack {
    pub fn silent(seconds: f32) -> Self {
        Self { samples: vec![0.0; (seconds.max(0.0) * SAMPLE_RATE as f32) as usize * 2], problems: Vec::new() }
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

/// What a note plays through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteInstrument {
    /// A library instrument, or Scene defaults' one for `None`.
    Library(Option<InstrumentId>),
    /// A built-in sound with its usual knobs, such as the drum machine's kit.
    BuiltIn(Instrument),
}

/// One note to play.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// The frame it starts on.
    pub frame: usize,
    pub instrument: NoteInstrument,
    /// MIDI key.
    pub key: u8,
    /// How hard it is played, 0 to 1.
    pub velocity: f32,
    /// Gains per side.
    pub left: f32,
    pub right: f32,
    /// The sound module that played it (its id), which is the channel it renders in. The mixer
    /// sets it; modules leave it at 0.
    pub channel: u32,
}

/// What the whole mix needs from the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct MixSettings {
    pub duration: f32,
    pub steps_per_second: u32,
    /// Every chord progression in the scene's library.
    pub progressions: BTreeMap<ProgressionId, Progression>,
    /// The one Scene defaults points at; modules set to "Scene's" use it.
    pub scene_chords: Option<ProgressionId>,
    /// The beat grid in video seconds, which chords are laid on.
    pub beats: Vec<f64>,
    /// Target loudness in dB.
    pub loudness: f32,
    pub seed: u64,
    /// Half the video's width, for panning.
    pub half_width: f32,
    /// Every thing's tags, for sounds that only play against some things.
    pub tags: BTreeMap<ThingId, BTreeSet<String>>,
    /// How far past the edge of the shot a hit fades to silence, in shot widths; 0 is off.
    pub off_shot_fade: f32,
    /// Half the video's height, for the shot's shape.
    pub half_height: f32,
    /// Every instrument in the scene's library.
    pub instruments: BTreeMap<InstrumentId, bpe_core::Instrument>,
    /// The one Scene defaults' sound plays through; modules set to "Scene's" use it.
    pub scene_instrument: Option<InstrumentId>,
    /// Each sound module's own macro values for its instrument, by module id.
    pub macros: BTreeMap<u32, BTreeMap<String, f32>>,
}

impl MixSettings {
    /// The progression a setting points at: that one when it is in the library, else Scene
    /// defaults' one, else the starter.
    pub fn progression(&self, r: ProgressionRef) -> &Progression {
        static STARTER: std::sync::LazyLock<Progression> = std::sync::LazyLock::new(Progression::starter);
        r.0.and_then(|id| self.progressions.get(&id))
            .or_else(|| self.scene_chords.and_then(|id| self.progressions.get(&id)))
            .unwrap_or(&STARTER)
    }

    /// Seconds per beat: the middle gap of the beat grid, or half a second without one.
    pub fn seconds_per_beat(&self) -> f32 {
        let mut gaps: Vec<f64> = self.beats.windows(2).map(|w| w[1] - w[0]).filter(|g| *g > 1e-3).collect();
        if gaps.is_empty() {
            return 0.5;
        }
        gaps.sort_by(f64::total_cmp);
        gaps[gaps.len() / 2] as f32
    }
}

/// What a Sound module can read while turning events into notes.
pub struct MixCx<'a> {
    pub settings: &'a MixSettings,
    /// The whole run, for modules that count a thing's hits.
    pub log: &'a Log,
    /// How many frames the track has.
    pub frames: usize,
    /// An event's position in the log, for repeatable note picks.
    pub hit_index: &'a dyn Fn(&Event) -> u64,
    /// Where a moment of the run lands in the video, in seconds: the beat sync's warp, or the
    /// same time when there is none. Notes are placed at the video time.
    pub video_time: &'a dyn Fn(f64) -> f64,
    /// Whether a thing is one a target points at, seen from another: `(target, me, other)`.
    pub matches: &'a dyn Fn(&Target, ThingId, ThingId) -> bool,
}

impl MixCx<'_> {
    /// Where an event sounds from left (-1) to right (1), by where it is on the screen when it
    /// happens, scaled by a sound's stereo `width`.
    pub fn pan_of(&self, e: &Event, width: f32) -> f32 {
        let at = self.log.camera_at(e.step).to_screen(e.point);
        (at.x / self.settings.half_width.max(1.0)).clamp(-1.0, 1.0) * width
    }

    /// How loud an event is for where it is: 1 in the shot, fading to 0 by the Off-shot fade's
    /// distance past its edge. Always 1 when the fade is off.
    pub fn gain_of(&self, e: &Event) -> f32 {
        if self.settings.off_shot_fade <= 0.0 {
            return 1.0;
        }
        let camera = self.log.camera_at(e.step);
        let video = glam::Vec2::new(self.settings.half_width, self.settings.half_height) * 2.0;
        let reach = self.settings.off_shot_fade * video.x / camera.zoom.max(1e-3);
        (1.0 - camera.distance_outside(video, e.point) / reach.max(1e-3)).max(0.0)
    }
}

/// How a Sound module kind turns events into notes.
pub trait ModuleMix: Send + Sync + 'static {
    /// The module kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    /// Adds the notes for `events`, which are the ones where `thing` is either party: the one
    /// that hit (`a`) or the one that was hit (`b`).
    fn notes(&self, params: &Params, thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>);
    /// Like [`ModuleMix::notes`], also given the module's graph when it has one, so an edited
    /// module can run it. The default ignores the graph.
    fn notes_with(
        &self,
        params: &Params,
        _graph: Option<&ModuleGraph>,
        thing: ThingId,
        events: &[Event],
        cx: &MixCx,
        out: &mut Vec<Note>,
    ) {
        self.notes(params, thing, events, cx, out)
    }
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
/// order; each module's notes play as a channel, and the channels are added up, levelled and
/// limited.
pub fn mix(settings: &MixSettings, resolved: &Resolved, log: &Log, mixes: &Mixes) -> Soundtrack {
    mix_with(settings, resolved, &Library::default(), log, mixes, None, &|t| t)
}

/// Keyframed sound settings are read this often: a window of one twentieth of a second.
const WINDOWS_PER_SECOND: u32 = 20;

/// Every note of a run, in thing order. With an `animation`, a thing whose Sound settings are
/// keyframed gets its events in windows of a twentieth of a second, each window's settings
/// taken at the window's start. `video_time` maps a moment of the run to the video. The
/// scene's `library` lets a module whose graph has been edited run it.
pub fn notes_with(
    settings: &MixSettings,
    resolved: &Resolved,
    library: &Library,
    log: &Log,
    mixes: &Mixes,
    animation: Option<&Animation>,
    video_time: &dyn Fn(f64) -> f64,
) -> Vec<Note> {
    let frames = Soundtrack::silent(settings.duration).frames();
    let hit_index = |e: &Event| -> u64 {
        let all = log.events();
        let start = all.partition_point(|x| x.step < e.step);
        let within = all[start..].iter().position(|x| x == e).unwrap_or(0);
        (start + within) as u64
    };
    let matches = |t: &Target, me: ThingId, other: ThingId| target_matches(resolved, settings, t, me, other);
    let cx = MixCx { settings, log, frames, hit_index: &hit_index, video_time, matches: &matches };
    let rate = settings.steps_per_second.max(1);
    let window = (rate / WINDOWS_PER_SECOND).max(1) as u64;
    let animated: std::collections::BTreeSet<ThingId> = animation
        .map(|a| {
            a.destinations().into_iter().filter(|(_, slot, _)| *slot == Some(SlotId::Sound)).map(|d| d.0).collect()
        })
        .unwrap_or_default();
    let mut notes = Vec::new();
    for (id, r) in &resolved.things {
        let Some(slot) = r.slots.get(&SlotId::Sound) else { continue };
        let Some(m) = mixes.module(&slot.kind) else { continue };
        let party = |e: &Event| e.a.thing == *id || e.b.is_some_and(|b| b.thing == *id);
        let events: Vec<Event> = log.events().iter().filter(|e| party(e)).cloned().collect();
        if events.is_empty() {
            continue;
        }
        let graph = library.modules.get(&slot.module).and_then(|m| m.graph.as_ref());
        let first = notes.len();
        match animation.filter(|_| animated.contains(id)) {
            None => m.notes_with(&slot.params, graph, *id, &events, &cx, &mut notes),
            Some(animation) => {
                let mut copy = resolved.clone();
                let mut start = 0;
                while start < events.len() {
                    let first = events[start].step / window * window;
                    let end = start + events[start..].partition_point(|e| e.step < first + window);
                    animation.apply(&mut copy, first as f64 / rate as f64, &Inputs::NONE);
                    let params = copy.things.get(id).and_then(|r| r.slots.get(&SlotId::Sound)).map(|s| &s.params);
                    m.notes_with(params.unwrap_or(&slot.params), graph, *id, &events[start..end], &cx, &mut notes);
                    start = end;
                }
            }
        }
        for n in &mut notes[first..] {
            n.channel = slot.module.0;
        }
    }
    notes.retain(|n| n.frame < frames);
    notes
}

/// [`mix`], with the scene's library, keyframed Sound settings and the video's time warp.
/// See [`notes_with`].
pub fn mix_with(
    settings: &MixSettings,
    resolved: &Resolved,
    library: &Library,
    log: &Log,
    mixes: &Mixes,
    animation: Option<&Animation>,
    video_time: &dyn Fn(f64) -> f64,
) -> Soundtrack {
    mix_cached(settings, resolved, library, log, mixes, animation, video_time, &mut ChannelCache::default())
}

/// [`mix_with`], rendering only the channels that changed since the mix that filled `cache`.
#[allow(clippy::too_many_arguments)]
pub fn mix_cached(
    settings: &MixSettings,
    resolved: &Resolved,
    library: &Library,
    log: &Log,
    mixes: &Mixes,
    animation: Option<&Animation>,
    video_time: &dyn Fn(f64) -> f64,
    cache: &mut ChannelCache,
) -> Soundtrack {
    let mut track = Soundtrack::silent(settings.duration);
    let notes = notes_with(settings, resolved, library, log, mixes, animation, video_time);
    if notes.is_empty() {
        return track;
    }
    let rendered = channels::render(settings, &notes, track.frames(), cache);
    track.samples = rendered.samples;
    track.problems = rendered.problems;
    normalize_loudness(&mut track.samples, settings.loudness);
    limit_peaks(&mut track.samples);
    track
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bpe_core::{Address, Ease, Owner, Scene, Value, resolve, testkit};
    use bpe_runtime::{EventKind, Party};

    /// Plays middle C on every event, as loud as its "volume" setting says.
    struct TestPluck;
    impl ModuleMix for TestPluck {
        fn kind(&self) -> &'static str {
            "sound.pluck"
        }
        fn notes(&self, params: &Params, _thing: ThingId, events: &[Event], cx: &MixCx, out: &mut Vec<Note>) {
            let volume = match params.get("volume") {
                Some(Value::Number(v)) => *v as f32,
                _ => 0.0,
            };
            let gain = 10f32.powf(volume / 20.0);
            for e in events {
                let time = (cx.video_time)(e.time(cx.settings.steps_per_second));
                let frame = (time * SAMPLE_RATE as f64).round() as usize;
                out.push(Note {
                    frame,
                    instrument: NoteInstrument::BuiltIn(Instrument::Pluck),
                    key: 60,
                    velocity: 1.0,
                    left: gain,
                    right: gain,
                    channel: 0,
                });
            }
        }
    }

    fn hit(step: u64) -> Event {
        Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(1), item: 0 },
            b: None,
            point: glam::Vec2::ZERO,
            normal: glam::Vec2::X,
            speed: 400.0,
        }
    }

    pub(crate) fn settings() -> MixSettings {
        MixSettings {
            duration: 5.0,
            steps_per_second: 240,
            progressions: Default::default(),
            scene_chords: None,
            beats: Vec::new(),
            loudness: -14.0,
            seed: 1,
            half_width: 540.0,
            tags: Default::default(),
            off_shot_fade: 0.0,
            half_height: 960.0,
            instruments: Default::default(),
            scene_instrument: None,
            macros: Default::default(),
        }
    }

    /// A scene whose one ball plucks through the root's shared sound module.
    fn plucking_scene() -> (bpe_core::Registry, Scene, Address) {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let pluck = scene.add_module(&reg, "sound.pluck", "Pluck").unwrap();
        scene.link(&reg, ThingId::ROOT, SlotId::Sound, pluck).unwrap();
        let ball = testkit::push(&mut scene, "balls", ThingId::ROOT);
        assert_eq!(ball, ThingId(1));
        (reg, scene, Address { owner: Owner::Module(pluck), slot: None, key: "volume".into() })
    }

    #[test]
    fn a_keyed_volume_plays_the_later_hit_quieter() {
        let (reg, mut scene, volume) = plucking_scene();
        let first = scene.set_key(&reg, &volume, 0.0, Value::Number(0.0)).unwrap();
        scene.set_key(&reg, &volume, 4.0, Value::Number(-40.0)).unwrap();
        scene.set_ease(&volume, first, Ease::Linear);
        let resolved = resolve(&scene, &reg);
        let mut mixes = Mixes::default();
        mixes.add(TestPluck);
        let mut log = Log::default();
        log.push(hit(240));
        log.push(hit(720));
        let animation = Animation::new(&scene, &resolved);
        let notes = notes_with(&settings(), &resolved, &Library::default(), &log, &mixes, Some(&animation), &|t| t);
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0].frame, notes[1].frame), (48_000, 144_000));
        let pluck = *scene.library.modules.keys().next().unwrap();
        assert!(notes.iter().all(|n| n.channel == pluck.0), "notes play in their module's channel");
        assert!((notes[0].left - 10f32.powf(-10.0 / 20.0)).abs() < 1e-3, "{}", notes[0].left);
        assert!((notes[1].left - 10f32.powf(-30.0 / 20.0)).abs() < 1e-3, "{}", notes[1].left);
        // Without the animation both play at the setting's own value.
        let plain = notes_with(&settings(), &resolved, &Library::default(), &log, &mixes, None, &|t| t);
        assert_eq!((plain[0].left, plain[1].left), (1.0, 1.0));
        let track = mix_with(&settings(), &resolved, &Library::default(), &log, &mixes, Some(&animation), &|t| t);
        assert_eq!(track.frames(), 5 * 48_000);
        assert!(track.samples[48_000 * 2..49_000 * 2].iter().any(|s| s.abs() > 0.0));
    }

    #[test]
    fn video_time_moves_a_note() {
        let (reg, scene, _) = plucking_scene();
        let resolved = resolve(&scene, &reg);
        let mut mixes = Mixes::default();
        mixes.add(TestPluck);
        let mut log = Log::default();
        log.push(hit(240));
        let notes = notes_with(&settings(), &resolved, &Library::default(), &log, &mixes, None, &|t| t + 1.0);
        assert_eq!(notes[0].frame, 96_000);
        // A note warped past the end is dropped.
        assert!(notes_with(&settings(), &resolved, &Library::default(), &log, &mixes, None, &|t| t + 10.0).is_empty());
    }
}
