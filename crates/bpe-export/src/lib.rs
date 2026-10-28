//! Video export.
//!
//! The soundtrack is mixed first and written to a temporary WAV. Then every frame is
//! rendered offscreen at full quality and piped as raw RGBA into ffmpeg, which encodes
//! H.264 video and AAC audio into one mp4. Export does not run in real time: a slow
//! machine just takes longer, and the result is identical.

use anyhow::{Context, bail};
use bpe_render::{FrameData, Renderer, wgpu};
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
}

impl ExportOptions {
    pub fn new(output: impl Into<PathBuf>) -> Self {
        Self {
            output: output.into(),
            ffmpeg: "ffmpeg".into(),
            crf: 18,
            preset: "medium".into(),
            scale: 1.0,
            max_frames: None,
            base_dir: None,
        }
    }
}

/// Renders `scene` to an mp4. `progress` is called after every frame with (done, total);
/// return `false` from it to cancel, which deletes the partial file.
pub fn export_video(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    options: &ExportOptions,
    mut progress: impl FnMut(u32, u32) -> bool,
) -> anyhow::Result<()> {
    let (width, height) = output_size(scene, options.scale);
    let fps = scene.video.fps.max(1);
    let mut output = options.output.clone();
    if output.extension().is_none() {
        output.set_extension("mp4");
    }
    let total = options.max_frames.map_or(scene.frame_count(), |m| m.min(scene.frame_count()));

    let hits = bpe_sim::collect_wall_hits(scene);
    let soundtrack = bpe_audio::render_soundtrack(scene, &hits, options.base_dir.as_deref())?;
    let wav = TempFile::new("wav");
    soundtrack.write_wav(&wav.0)?;

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
    ffmpeg.args(["-c:v", "libx264", "-preset", &options.preset, "-crf", &options.crf.to_string()]);
    ffmpeg.args(["-pix_fmt", "yuv420p", "-profile:v", "high"]);
    ffmpeg.args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]);
    ffmpeg.args(["-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-movflags", "+faststart", "-shortest", "-f", "mp4"]);
    ffmpeg.arg(&output);
    let mut child = spawn(ffmpeg, &options.ffmpeg)?;
    let mut stdin = child.stdin.take().context("ffmpeg stdin")?;
    let stderr = collect_stderr(&mut child);

    let mut renderer = Renderer::new(device, width, height);
    let mut playback = Playback::new(scene);
    let mut frame = FrameData::default();
    let pixel = renderer.pixel_size(scene);
    for index in 0..total {
        let time = index as f64 / fps as f64;
        playback.seek(time);
        frame.fill(scene, &playback, pixel);
        renderer.render(device, queue, scene, time, &frame);
        let pixels = renderer.read_output(device, queue)?;
        if let Err(error) = stdin.write_all(&pixels) {
            drop(stdin);
            let _ = child.wait();
            bail!("ffmpeg stopped accepting frames ({error}): {}", stderr.join().unwrap_or_default());
        }
        if !progress(index + 1, total) {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&output);
            bail!("export cancelled");
        }
    }
    drop(stdin);
    let status = child.wait()?;
    if !status.success() {
        bail!("ffmpeg failed ({status}): {}", stderr.join().unwrap_or_default());
    }
    Ok(())
}

/// Renders one frame at `time` and saves it as a PNG (through ffmpeg).
pub fn export_still(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    time: f64,
    scale: f32,
    ffmpeg: &Path,
    output: &Path,
) -> anyhow::Result<()> {
    let (width, height) = output_size(scene, scale);
    let mut renderer = Renderer::new(device, width, height);
    let mut playback = Playback::new(scene);
    playback.seek(time);
    let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
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
