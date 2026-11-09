//! The Sampler (W11): a sound file played back as an oscillator. [`SampleData`] is a decoded
//! file at the engine's rate (left and right, with octave-down copies for short samples so
//! pitching up doesn't alias, and peaks for the editor's waveforms); [`SamplerVoice`] is one note's
//! place in it, read with 4-point Hermite interpolation. Nothing here allocates once the data is
//! made.

use crate::BLOCK;

/// Frames each peak covers.
pub const PEAK_FRAMES: usize = 64;
/// Samples up to this many frames (30 s) get octave-down copies; longer ones (a song) play
/// pitched up from the full copy only.
pub const MIP_UP_TO: usize = 30 * 48_000;
/// The most copies a sample has: the full one and four octaves down.
pub const MAX_LEVELS: usize = 5;
/// How far a copy may be read faster than its own speed before the next copy down is used, in
/// octaves (about 1.5 times: up to a fifth up reads the full copy, keeping its top end).
const LEVEL_SLACK: f64 = 0.6;
/// Half the taps of the halfband filter the copies are made with.
const HALF_TAPS: usize = 32;

/// A decoded sound at the engine's rate.
#[derive(Debug)]
pub struct SampleData {
    /// Level 0 is the sound itself; level k is k octaves down (every 2^k-th frame, filtered).
    levels: Vec<[Vec<f32>; 2]>,
    frames: usize,
    /// The largest absolute value of either channel in each [`PEAK_FRAMES`] frames.
    peaks: Vec<f32>,
}

impl SampleData {
    /// The sound `left`/`right` (the same length; the shorter is padded with silence), with its
    /// copies and peaks made.
    pub fn new(mut left: Vec<f32>, mut right: Vec<f32>) -> SampleData {
        let frames = left.len().max(right.len());
        left.resize(frames, 0.0);
        right.resize(frames, 0.0);
        let peaks = left
            .chunks(PEAK_FRAMES)
            .zip(right.chunks(PEAK_FRAMES))
            .map(|(l, r)| l.iter().chain(r).fold(0.0f32, |m, v| m.max(v.abs())))
            .collect();
        let mut levels = vec![[left, right]];
        if frames <= MIP_UP_TO {
            while levels.len() < MAX_LEVELS && levels.last().is_some_and(|l| l[0].len() >= 2 * HALF_TAPS) {
                let [l, r] = levels.last().expect("a level");
                let down = [halve(l), halve(r)];
                levels.push(down);
            }
        }
        SampleData { levels, frames, peaks }
    }

    /// Frames in the sound.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Its length in seconds at the engine's rate.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / crate::SAMPLE_RATE as f64
    }

    /// How many copies it has (1 for a long sound).
    pub fn levels(&self) -> usize {
        self.levels.len()
    }

    /// The peaks, one per [`PEAK_FRAMES`] frames.
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    /// The left and right channel of the sound itself.
    pub fn channels(&self) -> (&[f32], &[f32]) {
        (&self.levels[0][0], &self.levels[0][1])
    }

    /// Frame `i` of the sound (silence outside it).
    pub fn frame(&self, i: i64) -> (f32, f32) {
        if i < 0 || i as usize >= self.frames {
            return (0.0, 0.0);
        }
        (self.levels[0][0][i as usize], self.levels[0][1][i as usize])
    }

    /// The copy to read for playing at `step` times the sound's speed.
    pub fn level_for(&self, step: f64) -> usize {
        if step <= (2.0f64).powf(LEVEL_SLACK) {
            return 0;
        }
        ((step.log2() - LEVEL_SLACK).ceil().max(0.0) as usize).min(self.levels.len() - 1)
    }

    /// Left and right at `pos` frames of the sound (fractional), read from copy `level`.
    fn read(&self, level: usize, pos: f64) -> (f32, f32) {
        let [l, r] = &self.levels[level];
        let q = pos / (1u64 << level) as f64;
        let i = q.floor();
        let f = (q - i) as f32;
        let i = i as i64;
        (hermite(l, i, f), hermite(r, i, f))
    }
}

/// `a` read between `a[i]` and `a[i + 1]` at `f` (0..1), silence outside it.
fn hermite(a: &[f32], i: i64, f: f32) -> f32 {
    let at = |k: i64| -> f32 {
        let j = i + k;
        if j < 0 || j as usize >= a.len() { 0.0 } else { a[j as usize] }
    };
    let (ym, y0, y1, y2) = (at(-1), at(0), at(1), at(2));
    let c1 = 0.5 * (y1 - ym);
    let c2 = ym - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - ym) + 1.5 * (y0 - y1);
    ((c3 * f + c2) * f + c1) * f + y0
}

/// The halfband filter's taps: a windowed sinc cutting at half the band (Blackman-Harris).
fn halfband_taps() -> [f32; 2 * HALF_TAPS + 1] {
    let n = HALF_TAPS as f64;
    let mut taps = [0.0f32; 2 * HALF_TAPS + 1];
    let mut sum = 0.0;
    for (k, t) in taps.iter_mut().enumerate() {
        let x = k as f64 - n;
        let sinc = if x == 0.0 { 0.5 } else { (std::f64::consts::PI * 0.5 * x).sin() / (std::f64::consts::PI * x) };
        let w = x / (n + 1.0);
        let pi = std::f64::consts::PI;
        let window =
            0.35875 + 0.48829 * (pi * w).cos() + 0.14128 * (2.0 * pi * w).cos() + 0.01168 * (3.0 * pi * w).cos();
        let v = sinc * window;
        *t = v as f32;
        sum += v;
    }
    for t in &mut taps {
        *t = (*t as f64 / sum) as f32;
    }
    taps
}

/// `a` an octave down: filtered, then every other frame (frame j of the result is frame 2j).
fn halve(a: &[f32]) -> Vec<f32> {
    let taps = halfband_taps();
    let n = a.len().div_ceil(2);
    (0..n)
        .map(|j| {
            let centre = 2 * j as i64;
            let mut acc = 0.0f32;
            for (k, t) in taps.iter().enumerate() {
                let i = centre + k as i64 - HALF_TAPS as i64;
                if i >= 0 && (i as usize) < a.len() {
                    acc += t * a[i as usize];
                }
            }
            acc
        })
        .collect()
}

/// How a note goes on past its loop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopMode {
    /// Plays once and goes silent at the end.
    #[default]
    Off,
    /// From Loop end back to Loop start, over and over.
    Forward,
    /// Back and forth between Loop start and Loop end.
    PingPong,
}

/// Where a note starts and how it loops, as shares of the sound's length (0..1), counted on the
/// reversed sound when `reverse`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplerParams {
    pub start: f64,
    pub loop_mode: LoopMode,
    pub loop_start: f64,
    pub loop_end: f64,
    /// Frames before Loop end that crossfade into those before Loop start (Forward only).
    pub fade_frames: f64,
    pub reverse: bool,
}

impl Default for SamplerParams {
    fn default() -> Self {
        Self { start: 0.0, loop_mode: LoopMode::Off, loop_start: 0.0, loop_end: 1.0, fade_frames: 0.0, reverse: false }
    }
}

/// A loop in frames: its start, end and fade, and whether it runs back and forth.
#[derive(Clone, Copy, Debug)]
struct Loop {
    start: f64,
    end: f64,
    fade: f64,
    ping: bool,
}

/// The loop, when there is one to play.
fn loop_of(data: &SampleData, p: &SamplerParams) -> Option<Loop> {
    if p.loop_mode == LoopMode::Off {
        return None;
    }
    let n = data.frames as f64;
    let (a, b) = (p.loop_start.clamp(0.0, 1.0) * n, p.loop_end.clamp(0.0, 1.0) * n);
    // Ends set the wrong way round still loop between them.
    let (ls, le) = (a.min(b), a.max(b));
    if le - ls < 2.0 {
        return None;
    }
    let fade = if p.loop_mode == LoopMode::Forward { p.fade_frames.max(0.0).min(ls).min((le - ls) * 0.5) } else { 0.0 };
    Some(Loop { start: ls, end: le, fade, ping: p.loop_mode == LoopMode::PingPong })
}

/// One note's place in a sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct SamplerVoice {
    /// Where it is, in frames of the sound as it plays (reversed when Reverse is on).
    pos: f64,
    /// Ping-pong: running backwards.
    back: bool,
    /// It has played into its loop's range (the loop takes over at Loop end).
    looping: bool,
    /// Samples of the next block to wait before it starts.
    wait: u32,
    done: bool,
}

impl SamplerVoice {
    /// A note starts `at` samples into the next block rendered, at Start.
    pub fn note_on(&mut self, data: &SampleData, p: &SamplerParams, at: u32) {
        let n = data.frames as f64;
        self.pos = p.start.clamp(0.0, 1.0) * n;
        self.back = false;
        self.looping = loop_of(data, p).is_some_and(|l| self.pos < l.end);
        self.wait = at;
        self.done = data.frames == 0 || self.pos >= n;
    }

    /// Whether it has played to the end (a one-shot) and is silent from now on.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Where it is, in frames of the sound as it plays.
    pub fn position(&self) -> f64 {
        self.pos
    }

    /// Adds a block of the note at `step` times the sound's speed, scaled by `gain` sample by
    /// sample, to `out_l`/`out_r`.
    pub fn render(
        &mut self,
        data: &SampleData,
        p: &SamplerParams,
        step: f64,
        gain: &[f32; BLOCK],
        out_l: &mut [f32; BLOCK],
        out_r: &mut [f32; BLOCK],
    ) {
        let step = if step.is_finite() { step.max(0.0) } else { 0.0 };
        let level = data.level_for(step);
        let n = data.frames as f64;
        let lp = loop_of(data, p);
        let read = |v: f64| -> (f32, f32) {
            let phys = if p.reverse { n - 1.0 - v } else { v };
            data.read(level, phys)
        };
        for s in 0..BLOCK {
            if self.wait > 0 {
                self.wait -= 1;
                continue;
            }
            if self.done {
                break;
            }
            let x = self.pos;
            let (mut l, mut r) = read(x);
            if let Some(Loop { start: ls, end: le, fade, .. }) = lp
                && self.looping
                && fade > 0.0
                && x >= le - fade
                && x < le
            {
                let t = ((x - (le - fade)) / fade) as f32;
                let (a, b) = ((t * std::f32::consts::FRAC_PI_2).cos(), (t * std::f32::consts::FRAC_PI_2).sin());
                let (bl, br) = read(x - (le - ls));
                l = l * a + bl * b;
                r = r * a + br * b;
            }
            out_l[s] += l * gain[s];
            out_r[s] += r * gain[s];
            self.advance(step, lp, n);
        }
    }

    fn advance(&mut self, step: f64, lp: Option<Loop>, n: f64) {
        match lp {
            Some(Loop { start: ls, end: le, ping, .. }) if self.looping => {
                let len = le - ls;
                if !self.back {
                    self.pos += step;
                    if self.pos >= le {
                        if ping {
                            self.pos = le - (self.pos - le).rem_euclid(len);
                            self.back = true;
                        } else {
                            self.pos = ls + (self.pos - le).rem_euclid(len);
                        }
                    }
                } else {
                    self.pos -= step;
                    if self.pos <= ls {
                        self.pos = ls + (ls - self.pos).rem_euclid(len);
                        self.back = false;
                    }
                }
            }
            _ => {
                self.pos += step;
                if self.pos >= n {
                    self.done = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sine;

    fn data(v: Vec<f32>) -> SampleData {
        SampleData::new(v.clone(), v)
    }

    /// `blocks` blocks of the voice at `step`, left channel, at full gain.
    fn play(v: &mut SamplerVoice, d: &SampleData, p: &SamplerParams, step: f64, blocks: usize) -> Vec<f32> {
        let gain = [1.0; BLOCK];
        let mut out = Vec::new();
        for _ in 0..blocks {
            let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
            v.render(d, p, step, &gain, &mut l, &mut r);
            out.extend_from_slice(&l);
        }
        out
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn at_its_own_speed_it_plays_the_sound_itself() {
        let s = sine(441.0, 0.8, 0.3, 4800);
        let d = data(s.clone());
        let p = SamplerParams::default();
        let mut v = SamplerVoice::default();
        v.note_on(&d, &p, 0);
        let out = play(&mut v, &d, &p, 1.0, 80);
        for i in 0..4800 {
            assert!((out[i] - s[i]).abs() < 1e-6, "{i}");
        }
        assert!(out[4800..].iter().all(|x| *x == 0.0), "silent after the end");
        assert!(v.is_done());
    }

    #[test]
    fn twice_the_speed_is_an_octave_up() {
        let d = data(sine(200.0, 0.8, 0.0, 48_000));
        let p = SamplerParams::default();
        let mut v = SamplerVoice::default();
        v.note_on(&d, &p, 0);
        let out = play(&mut v, &d, &p, 2.0, 375);
        // Half a second of output holds the whole second of sound: 200 rises.
        let rises = crossings(&out[..24_000]);
        assert!((198..=201).contains(&rises), "{rises}");
        assert!(d.levels() > 1 && d.level_for(2.0) == 1 && d.level_for(1.1) == 0);
    }

    #[test]
    fn a_note_waits_for_its_start_and_starts_at_start() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let d = data(ramp);
        let p = SamplerParams { start: 0.5, ..SamplerParams::default() };
        let mut v = SamplerVoice::default();
        v.note_on(&d, &p, 10);
        let out = play(&mut v, &d, &p, 1.0, 1);
        assert!(out[..10].iter().all(|x| *x == 0.0));
        assert!((out[10] - 0.5).abs() < 1e-6, "{}", out[10]);
        assert!((out[11] - 0.501).abs() < 1e-6);
    }

    #[test]
    fn a_forward_loop_keeps_sounding_round_its_span() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let d = data(ramp);
        let p =
            SamplerParams { loop_mode: LoopMode::Forward, loop_start: 0.2, loop_end: 0.6, ..SamplerParams::default() };
        let mut v = SamplerVoice::default();
        v.note_on(&d, &p, 0);
        let out = play(&mut v, &d, &p, 1.0, 100);
        assert!(!v.is_done());
        // 0..599, then 200..599 over and over.
        assert!((out[599] - 0.599).abs() < 1e-6);
        assert!((out[600] - 0.2).abs() < 1e-6, "{}", out[600]);
        assert!((out[1000] - 0.2).abs() < 1e-6, "{}", out[1000]);
        assert!(out[5000..].iter().all(|x| (0.199..0.6).contains(x)));
    }

    #[test]
    fn a_loop_fade_crossfades_into_the_stretch_before_loop_start() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let d = data(ramp);
        let p = SamplerParams {
            loop_mode: LoopMode::Forward,
            loop_start: 0.4,
            loop_end: 0.8,
            fade_frames: 100.0,
            ..SamplerParams::default()
        };
        let mut v = SamplerVoice::default();
        v.note_on(&d, &p, 0);
        let out = play(&mut v, &d, &p, 1.0, 20);
        // Just before Loop end it is nearly all the stretch before Loop start, so the jump to Loop
        // start is smooth.
        assert!((out[799] - 0.399).abs() < 0.02, "{}", out[799]);
        assert!((out[800] - 0.4).abs() < 1e-6);
        assert!((out[699] - 0.699).abs() < 1e-6, "untouched before the fade");
    }

    #[test]
    fn ping_pong_turns_at_both_ends() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let d = data(ramp);
        let p =
            SamplerParams { loop_mode: LoopMode::PingPong, loop_start: 0.2, loop_end: 0.6, ..SamplerParams::default() };
        let mut v = SamplerVoice::default();
        v.note_on(&d, &p, 0);
        let out = play(&mut v, &d, &p, 1.0, 40);
        assert!(out[620] < out[610] && out[610] < 0.6, "going back");
        let low = out[600..1200].iter().cloned().fold(1.0f32, f32::min);
        assert!((low - 0.2).abs() < 0.003, "{low}");
        assert!(out[1100] > out[1090], "forward again");
    }

    #[test]
    fn reverse_plays_back_to_front() {
        let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let d = data(ramp);
        let p = SamplerParams { reverse: true, ..SamplerParams::default() };
        let mut v = SamplerVoice::default();
        v.note_on(&d, &p, 0);
        let out = play(&mut v, &d, &p, 1.0, 16);
        assert!((out[0] - 0.999).abs() < 1e-6);
        assert!((out[999] - 0.0).abs() < 1e-6);
        assert!(v.is_done());
    }

    #[test]
    fn long_sounds_get_no_copies_and_peaks_cover_the_sound() {
        let d = data(vec![0.25; MIP_UP_TO + 1]);
        assert_eq!(d.levels(), 1);
        assert_eq!(d.level_for(4.0), 0);
        assert_eq!(d.peaks().len(), (MIP_UP_TO + 1).div_ceil(PEAK_FRAMES));
        assert!(d.peaks().iter().all(|p| *p == 0.25));
    }

    #[test]
    fn an_octave_down_copy_keeps_low_notes_and_drops_high_ones() {
        let low = data(sine(1000.0, 1.0, 0.0, 9600));
        let high = data(sine(20_000.0, 1.0, 0.0, 9600));
        let rms = |d: &SampleData| {
            let x = &d.levels[1][0][200..4600];
            (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
        };
        assert!((rms(&low) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "{}", rms(&low));
        assert!(rms(&high) < 0.01, "{}", rms(&high));
    }
}
