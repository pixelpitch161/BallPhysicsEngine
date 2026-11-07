//! Export checks: a short clip has picture and a (silent) sound track; failures leave no file behind.

use bpe_cli::{Encoder, ExportOptions, export_video, find_ffmpeg};
use bpe_gfx::headless_device;
use bpe_kit::{kit, new_scene, new_studio_scene};
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bpe-cli-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ready() -> Option<(bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue)> {
    let Ok(pair) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    if find_ffmpeg().is_none() {
        eprintln!("no ffmpeg available, skipping");
        return None;
    }
    Some(pair)
}

#[test]
fn exports_a_short_clip_with_sound() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let dir = temp_dir("clip");
    let mut options = ExportOptions::new(dir.join("one-ball.mp4"));
    options.max_frames = Some(30);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    let mut calls = 0;
    let used = export_video(&device, &queue, &scene, &kit, &options, |done, total| {
        calls += 1;
        assert!(done <= total && total == 30);
        true
    })
    .unwrap();
    assert_eq!(used, Encoder::Cpu);
    assert_eq!(calls, 30);
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,width,height:format=duration", "-of", "csv"])
        .arg(dir.join("one-ball.mp4"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.lines().any(|l| l.contains("video") && l.contains("270") && l.contains("480")), "{text}");
    assert!(text.lines().any(|l| l.contains("audio")), "{text}");
    let duration: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("format,"))
        .and_then(|d| d.trim().parse().ok())
        .unwrap_or_else(|| panic!("no duration in {text}"));
    assert!((duration - 0.5).abs() < 0.1, "{duration}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exports_a_scene_with_many_balls() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let (name, scene) = bpe_kit::scenes::stage3(&kit.registry).swap_remove(0);
    assert_eq!(name, "Ball pit");
    let dir = temp_dir("pit");
    let mut options = ExportOptions::new(dir.join("ball-pit.mp4"));
    options.max_frames = Some(30);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap();
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type:format=duration", "-of", "csv"])
        .arg(dir.join("ball-pit.mp4"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.lines().any(|l| l.contains("video")) && text.lines().any(|l| l.contains("audio")), "{text}");
    let duration: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("format,"))
        .and_then(|d| d.trim().parse().ok())
        .unwrap_or_else(|| panic!("no duration in {text}"));
    assert!((duration - 0.5).abs() < 0.1, "{duration}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exports_a_scene_with_a_trail_and_particles() {
    use bpe_core::SlotId;
    use bpe_kit::modules::look::layers::{PARTICLES, SHAPE_PARTICLES, SUB_GLOW, TRAIL};
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    let (shape, ball) = (bpe_core::ThingId(1), bpe_core::ThingId(2));
    let trail = scene.add_layer(reg, ball, SlotId::Look, TRAIL).unwrap();
    scene.add_sub_layer(reg, ball, SlotId::Look, trail, SUB_GLOW).unwrap();
    scene.add_layer(reg, ball, SlotId::Look, PARTICLES).unwrap();
    scene.add_layer(reg, shape, SlotId::Look, SHAPE_PARTICLES).unwrap();
    let dir = temp_dir("around");
    let mut options = ExportOptions::new(dir.join("around.mp4"));
    options.max_frames = Some(30);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap();
    let size = std::fs::metadata(dir.join("around.mp4")).unwrap().len();
    assert!(size > 1000, "{size} bytes");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_without_ffmpeg_fails_clearly_and_leaves_no_file() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let dir = temp_dir("noffmpeg");
    let mut options = ExportOptions::new(dir.join("out.mp4"));
    options.ffmpeg = "/no/such/ffmpeg".into();
    options.max_frames = Some(5);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    let error = export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap_err();
    assert!(error.to_string().starts_with("could not start ffmpeg"), "{error:#}");
    assert!(!dir.join("out.mp4").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_export_leaves_no_file() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let dir = temp_dir("cancel");
    let mut options = ExportOptions::new(dir.join("out.mp4"));
    options.max_frames = Some(60);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    let error = export_video(&device, &queue, &scene, &kit, &options, |done, _| done < 5).unwrap_err();
    assert!(error.to_string().contains("export cancelled"), "{error:#}");
    assert!(!dir.join("out.mp4").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn output_sizes_are_even_and_encoders_have_names() {
    assert_eq!(bpe_cli::output_size(1080, 1920, 0.25), (270, 480));
    assert_eq!(bpe_cli::output_size(1081, 1919, 1.0), (1080, 1918));
    assert_eq!(bpe_cli::output_size(10, 10, 0.0), (2, 2));
    for name in ["auto", "cpu", "nvidia", "amd", "intel"] {
        let e = Encoder::from_name(name).unwrap();
        assert!(!e.name().is_empty());
    }
    assert_eq!(Encoder::from_name("quantum"), None);
}

/// The audio of an exported file, decoded to interleaved stereo floats at 48 kHz.
fn decoded_audio(path: &std::path::Path) -> Vec<f32> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a", "-f", "f32le", "-ac", "2", "-ar", "48000", "-"])
        .output()
        .unwrap();
    out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

/// The audio stream's codec, sample rate and channels, as ffprobe reads them.
fn audio_stream(path: &std::path::Path) -> (String, u32, u32) {
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name,sample_rate,channels"])
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout).trim().to_owned();
    let parts: Vec<&str> = text.split(',').collect();
    let [codec, rate, channels] = parts[..] else { panic!("no audio stream: {text}") };
    (codec.to_owned(), rate.parse().unwrap(), channels.parse().unwrap())
}

#[test]
fn exported_mp4_has_320k_aac_with_the_tone() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().tags.insert("test-tone-decay=30".into());
    let fps = 60.0;
    let dir = temp_dir("tone");
    let out = dir.join("tone.mp4");
    let mut options = ExportOptions::new(&out);
    options.max_frames = Some(150);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    let mut last = 0;
    export_video(&device, &queue, &scene, &kit, &options, |done, total| {
        assert!(done >= last && done <= total && total == 150, "{done} of {total}");
        last = done;
        true
    })
    .unwrap();
    assert_eq!(last, 150);
    // AAC at 48 kHz stereo. (Its measured bit rate follows what is in it: a plain tone needs
    // far less than the 320 kbps asked for, which the encoder settings check.)
    assert_eq!(audio_stream(&out), ("aac".to_owned(), 48_000, 2));
    assert!(bpe_cli::AUDIO_ARGS.windows(2).any(|w| w == ["-b:a", "320k"]));
    // The tone is in the file near the first hit, as loud as the engine made it.
    let expected = bpe_sound::render_samples(&scene, &kit, 150.0 / fps);
    let first = expected.iter().position(|&v| v != 0.0).expect("the ball hits the shape") / 2;
    let audio = decoded_audio(&out);
    assert!(audio.len() / 2 >= first + 4_800, "the audio runs past the first hit: {} frames", audio.len() / 2);
    let peak = |a: &[f32]| a.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let around = peak(&audio[2 * first.saturating_sub(2_400)..2 * (first + 4_800)]);
    let before = peak(&audio[..2 * first.saturating_sub(4_800)]);
    assert!(around > 0.1, "the tone is there: {around}");
    assert!(before < 0.01, "and silence before it: {before}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_clip_shorter_than_ten_frames_keeps_its_sound_stems_and_midi() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().tags.insert("test-tone-decay=30".into());
    let dir = temp_dir("short");
    let out = dir.join("short.mp4");
    let mut options = ExportOptions::new(&out);
    options.max_frames = Some(5);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    options.stems = true;
    options.midi = true;
    export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap();
    let (buses, master) = bpe_sound::stem_paths(&scene, &out);
    for stem in buses.iter().chain([&master]) {
        assert!(stem.exists(), "{} was written", stem.display());
    }
    assert!(bpe_sound::midi_path(&out).exists(), "the MIDI file was written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_export_leaves_stems_and_midi_it_never_wrote() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    // No sound: the stems and the MIDI file beside the video are an earlier export's.
    let scene = new_scene(&kit.registry);
    let dir = temp_dir("keep");
    let out = dir.join("out.mp4");
    let (buses, master) = bpe_sound::stem_paths(&scene, &out);
    let earlier: Vec<PathBuf> = buses.iter().cloned().chain([master, bpe_sound::midi_path(&out)]).collect();
    for f in &earlier {
        std::fs::write(f, b"an earlier export").unwrap();
    }
    let mut options = ExportOptions::new(&out);
    options.max_frames = Some(60);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    options.stems = true;
    options.midi = true;
    let error = export_video(&device, &queue, &scene, &kit, &options, |done, _| done < 5).unwrap_err();
    assert!(error.to_string().contains("export cancelled"), "{error:#}");
    assert!(!out.exists());
    for f in &earlier {
        assert!(f.exists(), "{} is left alone", f.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blank_scene_exports_a_silent_track() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let scene = new_studio_scene(&kit.registry);
    let dir = temp_dir("silent");
    let out = dir.join("blank.mp4");
    let mut options = ExportOptions::new(&out);
    options.max_frames = Some(30);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap();
    assert_eq!(audio_stream(&out).0, "aac");
    let audio = decoded_audio(&out);
    assert!(!audio.is_empty() && audio.iter().all(|v| v.abs() < 1e-6), "a silent track");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_scene_with_text_exports_a_frame() {
    use bpe_core::{Scene, ThingId, Value};
    let Ok((device, queue)) = headless_device() else { return eprintln!("no GPU adapter available, skipping") };
    let kit = kit();
    let mut scene = Scene::empty(&kit.registry);
    bpe_kit::ensure_camera(&mut scene, &kit.registry);
    let id = bpe_kit::add_thing(&mut scene, &kit.registry, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.params.set("words", Value::Text("I".into()));
    t.params.set("size", Value::Number(800.0));
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (pixels, (w, h), _) = bpe_cli::final_frame(&device, &queue, &scene, &kit, 0.0, 0.25, None, &stop).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        pixels[i] as u32 + pixels[i + 1] as u32 + pixels[i + 2] as u32
    };
    // The white I in the middle, the background at the side.
    assert!(at(w / 2, h / 2) > at(w / 10, h / 2) + 300, "{} vs {}", at(w / 2, h / 2), at(w / 10, h / 2));
}
