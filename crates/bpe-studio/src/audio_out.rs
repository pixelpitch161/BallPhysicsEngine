//! The speakers: the default output device, fed from a ring of 48 kHz stereo frames that the
//! sound thread fills. The device may run at any rate; at another rate than 48 kHz the frames
//! are resampled in the callback. The callback never allocates, locks or waits. Every frame
//! waits 2 ms in a short line before it is heard, so when the ring runs dry the sound fades out
//! over what the line holds (no click), the clock stops, and the dropout is counted.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use rubato::{Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// The sound's own rate.
pub const RATE: u32 = 48_000;
/// Frames the ring holds: a quarter of a second.
pub const RING_FRAMES: usize = RATE as usize / 4;
/// Frames of fade in and out (2 ms): after a clear or a dropout, and before them.
const FADE_FRAMES: u32 = 96;
/// Device frames each frame waits before it is heard, so a dropout has them to fade out over.
const LINE: usize = FADE_FRAMES as usize;
/// Device frames the resampler makes at a time.
const RESAMPLE_CHUNK: usize = 256;

/// What the sound thread, the callback and the editor share.
#[derive(Debug)]
pub struct Shared {
    /// 48 kHz frames played since the ring was last cleared.
    pub(crate) played: AtomicU64,
    /// The listening volume, 0..1, as `f32` bits.
    volume: AtomicU32,
    muted: AtomicBool,
    /// The clear the sound thread asks for, and the last one the callback did.
    clear_wanted: AtomicU64,
    pub(crate) cleared: AtomicU64,
    /// The device stopped working (unplugged, taken away).
    failed: AtomicBool,
    /// The sound thread is playing (set by it): only then is a dry ring a dropout.
    playing: AtomicBool,
    /// Dropouts so far: the ring ran dry while playing, or the device missed a turn.
    underruns: AtomicU64,
    /// 48 kHz frames between a frame being taken off the ring and being heard (the device's
    /// own delay, half its buffer, the 2 ms line and the resampler), smoothed, as `f32` bits.
    delay: AtomicU32,
    /// The listener's own speaker delay (Bluetooth headphones), in 48 kHz frames.
    speaker_delay: AtomicU32,
}

impl Default for Shared {
    fn default() -> Self {
        Shared {
            played: AtomicU64::new(0),
            volume: AtomicU32::new(1.0f32.to_bits()),
            muted: AtomicBool::new(false),
            clear_wanted: AtomicU64::new(0),
            cleared: AtomicU64::new(0),
            failed: AtomicBool::new(false),
            playing: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
            delay: AtomicU32::new(0.0f32.to_bits()),
            speaker_delay: AtomicU32::new(0),
        }
    }
}

impl Shared {
    pub fn played(&self) -> u64 {
        self.played.load(Ordering::Acquire)
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    fn gain(&self) -> f32 {
        if self.muted.load(Ordering::Relaxed) { 0.0 } else { f32::from_bits(self.volume.load(Ordering::Relaxed)) }
    }

    /// Asks the callback to throw away what the ring holds and count from 0 again.
    pub fn ask_clear(&self, generation: u64) {
        self.clear_wanted.store(generation, Ordering::Release);
    }

    /// The last clear the callback did.
    pub fn cleared(&self) -> u64 {
        self.cleared.load(Ordering::Acquire)
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Set by the sound thread: it is playing and keeping the ring filled.
    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }

    /// Dropouts so far (the sound couldn't keep up, or the device missed a turn).
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    fn count_underrun(&self) {
        if self.playing.load(Ordering::Relaxed) {
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 48 kHz frames from taking a frame off the ring to hearing it: the device's delay and the
    /// listener's speaker delay.
    pub fn heard_delay(&self) -> u64 {
        let device = f32::from_bits(self.delay.load(Ordering::Relaxed));
        let device = if device.is_finite() { device.max(0.0) as u64 } else { 0 };
        device + self.speaker_delay.load(Ordering::Relaxed) as u64
    }

    /// The speaker delay, in milliseconds (0..500).
    pub fn set_speaker_delay_ms(&self, ms: f32) {
        let frames = (ms.clamp(0.0, 500.0) * RATE as f32 / 1000.0).round() as u32;
        self.speaker_delay.store(frames, Ordering::Relaxed);
    }

    /// Takes in the device's latest delay estimate (48 kHz frames), smoothing it.
    fn note_delay(&self, frames: f32) {
        if !frames.is_finite() {
            return;
        }
        let old = f32::from_bits(self.delay.load(Ordering::Relaxed));
        let new = if old <= 0.0 { frames } else { old + (frames - old) * 0.05 };
        self.delay.store(new.to_bits(), Ordering::Relaxed);
    }
}

/// A new ring for one sound thread and one feeder.
pub fn ring() -> (Producer<f32>, Consumer<f32>) {
    RingBuffer::new(RING_FRAMES * 2)
}

/// 48 kHz to the device's rate.
struct Resampling {
    resampler: SincFixedOut<f32>,
    input: [Vec<f32>; 2],
    output: [Vec<f32>; 2],
    /// The next frame of `output` to hand out, and how many it holds.
    at: usize,
    len: usize,
    /// 48 kHz frames per device frame.
    step: f64,
    played: f64,
    /// Device frames the resampler holds back.
    delay: usize,
}

impl Resampling {
    fn new(device_rate: u32) -> Option<Resampling> {
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Linear,
            window: WindowFunction::BlackmanHarris2,
        };
        let ratio = device_rate as f64 / RATE as f64;
        let resampler = SincFixedOut::<f32>::new(ratio, 1.0, params, RESAMPLE_CHUNK, 2).ok()?;
        let (inputs, outputs) = (resampler.input_frames_max(), resampler.output_frames_max());
        let delay = resampler.output_delay();
        Some(Resampling {
            resampler,
            input: [vec![0.0; inputs], vec![0.0; inputs]],
            output: [vec![0.0; outputs], vec![0.0; outputs]],
            at: 0,
            len: 0,
            step: RATE as f64 / device_rate as f64,
            played: 0.0,
            delay,
        })
    }

    fn reset(&mut self) {
        self.resampler.reset();
        self.at = 0;
        self.len = 0;
        self.played = 0.0;
    }
}

/// The callback's side: takes frames off the ring (resampled if need be), counts them, and
/// hands them out through the 2 ms line.
pub struct Feeder {
    ring: Consumer<f32>,
    shared: Arc<Shared>,
    resampling: Option<Resampling>,
    seen_clear: u64,
    /// A clear under way: the old sound fades out over the line first.
    clearing: bool,
    /// The line: device frames waiting to be heard, oldest at `head`.
    line: [[f32; 2]; LINE],
    head: usize,
    filled: usize,
    /// The level, 0..=[`FADE_FRAMES`] (of full): rising while frames flow, falling while the
    /// line drains.
    level: u32,
    /// The ring ran dry (or a clear came): the line is draining, fading out.
    draining: bool,
    /// 48 kHz frames per device frame.
    step: f64,
}

impl Feeder {
    pub fn new(ring: Consumer<f32>, shared: Arc<Shared>, device_rate: u32) -> Feeder {
        let resampling = if device_rate == RATE { None } else { Resampling::new(device_rate) };
        Feeder {
            ring,
            shared,
            resampling,
            seen_clear: 0,
            clearing: false,
            line: [[0.0; 2]; LINE],
            head: 0,
            filled: 0,
            level: 0,
            draining: false,
            step: RATE as f64 / device_rate.max(1) as f64,
        }
    }

    /// 48 kHz frames waiting in the ring.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn buffered_frames(&self) -> usize {
        self.ring.slots() / 2
    }

    /// Starts a clear the sound thread asked for: the old sound fades out over the line (2 ms),
    /// then what the ring still holds is dropped and the count starts from 0.
    pub fn poll_clear(&mut self) {
        let wanted = self.shared.clear_wanted.load(Ordering::Acquire);
        if wanted != self.seen_clear && !self.clearing {
            self.clearing = true;
            // Fading out from where the level is, so it never jumps.
            self.draining = self.filled > 0 && self.level > 0;
        }
    }

    /// Ends a clear: drops what the ring and the line hold and counts from 0.
    fn finish_clear(&mut self) {
        let wanted = self.shared.clear_wanted.load(Ordering::Acquire);
        self.clearing = false;
        let stale = self.ring.slots();
        if let Ok(chunk) = self.ring.read_chunk(stale) {
            chunk.commit_all();
        }
        if let Some(r) = &mut self.resampling {
            r.reset();
        }
        self.head = 0;
        self.filled = 0;
        self.level = 0;
        self.draining = false;
        self.seen_clear = wanted;
        self.shared.played.store(0, Ordering::Release);
        self.shared.cleared.store(wanted, Ordering::Release);
    }

    fn pop(&mut self) -> Option<[f32; 2]> {
        if self.ring.slots() < 2 {
            return None;
        }
        let l = self.ring.pop().ok()?;
        let r = self.ring.pop().ok()?;
        Some([l, r])
    }

    /// The next device frame, before the volume; `None` when the ring has run dry.
    fn next_raw(&mut self) -> Option<[f32; 2]> {
        let Some(mut r) = self.resampling.take() else {
            let frame = self.pop()?;
            self.shared.played.fetch_add(1, Ordering::AcqRel);
            return Some(frame);
        };
        let frame = self.next_resampled(&mut r);
        self.resampling = Some(r);
        frame
    }

    fn next_resampled(&mut self, r: &mut Resampling) -> Option<[f32; 2]> {
        if r.at >= r.len {
            let need = r.resampler.input_frames_next();
            if self.ring.slots() < need * 2 {
                return None;
            }
            for i in 0..need {
                let [a, b] = self.pop()?;
                r.input[0][i] = a;
                r.input[1][i] = b;
            }
            let input = [&r.input[0][..need], &r.input[1][..need]];
            let (_, made) = r.resampler.process_into_buffer(&input, &mut r.output, None).ok()?;
            r.at = 0;
            r.len = made;
            if made == 0 {
                return None;
            }
        }
        let frame = [r.output[0][r.at], r.output[1][r.at]];
        r.at += 1;
        let before = r.played as u64;
        r.played += r.step;
        self.shared.played.fetch_add(r.played as u64 - before, Ordering::AcqRel);
        Some(frame)
    }

    /// Takes the line's oldest frame out.
    fn take_oldest(&mut self) -> [f32; 2] {
        let frame = self.line[self.head];
        self.head = (self.head + 1) % LINE;
        self.filled -= 1;
        frame
    }

    fn put(&mut self, frame: [f32; 2]) {
        self.line[(self.head + self.filled) % LINE] = frame;
        self.filled += 1;
    }

    /// The next device frame at the listening volume. Frames come out of the line 2 ms after
    /// they went in; when the ring runs dry the line drains, fading out, then it is silent until
    /// it has filled again and fades back in.
    pub fn next_frame(&mut self) -> [f32; 2] {
        if self.clearing && !self.draining {
            self.finish_clear();
        }
        let input = if self.clearing { None } else { self.next_raw() };
        let out = if self.draining {
            let out = if self.level > 0 && self.filled > 0 {
                self.level -= 1;
                let [l, r] = self.take_oldest();
                let g = self.level as f32 / FADE_FRAMES as f32;
                [l * g, r * g]
            } else {
                [0.0, 0.0]
            };
            if self.level == 0 || self.filled == 0 {
                // Faded out: from here the line fills again before anything is heard.
                self.draining = false;
                self.level = 0;
            }
            if let Some(f) = input {
                self.put(f);
            }
            out
        } else if self.filled == LINE {
            match input {
                Some(f) => {
                    let [l, r] = self.take_oldest();
                    self.put(f);
                    self.level = (self.level + 1).min(FADE_FRAMES);
                    let g = self.level as f32 / FADE_FRAMES as f32;
                    [l * g, r * g]
                }
                None => {
                    // The ring ran dry: fade out over what the line holds.
                    self.shared.count_underrun();
                    self.draining = true;
                    self.level -= self.level.min(1);
                    let [l, r] = self.take_oldest();
                    let g = self.level as f32 / FADE_FRAMES as f32;
                    [l * g, r * g]
                }
            }
        } else {
            if let Some(f) = input {
                self.put(f);
            }
            [0.0, 0.0]
        };
        if self.clearing && !self.draining {
            self.finish_clear();
        }
        let gain = self.shared.gain();
        [out[0] * gain, out[1] * gain]
    }

    /// 48 kHz frames between taking a frame off the ring and it leaving the device's buffer
    /// (the line and the resampler), not counting the device's own delay.
    fn own_delay(&self) -> f64 {
        let resampler = self.resampling.as_ref().map_or(0, |r| r.delay);
        (LINE + resampler) as f64 * self.step
    }

    /// Fills interleaved device frames of `channels` channels, and notes how long until they
    /// are heard (`latency`: from now until the device plays the first of them).
    pub fn fill_timed<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        data: &mut [T],
        channels: usize,
        latency: Option<Duration>,
    ) {
        self.fill(data, channels);
        let frames = data.len() / channels.max(1);
        let device = latency.map_or(0.0, |d| d.as_secs_f64() * RATE as f64);
        // A frame is read on average half a buffer before it is played out.
        self.shared.note_delay((device + frames as f64 * self.step * 0.5 + self.own_delay()) as f32);
    }

    /// Fills interleaved device frames of `channels` channels.
    pub fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(&mut self, data: &mut [T], channels: usize) {
        self.poll_clear();
        let channels = channels.max(1);
        for frame in data.chunks_mut(channels) {
            let [l, r] = self.next_frame();
            match frame {
                [one] => *one = T::from_sample((l + r) * 0.5),
                [a, b, rest @ ..] => {
                    *a = T::from_sample(l);
                    *b = T::from_sample(r);
                    for x in rest {
                        *x = T::EQUILIBRIUM;
                    }
                }
                [] => {}
            }
        }
    }
}

/// The open output device.
#[allow(dead_code)]
pub struct AudioOut {
    _stream: cpal::Stream,
    shared: Arc<Shared>,
    ring: Option<Producer<f32>>,
    rate: u32,
}

// The volume, mute and rate calls are the device's own; the editor goes through `Shared`.
#[allow(dead_code)]
impl AudioOut {
    /// Opens the default output device. `None` when there is none or it won't start; never
    /// panics.
    pub fn open() -> Option<AudioOut> {
        std::panic::catch_unwind(Self::try_open).ok().flatten()
    }

    fn try_open() -> Option<AudioOut> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let config = supported.config();
        let (producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        let feeder = Feeder::new(consumer, shared.clone(), config.sample_rate);
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, feeder, shared.clone()),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, feeder, shared.clone()),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, feeder, shared.clone()),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, feeder, shared.clone()),
            _ => None,
        }?;
        stream.play().ok()?;
        Some(AudioOut { _stream: stream, shared, ring: Some(producer), rate: config.sample_rate })
    }

    /// The ring's writing end, for the sound thread (once).
    pub fn ring_writer(&mut self) -> Option<Producer<f32>> {
        self.ring.take()
    }

    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    /// 48 kHz frames played since the ring was last cleared.
    pub fn played_samples(&self) -> u64 {
        self.shared.played()
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared.set_volume(volume);
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.set_muted(muted);
    }

    pub fn device_rate(&self) -> u32 {
        self.rate
    }
}

fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut feeder: Feeder,
    shared: Arc<Shared>,
) -> Option<cpal::Stream> {
    let channels = config.channels as usize;
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let at = info.timestamp();
                feeder.fill_timed(data, channels, at.playback.checked_duration_since(at.callback))
            },
            move |error: cpal::Error| {
                if error.kind() == cpal::ErrorKind::Xrun {
                    shared.count_underrun();
                }
                if stops_the_speakers(error.kind()) {
                    shared.failed.store(true, Ordering::Relaxed);
                }
            },
            None,
        )
        .ok()
}

/// Whether a stream error means the speakers have stopped. A glitch (an underrun), a move to
/// another output that the stream follows, or no real-time priority leave them playing.
fn stops_the_speakers(kind: cpal::ErrorKind) -> bool {
    !matches!(kind, cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied)
}

/// Counts this thread's allocations while a check runs (tests only).
#[cfg(test)]
pub(crate) mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub struct Counting;

    fn count() {
        if ON.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count();
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count();
            unsafe { System.realloc(ptr, layout, size) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    /// How many times `f` allocated on this thread.
    pub fn allocations(f: impl FnOnce()) -> usize {
        COUNT.with(|c| c.set(0));
        ON.with(|o| o.set(true));
        f();
        ON.with(|o| o.set(false));
        COUNT.with(Cell::get)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_lost_stream_stops_the_speakers() {
        use cpal::ErrorKind::*;
        for glitch in [Xrun, DeviceChanged, RealtimeDenied] {
            assert!(!stops_the_speakers(glitch), "{glitch:?} leaves the stream playing");
        }
        for gone in [DeviceNotAvailable, StreamInvalidated, BackendError, HostUnavailable, Other] {
            assert!(stops_the_speakers(gone), "{gone:?} stops it");
        }
    }

    /// How strongly `signal` (at `rate`) holds frequency `f`.
    fn strength(signal: &[f32], rate: f64, f: f64) -> f64 {
        let w = std::f64::consts::TAU * f / rate;
        let (mut re, mut im) = (0.0, 0.0);
        for (n, &x) in signal.iter().enumerate() {
            re += x as f64 * (w * n as f64).cos();
            im -= x as f64 * (w * n as f64).sin();
        }
        (re * re + im * im).sqrt()
    }

    #[test]
    fn resampler_keeps_pitch() {
        let (mut producer, consumer) = RingBuffer::new(RATE as usize * 2 * 2);
        for n in 0..RATE as usize * 2 - 100 {
            let v = (std::f64::consts::TAU * 1_000.0 * n as f64 / RATE as f64).sin() as f32 * 0.5;
            producer.push(v).unwrap();
            producer.push(v).unwrap();
        }
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), 44_100);
        let mut out = vec![0.0f32; 44_100 * 2];
        feeder.fill(&mut out, 2);
        // Leave out the resampler's start.
        let left: Vec<f32> = out.chunks(2).skip(4_100).take(40_000).map(|f| f[0]).collect();
        let peak = (0..=80)
            .map(|k| 980.0 + k as f64 * 0.5)
            .max_by(|a, b| strength(&left, 44_100.0, *a).total_cmp(&strength(&left, 44_100.0, *b)));
        let peak = peak.unwrap();
        assert!((peak - 1_000.0).abs() <= 2.0, "the tone sits at {peak} Hz");
        let played = shared.played();
        assert!((played as i64 - 48_000).abs() < 300, "a second of 48 kHz frames played: {played}");
    }

    #[test]
    fn an_empty_ring_plays_silence_and_stops_the_clock() {
        let (_producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        let mut out = vec![1.0f32; 256];
        feeder.fill(&mut out, 2);
        assert!(out.iter().all(|&v| v == 0.0));
        assert_eq!(shared.played(), 0);
    }

    /// `n` frames of `v` into the ring.
    fn push(producer: &mut Producer<f32>, n: usize, v: f32) {
        for _ in 0..n * 2 {
            producer.push(v).unwrap();
        }
    }

    fn frames(feeder: &mut Feeder, n: usize) -> Vec<f32> {
        (0..n)
            .map(|_| {
                feeder.poll_clear();
                feeder.next_frame()[0]
            })
            .collect()
    }

    #[test]
    fn frames_are_heard_2_ms_late_and_fade_in() {
        let (mut producer, consumer) = ring();
        push(&mut producer, 400, 0.25);
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        let out = frames(&mut feeder, 300);
        assert_eq!(shared.played(), 300, "every frame taken is counted");
        assert!(out[..LINE].iter().all(|&v| v == 0.0), "the line fills first");
        let ramp = &out[LINE..LINE + FADE_FRAMES as usize];
        assert!(ramp.windows(2).all(|w| w[1] > w[0]) && ramp[0] > 0.0, "then it fades in");
        assert_eq!(out[LINE + FADE_FRAMES as usize], 0.25, "fully in after 2 ms");
    }

    #[test]
    fn a_dry_ring_fades_out_then_back_in_and_counts_while_playing() {
        let (mut producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        shared.set_playing(true);
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        push(&mut producer, 400, 0.5);
        let before = frames(&mut feeder, 400);
        assert_eq!(*before.last().unwrap(), 0.5);
        assert_eq!(shared.underruns(), 0);
        // The ring runs dry: the 2 ms the line holds fade out, no step.
        let out = frames(&mut feeder, 200);
        let ramp = &out[..LINE];
        assert!(ramp.windows(2).all(|w| w[1] < w[0] && w[0] - w[1] < 0.5 / FADE_FRAMES as f32 + 1e-6), "{ramp:?}");
        assert!(ramp[0] < 0.5 && ramp[0] > 0.49 && ramp[LINE - 1] < 0.01);
        assert!(out[LINE..].iter().all(|&v| v == 0.0), "then silence");
        assert_eq!(shared.underruns(), 1, "one dropout");
        assert_eq!(shared.played(), 400, "the clock stops");
        // Frames come back: the line fills, then they fade back in.
        push(&mut producer, 400, 0.5);
        let back = frames(&mut feeder, 300);
        assert!(back[..LINE].iter().all(|&v| v == 0.0));
        assert!(back[LINE..LINE + FADE_FRAMES as usize].windows(2).all(|w| w[1] > w[0]));
        assert_eq!(back[299], 0.5);
        // Stopped, a dry ring is no dropout.
        shared.set_playing(false);
        frames(&mut feeder, 400);
        assert_eq!(shared.underruns(), 1);
    }

    #[test]
    fn a_clear_fades_the_old_sound_out_first() {
        let (mut producer, consumer) = ring();
        push(&mut producer, 1000, 0.5);
        let shared = Arc::new(Shared::default());
        shared.set_playing(true);
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        assert_eq!(frames(&mut feeder, 300)[299], 0.5);
        shared.ask_clear(1);
        let out = frames(&mut feeder, FADE_FRAMES as usize);
        assert!(out.windows(2).all(|w| w[1] < w[0]), "a smooth ramp down: {out:?}");
        assert!(out[0] <= 0.5 && *out.last().unwrap() < 0.01);
        assert_eq!(feeder.next_frame(), [0.0, 0.0]);
        assert_eq!(shared.cleared(), 1);
        assert_eq!(feeder.buffered_frames(), 0, "the rest is dropped");
        assert_eq!(shared.underruns(), 0, "a clear is no dropout");
        // A clear while the line is still filling is at once.
        push(&mut producer, 10, 0.5);
        frames(&mut feeder, 10);
        shared.ask_clear(2);
        frames(&mut feeder, 1);
        assert_eq!(shared.cleared(), 2);
    }

    #[test]
    fn the_delay_to_the_ears_counts_the_device_the_line_and_the_speakers() {
        let (_producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        let mut out = vec![0.0f32; 512 * 2];
        feeder.fill_timed(&mut out, 2, Some(Duration::from_millis(10)));
        // 10 ms (480 frames), half the 512-frame buffer and the 96-frame line.
        assert_eq!(shared.heard_delay(), 480 + 256 + 96);
        shared.set_speaker_delay_ms(100.0);
        assert_eq!(shared.heard_delay(), 480 + 256 + 96 + 4_800);
    }

    #[test]
    fn the_callback_never_allocates() {
        for rate in [RATE, 44_100] {
            let (mut producer, consumer) = ring();
            for n in 0..20_000 {
                producer.push((n as f32 * 0.01).sin()).unwrap();
            }
            let shared = Arc::new(Shared::default());
            let mut feeder = Feeder::new(consumer, shared.clone(), rate);
            let mut out = vec![0.0f32; 512 * 2];
            let mut out16 = vec![0i16; 512 * 2];
            let before = counting::allocations(|| {
                for k in 0..20 {
                    if k == 7 {
                        shared.ask_clear(1);
                    }
                    if k == 12 {
                        for _ in 0..4_000 {
                            let _ = producer.push(0.25);
                        }
                    }
                    feeder.fill(&mut out, 2);
                    feeder.fill(&mut out16, 2);
                }
            });
            assert_eq!(before, 0, "the callback allocated at {rate} Hz");
            assert_eq!(shared.cleared(), 1);
        }
    }

    #[test]
    fn the_volume_and_mute_scale_the_sound_but_not_the_clock() {
        let (mut producer, consumer) = ring();
        for _ in 0..1000 {
            producer.push(0.5).unwrap();
        }
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        for _ in 0..2 * FADE_FRAMES {
            feeder.next_frame();
        }
        shared.set_volume(0.5);
        assert_eq!(feeder.next_frame(), [0.25, 0.25]);
        shared.set_muted(true);
        assert_eq!(feeder.next_frame(), [0.0, 0.0]);
        assert_eq!(shared.played(), 2 * FADE_FRAMES as u64 + 2);
    }
}
