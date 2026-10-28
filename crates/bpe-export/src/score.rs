//! Scores how satisfying a video is likely to be, so a batch can keep only the best ones.
//!
//! The score has six readable parts, each from 0 (bad) to 1 (great):
//!
//! - **Rhythm**: bounces you can hear at a pleasing pace (not silence, not a wash of noise), fairly
//!   regular, and on the beat when a song is loaded.
//! - **Build-up**: more happens towards the end than at the start.
//! - **Climax**: the biggest moment comes in the last third, and something happens at the end
//!   (for ring scenes, rings keep breaking right to the end).
//! - **Motion**: balls never stall, stay on screen, and counts don't run away.
//! - **Variety**: from sampled frames, colourful, neither empty nor full, changing but not noise.
//! - **Sound health**: no clipping, and loudness near the scene's target.
//!
//! The first four come from simulating the scene alone, which is fast enough to pre-screen many
//! seeds. Variety needs rendered frames and Sound health needs the mixed soundtrack; parts that
//! weren't measured are left out of the total. The total is a weighted average with weights the
//! user can change, and thumbs up or down on a variation nudge the weights towards their taste.

use bpe_audio::{SAMPLE_RATE, Soundtrack};
use bpe_scene::{Container, Scene};
use bpe_sim::{HitKind, Simulation, WallHit};
use serde::{Deserialize, Serialize};

/// Hits are grouped into time slots this long, about the shortest gap the ear hears as two
/// separate sounds in a busy passage.
const SLOT: f64 = 1.0 / 16.0;
/// How often ball positions and speeds are sampled, in seconds.
const SAMPLE_EVERY: f64 = 0.1;
/// A hit within this many seconds of a beat counts as on the beat.
const ON_BEAT: f64 = 0.05;

/// The parts of a score, each 0 to 1. `None` means that part wasn't measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Parts {
    pub rhythm: f32,
    pub build_up: f32,
    pub climax: f32,
    pub motion: f32,
    pub variety: Option<f32>,
    pub sound: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    /// 0 to 1: the weighted average of the measured parts.
    pub total: f32,
    pub parts: Parts,
}

/// How much each part counts towards the total.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Weights {
    pub rhythm: f32,
    pub build_up: f32,
    pub climax: f32,
    pub motion: f32,
    pub variety: f32,
    pub sound: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self { rhythm: 1.0, build_up: 0.7, climax: 1.0, motion: 1.0, variety: 0.8, sound: 0.5 }
    }
}

/// Lowest and highest a weight can be nudged to by thumbs up and down.
const WEIGHT_RANGE: (f32, f32) = (0.05, 3.0);

impl Parts {
    /// (name, value, what it means) for each part, in order, for tooltips and reports.
    pub fn list(&self) -> [(&'static str, Option<f32>, &'static str); 6] {
        [
            ("Rhythm", Some(self.rhythm), "Bounces at a pleasing pace, fairly regular, on the beat"),
            ("Build-up", Some(self.build_up), "More happens towards the end than at the start"),
            ("Climax", Some(self.climax), "The biggest moment comes near the end"),
            ("Motion", Some(self.motion), "Balls keep moving and stay on screen"),
            ("Variety", self.variety, "Colourful and changing, neither empty nor noisy"),
            ("Sound health", self.sound, "No clipping, loudness on target"),
        ]
    }

    /// Each part's distance from the middle, for learning weights. Unmeasured parts give 0.
    fn centred(&self) -> [f32; 6] {
        [
            self.rhythm - 0.5,
            self.build_up - 0.5,
            self.climax - 0.5,
            self.motion - 0.5,
            self.variety.map_or(0.0, |v| v - 0.5),
            self.sound.map_or(0.0, |v| v - 0.5),
        ]
    }
}

impl Weights {
    fn array(&self) -> [f32; 6] {
        [self.rhythm, self.build_up, self.climax, self.motion, self.variety, self.sound]
    }

    fn set_array(&mut self, w: [f32; 6]) {
        [self.rhythm, self.build_up, self.climax, self.motion, self.variety, self.sound] = w;
    }

    /// The weighted average of the measured parts.
    pub fn total(&self, parts: &Parts) -> f32 {
        let w = self.array();
        let values = [
            Some(parts.rhythm),
            Some(parts.build_up),
            Some(parts.climax),
            Some(parts.motion),
            parts.variety,
            parts.sound,
        ];
        let (mut sum, mut weight) = (0.0, 0.0);
        for (value, w) in values.iter().zip(w) {
            if let Some(value) = value {
                sum += value.clamp(0.0, 1.0) * w.max(0.0);
                weight += w.max(0.0);
            }
        }
        if weight > 0.0 { sum / weight } else { 0.0 }
    }

    pub fn score(&self, parts: Parts) -> Score {
        Score { total: self.total(&parts), parts }
    }

    /// Nudges the weights after a thumbs up (`liked`) or down, by one step of online logistic
    /// regression: parts that were high on liked videos (and low on disliked ones) count more.
    pub fn learn(&mut self, parts: &Parts, liked: bool) {
        const RATE: f32 = 0.4;
        const SHARPNESS: f32 = 4.0;
        let x = parts.centred();
        let mut w = self.array();
        let guess = 1.0 / (1.0 + (-SHARPNESS * w.iter().zip(&x).map(|(w, x)| w * x).sum::<f32>()).exp());
        let error = if liked { 1.0 } else { 0.0 } - guess;
        for (w, x) in w.iter_mut().zip(x) {
            *w = (*w + RATE * error * SHARPNESS * x).clamp(WEIGHT_RANGE.0, WEIGHT_RANGE.1);
        }
        self.set_array(w);
    }
}

/// A small picture from the video, for judging variety.
#[derive(Clone, Debug, Default)]
pub struct SampledFrame {
    pub width: u32,
    pub height: u32,
    /// sRGB RGBA bytes, row by row.
    pub pixels: Vec<u8>,
}

impl SampledFrame {
    /// Shrinks a full RGBA frame to at most `max_width` pixels wide by averaging blocks.
    pub fn shrink(pixels: &[u8], width: u32, height: u32, max_width: u32) -> Self {
        let step = width.div_ceil(max_width.max(1)).max(1);
        let (w, h) = (width / step, height / step);
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 4];
                for dy in 0..step {
                    let row = ((y * step + dy) * width + x * step) as usize * 4;
                    for dx in 0..step as usize {
                        for (c, s) in sum.iter_mut().enumerate() {
                            *s += pixels[row + dx * 4 + c] as u32;
                        }
                    }
                }
                let n = step * step;
                let at = ((y * w + x) * 4) as usize;
                for c in 0..4 {
                    out[at + c] = (sum[c] / n) as u8;
                }
            }
        }
        Self { width: w, height: h, pixels: out }
    }
}

/// What else is known about a variation besides its scene.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScoreInputs<'a> {
    /// Beat times of the loaded song, in seconds.
    pub beats: &'a [f64],
    pub soundtrack: Option<&'a Soundtrack>,
    /// Frames sampled evenly through the video (16 is plenty).
    pub frames: &'a [SampledFrame],
}

/// What simulating a scene once tells the scorer.
#[derive(Clone, Debug, Default)]
pub struct SimStats {
    pub duration: f64,
    pub hits: Vec<WallHit>,
    /// Every [`SAMPLE_EVERY`] seconds: (ball count, fraction of balls on screen, fastest speed,
    /// average speed).
    pub samples: Vec<(u32, f32, f32, f32)>,
    pub rings: u32,
    pub max_balls: u32,
    /// Something went badly wrong: a position or speed stopped being a number.
    pub broken: bool,
}

/// Simulates `scene` once, without drawing or mixing sound.
pub fn simulate(scene: &Scene) -> SimStats {
    simulate_with(scene, &bpe_sim::SimInputs::default())
}

/// [`simulate`] with the song's beats, as the finished video plays it.
pub fn simulate_with(scene: &Scene, inputs: &bpe_sim::SimInputs) -> SimStats {
    let duration = scene.video.duration.max(0.0) as f64;
    let mut sim = Simulation::with_inputs(scene, inputs);
    let mut stats = SimStats {
        duration,
        rings: match scene.container {
            Container::Rings { count, .. } => count,
            _ => 0,
        },
        max_balls: scene.rules.max_balls,
        ..SimStats::default()
    };
    if scene.space().is_some() {
        // 3D: only the hits are measured; the balls live in the 3D simulation.
        stats.hits = bpe_sim::collect_events(scene, inputs);
        return stats;
    }
    let (half_w, half_h) = (scene.video.width as f32 * 0.5, scene.video.height as f32 * 0.5);
    let samples = (duration / SAMPLE_EVERY).ceil() as usize;
    for i in 1..=samples {
        sim.advance_to((i as f64 * SAMPLE_EVERY).min(duration), &mut stats.hits);
        let balls: Vec<_> = sim.balls().iter().filter(|b| b.alive).collect();
        let mut on_screen = 0;
        let (mut fastest, mut total): (f32, f32) = (0.0, 0.0);
        for ball in &balls {
            if !(ball.position.is_finite() && ball.velocity.is_finite()) {
                stats.broken = true;
                continue;
            }
            if ball.position.x.abs() <= half_w && ball.position.y.abs() <= half_h {
                on_screen += 1;
            }
            let speed = ball.velocity.length();
            fastest = fastest.max(speed);
            total += speed;
        }
        let count = balls.len().max(1) as f32;
        let fraction = if balls.is_empty() { 0.0 } else { on_screen as f32 / count };
        stats.samples.push((balls.len() as u32, fraction, fastest, total / count));
    }
    stats
}

/// Scores a scene from a simulation pass plus whatever `inputs` holds. Weights are applied by
/// the caller; this returns the total under the default weights.
pub fn score_scene(scene: &Scene, inputs: &ScoreInputs) -> Score {
    let stats = simulate(scene);
    Weights::default().score(parts(scene, &stats, inputs))
}

/// The parts of the score from a finished simulation pass.
pub fn parts(scene: &Scene, stats: &SimStats, inputs: &ScoreInputs) -> Parts {
    let activity = Activity::new(scene, stats);
    Parts {
        rhythm: rhythm(&activity, inputs.beats),
        build_up: build_up(&activity, stats),
        climax: climax(&activity, stats),
        motion: motion(&activity, stats),
        variety: (!inputs.frames.is_empty()).then(|| variety(inputs.frames)),
        sound: inputs.soundtrack.map(|track| sound_health(track, scene.sound.target_loudness)),
    }
}

/// Hits you can hear, sorted into slots.
struct Activity {
    /// Per slot: summed loudness of the hits in it, each hit 0 to 1.
    slots: Vec<f32>,
    /// Time of the first audible hit in each slot that has one.
    onsets: Vec<f64>,
    escapes: Vec<f64>,
}

impl Activity {
    fn new(scene: &Scene, stats: &SimStats) -> Self {
        let count = ((stats.duration / SLOT).ceil() as usize).max(1);
        let mut slots = vec![0.0f32; count];
        let mut first = vec![f64::NAN; count];
        let mut escapes = Vec::new();
        let quietest = scene.sound.min_impact_speed.max(0.0);
        let loudest = scene.sound.full_volume_speed.max(quietest + 1.0);
        for hit in &stats.hits {
            let slot = ((hit.time / SLOT) as usize).min(count - 1);
            let strength = match hit.kind {
                HitKind::Escape(_) => {
                    escapes.push(hit.time);
                    1.0
                }
                _ if !hit.kind.is_hit() || hit.speed < quietest => continue,
                _ => ((hit.speed - quietest) / (loudest - quietest)).clamp(0.05, 1.0),
            };
            slots[slot] += strength;
            if first[slot].is_nan() {
                first[slot] = hit.time;
            }
        }
        let onsets = first.into_iter().filter(|t| !t.is_nan()).collect();
        Self { slots, onsets, escapes }
    }

    /// Activity in each slot, each from 0 to 1 (a few loud hits fill a slot).
    fn level(&self, slot: usize) -> f32 {
        1.0 - (-self.slots[slot]).exp()
    }

    fn mean_level(&self, range: std::ops::Range<usize>) -> f32 {
        let len = range.len().max(1) as f32;
        range.map(|i| self.level(i)).sum::<f32>() / len
    }
}

/// 1 inside `low..high`, falling off smoothly (on a log scale) outside it.
fn band(value: f32, low: f32, high: f32) -> f32 {
    if value <= 0.0 {
        return 0.0;
    }
    let off = if value < low {
        (low / value).ln()
    } else if value > high {
        (value / high).ln()
    } else {
        0.0
    };
    (-off * off / (2.0 * 0.45 * 0.45)).exp()
}

fn rhythm(activity: &Activity, beats: &[f64]) -> f32 {
    let slots = activity.slots.len() as f32;
    // Share of slots with a sound in them: a few a second is pleasing, every slot is a wash.
    let busy = activity.onsets.len() as f32 / slots;
    let pace = band(busy, 0.1, 0.55);
    if activity.onsets.len() < 3 {
        return pace * 0.5;
    }
    let gaps: Vec<f64> = activity.onsets.windows(2).map(|w| w[1] - w[0]).collect();
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    let spread = (gaps.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / gaps.len() as f64).sqrt();
    let wobble = (spread / mean.max(1e-6)) as f32;
    // Some swing is fine; a wildly uneven pattern feels random.
    let regular = (-(wobble - 0.4).max(0.0) * 1.2).exp();
    if beats.is_empty() {
        return 0.65 * pace + 0.35 * regular;
    }
    let on_beat =
        activity.onsets.iter().filter(|&&t| near_beat(beats, t)).count() as f32 / activity.onsets.len() as f32;
    // Random hits land within 50 ms of a beat about a fifth of the time at 120 bpm.
    let beat_score = ((on_beat - 0.2) / 0.5).clamp(0.0, 1.0);
    0.4 * pace + 0.2 * regular + 0.4 * beat_score
}

/// Whether `time` is within [`ON_BEAT`] of a beat. `beats` must be sorted.
pub fn near_beat(beats: &[f64], time: f64) -> bool {
    let i = beats.partition_point(|&b| b < time);
    let after = beats.get(i).map_or(f64::INFINITY, |b| b - time);
    let before = i.checked_sub(1).map_or(f64::INFINITY, |j| time - beats[j]);
    after.min(before) <= ON_BEAT
}

/// A measure of how much is going on in samples `range`: sound, balls and speed.
fn energy(activity: &Activity, stats: &SimStats, from: f64, to: f64) -> [f32; 3] {
    let slot = |t: f64| ((t / SLOT) as usize).min(activity.slots.len());
    let sound = activity.mean_level(slot(from)..slot(to));
    let sample = |t: f64| ((t / SAMPLE_EVERY) as usize).min(stats.samples.len());
    let picked = &stats.samples[sample(from)..sample(to).max(sample(from))];
    let n = picked.len().max(1) as f32;
    let balls = picked.iter().map(|s| s.0 as f32).sum::<f32>() / n;
    let speed = picked.iter().map(|s| s.3).sum::<f32>() / n;
    [sound, balls, speed]
}

fn build_up(activity: &Activity, stats: &SimStats) -> f32 {
    let d = stats.duration;
    if d <= 0.0 {
        return 0.0;
    }
    let start = energy(activity, stats, 0.0, d / 3.0);
    let end = energy(activity, stats, d * 2.0 / 3.0, d);
    // Relative growth of each measure from the first third to the last, -1 to 1.
    let growth = |a: f32, b: f32| if a + b > 1e-6 { (b - a) / (a + b) } else { 0.0 };
    let g = 0.5 * growth(start[0], end[0]) + 0.3 * growth(start[1], end[1]) + 0.2 * growth(start[2], end[2]);
    // A video that goes silent scores low however it started.
    let alive = (end[0] / 0.05).min(1.0);
    (0.5 + 0.5 * (2.5 * g).tanh()) * (0.3 + 0.7 * alive)
}

fn climax(activity: &Activity, stats: &SimStats) -> f32 {
    let d = stats.duration;
    let slots = activity.slots.len();
    if d <= 0.0 || activity.onsets.is_empty() {
        return 0.0;
    }
    // The busiest second, found with a sliding window.
    let window = ((1.0 / SLOT) as usize).clamp(1, slots);
    let mut sum: f32 = (0..window).map(|i| activity.level(i)).sum();
    let (mut best, mut best_at) = (sum, 0);
    for i in window..slots {
        sum += activity.level(i) - activity.level(i - window);
        if sum > best + 1e-4 {
            best = sum;
            best_at = i + 1 - window;
        }
    }
    let peak_time = (best_at + window / 2) as f64 * SLOT;
    let peak = (peak_time / d) as f32;
    let peak_score = if peak >= 0.66 {
        1.0
    } else if peak >= 0.4 {
        0.4 + (peak - 0.4) / 0.26 * 0.6
    } else {
        0.25
    };
    let ending = if stats.rings > 0 {
        // Rings breaking right to the end make the climax; all of them breaking makes it great.
        let broken = activity.escapes.len() as f32 / stats.rings as f32;
        let last = activity.escapes.last().map_or(0.0, |t| (t / d) as f32);
        broken.min(1.0).sqrt() * (0.4 + 0.6 * ((last - 0.3) / 0.6).clamp(0.0, 1.0))
    } else {
        // Something still happening in the last part, compared with the whole.
        let overall = activity.mean_level(0..slots);
        let tail = activity.mean_level((slots * 85 / 100)..slots);
        if overall > 0.0 { (tail / overall / 1.2).min(1.0) } else { 0.0 }
    };
    0.55 * peak_score + 0.45 * ending
}

fn motion(activity: &Activity, stats: &SimStats) -> f32 {
    if stats.broken {
        return 0.0;
    }
    // Long silences feel like the video stopped.
    let quiet = longest_gap(&activity.onsets, stats.duration) as f32;
    let silence = (1.0 - ((quiet - 2.0) / 6.0).clamp(0.0, 1.0)).max(0.0);
    if stats.samples.is_empty() {
        // No ball positions (a 3D scene): judge on the sound alone.
        return 0.3 + 0.6 * silence;
    }
    let n = stats.samples.len() as f32;
    let stalled = stats.samples.iter().filter(|s| s.0 == 0 || s.2 < 15.0).count() as f32 / n;
    let on_screen = stats.samples.iter().map(|s| s.1).sum::<f32>() / n;
    // Balls multiplying until they hit the limit, then staying pinned there for most of the
    // video, looks like a glitch. Scenes that simply start with many balls are fine.
    let start = stats.samples.first().map_or(0, |s| s.0);
    let capped = stats.samples.iter().filter(|s| s.0 >= stats.max_balls.max(2) && s.0 > start).count() as f32 / n;
    let runaway = 1.0 - ((capped - 0.4) / 0.6).clamp(0.0, 1.0) * 0.6;
    (1.0 - stalled).powf(1.5) * (0.3 + 0.7 * on_screen) * (0.4 + 0.6 * silence) * runaway
}

/// The longest stretch with no audible hit, including before the first and after the last.
fn longest_gap(onsets: &[f64], duration: f64) -> f64 {
    let mut last = 0.0;
    let mut longest: f64 = 0.0;
    for &t in onsets {
        longest = longest.max(t - last);
        last = t;
    }
    longest.max(duration - last)
}

/// Lightness, colourfulness and hue (0 to 1 each) of an sRGB pixel.
fn pixel_color(p: &[u8]) -> (f32, f32, f32) {
    let (r, g, b) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let chroma = max - min;
    let hue = if chroma <= 1e-6 {
        0.0
    } else if max == r {
        ((g - b) / chroma).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / chroma + 2.0) / 6.0
    } else {
        ((r - g) / chroma + 4.0) / 6.0
    };
    (0.2126 * r + 0.7152 * g + 0.0722 * b, chroma, hue)
}

fn variety(frames: &[SampledFrame]) -> f32 {
    let mut coverage = Vec::new();
    let mut hues = [0.0f32; 12];
    let mut noise = Vec::new();
    for frame in frames {
        let pixels = frame.pixels.chunks_exact(4);
        let mut lightness: Vec<f32> = pixels.clone().map(|p| pixel_color(p).0).collect();
        if lightness.is_empty() {
            continue;
        }
        let colors: Vec<(f32, f32, f32)> = pixels.map(pixel_color).collect();
        // The background is about as light as the darkest tenth of the picture.
        let mut sorted = lightness.clone();
        sorted.sort_by(f32::total_cmp);
        let background = sorted[sorted.len() / 10];
        let lit = colors.iter().filter(|c| c.0 > background + 0.08).count();
        coverage.push(lit as f32 / colors.len() as f32);
        for &(l, chroma, hue) in &colors {
            if l > background + 0.08 && chroma > 0.15 {
                hues[((hue * 12.0) as usize).min(11)] += chroma;
            }
        }
        // Noise: how much neighbouring pixels differ, compared with the picture's contrast.
        let w = frame.width as usize;
        let mut edges = 0.0;
        for (i, pair) in lightness.windows(2).enumerate() {
            if (i + 1) % w != 0 {
                edges += (pair[1] - pair[0]).abs();
            }
        }
        let contrast = sorted[sorted.len() * 9 / 10] - background;
        noise.push(if contrast > 0.02 { edges / lightness.len() as f32 / contrast } else { 0.0 });
        lightness.clear();
    }
    if coverage.is_empty() {
        return 0.0;
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    let cover = band(mean(&coverage), 0.04, 0.45);
    let total: f32 = hues.iter().sum();
    let spread = if total > 0.0 {
        let entropy: f32 = hues.iter().filter(|&&h| h > 0.0).map(|&h| -(h / total) * (h / total).ln()).sum();
        // Two or three colours already look lively; one colour still gets something.
        (0.25 + entropy / 2.0_f32.ln().max(1e-6) * 0.5).min(1.0)
    } else {
        0.1
    };
    // Change between frames: small pictures, so compare lightness pixel by pixel.
    let mut changes = Vec::new();
    for pair in frames.windows(2) {
        if pair[0].pixels.len() == pair[1].pixels.len() && !pair[0].pixels.is_empty() {
            let diff: u64 = pair[0].pixels.iter().zip(&pair[1].pixels).map(|(a, b)| a.abs_diff(*b) as u64).sum();
            changes.push(diff as f32 / pair[0].pixels.len() as f32 / 255.0);
        }
    }
    let change = if changes.is_empty() { 0.5 } else { band(mean(&changes), 0.01, 0.2) };
    let calm = 1.0 - ((mean(&noise) - 0.25) / 0.5).clamp(0.0, 1.0);
    0.3 * cover + 0.25 * spread + 0.3 * change + 0.15 * calm
}

fn sound_health(track: &Soundtrack, target: f32) -> f32 {
    let samples = &track.samples;
    if samples.is_empty() {
        return 0.0;
    }
    let clipped = samples.iter().filter(|s| s.abs() >= 0.999).count() as f32 / samples.len() as f32;
    // Loudness of the louder half of 400 ms blocks, roughly how loudness meters gate quiet parts.
    let block = (SAMPLE_RATE as usize * 2 * 2 / 5).max(2);
    let mut levels: Vec<f32> = samples
        .chunks(block)
        .map(|c| c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32)
        .filter(|p| *p > 1e-9)
        .collect();
    if levels.is_empty() {
        return 0.0;
    }
    levels.sort_by(f32::total_cmp);
    let loud = &levels[levels.len() / 2..];
    let power = loud.iter().sum::<f32>() / loud.len() as f32;
    let loudness = 10.0 * power.log10();
    let off = (loudness - target) / 6.0;
    (-off * off).exp() * (-clipped * 2000.0).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Spawner, Vec2};

    fn sim_score(scene: &Scene) -> Score {
        score_scene(scene, &ScoreInputs::default())
    }

    #[test]
    fn parts_and_totals_stay_between_zero_and_one() {
        for preset in Scene::presets() {
            let mut scene = preset.scene;
            scene.video.duration = scene.video.duration.min(12.0);
            let score = sim_score(&scene);
            for (name, value, _) in score.parts.list() {
                if let Some(v) = value {
                    assert!((0.0..=1.0).contains(&v), "{} {name} = {v}", preset.name);
                }
            }
            assert!((0.0..=1.0).contains(&score.total), "{}", preset.name);
        }
    }

    #[test]
    fn nothing_happening_scores_low() {
        let still = Scene {
            physics: bpe_scene::Physics { gravity: Vec2::ZERO, ..Default::default() },
            spawners: vec![Spawner::Single {
                position: Vec2::ZERO,
                velocity: Vec2::ZERO,
                radius: 20.0,
                color: bpe_scene::Color::WHITE,
                depth: 0.0,
                depth_velocity: 0.0,
            }],
            ..Scene::default()
        };
        let score = sim_score(&still);
        assert!(score.total < 0.3, "{score:?}");
        let empty = Scene { spawners: Vec::new(), ..Scene::default() };
        assert!(sim_score(&empty).total < 0.3);
    }

    #[test]
    fn zero_length_and_empty_inputs_are_safe() {
        let scene = Scene { video: bpe_scene::Video { duration: 0.0, ..Default::default() }, ..Scene::default() };
        let score = score_scene(
            &scene,
            &ScoreInputs { beats: &[], soundtrack: Some(&Soundtrack::default()), frames: &[SampledFrame::default()] },
        );
        assert!(score.total.is_finite());
        assert_eq!(score.parts.sound, Some(0.0));
    }

    #[test]
    fn on_beat_hits_score_higher_rhythm() {
        let scene = Scene::growing_ball();
        let stats = simulate(&scene);
        let onsets: Vec<f64> = Activity::new(&scene, &stats).onsets;
        // Beats exactly on the hits, against beats halfway between them.
        let on: Vec<f64> = onsets.clone();
        let off: Vec<f64> = onsets.windows(2).map(|w| (w[0] + w[1]) * 0.5).collect();
        let with = |beats: &[f64]| parts(&scene, &stats, &ScoreInputs { beats, ..Default::default() }).rhythm;
        assert!(with(&on) > with(&off) + 0.2, "{} vs {}", with(&on), with(&off));
    }

    #[test]
    fn near_beat_finds_the_closest_beat() {
        let beats = [1.0, 2.0, 3.0];
        assert!(near_beat(&beats, 2.04));
        assert!(near_beat(&beats, 0.96));
        assert!(!near_beat(&beats, 2.5));
        assert!(!near_beat(&[], 1.0));
        assert!(near_beat(&beats, 3.03));
    }

    #[test]
    fn weights_change_the_total_and_learn_from_thumbs() {
        let high_rhythm = Parts { rhythm: 1.0, build_up: 0.5, climax: 0.5, motion: 0.5, variety: None, sound: None };
        let high_climax = Parts { rhythm: 0.5, climax: 1.0, ..high_rhythm };
        let mut weights = Weights::default();
        let before = weights.total(&high_rhythm) - weights.total(&high_climax);
        for _ in 0..10 {
            weights.learn(&high_rhythm, true);
            weights.learn(&high_climax, false);
        }
        let after = weights.total(&high_rhythm) - weights.total(&high_climax);
        assert!(after > before + 0.05, "{before} -> {after}");
        assert!(weights.array().iter().all(|w| (WEIGHT_RANGE.0..=WEIGHT_RANGE.1).contains(w)));
        // Unmeasured parts leave their weights alone.
        assert_eq!(weights.variety, Weights::default().variety);
    }

    #[test]
    fn total_ignores_unmeasured_parts() {
        let parts = Parts { rhythm: 1.0, build_up: 1.0, climax: 1.0, motion: 1.0, variety: None, sound: None };
        assert!((Weights::default().total(&parts) - 1.0).abs() < 1e-6);
        let zero = Weights { rhythm: 0.0, build_up: 0.0, climax: 0.0, motion: 0.0, variety: 0.0, sound: 0.0 };
        assert_eq!(zero.total(&parts), 0.0);
    }

    fn frame(fill: impl Fn(u32, u32) -> [u8; 4]) -> SampledFrame {
        let (w, h) = (27, 48);
        let pixels = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| fill(x, y)).collect();
        SampledFrame { width: w, height: h, pixels }
    }

    #[test]
    fn variety_prefers_colourful_changing_pictures_to_blank_or_noise() {
        let blank: Vec<SampledFrame> = (0..16).map(|_| frame(|_, _| [5, 5, 20, 255])).collect();
        let lively: Vec<SampledFrame> = (0..16)
            .map(|i| {
                frame(move |x, y| {
                    let d = ((x as i32 - 13 - (i % 4)).pow(2) + (y as i32 - 24).pow(2)) as f32;
                    if d < 60.0 {
                        [255, (i * 15) as u8, 120, 255]
                    } else if x < 5 {
                        [40, 220, 255, 255]
                    } else {
                        [5, 5, 20, 255]
                    }
                })
            })
            .collect();
        let noise: Vec<SampledFrame> = (0..16u32)
            .map(|i| {
                frame(move |x, y| {
                    let n = (x.wrapping_mul(7919) ^ y.wrapping_mul(104729) ^ i.wrapping_mul(1299709))
                        .wrapping_mul(2654435761)
                        >> 24;
                    [n as u8, (n * 3) as u8, (n * 7) as u8, 255]
                })
            })
            .collect();
        let (b, l, n) = (variety(&blank), variety(&lively), variety(&noise));
        assert!(l > b + 0.2 && l > n + 0.1, "blank {b}, lively {l}, noise {n}");
    }

    #[test]
    fn shrink_averages_blocks() {
        let pixels: Vec<u8> =
            (0..8 * 4).flat_map(|i| if i % 2 == 0 { [0, 0, 0, 255] } else { [200, 100, 50, 255] }).collect();
        let small = SampledFrame::shrink(&pixels, 8, 4, 4);
        assert_eq!((small.width, small.height), (4, 2));
        assert_eq!(&small.pixels[..4], &[100, 50, 25, 255]);
    }

    #[test]
    fn sound_health_wants_target_loudness_without_clipping() {
        let tone = |amplitude: f32| Soundtrack {
            samples: (0..SAMPLE_RATE as usize * 4).map(|i| (i as f32 * 0.05).sin() * amplitude).collect(),
        };
        // A sine at amplitude 0.28 is about -14 dB.
        let good = sound_health(&tone(0.28), -14.0);
        let quiet = sound_health(&tone(0.01), -14.0);
        let clipping = sound_health(&Soundtrack { samples: vec![1.0; SAMPLE_RATE as usize] }, -14.0);
        assert!(good > 0.9, "{good}");
        assert!(quiet < 0.1 && clipping < 0.01, "{quiet} {clipping}");
        assert_eq!(sound_health(&Soundtrack::silent(2.0), -14.0), 0.0);
    }

    #[test]
    fn simulation_pass_matches_the_real_hits() {
        let scene = Scene::escape_rings();
        let stats = simulate(&scene);
        assert_eq!(stats.hits, bpe_sim::collect_wall_hits(&scene));
        assert_eq!(stats.rings, 10);
        assert!(!stats.broken);
    }

    #[test]
    fn hand_made_scenes_rank_in_order() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenes/score-tests");
        let mut good = Vec::new();
        let mut bad = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.extension().is_none_or(|e| e != "ron") {
                continue;
            }
            let scene = Scene::load(&path).unwrap();
            let hits = bpe_sim::collect_wall_hits(&scene);
            let track = bpe_audio::render_soundtrack(&scene, &hits, None).unwrap();
            let total = score_scene(&scene, &ScoreInputs { soundtrack: Some(&track), ..Default::default() }).total;
            if name.starts_with("good-") {
                good.push((total, name));
            } else if name.starts_with("bad-") {
                bad.push((total, name));
            }
        }
        assert!(good.len() >= 3 && bad.len() >= 3);
        let worst_good = good.iter().cloned().fold((f32::MAX, String::new()), |a, b| if b.0 < a.0 { b } else { a });
        let best_bad = bad.iter().cloned().fold((f32::MIN, String::new()), |a, b| if b.0 > a.0 { b } else { a });
        assert!(worst_good.0 > best_bad.0 + 0.1, "{worst_good:?} should beat {best_bad:?}\ngood {good:?}\nbad {bad:?}");
    }
}
