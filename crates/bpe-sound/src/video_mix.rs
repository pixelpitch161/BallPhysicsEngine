//! A video's sound (`docs/superpowers/specs/2026-11-24-video-arrangement-design.md`, "Sound"):
//! each clip's scene sound before its limiter ([`Engine::set_bare`]) times its level, pan and
//! fades, plus the song, through the video's one Master and its End fade. Clips on the On top lane
//! (part 4) join the mix the same way, fading over their own Fade in and Fade out.
//!
//! [`VideoMix`] walks the video in blocks and keeps an engine only for the clips heard now. A
//! clip's engine renders its scene from 0 (not a seek), throws its first `offset` away, and is
//! dropped after its last sample.

use std::f64::consts::FRAC_PI_2;
use std::path::Path;

use anyhow::bail;
use bpe_core::video::{Clip, TransitionSound, Video};
use bpe_dsp::db_to_gain;
use bpe_kit::sound::MixerSettings;
use bpe_kit::video::clip_scene;
use bpe_kit::{Kit, resolve_scene};

use crate::engine::end_fade_gain;
use crate::patch::Backing;
use crate::wav::WavOut;
use crate::{BLOCK, Engine, Master, SAMPLE_RATE, SampleBank, has_sound};

/// Samples in the short fade at a cut or a clip's hard end (5 ms), so nothing clicks.
const EDGE: i64 = SAMPLE_RATE as i64 / 200;

/// Blocks in one written chunk: about a second.
const CHUNK_BLOCKS: usize = 750;

/// Whether the video makes any sound: its song is on, or a clip's scene has sound.
pub fn video_has_sound(video: &Video, kit: &Kit) -> bool {
    let song = video.song.as_ref().is_some_and(|s| s.on && !s.audio.bytes.is_empty());
    song || video.all_clips().any(|c| has_sound(&clip_scene(video, c, &kit.registry)))
}

/// `seconds` in samples.
fn samples(seconds: f64) -> i64 {
    (seconds * SAMPLE_RATE as f64).round() as i64
}

/// The whole video's sound, interleaved stereo, in memory (tests and short videos).
pub fn video_samples(video: &Video, kit: &Kit) -> Vec<f32> {
    let mut mix = VideoMix::new(video, kit);
    let total = mix.total();
    let mut out = Vec::with_capacity(total * 2);
    mix.fill(&mut out, total);
    out
}

/// Renders the video's sound (up to `seconds`, or its end if sooner) to `path` as a 32-bit float stereo WAV at 48 kHz, a second at a
/// time. `progress` hears how far it is (0 to 1); returning `false` cancels, which deletes the
/// file and gives an error.
pub fn render_video_sound(
    video: &Video,
    kit: &Kit,
    path: &Path,
    seconds: f64,
    mut progress: impl FnMut(f64) -> bool,
) -> anyhow::Result<()> {
    let mut mix = VideoMix::new(video, kit);
    mix.set_end(seconds);
    let total = mix.total();
    let mut out = WavOut::create(path)?;
    let mut chunk = Vec::with_capacity(CHUNK_BLOCKS * BLOCK * 2);
    let mut written = 0;
    while written < total {
        chunk.clear();
        let frames = (CHUNK_BLOCKS * BLOCK).min(total - written);
        mix.fill(&mut chunk, frames);
        if let Err(e) = out.write(&chunk) {
            drop(out);
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
        written += frames;
        if !progress(written as f64 / total.max(1) as f64) {
            drop(out);
            let _ = std::fs::remove_file(path);
            bail!("sound render cancelled");
        }
    }
    let result = out.finish();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// A clip as the mix hears it, in video samples.
struct ClipPlan {
    /// The clip (on either lane).
    clip: Clip,
    /// First and one-past-last video sample.
    start: i64,
    end: i64,
    /// Samples into the scene it starts at.
    offset: i64,
    gain: f32,
    /// The pan's left and right gains (both exactly 1 at the middle).
    pan: [f32; 2],
    /// The equal-power fade in over this many samples (0: none), or the short edge fade.
    fade_in: Fade,
    fade_out: Fade,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fade {
    None,
    /// Equal power over this many samples.
    Power(i64),
    /// The 5 ms edge.
    Edge,
}

impl ClipPlan {
    fn new(c: &Clip, fade_in: Fade, fade_out: Fade) -> ClipPlan {
        let pan = c.pan.clamp(-1.0, 1.0);
        ClipPlan {
            clip: c.clone(),
            start: samples(c.start),
            end: samples(c.end()),
            offset: samples(c.offset),
            gain: db_to_gain(c.level_db),
            pan: [(1.0 - pan).min(1.0), (1.0 + pan).min(1.0)],
            fade_in,
            fade_out,
        }
    }
}

/// A clip being heard: its engine and the block it is reading.
struct Playing {
    plan: usize,
    engine: Option<Engine>,
    block: [f32; BLOCK * 2],
    /// The next frame to read in `block` (`BLOCK` when used up).
    at: usize,
}

impl Playing {
    fn next(&mut self) -> [f32; 2] {
        let Some(engine) = &mut self.engine else { return [0.0; 2] };
        if self.at == BLOCK {
            engine.render(&mut self.block);
            self.at = 0;
        }
        let i = self.at;
        self.at += 1;
        [self.block[2 * i], self.block[2 * i + 1]]
    }
}

/// The streaming mixer behind [`video_samples`] and [`render_video_sound`].
pub struct VideoMix<'a> {
    video: &'a Video,
    kit: &'a Kit,
    plans: Vec<ClipPlan>,
    /// The next plan not yet started.
    next_plan: usize,
    playing: Vec<Playing>,
    song: Option<Backing>,
    master: Master,
    /// Master samples still to drop (its delay).
    discard: usize,
    /// Video samples mixed so far (before the master).
    pos: i64,
    /// Output samples handed out so far.
    out_pos: i64,
    /// Output waiting to be handed out, interleaved.
    queue: Vec<f32>,
    queue_at: usize,
    end: i64,
    end_fade: f64,
}

impl<'a> VideoMix<'a> {
    pub fn new(video: &'a Video, kit: &'a Kit) -> Self {
        let mut plans: Vec<ClipPlan> = video
            .clips
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let overlap_in = samples(video.overlap(i));
                let fade_in = match overlap_in {
                    0 if c.offset > 0.0 => Fade::Edge,
                    0 => Fade::None,
                    n if c.transition_in.sound == TransitionSound::Crossfade => Fade::Power(n),
                    // Kept whole over the transition, but cut in partway: the edge, so it doesn't click.
                    _ if c.offset > 0.0 => Fade::Edge,
                    _ => Fade::None,
                };
                let overlap_out = samples(video.overlap(i + 1));
                let keep_next =
                    video.clips.get(i + 1).is_some_and(|n| n.transition_in.sound == TransitionSound::KeepBoth);
                let fade_out = if overlap_out > 0 && !keep_next { Fade::Power(overlap_out) } else { Fade::Edge };
                ClipPlan::new(c, fade_in, fade_out)
            })
            .collect();
        // The On top lane's clips fade over their own fades.
        plans.extend(video.on_top.iter().map(|t| {
            let c = &t.clip;
            let fade_in = match samples(t.place.fade_in) {
                n if n > 0 => Fade::Power(n),
                _ if c.offset > 0.0 => Fade::Edge,
                _ => Fade::None,
            };
            let fade_out = match samples(t.place.fade_out) {
                n if n > 0 => Fade::Power(n),
                _ => Fade::Edge,
            };
            ClipPlan::new(c, fade_in, fade_out)
        }));
        // Engines are made as their first sample comes up, so the plans go by start (stable: the
        // main lane's first at the same start, so a video without On top clips is as before).
        plans.sort_by_key(|p| p.start);
        let song = video.song.as_ref().filter(|s| s.on && !s.audio.bytes.is_empty()).and_then(|s| {
            let audio = SampleBank::get(&s.audio.bytes).ok()?;
            Some(Backing { audio, start: samples(s.starts_at), gain: db_to_gain(s.level_db) })
        });
        let m = &video.settings.master;
        let settings = MixerSettings {
            master_input: m.input,
            master_output: m.output,
            master_ceiling: m.ceiling,
            master_release: m.release,
            master_end_fade: m.end_fade,
            ..MixerSettings::default()
        };
        let master = Master::new(&settings);
        let discard = master.latency();
        Self {
            video,
            kit,
            plans,
            next_plan: 0,
            playing: Vec::new(),
            song,
            master,
            discard,
            pos: 0,
            out_pos: 0,
            queue: Vec::new(),
            queue_at: 0,
            end: samples(video.end()),
            end_fade: f64::from(m.end_fade.max(0.0)),
        }
    }

    /// Ends the sound at `seconds` (an export of the first frames only) when that's before the
    /// video's end: the length and the End fade follow. Set it before the first block.
    pub fn set_end(&mut self, seconds: f64) {
        self.end = self.end.min(samples(seconds.max(0.0)));
    }

    /// Frames in the whole video's sound.
    pub fn total(&self) -> usize {
        self.end.max(0) as usize
    }

    /// Appends the next `frames` frames of the video's sound to `out`, interleaved.
    pub fn fill(&mut self, out: &mut Vec<f32>, frames: usize) {
        let mut left = frames;
        while left > 0 {
            if self.queue_at == self.queue.len() {
                self.queue.clear();
                self.queue_at = 0;
                self.mix_block();
                continue;
            }
            let take = ((self.queue.len() - self.queue_at) / 2).min(left);
            let rate = SAMPLE_RATE as f64;
            let (end, fade) = (self.end as f64 / rate, self.end_fade);
            for k in 0..take {
                let g = end_fade_gain((self.out_pos + k as i64) as f64 / rate, end, fade);
                let j = self.queue_at + 2 * k;
                out.push(self.queue[j] * g);
                out.push(self.queue[j + 1] * g);
            }
            self.queue_at += take * 2;
            self.out_pos += take as i64;
            left -= take;
        }
    }

    /// One block of the mix at `pos`, through the master, onto the queue (less the master's delay).
    fn mix_block(&mut self) {
        let (first, last) = (self.pos, self.pos + BLOCK as i64);
        // Clips whose first sample comes up in this block start now.
        while let Some(plan) = self.plans.get(self.next_plan)
            && plan.start < last
        {
            let index = self.next_plan;
            self.next_plan += 1;
            self.playing.push(self.start(index));
        }
        let mut l = [0.0f32; BLOCK];
        let mut r = [0.0f32; BLOCK];
        for p in &mut self.playing {
            let plan = &self.plans[p.plan];
            for i in 0..BLOCK {
                let j = first + i as i64;
                if j < plan.start || j >= plan.end {
                    continue;
                }
                let [a, b] = p.next();
                let g = plan.gain * fade_at(plan, j);
                l[i] += a * g * plan.pan[0];
                r[i] += b * g * plan.pan[1];
            }
        }
        let plans = &self.plans;
        self.playing.retain(|p| plans[p.plan].end > last);
        if let Some(song) = &self.song {
            song.add(first.max(0) as u64, &mut l, &mut r);
        }
        self.master.process(&mut l, &mut r);
        self.pos = last;
        for i in 0..BLOCK {
            if self.discard > 0 {
                self.discard -= 1;
                continue;
            }
            self.queue.push(l[i]);
            self.queue.push(r[i]);
        }
    }

    /// The engine for plan `index`, run through its offset.
    fn start(&self, index: usize) -> Playing {
        let plan = &self.plans[index];
        let clip = &plan.clip;
        let scene = clip_scene(self.video, clip, &self.kit.registry);
        let engine = has_sound(&scene).then(|| {
            let resolved = resolve_scene(&scene, &self.kit.registry);
            let mut engine = Engine::offline(&scene, &resolved, self.kit);
            engine.set_bare(true);
            engine.set_end(clip.offset + clip.length);
            engine
        });
        let mut playing = Playing { plan: index, engine, block: [0.0; BLOCK * 2], at: BLOCK };
        if playing.engine.is_some() {
            // From 0, exactly as the scene plays, then the part before the offset thrown away.
            for _ in 0..plan.offset {
                playing.next();
            }
        }
        playing
    }
}

/// A clip's fade at video sample `j`.
fn fade_at(plan: &ClipPlan, j: i64) -> f32 {
    let into = j - plan.start;
    let left = plan.end - 1 - j;
    let fade_in = match plan.fade_in {
        Fade::None => 1.0,
        Fade::Edge => ((into + 1) as f64 / EDGE as f64).min(1.0),
        Fade::Power(n) if into < n => (into as f64 / n as f64 * FRAC_PI_2).sin(),
        Fade::Power(_) => 1.0,
    };
    let fade_out = match plan.fade_out {
        Fade::None => 1.0,
        Fade::Edge => ((left + 1) as f64 / EDGE as f64).min(1.0),
        Fade::Power(n) if left < n => {
            let p = (n - 1 - left) as f64 / n as f64;
            (p * FRAC_PI_2).cos()
        }
        Fade::Power(_) => 1.0,
    };
    (fade_in * fade_out) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::scene_audio::SceneAudio;
    use bpe_core::video::{Clip, ClipId, KeptId, KeptScene, Song, Transition, TransitionKind};
    use bpe_core::{Scene, SlotId, ThingId, TrackColour, Value};

    const RATE: f64 = SAMPLE_RATE as f64;

    /// A scene whose sound is a steady sine (its backing track) at `hz`, `length` seconds long.
    fn tone_scene(kit: &Kit, hz: f64, length: f64) -> Scene {
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("duration", Value::Number(length));
        scene.audio.insert("tone".into(), SceneAudio::new(crate::audio::sine_wav(hz, length + 1.0, 48_000, 2)));
        let mixer = scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap();
        mixer.params.set("backing", Value::Text("tone".into()));
        scene
    }

    /// A video of `scenes` and `clips` (scene index, start, length, offset), every clip playing
    /// its scene's song, with a Master that does nothing (ceiling 0 dB).
    fn video(scenes: Vec<Scene>, clips: &[(u32, f64, f64, f64)]) -> Video {
        let mut v = Video::new();
        v.settings.master.ceiling = 0.0;
        for (i, scene) in scenes.into_iter().enumerate() {
            v.scenes.insert(KeptId(i as u32), KeptScene { name: format!("{i}"), colour: TrackColour::PINK, scene });
        }
        for (i, &(scene, start, length, offset)) in clips.iter().enumerate() {
            v.clips.push(Clip {
                id: ClipId(i as u32),
                scene: KeptId(scene),
                start,
                length,
                offset,
                scene_song: true,
                follow_video_tempo: false,
                transition_in: Transition { kind: TransitionKind::Crossfade, ..Transition::default() },
                ..Clip::default()
            });
        }
        v
    }

    /// The scene's own sound before its limiter.
    fn bare(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<f32> {
        let resolved = resolve_scene(scene, &kit.registry);
        let mut engine = Engine::offline(scene, &resolved, kit);
        engine.set_bare(true);
        let mut out = Vec::new();
        let mut block = [0.0f32; BLOCK * 2];
        while out.len() < (seconds * RATE) as usize * 2 {
            engine.render(&mut block);
            out.extend_from_slice(&block);
        }
        out
    }

    fn rms(s: &[f32]) -> f64 {
        (s.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>() / s.len().max(1) as f64).sqrt()
    }

    fn window(s: &[f32], from: f64, to: f64) -> &[f32] {
        &s[(from * RATE) as usize * 2..(to * RATE) as usize * 2]
    }

    fn close(a: &[f32], b: &[f32], tol: f32) {
        assert_eq!(a.len(), b.len());
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            assert!((x - y).abs() <= tol, "sample {i}: {x} vs {y}");
        }
    }

    #[test]
    fn a_bare_master_is_input_times_output() {
        let settings = MixerSettings { master_input: 6.0, master_ceiling: -20.0, ..MixerSettings::default() };
        let mut m = Master::new(&settings);
        m.set_bare(true);
        assert_eq!(m.latency(), 0);
        let (mut l, mut r) = ([0.5f32; BLOCK], [0.5f32; BLOCK]);
        for _ in 0..100 {
            (l, r) = ([0.5; BLOCK], [0.5; BLOCK]);
            m.process(&mut l, &mut r);
        }
        let want = 0.5 * db_to_gain(6.0);
        assert!((l[BLOCK - 1] - want).abs() < 1e-3 && (r[0] - want).abs() < 1e-3, "{} vs {want}", l[0]);
    }

    #[test]
    fn one_clip_is_its_scene_without_its_limiter() {
        let kit = crate::kit();
        let scene = tone_scene(&kit, 440.0, 2.0);
        let v = video(vec![scene.clone()], &[(0, 0.0, 1.0, 0.0)]);
        let mix = video_samples(&v, &kit);
        assert_eq!(mix.len(), 48_000 * 2);
        let own = bare(&scene, &kit, 1.0);
        assert!(rms(window(&own, 0.1, 0.9)) > 0.1, "the scene sounds");
        close(window(&mix, 0.0, 0.99), window(&own, 0.0, 0.99), 1e-5);
    }

    #[test]
    fn an_offset_clip_is_the_scene_from_there() {
        let kit = crate::kit();
        let scene = tone_scene(&kit, 440.0, 2.0);
        let v = video(vec![scene.clone()], &[(0, 0.0, 1.0, 0.5)]);
        let mix = video_samples(&v, &kit);
        let own = bare(&scene, &kit, 1.5);
        close(window(&mix, 0.01, 0.99), window(&own, 0.51, 1.49), 1e-5);
    }

    #[test]
    fn a_crossfade_keeps_constant_power() {
        let kit = crate::kit();
        let (a, b) = (tone_scene(&kit, 440.0, 3.0), tone_scene(&kit, 1000.0, 3.0));
        let v = video(vec![a, b], &[(0, 0.0, 2.0, 0.0), (1, 1.5, 2.0, 0.0)]);
        let mix = video_samples(&v, &kit);
        let steady = 20.0 * rms(window(&mix, 0.5, 1.0)).log10();
        let mut t = 1.5;
        while t < 1.99 {
            let db = 20.0 * rms(window(&mix, t, t + 0.01)).log10();
            assert!((db - steady).abs() < 0.5, "at {t:.2} s: {db:.2} dB vs {steady:.2} dB");
            t += 0.01;
        }
    }

    #[test]
    fn keep_both_does_not_fade() {
        let kit = crate::kit();
        let (a, b) = (tone_scene(&kit, 440.0, 3.0), tone_scene(&kit, 1000.0, 3.0));
        let mut v = video(vec![a.clone(), b], &[(0, 0.0, 2.0, 0.0), (1, 1.5, 2.0, 0.0)]);
        v.clips[1].transition_in.sound = TransitionSound::KeepBoth;
        // Low enough that the two together stay clear of the Master's ceiling.
        v.clips.iter_mut().for_each(|c| c.level_db = -6.0);
        let mix = video_samples(&v, &kit);
        let alone = rms(window(&mix, 1.0, 1.5));
        let both = rms(window(&mix, 1.6, 1.95));
        // Two equal uncorrelated tones: √2 the level.
        assert!((both / alone - 2f64.sqrt()).abs() < 0.05, "{both} vs {alone}");
    }

    #[test]
    fn a_gap_is_silent_but_the_song_plays() {
        let kit = crate::kit();
        let scene = tone_scene(&kit, 440.0, 2.0);
        let mut v = video(vec![scene], &[(0, 0.0, 1.0, 0.0), (0, 2.0, 1.0, 0.0)]);
        let mix = video_samples(&v, &kit);
        assert!(window(&mix, 1.0, 2.0).iter().all(|&x| x == 0.0), "a gap with no song is silent");
        v.song = Some(Song {
            name: "song".into(),
            audio: SceneAudio::new(crate::audio::sine_wav(250.0, 4.0, 48_000, 2)),
            ..Song::default()
        });
        let with_song = video_samples(&v, &kit);
        let song = rms(window(&with_song, 1.2, 1.8));
        assert!(song > 0.1, "the song plays in the gap: {song}");
        // Only the song: the same level as the song over a clip-free stretch of itself.
        let alone = video_samples(&Video { clips: Vec::new(), ..v.clone() }, &kit);
        assert!(alone.is_empty(), "no clips, no length");
        assert!((song - std::f64::consts::FRAC_1_SQRT_2 * rms_of_sine(&v)).abs() < 0.02, "{song}");
    }

    /// The song's own RMS × √2 (its peak), read from the decoded file.
    fn rms_of_sine(v: &Video) -> f64 {
        let audio = SampleBank::get(&v.song.as_ref().unwrap().audio.bytes).unwrap();
        let (l, _) = audio.channels();
        l.iter().fold(0.0f32, |m, x| m.max(x.abs())) as f64
    }

    #[test]
    fn a_clip_to_the_scene_end_has_no_scene_end_fade() {
        let kit = crate::kit();
        let mut scene = tone_scene(&kit, 440.0, 2.0);
        scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap().params.set("master_end_fade", Value::Number(1.0));
        let v = video(vec![scene], &[(0, 0.0, 2.0, 0.0)]);
        let mix = video_samples(&v, &kit);
        let early = rms(window(&mix, 0.5, 1.0));
        let late = rms(window(&mix, 1.5, 1.99));
        assert!((late / early - 1.0).abs() < 0.01, "not faded by the scene: {late} vs {early}");
    }

    #[test]
    fn a_shorter_end_cuts_the_sound_and_fades_there() {
        let kit = crate::kit();
        let mut v = video(vec![tone_scene(&kit, 440.0, 3.0)], &[(0, 0.0, 2.0, 0.0)]);
        v.settings.master.end_fade = 0.5;
        let mut mix = VideoMix::new(&v, &kit);
        mix.set_end(1.0);
        assert_eq!(mix.total(), 48_000);
        let mut out = Vec::new();
        mix.fill(&mut out, 48_000);
        let before = rms(window(&out, 0.2, 0.4));
        let fading = rms(window(&out, 0.9, 0.99));
        assert!(fading < before * 0.3, "the End fade lands at the cut end: {fading} vs {before}");
    }

    #[test]
    fn keep_both_cut_in_partway_fades_its_edge() {
        let kit = crate::kit();
        let (a, b) = (tone_scene(&kit, 440.0, 3.0), tone_scene(&kit, 1000.0, 3.0));
        let mut v = video(vec![a, b.clone()], &[(0, 0.0, 2.0, 0.0), (1, 1.5, 2.0, 0.70025)]);
        v.clips[1].transition_in.sound = TransitionSound::KeepBoth;
        let both = video_samples(&v, &kit);
        let only_a = video_samples(&video(vec![tone_scene(&kit, 440.0, 3.0)], &[(0, 0.0, 2.0, 0.0)]), &kit);
        // B's first sample, alone: the mix less A.
        let at = (1.5 * RATE) as usize * 2;
        let first_b = (both[at] - only_a[at]).abs();
        let own = bare(&b, &kit, 1.0);
        let raw = own[(0.70025 * RATE) as usize * 2].abs();
        assert!(raw > 0.05, "the tone is mid-wave there: {raw}");
        assert!(first_b < raw * 0.05, "B starts faded in: {first_b} vs {raw}");
    }

    #[test]
    fn level_moves_the_clip() {
        let kit = crate::kit();
        let scene = tone_scene(&kit, 440.0, 2.0);
        let mut v = video(vec![scene.clone()], &[(0, 0.0, 1.0, 0.0)]);
        let full = video_samples(&v, &kit);
        v.clips[0].level_db = -6.0;
        let quiet = video_samples(&v, &kit);
        let ratio = rms(window(&quiet, 0.1, 0.9)) / rms(window(&full, 0.1, 0.9));
        assert!((ratio - f64::from(db_to_gain(-6.0))).abs() < 1e-2 && (ratio - 0.5).abs() < 1e-2, "{ratio}");
    }

    #[test]
    fn a_silent_video_has_no_sound() {
        let kit = crate::kit();
        let mut scene = tone_scene(&kit, 440.0, 2.0);
        scene.audio.clear();
        let v = video(vec![scene], &[(0, 0.0, 1.0, 0.0)]);
        assert!(!video_has_sound(&v, &kit));
        let mut v2 = v.clone();
        v2.clips[0].scene_song = false;
        assert!(!video_has_sound(&v2, &kit));
        let kit2 = crate::kit();
        let v3 = video(vec![tone_scene(&kit2, 440.0, 2.0)], &[(0, 0.0, 1.0, 0.0)]);
        assert!(video_has_sound(&v3, &kit2));
    }

    /// `v` with an On top clip of kept scene `scene` (start, length, offset, fade in, fade out).
    fn with_top(mut v: Video, scene: u32, at: (f64, f64, f64), fades: (f64, f64), pan: f32) -> Video {
        use bpe_core::video::{OnTopClip, Placement};
        let clip = Clip {
            id: v.next_clip_id(),
            scene: KeptId(scene),
            start: at.0,
            length: at.1,
            offset: at.2,
            pan,
            scene_song: true,
            follow_video_tempo: false,
            ..Clip::default()
        };
        let place = Placement { fade_in: fades.0, fade_out: fades.1, ..Placement::default() };
        v.on_top.push(OnTopClip { clip, row: 0, place });
        v
    }

    #[test]
    fn an_on_top_clip_sounds_like_its_scene() {
        let kit = crate::kit();
        let scene = tone_scene(&kit, 440.0, 2.0);
        let v = with_top(video(vec![scene.clone()], &[]), 0, (0.0, 1.0, 0.5), (0.0, 0.0), 0.0);
        let mix = video_samples(&v, &kit);
        assert_eq!(mix.len(), 48_000 * 2, "the video ends where the On top clip does");
        let own = bare(&scene, &kit, 1.5);
        close(window(&mix, 0.01, 0.99), window(&own, 0.51, 1.49), 1e-5);
    }

    #[test]
    fn a_pan_moves_the_sound_and_the_middle_changes_nothing() {
        let kit = crate::kit();
        let scene = tone_scene(&kit, 440.0, 2.0);
        let plain = video_samples(&video(vec![scene.clone()], &[(0, 0.0, 1.0, 0.0)]), &kit);
        let mut v = video(vec![scene], &[(0, 0.0, 1.0, 0.0)]);
        let middle = video_samples(&v, &kit);
        assert!(plain == middle);
        v.clips[0].pan = 1.0;
        let right = video_samples(&v, &kit);
        for (k, (got, was)) in right.chunks(2).zip(plain.chunks(2)).enumerate() {
            assert_eq!(got[0], 0.0, "frame {k}: the left is silent");
            assert_eq!(got[1], was[1], "frame {k}: the right is as it was");
        }
        v.clips[0].pan = -0.5;
        let left = video_samples(&v, &kit);
        let (l, r) = (left[48_000], left[48_001]);
        assert_eq!(l, plain[48_000]);
        assert!((r - plain[48_001] * 0.5).abs() < 1e-6, "half the right at -0.5");
    }

    #[test]
    fn an_on_top_fade_in_is_equal_power() {
        let kit = crate::kit();
        let scene = tone_scene(&kit, 440.0, 3.0);
        let v = with_top(video(vec![scene], &[]), 0, (0.0, 2.0, 0.0), (1.0, 0.5), 0.0);
        let mix = video_samples(&v, &kit);
        let steady = rms(window(&mix, 1.1, 1.4));
        let middle = rms(window(&mix, 0.49, 0.51));
        assert!((middle / steady - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02, "{}", middle / steady);
        assert!(rms(window(&mix, 0.0, 0.01)) < steady * 0.05, "it starts from silence");
        assert!(rms(window(&mix, 1.99, 2.0)) < steady * 0.05, "and fades out to it");
    }
}
