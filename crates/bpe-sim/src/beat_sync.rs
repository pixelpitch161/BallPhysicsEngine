//! Beat sync: making bounces land on the beat of the song (or of a tempo).
//!
//! - **Lock** steers the lead balls while the simulation runs. Right after a lead ball
//!   bounces, a copy of the simulation with only that ball in it is run a little way ahead to
//!   see when it will hit next. Its speed is then scaled (by at most `max_change`) so the
//!   next hit lands on a beat, or on a half or quarter beat when a whole one is out of reach.
//!   Everything is worked out from the simulation's own state, so it stays deterministic.
//! - **Nudge** ([`nudge`]) searches gravity, launch speed and arena size once, before playing,
//!   for the mix that puts the most hits on beats.
//! - **Warp** ([`TimeWarp`]) leaves the physics alone and gently speeds up and slows down the
//!   video's clock between hits so they land on beats.
//!
//! Physics only ever follows the song's beats, never the bounce sounds.

use crate::{Event, SimInputs, Simulation};
use bpe_music::AudioFeatures;
use bpe_scene::{Container, Scene, Spawner, SyncMode};
use std::sync::Arc;

/// Tempo used when there is no song and no tempo set.
const DEFAULT_BPM: f32 = 120.0;
/// How far ahead Lock looks for the next hit, in seconds.
const LOOK_AHEAD: f64 = 2.5;
/// How many hits ahead Lock checks it can keep landing on beats.
const PLAN_DEPTH: u32 = 2;
/// Tries Lock makes to find the right speed for one target.
const SEARCH_STEPS: usize = 7;

/// The beat times a scene follows: the song's beats, or a steady grid from its tempo.
pub fn beat_grid(scene: &Scene, song_beats: &[f64]) -> Vec<f64> {
    let music = &scene.music;
    if !song_beats.is_empty() && music.bpm <= 0.0 {
        return song_beats.to_vec();
    }
    let bpm = if music.bpm > 0.0 { music.bpm } else { DEFAULT_BPM };
    let period = 60.0 / bpm.clamp(20.0, 400.0) as f64;
    // A song's first beat anchors an overriding tempo; otherwise beats start at zero.
    let first = song_beats.first().copied().unwrap_or(0.0);
    let start = first - (first / period).floor() * period;
    let end = scene.video.duration as f64 + LOOK_AHEAD + period;
    (0..).map(|k| start + k as f64 * period).take_while(|&t| t <= end).collect()
}

/// What a scene's simulation follows: the song's beats and levels, or a steady beat when the
/// scene sets a tempo or uses beat sync. Scenes that do neither get no beats, so their event
/// logs stay as they were.
pub fn inputs(scene: &Scene, song: Option<Arc<AudioFeatures>>) -> SimInputs {
    let wants_beats = scene.music.bpm > 0.0 || scene.music.beat_sync.mode != SyncMode::Off;
    match song {
        Some(music) => SimInputs { beats: beat_grid(scene, &music.beats), music: Some(music) },
        None if wants_beats => SimInputs { beats: beat_grid(scene, &[]), music: None },
        None => SimInputs::default(),
    }
}

/// The beats with `subdivisions` (1, 2 or 4) evenly spaced points per beat, which is what
/// Lock aims for.
pub fn subdivided(beats: &[f64], subdivisions: u32) -> Vec<f64> {
    let division = subdivisions.clamp(1, 4) as usize;
    let mut out = Vec::with_capacity(beats.len() * division);
    for pair in beats.windows(2) {
        out.extend((0..division).map(|k| pair[0] + (pair[1] - pair[0]) * k as f64 / division as f64));
    }
    out.extend(beats.last());
    out
}

/// Share of hits and escapes (0 to 1) that land within `tolerance` seconds of a beat. `beats`
/// must be sorted.
pub fn on_beat_share(events: &[Event], beats: &[f64], tolerance: f64) -> f32 {
    on_beat_share_of(events.iter().filter(|e| e.kind.is_bounce()).map(|e| e.time), beats, tolerance)
}

/// Like [`on_beat_share`], for any list of times.
pub fn on_beat_share_of(times: impl IntoIterator<Item = f64>, beats: &[f64], tolerance: f64) -> f32 {
    let (mut on, mut all) = (0usize, 0usize);
    for time in times {
        all += 1;
        if distance_to_beat(time, beats) <= tolerance {
            on += 1;
        }
    }
    if all == 0 { 0.0 } else { on as f32 / all as f32 }
}

/// Seconds from `time` to the nearest beat.
pub fn distance_to_beat(time: f64, beats: &[f64]) -> f64 {
    let i = beats.partition_point(|&b| b < time);
    let after = beats.get(i).map_or(f64::INFINITY, |b| b - time);
    let before = i.checked_sub(1).map_or(f64::INFINITY, |j| time - beats[j]);
    after.min(before)
}

/// Lock mode's state inside a running simulation.
#[derive(Clone, Debug, Default)]
pub struct Controller {
    active: bool,
    /// The beats and the half or quarter beats Lock may aim for.
    grid: Vec<f64>,
    /// Each lead ball's hit count when last looked at, to notice new bounces.
    seen: Vec<u32>,
}

impl Controller {
    pub fn new(scene: &Scene, inputs: &SimInputs) -> Self {
        let sync = &scene.music.beat_sync;
        if sync.mode != SyncMode::Lock || sync.lead_balls == 0 {
            return Self::default();
        }
        let beats = if inputs.beats.is_empty() { beat_grid(scene, &[]) } else { inputs.beats.clone() };
        let grid = subdivided(&beats, sync.subdivisions);
        // No count has been seen yet, so the first flight is steered too.
        Self { active: true, grid, seen: vec![u32::MAX; sync.lead_balls as usize] }
    }

    /// Runs at the start of each step. When a lead ball bounced in the last step, adjusts its
    /// speed so its next hit lands on a beat.
    pub fn before_step(sim: &mut Simulation) {
        if !sim.controller.active {
            return;
        }
        let mut controller = std::mem::take(&mut sim.controller);
        for lead in 0..controller.seen.len().min(sim.balls.len()) {
            let hits = sim.balls[lead].wall_hits;
            if hits == controller.seen[lead] || !sim.balls[lead].alive {
                continue;
            }
            controller.seen[lead] = hits;
            let max_change = sim.scene.music.beat_sync.max_change.clamp(0.0, 0.5) as f64;
            let probe = Probe::new(sim, lead);
            if let Some(scale) = controller.plan(&probe, max_change) {
                sim.balls[lead].velocity *= scale as f32;
            }
        }
        sim.controller = controller;
    }

    /// The speed scale for the lead ball now. It aims the next hit at the beat nearest where it
    /// would land anyway, preferring one after which the following hit can reach a beat too, so
    /// one miss doesn't lead to another.
    fn plan(&self, probe: &Probe, max_change: f64) -> Option<f64> {
        let reach = probe.reach(max_change)?;
        let mut targets: Vec<f64> = self.points_between(reach.earliest, reach.latest).collect();
        targets.sort_by(|a, b| (a - reach.natural).abs().total_cmp(&(b - reach.natural).abs()));
        let mut fallback = None;
        for &target in targets.iter().take(3) {
            let Some((scale, _)) = probe.solve(target, &reach) else { continue };
            if self.next_can_land(probe, scale, max_change) {
                return Some(scale);
            }
            fallback.get_or_insert(scale);
        }
        if fallback.is_some() {
            return fallback;
        }
        // No beat in reach. Pick the speed that sets up the following hit best, landing this
        // one as near a beat as possible.
        let mut best: Option<(bool, f64, f64)> = None;
        for k in 0..=8 {
            let scale = 1.0 - max_change + 2.0 * max_change * k as f64 / 8.0;
            let Some(time) = probe.next_hit(scale) else { continue };
            let ready = self.next_can_land(probe, scale, max_change);
            let miss = distance_to_beat(time, &self.grid);
            if best.is_none_or(|(r, m, _)| (ready && !r) || (ready == r && miss < m)) {
                best = Some((ready, miss, scale));
            }
        }
        best.map(|(_, _, scale)| scale).filter(|&scale| scale != 1.0)
    }

    /// Whether, after flying at `scale`, the following hits could be put on beats too, looking
    /// `depth` hits further ahead.
    fn next_can_land(&self, probe: &Probe, scale: f64, max_change: f64) -> bool {
        self.can_land(probe.after_hit(scale), max_change, PLAN_DEPTH)
    }

    fn can_land(&self, probe: Option<Probe>, max_change: f64, depth: u32) -> bool {
        let Some(probe) = probe else { return false };
        let Some(reach) = probe.reach(max_change) else { return false };
        let mut targets: Vec<f64> = self.points_between(reach.earliest, reach.latest).collect();
        if depth <= 1 || targets.is_empty() {
            return !targets.is_empty();
        }
        targets.sort_by(|a, b| (a - reach.natural).abs().total_cmp(&(b - reach.natural).abs()));
        targets.iter().take(2).any(|&target| {
            probe
                .solve(target, &reach)
                .is_some_and(|(scale, _)| self.can_land(probe.after_hit(scale), max_change, depth - 1))
        })
    }

    fn points_between(&self, from: f64, to: f64) -> impl Iterator<Item = f64> + '_ {
        let start = self.grid.partition_point(|&t| t < from);
        self.grid[start..].iter().copied().take_while(move |&t| t <= to)
    }
}

/// When the next hit happens at normal speed and at the lowest and highest allowed speeds.
struct Reach {
    natural: f64,
    points: Vec<(f64, f64)>,
    earliest: f64,
    latest: f64,
}

/// A light copy of a simulation holding just one ball, for looking ahead.
struct Probe {
    start: Simulation,
}

impl Probe {
    fn new(sim: &mut Simulation, lead: usize) -> Self {
        // Copy everything but the balls, trails and later spawns, which the look-ahead doesn't need.
        let balls = std::mem::take(&mut sim.balls);
        let trail = std::mem::take(&mut sim.trail);
        let pending = std::mem::take(&mut sim.pending);
        let beats = std::mem::take(&mut sim.inputs.beats);
        let mut start = sim.clone();
        sim.balls = balls;
        sim.trail = trail;
        sim.pending = pending;
        sim.inputs.beats = beats;
        start.balls = vec![sim.balls[lead].clone()];
        start.trail_len = 0;
        start.trail_filled = 0;
        start.controller = Controller::default();
        Self { start }
    }

    /// Runs the ball at `scale` times its speed until it next hits something. Returns the hit's
    /// time and the simulation at the end of that step.
    fn run(&self, scale: f64) -> Option<(f64, Simulation)> {
        let mut sim = self.start.clone();
        sim.balls[0].velocity *= scale as f32;
        let mut hits = Vec::new();
        let rate = sim.scene.physics.steps_per_second.max(1) as f64;
        let end = sim.step + (LOOK_AHEAD * rate) as u64;
        while sim.step < end {
            sim.step(&mut hits);
            if let Some(hit) = hits.iter().find(|e| e.kind.is_bounce()) {
                return Some((hit.time, sim));
            }
            if sim.balls.is_empty() {
                return None;
            }
        }
        None
    }

    fn next_hit(&self, scale: f64) -> Option<f64> {
        self.run(scale).map(|(time, _)| time)
    }

    /// The look-ahead from just after the next hit, had the ball flown at `scale`.
    fn after_hit(&self, scale: f64) -> Option<Probe> {
        self.run(scale).map(|(_, start)| Probe { start })
    }

    fn reach(&self, max_change: f64) -> Option<Reach> {
        let natural = self.next_hit(1.0)?;
        let mut points = vec![(1.0, natural)];
        for scale in [1.0 - max_change, 1.0 + max_change] {
            points.extend(self.next_hit(scale).map(|t| (scale, t)));
        }
        let earliest = points.iter().map(|p| p.1).fold(f64::MAX, f64::min);
        let latest = points.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        Some(Reach { natural, points, earliest, latest })
    }

    /// The speed scale whose next hit lands nearest `target`, found with a bracketing secant
    /// search between speeds whose hits land either side of it.
    fn solve(&self, target: f64, reach: &Reach) -> Option<(f64, f64)> {
        let points = &reach.points;
        let below = points.iter().filter(|p| p.1 <= target).max_by(|a, b| a.1.total_cmp(&b.1)).copied();
        let above = points.iter().filter(|p| p.1 >= target).min_by(|a, b| a.1.total_cmp(&b.1)).copied();
        let (Some(mut a), Some(mut b)) = (below, above) else { return None };
        let mut best = *points.iter().min_by(|p, q| (p.1 - target).abs().total_cmp(&(q.1 - target).abs()))?;
        for i in 0..SEARCH_STEPS {
            if (best.1 - target).abs() < 0.5e-3 || (a.0 - b.0).abs() < 1e-6 {
                break;
            }
            let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
            let guess = if (b.1 - a.1).abs() > 1e-9 { a.0 + (target - a.1) * (b.0 - a.0) / (b.1 - a.1) } else { lo };
            let margin = (hi - lo) * 0.1;
            // Halve instead when the secant guess hugs an end, and every third try regardless.
            let scale =
                if i % 3 == 2 || guess <= lo + margin || guess >= hi - margin { (lo + hi) * 0.5 } else { guess };
            let Some(time) = self.next_hit(scale) else { break };
            if (time - target).abs() < (best.1 - target).abs() {
                best = (scale, time);
            }
            if time <= target {
                a = (scale, time);
            } else {
                b = (scale, time);
            }
        }
        Some(best)
    }
}

/// Video time stretched against simulation time so hits land on beats (Warp mode).
#[derive(Clone, Debug, PartialEq)]
pub struct TimeWarp {
    /// (video time, simulation time) points the curve passes through, both increasing.
    anchors: Vec<(f64, f64)>,
    /// Slope of the curve (simulation seconds per video second) at each anchor.
    slopes: Vec<f64>,
}

impl TimeWarp {
    /// Fits a warp so as many of the lead balls' hits as possible land on beats, never running
    /// the video more than `warp_strength` faster or slower than real time between hits.
    pub fn fit(scene: &Scene, events: &[Event], beats: &[f64]) -> Self {
        let sync = &scene.music.beat_sync;
        let beats = &subdivided(beats, sync.subdivisions);
        let k = sync.warp_strength.clamp(0.0, 0.9) as f64;
        let leads = sync.lead_balls;
        let duration = scene.video.duration as f64;
        let min_speed = scene.sound.min_impact_speed;
        let mut times: Vec<f64> = events
            .iter()
            .filter(|h| h.kind.is_bounce() && (leads == 0 || h.ball < leads) && h.speed >= min_speed)
            .map(|h| h.time)
            .collect();
        times.sort_by(f64::total_cmp);
        let mut anchors = vec![(0.0, 0.0)];
        for &hit in &times {
            let (v0, s0) = *anchors.last().unwrap();
            let gap = hit - s0;
            // Hits too close to the last anchor would need too sharp a bend.
            if gap < 0.06 {
                continue;
            }
            let (earliest, latest) = (v0 + gap / (1.0 + k), v0 + gap * (1.0 + k));
            let natural = v0 + gap;
            let mut best: Option<f64> = None;
            let i = beats.partition_point(|&b| b < earliest);
            for &b in beats[i..].iter().take_while(|&&b| b <= latest) {
                if best.is_none_or(|x| (b - natural).abs() < (x - natural).abs()) {
                    best = Some(b);
                }
            }
            if let Some(beat) = best.filter(|&b| b < duration) {
                anchors.push((beat, hit));
            }
        }
        // Past the last hit, time runs at its normal speed.
        let (v, s) = *anchors.last().unwrap();
        anchors.push((v.max(duration) + 1.0, s + (v.max(duration) + 1.0 - v)));
        let slopes = monotone_slopes(&anchors);
        Self { anchors, slopes }
    }

    /// No stretching at all.
    pub fn identity() -> Self {
        Self { anchors: vec![(0.0, 0.0), (1.0, 1.0)], slopes: vec![1.0, 1.0] }
    }

    /// The simulation time to show at video time `video`.
    pub fn sim_time(&self, video: f64) -> f64 {
        let a = &self.anchors;
        if video <= 0.0 {
            return video;
        }
        let i = a.partition_point(|p| p.0 <= video);
        if i >= a.len() {
            let (v, s) = a[a.len() - 1];
            return s + (video - v);
        }
        let (v0, s0) = a[i - 1];
        let (v1, s1) = a[i];
        let h = v1 - v0;
        let t = (video - v0) / h;
        // Cubic Hermite between the two anchors.
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * s0 + h10 * h * self.slopes[i - 1] + h01 * s1 + h11 * h * self.slopes[i]
    }

    /// The video time at which simulation time `sim` is shown: the inverse of [`TimeWarp::sim_time`].
    pub fn video_time(&self, sim: f64) -> f64 {
        if sim <= 0.0 {
            return sim;
        }
        let a = &self.anchors;
        let i = a.partition_point(|p| p.1 <= sim);
        if i >= a.len() {
            let (v, s) = a[a.len() - 1];
            return v + (sim - s);
        }
        if a[i - 1].1 == sim {
            return a[i - 1].0;
        }
        let (mut lo, mut hi) = (a[i - 1].0, a[i].0);
        for _ in 0..60 {
            let mid = (lo + hi) * 0.5;
            if self.sim_time(mid) < sim {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) * 0.5
    }

    /// Simulation time reached at the end of a video of `duration` seconds.
    pub fn sim_end(&self, duration: f64) -> f64 {
        self.sim_time(duration)
    }
}

/// Slopes at each point for a monotone cubic through them (Fritsch and Carlson), so the
/// curve never runs backwards or overshoots.
fn monotone_slopes(points: &[(f64, f64)]) -> Vec<f64> {
    let n = points.len();
    let secants: Vec<f64> = points.windows(2).map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0)).collect();
    let mut slopes = vec![0.0; n];
    slopes[0] = secants[0];
    slopes[n - 1] = secants[n - 2];
    for i in 1..n - 1 {
        let (a, b) = (secants[i - 1], secants[i]);
        slopes[i] = if a * b <= 0.0 { 0.0 } else { 2.0 / (1.0 / a + 1.0 / b) };
    }
    slopes
}

/// What [`nudge`] found.
#[derive(Clone, Debug)]
pub struct Nudged {
    pub scene: Scene,
    /// Share of hits on the beat before and after.
    pub before: f32,
    pub after: f32,
    /// The multipliers applied to gravity, launch speed and arena size.
    pub gravity: f32,
    pub speed: f32,
    pub size: f32,
}

/// Searches gravity, launch speed and arena size for the mix that lands the most hits within
/// the scene's tolerance of a beat, and returns the scene with them applied. Runs on all cores.
pub fn nudge(scene: &Scene, beats: &[f64]) -> Nudged {
    let tolerance = scene.music.beat_sync.tolerance.max(0.001) as f64;
    // Long scenes are judged on their first 20 seconds, which is what decides the search anyway.
    let horizon = (scene.video.duration as f64).min(20.0);
    let score = |g: f32, v: f32, z: f32| -> f32 {
        let tuned = scaled(scene, g, v, z);
        let mut sim = Simulation::new(&tuned);
        let mut hits = Vec::new();
        sim.advance_to(horizon, &mut hits);
        let min_speed = tuned.sound.min_impact_speed;
        let sounding = hits.iter().filter(|h| h.kind.is_bounce() && h.speed >= min_speed);
        on_beat_share_of(sounding.map(|h| h.time), beats, tolerance)
    };
    let before = score(1.0, 1.0, 1.0);
    let mut best = (before, 1.0f32, 1.0f32, 1.0f32);
    // A coarse grid over the whole range, then a finer one around the best point.
    for (spread, steps) in [(1.0f32, 3i32), (0.35, 3)] {
        let (cg, cv, cz) = (best.1, best.2, best.3);
        let mut candidates = Vec::new();
        for a in -steps..=steps {
            for b in -steps..=steps {
                let sizes = if matches!(scene.container, Container::Custom { .. }) { 0 } else { 1 };
                for c in -sizes..=sizes {
                    let g = (cg * (1.0 + 0.3 * spread * a as f32 / steps as f32)).clamp(0.6, 1.6);
                    let v = (cv * (1.0 + 0.2 * spread * b as f32 / steps as f32)).clamp(0.75, 1.3);
                    let z = (cz * (1.0 + 0.1 * spread * c as f32)).clamp(0.85, 1.15);
                    candidates.push((g, v, z));
                }
            }
        }
        let results = parallel_map(&candidates, |&(g, v, z)| score(g, v, z));
        for (&(g, v, z), share) in candidates.iter().zip(results) {
            // Ties keep the earlier (smaller change), so results are the same on every machine.
            if share > best.0 + 1e-6 {
                best = (share, g, v, z);
            }
        }
    }
    let (after, gravity, speed, size) = best;
    Nudged { scene: scaled(scene, gravity, speed, size), before, after, gravity, speed, size }
}

/// Runs `f` over `items` on all cores, keeping the order.
fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 32);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<R>>> = items.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads.min(items.len()) {
            scope.spawn(|| {
                while let Some(item) = items.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                    let i = (item as *const T as usize - items.as_ptr() as usize) / std::mem::size_of::<T>().max(1);
                    *results[i].lock().unwrap() = Some(f(item));
                }
            });
        }
    });
    results.into_iter().map(|r| r.into_inner().unwrap().expect("every item is done")).collect()
}

/// The scene with gravity, launch speeds and arena size multiplied.
pub fn scaled(scene: &Scene, gravity: f32, speed: f32, size: f32) -> Scene {
    let mut scene = scene.clone();
    scene.physics.gravity *= gravity;
    let center = scene.container.center();
    for spawner in &mut scene.spawners {
        let (position, velocity) = match spawner {
            Spawner::Grid { center: p, velocity, .. }
            | Spawner::Single { position: p, velocity, .. }
            | Spawner::Stream { position: p, velocity, .. } => (p, velocity),
        };
        *velocity *= speed;
        // Balls start in the same place relative to the arena.
        *position = center + (*position - center) * size;
    }
    scale_container(&mut scene.container, size);
    scene
}

fn scale_container(container: &mut Container, k: f32) {
    match container {
        Container::Circle { radius, .. } | Container::Polygon { radius, .. } => *radius *= k,
        Container::Rect { size, .. } => *size *= k,
        Container::Rings { inner_radius, spacing, .. } => {
            *inner_radius *= k;
            *spacing *= k;
        }
        // Drawn shapes keep their size; Nudge leaves it alone for them.
        Container::Custom { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{BeatSync, Music};

    #[test]
    fn beat_grids_and_shares() {
        let mut scene = Scene::default();
        scene.video.duration = 4.0;
        scene.music.bpm = 120.0;
        let grid = beat_grid(&scene, &[]);
        assert_eq!(&grid[..3], &[0.0, 0.5, 1.0]);
        // A song's beats are used as they are, unless a tempo overrides them.
        scene.music.bpm = 0.0;
        assert_eq!(beat_grid(&scene, &[0.3, 0.8]), vec![0.3, 0.8]);
        scene.music.bpm = 60.0;
        assert!((beat_grid(&scene, &[2.25])[0] - 0.25).abs() < 1e-9);
        let times = [0.01, 0.26, 0.49, 0.75];
        assert_eq!(on_beat_share_of(times, &[0.0, 0.5, 1.0], 0.02), 0.5);
        assert_eq!(on_beat_share_of([], &[0.0], 0.02), 0.0);
        assert_eq!(on_beat_share_of([1.0], &[], 0.02), 0.0);
        assert!((distance_to_beat(0.7, &[0.5, 1.0]) - 0.2).abs() < 1e-12);
        assert_eq!(distance_to_beat(0.7, &[]), f64::INFINITY);
    }

    fn one_ball(bpm: f32, mode: SyncMode) -> Scene {
        let mut scene = Scene::growing_ball();
        scene.rules = Default::default();
        scene.physics.gravity.y = 100.0;
        scene.video.duration = 20.0;
        scene.music = Music { bpm, beat_sync: BeatSync { mode, ..BeatSync::default() }, ..Music::default() };
        scene
    }

    /// Share of the lead ball's hits within 20 ms of a beat or half beat.
    fn lead_share(scene: &Scene) -> f32 {
        let hits = crate::collect_wall_hits(scene);
        let beats = subdivided(&beat_grid(scene, &[]), scene.music.beat_sync.subdivisions);
        let lead: Vec<f64> = hits.iter().filter(|h| h.ball == 0 && h.kind.is_bounce()).map(|h| h.time).collect();
        on_beat_share_of(lead, &beats, 0.02)
    }

    #[test]
    fn lock_puts_the_lead_ball_on_the_beat() {
        for bpm in [96.0, 120.0, 140.0] {
            let free = lead_share(&one_ball(bpm, SyncMode::Off));
            let locked = lead_share(&one_ball(bpm, SyncMode::Lock));
            assert!(locked >= 0.95, "{bpm} bpm: {locked} on the beat (free {free})");
        }
    }

    #[test]
    fn lock_helps_even_under_heavy_gravity() {
        // Strong gravity leaves speed little say over when the next hit comes, so Lock can't
        // reach every beat, but it still lands far more hits on them.
        let heavy = |mode| {
            let mut scene = one_ball(120.0, mode);
            scene.physics.gravity.y = 900.0;
            lead_share(&scene)
        };
        let (free, locked) = (heavy(SyncMode::Off), heavy(SyncMode::Lock));
        assert!(locked > 0.6 && locked > free * 3.0, "{free} -> {locked}");
    }

    #[test]
    fn lock_with_no_lead_balls_or_no_balls_does_nothing() {
        let mut scene = one_ball(120.0, SyncMode::Lock);
        scene.music.beat_sync.lead_balls = 0;
        assert_eq!(crate::collect_wall_hits(&scene), crate::collect_wall_hits(&one_ball(120.0, SyncMode::Off)));
        scene.music.beat_sync.lead_balls = 3;
        scene.spawners.clear();
        assert!(crate::collect_wall_hits(&scene).is_empty());
    }

    #[test]
    fn lock_is_deterministic_and_seekable() {
        let scene = one_ball(120.0, SyncMode::Lock);
        let a = crate::collect_wall_hits(&scene);
        assert_eq!(a, crate::collect_wall_hits(&scene));
        let mut playback = crate::Playback::new(&scene);
        playback.seek(12.0);
        playback.seek(3.0);
        playback.seek(15.0);
        let replay: Vec<_> = a.iter().filter(|h| h.time <= playback.simulation().time()).copied().collect();
        assert_eq!(playback.hits(), &replay[..]);
    }

    #[test]
    fn warp_lands_hits_on_beats_and_keeps_time_running_forwards() {
        let scene = one_ball(110.0, SyncMode::Warp);
        let hits = crate::collect_wall_hits(&scene);
        let beats = beat_grid(&scene, &[]);
        let warp = TimeWarp::fit(&scene, &hits, &beats);
        let warped: Vec<f64> = hits.iter().map(|h| warp.video_time(h.time)).filter(|&t| t < 19.0).collect();
        let share = on_beat_share_of(warped.iter().copied(), &subdivided(&beats, 2), 0.02);
        assert!(share >= 0.9, "{share}");
        let mut last = -1.0;
        for i in 0..2000 {
            let v = i as f64 * 0.01;
            let s = warp.sim_time(v);
            assert!(s > last, "time ran backwards at {v}");
            assert!((warp.video_time(s) - v).abs() < 1e-6);
            last = s;
        }
        assert_eq!(TimeWarp::identity().sim_time(3.5), 3.5);
    }

    #[test]
    fn nudge_lands_more_hits_on_the_beat() {
        let mut scene = Scene::rainbow_trails();
        scene.video.duration = 15.0;
        scene.music.bpm = 100.0;
        let beats = beat_grid(&scene, &[]);
        let nudged = nudge(&scene, &beats);
        assert!(nudged.after >= nudged.before * 2.0, "{} -> {}", nudged.before, nudged.after);
        // Applying it again finds nothing better, and the same input gives the same answer.
        assert_eq!(nudge(&scene, &beats).after, nudged.after);
    }
}
