//! Drawing and exporting a video (`docs/superpowers/specs/2026-11-24-video-arrangement-design.md`,
//! "Export"): each frame draws the clips on screen (one, or two in a transition) with their own
//! scenes' renderers, puts them together with [`bpe_gfx::compose`], and reads the result back.
//! A clip alone at the video's size is read back from its renderer, so those frames are the same
//! bytes as the scene's own export.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::bail;
use bpe_core::Scene;
use bpe_core::video::{Direction, TransitionKind, Video};
use bpe_gfx::compose::{Compose, Look, params};
use bpe_gfx::yuv::PixelLayout;
use bpe_gfx::{Readback, Renderer, wgpu};
use bpe_kit::Kit;
use bpe_kit::video::{CoverFit, clip_scene, cover, moment, scene_length, within};

use crate::{
    Encoder, ExportOptions, FRAMES_IN_FLIGHT, FrameMaker, Parts, Picture, bake_for, encode_frames, export_parts,
    output_size, resolve_encoder, retry_on_cpu,
};

/// Why a video can't be exported.
pub const NO_CLIPS: &str = "This video has no clips";
pub const NOT_FOR_VIDEOS: &str = "Stems, MIDI and baking aren't made for a video yet";

/// A clip being drawn: its scene's run, its renderer at the cover size, and the next scene frame.
struct ClipDraw<'a> {
    maker: FrameMaker<'a>,
    renderer: Renderer,
    bake: Option<bpe_gfx::bake::Bake>,
    fit: CoverFit,
    next: u32,
}

impl<'a> ClipDraw<'a> {
    /// Made when the clip first comes on screen; runs its scene from 0 up to `first`, drawing
    /// every frame as the scene's own export does (Effects remember frames) but reading none back.
    #[allow(clippy::too_many_arguments)]
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &'a Scene,
        kit: &'a Kit,
        out: (u32, u32),
        fps: u32,
        first: u32,
    ) -> Self {
        let s = bpe_kit::video::scene_settings(scene, &kit.registry);
        // Drawn at its own shape, at the size that covers the video (at the export's scale).
        // No bigger than the card can hold.
        let fit = within(cover((s.width, s.height), out), device.limits().max_texture_dimension_2d);
        let mut draw = ClipDraw {
            maker: FrameMaker::new(scene, kit),
            renderer: Renderer::new(device, fit.draw.0, fit.draw.1),
            bake: bake_for(scene, kit, None),
            fit,
            next: 0,
        };
        while draw.next < first {
            draw.draw(device, queue, fps);
        }
        draw
    }

    /// Draws scene frame `self.next` and moves on.
    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, fps: u32) {
        let time = self.next as f64 / fps as f64;
        let samples = self.maker.samples(time, fps, self.bake.as_ref());
        self.renderer.render_samples(device, queue, &samples);
        self.next += 1;
    }

    /// Draws up to and including scene frame `frame`.
    fn draw_to(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, fps: u32, frame: u32) {
        while self.next <= frame {
            self.draw(device, queue, fps);
        }
    }
}

/// The compose look of a transition (`None`: a cut, the new picture alone).
fn look_of(kind: TransitionKind) -> Option<Look> {
    let rgb = |c: [u8; 3]| c.map(|v| f32::from(v) / 255.0);
    let dir = |d: Direction| match d {
        Direction::Up => 0,
        Direction::Down => 1,
        Direction::Left => 2,
        Direction::Right => 3,
    };
    match kind {
        TransitionKind::Cut => None,
        TransitionKind::Crossfade => Some(Look::Crossfade),
        TransitionKind::FadeThrough { colour } => Some(Look::FadeThrough(rgb(colour))),
        TransitionKind::Flash { colour } => Some(Look::Flash(rgb(colour))),
        TransitionKind::Wipe { direction, softness } => Some(Look::Wipe { direction: dir(direction), softness }),
        TransitionKind::Push { direction } => Some(Look::Push { direction: dir(direction) }),
        TransitionKind::ZoomThrough => Some(Look::ZoomThrough),
    }
}

/// The video with its clips inside their limits (as opening it does).
pub fn tidied(video: &Video, kit: &Kit) -> Video {
    let mut video = video.clone();
    video.tidy(|s| scene_length(s, &kit.registry));
    video
}

/// The video's frames at `scale` of its size, in `layout`, in order: `each(index, pixels)`, which
/// says `false` to stop (then this gives back `false`). At most `max_frames`.
#[allow(clippy::too_many_arguments)]
pub fn arranged_frames(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    video: &Video,
    kit: &Kit,
    scale: f32,
    layout: PixelLayout,
    max_frames: Option<u32>,
    mut each: impl FnMut(u32, Vec<u8>) -> bool,
) -> anyhow::Result<bool> {
    // Huge swarms step on the card the video is drawn with; liquids use their Export detail.
    let _card = bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(device, queue)));
    let _export = bpe_kit::things::liquid::exporting_for_now();
    let video = tidied(video, kit);
    let s = &video.settings;
    let fps = s.fps.max(1);
    let out = output_size(s.width, s.height, scale);
    let total = max_frames.map_or(video.frame_count(), |m| m.min(video.frame_count()));
    // Each clip's scene copy, made up front so the draws can borrow them.
    let scenes: Vec<Scene> = video.clips.iter().map(|c| clip_scene(&video, c, &kit.registry)).collect();
    let mut draws: BTreeMap<usize, ClipDraw<'_>> = BTreeMap::new();
    let mut compose: Option<Compose> = None;
    let mut readback = Readback::with_layout(device, out.0, out.1, FRAMES_IN_FLIGHT, layout);
    let mut handed = 0u32;
    let mut hand = |pixels: Option<Vec<u8>>, handed: &mut u32| -> bool {
        let Some(pixels) = pixels else { return true };
        let go_on = each(*handed, pixels);
        *handed += 1;
        go_on
    };
    for frame in 0..total {
        let m = moment(&video, frame);
        // A cut over an overlap shows the new clip alone.
        let look = look_of(m.kind);
        let old = m.old.filter(|_| look.is_some());
        for on in [old, m.new].into_iter().flatten() {
            let draw = draws
                .entry(on.clip)
                .or_insert_with(|| ClipDraw::new(device, queue, &scenes[on.clip], kit, out, fps, on.scene_frame));
            draw.draw_to(device, queue, fps, on.scene_frame);
        }
        let ready = match (old, m.new) {
            (None, Some(new)) if draws[&new.clip].fit.draw == out => {
                readback.push(device, queue, &draws[&new.clip].renderer)?
            }
            (old, new) => {
                let compose = compose.get_or_insert_with(|| Compose::new(device, queue, out.0, out.1));
                let crop =
                    |o: Option<bpe_kit::video::OnScreen>| o.map_or([0.0, 0.0, 1.0, 1.0], |o| draws[&o.clip].fit.crop);
                let view = |o: Option<bpe_kit::video::OnScreen>| o.map(|o| draws[&o.clip].renderer.output_view());
                let p = params(if old.is_some() { look } else { None }, m.progress, crop(old), crop(new));
                compose.draw(device, queue, view(old), view(new), &p);
                readback.push_texture(device, queue, compose.output_texture(), compose.output_view())?
            }
        };
        if !hand(ready, &mut handed) {
            return Ok(false);
        }
        // Clips that have ended are dropped, so at most two scenes are in memory at once.
        let next = i64::from(frame) + 1;
        draws.retain(|&i, _| video.frames(video.clips[i].end()) > next);
    }
    for pixels in readback.finish(device)? {
        if !hand(Some(pixels), &mut handed) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Exports `video` to an mp4, as [`crate::export_video`] does a scene: the same encoders and CPU
/// retry, the sound ([`bpe_sound::render_video_sound`]) beside the frames, the same bar, cancel
/// and clean-up. Stems, MIDI and baking are refused for a video, and so is a video with no clips.
pub fn export_arranged(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    video: &Video,
    kit: &Kit,
    options: &ExportOptions,
    mut progress: impl FnMut(u32, u32) -> bool,
) -> anyhow::Result<Encoder> {
    if options.stems || options.midi || options.bake.is_some() {
        bail!(NOT_FOR_VIDEOS);
    }
    let video = tidied(video, kit);
    if video.clips.is_empty() {
        bail!(NO_CLIPS);
    }
    let encoder = resolve_encoder(options.encoder, &options.ffmpeg);
    match export_arranged_with(device, queue, &video, kit, options, encoder, &mut progress) {
        Err(error) if retry_on_cpu(encoder, &error) => {
            export_arranged_with(device, queue, &video, kit, options, Encoder::Cpu, &mut progress)
                .map_err(|e| e.context(format!("the {} failed first ({error:#})", encoder.name())))?;
            Ok(Encoder::Cpu)
        }
        result => result.map(|()| encoder),
    }
}

fn export_arranged_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    video: &Video,
    kit: &Kit,
    options: &ExportOptions,
    encoder: Encoder,
    progress: &mut impl FnMut(u32, u32) -> bool,
) -> anyhow::Result<()> {
    let _card = bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(device, queue)));
    let _export = bpe_kit::things::liquid::exporting_for_now();
    let s = &video.settings;
    let (width, height) = output_size(s.width, s.height, options.scale);
    let fps = s.fps.max(1);
    let mut output = options.output.clone();
    if output.extension().is_none() {
        output.set_extension("mp4");
    }
    let total = options.max_frames.map_or(video.frame_count(), |m| m.min(video.frame_count())).max(1);
    let picture = Picture { width, height, fps, total, encoder };
    let has_sound = bpe_sound::video_has_sound(video, kit);
    let written: Vec<PathBuf> = vec![output.clone()];
    let parts = Parts { output: &output, total, seconds: total as f64 / fps as f64, has_sound, written: &written };
    export_parts(
        options,
        &parts,
        |sound, heard| bpe_sound::render_video_sound(video, kit, sound, parts.seconds, heard),
        |into, silence, done| {
            encode_frames(options, &picture, into, silence, |layout, send| {
                let mut cancelled = false;
                let finished =
                    arranged_frames(device, queue, video, kit, options.scale, layout, Some(total), |i, px| {
                        if !send(px) {
                            return false;
                        }
                        let go_on = done(i + 1);
                        cancelled |= !go_on;
                        go_on
                    })?;
                Ok(finished || !cancelled)
            })
        },
        progress,
    )
}
