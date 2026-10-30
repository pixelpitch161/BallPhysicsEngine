//! Voices: playing a channel's notes through a program, with a voice limit, stealing, mono and
//! glide, and a cap across the whole mix.

use crate::{BLOCK, Buf, Ctx, HitPhysics, Patch, Program, SILENT, SceneSignals, Sources};
pub use bpe_core::{NoteLength, VoiceSettings};
use std::collections::BTreeSet;
use std::sync::Arc;

/// A stolen voice fades out over this many frames (5 ms at 48 kHz).
pub const STEAL_FADE: usize = 240;
/// Under this a voice counts as silent (-90 dBFS).
const QUIET: f32 = 3.162_277_7e-5;
/// Seconds a voice must stay quiet after its gate closes before it ends.
const QUIET_FOR: f32 = 0.05;
/// Seconds after the gate closes when a voice ends regardless.
const LONGEST_TAIL: f32 = 10.0;

/// One note for a channel to play.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayNote {
    pub frame: usize,
    pub key: u8,
    pub velocity: f32,
    /// Gains for the two sides.
    pub left: f32,
    pub right: f32,
    /// Where the mix-wide voice cap cuts this note off, if it does.
    pub steal_at: Option<usize>,
    /// What the hit that played it gives the voice's input blocks.
    pub physics: Option<Arc<HitPhysics>>,
}

/// A rendered channel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelAudio {
    /// Interleaved stereo, `frames * 2` long.
    pub samples: Vec<f32>,
    /// A voice made a sample that wasn't a number and was stopped.
    pub bad_samples: bool,
}

/// The gate in frames, never shorter than one block, so even a zero-length note plays its
/// attack and release.
fn gate_frames(voices: &VoiceSettings, seconds_per_beat: f32, rate: f32) -> usize {
    voices.note_length.frames(seconds_per_beat, rate).max(BLOCK)
}

fn note_ctx(note: &PlayNote, rate: f32, seconds_per_beat: f32) -> Ctx {
    Ctx {
        rate,
        note: note.key as f32,
        velocity: note.velocity,
        gate: true,
        trigger: true,
        since_on: 0.0,
        time: note.frame as f64 / rate as f64,
        seconds_per_beat,
        seed: (note.frame as u64) << 8 | note.key as u64,
    }
}

/// Runs one block, replacing bad samples. Returns the block, its peak and whether it was bad.
fn run_block(patch: &mut Patch, cx: &Ctx) -> (Buf, f32, bool) {
    #[cfg(test)]
    BLOCKS_RUN.with(|n| n.set(n.get() + 1));
    let mut buf = SILENT;
    patch.process(cx, &mut buf);
    let mut peak = 0.0f32;
    let mut bad = false;
    for side in &mut buf {
        for v in side.iter_mut() {
            if !v.is_finite() {
                *v = 0.0;
                bad = true;
            }
            peak = peak.max(v.abs());
        }
    }
    (buf, peak, bad)
}

#[cfg(test)]
thread_local! {
    /// Blocks run on this thread, to check that voices the limit steals stop rendering.
    static BLOCKS_RUN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A voice's rendered sound, waiting to be added to the channel until no later note can steal it.
struct Pending {
    start: usize,
    /// Interleaved stereo with the note's gains applied.
    samples: Vec<f32>,
}

impl Pending {
    fn end(&self) -> usize {
        self.start + self.samples.len() / 2
    }

    /// Fades the voice out from `at` over [`STEAL_FADE`] frames and drops the rest.
    fn cut(&mut self, at: usize) {
        let from = at.saturating_sub(self.start);
        for k in 0..STEAL_FADE {
            let i = from + k;
            if i * 2 >= self.samples.len() {
                break;
            }
            let g = 1.0 - (k + 1) as f32 / STEAL_FADE as f32;
            self.samples[i * 2] *= g;
            self.samples[i * 2 + 1] *= g;
        }
        self.samples.truncate(((from + STEAL_FADE) * 2).min(self.samples.len()));
    }

    fn add_to(&self, out: &mut [f32]) {
        let begin = (self.start * 2).min(out.len());
        for (o, v) in out[begin..].iter_mut().zip(&self.samples) {
            *o += v;
        }
    }
}

/// A voice being rendered a stretch at a time: far enough to know whether it has ended by the
/// next note, and no further, so a voice the limit steals never renders the tail it won't play.
struct Live {
    patch: Option<Patch>,
    cx: Ctx,
    note: PlayNote,
    gate: usize,
    /// The most frames it can play: to the video's end, its longest tail or its global steal.
    len: usize,
    quiet: usize,
    quiet_needed: usize,
    voice: Pending,
    /// Frames rendered so far.
    done: usize,
    bad: bool,
}

impl Live {
    fn new(
        program: &Program,
        note: &PlayNote,
        gate: usize,
        seconds_per_beat: f32,
        rate: f32,
        frames: usize,
        scene: &Option<Arc<SceneSignals>>,
    ) -> Self {
        let cx = note_ctx(note, rate, seconds_per_beat);
        let longest = gate + (LONGEST_TAIL * rate) as usize;
        let mut len = (frames - note.frame).min(longest);
        if let Some(at) = note.steal_at {
            len = len.min(at.saturating_sub(note.frame) + STEAL_FADE);
        }
        let quiet_needed = (QUIET_FOR * rate) as usize;
        let samples = Vec::with_capacity(len.min(gate + quiet_needed + BLOCK) * 2);
        Live {
            patch: Some(program.instantiate_with(&cx, &Sources { hit: note.physics.clone(), scene: scene.clone() })),
            cx,
            note: note.clone(),
            gate,
            len,
            quiet: 0,
            quiet_needed,
            voice: Pending { start: note.frame, samples },
            done: 0,
            bad: false,
        }
    }

    fn finished(&self) -> bool {
        self.patch.is_none()
    }

    /// Renders on until the voice has played up to the channel's frame `to`, or has ended.
    fn advance_to(&mut self, to: usize) {
        let want = to.saturating_sub(self.note.frame);
        while let Some(patch) = &mut self.patch {
            if self.done >= self.len {
                self.end();
                break;
            }
            if self.done >= want {
                break;
            }
            self.cx.gate = self.done < self.gate;
            let (buf, peak, was_bad) = run_block(patch, &self.cx);
            let take = BLOCK.min(self.len - self.done);
            for s in 0..take {
                self.voice.samples.push(buf[0][s] * self.note.left);
                self.voice.samples.push(buf[1][s] * self.note.right);
            }
            self.done += take;
            if was_bad {
                self.bad = true;
                self.end();
                break;
            }
            if !self.cx.gate {
                self.quiet = if peak < QUIET { self.quiet + BLOCK } else { 0 };
                if self.quiet >= self.quiet_needed {
                    self.end();
                    break;
                }
            }
            self.cx.advance();
        }
    }

    /// Stops rendering, and cuts the voice where the mix-wide cap steals it.
    fn end(&mut self) {
        if self.patch.take().is_some()
            && let Some(at) = self.note.steal_at
        {
            self.voice.cut(at);
        }
    }
}

/// Advances every voice to the channel's frame `to`, side by side on up to `threads` threads.
fn advance_all(live: &mut [Live], to: usize, threads: usize) {
    let busy = live.iter().filter(|v| !v.finished()).count();
    if threads <= 1 || busy < 2 {
        for v in live.iter_mut() {
            v.advance_to(to);
        }
        return;
    }
    let per = live.len().div_ceil(threads.min(busy));
    std::thread::scope(|scope| {
        for chunk in live.chunks_mut(per) {
            scope.spawn(move || {
                for v in chunk {
                    v.advance_to(to);
                }
            });
        }
    });
}

/// Plays `notes` through `program` for `frames` frames. Notes are played in frame order.
pub fn render_channel(
    program: &Program,
    notes: &[PlayNote],
    voices: &VoiceSettings,
    seconds_per_beat: f32,
    rate: f32,
    frames: usize,
) -> ChannelAudio {
    render_channel_on(program, notes, voices, seconds_per_beat, rate, frames, 1)
}

/// [`render_channel`] with up to `threads` voices rendering at once. A voice's sound doesn't
/// depend on the others, so the sounding voices render side by side between notes: the result
/// is the same as on one thread.
pub fn render_channel_on(
    program: &Program,
    notes: &[PlayNote],
    voices: &VoiceSettings,
    seconds_per_beat: f32,
    rate: f32,
    frames: usize,
    threads: usize,
) -> ChannelAudio {
    render_channel_with(program, notes, voices, seconds_per_beat, rate, frames, threads, None)
}

/// [`render_channel_on`], with input blocks reading `scene` and each note's own physics.
#[allow(clippy::too_many_arguments)]
pub fn render_channel_with(
    program: &Program,
    notes: &[PlayNote],
    voices: &VoiceSettings,
    seconds_per_beat: f32,
    rate: f32,
    frames: usize,
    threads: usize,
    scene: Option<Arc<SceneSignals>>,
) -> ChannelAudio {
    let mut order: Vec<&PlayNote> = notes.iter().filter(|n| n.frame < frames).collect();
    order.sort_by_key(|n| n.frame);
    let gate = gate_frames(voices, seconds_per_beat, rate);
    let mut out = ChannelAudio { samples: vec![0.0; frames * 2], bad_samples: false };
    if voices.mono {
        render_mono(program, &order, voices, gate, seconds_per_beat, rate, &scene, &mut out);
        return out;
    }
    let limit = voices.limit.max(1) as usize;
    let mut live: Vec<Live> = Vec::new();
    for note in order {
        // The sounding voices render up to this note, so it's known which have ended.
        advance_all(&mut live, note.frame, threads);
        live.retain_mut(|v| {
            let ended = v.finished() && v.voice.end() <= note.frame;
            if ended {
                out.bad_samples |= v.bad;
                v.voice.add_to(&mut out.samples);
            }
            !ended
        });
        while live.len() >= limit {
            // Voices are live in start order, so the first is the oldest.
            let mut oldest = live.remove(0);
            oldest.advance_to(note.frame + STEAL_FADE);
            oldest.end();
            oldest.voice.cut(note.frame);
            out.bad_samples |= oldest.bad;
            oldest.voice.add_to(&mut out.samples);
        }
        live.push(Live::new(program, note, gate, seconds_per_beat, rate, frames, &scene));
    }
    advance_all(&mut live, usize::MAX, threads);
    for v in &live {
        out.bad_samples |= v.bad;
        v.voice.add_to(&mut out.samples);
    }
    out
}

/// One voice for the whole channel. Notes take effect at the start of the block they fall in.
#[allow(clippy::too_many_arguments)]
fn render_mono(
    program: &Program,
    notes: &[&PlayNote],
    voices: &VoiceSettings,
    gate: usize,
    seconds_per_beat: f32,
    rate: f32,
    scene: &Option<Arc<SceneSignals>>,
    out: &mut ChannelAudio,
) {
    let frames = out.samples.len() / 2;
    let quiet_needed = (QUIET_FOR * rate) as usize;
    let longest = (LONGEST_TAIL * rate) as usize;
    let mut next = 0;
    let mut voice: Option<(Patch, Ctx)> = None;
    let (mut gate_end, mut quiet) = (0usize, 0usize);
    let (mut gains, mut steal_at) = ((1.0, 1.0), None::<usize>);
    let (mut from, mut target, mut glided) = (0.0f32, 0.0f32, f32::INFINITY);
    let mut block_start = notes.first().map_or(frames, |n| n.frame / BLOCK * BLOCK);
    while block_start < frames {
        let block_end = block_start + BLOCK;
        let mut started = None;
        while next < notes.len() && notes[next].frame < block_end {
            started = Some(notes[next]);
            next += 1;
        }
        if let Some(note) = started {
            let sources = Sources { hit: note.physics.clone(), scene: scene.clone() };
            match &mut voice {
                Some((patch, cx)) => {
                    patch.bind(&sources);
                    from = cx.note;
                    glided = 0.0;
                    cx.trigger = voices.retrigger || !cx.gate;
                    cx.velocity = note.velocity;
                }
                None => {
                    let cx = note_ctx(note, rate, seconds_per_beat);
                    voice = Some((program.instantiate_with(&cx, &sources), cx));
                    from = note.key as f32;
                    glided = f32::INFINITY;
                }
            }
            target = note.key as f32;
            gate_end = note.frame + gate;
            gains = (note.left, note.right);
            steal_at = note.steal_at;
            quiet = 0;
        }
        let Some((patch, cx)) = &mut voice else {
            // Nothing sounding: skip to the next note.
            block_start = notes.get(next).map_or(frames, |n| n.frame / BLOCK * BLOCK);
            continue;
        };
        cx.gate = block_start < gate_end;
        cx.note = if voices.glide > 0.0 && glided < voices.glide {
            from + (target - from) * (glided / voices.glide)
        } else {
            target
        };
        glided += BLOCK as f32 / rate;
        let (buf, peak, bad) = run_block(patch, cx);
        for s in 0..BLOCK.min(frames - block_start) {
            let f = block_start + s;
            let fade = match steal_at {
                Some(at) if f >= at => 1.0 - ((f - at) as f32 / STEAL_FADE as f32).min(1.0),
                _ => 1.0,
            };
            out.samples[f * 2] += buf[0][s] * gains.0 * fade;
            out.samples[f * 2 + 1] += buf[1][s] * gains.1 * fade;
        }
        let mut end = bad || steal_at.is_some_and(|at| block_end >= at + STEAL_FADE);
        if bad {
            out.bad_samples = true;
        }
        if !cx.gate {
            quiet = if peak < QUIET { quiet + BLOCK } else { 0 };
            end |= quiet >= quiet_needed || block_end >= gate_end + longest;
        }
        cx.advance();
        if end {
            voice = None;
        }
        block_start = block_end;
    }
}

/// Picks which notes the mix-wide cap steals: whenever more than `cap` notes would sound at
/// once, the oldest is cut off where the new one starts. A note counts as sounding from its
/// frame through its gate (`gate_frames(channel)`) plus one second. `notes` are (channel, note)
/// pairs; the result is the same for the same input every time.
pub fn global_steals(notes: &mut [(usize, PlayNote)], gate_frames: impl Fn(usize) -> usize, cap: usize) {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&i| (notes[i].1.frame, notes[i].0, i));
    // (start, order index) of the sounding notes, and (end, order index) to retire them.
    let mut sounding = BTreeSet::<(usize, usize)>::new();
    let mut ends = BTreeSet::<(usize, usize)>::new();
    let mut start_of = vec![0usize; notes.len()];
    for (rank, &i) in order.iter().enumerate() {
        let (channel, frame) = (notes[i].0, notes[i].1.frame);
        while let Some(&(end, r)) = ends.first() {
            if end > frame {
                break;
            }
            ends.pop_first();
            sounding.remove(&(start_of[r], r));
        }
        if sounding.len() >= cap.max(1) {
            let (start, r) = sounding.pop_first().expect("more than none");
            let victim = order[r];
            notes[victim].1.steal_at = Some(frame);
            let end = start + gate_frames(notes[victim].0) + 48_000;
            ends.remove(&(end, r));
        }
        start_of[rank] = frame;
        sounding.insert((frame, rank));
        ends.insert((frame + gate_frames(channel) + 48_000, rank));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{catalog, compile};
    use bpe_graph::{Catalog, Graph, Link, Param};

    /// Note -> sine osc -> amp <- envelope -> voice out, with the given envelope knobs.
    fn sine_patch(attack: f32, release: f32) -> Graph {
        let cat = catalog();
        let mut g = Graph::new("test");
        let osc = g.add(cat.spec("osc").unwrap(), [0.0; 2]);
        g.node_mut(osc).unwrap().params.insert("wave".into(), Param::Choice("sine".into()));
        let env = g.add(cat.spec("adsr").unwrap(), [0.0; 2]);
        let n = g.node_mut(env).unwrap();
        n.params.insert("attack".into(), Param::Number(attack));
        n.params.insert("release".into(), Param::Number(release));
        n.params.insert("sustain".into(), Param::Number(0.7));
        let amp = g.add(cat.spec("amp").unwrap(), [0.0; 2]);
        let out = g.add(cat.spec("voice_out").unwrap(), [0.0; 2]);
        g.connect(Link { from: (osc, 0), to: (amp, 0) }, cat);
        g.connect(Link { from: (env, 0), to: (amp, 1) }, cat);
        g.connect(Link { from: (amp, 0), to: (out, 0) }, cat);
        g
    }

    /// Note pitch straight into voice out: the output is the pitch.
    fn pitch_patch() -> Graph {
        let cat = catalog();
        let mut g = Graph::new("pitch");
        let note = g.add(cat.spec("note").unwrap(), [0.0; 2]);
        let out = g.add(cat.spec("voice_out").unwrap(), [0.0; 2]);
        g.connect(Link { from: (note, 0), to: (out, 0) }, cat);
        g
    }

    fn note(frame: usize, key: u8) -> PlayNote {
        PlayNote { frame, key, velocity: 1.0, left: 1.0, right: 1.0, steal_at: None, physics: None }
    }

    fn seconds(s: f32) -> VoiceSettings {
        VoiceSettings { note_length: NoteLength::Seconds(s), ..VoiceSettings::default() }
    }

    #[test]
    fn a_stolen_voice_stops_rendering() {
        // Ten-second tails, a note every 50 ms and room for two: each voice lives about two
        // notes, so rendering every tail in full would be a hundred times the work.
        let program = compile(&sine_patch(0.001, 10.0)).unwrap();
        let notes: Vec<PlayNote> = (0..200).map(|i| note(i * 2_400, 60 + (i % 12) as u8)).collect();
        let mut two = seconds(0.1);
        two.limit = 2;
        BLOCKS_RUN.with(|n| n.set(0));
        render_channel(&program, &notes, &two, 0.5, 48_000.0, 200 * 2_400 + 48_000);
        let blocks = BLOCKS_RUN.with(|n| n.get());
        assert!(blocks < 30_000, "{blocks} blocks");
    }

    #[test]
    fn a_voice_ends_after_its_release() {
        let program = compile(&sine_patch(0.001, 0.2)).unwrap();
        let out = render_channel(&program, &[note(0, 60)], &seconds(0.1), 0.5, 48_000.0, 48_000);
        assert!(out.samples[..2 * 4_800].iter().any(|v| v.abs() > 0.1));
        assert!(out.samples[2 * 16_800..].iter().all(|v| v.abs() < QUIET), "still sounding after 0.35 s");
        assert_eq!(out.samples.len(), 96_000);
    }

    #[test]
    fn the_oldest_voice_is_stolen_with_a_fade() {
        let program = compile(&sine_patch(0.001, 0.5)).unwrap();
        let notes = [note(0, 36), note(4_800, 40), note(9_600, 43)];
        let two = VoiceSettings { limit: 2, ..seconds(1.0) };
        let out = render_channel(&program, &notes, &two, 0.5, 48_000.0, 24_000);
        let alone = render_channel(&program, &notes[1..], &two, 0.5, 48_000.0, 24_000);
        // After the fade the first note is gone: the mix is just the other two.
        for f in 9_600 + STEAL_FADE..24_000 {
            assert!((out.samples[f * 2] - alone.samples[f * 2]).abs() < 1e-6, "first note still at {f}");
        }
        for f in 1..24_000 {
            let step = (out.samples[f * 2] - out.samples[f * 2 - 2]).abs();
            assert!(step <= 0.05, "a {step} jump at {f}");
        }
    }

    #[test]
    fn mono_glides_between_notes() {
        let program = compile(&pitch_patch()).unwrap();
        let mono = VoiceSettings { mono: true, glide: 0.1, ..seconds(2.0) };
        let out = render_channel(&program, &[note(0, 60), note(9_600, 72)], &mono, 0.5, 48_000.0, 48_000);
        let at = |f: usize| out.samples[f * 2];
        assert_eq!(at(9_000), 60.0);
        let reached = (9_600..48_000).find(|&f| at(f) >= 72.0).unwrap();
        assert!((reached as i64 - 9_600 - 4_800).abs() <= 64, "reached 72 at {reached}");
        assert!(at(12_000) > 60.0 && at(12_000) < 72.0);
    }

    #[test]
    fn mono_retrigger_off_keeps_the_envelope_going() {
        let program = compile(&sine_patch(0.05, 0.2)).unwrap();
        let level = |retrigger: bool| {
            let mono = VoiceSettings { mono: true, glide: 0.0, retrigger, ..seconds(2.0) };
            let notes = [note(0, 60), note(24_000, 60)];
            let out = render_channel(&program, &notes, &mono, 0.5, 48_000.0, 30_000);
            // The loudest point in the 20 ms after the second note's attack ends.
            out.samples[2 * 26_400..2 * 27_360].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        };
        // Held at the 0.7 sustain without retrigger; back up near 1 with it.
        assert!(level(false) < 0.75, "{}", level(false));
        assert!(level(true) > 0.85, "{}", level(true));
    }

    #[test]
    fn thousands_of_notes_are_capped_and_repeatable() {
        let make = || {
            let mut notes: Vec<(usize, PlayNote)> =
                (0..3_000).map(|i| (i % 4, note(i * 32, 40 + (i % 30) as u8))).collect();
            global_steals(&mut notes, |_| 24_000, 256);
            notes
        };
        let notes = make();
        assert_eq!(notes, make());
        let span = |(_, n): &(usize, PlayNote)| (n.frame, n.steal_at.unwrap_or(n.frame + 24_000 + 48_000));
        for f in (0..96_000).step_by(997) {
            let sounding = notes.iter().map(span).filter(|&(a, b)| a <= f && f < b).count();
            assert!(sounding <= 256, "{sounding} at {f}");
        }
        let program = compile(&sine_patch(0.001, 0.05)).unwrap();
        let channel: Vec<PlayNote> = notes.iter().filter(|(c, _)| *c == 0).map(|(_, n)| n.clone()).collect();
        let a = render_channel(&program, &channel, &seconds(0.5), 0.5, 48_000.0, 96_000);
        let b = render_channel(&program, &channel, &seconds(0.5), 0.5, 48_000.0, 96_000);
        assert!(a.samples.iter().zip(&b.samples).all(|(x, y)| x.to_bits() == y.to_bits()));
        // Voices rendered side by side cut and sum to the very same channel.
        let mut limited = seconds(0.5);
        limited.limit = 3;
        let one = render_channel(&program, &channel, &limited, 0.5, 48_000.0, 96_000);
        let four = render_channel_on(&program, &channel, &limited, 0.5, 48_000.0, 96_000, 4);
        assert!(one.samples == four.samples, "the same on four threads");
    }

    #[test]
    fn notes_at_the_end_are_cut_and_zero_length_plays() {
        let program = compile(&sine_patch(0.001, 0.3)).unwrap();
        let out = render_channel(&program, &[note(47_990, 60)], &seconds(1.0), 0.5, 48_000.0, 48_000);
        assert_eq!(out.samples.len(), 96_000);
        let zero = render_channel(&program, &[note(0, 60)], &seconds(0.0), 0.5, 48_000.0, 48_000);
        assert!(zero.samples[..2 * 480].iter().any(|v| v.abs() > 0.05));
    }

    #[test]
    fn non_finite_output_ends_the_voice_and_flags_it() {
        let program = compile(&sine_patch(0.001, 0.1)).unwrap().with_block_replaced("osc", "nan");
        let out = render_channel(&program, &[note(0, 60)], &seconds(0.1), 0.5, 48_000.0, 4_800);
        assert!(out.bad_samples);
        assert!(out.samples.iter().all(|v| v.is_finite()));
    }
}
