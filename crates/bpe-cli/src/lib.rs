//! Video export for the modular studio.
//!
//! Every frame is drawn offscreen and piped as raw RGBA into ffmpeg, which encodes H.264 video
//! into an mp4 with a silent AAC track (upload apps expect the clip to have sound). Export does
//! not run in real time: a slow machine just takes longer, and the result is identical.

use anyhow::{Context, bail};
use bpe_core::{Scene, Settings};
use bpe_gfx::{Frame, Readback, Renderer, build_frame_sensing, wgpu};
use bpe_kit::{Kit, SceneSettings};
use bpe_runtime::{Log, World};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub output: PathBuf,
    /// ffmpeg executable. A bare name is looked up on PATH.
    pub ffmpeg: PathBuf,
    /// H.264 quality: lower is better and bigger. 18 is visually lossless.
    pub crf: u8,
    pub preset: String,
    /// Output size as a fraction of the scene's video size, for quick drafts.
    pub scale: f32,
    /// Only render this many frames (for tests and drafts).
    pub max_frames: Option<u32>,
    /// Which video encoder to use.
    pub encoder: Encoder,
    /// Also save each bus and the master as 24-bit WAVs beside the video.
    pub stems: bool,
    /// Also save the notes the shapes played as a MIDI file beside the video.
    pub midi: bool,
    /// Bake the lighting (the cinema render, lighting part 3): `None` does as the scene's Lighting
    /// card says (On export); a scene without the card always renders live.
    pub bake: Option<bool>,
}

impl ExportOptions {
    pub fn new(output: impl Into<PathBuf>) -> Self {
        Self {
            output: output.into(),
            ffmpeg: find_ffmpeg().unwrap_or_else(|| "ffmpeg".into()),
            crf: 18,
            preset: "medium".into(),
            scale: 1.0,
            max_frames: None,
            encoder: Encoder::Auto,
            stems: false,
            midi: false,
            bake: None,
        }
    }
}

/// The program that compresses the video. The graphics card encoders are much faster than the
/// CPU and leave it free for drawing; the picture going in is exactly the same either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    /// The fastest one that works on this computer.
    Auto,
    /// x264 on the CPU: works everywhere, smallest files for the quality.
    Cpu,
    /// NVIDIA graphics cards.
    Nvidia,
    /// AMD graphics cards.
    Amd,
    /// Intel graphics (Quick Sync).
    Intel,
}

impl Encoder {
    pub const GPU: [Encoder; 3] = [Encoder::Nvidia, Encoder::Amd, Encoder::Intel];

    pub fn name(self) -> &'static str {
        match self {
            Encoder::Auto => "automatic",
            Encoder::Cpu => "CPU (x264)",
            Encoder::Nvidia => "NVIDIA graphics card (NVENC)",
            Encoder::Amd => "AMD graphics card (AMF)",
            Encoder::Intel => "Intel graphics (Quick Sync)",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "auto" => Encoder::Auto,
            "cpu" | "x264" => Encoder::Cpu,
            "nvidia" | "nvenc" => Encoder::Nvidia,
            "amd" | "amf" => Encoder::Amd,
            "intel" | "qsv" => Encoder::Intel,
            _ => return None,
        })
    }

    /// ffmpeg arguments for this encoder at x264-style quality `crf` and speed `preset`.
    fn args(self, crf: u8, preset: &str) -> Vec<String> {
        let crf = crf.to_string();
        let slow = matches!(preset, "slow" | "slower" | "veryslow" | "placebo");
        let fast = matches!(preset, "ultrafast" | "superfast" | "veryfast" | "faster" | "fast");
        let pick = |s, m, f| {
            if slow {
                s
            } else if fast {
                f
            } else {
                m
            }
        };
        let args: Vec<&str> = match self {
            Encoder::Auto | Encoder::Cpu => {
                vec!["-c:v", "libx264", "-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p", "-profile:v", "high"]
            }
            Encoder::Nvidia => vec![
                "-c:v",
                "h264_nvenc",
                "-preset",
                pick("p6", "p4", "p2"),
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                &crf,
                "-b:v",
                "0",
                "-pix_fmt",
                "yuv420p",
                "-profile:v",
                "high",
            ],
            Encoder::Amd => vec![
                "-c:v",
                "h264_amf",
                "-quality",
                pick("quality", "balanced", "speed"),
                "-rc",
                "cqp",
                "-qp_i",
                &crf,
                "-qp_p",
                &crf,
                "-qp_b",
                &crf,
                "-pix_fmt",
                "nv12",
                "-profile:v",
                "high",
            ],
            Encoder::Intel => vec![
                "-c:v",
                "h264_qsv",
                "-preset",
                pick("slower", "medium", "veryfast"),
                "-global_quality",
                &crf,
                "-pix_fmt",
                "nv12",
                "-profile:v",
                "high",
            ],
        };
        args.into_iter().map(String::from).collect()
    }
}

/// The graphics card encoders that work with this ffmpeg on this computer, fastest first.
/// Each is tried on a tiny clip once, and the answer is remembered.
pub fn gpu_encoders(ffmpeg: &Path) -> Vec<Encoder> {
    use std::sync::Mutex;
    static FOUND: Mutex<BTreeMap<PathBuf, Vec<Encoder>>> = Mutex::new(BTreeMap::new());
    if let Some(found) = FOUND.lock().unwrap_or_else(|e| e.into_inner()).get(ffmpeg) {
        return found.clone();
    }
    let working: Vec<Encoder> = Encoder::GPU.into_iter().filter(|&encoder| encoder_works(ffmpeg, encoder)).collect();
    FOUND.lock().unwrap_or_else(|e| e.into_inner()).insert(ffmpeg.to_path_buf(), working.clone());
    working
}

/// Whether ffmpeg can encode a few frames with `encoder`.
fn encoder_works(ffmpeg: &Path, encoder: Encoder) -> bool {
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=black:s=256x256:r=30"]);
    command.args(["-frames:v", "3"]);
    command.args(encoder.args(20, "medium"));
    command.args(["-f", "null", "-"]);
    let Ok(mut child) = spawn(command, ffmpeg) else { return false };
    drop(child.stdin.take());
    // A missing driver can make ffmpeg hang for a while, so give up after a few seconds.
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < std::time::Duration::from_secs(8) => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// The encoder `choice` stands for on this computer.
pub fn resolve_encoder(choice: Encoder, ffmpeg: &Path) -> Encoder {
    match choice {
        Encoder::Auto => gpu_encoders(ffmpeg).first().copied().unwrap_or(Encoder::Cpu),
        other => other,
    }
}

/// The root settings that size and time the video.
fn video_of(scene: &Scene, kit: &Kit) -> anyhow::Result<(bpe_core::Resolved, SceneSettings)> {
    let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
    let settings = {
        let root = resolved.things.get(&bpe_core::ThingId::ROOT).context("the scene has no root")?;
        SceneSettings::from_params(&root.params)
    };
    Ok((resolved, settings))
}

/// How many frames a scene's video has.
pub fn frame_count(settings: &SceneSettings) -> u32 {
    (settings.duration.max(0.0) as f64 * settings.fps.max(1) as f64).round().max(1.0) as u32
}

/// Renders `scene` to an mp4. `progress` is called after every frame with (done, total);
/// return `false` from it to cancel, which deletes the partial file. If a graphics card
/// encoder fails, the export starts again on the CPU. Gives back the encoder that made it.
pub fn export_video(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    options: &ExportOptions,
    mut progress: impl FnMut(u32, u32) -> bool,
) -> anyhow::Result<Encoder> {
    let encoder = resolve_encoder(options.encoder, &options.ffmpeg);
    match export_with(device, queue, scene, kit, options, encoder, &mut progress) {
        Err(error) if retry_on_cpu(encoder, &error) => {
            export_with(device, queue, scene, kit, options, Encoder::Cpu, &mut progress)
                .with_context(|| format!("the {} failed first ({error:#})", encoder.name()))?;
            Ok(Encoder::Cpu)
        }
        result => result.map(|()| encoder),
    }
}

const CANCELLED: &str = "export cancelled";

/// How the sound is encoded: AAC at 320 kbps, 48 kHz.
pub const AUDIO_ARGS: [&str; 6] = ["-c:a", "aac", "-b:a", "320k", "-ar", "48000"];

/// Whether a failed export is worth trying again on the CPU: only when a graphics card encoder
/// made the video, and not because it was cancelled.
fn retry_on_cpu(encoder: Encoder, error: &anyhow::Error) -> bool {
    encoder != Encoder::Cpu && !error.to_string().contains(CANCELLED)
}

/// Frames that can be on their way back from the graphics card at once.
const FRAMES_IN_FLIGHT: usize = 3;
/// Finished frames that can wait for ffmpeg at once.
const FRAMES_QUEUED: usize = 4;

fn export_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    options: &ExportOptions,
    encoder: Encoder,
    progress: &mut impl FnMut(u32, u32) -> bool,
) -> anyhow::Result<()> {
    // Huge swarms step on the card the video is drawn with.
    let _card = bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(device, queue)));
    let (_, video) = video_of(scene, kit)?;
    let (width, height) = output_size(video.width, video.height, options.scale);
    let fps = video.fps.max(1);
    let mut output = options.output.clone();
    if output.extension().is_none() {
        output.set_extension("mp4");
    }
    let frames_in_video = frame_count(&video);
    let total = options.max_frames.map_or(frames_in_video, |m| m.min(frames_in_video)).max(1);
    let picture = Picture { width, height, fps, total, encoder };

    let has_sound = bpe_sound::has_sound(scene);
    let temp = tempfile::tempdir().context("a folder for the sound")?;
    let sound = temp.path().join("sound.wav");
    let extras = bpe_sound::Extras {
        stems: options.stems.then(|| bpe_sound::stem_paths(scene, &output)),
        midi: options.midi.then(|| bpe_sound::midi_path(&output)),
    };
    // The video and the stems and MIDI file beside it, all gone if the export stops. The stems
    // and MIDI file are only this export's when its sound is rendered: a file of the same name
    // from an earlier export is left alone.
    let written: Vec<PathBuf> = std::iter::once(output.clone())
        .chain(
            extras
                .stems
                .iter()
                .filter(|_| has_sound)
                .flat_map(|(buses, master)| buses.iter().cloned().chain([master.clone()])),
        )
        .chain(extras.midi.clone().filter(|_| has_sound))
        .collect();
    let remove_outputs = || {
        for f in &written {
            let _ = std::fs::remove_file(f);
        }
    };
    if !has_sound {
        // One pass: the frames and a silent track straight into the video.
        let made =
            draw_video(device, queue, scene, kit, options, &picture, &output, true, |done| progress(done, total));
        if made.is_err() {
            remove_outputs();
        }
        return made;
    }

    // The sound is worked out on its own thread while the frames are drawn into a video-only file
    // (W7, S13); then a quick second pass puts the two together. The bar is nine tenths frames and
    // one tenth sound, both moving at once.
    let seconds = total as f64 / fps as f64;
    let video_only = temp.path().join("video.mp4");
    let sound_at = AtomicU64::new(0f64.to_bits());
    let cancel = AtomicBool::new(false);
    let shown = |frames: u32| {
        let heard = f64::from_bits(sound_at.load(Ordering::Relaxed)).clamp(0.0, 1.0);
        let share = frames as f64 / total as f64 * 0.9 + heard * 0.1;
        ((share * total as f64).floor() as u32).min(total)
    };
    let made = std::thread::scope(|s| -> anyhow::Result<()> {
        let mut job = Some(s.spawn(|| {
            bpe_sound::render_sound(scene, kit, &sound, seconds, &extras, |done| {
                sound_at.store(done.to_bits(), Ordering::Relaxed);
                !cancel.load(Ordering::Relaxed)
            })
        }));
        let join = |job: std::thread::ScopedJoinHandle<'_, anyhow::Result<()>>| {
            job.join().unwrap_or_else(|_| Err(anyhow::anyhow!("the sound render stopped unexpectedly")))
        };
        let mut heard: Option<anyhow::Result<()>> = None;
        let mut cancelled = false;
        let drawn = draw_video(device, queue, scene, kit, options, &picture, &video_only, false, |done| {
            // A sound that fails stops the frames.
            if job.as_ref().is_some_and(|j| j.is_finished())
                && let Some(j) = job.take()
            {
                let result = join(j);
                let failed = result.is_err();
                heard = Some(result);
                if failed {
                    return false;
                }
            }
            let go_on = progress(shown(done), total);
            cancelled |= !go_on;
            go_on
        });
        if drawn.is_err() {
            cancel.store(true, Ordering::Relaxed);
        }
        // The frames are done: wait for the sound, the bar still moving.
        while job.as_ref().is_some_and(|j| !j.is_finished()) {
            if !cancelled && !progress(shown(total), total) {
                cancelled = true;
                cancel.store(true, Ordering::Relaxed);
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let heard = match job.take() {
            Some(j) => join(j),
            None => heard.unwrap_or(Ok(())),
        };
        match (heard, drawn) {
            (Err(e), _) if !e.to_string().contains("cancelled") => Err(e),
            (_, Err(e)) => Err(e),
            _ if cancelled => Err(anyhow::anyhow!(CANCELLED)),
            (Err(_), Ok(())) => Err(anyhow::anyhow!(CANCELLED)),
            (Ok(()), Ok(())) => Ok(()),
        }
    })
    .and_then(|()| if progress(total, total) { Ok(()) } else { Err(anyhow::anyhow!(CANCELLED)) })
    .and_then(|()| mux(&options.ffmpeg, &video_only, &sound, &output));
    if made.is_err() {
        remove_outputs();
    }
    made
}

/// The video's size, speed, length in frames and encoder.
struct Picture {
    width: u32,
    height: u32,
    fps: u32,
    total: u32,
    encoder: Encoder,
}

/// Draws the frames into `output` through ffmpeg: with a silent sound track when `silence`, else
/// the picture alone (to be put together with the sound by [`mux`]). `progress` is told how many
/// frames are done after each one; `false` from it stops the export. The caller removes the files.
#[allow(clippy::too_many_arguments)]
fn draw_video(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    options: &ExportOptions,
    picture: &Picture,
    output: &Path,
    silence: bool,
    mut progress: impl FnMut(u32) -> bool,
) -> anyhow::Result<()> {
    let Picture { width, height, fps, total, encoder } = *picture;
    let mut ffmpeg = Command::new(&options.ffmpeg);
    ffmpeg.args(["-hide_banner", "-loglevel", "error", "-y"]);
    ffmpeg.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{width}x{height}"), "-r", &fps.to_string()]);
    ffmpeg.args(["-i", "-"]);
    if silence {
        // A silent stereo track, so apps that expect sound take the clip.
        ffmpeg.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]);
        ffmpeg.args(["-map", "0:v", "-map", "1:a"]);
    }
    ffmpeg.args(["-vf", "scale=out_color_matrix=bt709:out_range=tv"]);
    ffmpeg.args(encoder.args(options.crf, &options.preset));
    ffmpeg.args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]);
    if silence {
        ffmpeg.args(AUDIO_ARGS);
        ffmpeg.args(["-movflags", "+faststart", "-shortest"]);
    }
    ffmpeg.args(["-f", "mp4"]);
    ffmpeg.arg(output);
    let mut child = spawn(ffmpeg, &options.ffmpeg)?;
    let mut stdin = child.stdin.take().context("ffmpeg stdin")?;
    let stderr = collect_stderr(&mut child);

    // ffmpeg is fed on its own thread, so drawing carries on while it encodes.
    let (frames, queued) = std::sync::mpsc::sync_channel::<Vec<u8>>(FRAMES_QUEUED);
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        for pixels in queued {
            stdin.write_all(&pixels)?;
        }
        Ok(())
    });
    let stop = |child: &mut Child, writer: std::thread::JoinHandle<std::io::Result<()>>, kill: bool| {
        if kill {
            let _ = child.kill();
        }
        let written = writer.join().unwrap_or(Ok(()));
        let _ = child.wait();
        written
    };

    let mut renderer = Renderer::new(device, width, height);
    let mut readback = Readback::new(device, width, height, FRAMES_IN_FLIGHT);
    // Drawn from a run that keeps pace with the frames, exactly as the preview plays.
    let mut maker = FrameMaker::new(scene, kit);
    let bake = bake_for(scene, kit, options.bake);
    let mut failed = None;
    for index in 0..total {
        let time = index as f64 / fps as f64;
        let samples = maker.samples(time, fps, bake.as_ref());
        renderer.render_samples(device, queue, &samples);
        let ready = match readback.push(device, queue, &renderer) {
            Ok(ready) => ready,
            Err(error) => {
                failed = Some(error);
                break;
            }
        };
        if let Some(pixels) = ready
            && frames.send(pixels).is_err()
        {
            break;
        }
        if !progress(index + 1) {
            drop(frames);
            let _ = stop(&mut child, writer, true);
            bail!(CANCELLED);
        }
    }
    if failed.is_none() {
        match readback.finish(device) {
            Ok(rest) => {
                for pixels in rest {
                    if frames.send(pixels).is_err() {
                        break;
                    }
                }
            }
            Err(error) => failed = Some(error),
        }
    }
    drop(frames);
    if let Some(error) = failed {
        let _ = stop(&mut child, writer, true);
        return Err(error);
    }
    if let Err(error) = writer.join().unwrap_or(Ok(())) {
        let _ = child.wait();
        bail!("ffmpeg stopped accepting frames ({error}): {}", stderr.join().unwrap_or_default());
    }
    let status = child.wait()?;
    if !status.success() {
        bail!("ffmpeg failed ({status}): {}", stderr.join().unwrap_or_default());
    }
    Ok(())
}

/// Puts the drawn `video` and the `sound` together into `output`: the picture copied as it is, the
/// sound encoded as every export's is.
fn mux(ffmpeg: &Path, video: &Path, sound: &Path, output: &Path) -> anyhow::Result<()> {
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    command.arg("-i").arg(video).arg("-i").arg(sound);
    command.args(["-map", "0:v", "-map", "1:a", "-c:v", "copy"]);
    command.args(AUDIO_ARGS);
    command.args(["-movflags", "+faststart", "-shortest", "-f", "mp4"]);
    command.arg(output);
    let done = spawn(command, ffmpeg)?.wait_with_output()?;
    if !done.status.success() {
        bail!("ffmpeg failed putting the sound in ({}): {}", done.status, String::from_utf8_lossy(&done.stderr).trim());
    }
    Ok(())
}

/// How `scene` bakes in a video: `choice` (the export window's or the command line's) or, with
/// none, the Lighting card's On export. `None` renders live; a scene without a Lighting card
/// always renders live (there is nothing to bake).
pub fn bake_for(scene: &Scene, kit: &Kit, choice: Option<bool>) -> Option<bpe_gfx::bake::Bake> {
    let card = bpe_kit::modules::lighting::lighting_of(scene, &kit.registry)?;
    let wanted = choice.unwrap_or(card.on_export == bpe_kit::modules::lighting::OnExport::Bake);
    wanted.then(|| {
        let mut forced = card.clone();
        forced.on_export = bpe_kit::modules::lighting::OnExport::Bake;
        forced.bake().expect("asked to bake")
    })
}

/// Builds a scene's frames in order from one run of its physics, as the export draws them: a
/// frame at a moment, or a baked frame's samples (lighting part 3), each at its own moment
/// within the shutter.
pub struct FrameMaker<'a> {
    scene: &'a Scene,
    kit: &'a Kit,
    resolved: bpe_core::Resolved,
    world: World,
    log: Log,
    follow: Option<std::sync::Arc<dyn bpe_runtime::SoundAt>>,
    light: Option<std::sync::Arc<dyn bpe_runtime::LightAt>>,
}

impl<'a> FrameMaker<'a> {
    pub fn new(scene: &'a Scene, kit: &'a Kit) -> Self {
        let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
        let world = World::build(scene, &resolved, &kit.sims);
        // Settings that follow sound read it from the physics, as the preview does.
        let follow: Option<std::sync::Arc<dyn bpe_runtime::SoundAt>> = bpe_sound::follows_sound(scene).then(|| {
            std::sync::Arc::new(bpe_sound::ControlSampler::new(scene, &resolved))
                as std::sync::Arc<dyn bpe_runtime::SoundAt>
        });
        // And settings that sense light read the light probe, as the preview does.
        let light =
            bpe_kit::sensing::uses_light(scene).then(|| bpe_kit::sensing::probe(scene, &resolved, follow.clone()));
        FrameMaker { scene, kit, resolved, world, log: Log::default(), follow, light }
    }

    /// The frame at `time` (never earlier than the last asked for: the physics runs forward).
    pub fn frame(&mut self, time: f64) -> Frame {
        self.world.advance_to(time, &self.kit.sims, &mut self.log);
        self.build(time)
    }

    fn build(&self, time: f64) -> Frame {
        let mut frame = Frame::default();
        build_frame_sensing(
            self.scene,
            &self.resolved,
            &self.world,
            &self.log,
            time,
            &self.kit.draws,
            self.follow.as_deref(),
            self.light.as_deref(),
            &mut frame,
        );
        frame.cull();
        frame
    }

    /// The frame at `time`, or with `bake` its samples, oldest first (the last at `time`).
    pub fn samples(&mut self, time: f64, fps: u32, bake: Option<&bpe_gfx::bake::Bake>) -> Vec<Frame> {
        let Some(bake) = bake else { return vec![self.frame(time)] };
        let rate = self.world.steps_per_second;
        let mut frames = Vec::with_capacity(bake.samples.max(1) as usize);
        for moment in bpe_gfx::bake::moments(time, fps, rate, bake) {
            // A moment between two steps draws the later step with the balls moved back.
            let step_time = moment.step as f64 / rate as f64;
            self.world.advance_to(step_time.max(self.world.time()), &self.kit.sims, &mut self.log);
            let mut frame = match moment.between {
                Some(a) if self.world.step == moment.step => {
                    let between = self.world.set_between(a);
                    let frame = self.build(moment.time);
                    self.world.restore_between(between);
                    frame
                }
                _ => self.build(moment.time),
            };
            frame.bake = Some(bpe_gfx::bake::BakeSample { index: frames.len() as u32, count: bake.samples.max(1) });
            frames.push(frame);
        }
        frames
    }
}

/// Draws the frame at `time` as the export would (`bake` as in [`ExportOptions::bake`]), at
/// `scale` of the video's size: its RGBA pixels, its size and how long the frame itself took.
/// Effects that remember (and glow in the dark) need the frames before: as many seconds of them
/// as the scene remembers are drawn first, unbaked but at the bake's quality, and not timed.
/// Setting `stop` gives up between frames.
#[allow(clippy::too_many_arguments)]
pub fn final_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    time: f64,
    scale: f32,
    bake: Option<bool>,
    stop: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<(Vec<u8>, (u32, u32), f32)> {
    // Huge swarms step on the card the picture is drawn with.
    let _card = bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(device, queue)));
    let (_, video) = video_of(scene, kit)?;
    let (width, height) = output_size(video.width, video.height, scale);
    let fps = video.fps.max(1);
    let mut renderer = Renderer::new(device, width, height);
    let mut maker = FrameMaker::new(scene, kit);
    let bake = bake_for(scene, kit, bake);
    let stopped = || stop.load(std::sync::atomic::Ordering::Relaxed);
    let last = (time * fps as f64).round() as u32;
    let earliest = ((time - bpe_gfx::effects::MAX_MEMORY as f64).max(0.0) * fps as f64).ceil() as u32;
    if earliest < last {
        // How long the scene remembers, read at the first frame it could need.
        let mut first = maker.frame(earliest as f64 / fps as f64);
        let memory = first.memory() as f64;
        if memory > 0.0 {
            let from = (((time - memory).max(0.0) * fps as f64).ceil() as u32).max(earliest);
            for index in from..last {
                if stopped() {
                    bail!("stopped");
                }
                let mut frame =
                    if index == earliest { std::mem::take(&mut first) } else { maker.frame(index as f64 / fps as f64) };
                if bake.is_some()
                    && let Some(light) = frame.light.as_mut()
                {
                    // The bake's quality, so the lighting keeps what it remembers into the bake.
                    light.settings.more[2] = 4.0;
                }
                renderer.render(device, queue, &frame);
            }
        }
    }
    if stopped() {
        bail!("stopped");
    }
    let started = std::time::Instant::now();
    let samples = maker.samples(last as f64 / fps as f64, fps, bake.as_ref());
    renderer.render_samples(device, queue, &samples);
    let mut readback = Readback::new(device, width, height, 1);
    let mut pixels = readback.push(device, queue, &renderer)?;
    if pixels.is_none() {
        pixels = readback.finish(device)?.pop();
    }
    let pixels = pixels.context("the frame did not come back from the graphics card")?;
    Ok((pixels, (width, height), started.elapsed().as_secs_f32()))
}

/// Looks for ffmpeg on PATH, next to this program, and where common Windows installers put
/// it (winget, Chocolatey, Scoop, C:\ffmpeg). A program started from Explorer may not see a
/// PATH change from a recent install, so the install folders are checked directly too.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut folders: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        folders.push(dir.join("ffmpeg").join("bin"));
        folders.push(dir.clone());
    }
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if let Some(local) = env("LOCALAPPDATA") {
        folders.push(local.join("Microsoft").join("WinGet").join("Links"));
        // winget unpacks to Packages\Gyan.FFmpeg_<source>\ffmpeg-<version>-full_build\bin.
        let packages = local.join("Microsoft").join("WinGet").join("Packages");
        for package in read_dir(&packages).filter(|p| file_name(p).to_ascii_lowercase().contains("ffmpeg")) {
            folders.extend(read_dir(&package).map(|build| build.join("bin")));
        }
    }
    if let Some(home) = env("USERPROFILE") {
        folders.push(home.join("scoop").join("shims"));
    }
    if let Some(data) = env("ProgramData") {
        folders.push(data.join("chocolatey").join("bin"));
    }
    for root in [env("ProgramFiles"), Some(PathBuf::from("C:\\"))].into_iter().flatten() {
        folders.push(root.join("ffmpeg").join("bin"));
    }
    folders.push("/opt/homebrew/bin".into());
    folders.push("/usr/local/bin".into());
    folders.into_iter().map(|dir| dir.join(exe)).find(|path| path.is_file())
}

fn read_dir(dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path())
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Output size for a scale factor, rounded to even numbers as H.264 requires.
pub fn output_size(width: u32, height: u32, scale: f32) -> (u32, u32) {
    let even = |v: u32| ((v as f32 * scale.clamp(0.05, 4.0)).round() as u32 / 2 * 2).max(2);
    (even(width), even(height))
}

fn spawn(mut command: Command, ffmpeg: &Path) -> anyhow::Result<Child> {
    command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // Keep ffmpeg from flashing a console window when started from the editor.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn().with_context(|| {
        format!("could not start ffmpeg ({}). Install ffmpeg and make sure it is on your PATH", ffmpeg.display())
    })
}

/// Reads ffmpeg's error output on a thread, so a chatty ffmpeg cannot block the pipe.
fn collect_stderr(child: &mut Child) -> std::thread::JoinHandle<String> {
    let mut stderr = child.stderr.take();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stderr) = stderr.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text.trim().to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_graphics_card_failure_is_tried_again_on_the_cpu() {
        assert!(!retry_on_cpu(Encoder::Nvidia, &anyhow::anyhow!(CANCELLED)));
        assert!(retry_on_cpu(Encoder::Nvidia, &anyhow::anyhow!("ffmpeg failed")));
        assert!(!retry_on_cpu(Encoder::Cpu, &anyhow::anyhow!("ffmpeg failed")));
    }
}
