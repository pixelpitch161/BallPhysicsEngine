//! Beat sync against real songs: click tracks at three tempos are written as WAV files, read
//! back through the song analysis, and Lock steers a ball onto the beats it finds.

use bpe_audio::{SAMPLE_RATE, Soundtrack};
use bpe_scene::{Scene, SyncMode};

/// A click track: a low thump and a bright tick on every beat, a louder one on each bar.
fn click_track(bpm: f64, seconds: f64) -> Soundtrack {
    let rate = SAMPLE_RATE as f64;
    let mut samples = vec![0.0f32; (seconds * rate) as usize * 2];
    let period = 60.0 / bpm;
    let mut beat = 0;
    loop {
        let start = (0.25 + beat as f64 * period) * rate;
        if start as usize * 2 >= samples.len() {
            break;
        }
        let loud = if beat % 4 == 0 { 0.8 } else { 0.55 };
        for i in 0..(0.12 * rate) as usize {
            let t = i as f64 / rate;
            let thump = (std::f64::consts::TAU * 60.0 * t).sin() * (-t * 30.0).exp();
            let tick = (std::f64::consts::TAU * 2400.0 * t).sin() * (-t * 120.0).exp();
            let s = (loud * (0.7 * thump + 0.5 * tick)) as f32;
            let at = (start as usize + i) * 2;
            if at + 1 < samples.len() {
                samples[at] += s;
                samples[at + 1] += s;
            }
        }
        beat += 1;
    }
    Soundtrack { samples }
}

#[test]
fn lock_lands_on_the_beats_of_real_songs() {
    let dir = std::env::temp_dir().join(format!("bpe-click-tracks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for bpm in [96.0, 120.0, 140.0] {
        let path = dir.join(format!("click-{bpm}.wav"));
        click_track(bpm, 24.0).write_wav(&path).unwrap();
        let mut scene = Scene::beat_locked_bounce();
        scene.music.bpm = 0.0;
        scene.music.song = Some(path.display().to_string());
        scene.music.beat_sync.tolerance = 0.02;
        let sound = bpe_audio::render_scene(&scene, None).unwrap();
        assert!(sound.song_error.is_none(), "{:?}", sound.song_error);
        let song = sound.song.as_ref().unwrap();
        let found = song.features.bpm as f64;
        assert!((found - bpm).abs() / bpm < 0.02, "found {found:.1} BPM in a {bpm} BPM click track");
        // The beats found sit on the clicks.
        for &b in &song.features.beats {
            let off = ((b - 0.25) / (60.0 / bpm)).fract();
            let off = off.min(1.0 - off) * 60.0 / bpm;
            assert!(off < 0.02, "a beat at {b:.3} s is {off:.3} s from a click at {bpm} BPM");
        }
        // The lead ball's sounding hits land within 20 ms of a beat or half beat.
        let lead: Vec<f64> = sound
            .events
            .iter()
            .filter(|e| e.kind.is_bounce() && e.ball == 0 && e.speed >= scene.sound.min_impact_speed)
            .map(|e| e.time)
            .filter(|&t| t > 0.5 && t < scene.video.duration as f64)
            .collect();
        assert!(lead.len() >= 12, "only {} hits at {bpm} BPM", lead.len());
        let share = bpe_sim::beat_sync::on_beat_share_of(lead.iter().copied(), &sound.beats, 0.02);
        assert!(share >= 0.95, "{:.0}% of hits on the beat at {bpm} BPM", share * 100.0);

        scene.music.beat_sync.mode = SyncMode::Off;
        let free = bpe_audio::render_scene(&scene, None).unwrap();
        let free_share = free.on_beat.unwrap();
        assert!(share > free_share + 0.3, "Lock {share:.2} vs free {free_share:.2} at {bpm} BPM");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
