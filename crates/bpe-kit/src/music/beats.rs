//! The beat grid a scene follows, and how close to it things land.

use bpe_core::Bands;
use bpe_music::AudioFeatures;

/// Tempo used when there is no song and no tempo set.
pub const DEFAULT_BPM: f32 = 120.0;

/// The beat times a scene follows, in video seconds: the song's beats (shifted by `offset`,
/// how far into the song the video starts) when `bpm` is 0 and the song has beats, else an
/// even grid at `bpm` (or [`DEFAULT_BPM`]) from 0 to a beat past `duration`.
pub fn beat_grid(bpm: f32, song: Option<&AudioFeatures>, offset: f32, duration: f64) -> Vec<f64> {
    let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
    if bpm <= 0.0
        && let Some(song) = song.filter(|s| !s.beats.is_empty())
    {
        return song.beats.iter().map(|b| b - offset as f64).filter(|&b| b >= 0.0 && b <= duration + 1.0).collect();
    }
    let bpm = if bpm > 0.0 { bpm } else { DEFAULT_BPM };
    let period = 60.0 / bpm.clamp(20.0, 400.0) as f64;
    (0..).map(|k| k as f64 * period).take_while(|&t| t <= duration + period).collect()
}

/// The beats with `subdivisions` (1 to 4) evenly spaced points per beat.
pub fn subdivided(beats: &[f64], subdivisions: u32) -> Vec<f64> {
    let division = subdivisions.clamp(1, 4) as usize;
    let mut out = Vec::with_capacity(beats.len() * division);
    for pair in beats.windows(2) {
        out.extend((0..division).map(|k| pair[0] + (pair[1] - pair[0]) * k as f64 / division as f64));
    }
    out.extend(beats.last());
    out
}

/// Share of `times` (0 to 1) within `tolerance` seconds of a beat. `beats` must be sorted.
pub fn on_beat_share(times: &[f64], beats: &[f64], tolerance: f64) -> f32 {
    if times.is_empty() {
        return 0.0;
    }
    let on = times.iter().filter(|&&t| distance_to_beat(t, beats) <= tolerance).count();
    on as f32 / times.len() as f32
}

/// Seconds from `time` to the nearest beat.
pub fn distance_to_beat(time: f64, beats: &[f64]) -> f64 {
    let i = beats.partition_point(|&b| b < time);
    let after = beats.get(i).map_or(f64::INFINITY, |b| b - time);
    let before = i.checked_sub(1).map_or(f64::INFINITY, |j| time - beats[j]);
    after.min(before)
}

/// The song's bass, mid, treble and level over video time: its analysis shifted by `offset`,
/// how far into the song the video starts.
pub fn bands_of(features: &AudioFeatures, offset: f32) -> Bands {
    let rate = features.rate.max(1.0);
    let shift = (offset * rate).round() as i64;
    let n = features.level.len();
    let shifted = |v: &[f32]| -> Vec<f32> {
        (0..n)
            .map(|i| {
                let j = i as i64 + shift;
                if j >= 0 && (j as usize) < v.len() { v[j as usize] } else { 0.0 }
            })
            .collect()
    };
    Bands {
        rate,
        bass: shifted(&features.bass),
        mid: shifted(&features.mid),
        treble: shifted(&features.treble),
        level: shifted(&features.level),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tempo_lays_an_even_grid() {
        let grid = beat_grid(120.0, None, 0.0, 20.0);
        assert_eq!(grid.len(), 42, "every half second from 0 to a beat past the end");
        assert!(grid.windows(2).all(|w| (w[1] - w[0] - 0.5).abs() < 1e-9));
        assert_eq!(grid[0], 0.0);
        assert_eq!(beat_grid(0.0, None, 0.0, 1.0).len(), 4, "120 bpm when nothing says otherwise");
        assert!(beat_grid(0.0, None, 0.0, f64::NAN).len() <= 2);
    }

    #[test]
    fn a_song_with_beats_wins_unless_a_tempo_is_set() {
        let song = AudioFeatures { beats: vec![0.5, 1.1, 1.7, 2.3, 30.0], ..AudioFeatures::default() };
        assert_eq!(beat_grid(0.0, Some(&song), 0.0, 10.0), vec![0.5, 1.1, 1.7, 2.3]);
        // Starting a second into the song shifts its beats earlier and drops the one before 0.
        let shifted = beat_grid(0.0, Some(&song), 1.0, 10.0);
        assert!((shifted[0] - 0.1).abs() < 1e-9 && shifted.len() == 3, "{shifted:?}");
        // A tempo overrides the song's beats.
        assert_eq!(beat_grid(60.0, Some(&song), 0.0, 3.0), vec![0.0, 1.0, 2.0, 3.0, 4.0]);
        // A song without beats falls back to the tempo.
        assert_eq!(beat_grid(0.0, Some(&AudioFeatures::default()), 0.0, 1.0).len(), 4);
    }

    #[test]
    fn subdivided_and_on_beat_share() {
        let beats = [0.0, 1.0, 2.0];
        assert_eq!(subdivided(&beats, 2), vec![0.0, 0.5, 1.0, 1.5, 2.0]);
        assert_eq!(subdivided(&beats, 1), beats.to_vec());
        assert_eq!(subdivided(&[], 4), Vec::<f64>::new());
        assert_eq!(on_beat_share(&[0.02, 0.5, 1.95], &beats, 0.06), 2.0 / 3.0);
        assert_eq!(on_beat_share(&[], &beats, 0.06), 0.0);
        assert!((distance_to_beat(1.4, &beats) - 0.4).abs() < 1e-9);
        assert_eq!(distance_to_beat(5.0, &beats), 3.0);
        assert_eq!(distance_to_beat(5.0, &[]), f64::INFINITY);
        let features = AudioFeatures {
            rate: 2.0,
            bass: vec![0.1, 0.2, 0.3, 0.4],
            mid: vec![0.0; 4],
            treble: vec![0.0; 4],
            level: vec![1.0; 4],
            ..AudioFeatures::default()
        };
        let bands = bands_of(&features, 0.5);
        assert_eq!(bands.bass, vec![0.2, 0.3, 0.4, 0.0], "a half-second offset is one frame at 2 Hz");
        assert_eq!(bands_of(&features, -0.5).bass, vec![0.0, 0.1, 0.2, 0.3]);
    }
}
