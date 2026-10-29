//! Mixing the scene's song under the sounds, dipping it a little while they play.

use bpe_mix::{SAMPLE_RATE, limit_peaks, normalize_loudness};

use super::Song;

/// Seconds the song fades out over at the end of the video.
const FADE_OUT: f32 = 0.75;

/// Adds `song` under `track` (both interleaved stereo at the mix rate), starting `offset`
/// seconds into the song (negative starts it late), `gain_db` quieter or louder than the
/// track's `target_db`, dipping by `ducking` (0 to 1) while the track is loud, then levels and
/// limits the whole mix.
pub fn mix_in(track: &mut [f32], song: &Song, offset: f32, gain_db: f32, ducking: f32, target_db: f32) {
    let frames = track.len() / 2;
    if frames == 0 {
        return;
    }
    normalize_loudness(track, target_db);
    let offset = (offset as f64 * SAMPLE_RATE as f64).round() as i64;
    let mut layer = vec![0.0f32; track.len()];
    for (i, frame) in layer.chunks_exact_mut(2).enumerate() {
        let source = i as i64 + offset;
        if source >= 0 && ((source as usize) * 2 + 1) < song.samples.len() {
            frame[0] = song.samples[source as usize * 2];
            frame[1] = song.samples[source as usize * 2 + 1];
        }
    }
    if layer.iter().all(|&s| s == 0.0) {
        limit_peaks(track);
        return;
    }
    normalize_loudness(&mut layer, target_db + gain_db);
    let amount = ducking.clamp(0.0, 1.0);
    // Duck: follow the track's level and lower the song while it's loud.
    let reference = 10f32.powf(target_db / 20.0) * 1.4;
    let attack = 1.0 - (-1.0 / (0.005 * SAMPLE_RATE as f32)).exp();
    let release = 1.0 - (-1.0 / (0.25 * SAMPLE_RATE as f32)).exp();
    let fade_frames = ((FADE_OUT * SAMPLE_RATE as f32) as usize).clamp(1, frames);
    let mut level = 0.0f32;
    for (i, (out, song)) in track.chunks_exact_mut(2).zip(layer.chunks_exact(2)).enumerate() {
        let now = (out[0].abs() + out[1].abs()) * 0.5;
        level += (now - level) * if now > level { attack } else { release };
        let duck = 1.0 - amount * 0.75 * (level / reference).min(1.0);
        let fade = ((frames - i) as f32 / fade_frames as f32).min(1.0);
        out[0] += song[0] * duck * fade;
        out[1] += song[1] * duck * fade;
    }
    normalize_loudness(track, target_db);
    limit_peaks(track);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_music::AudioFeatures;

    /// A steady 220 Hz tone, as a stand-in song.
    fn tone(seconds: f32) -> Song {
        let samples = (0..(seconds * SAMPLE_RATE as f32) as usize)
            .flat_map(|i| {
                let s = (std::f32::consts::TAU * 220.0 * i as f32 / SAMPLE_RATE as f32).sin() * 0.3;
                [s, s]
            })
            .collect();
        Song { samples, duration: seconds as f64, features: AudioFeatures::default() }
    }

    fn energy(samples: &[f32], from: f32, to: f32) -> f32 {
        let (a, b) = ((from * SAMPLE_RATE as f32) as usize * 2, (to * SAMPLE_RATE as f32) as usize * 2);
        samples[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32
    }

    #[test]
    fn the_song_plays_under_the_notes_and_dips_on_hits() {
        // Four seconds of silence with a loud burst from 2 s to 2.5 s.
        let mut track = vec![0.0f32; 4 * SAMPLE_RATE as usize * 2];
        for i in (2 * SAMPLE_RATE as usize)..(5 * SAMPLE_RATE as usize / 2) {
            let s = (std::f32::consts::TAU * 880.0 * i as f32 / SAMPLE_RATE as f32).sin() * 0.8;
            track[i * 2] = s;
            track[i * 2 + 1] = s;
        }
        let song = tone(10.0);
        let mut no_duck = track.clone();
        mix_in(&mut no_duck, &song, 0.0, 0.0, 0.0, -14.0);
        assert!(energy(&no_duck, 0.5, 1.5) > 1e-4, "the song plays where the track is silent");
        mix_in(&mut track, &song, 0.0, 0.0, 1.0, -14.0);
        // With full ducking the song is quieter during the burst than without ducking, allowing
        // for the burst itself by comparing the moment just after the burst.
        let after_duck = energy(&track, 2.5, 2.6);
        let after_plain = energy(&no_duck, 2.5, 2.6);
        assert!(after_duck < after_plain, "{after_duck} {after_plain}");
        assert!(track.iter().all(|s| s.abs() <= 1.0));
        // The song fades out at the end.
        assert!(energy(&track, 3.98, 4.0) < energy(&track, 1.0, 1.02));
    }

    #[test]
    fn an_offset_starts_the_song_late() {
        let mut track = vec![0.0f32; 3 * SAMPLE_RATE as usize * 2];
        let song = tone(10.0);
        mix_in(&mut track, &song, -1.0, 0.0, 0.0, -14.0);
        assert_eq!(energy(&track, 0.0, 0.9), 0.0, "silent before the song starts");
        assert!(energy(&track, 1.5, 2.0) > 1e-4);
        // A song that ends before the video leaves silence, and never panics.
        let mut long = vec![0.0f32; 3 * SAMPLE_RATE as usize * 2];
        mix_in(&mut long, &tone(1.0), 0.0, 0.0, 0.35, -14.0);
        assert!(energy(&long, 0.2, 0.5) > 1e-4 && energy(&long, 2.0, 3.0) == 0.0);
        // A song entirely out of range leaves the track as it was, levelled.
        let mut out = vec![0.0f32; SAMPLE_RATE as usize * 2];
        mix_in(&mut out, &tone(1.0), 50.0, 0.0, 0.35, -14.0);
        assert!(out.iter().all(|&s| s == 0.0));
    }
}
