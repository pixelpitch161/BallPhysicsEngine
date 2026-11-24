//! Drawing and exporting a video (`docs/superpowers/specs/2026-11-24-video-arrangement-design.md`,
//! "Export"): each frame draws the clips on screen (one, or two in a transition) with their own
//! scenes' renderers, puts them together with [`bpe_gfx::compose`], and reads the result back.
//! A clip alone at the video's size is read back from its renderer, so those frames are the same
//! bytes as the scene's own export. The On top lane's clips (part 4) are drawn by their own
//! renderers at their frame's size and laid over the main picture ([`Compose::overlay`]).

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::bail;
use bpe_core::Scene;
use bpe_core::video::{Direction, OnTopClip, TransitionKind, Video};
use bpe_gfx::compose::{Compose, Look, OverlayParams, overlay_params, params};
use bpe_gfx::yuv::PixelLayout;
use bpe_gfx::{Readback, Renderer, wgpu};
use bpe_kit::Kit;
use bpe_kit::video::{
    CoverFit, OnScreen, TopOnScreen, clip_scene, cover, moment, on_top_at, scene_length, top_rect, within,
};

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

/// Each clip's scene copy ([`clip_scene`]) on both lanes, in the clips' order.
pub struct ClipScenes {
    /// `Video::clips`'.
    pub main: Vec<Scene>,
    /// `Video::on_top`'s.
    pub tops: Vec<Scene>,
}

/// Each clip's scene copy ([`clip_scene`]), in the clips' order.
pub fn clip_scenes(video: &Video, kit: &Kit) -> ClipScenes {
    ClipScenes {
        main: video.clips.iter().map(|c| clip_scene(video, c, &kit.registry)).collect(),
        tops: video.on_top.iter().map(|t| clip_scene(video, &t.clip, &kit.registry)).collect(),
    }
}

const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// A finished picture and the part of it shown.
type Shown<'v> = (&'v wgpu::TextureView, [f32; 4]);

/// The main lane's part of a frame: one picture, two in a transition, or none in a gap. `lone` is
/// the renderer of a clip alone at the video's size, read back as it is when nothing is on top.
struct Main<'v> {
    old: Option<Shown<'v>>,
    new: Option<Shown<'v>>,
    look: Option<Look>,
    progress: f32,
    lone: Option<&'v Renderer>,
}

/// The size an On top clip's frame is drawn at in an output `out` pixels.
fn top_size(top: &OnTopClip, out: (u32, u32)) -> (u32, u32) {
    let [_, _, w, h] = top_rect(&top.place, out);
    (w, h)
}

/// The overlay of On top clip `top` (drawn with `fit`) at `alpha` in an output `out` pixels: its
/// corner and border are pixels of the video at its own size, so they scale with the output.
fn top_overlay(video: &Video, top: &OnTopClip, out: (u32, u32), fit: &CoverFit, alpha: f32) -> OverlayParams {
    let rect = top_rect(&top.place, out).map(|v| v as f32);
    let k = out.0 as f32 / video.settings.width.max(1) as f32;
    let p = &top.place;
    overlay_params(rect, fit.crop, p.corner * k, p.border * k, p.border_colour.map(|v| f32::from(v) / 255.0), alpha)
}

/// Puts a frame together and reads it back: the main picture, then each On top overlay in order.
/// A lone clip with nothing on top is read straight from its renderer (the scene's own bytes).
fn put_together(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    compose: &mut Option<Compose>,
    readback: &mut Readback,
    out: (u32, u32),
    main: Main<'_>,
    tops: &[(&wgpu::TextureView, OverlayParams)],
) -> anyhow::Result<Option<Vec<u8>>> {
    if tops.is_empty()
        && let Some(renderer) = main.lone
    {
        return readback.push(device, queue, renderer);
    }
    let compose = compose.get_or_insert_with(|| Compose::new(device, queue, out.0, out.1));
    let crop = |s: Option<Shown<'_>>| s.map_or(WHOLE, |s| s.1);
    let look = if main.old.is_some() { main.look } else { None };
    let p = params(look, main.progress, crop(main.old), crop(main.new));
    compose.draw(device, queue, main.old.map(|s| s.0), main.new.map(|s| s.0), &p);
    for (view, overlay) in tops {
        compose.overlay(device, queue, view, overlay);
    }
    readback.push_texture(device, queue, compose.output_texture(), compose.output_view())
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
    let scenes = clip_scenes(&video, kit);
    let mut draws: BTreeMap<usize, ClipDraw<'_>> = BTreeMap::new();
    let mut top_draws: BTreeMap<usize, ClipDraw<'_>> = BTreeMap::new();
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
                .or_insert_with(|| ClipDraw::new(device, queue, &scenes.main[on.clip], kit, out, fps, on.scene_frame));
            draw.draw_to(device, queue, fps, on.scene_frame);
        }
        let tops = on_top_at(&video, frame);
        for t in &tops {
            let size = top_size(&video.on_top[t.top], out);
            let draw = top_draws
                .entry(t.top)
                .or_insert_with(|| ClipDraw::new(device, queue, &scenes.tops[t.top], kit, size, fps, t.scene_frame));
            draw.draw_to(device, queue, fps, t.scene_frame);
        }
        let shown = |o: Option<OnScreen>| o.map(|o| (draws[&o.clip].renderer.output_view(), draws[&o.clip].fit.crop));
        let lone = match (old, m.new) {
            (None, Some(new)) if draws[&new.clip].fit.draw == out => Some(&draws[&new.clip].renderer),
            _ => None,
        };
        let main = Main { old: shown(old), new: shown(m.new), look, progress: m.progress, lone };
        let overlays: Vec<_> = tops
            .iter()
            .filter(|t| t.alpha > 0.0)
            .map(|t| {
                let d = &top_draws[&t.top];
                (d.renderer.output_view(), top_overlay(&video, &video.on_top[t.top], out, &d.fit, t.alpha))
            })
            .collect();
        let ready = put_together(device, queue, &mut compose, &mut readback, out, main, &overlays)?;
        if !hand(ready, &mut handed) {
            return Ok(false);
        }
        // Clips that have ended are dropped, so only the scenes on screen are in memory.
        let next = i64::from(frame) + 1;
        draws.retain(|&i, _| video.frames(video.clips[i].end()) > next);
        top_draws.retain(|&i, _| video.frames(video.on_top[i].clip.end()) > next);
    }
    for pixels in readback.finish(device)? {
        if !hand(Some(pixels), &mut handed) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The editor's still frame of a video (part 2): video frame `frame` at `scale` of the video's
/// size, RGBA, and its size. Each clip on screen runs its scene with the quick run-up the Preview
/// final frame uses ([`crate::run_up`]: the physics from 0, only the frames the scene remembers
/// drawn), not the export's exact one, then the clips are put together as the export does. A gap
/// or a frame past the end is black. Gives up with "stopped" once `stop` is set.
pub fn still_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    video: &Video,
    kit: &Kit,
    frame: u32,
    scale: f32,
    stop: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<(Vec<u8>, (u32, u32))> {
    let _card = bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(device, queue)));
    // Liquids use their Export detail, as the export and the Preview final frame do.
    let _export = bpe_kit::things::liquid::exporting_for_now();
    let video = tidied(video, kit);
    let s = &video.settings;
    let fps = s.fps.max(1);
    let out = output_size(s.width, s.height, scale);
    let m = moment(&video, frame);
    let look = look_of(m.kind);
    let old = m.old.filter(|_| look.is_some());
    let tops: Vec<TopOnScreen> = on_top_at(&video, frame).into_iter().filter(|t| t.alpha > 0.0).collect();
    // Each picture to draw: its scene copy, the size it covers and the scene frame.
    let mut wanted: Vec<(Scene, (u32, u32), u32)> = Vec::new();
    for o in [old, m.new].into_iter().flatten() {
        wanted.push((clip_scene(&video, &video.clips[o.clip], &kit.registry), out, o.scene_frame));
    }
    for t in &tops {
        let top = &video.on_top[t.top];
        wanted.push((clip_scene(&video, &top.clip, &kit.registry), top_size(top, out), t.scene_frame));
    }
    let mut drawn: Vec<(Renderer, CoverFit)> = Vec::new();
    for (scene, size, scene_frame) in &wanted {
        let ss = bpe_kit::video::scene_settings(scene, &kit.registry);
        let fit = within(cover((ss.width, ss.height), *size), device.limits().max_texture_dimension_2d);
        let mut renderer = Renderer::new(device, fit.draw.0, fit.draw.1);
        let mut maker = FrameMaker::new(scene, kit);
        crate::run_up(device, queue, &mut maker, &mut renderer, *scene_frame, fps, false, stop)?;
        let samples = maker.samples(f64::from(*scene_frame) / f64::from(fps), fps, None);
        renderer.render_samples(device, queue, &samples);
        drawn.push((renderer, fit));
    }
    let mut readback = Readback::new(device, out.0, out.1, 1);
    // Which of `drawn` is the old picture and which the new; the On top pictures follow them.
    let (old_at, new_at) = match (old.is_some(), m.new.is_some()) {
        (true, true) => (Some(0), Some(1)),
        (true, false) => (Some(0), None),
        (false, true) => (None, Some(0)),
        (false, false) => (None, None),
    };
    let mains = usize::from(old.is_some()) + usize::from(m.new.is_some());
    let shown = |i: Option<usize>| i.map(|i| (drawn[i].0.output_view(), drawn[i].1.crop));
    let lone = match new_at {
        Some(i) if old.is_none() && drawn[i].1.draw == out => Some(&drawn[i].0),
        _ => None,
    };
    let main = Main { old: shown(old_at), new: shown(new_at), look, progress: m.progress, lone };
    let overlays: Vec<_> = tops
        .iter()
        .enumerate()
        .map(|(k, t)| {
            let (renderer, fit) = &drawn[mains + k];
            (renderer.output_view(), top_overlay(&video, &video.on_top[t.top], out, fit, t.alpha))
        })
        .collect();
    let mut compose = None;
    let mut pixels = put_together(device, queue, &mut compose, &mut readback, out, main, &overlays)?;
    if pixels.is_none() {
        pixels = readback.finish(device)?.pop();
    }
    let pixels = pixels.ok_or_else(|| anyhow::anyhow!("the frame did not come back from the graphics card"))?;
    Ok((pixels, out))
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
    if !video.has_clips() {
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

/// How far ahead (seconds) the editor's [`Player`] makes and runs up a clip coming on screen.
pub const LOOKAHEAD: f64 = 4.0;

/// A clip the editor's [`Player`] keeps running: its scene's run and renderer, run up to its first
/// frame on screen with the quick run-up's rule ([`crate::run_up`]: the physics from 0, only the
/// frames the scene remembers drawn), a piece at a time.
struct LiveClip<'a> {
    maker: FrameMaker<'a>,
    renderer: Renderer,
    fit: CoverFit,
    /// The scene frame it is run up to: its first frame on screen.
    first: u32,
    /// The next scene frame to step.
    at: u32,
    /// The first frame the scene's memory could need.
    earliest: u32,
    /// The first frame the run-up draws, once read at `earliest`.
    from: Option<u32>,
}

impl<'a> LiveClip<'a> {
    fn new(device: &wgpu::Device, scene: &'a Scene, kit: &'a Kit, out: (u32, u32), fps: u32, first: u32) -> Self {
        let s = bpe_kit::video::scene_settings(scene, &kit.registry);
        let fit = within(cover((s.width, s.height), out), device.limits().max_texture_dimension_2d);
        let time = f64::from(first) / f64::from(fps);
        let earliest = ((time - f64::from(bpe_gfx::effects::MAX_MEMORY)).max(0.0) * f64::from(fps)).ceil() as u32;
        LiveClip {
            maker: FrameMaker::new(scene, kit),
            renderer: Renderer::new(device, fit.draw.0, fit.draw.1),
            fit,
            first,
            at: 0,
            earliest: earliest.min(first),
            from: None,
        }
    }

    fn ready(&self) -> bool {
        self.at >= self.first
    }

    /// Runs up towards its first frame until `until` (no limit: to the end). True once there;
    /// "stopped" once `stop` is set.
    fn warm(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fps: u32,
        until: Option<std::time::Instant>,
        stop: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<bool> {
        while self.at < self.first {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                bail!("stopped");
            }
            if until.is_some_and(|u| std::time::Instant::now() >= u) {
                return Ok(false);
            }
            let k = self.at;
            let t = f64::from(k) / f64::from(fps);
            if k < self.earliest || self.from.is_some_and(|from| k < from) {
                self.maker.advance_to(t);
            } else {
                let frame = self.maker.frame(t);
                if k == self.earliest {
                    // How long the scene remembers, read at the first frame it could need.
                    let memory = f64::from(frame.memory());
                    let first_t = f64::from(self.first) / f64::from(fps);
                    self.from = Some(if memory > 0.0 {
                        (((first_t - memory).max(0.0) * f64::from(fps)).ceil() as u32).max(self.earliest)
                    } else {
                        self.first
                    });
                }
                if self.from.is_some_and(|from| k >= from) {
                    self.renderer.render(device, queue, &frame);
                }
            }
            self.at += 1;
        }
        Ok(true)
    }

    /// Draws scene frame `n` (never earlier than the last).
    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, fps: u32, n: u32) {
        let frame = self.maker.frame(f64::from(n) / f64::from(fps));
        self.renderer.render(device, queue, &frame);
        self.at = n + 1;
    }

    /// Runs the physics to scene frame `n` without drawing it.
    fn skip(&mut self, fps: u32, n: u32) {
        if self.ready() && n >= self.at {
            self.maker.advance_to(f64::from(n) / f64::from(fps));
            self.at = n + 1;
        }
    }
}

/// The editor's player (part 3): draws a video's frames in order, from any frame, as the still
/// frame draws them (the quick run-up, no bake), one at a time, each read back at once as RGBA.
/// It keeps the clips on screen (on both lanes) running between frames, makes and runs up the next
/// clips in the time it is given ([`Player::warm_ahead`]), and can step the physics past frames
/// nobody will see ([`Player::skip`]). `video` must be tidied ([`tidied`]) and `scenes` its
/// [`clip_scenes`].
pub struct Player<'a> {
    video: &'a Video,
    scenes: &'a ClipScenes,
    kit: &'a Kit,
    out: (u32, u32),
    fps: u32,
    live: BTreeMap<usize, LiveClip<'a>>,
    /// The On top clips kept running, by their index in `Video::on_top`.
    tops: BTreeMap<usize, LiveClip<'a>>,
    compose: Option<Compose>,
    readback: Readback,
    /// The last frame drawn or skipped.
    last: Option<u32>,
}

impl<'a> Player<'a> {
    pub fn new(device: &wgpu::Device, video: &'a Video, scenes: &'a ClipScenes, kit: &'a Kit, scale: f32) -> Self {
        let s = &video.settings;
        let out = output_size(s.width, s.height, scale);
        Player {
            video,
            scenes,
            kit,
            out,
            fps: s.fps.max(1),
            live: BTreeMap::new(),
            tops: BTreeMap::new(),
            compose: None,
            readback: Readback::new(device, out.0, out.1, 1),
            last: None,
        }
    }

    /// The frames' size.
    pub fn size(&self) -> (u32, u32) {
        self.out
    }

    /// How many frames the video has.
    pub fn frame_count(&self) -> u32 {
        self.video.frame_count()
    }

    /// The clips drawn at `frame`: the look, the old and the new.
    fn on_screen(&self, frame: u32) -> (bpe_kit::video::Moment, Option<Look>, Option<OnScreen>) {
        let m = moment(self.video, frame);
        let look = look_of(m.kind);
        let old = m.old.filter(|_| look.is_some());
        (m, look, old)
    }

    fn live_clip(&mut self, device: &wgpu::Device, clip: usize, first: u32) -> &mut LiveClip<'a> {
        let (scenes, kit, out, fps) = (self.scenes, self.kit, self.out, self.fps);
        self.live.entry(clip).or_insert_with(|| LiveClip::new(device, &scenes.main[clip], kit, out, fps, first))
    }

    fn live_top(&mut self, device: &wgpu::Device, top: usize, first: u32) -> &mut LiveClip<'a> {
        let (scenes, kit, fps) = (self.scenes, self.kit, self.fps);
        let size = top_size(&self.video.on_top[top], self.out);
        self.tops.entry(top).or_insert_with(|| LiveClip::new(device, &scenes.tops[top], kit, size, fps, first))
    }

    /// Draws video frame `frame` (frames are asked in order) and gives its RGBA pixels.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: u32,
        stop: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<Vec<u8>> {
        let (m, look, old) = self.on_screen(frame);
        let fps = self.fps;
        for on in [old, m.new].into_iter().flatten() {
            let live = self.live_clip(device, on.clip, on.scene_frame);
            live.warm(device, queue, fps, None, stop)?;
            // Frames come in order; never ask the physics for a time it has passed.
            let n = on.scene_frame.max(live.at.saturating_sub(1));
            live.draw(device, queue, fps, n);
        }
        let tops = on_top_at(self.video, frame);
        for t in &tops {
            let live = self.live_top(device, t.top, t.scene_frame);
            live.warm(device, queue, fps, None, stop)?;
            let n = t.scene_frame.max(live.at.saturating_sub(1));
            if t.alpha > 0.0 {
                live.draw(device, queue, fps, n);
            } else {
                live.skip(fps, n);
            }
        }
        let (out, video) = (self.out, self.video);
        let Player { live, tops: top_live, compose, readback, .. } = self;
        let shown = |o: Option<OnScreen>| o.map(|o| (live[&o.clip].renderer.output_view(), live[&o.clip].fit.crop));
        let lone = match (old, m.new) {
            (None, Some(new)) if live[&new.clip].fit.draw == out => Some(&live[&new.clip].renderer),
            _ => None,
        };
        let main = Main { old: shown(old), new: shown(m.new), look, progress: m.progress, lone };
        let overlays: Vec<_> = tops
            .iter()
            .filter(|t| t.alpha > 0.0)
            .map(|t| {
                let l = &top_live[&t.top];
                (l.renderer.output_view(), top_overlay(video, &video.on_top[t.top], out, &l.fit, t.alpha))
            })
            .collect();
        let pushed = put_together(device, queue, compose, readback, out, main, &overlays)?;
        let pixels = match pushed {
            Some(pixels) => pixels,
            None => readback.finish(device)?.pop().ok_or_else(|| anyhow::anyhow!("the frame did not come back"))?,
        };
        self.passed(frame);
        Ok(pixels)
    }

    /// Steps the clips on screen at `frame` to it without drawing (the editor fell behind).
    pub fn skip(&mut self, frame: u32) {
        let (m, _, old) = self.on_screen(frame);
        let fps = self.fps;
        for on in [old, m.new].into_iter().flatten() {
            if let Some(live) = self.live.get_mut(&on.clip) {
                live.skip(fps, on.scene_frame);
            }
        }
        for t in on_top_at(self.video, frame) {
            if let Some(live) = self.tops.get_mut(&t.top) {
                live.skip(fps, t.scene_frame);
            }
        }
        self.passed(frame);
    }

    /// Frame `frame` is done: clips that have ended are dropped.
    fn passed(&mut self, frame: u32) {
        self.last = Some(frame);
        let next = i64::from(frame) + 1;
        let video = self.video;
        self.live.retain(|&i, _| video.frames(video.clips[i].end()) > next);
        self.tops.retain(|&i, _| video.frames(video.on_top[i].clip.end()) > next);
    }

    /// Makes the next clip on each lane that comes on screen within [`LOOKAHEAD`] of the last
    /// frame and runs it (and any clip not yet run up) up until `until`.
    pub fn warm_ahead(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        until: std::time::Instant,
        stop: &std::sync::atomic::AtomicBool,
    ) -> anyhow::Result<()> {
        let last = i64::from(self.last.unwrap_or(0));
        let reach = last + (LOOKAHEAD * f64::from(self.fps)).round() as i64;
        let video = self.video;
        let coming = |c: &bpe_core::video::Clip| {
            let start = video.frames(c.start);
            start > last && start <= reach
        };
        let next = video.clips.iter().enumerate().find(|(i, c)| coming(c) && !self.live.contains_key(i));
        if let Some((i, c)) = next {
            let first = video.frames(c.offset).max(0) as u32;
            self.live_clip(device, i, first);
        }
        let next_top = video.on_top.iter().enumerate().find(|(i, t)| coming(&t.clip) && !self.tops.contains_key(i));
        if let Some((i, t)) = next_top {
            let first = video.frames(t.clip.offset).max(0) as u32;
            self.live_top(device, i, first);
        }
        let fps = self.fps;
        for live in self.live.values_mut().chain(self.tops.values_mut()).filter(|l| !l.ready()) {
            if !live.warm(device, queue, fps, Some(until), stop)? {
                break;
            }
        }
        Ok(())
    }

    /// Whether nothing is waiting to be run up.
    pub fn all_warm(&self) -> bool {
        self.live.values().chain(self.tops.values()).all(LiveClip::ready)
    }
}
