//! The voices table: each kind of event can play its own instrument, and every bounce can play
//! the next note of a MIDI melody. Each voice holds at most [`MAX_VOICES`] notes at once; when
//! a new note needs room, the oldest one fades out quickly.

use crate::instruments::Voice;
use crate::{ESCAPE_STRUM, MixOptions, PlayedNote, Progression, SAMPLE_RATE, STRUM_SPACING, Soundtrack, mix_notes, mix64};
use bpe_music::midi::Step;
use bpe_scene::{Advance, EventFilter, Instrument, PitchMode, Scene, VoiceMap};
use bpe_sim::{Event, EventKind};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Most notes one voice plays at the same time.
pub const MAX_VOICES: usize = 64;
/// Frames a stolen note takes to fade out (5 ms).
const STEAL_FADE: usize = SAMPLE_RATE as usize / 200;
/// Strength of events that have no impact speed, such as beats.
const PLAIN_VELOCITY: f32 = 0.8;

/// Whether a scene uses none of the voices table or melody, so it plays exactly as scenes did
/// before they existed.
pub(crate) fn plays_like_before(scene: &Scene) -> bool {
    let classic = matches!(scene.sound.instrument, Instrument::Pluck { .. } | Instrument::Sample { .. });
    classic && scene.sound.voices.is_empty() && scene.music.melody.is_none()
}

/// What happened, as far as choosing a sound goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Wall,
    Obstacle,
    Ball,
    Soft,
    Escape,
    Spawn,
    Beat { downbeat: bool },
}

impl Class {
    pub(crate) fn of(kind: EventKind) -> Option<Class> {
        Some(match kind {
            EventKind::Wall => Class::Wall,
            EventKind::Obstacle(_) => Class::Obstacle,
            EventKind::Ball(_) => Class::Ball,
            EventKind::Soft { .. } => Class::Soft,
            EventKind::Escape(_) => Class::Escape,
            EventKind::Spawn => Class::Spawn,
            EventKind::Beat { downbeat, .. } => Class::Beat { downbeat },
            EventKind::Remove | EventKind::Timer(_) | EventKind::Action { .. } => return None,
        })
    }

    /// Hits and escapes, which are quiet below the scene's slowest sounding impact speed.
    fn is_impact(self) -> bool {
        matches!(self, Class::Wall | Class::Obstacle | Class::Ball | Class::Soft | Class::Escape)
    }

    fn matches(self, filter: EventFilter) -> bool {
        match filter {
            EventFilter::AllHits => self.is_impact(),
            EventFilter::Walls => self == Class::Wall,
            EventFilter::Obstacles => self == Class::Obstacle,
            EventFilter::BallHits => self == Class::Ball,
            EventFilter::SoftHits => self == Class::Soft,
            EventFilter::Escapes => self == Class::Escape,
            EventFilter::Spawns => self == Class::Spawn,
            EventFilter::Beats => matches!(self, Class::Beat { .. }),
            EventFilter::Downbeats => self == Class::Beat { downbeat: true },
            // Rule notes arrive with the rules; until then nothing matches them.
            EventFilter::RuleNotes => false,
        }
    }
}

/// Where the next notes come from: a melody, or the chord progression.
struct Tune {
    melody: Option<Melody>,
    progression: Progression,
    seed: u64,
}

struct Melody {
    steps: Vec<Step>,
    advance: Advance,
    from_impact: bool,
    /// Next step to play, for everyone or (per ball) for each ball.
    cursor: usize,
    per_ball: HashMap<u32, usize>,
}

/// The notes one event plays, before any voice picks its instrument.
struct Chosen {
    keys: Vec<u8>,
    velocity: f32,
    /// Frames between the keys, for strums.
    spacing: usize,
    step: Option<u32>,
}

impl Tune {
    fn new(scene: &Scene, options: &MixOptions) -> anyhow::Result<Self> {
        let melody = match &scene.music.melody {
            Some(m) => {
                let file = bpe_music::midi::load(&m.path, options.base_dir)?;
                let steps = file.steps(m.track, m.transpose);
                (!steps.is_empty()).then(|| Melody {
                    steps,
                    advance: m.advance_on,
                    from_impact: m.velocity_from_impact,
                    cursor: 0,
                    per_ball: HashMap::new(),
                })
            }
            None => None,
        };
        Ok(Self { melody, progression: Progression::new(scene)?, seed: scene.sound.seed })
    }

    /// Notes for event number `index`, which happened at video `time`.
    fn choose(&mut self, index: usize, event: &Event, class: Class, time: f64, impact: f32) -> Option<Chosen> {
        let Some(melody) = &mut self.melody else {
            // Chords, picked exactly as the classic sound picks them.
            let candidates = self.progression.notes_at(time);
            if candidates.is_empty() {
                return None;
            }
            let pick = (mix64(self.seed ^ index as u64) % candidates.len() as u64) as usize;
            let (count, velocity, spacing) =
                if class == Class::Escape { (ESCAPE_STRUM, PLAIN_VELOCITY, STRUM_SPACING) } else { (1, impact, 0) };
            let pick = pick.min(candidates.len().saturating_sub(count));
            let keys = candidates[pick..].iter().take(count).copied().collect();
            return Some(Chosen { keys, velocity, spacing, step: None });
        };
        let len = melody.steps.len();
        let step = match melody.advance {
            Advance::AllHits => post_increment(&mut melody.cursor),
            Advance::WallHits if class == Class::Wall => post_increment(&mut melody.cursor),
            // Other hits repeat the note just played (or the first one, before any).
            Advance::WallHits => melody.cursor.saturating_sub(1),
            Advance::PerBall => post_increment(melody.per_ball.entry(event.ball).or_insert(0)),
        } % len;
        let played = &melody.steps[step];
        let velocity = if melody.from_impact { impact } else { played.velocity as f32 / 127.0 };
        let spacing = if class == Class::Escape { STRUM_SPACING } else { 0 };
        Some(Chosen { keys: played.keys.clone(), velocity, spacing, step: Some(step as u32) })
    }
}

fn post_increment(value: &mut usize) -> usize {
    *value += 1;
    *value - 1
}

/// A row of the voices table, ready to play.
struct Row {
    /// Place in the voices table, counting disabled rows, for the notes lane.
    index: u16,
    voice: Voice,
    filter: EventFilter,
    pitch: PitchMode,
    gain: f32,
}

/// A note chosen but not yet given its sound.
struct Pending {
    row: usize,
    start: usize,
    key: u8,
    velocity: f32,
    left: f32,
    right: f32,
}

struct NoteOn {
    start: usize,
    /// One past the last frame that sounds. Shortened when the note is stolen.
    end: usize,
    stolen: bool,
    wave: Arc<[f32]>,
    left: f32,
    right: f32,
}

/// The rows that play: the voices table, or one row with the scene's instrument for every hit.
fn rows(scene: &Scene, options: &MixOptions) -> anyhow::Result<Vec<Row>> {
    let sound = &scene.sound;
    let fallback = [VoiceMap { instrument: sound.instrument.clone(), ..VoiceMap::default() }];
    let table = if sound.voices.is_empty() { &fallback[..] } else { &sound.voices[..] };
    table
        .iter()
        .enumerate()
        .filter(|(_, v)| v.enabled)
        .map(|(index, v)| {
            Ok(Row {
                index: index as u16,
                voice: Voice::new(&v.instrument, options.base_dir)?,
                filter: v.on,
                pitch: v.pitch,
                gain: 10f32.powf(v.gain / 20.0),
            })
        })
        .collect()
}

pub(crate) fn layer(
    scene: &Scene,
    events: &[Event],
    options: &MixOptions,
    track: &mut Soundtrack,
    played: &mut Vec<PlayedNote>,
) -> anyhow::Result<()> {
    let sound = &scene.sound;
    let mut tune = Tune::new(scene, options)?;
    let mut rows = rows(scene, options)?;
    let (center, half_width) = (scene.container.center().x, scene.container.half_width());
    let speed_range = (sound.full_volume_speed - sound.min_impact_speed).max(1.0);
    let frames = track.frames();
    let mut pending = Vec::new();

    // Chords are picked by bounce number, as the classic sound does.
    let mut bounces = 0usize;
    for event in events {
        let Some(class) = Class::of(event.kind) else { continue };
        let index = bounces;
        if class.is_impact() {
            bounces += 1;
            if event.speed < sound.min_impact_speed {
                continue;
            }
        }
        if !rows.iter().any(|r| class.matches(r.filter)) {
            continue;
        }
        let time = options.video_time(event.time);
        let start = (time * SAMPLE_RATE as f64).round() as usize;
        if start >= frames {
            break;
        }
        let impact = if class.is_impact() {
            let v = ((event.speed - sound.min_impact_speed) / speed_range).clamp(0.0, 1.0);
            v * v.sqrt()
        } else {
            PLAIN_VELOCITY
        };
        // The melody moves on once per event, however many voices play it.
        let wants_tune = rows.iter().any(|r| class.matches(r.filter) && r.pitch == PitchMode::Tune);
        let chosen = if wants_tune { tune.choose(index, event, class, time, impact) } else { None };
        // Beats and other events without a ball sit in the middle.
        let x = if event.ball_index().is_some() { event.position.x - center } else { 0.0 };
        let pan = (x / half_width.max(1.0)).clamp(-1.0, 1.0) * sound.stereo_width;
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        for (r, row) in rows.iter().enumerate() {
            if !class.matches(row.filter) {
                continue;
            }
            let (keys, velocity, spacing, step) = match (row.pitch, &chosen) {
                (PitchMode::Fixed(key), _) => (vec![key], impact, 0, None),
                (PitchMode::Tune, Some(c)) => (c.keys.clone(), c.velocity, c.spacing, c.step),
                (PitchMode::Tune, None) => continue,
            };
            let gain = velocity * row.gain;
            for (n, &key) in keys.iter().enumerate() {
                let start = start + n * spacing;
                if start >= frames {
                    break;
                }
                let (left, right) = (gain * angle.cos(), gain * angle.sin());
                pending.push(Pending { row: r, start, key, velocity, left, right });
                let time = start as f64 / SAMPLE_RATE as f64;
                played.push(PlayedNote { time, key, velocity, voice: row.index, step });
            }
        }
    }

    // Make each row's sounds once, on all cores, then place them.
    for (r, row) in rows.iter_mut().enumerate() {
        let wanted: Vec<(u8, f32)> = pending.iter().filter(|p| p.row == r).map(|p| (p.key, p.velocity)).collect();
        row.voice.prepare(&wanted);
    }
    let mut notes: Vec<Vec<NoteOn>> = rows.iter().map(|_| Vec::new()).collect();
    for p in pending {
        let wave = rows[p.row].voice.note(p.key, p.velocity);
        let end = (p.start + wave.len()).min(frames);
        notes[p.row].push(NoteOn { start: p.start, end, stolen: false, wave, left: p.left, right: p.right });
    }
    for row_notes in &mut notes {
        steal_voices(row_notes);
    }
    // Rows are mixed one after another, each note in time order.
    let all: Vec<NoteOn> = notes.into_iter().flatten().collect();
    mix_notes(&mut track.samples, &all, |n| {
        let fade = if n.stolen { STEAL_FADE.min(n.end - n.start) } else { 0 };
        (n.start, &n.wave[..n.end - n.start], n.left, n.right, fade)
    });
    Ok(())
}

/// Caps how many notes ring at once: when a note starts with [`MAX_VOICES`] still sounding,
/// the oldest is cut short with a quick fade.
fn steal_voices(notes: &mut [NoteOn]) {
    notes.sort_by_key(|n| n.start);
    let mut sounding: VecDeque<usize> = VecDeque::new();
    for i in 0..notes.len() {
        let start = notes[i].start;
        sounding.retain(|&j| notes[j].end > start);
        if sounding.len() >= MAX_VOICES
            && let Some(oldest) = sounding.pop_front()
        {
            let note = &mut notes[oldest];
            note.end = note.end.min(start + STEAL_FADE);
            note.stolen = true;
        }
        sounding.push_back(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Instrument, Melody};

    fn note(start: usize, len: usize) -> NoteOn {
        NoteOn { start, end: start + len, stolen: false, wave: vec![1.0; len].into(), left: 1.0, right: 1.0 }
    }

    #[test]
    fn the_oldest_note_is_stolen_when_too_many_ring() {
        let mut notes: Vec<NoteOn> = (0..MAX_VOICES + 2).map(|i| note(i * 10, 100_000)).collect();
        steal_voices(&mut notes);
        assert!(notes[0].stolen && notes[1].stolen && !notes[2].stolen);
        assert_eq!(notes[0].end, MAX_VOICES * 10 + STEAL_FADE);
        // Notes that ended on their own don't count.
        let mut short: Vec<NoteOn> = (0..200).map(|i| note(i * 100, 50)).collect();
        steal_voices(&mut short);
        assert!(short.iter().all(|n| !n.stolen));
    }

    #[test]
    fn stolen_notes_fade_instead_of_clicking() {
        let mut out = vec![0.0; 2000];
        let wave = vec![1.0f32; 1000];
        mix_notes(&mut out, &[()], |_| (0, &wave[..400], 1.0, 1.0, STEAL_FADE));
        assert_eq!(out[0], 1.0);
        assert!(out[399 * 2] < 0.01 && out[399 * 2] > 0.0);
        assert!(out[(400 - STEAL_FADE / 2) * 2] > 0.4 && out[(400 - STEAL_FADE / 2) * 2] < 0.6);
        assert_eq!(out[400 * 2], 0.0);
    }

    fn melody_scene(advance: Advance) -> Scene {
        let mut scene = Scene::default();
        scene.music.melody = Some(Melody { advance_on: advance, ..Melody::default() });
        scene
    }

    fn hit(time: f64, ball: u32, kind: EventKind) -> Event {
        Event { time, ball, position: bpe_scene::Vec2::ZERO, depth: 0.0, speed: 500.0, kind }
    }

    fn keys(scene: &Scene, events: &[Event]) -> Vec<u8> {
        crate::mix(scene, events, &MixOptions::default()).unwrap().notes.iter().map(|n| n.key).collect()
    }

    #[test]
    fn each_hit_plays_the_next_note_of_the_melody() {
        let events: Vec<Event> = (0..5).map(|i| hit(0.5 + i as f64 * 0.3, 0, EventKind::Wall)).collect();
        // Ode to Joy: E E F G G.
        assert_eq!(keys(&melody_scene(Advance::AllHits), &events), vec![64, 64, 65, 67, 67]);
    }

    #[test]
    fn melody_loops_and_advances_as_asked() {
        let scene = melody_scene(Advance::AllHits);
        let file = bpe_music::midi::load("builtin:ode-to-joy", None).unwrap();
        let len = file.steps(None, 0).len();
        let mut long: Vec<Event> = (0..len + 1).map(|i| hit(0.1 + i as f64 * 0.02, 0, EventKind::Wall)).collect();
        long.iter_mut().for_each(|h| h.time *= 0.5);
        let played = keys(&scene, &long);
        assert_eq!(played[len], played[0]);

        // Only wall hits move on; a peg hit repeats the last note.
        let mixed = [hit(0.5, 0, EventKind::Wall), hit(0.6, 0, EventKind::Obstacle(0)), hit(0.7, 0, EventKind::Wall), hit(0.8, 0, EventKind::Wall)];
        assert_eq!(keys(&melody_scene(Advance::WallHits), &mixed), vec![64, 64, 64, 65]);

        // Each ball plays the tune on its own.
        let balls = [hit(0.5, 0, EventKind::Wall), hit(0.6, 1, EventKind::Wall), hit(0.7, 0, EventKind::Wall), hit(0.8, 0, EventKind::Wall)];
        assert_eq!(keys(&melody_scene(Advance::PerBall), &balls), vec![64, 64, 64, 65]);
    }

    #[test]
    fn voices_split_events_by_kind() {
        let mut scene = Scene::escape_rings();
        scene.sound.voices = vec![
            VoiceMap { on: EventFilter::Walls, instrument: Instrument::library("Drum kit"), pitch: PitchMode::Fixed(36), ..VoiceMap::default() },
            VoiceMap { on: EventFilter::Escapes, instrument: Instrument::library("Drum kit"), pitch: PitchMode::Fixed(49), ..VoiceMap::default() },
            VoiceMap { enabled: false, ..VoiceMap::default() },
        ];
        let hits = bpe_sim::collect_wall_hits(&scene);
        let mix = crate::mix(&scene, &hits, &MixOptions::default()).unwrap();
        let escapes = hits.iter().filter(|h| matches!(h.kind, EventKind::Escape(_))).count();
        assert!(escapes > 0);
        assert_eq!(mix.notes.iter().filter(|n| n.key == 49).count(), escapes);
        assert!(mix.notes.iter().filter(|n| n.key == 36).all(|n| n.voice == 0));
        assert!(mix.notes.iter().filter(|n| n.key == 49).all(|n| n.voice == 1));
        let peak = mix.track.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1 && peak <= 0.9);
    }

    #[test]
    fn the_same_scene_mixes_the_same_every_time() {
        let mut scene = Scene::default();
        scene.sound.voices = vec![
            VoiceMap { instrument: Instrument::library("Marimba"), ..VoiceMap::default() },
            VoiceMap { instrument: Instrument::library("Bells"), gain: -6.0, ..VoiceMap::default() },
        ];
        scene.video.duration = 6.0;
        let hits = bpe_sim::collect_wall_hits(&scene);
        let a = crate::mix(&scene, &hits, &MixOptions::default()).unwrap();
        let b = crate::mix(&scene, &hits, &MixOptions::default()).unwrap();
        assert_eq!(a.track.samples, b.track.samples);
        assert_eq!(a.notes, b.notes);
    }
}
