//! Mixes the scene's song under the bounce sounds, dipping it a little while bounces play.

use crate::{SAMPLE_RATE, normalize_loudness};
use bpe_scene::{Property, Scene};

/// Seconds the song fades out over at the end of the video.
const FADE_OUT: f32 = 0.75;

/// Adds `song` (interleaved stereo at the mix rate) under `bounces`, then levels the whole mix
/// to `target_db`.
pub(crate) fn mix_in(bounces: &mut [f32], song: &[f32], scene: &Scene, target_db: f32) {
    let music = &scene.music;
    let frames = bounces.len() / 2;
    normalize_loudness(bounces, target_db);
    // The part of the song the video plays over. A negative offset starts the song late.
    let offset = (music.offset as f64 * SAMPLE_RATE as f64).round() as i64;
    let mut layer = vec![0.0f32; bounces.len()];
    for (i, frame) in layer.chunks_exact_mut(2).enumerate() {
        let source = i as i64 + offset;
        if source >= 0 && ((source as usize) * 2 + 1) < song.len() {
            frame[0] = song[source as usize * 2];
            frame[1] = song[source as usize * 2 + 1];
        }
    }
    normalize_loudness(&mut layer, target_db + music.gain);
    // Keyframed volume and ducking, worked out a few hundred times a second.
    let animated = |p: &Property| scene.track(p).is_some_and(|t| !t.keys.is_empty());
    let (volume_keyed, duck_keyed) = (animated(&Property::SongVolume), animated(&Property::SongDucking));
    let mut volume = 1.0f32;
    let mut amount = music.duck_amount.clamp(0.0, 1.0);
    const CONTROL_FRAMES: usize = 64;

    // Duck: follow the bounce sounds' level and lower the song while they're loud.
    let reference = 10f32.powf(target_db / 20.0) * 1.4;
    let attack = 1.0 - (-1.0 / (0.005 * SAMPLE_RATE as f32)).exp();
    let release = 1.0 - (-1.0 / (0.25 * SAMPLE_RATE as f32)).exp();
    let fade_frames = ((FADE_OUT * SAMPLE_RATE as f32) as usize).min(frames).max(1);
    let mut level = 0.0f32;
    for (i, (out, song)) in bounces.chunks_exact_mut(2).zip(layer.chunks_exact(2)).enumerate() {
        if (volume_keyed || duck_keyed) && i % CONTROL_FRAMES == 0 {
            let time = i as f32 / SAMPLE_RATE as f32;
            if let Some(gain) = scene.number_at(&Property::SongVolume, time).filter(|_| volume_keyed) {
                volume = 10f32.powf((gain - music.gain) / 20.0);
            }
            if let Some(duck) = scene.number_at(&Property::SongDucking, time).filter(|_| duck_keyed) {
                amount = duck.clamp(0.0, 1.0);
            }
        }
        let now = (out[0].abs() + out[1].abs()) * 0.5;
        level += (now - level) * if now > level { attack } else { release };
        let duck = 1.0 - amount * 0.75 * (level / reference).min(1.0);
        let fade = ((frames - i) as f32 / fade_frames as f32).min(1.0);
        out[0] += song[0] * duck * fade * volume;
        out[1] += song[1] * duck * fade * volume;
    }
    normalize_loudness(bounces, target_db);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MixOptions, mix};
    use bpe_scene::{Ease, Key, Music, Track, Value};

    /// A steady 220 Hz tone, as a stand-in song.
    fn tone(seconds: f32) -> Vec<f32> {
        (0..(seconds * SAMPLE_RATE as f32) as usize)
            .flat_map(|i| {
                let s = (std::f32::consts::TAU * 220.0 * i as f32 / SAMPLE_RATE as f32).sin() * 0.3;
                [s, s]
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    #[test]
    fn the_song_plays_under_the_bounces_and_dips_on_hits() {
        let mut scene = Scene::big_ball();
        scene.video.duration = 6.0;
        scene.music.song = Some("song.wav".into());
        scene.music.duck_amount = 1.0;
        let song = tone(10.0);
        let hits = bpe_sim::collect_wall_hits(&scene);
        let with = mix(&scene, &hits, &MixOptions { song: Some(&song), ..MixOptions::default() }).unwrap();
        let without = mix(&scene, &hits, &MixOptions::default()).unwrap();
        // Before the first hit only the song plays.
        let first = (hits[0].time * SAMPLE_RATE as f64) as usize;
        assert!(rms(&with.track.samples[..first * 2]) > 0.01);
        assert!(without.track.samples[..first * 2].iter().all(|&s| s == 0.0));
        // Bounce notes are still there, and the clip ends quietly.
        assert_eq!(with.notes, without.notes);
        let end = with.track.samples.len();
        assert!(rms(&with.track.samples[end - 200..]) < 0.01);
        let peak = with.track.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= 0.9);
    }

    #[test]
    fn offset_and_silent_songs() {
        let music = Music { song: Some("x".into()), offset: -1.0, duck_amount: 0.0, ..Music::default() };
        let mut scene = Scene { music, ..Scene::default() };
        let mut bounces = vec![0.0; SAMPLE_RATE as usize * 2 * 3];
        mix_in(&mut bounces, &tone(5.0), &scene, -14.0);
        // The song starts a second into the video.
        assert!(bounces[..SAMPLE_RATE as usize * 2 - 2].iter().all(|&s| s == 0.0));
        assert!(rms(&bounces[SAMPLE_RATE as usize * 2..]) > 0.05);
        // A silent or empty song changes nothing.
        scene.music.offset = 0.0;
        let mut quiet = vec![0.0; 1000];
        mix_in(&mut quiet, &[], &scene, -14.0);
        mix_in(&mut quiet, &vec![0.0; 4000], &scene, -14.0);
        assert!(quiet.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn keyframed_song_volume_fades_the_song() {
        let mut scene =
            Scene { music: Music { song: Some("x".into()), duck_amount: 0.0, ..Music::default() }, ..Scene::default() };
        let mut plain = vec![0.0; SAMPLE_RATE as usize * 2 * 2];
        let mut faded = plain.clone();
        mix_in(&mut plain, &tone(3.0), &scene, -14.0);
        let gain = scene.music.gain;
        let key = |time, v| Key { time, value: Value::Number(v), ease: Ease::Linear };
        scene
            .animation
            .push(Track { property: Property::SongVolume, keys: vec![key(0.0, gain), key(1.0, gain - 40.0)] });
        mix_in(&mut faded, &tone(3.0), &scene, -14.0);
        let second = SAMPLE_RATE as usize * 2;
        let (early, late) =
            (rms(&faded[..second / 10]), rms(&faded[second + 200..second * 2 - 2 * SAMPLE_RATE as usize / 2]));
        assert!(late < early * 0.3, "{early} {late}");
        assert!(rms(&plain[second + 200..]) > late);
    }
}
