//! Video checks: frames outside transitions are the same bytes as each scene's own, transitions
//! mix the two, scenes of another shape cover the video, gaps are black, and the export has
//! picture and sound and cleans up after itself.

use bpe_cli::arranged::{NO_CLIPS, NOT_FOR_VIDEOS, arranged_frames, still_frame, tidied};
use bpe_cli::{Encoder, ExportOptions, FrameMaker, export_arranged, find_ffmpeg};
use bpe_core::video::{Clip, ClipId, KeptId, KeptScene, Transition, TransitionKind, Video};
use bpe_core::{Scene, SlotId, ThingId, TrackColour, Value, layered_key};
use bpe_gfx::yuv::PixelLayout;
use bpe_gfx::{Readback, Renderer, test_device};
use bpe_kit::video::clip_scene;
use bpe_kit::{Kit, kit, new_scene};
use std::path::PathBuf;

const W: u32 = 270;
const H: u32 = 480;
const FPS: u32 = 30;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bpe-cli-video-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gpu() -> (bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue) {
    test_device().expect("these tests draw")
}

fn ffmpeg_ready() -> bool {
    let ok = find_ffmpeg().is_some();
    if !ok {
        eprintln!("no ffmpeg available, skipping");
    }
    ok
}

/// A small one-ball scene, `w` × `h`, 2 s long.
fn scene(kit: &Kit, w: u32, h: u32) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let root = &mut scene.things.get_mut(&ThingId::ROOT).unwrap().params;
    root.set("width", Value::Int(i64::from(w)));
    root.set("height", Value::Int(i64::from(h)));
    root.set("duration", Value::Number(2.0));
    scene
}

/// A video at 270 × 480, 30 fps, of `scenes` and `clips` (scene, start, length, offset, look).
fn video(scenes: Vec<Scene>, clips: &[(u32, f64, f64, f64, TransitionKind)]) -> Video {
    let mut v = Video::new();
    (v.settings.width, v.settings.height, v.settings.fps) = (W, H, FPS);
    for (i, scene) in scenes.into_iter().enumerate() {
        v.scenes.insert(KeptId(i as u32), KeptScene { name: format!("Scene {i}"), colour: TrackColour::LIME, scene });
    }
    for (i, &(scene, start, length, offset, kind)) in clips.iter().enumerate() {
        v.clips.push(Clip {
            id: ClipId(i as u32),
            scene: KeptId(scene),
            start,
            length,
            offset,
            transition_in: Transition { kind, ..Transition::default() },
            ..Clip::default()
        });
    }
    v
}

/// Every frame of the video, RGBA.
fn frames(video: &Video, kit: &Kit) -> Vec<Vec<u8>> {
    let (device, queue) = gpu();
    let mut out = Vec::new();
    let done = arranged_frames(&device, &queue, video, kit, 1.0, PixelLayout::Rgba, None, |i, px| {
        assert_eq!(i as usize, out.len());
        out.push(px);
        true
    })
    .unwrap();
    assert!(done);
    out
}

/// The scene's own frames `0..count` as its export draws them (RGBA readback), at its own size.
fn own_frames(scene: &Scene, kit: &Kit, count: u32) -> Vec<Vec<u8>> {
    let (device, queue) = gpu();
    let s = bpe_kit::video::scene_settings(scene, &kit.registry);
    let mut renderer = Renderer::new(&device, s.width, s.height);
    let mut readback = Readback::new(&device, s.width, s.height, 3);
    let mut maker = FrameMaker::new(scene, kit);
    let mut out = Vec::new();
    for index in 0..count {
        let samples = maker.samples(index as f64 / FPS as f64, FPS, None);
        renderer.render_samples(&device, &queue, &samples);
        out.extend(readback.push(&device, &queue, &renderer).unwrap());
    }
    out.extend(readback.finish(&device).unwrap());
    out
}

/// The clip's scene as the video plays it.
fn played(video: &Video, kit: &Kit, clip: usize) -> Scene {
    let video = tidied(video, kit);
    clip_scene(&video, &video.clips[clip], &kit.registry)
}

fn same_bytes_case(b_scene: Scene) {
    let kit = kit();
    let v = video(
        vec![scene(&kit, W, H), b_scene],
        &[(0, 0.0, 1.5, 0.0, TransitionKind::Cut), (1, 1.0, 1.5, 0.5, TransitionKind::Crossfade)],
    );
    let got = frames(&v, &kit);
    assert_eq!(got.len(), 75);
    let a = own_frames(&played(&v, &kit, 0), &kit, 30);
    let b = own_frames(&played(&v, &kit, 1), &kit, 60);
    for f in 0..30 {
        assert!(got[f] == a[f], "frame {f} is scene A's frame {f}");
    }
    for f in 45..75 {
        assert!(got[f] == b[f - 15], "frame {f} is scene B's frame {}", f - 15);
    }
    assert!(a[0] != a[29], "the ball moves, so the frames say something");
}

#[test]
fn same_bytes_outside_transitions() {
    let kit = kit();
    same_bytes_case(scene(&kit, W, H));
}

#[test]
fn same_bytes_with_an_offset_and_memory() {
    let kit = kit();
    let mut b = scene(&kit, W, H);
    b.add_slot(&kit.registry, ThingId::ROOT, SlotId::Shader, "effects").unwrap();
    let echo = b.add_layer(&kit.registry, ThingId::ROOT, SlotId::Shader, "picture.echo").unwrap();
    b.card_mut(ThingId::ROOT, SlotId::Shader).unwrap().params.set(&layered_key(echo, "fade"), Value::Number(0.9));
    same_bytes_case(b);
}

#[test]
fn a_transition_mixes_the_two() {
    let kit = kit();
    let v = video(
        vec![scene(&kit, W, H), scene(&kit, W, H)],
        &[(0, 0.0, 1.5, 0.0, TransitionKind::Cut), (1, 1.0, 1.5, 0.5, TransitionKind::Crossfade)],
    );
    let got = frames(&v, &kit);
    let a = own_frames(&played(&v, &kit, 0), &kit, 45);
    let b = own_frames(&played(&v, &kit, 1), &kit, 60);
    // Frame 37 is halfway through the 15-frame overlap from frame 30: progress 7/15.
    let p = 7.0f32 / 15.0;
    let e = p * p * (3.0 - 2.0 * p);
    let (mix, old, new) = (&got[37], &a[37], &b[37 - 30 + 15]);
    for i in (0..mix.len()).step_by(4 * 97) {
        for c in 0..3 {
            let want = f32::from(old[i + c]) + (f32::from(new[i + c]) - f32::from(old[i + c])) * e;
            assert!((f32::from(mix[i + c]) - want).abs() <= 2.0, "byte {i}: {} vs {want}", mix[i + c]);
        }
    }
}

#[test]
fn a_square_scene_covers_the_video() {
    let kit = kit();
    let v = video(vec![scene(&kit, 480, 480)], &[(0, 0.0, 0.5, 0.0, TransitionKind::Cut)]);
    let got = frames(&v, &kit);
    let own = own_frames(&played(&v, &kit, 0), &kit, 1);
    // The middle 270 columns of the 480 × 480 picture (105 to 375) fill the frame.
    for y in 0..H as usize {
        for x in 0..W as usize {
            let g = &got[0][(y * W as usize + x) * 4..][..3];
            let o = &own[0][(y * 480 + x + 105) * 4..][..3];
            for c in 0..3 {
                assert!((i32::from(g[c]) - i32::from(o[c])).abs() <= 1, "({x}, {y}): {g:?} vs {o:?}");
            }
        }
    }
}

#[test]
fn a_gap_is_black() {
    let kit = kit();
    let v = video(
        vec![scene(&kit, W, H)],
        &[(0, 0.0, 0.5, 0.0, TransitionKind::Cut), (0, 1.0, 0.5, 0.0, TransitionKind::Cut)],
    );
    let got = frames(&v, &kit);
    assert_eq!(got.len(), 45);
    for frame in &got[15..30] {
        assert!(frame.chunks(4).all(|px| px == [0, 0, 0, 255]), "the gap is black");
    }
    assert!(got[14].chunks(4).any(|px| px != [0, 0, 0, 255]));
}

/// The editor's still frame of `video` at `frame`, full size.
fn still(video: &Video, kit: &Kit, frame: u32) -> Vec<u8> {
    let (device, queue) = gpu();
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (pixels, size) = still_frame(&device, &queue, video, kit, frame, 1.0, &stop).unwrap();
    assert_eq!(size, (W, H));
    pixels
}

#[test]
fn still_frame_matches_the_export_frame_outside_a_transition() {
    let kit = kit();
    let v = video(
        vec![scene(&kit, W, H), scene(&kit, W, H)],
        &[(0, 0.0, 1.5, 0.0, TransitionKind::Cut), (1, 1.0, 1.5, 0.5, TransitionKind::Crossfade)],
    );
    let got = frames(&v, &kit);
    // The scene has no Effects that remember, so the quick run-up draws the same picture.
    for f in [0, 20, 50, 74] {
        assert!(still(&v, &kit, f) == got[f as usize], "still frame {f} is the export's");
    }
    // In the transition it mixes the same two pictures the export does.
    let mid = still(&v, &kit, 37);
    let diff = mid.iter().zip(&got[37]).map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs()).max().unwrap();
    assert!(diff <= 1, "the transition's still frame is the export's ({diff} off)");
}

#[test]
fn still_frame_in_a_gap_is_black() {
    let kit = kit();
    let v = video(
        vec![scene(&kit, W, H)],
        &[(0, 0.0, 0.5, 0.0, TransitionKind::Cut), (0, 1.0, 0.5, 0.0, TransitionKind::Cut)],
    );
    for f in [20, 200] {
        assert!(still(&v, &kit, f).chunks(4).all(|px| px == [0, 0, 0, 255]), "frame {f} is black");
    }
    assert!(still(&video(vec![], &[]), &kit, 0).chunks(4).all(|px| px == [0, 0, 0, 255]), "an empty video is black");
    let stop = std::sync::atomic::AtomicBool::new(true);
    let (device, queue) = gpu();
    let stopped = still_frame(&device, &queue, &v, &kit, 5, 1.0, &stop);
    assert!(stopped.is_err(), "a stopped ask gives up");
}

fn probe(path: &std::path::Path) -> String {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,width,height:format=duration", "-of", "csv"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn duration(text: &str) -> f64 {
    text.lines()
        .find_map(|l| l.strip_prefix("format,"))
        .and_then(|d| d.trim().parse().ok())
        .unwrap_or_else(|| panic!("no duration in {text}"))
}

/// A scene whose ball makes a sound on every hit.
fn sounding(kit: &Kit) -> Scene {
    let mut s = scene(kit, W, H);
    let shape = s.order.iter().copied().find(|id| s.things[id].kind == "shape").unwrap();
    s.things.get_mut(&shape).unwrap().tags.insert("test-tone-decay=30".into());
    s
}

#[test]
fn exports_a_two_clip_video_with_sound() {
    if !ffmpeg_ready() {
        return;
    }
    let (device, queue) = gpu();
    let kit = bpe_sound::kit();
    let v = video(
        vec![sounding(&kit), sounding(&kit)],
        &[(0, 0.0, 1.0, 0.0, TransitionKind::Cut), (1, 0.8, 0.6, 0.3, TransitionKind::Crossfade)],
    );
    assert!(bpe_sound::video_has_sound(&v, &kit));
    let dir = temp_dir("two");
    let out = dir.join("two.mp4");
    let mut options = ExportOptions::new(&out);
    options.encoder = Encoder::Cpu;
    let mut last = 0;
    let used = export_arranged(&device, &queue, &v, &kit, &options, |done, total| {
        assert!(done >= last && done <= total && total == 42, "{done} of {total}");
        last = done;
        true
    })
    .unwrap();
    assert_eq!((used, last), (Encoder::Cpu, 42));
    let text = probe(&out);
    assert!(text.lines().any(|l| l.contains("video") && l.contains("270") && l.contains("480")), "{text}");
    assert!(text.lines().any(|l| l.contains("audio")), "{text}");
    assert!((duration(&text) - tidied(&v, &kit).end()).abs() < 0.1, "{text}");
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 1, "only the video is left: {left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_silent_video_has_a_silent_track() {
    if !ffmpeg_ready() {
        return;
    }
    let (device, queue) = gpu();
    let kit = kit();
    let v = video(vec![scene(&kit, W, H)], &[(0, 0.0, 0.5, 0.0, TransitionKind::Cut)]);
    assert!(!bpe_sound::video_has_sound(&v, &kit));
    let dir = temp_dir("silent");
    let out = dir.join("silent.mp4");
    let mut options = ExportOptions::new(&out);
    options.encoder = Encoder::Cpu;
    export_arranged(&device, &queue, &v, &kit, &options, |_, _| true).unwrap();
    let text = probe(&out);
    assert!(text.lines().any(|l| l.contains("audio")), "{text}");
    assert!((duration(&text) - 0.5).abs() < 0.1, "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_video_says_so() {
    let (device, queue) = gpu();
    let kit = kit();
    let dir = temp_dir("empty");
    let out = dir.join("empty.mp4");
    let options = ExportOptions::new(&out);
    let error = export_arranged(&device, &queue, &Video::new(), &kit, &options, |_, _| true).unwrap_err();
    assert_eq!(error.to_string(), NO_CLIPS);
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stems_are_refused_for_a_video() {
    let (device, queue) = gpu();
    let kit = kit();
    let v = video(vec![scene(&kit, W, H)], &[(0, 0.0, 0.5, 0.0, TransitionKind::Cut)]);
    let dir = temp_dir("stems");
    let out = dir.join("stems.mp4");
    let sets: [fn(&mut ExportOptions); 3] = [|o| o.stems = true, |o| o.midi = true, |o| o.bake = Some(true)];
    for set in sets {
        let mut options = ExportOptions::new(&out);
        set(&mut options);
        let error = export_arranged(&device, &queue, &v, &kit, &options, |_, _| true).unwrap_err();
        assert_eq!(error.to_string(), NOT_FOR_VIDEOS);
    }
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "no file left");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_video_export_leaves_no_file() {
    if !ffmpeg_ready() {
        return;
    }
    let (device, queue) = gpu();
    let kit = bpe_sound::kit();
    let v = video(
        vec![sounding(&kit), sounding(&kit)],
        &[(0, 0.0, 1.0, 0.0, TransitionKind::Cut), (1, 0.8, 1.0, 0.0, TransitionKind::ZoomThrough)],
    );
    let dir = temp_dir("cancel");
    let mut options = ExportOptions::new(dir.join("out.mp4"));
    options.encoder = Encoder::Cpu;
    let error = export_arranged(&device, &queue, &v, &kit, &options, |done, _| done < 20).unwrap_err();
    assert!(error.to_string().contains("export cancelled"), "{error:#}");
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    assert!(left.is_empty(), "files left behind: {left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_command_line_renders_a_video_file() {
    if !ffmpeg_ready() {
        return;
    }
    let _ = gpu();
    let kit = kit();
    let v = video(
        vec![scene(&kit, W, H), scene(&kit, 480, 480)],
        &[(0, 0.0, 0.5, 0.0, TransitionKind::Cut), (1, 0.4, 0.5, 0.0, TransitionKind::WIPE)],
    );
    let dir = temp_dir("cli");
    let file = dir.join("drop tape.ballvideo");
    std::fs::write(&file, bpe_core::video_save::save(&v)).unwrap();
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_bpe-render-next"))
        .arg(&file)
        .args(["--encoder", "cpu"])
        .output()
        .unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let text = probe(&dir.join("drop tape.mp4"));
    assert!(text.lines().any(|l| l.contains("video") && l.contains("270")), "{text}");
    assert!((duration(&text) - 0.9).abs() < 0.1, "{text}");
    let refused =
        std::process::Command::new(env!("CARGO_BIN_EXE_bpe-render-next")).arg(&file).arg("--stems").output().unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains(NOT_FOR_VIDEOS));
    let _ = std::fs::remove_dir_all(&dir);
}
