//! What the run gives input blocks: each hit's values and the ball's track after it, and the
//! scene-wide series on the video clock. Only built for what some patch or effect reads.

use std::collections::BTreeMap;
use std::sync::Arc;

use bpe_core::{Color, ProgressionRef, Quality, Resolved, Scene, Settings, SlotId, ThingId};
use bpe_mix::{ChordClock, MixSettings, PhysicsInputs};
use bpe_runtime::{Event, EventKind, Log, MOTION_RATE, Party, TRACK_SECONDS, TrackSample};
use bpe_synth::{HitPhysics, SceneSignals, Track};
use glam::Vec2;

use crate::modules::sound::NoteSound;
use crate::paint::{Inputs, colour_at};
use crate::{ClassicLook, StyledLook, music};

/// How far back Hits per second counts, in seconds.
const HIT_RATE_WINDOW: f64 = 0.5;

/// The synth patches in the scene's library.
fn patches(scene: &Scene) -> impl Iterator<Item = &bpe_graph::Graph> {
    scene.library.instruments.values().filter_map(|i| match &i.source {
        bpe_core::InstrumentSource::Patch { graph } => Some(graph),
        _ => None,
    })
}

/// Whether some instrument patch follows a ball while its note sounds, so the run has to
/// record tracks.
pub fn uses_live_inputs(scene: &Scene) -> bool {
    patches(scene).any(|g| bpe_synth::uses(g).live)
}

/// Whether some instrument patch reads anything about the hit that started its note.
fn uses_hit_inputs(scene: &Scene) -> bool {
    patches(scene).any(|g| {
        let u = bpe_synth::uses(g);
        u.hit || u.live
    })
}

/// Where the screen point `s` sits in the frame: height (1 = top) and left-right (1 = right).
fn in_frame(s: Vec2, settings: &MixSettings) -> (f32, f32) {
    let h = (0.5 - s.y / (2.0 * settings.half_height.max(1.0))).clamp(0.0, 1.0);
    let x = (0.5 + s.x / (2.0 * settings.half_width.max(1.0))).clamp(0.0, 1.0);
    (h, x)
}

/// 0 top, 1/3 bottom, 2/3 left, 1 right: the side of the ball the contact was on.
fn side(normal: Vec2) -> f32 {
    let c = -normal;
    if c.y.abs() >= c.x.abs() {
        if c.y < 0.0 { 0.0 } else { 1.0 / 3.0 }
    } else if c.x < 0.0 {
        2.0 / 3.0
    } else {
        1.0
    }
}

/// Degrees between the bounce-off velocity and the surface: 0 grazing, 90 straight on.
fn angle(velocity: Vec2, normal: Vec2) -> f32 {
    let n = normal.normalize_or_zero();
    if n == Vec2::ZERO || velocity == Vec2::ZERO {
        return 0.0;
    }
    let along = velocity.dot(n);
    let across = (velocity - along * n).length();
    along.abs().atan2(across).to_degrees()
}

/// The fill paint of a thing's classic or styled look.
fn fill_of(resolved: &Resolved, thing: ThingId) -> Option<(crate::paint::Paint, bool)> {
    let r = resolved.things.get(&thing)?;
    let slot = r.slots.get(&SlotId::Look)?;
    let fill = match slot.kind.as_str() {
        "look.classic" => ClassicLook::from_params(&slot.params).fill,
        "look.styled" => StyledLook::from_params(&slot.params).fill,
        _ => return None,
    };
    Some((crate::modules::look::draw::for_balls(fill), r.screen))
}

/// Values one sample of a track gives: speed, height, left-right, distance from the centre.
fn live_values(s: &TrackSample, log: &Log, settings: &MixSettings, screen: bool) -> [f32; 4] {
    let p = if screen { s.position } else { log.camera_at(s.step).to_screen(s.position) };
    let (h, x) = in_frame(p, settings);
    [s.velocity.length(), h, x, p.length()]
}

/// `points` (time, values), in time order, blended at `rate` from `start`, holding the last.
fn resample(points: &[(f64, [f32; 4])], start: f64, rate: f64, until: f64) -> Vec<[f32; 4]> {
    let Some(last) = points.last() else { return Vec::new() };
    let n = (((last.0.min(until) - start).max(0.0) * rate).floor() as usize) + 1;
    let mut out = Vec::with_capacity(n);
    let mut j = 0;
    for k in 0..n {
        let t = start + k as f64 / rate;
        while j + 1 < points.len() && points[j + 1].0 <= t {
            j += 1;
        }
        let (t0, a) = points[j];
        let v = match points.get(j + 1) {
            Some(&(t1, b)) if t >= t0 && t1 > t0 => {
                let f = ((t - t0) / (t1 - t0)) as f32;
                std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f)
            }
            _ => a,
        };
        out.push(v);
    }
    out
}

/// A ball's live values on the video clock at [`MOTION_RATE`], from `start` video seconds.
struct Series {
    samples: Arc<Vec<[f32; 4]>>,
    start: f64,
    key: u64,
}

/// What working out the hits shares from one hit to the next.
struct Run<'a> {
    resolved: &'a Resolved,
    log: &'a Log,
    settings: &'a MixSettings,
    video_time: &'a dyn Fn(f64) -> f64,
    /// Whether some patch reads live inputs, so hits need tracks.
    live: bool,
    hits_so_far: BTreeMap<Party, u32>,
    born: BTreeMap<Party, u64>,
    fills: BTreeMap<ThingId, Option<(crate::paint::Paint, bool)>>,
    series: BTreeMap<Party, Option<Series>>,
}

impl Run<'_> {
    /// The ball's whole recorded track, on the video clock, built once per ball.
    fn series(&mut self, ball: Party, screen: bool) -> Option<&Series> {
        let (log, settings, video_time) = (self.log, self.settings, self.video_time);
        self.series
            .entry(ball)
            .or_insert_with(|| {
                let sps = settings.steps_per_second.max(1) as f64;
                let points: Vec<(f64, [f32; 4])> = log
                    .track(ball)
                    .iter()
                    .map(|s| (video_time(s.step as f64 / sps), live_values(s, log, settings, screen)))
                    .collect();
                let start = points.first()?.0;
                let track = Track::new(MOTION_RATE as f32, resample(&points, start, MOTION_RATE as f64, f64::INFINITY));
                Some(Series { samples: track.samples, start, key: track.key })
            })
            .as_ref()
    }
}

/// Every Hit and Note event's values, by its place in the log.
fn hit_values(
    resolved: &Resolved,
    log: &Log,
    settings: &MixSettings,
    video_time: &dyn Fn(f64) -> f64,
    live: bool,
) -> BTreeMap<u64, Arc<HitPhysics>> {
    let mut run = Run {
        resolved,
        log,
        settings,
        video_time,
        live,
        hits_so_far: BTreeMap::new(),
        born: BTreeMap::new(),
        fills: BTreeMap::new(),
        series: BTreeMap::new(),
    };
    let mut out = BTreeMap::new();
    for (i, e) in log.events().iter().enumerate() {
        match e.kind {
            EventKind::Spawn => {
                run.born.insert(e.a, e.step);
                continue;
            }
            EventKind::Hit => *run.hits_so_far.entry(e.a).or_default() += 1,
            EventKind::Note => {}
            _ => continue,
        }
        out.insert(i as u64, Arc::new(one_hit(e, &mut run)));
    }
    out
}

fn one_hit(e: &Event, run: &mut Run) -> HitPhysics {
    let (resolved, log, settings) = (run.resolved, run.log, run.settings);
    let sps = settings.steps_per_second.max(1);
    let fills = &mut run.fills;
    let fill = fills.entry(e.a.thing).or_insert_with(|| fill_of(resolved, e.a.thing));
    let screen =
        fill.as_ref().map(|f| f.1).unwrap_or_else(|| resolved.things.get(&e.a.thing).is_some_and(|r| r.screen));
    let track = log.track(e.a);
    let from = track.partition_point(|s| s.step < e.step);
    let at_hit = track.get(from).filter(|s| s.step == e.step);
    let place = if screen { e.point } else { log.camera_at(e.step).to_screen(e.point) };
    let (height, left_right) = in_frame(place, settings);
    let hits = run.hits_so_far.get(&e.a).copied().unwrap_or(0);
    let time = e.time(sps);
    let speed = at_hit.map_or(e.speed, |s| s.velocity.length());
    let (hue, brightness) = match fill {
        Some((paint, _)) => {
            let inputs = Inputs {
                count: 1,
                place: e.point,
                place_screen: place,
                start: e.point,
                speed,
                hits,
                time: time as f32,
                ..Inputs::default()
            };
            let (h, _, v) = Color::from_linear(colour_at(paint, &inputs)).to_hsv();
            (h, v)
        }
        None => (0.0, 1.0),
    };
    let radius = at_hit.map(|s| s.radius).or_else(|| {
        let r = resolved.things.get(&e.a.thing).filter(|r| r.kind.as_str() == "balls")?;
        Some(crate::things::balls::BallsSettings::from_params(&r.params).radius)
    });
    let age = time - run.born.get(&e.a).map_or(0.0, |s| *s as f64 / sps as f64);
    let mut values = [0.0; 11];
    values[0] = e.speed;
    values[1] = at_hit.map_or(0.0, |s| angle(s.velocity, e.normal));
    values[2] = side(e.normal);
    values[4] = radius.unwrap_or(0.0);
    values[5] = hue;
    values[6] = brightness;
    values[7] = height;
    values[8] = left_right;
    values[9] = hits as f32;
    values[10] = age as f32;
    let tags = e.b.and_then(|b| settings.tags.get(&b.thing)).map(|t| t.iter().cloned().collect()).unwrap_or_default();
    let has_track = run.live && from < track.len();
    let video_time = run.video_time;
    let track = has_track.then(|| run.series(e.a, screen)).flatten().map(|series| {
        let rate = MOTION_RATE as f64;
        let at = video_time(time);
        let until = video_time(time + TRACK_SECONDS);
        let start = ((at - series.start) * rate).round().max(0.0) as usize;
        let len = ((until - at).max(0.0) * rate).ceil() as usize + 1;
        Arc::new(Track { rate: rate as f32, samples: series.samples.clone(), start, len, key: series.key })
    });
    HitPhysics { values, tags, track }
}

/// A chord as input blocks read it: root pitch class / 11, quality Major 0 to Augmented 1.
fn chord_value(c: &bpe_core::Chord) -> [f32; 2] {
    let quality = match c.quality {
        Quality::Major => 0.0,
        Quality::Minor => 1.0 / 3.0,
        Quality::Diminished => 2.0 / 3.0,
        Quality::Augmented => 1.0,
    };
    [c.root as f32 / 11.0, quality]
}

fn chord_series(p: &bpe_core::Progression, settings: &MixSettings, n: usize, rate: f64) -> Vec<[f32; 2]> {
    let clock = ChordClock::new(p, &settings.beats);
    (0..n).map(|k| chord_value(clock.at(k as f64 / rate).1)).collect()
}

/// The scene's series at [`MOTION_RATE`] on the video clock.
fn scene_series(
    resolved: &Resolved,
    log: &Log,
    settings: &MixSettings,
    video_time: &dyn Fn(f64) -> f64,
) -> SceneSignals {
    let rate = MOTION_RATE as f64;
    let sps = settings.steps_per_second.max(1) as f64;
    let n = (settings.duration.max(0.0) as f64 * rate).ceil() as usize + 1;
    let samples: Vec<(f64, [f32; 4])> = log
        .scene_samples()
        .iter()
        .map(|s| (video_time(s.step as f64 / sps), [s.balls as f32, s.energy, 0.0, 0.0]))
        .collect();
    let motion = resample(&samples, 0.0, rate, f64::INFINITY);
    let held = |k: usize, i: usize| motion.get(k).or(motion.last()).map_or(0.0, |v| v[i]);
    let mut hits: Vec<f64> =
        log.events().iter().filter(|e| e.kind == EventKind::Hit).map(|e| video_time(e.step as f64 / sps)).collect();
    hits.sort_by(f64::total_cmp);
    let hit_rate = |t: f64| {
        let upto = hits.partition_point(|h| *h <= t);
        let from = hits.partition_point(|h| *h <= t - HIT_RATE_WINDOW);
        (upto - from) as f32 / HIT_RATE_WINDOW as f32
    };
    let root = music::root_settings(resolved);
    let bands = music::load_song(&root.song).map(|song| music::bands_of(&song.features, root.song_offset));
    let band = |t: f64, i: usize| bands.as_ref().map_or(0.0, |b| b.at(t)[i]);
    let time = |k: usize| k as f64 / rate;
    SceneSignals {
        rate: rate as f32,
        balls: (0..n).map(|k| held(k, 0)).collect(),
        energy: (0..n).map(|k| held(k, 1)).collect(),
        hit_rate: (0..n).map(|k| hit_rate(time(k))).collect(),
        bass: (0..n).map(|k| band(time(k), 0)).collect(),
        mid: (0..n).map(|k| band(time(k), 1)).collect(),
        high: (0..n).map(|k| band(time(k), 2)).collect(),
        beats: settings.beats.clone(),
        chords: chord_series(settings.progression(ProgressionRef(None)), settings, n, rate),
        levels: BTreeMap::new(),
    }
}

/// Everything input blocks can read from this run.
pub fn physics_inputs(
    scene: &Scene,
    resolved: &Resolved,
    log: &Log,
    settings: &MixSettings,
    video_time: &dyn Fn(f64) -> f64,
) -> PhysicsInputs {
    let hits = if uses_hit_inputs(scene) {
        hit_values(resolved, log, settings, video_time, uses_live_inputs(scene))
    } else {
        BTreeMap::new()
    };
    let signals = scene_series(resolved, log, settings, video_time);
    let n = signals.chords.len();
    let scene_progression = settings.progression(ProgressionRef(None));
    let mut chords = BTreeMap::new();
    for r in resolved.things.values() {
        let Some(slot) = r.slots.get(&SlotId::Sound).filter(|s| s.kind.as_str() == "sound.notes") else { continue };
        if chords.contains_key(&slot.module.0) {
            continue;
        }
        let p = settings.progression(NoteSound::from_params(&slot.params).chords);
        if p != scene_progression {
            chords.insert(slot.module.0, chord_series(p, settings, n, MOTION_RATE as f64));
        }
    }
    PhysicsInputs { hits, scene: signals, chords }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{BALLS, run};
    use crate::{kit, mix_settings, new_scene};
    use bpe_core::resolve;
    use bpe_graph::Param;

    fn same(t: f64) -> f64 {
        t
    }

    /// The new scene with its instrument swapped for a patch that has a block of `kind`.
    fn with_input(kind: &str, input: &str) -> Scene {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let mut graph = bpe_synth::blank_patch();
        let cat = bpe_synth::catalog();
        let spec = bpe_graph::Catalog::spec(cat, kind).unwrap().clone();
        let id = graph.add(&spec, [0.0, 0.0]);
        graph.nodes.iter_mut().find(|n| n.id == id).unwrap().params.insert("input".into(), Param::Choice(input.into()));
        for i in scene.library.instruments.values_mut() {
            i.source = bpe_core::InstrumentSource::Patch { graph: graph.clone() };
        }
        scene
    }

    #[test]
    fn a_falling_ball_reads_falling_height() {
        let kit = kit();
        let scene = with_input("live_input", "height");
        let resolved = resolve(&scene, &kit.registry);
        let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        world.record_tracks = true;
        let mut log = Log::default();
        world.advance_to(4.0, &kit.sims, &mut log);
        let settings = mix_settings(&scene, &resolved);
        let p = physics_inputs(&scene, &resolved, &log, &settings, &same);
        let (i, first) =
            log.events().iter().enumerate().find(|(_, e)| e.kind == EventKind::Hit && e.a.thing == BALLS).unwrap();
        let next = log.events().iter().skip(i + 1).find(|e| e.kind == EventKind::Hit && e.a == first.a).unwrap();
        let track = p.hits[&(i as u64)].track.clone().unwrap();
        let sps = settings.steps_per_second as f64;
        let gap = ((next.step - first.step) as f64 / sps * 60.0) as usize;
        let heights: Vec<f32> = (0..gap.min(track.len)).map(|k| track.at(k as f32 / 60.0)[1]).collect();
        let top = heights.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert!(heights[top] > heights[heights.len() - 1] + 0.05, "it moves: {heights:?}");
        for w in heights[top..].windows(2) {
            assert!(w[1] <= w[0] + 1e-4, "falling: {heights:?}");
        }
    }

    fn hand_log(hits: &[(u64, Vec2, Vec2, f32)]) -> Log {
        let mut log = Log::default();
        for &(step, point, normal, speed) in hits {
            log.push(Event {
                step,
                kind: EventKind::Hit,
                a: Party { thing: BALLS, item: 0 },
                b: Some(Party { thing: crate::testing::ARENA, item: 0 }),
                point,
                normal,
                speed,
            });
        }
        log
    }

    #[test]
    fn hit_values_from_a_known_hit() {
        let kit = kit();
        let scene = with_input("hit_input", "speed");
        let resolved = resolve(&scene, &kit.registry);
        let settings = mix_settings(&scene, &resolved);
        let log = hand_log(&[(10, Vec2::ZERO, Vec2::new(0.0, -1.0), 600.0)]);
        let p = physics_inputs(&scene, &resolved, &log, &settings, &same);
        let v = p.hits[&0].values;
        assert_eq!(v[0], 600.0);
        assert!((v[2] - 1.0 / 3.0).abs() < 1e-6);
        assert!((v[7] - 0.5).abs() < 1e-6);
        assert!((v[8] - 0.5).abs() < 1e-6);
        assert_eq!(v[9], 1.0);
        assert!(v[4] > 0.0, "the ball's radius setting");
    }

    #[test]
    fn scene_series_count_hits_per_second() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let settings = mix_settings(&scene, &resolved);
        let sps = settings.steps_per_second as u64;
        let hits: Vec<_> = (0..10).map(|k| (k * sps / 10 + 1, Vec2::ZERO, Vec2::Y, 100.0)).collect();
        let p = physics_inputs(&scene, &resolved, &hand_log(&hits), &settings, &same);
        let at_one = p.scene.value("hit_rate", "", 1.0);
        assert!((at_one - 10.0).abs() <= 2.0, "{at_one}");
        assert_eq!(p.scene.value("hit_rate", "", 3.0), 0.0);
    }

    #[test]
    fn no_physics_work_without_inputs() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 2.0);
        assert!(log.events().iter().any(|e| e.kind == EventKind::Hit));
        let p = physics_inputs(&scene, &resolved, &log, &mix_settings(&scene, &resolved), &same);
        assert!(p.hits.is_empty());
        assert!(!uses_live_inputs(&scene));
    }

    #[test]
    fn uses_live_inputs_sees_patches() {
        assert!(uses_live_inputs(&with_input("live_input", "speed")));
        assert!(!uses_live_inputs(&with_input("hit_input", "speed")));
        assert!(!uses_live_inputs(&with_input("scene_input", "balls")));
    }

    /// A ball moving up at 100 px/s, sampled every run step for 3 s, hit at step 0 and 60.
    fn rising_log(sps: u64) -> Log {
        let mut log = hand_log(&[(0, Vec2::ZERO, Vec2::Y, 100.0), (sps, Vec2::ZERO, Vec2::Y, 100.0)]);
        let ball = Party { thing: BALLS, item: 0 };
        for step in 0..=3 * sps {
            let t = step as f32 / sps as f32;
            let position = Vec2::new(0.0, -100.0 * t);
            log.push_track(ball, TrackSample { step, position, velocity: Vec2::new(0.0, -100.0), radius: 20.0 });
        }
        log
    }

    #[test]
    fn tracks_run_on_the_video_clock() {
        let kit = kit();
        let scene = with_input("live_input", "height");
        let resolved = resolve(&scene, &kit.registry);
        let settings = mix_settings(&scene, &resolved);
        let sps = settings.steps_per_second as u64;
        let log = rising_log(sps);
        // The video runs at half speed: one run second lasts two video seconds.
        let slow = |t: f64| 2.0 * t;
        let p = physics_inputs(&scene, &resolved, &log, &settings, &slow);
        let track = p.hits[&0].track.clone().unwrap();
        let at = |s: f32| track.at(s)[1];
        let per_px = 1.0 / (2.0 * settings.half_height);
        // One video second after the hit is half a run second: 50 px higher.
        assert!((at(1.0) - at(0.0) - 50.0 * per_px).abs() < 1e-3, "{} {}", at(0.0), at(1.0));
    }

    #[test]
    fn hits_share_one_series() {
        let kit = kit();
        let scene = with_input("live_input", "height");
        let resolved = resolve(&scene, &kit.registry);
        let settings = mix_settings(&scene, &resolved);
        let log = rising_log(settings.steps_per_second as u64);
        let p = physics_inputs(&scene, &resolved, &log, &settings, &same);
        let (a, b) = (p.hits[&0].track.clone().unwrap(), p.hits[&1].track.clone().unwrap());
        assert!(Arc::ptr_eq(&a.samples, &b.samples));
        assert_eq!(b.start, 60);
        // The second hit's window starts where the ball was then.
        assert!((b.at(0.0)[1] - a.at(1.0)[1]).abs() < 1e-4);
    }
}
