//! The Song lane: the song's waveform with its bass glowing underneath, and a tick on every
//! beat (stronger on the first beat of each bar). With no song but a tempo or beat sync, it
//! shows just the beats.

use super::lanes::{Lane, LaneContext, lane_message};
use crate::theme;
use bpe_audio::{PlayedNote, SceneSound};
use bpe_scene::Scene;
use eframe::egui::{self, Pos2, Rect, Stroke, Ui};

/// Waveform columns kept per second of video.
const PEAKS_PER_SECOND: f64 = 100.0;

/// Everything the Song and Notes lanes draw, in video time, worked out with the soundtrack.
#[derive(Clone, Debug, Default)]
pub struct MusicView {
    /// The song's loudest sample in each 1/100 s, 0 to 1. Empty with no song.
    pub peaks: Vec<f32>,
    /// The song's bass level, 0 to 1, and how many values a second.
    pub bass: Vec<f32>,
    pub bass_rate: f32,
    /// Beat times, and the ones that start a bar.
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
    /// Tempo, when the song has one or the scene sets one.
    pub bpm: Option<f32>,
    /// Every note played, by start time.
    pub notes: Vec<PlayedNote>,
}

impl MusicView {
    pub fn new(scene: &Scene, sound: &SceneSound) -> Self {
        let duration = scene.video.duration as f64;
        let to_video = |t: f64| sound.warp.as_ref().map_or(t, |w| w.video_time(t));
        let beats: Vec<f64> = sound.inputs.beats.iter().map(|&b| to_video(b)).filter(|&b| b <= duration).collect();
        let mut view = Self { beats, notes: sound.mix.notes.clone(), ..Self::default() };
        view.bpm = (scene.music.bpm > 0.0).then_some(scene.music.bpm);
        if let Some(song) = &sound.song {
            let features = bpe_audio::in_video_time(&song.features, scene.music.offset);
            view.downbeats = features.downbeats.iter().copied().filter(|&b| b <= duration).collect();
            view.bass_rate = features.rate;
            let frames = (duration * features.rate as f64).ceil() as usize;
            view.bass = features.bass.iter().copied().take(frames).collect();
            view.bpm = view.bpm.or((features.bpm > 0.0).then_some(features.bpm));
            view.peaks = peaks(&song.samples, scene.music.offset as f64, duration);
        } else if !view.beats.is_empty() {
            view.downbeats = view.beats.iter().step_by(4).copied().collect();
            view.bpm = view.bpm.or(Some(120.0));
        }
        view
    }
}

/// The loudest sample in each waveform column of the part of the song the video plays.
fn peaks(stereo: &[f32], offset: f64, duration: f64) -> Vec<f32> {
    let rate = bpe_audio::SAMPLE_RATE as f64;
    let columns = (duration * PEAKS_PER_SECOND).ceil() as usize;
    let per_column = (rate / PEAKS_PER_SECOND) as i64;
    let first = (offset * rate).round() as i64;
    let frames = (stereo.len() / 2) as i64;
    (0..columns as i64)
        .map(|c| {
            let start = (first + c * per_column).clamp(0, frames);
            let end = (first + (c + 1) * per_column).clamp(0, frames);
            let range = start as usize * 2..end as usize * 2;
            stereo[range].iter().fold(0.0f32, |m, s| m.max(s.abs())).min(1.0)
        })
        .collect()
}

#[derive(Default)]
pub struct SongLane {}

impl Lane for SongLane {
    fn id(&self) -> &'static str {
        "song"
    }

    fn title(&self, ctx: &LaneContext) -> String {
        let music = ctx.hits.and_then(|h| h.music.as_deref());
        let name = if ctx.scene.music.song.is_some() { "Song" } else { "Beats" };
        match music.and_then(|m| m.bpm) {
            Some(bpm) => format!("{name}  {bpm:.0} BPM"),
            None => name.into(),
        }
    }

    fn height(&self, _ctx: &LaneContext) -> f32 {
        40.0
    }

    fn visible(&self, ctx: &LaneContext) -> bool {
        let music = &ctx.scene.music;
        music.song.is_some() || music.bpm > 0.0 || music.beat_sync.mode != bpe_scene::SyncMode::Off
    }

    fn draw(&mut self, ui: &mut Ui, rect: Rect, ctx: &LaneContext) {
        let Some(music) = ctx.hits.and_then(|h| h.music.as_deref()) else {
            lane_message(ui, rect, "Reading the song…");
            return;
        };
        let (scale, lane) = (ctx.scale, rect);
        let painter = ui.painter_at(lane);
        painter.rect_filled(lane, 3.0, theme::CARD);
        let middle = lane.center().y;
        let half = lane.height() / 2.0 - 3.0;

        // Bass as a soft glow along the bottom.
        if !music.bass.is_empty() {
            let mut points = vec![Pos2::new(lane.left(), lane.bottom())];
            let mut x = lane.left();
            while x <= lane.right() {
                let i = (scale.time(x) * music.bass_rate) as usize;
                let level = music.bass.get(i).copied().unwrap_or(0.0);
                points.push(Pos2::new(x, lane.bottom() - level * (lane.height() - 4.0)));
                x += 3.0;
            }
            points.push(Pos2::new(x.min(lane.right()), lane.bottom()));
            let glow = theme::ACCENT.gamma_multiply(0.18);
            for pair in points.windows(2) {
                let quad =
                    vec![pair[0], pair[1], Pos2::new(pair[1].x, lane.bottom()), Pos2::new(pair[0].x, lane.bottom())];
                painter.add(egui::Shape::convex_polygon(quad, glow, Stroke::NONE));
            }
        }

        // The waveform, one line per screen point.
        if !music.peaks.is_empty() {
            let stroke = Stroke::new(1.0, theme::MUTED.gamma_multiply(0.8));
            let mut x = lane.left();
            while x < lane.right() {
                let (from, to) = (scale.time(x) as f64, scale.time(x + 1.0) as f64);
                let a = (from * PEAKS_PER_SECOND) as usize;
                let b = ((to * PEAKS_PER_SECOND).ceil() as usize).max(a + 1).min(music.peaks.len());
                if a < b {
                    let peak = music.peaks[a..b].iter().fold(0.0f32, |m, &p| m.max(p));
                    let h = peak.sqrt() * half;
                    painter.line_segment([Pos2::new(x, middle - h), Pos2::new(x, middle + h)], stroke);
                }
                x += 1.0;
            }
        }

        // Beat ticks, thinned out when they'd crowd together.
        let pixels_per_beat =
            music.beats.windows(2).next().map_or(f32::MAX, |w| (w[1] - w[0]) as f32 * scale.width / scale.span);
        if pixels_per_beat >= 4.0 {
            for &beat in &music.beats {
                let x = scale.x(beat as f32);
                painter.line_segment(
                    [Pos2::new(x, lane.bottom() - 7.0), Pos2::new(x, lane.bottom())],
                    Stroke::new(1.0, theme::ACCENT_TEXT.gamma_multiply(0.7)),
                );
            }
        }
        for &bar in &music.downbeats {
            let x = scale.x(bar as f32);
            painter.line_segment(
                [Pos2::new(x, lane.top()), Pos2::new(x, lane.bottom())],
                Stroke::new(1.0, theme::ACCENT_TEXT.gamma_multiply(0.45)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_follow_the_song_from_its_offset() {
        let rate = bpe_audio::SAMPLE_RATE as usize;
        // One second of silence, then one second at half volume.
        let mut stereo = vec![0.0f32; rate * 2];
        stereo.extend(std::iter::repeat_n(0.5f32, rate * 2));
        let from_start = peaks(&stereo, 0.0, 2.0);
        assert_eq!(from_start.len(), 200);
        assert_eq!(from_start[50], 0.0);
        assert_eq!(from_start[150], 0.5);
        // Starting a second in, the loud part comes first, then the song runs out.
        let later = peaks(&stereo, 1.0, 2.0);
        assert_eq!((later[50], later[150]), (0.5, 0.0));
        // A negative offset starts the song late.
        let early = peaks(&stereo, -1.0, 2.0);
        assert_eq!(early[150], 0.0);
    }
}
