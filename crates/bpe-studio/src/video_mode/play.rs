//! Playing the video in Video mode (part 3 plan, decisions 2 to 12): the playhead follows the
//! speakers (or the wall clock), the picture worker draws frames ahead of it, and the sound thread
//! plays a take of the video's mix. Pressing Play waits ("Getting ready…") until the first frame
//! and the start of the sound are ready; a jump of the playhead while playing starts again there.

use super::still::{PlayFeed, Played};
use super::take::Take;
use crate::app::Studio;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// How long the video must rest (no change, no clip drag held) before its sound is rendered
/// again, or the picture restarted while playing.
pub const TAKE_REST: Duration = Duration::from_millis(400);

/// How far past the playhead the take must have rendered before playing starts (seconds).
pub const SOUND_AHEAD: f64 = 0.25;

/// Played frames held by the editor, waiting for the playhead.
pub const QUEUE: usize = 4;

/// The session's playing.
#[derive(Default)]
pub struct Playing {
    /// Playing (or getting ready to).
    pub on: bool,
    /// Getting ready: the playhead holds until the first frame and the sound are ready.
    pub starting: bool,
    /// Play again from the start at the end (the editor's own, not saved).
    pub looping: bool,
    /// The time the playing set the playhead to last; anything else is a jump.
    expected: f64,
    /// The frame playing started from.
    from: u32,
    /// The first frame of this start came back.
    got_first: bool,
    feed: Option<PlayFeed>,
    queue: VecDeque<Played>,
    /// The played frame on show.
    pub shown: Option<u32>,
    /// The video's revision that frame was drawn from.
    shown_revision: u64,
    /// The picture job has sent its last frame (or stopped with a problem).
    feed_done: bool,
    /// The video's revision the picture plays.
    picture_revision: u64,
    /// The take of the video's sound (`None`: none made yet, or no clips).
    pub take: Option<Arc<Take>>,
    /// The revision the take was made for.
    take_revision: Option<u64>,
    /// The revision seen last and when it changed.
    seen: Option<(u64, Instant)>,
    /// The sound thread was told to play (which one: its shared state's address).
    told: Option<usize>,
    /// The sound thread is playing the take.
    sound_on: bool,
    last_tick: Option<Instant>,
}

impl Playing {
    /// What a new video's session keeps from the last one's: Loop, and which sound thread was
    /// told to play a take (so leaving Video mode still gives it the scene back).
    pub fn carry_from(&mut self, old: &Playing) {
        self.looping = old.looping;
        self.told = old.told;
    }

    /// Whether the video has rested since it last changed (`revision`), with no clip drag held.
    fn rested(&mut self, revision: u64, dragging: bool, now: Instant) -> bool {
        match self.seen {
            Some((r, _)) if r == revision => {}
            _ => self.seen = Some((revision, now)),
        }
        !dragging && self.seen.is_some_and(|(_, at)| now.duration_since(at) >= TAKE_REST)
    }
}

/// The address of a sound thread's shared state: which thread it is.
fn job_id(job: &crate::sound_job::SoundJob) -> usize {
    Arc::as_ptr(job.shared()) as usize
}

impl Studio {
    /// Space and the Play button in Video mode.
    pub fn video_toggle_play(&mut self) {
        if self.video.playing.on {
            self.video_stop();
        } else {
            self.video_play();
        }
    }

    /// Plays the video from the playhead (from the start when it is at the end).
    pub fn video_play(&mut self) {
        if !self.video.video.has_clips() {
            self.status = "Add a clip first: drop a scene on the Scenes lane.".into();
            return;
        }
        if self.playing {
            self.set_playing(false);
        }
        // On the last frame (where playing stops) or past it: from the start.
        let v = &self.video.video;
        if v.frames(self.video.time) + 1 >= i64::from(v.frame_count()) {
            self.video.time = 0.0;
        }
        self.video.playing.on = true;
        self.start_video_at(self.video.time);
    }

    /// Starts (again) at `time`: the picture worker from its frame, the sound held until ready.
    fn start_video_at(&mut self, time: f64) {
        let kit = self.kit.clone();
        let session = &mut self.video;
        let mut frame = session.video.frames(time).max(0) as u32;
        if frame >= session.video.frame_count() {
            // At (or past) the end: from the start with Loop, else stopped there.
            if !session.playing.looping {
                session.playing.on = false;
                session.playing.starting = false;
                session.playing.feed = None;
                session.playing.shown = None;
                session.time = session.video.seconds(i64::from(session.video.frame_count().saturating_sub(1)));
                let time = session.time;
                if let Some(job) = &mut self.sound
                    && std::mem::take(&mut self.video.playing.sound_on)
                {
                    job.pause(time);
                }
                return;
            }
            frame = 0;
        }
        session.time = session.video.seconds(i64::from(frame));
        let p = &mut session.playing;
        p.starting = true;
        // A frame from before the jump is no longer the one to stop on.
        p.shown = None;
        p.feed_done = false;
        p.expected = session.time;
        p.from = frame;
        p.got_first = false;
        p.queue.clear();
        p.feed = None;
        p.last_tick = None;
        p.picture_revision = session.revision;
        if session.still.enabled {
            let video = bpe_cli::arranged::tidied(&session.video, &kit);
            let ctx = self.repaint.clone().unwrap_or_default();
            session.playing.feed = session.still.play(&video, frame, &kit, &ctx);
        }
        if session.playing.take_revision != Some(session.revision) {
            self.new_take();
        }
        let time = self.video.time;
        let p = &mut self.video.playing;
        if let Some(job) = &mut self.sound {
            if p.sound_on {
                job.pause(time);
                p.sound_on = false;
            }
            if let Some(take) = &p.take {
                job.set_video(Some(take.clone()));
                job.seek(time);
                p.told = Some(job_id(job));
            }
        }
    }

    /// Makes a take of the video's sound as it is now (a silent one when nothing in it sounds),
    /// and gives it to the sound thread when it is playing a take.
    fn new_take(&mut self) {
        let session = &mut self.video;
        let video = bpe_cli::arranged::tidied(&session.video, &self.kit);
        let take = if bpe_sound::video_has_sound(&video, &self.kit) {
            Take::start(video, self.kit.clone())
        } else {
            Take::silent()
        };
        session.playing.take_revision = Some(session.revision);
        session.playing.take = Some(take.clone());
        if session.playing.on
            && let Some(job) = &mut self.sound
        {
            job.set_video(Some(take));
        }
    }

    /// Stops playing, the playhead on the frame on show.
    pub fn video_stop(&mut self) {
        let session = &mut self.video;
        let p = &mut session.playing;
        if !p.on {
            return;
        }
        p.on = false;
        p.feed = None;
        p.queue.clear();
        if let Some(shown) = p.shown.take() {
            session.time = session.video.seconds(i64::from(shown));
            let revision = p.shown_revision;
            session.still.shown_from_play(revision, shown);
        } else if p.starting {
            // Stopped while getting ready: where it was going.
            session.time = session.video.seconds(i64::from(p.from));
        } else {
            // No picture played: on the playhead's whole frame.
            session.time = session.video.seconds(session.video.frames(session.time));
        }
        p.starting = false;
        let time = session.time;
        if let Some(job) = &mut self.sound
            && std::mem::take(&mut self.video.playing.sound_on)
        {
            job.pause(time);
        }
    }

    /// Video mode's part of the frame's tick: the take, the playhead, the picture.
    pub(crate) fn video_tick(&mut self, now: Instant) {
        let (revision, dragging) = (self.video.revision, self.video.drag.is_some());
        let rested = self.video.playing.rested(revision, dragging, now);
        // While stopped the take is made only when playing starts; playing, once the video rests.
        if rested
            && self.video.playing.on
            && self.video.video.has_clips()
            && self.video.playing.take_revision != Some(revision)
        {
            self.new_take();
        }
        if !self.video.playing.on {
            return;
        }
        // A new sound thread (the speakers came back) is told what to play.
        if let Some(job) = &mut self.sound
            && self.video.playing.told != Some(job_id(job))
            && let Some(take) = &self.video.playing.take
        {
            job.set_video(Some(take.clone()));
            job.seek(self.video.time);
            self.video.playing.told = Some(job_id(job));
            if self.video.playing.sound_on {
                job.play(self.video.time);
            }
        }
        if (self.video.time - self.video.playing.expected).abs() > 1e-9 {
            // The playhead was moved: start again there.
            self.start_video_at(self.video.time);
        } else if rested && self.video.playing.picture_revision != revision && !self.video.playing.starting {
            // The video changed while playing: the picture starts again where it is (the sound
            // swaps to the new take once it has caught up).
            self.restart_picture();
        }
        self.take_frames();
        let session = &mut self.video;
        let p = &mut session.playing;
        if p.starting {
            let picture_ready = p.got_first || p.feed.is_none() || p.feed_done;
            let sound_ready =
                self.sound.is_none() || p.take.as_ref().is_none_or(|t| t.ready_to(session.time + SOUND_AHEAD));
            if picture_ready && sound_ready {
                p.starting = false;
                p.last_tick = Some(now);
                if let Some(job) = &mut self.sound {
                    job.play(session.time);
                    p.sound_on = true;
                }
            }
            return;
        }
        let dt = p.last_tick.replace(now).map_or(0.0, |last| (now - last).as_secs_f64().min(0.25));
        let next = match &self.sound {
            Some(job) if p.sound_on => job.clock(),
            _ => session.time + dt,
        };
        let end = session.video.end();
        if next >= end {
            if p.looping {
                session.time = 0.0;
                self.start_video_at(0.0);
            } else {
                // The playhead stays on the last frame on show (the last frame, with no picture).
                if p.shown.is_none() {
                    session.time = session.video.seconds(i64::from(session.video.frame_count().saturating_sub(1)));
                }
                self.video_stop();
            }
            return;
        }
        session.time = next.max(session.time);
        p.expected = session.time;
        if let Some(feed) = &p.feed {
            feed.clock.store(session.video.frames(session.time).max(0) as u32, Ordering::Relaxed);
        }
    }

    /// The picture plays again from the playhead (the video changed while playing).
    fn restart_picture(&mut self) {
        let kit = self.kit.clone();
        let session = &mut self.video;
        let frame = session.frame();
        session.playing.picture_revision = session.revision;
        session.playing.queue.clear();
        session.playing.feed = None;
        session.playing.feed_done = false;
        if session.still.enabled {
            let video = bpe_cli::arranged::tidied(&session.video, &kit);
            let ctx = self.repaint.clone().unwrap_or_default();
            session.playing.feed = session.still.play(&video, frame, &kit, &ctx);
            if let Some(feed) = &session.playing.feed {
                feed.clock.store(frame, Ordering::Relaxed);
            }
        }
    }

    /// Takes the frames the worker drew and shows the newest one at or before the playhead.
    fn take_frames(&mut self) {
        let session = &mut self.video;
        let p = &mut session.playing;
        let Some(feed) = &p.feed else { return };
        let at = if p.starting { p.from } else { session.video.frames(session.time).max(0) as u32 };
        let mut show: Option<Played> = None;
        loop {
            while p.queue.len() < QUEUE {
                match feed.frames.try_recv() {
                    Ok(played) => p.queue.push_back(played),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        p.feed_done = true;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                }
            }
            let mut took = false;
            while p.queue.front().is_some_and(|(f, ..)| *f <= at) {
                show = p.queue.pop_front();
                took = true;
            }
            if !took {
                break;
            }
        }
        if let Some((frame, pixels, size)) = show {
            if let Some(ctx) = &self.repaint {
                session.still.show_frame(ctx, &pixels, size);
            }
            p.shown = Some(frame);
            p.shown_revision = p.picture_revision;
            if frame >= p.from {
                p.got_first = true;
            }
        }
    }

    /// Leaving Video mode (or the editor stopping it): the video stops and the sound thread plays
    /// the scene again, at the Build playhead.
    pub(crate) fn leave_video_mode(&mut self) {
        self.video_stop();
        // Its take isn't needed outside Video mode (and stops rendering once nobody holds it).
        self.video.playing.take = None;
        self.video.playing.take_revision = None;
        if let Some(job) = &mut self.sound
            && self.video.playing.told.take().is_some()
        {
            job.set_video(None);
            job.seek(self.time);
        }
    }
}
