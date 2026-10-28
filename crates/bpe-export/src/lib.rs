//! Video export.
//!
//! The soundtrack is mixed first and written to a temporary WAV. Then every frame is
//! rendered offscreen at full quality and piped as raw RGBA into ffmpeg, which encodes
//! H.264 video and AAC audio into one mp4. Export does not run in real time: a slow
//! machine just takes longer, and the result is identical.

pub mod batch;
pub mod profile;
pub mod score;

use anyhow::{Context, bail};
use bpe_render::{EffectLibrary, FrameData, Readback, Renderer, wgpu};
use bpe_scene::Scene;
use bpe_sim::Playback;
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
    /// Folder that relative paths in the scene (such as samples) are resolved against.
    pub base_dir: Option<PathBuf>,
    /// Where the scene's shader effects come from. Built-ins only unless folders are set.
    pub effects: EffectLibrary,
    /// Which video encoder to use.
    pub encoder: Encoder,
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
        let args: Vec<&str> = match self {
            Encoder::Auto | Encoder::Cpu => {
                vec!["-c:v", "libx264", "-preset", preset, "-crf", &crf, "-pix_fmt", "yuv420p", "-profile:v", "high"]
            }
            Encoder::Nvidia => {
                let speed = if slow {
                    "p6"
                } else if fast {
                    "p2"
                } else {
                    "p4"
                };
                vec![
                    "-c:v",
                    "h264_nvenc",
                    "-preset",
                    speed,
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
                ]
            }
            Encoder::Amd => {
                let speed = if slow {
                    "quality"
                } else if fast {
                    "speed"
                } else {
                    "balanced"
                };
                vec![
                    "-c:v",
                    "h264_amf",
                    "-quality",
                    speed,
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
                ]
            }
            Encoder::Intel => {
                let speed = if slow {
                    "slower"
                } else if fast {
                    "veryfast"
                } else {
                    "medium"
                };
                vec![
                    "-c:v",
                    "h264_qsv",
                    "-preset",
                    speed,
                    "-global_quality",
                    &crf,
                    "-pix_fmt",
                    "nv12",
                    "-profile:v",
                    "high",
                ]
            }
        };
        args.into_iter().map(String::from).collect()
    }
}

/// The graphics card encoders that work with this ffmpeg on this computer, fastest first.
/// Each is tried on a tiny clip once, and the answer is remembered.
pub fn gpu_encoders(ffmpeg: &Path) -> Vec<Encoder> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static FOUND: Mutex<Option<HashMap<PathBuf, Vec<Encoder>>>> = Mutex::new(None);
    if let Some(found) = FOUND.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|f| f.get(ffmpeg)) {
        return found.clone();
    }
    let working: Vec<Encoder> = Encoder::GPU.into_iter().filter(|&encoder| encoder_works(ffmpeg, encoder)).collect();
    FOUND
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(ffmpeg.to_path_buf(), working.clone());
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

impl ExportOptions {
    pub fn new(output: impl Into<PathBuf>) -> Self {
        Self {
            output: output.into(),
            ffmpeg: find_ffmpeg().unwrap_or_else(|| "ffmpeg".into()),
            crf: 18,
            preset: "medium".into(),
            scale: 1.0,
            max_frames: None,
            base_dir: None,
            effects: EffectLibrary::new(),
            encoder: Encoder::Auto,
        }
    }
}

/// Renders `scene` to an mp4. `progress` is called after every frame with (done, total);
/// return `false` from it to cancel, which deletes the partial file. If a graphics card
/// encoder fails, the export starts again on the CPU.
pub fn export_video(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    options: &ExportOptions,
    progress: impl FnMut(u32, u32) -> bool,
) -> anyhow::Result<Encoder> {
    export_video_with(device, queue, scene, options, None, progress, |_, _| {})
}

/// [`export_video`] with a soundtrack that was already mixed (so a batch can mix the next video
/// while this one encodes), and `on_frame` called with each finished frame's index and RGBA
/// pixels, in order, for sampling frames to score.
pub fn export_video_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    options: &ExportOptions,
    sound: Option<&bpe_audio::SceneSound>,
    mut progress: impl FnMut(u32, u32) -> bool,
    mut on_frame: impl FnMut(u32, &[u8]),
) -> anyhow::Result<Encoder> {
    let encoder = resolve_encoder(options.encoder, &options.ffmpeg);
    match export_with(device, queue, scene, options, encoder, sound, &mut progress, &mut on_frame) {
        Err(error) if encoder != Encoder::Cpu && !error.to_string().contains(CANCELLED) => {
            export_with(device, queue, scene, options, Encoder::Cpu, sound, &mut progress, &mut on_frame)
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

#[allow(clippy::too_many_arguments)]
fn export_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    options: &ExportOptions,
    encoder: Encoder,
    sound: Option<&bpe_audio::SceneSound>,
    progress: &mut impl FnMut(u32, u32) -> bool,
    on_frame: &mut impl FnMut(u32, &[u8]),
) -> anyhow::Result<()> {
    let (width, height) = output_size(scene, options.scale);
    let fps = scene.video.fps.max(1);
    let mut output = options.output.clone();
    if output.extension().is_none() {
        output.set_extension("mp4");
    }
    let total = options.max_frames.map_or(scene.frame_count(), |m| m.min(scene.frame_count()));

    // Music workstream: the song, beat sync and its clock, worked out with the soundtrack. A
    // batch passes it in, mixed while the video before rendered.
    let rendered;
    let sound = match sound {
        Some(sound) => sound,
        None => {
            rendered = bpe_audio::render_scene(scene, options.base_dir.as_deref())?;
            &rendered
        }
    };
    if let Some(error) = &sound.song_error {
        bail!("the song can't be used: {error}");
    }
    let wav = TempFile::new("wav");
    sound.mix.track.write_wav(&wav.0)?;

    let mut ffmpeg = Command::new(&options.ffmpeg);
    ffmpeg.args(["-hide_banner", "-loglevel", "error", "-y"]);
    ffmpeg.args([
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{width}x{height}"),
        "-r",
        &fps.to_string(),
        "-i",
        "-",
    ]);
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
    renderer.set_effect_library(options.effects.clone());
    renderer.set_font_folder(options.base_dir.clone());
    let mut readback = Readback::new(device, width, height, FRAMES_IN_FLIGHT);
    let mut playback = Playback::with_inputs(scene, &sound.inputs);
    // Effects and modulators that pulse with the music follow the finished soundtrack.
    let mix = bpe_render::effects::mix_features(&sound.mix.track.samples, bpe_audio::SAMPLE_RATE, 2);
    let mut frame = FrameData { mix: Some(mix), ..FrameData::default() };
    let pixel = renderer.pixel_size(scene);
    let mut failed = None;
    // Frames handed to ffmpeg so far; read-back runs a few frames behind drawing.
    let mut sent = 0;
    for index in 0..total {
        let time = index as f64 / fps as f64;
        playback.seek(sound.sim_time(time));
        frame.fill(scene, &playback, pixel);
        renderer.render(device, queue, scene, time, &frame);
        let ready = match readback.push(device, queue, &renderer) {
            Ok(ready) => ready,
            Err(error) => {
                failed = Some(error);
                break;
            }
        };
        if let Some(pixels) = ready {
            on_frame(sent, &pixels);
            sent += 1;
            if frames.send(pixels).is_err() {
                break;
            }
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
                    on_frame(sent, &pixels);
                    sent += 1;
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

/// Renders one frame at `time` and saves it as a PNG (through ffmpeg).
#[allow(clippy::too_many_arguments)]
pub fn export_still(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    effects: &EffectLibrary,
    time: f64,
    scale: f32,
    ffmpeg: &Path,
    output: &Path,
    base_dir: Option<&Path>,
) -> anyhow::Result<()> {
    let (width, height) = output_size(scene, scale);
    let mut renderer = Renderer::new(device, width, height);
    renderer.set_effect_library(effects.clone());
    // Music workstream: a scene that follows a beat is simulated with it, on beat sync's clock.
    let clock = bpe_audio::scene_clock(scene, base_dir);
    if let Some(error) = &clock.song_error {
        bail!("the song can't be used: {error}");
    }
    let sim_time = |t: f64| clock.sim_time(t);
    let mut playback = Playback::with_inputs(scene, &clock.inputs);
    let mut frame = FrameData::default();
    let pixel = renderer.pixel_size(scene);
    // Effects like Echo build up over many frames, so play the moments before `time` first.
    let uses_feedback = scene.effects.iter().any(|e| e.enabled && effects.get(&e.shader).is_some_and(|i| i.uses_prev));
    let fps = scene.video.fps.max(1) as f64;
    let first = if uses_feedback { ((time - 3.0).max(0.0) * fps).round() as u64 } else { (time * fps).round() as u64 };
    let last = (time * fps).round() as u64;
    for index in first..last {
        let t = index as f64 / fps;
        playback.seek(sim_time(t));
        frame.fill(scene, &playback, pixel);
        renderer.render(device, queue, scene, t, &frame);
    }
    playback.seek(sim_time(time));
    frame.fill(scene, &playback, pixel);
    renderer.render(device, queue, scene, time, &frame);
    let pixels = renderer.read_output(device, queue)?;

    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-y"]);
    command.args([
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
        &format!("{width}x{height}"),
        "-i",
        "-",
        "-frames:v",
        "1",
    ]);
    command.arg(output);
    let mut child = spawn(command, ffmpeg)?;
    let stderr = collect_stderr(&mut child);
    child.stdin.take().context("ffmpeg stdin")?.write_all(&pixels)?;
    if !child.wait()?.success() {
        bail!("ffmpeg failed: {}", stderr.join().unwrap_or_default());
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
pub fn output_size(scene: &Scene, scale: f32) -> (u32, u32) {
    let even = |v: u32| ((v as f32 * scale.clamp(0.05, 4.0)).round() as u32 / 2 * 2).max(2);
    (even(scene.video.width), even(scene.video.height))
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
        Self(std::env::temp_dir().join(format!("bpe-{}-{nanos}.{extension}", std::process::id())))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
