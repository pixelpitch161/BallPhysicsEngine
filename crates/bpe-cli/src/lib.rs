//! Video export for the modular studio.
//!
//! The scene is simulated once for its event log, the soundtrack is mixed from that log and
//! written to a temporary WAV. Then every frame is drawn offscreen and piped as raw RGBA into
//! ffmpeg, which encodes H.264 video and AAC audio into one mp4. Export does not run in real
//! time: a slow machine just takes longer, and the result is identical.

pub mod batch;

use anyhow::{Context, bail};
use bpe_core::{Scene, Settings};
use bpe_gfx::{Frame, Readback, Renderer, build_frame_at, wgpu};
use bpe_kit::{Kit, SceneSettings, flow_source, mix_scene};
use bpe_runtime::{Log, World};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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
/// encoder fails, the export starts again on the CPU.
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
        Err(error) if encoder != Encoder::Cpu && !error.to_string().contains(CANCELLED) => {
            export_with(device, queue, scene, kit, options, Encoder::Cpu, &mut progress)
                .with_context(|| format!("the {} failed first ({error:#})", encoder.name()))?;
            Ok(Encoder::Cpu)
        }
        result => result.map(|()| encoder),
    }
}

const CANCELLED: &str = "export cancelled";
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
    let (resolved, video) = video_of(scene, kit)?;
    let (width, height) = output_size(video.width, video.height, options.scale);
    let fps = video.fps.max(1);
    let mut output = options.output.clone();
    if output.extension().is_none() {
        output.set_extension("mp4");
    }
    let frames_in_video = frame_count(&video);
    let total = options.max_frames.map_or(frames_in_video, |m| m.min(frames_in_video)).max(1);

    // The whole run first, for the sounds: every hit is known before the first frame is drawn.
    let mut run = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    run.advance_to(video.duration as f64, &kit.sims, &mut log);
    let wav = TempFile::new("wav");
    let track = mix_scene(scene, &resolved, &log, kit);
    for problem in &track.problems {
        eprintln!("sound: {problem}");
    }
    if let Some(report) = &track.report {
        eprintln!("Loudness {:.1} LUFS, true peak {:.1} dBTP", report.integrated, report.true_peak);
    }
    track.write_wav(&wav.0)?;
    // Beat sync by time warp: each frame shows the moment of the run the warp puts there.
    let warp = bpe_kit::warp_for(scene, &resolved, &log);

    let mut ffmpeg = Command::new(&options.ffmpeg);
    ffmpeg.args(["-hide_banner", "-loglevel", "error", "-y"]);
    ffmpeg.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{width}x{height}"), "-r", &fps.to_string()]);
    ffmpeg.args(["-i", "-"]);
    ffmpeg.arg("-i").arg(&wav.0);
    ffmpeg.args(["-map", "0:v", "-map", "1:a"]);
    ffmpeg.args(["-vf", "scale=out_color_matrix=bt709:out_range=tv"]);
    ffmpeg.args(encoder.args(options.crf, &options.preset));
    ffmpeg.args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]);
    ffmpeg.args(["-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-movflags", "+faststart", "-shortest", "-f", "mp4"]);
    ffmpeg.arg(&output);
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
    // Drawn from a second run that keeps pace with the frames, exactly as the preview plays.
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    let mut frame = Frame::default();
    // Liquids and swarms on the graphics card step with the frames; without a time budget
    // every frame waits for them, so the clip never skips.
    let source = flow_source(scene, &resolved, &kit.sims, None);
    let mut failed = None;
    for index in 0..total {
        let video_time = index as f64 / fps as f64;
        let time = warp.sim_time(video_time);
        world.advance_to(time, &kit.sims, &mut log);
        build_frame_at(scene, &resolved, &world, &log, time, video_time, &kit.draws, &mut frame);
        frame.cull();
        while !renderer.render_with_flow(device, queue, &frame, &source).caught_up {}
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
        if !progress(index + 1, total) {
            drop(frames);
            let _ = stop(&mut child, writer, true);
            let _ = std::fs::remove_file(&output);
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
        let _ = std::fs::remove_file(&output);
        return Err(error);
    }
    if let Err(error) = writer.join().unwrap_or(Ok(())) {
        let _ = child.wait();
        let _ = std::fs::remove_file(&output);
        bail!("ffmpeg stopped accepting frames ({error}): {}", stderr.join().unwrap_or_default());
    }
    let status = child.wait()?;
    if !status.success() {
        let _ = std::fs::remove_file(&output);
        bail!("ffmpeg failed ({status}): {}", stderr.join().unwrap_or_default());
    }
    Ok(())
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

/// A file in the temp folder that is deleted when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(extension: &str) -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        Self(std::env::temp_dir().join(format!("bpe-next-{}-{nanos}.{extension}", std::process::id())))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
