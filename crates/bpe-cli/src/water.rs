//! A Water scene's video (Water mode part 8): the camera's shot at each video frame's water time,
//! drawn in the scene's look (part 6) from the scene's final take, its drops moved on
//! to the exact moment, with a silent track. The final is never worked out again here, only read;
//! a missing final is made first when asked (the command line), and the draft is exported only when
//! asked for by name.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context;
use bpe_core::{Address, Registry, Resolved, Scene, Settings, ThingId, Value};
use bpe_gfx::{Readback, wgpu};
use bpe_kit::things::water::draft::{DRAFT_MOST, Speeds, WaterSetup};
use bpe_kit::things::water::finals::{Finals, Status};
use bpe_kit::things::water::look::look_of;
use bpe_kit::things::water::make::{MakeFinal, MakeProgress, this_card};
use bpe_kit::{Kit, SceneSettings};
use bpe_water::draw::wet::HeldWet;
use bpe_water::draw::{LookDraw, LookGpu, StoredDrops, View, WetField};
use bpe_water::take::{Chunk, FrameCodec, TakeFrame, chunk};
use bpe_water::{SolidField, WaterGpu};
use glam::Vec2;

use crate::{CANCELLED, ExportOptions, FRAMES_IN_FLIGHT, Picture, output_size};

/// What a Water scene's export says when the final doesn't cover the video.
pub const FINAL_MISSING: &str = "The final isn't made for the whole video yet: make it first, or export the draft.";
/// What Preview final frame says at a moment the final hasn't made.
pub const FRAME_NOT_MADE: &str =
    "This moment of the final isn't made yet: press Make final, or see the draft on the stage.";

/// Where a Water scene's pictures come from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WaterSource {
    /// The final (made first when `make`, else refused when it doesn't cover the video).
    #[default]
    Final,
    MakeFinal,
    /// The draft, worked out as the stage plays it.
    Draft,
}

/// A Water scene's video: its settings, its water length and its Speed.
pub struct WaterVideo {
    pub resolved: Resolved,
    pub settings: SceneSettings,
    pub speeds: Speeds,
}

impl WaterVideo {
    pub fn of(scene: &Scene, kit: &Kit) -> anyhow::Result<WaterVideo> {
        let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
        let root = resolved.things.get(&ThingId::ROOT).context("the scene has no root")?;
        let settings = SceneSettings::from_params(&root.params);
        let speeds = Speeds::of(scene, &resolved, f64::from(settings.duration.max(0.0)));
        Ok(WaterVideo { resolved, settings, speeds })
    }

    /// The water seconds the scene plays (its Length).
    pub fn length(&self) -> f64 {
        f64::from(self.settings.duration.max(0.0))
    }

    /// The video's seconds (longer with slow motion and Un-mix).
    pub fn video_seconds(&self) -> f64 {
        self.speeds.video_length(self.length())
    }

    /// The video's frames.
    pub fn frames(&self) -> u32 {
        (self.video_seconds() * f64::from(self.settings.fps.max(1))).round().max(1.0) as u32
    }

    /// The scene's final.
    pub fn final_of(&self, scene: &Scene) -> MakeFinal {
        MakeFinal::of(scene, &self.resolved, self.length())
    }
}

/// The camera's 9:16 shot at water time `tau` (its Position and Zoom then), as a `width` x
/// `height` picture.
pub fn shot_view(scene: &Scene, reg: &Registry, tau: f64, width: u32, height: u32) -> View {
    let (centre, shot) = shot_at(scene, reg, tau);
    View { lo: centre - shot / 2.0, scale: width as f32 / shot.x, width, height }
}

/// The camera's 9:16 shot at water time `tau`: its middle and its size, in world pixels.
pub fn shot_at(scene: &Scene, reg: &Registry, tau: f64) -> (Vec2, Vec2) {
    let screen = bpe_kit::modules::water::SCREEN;
    let (centre, zoom) = match bpe_kit::camera_of(scene) {
        Some(camera) => {
            let at = |key: &str| scene.value_at_time(reg, &Address::own(camera, key), tau);
            let centre = match at("position") {
                Some(Value::Vec2([x, y])) => Vec2::new(x, y),
                _ => Vec2::ZERO,
            };
            let zoom = match at("zoom") {
                Some(Value::Number(z)) if z > 0.0 && z.is_finite() => z as f32,
                _ => 1.0,
            };
            (centre, zoom)
        }
        None => (Vec2::ZERO, 1.0),
    };
    (centre, screen / zoom)
}

/// Reads a take's moments by water time, keeping the chunks it read last open.
pub struct TakeReader {
    dir: PathBuf,
    layout: bpe_water::Layout,
    good: u32,
    chunks: Vec<(u32, Chunk)>,
    codec: FrameCodec,
    /// The wet marks of the chunk read last (`None`: none there).
    wet: Option<(u32, Option<Arc<Vec<u32>>>)>,
}

impl TakeReader {
    /// The take `status` names, its first `status.good` frames to be read.
    pub fn open(finals: &Finals, status: &Status) -> Option<TakeReader> {
        let take = status.take.as_ref()?;
        Some(TakeReader {
            dir: finals.take_dir(take.print),
            layout: take.layout(),
            good: status.good,
            chunks: Vec::new(),
            codec: FrameCodec::new(),
            wet: None,
        })
    }

    pub fn layout(&self) -> bpe_water::Layout {
        self.layout
    }

    fn chunk(&mut self, second: u32) -> anyhow::Result<&mut Chunk> {
        if let Some(i) = self.chunks.iter().position(|c| c.0 == second) {
            return Ok(&mut self.chunks[i].1);
        }
        let chunk = Chunk::open(&self.dir, second)?;
        if self.chunks.len() >= 2 {
            self.chunks.remove(0);
        }
        self.chunks.push((second, chunk));
        Ok(&mut self.chunks.last_mut().expect("just pushed").1)
    }

    /// The wet marks by water time `tau` (the marks kept with its chunk) and the water frame to read
    /// them against; `None` when the final has not made that moment, or its marks don't read.
    pub fn wet(&mut self, tau: f64) -> Option<(Arc<Vec<u32>>, u32)> {
        let k = (tau.max(0.0) * 60.0).floor() as u32;
        if k >= self.good {
            return None;
        }
        let second = k / 60;
        if self.wet.as_ref().is_none_or(|w| w.0 != second) {
            let marks = chunk::read_wet(&self.dir, second, self.layout.nodes()).ok().map(Arc::new);
            self.wet = Some((second, marks));
        }
        self.wet.as_ref().and_then(|w| w.1.clone()).map(|m| (m, k))
    }

    /// The kept moment nearest water time `tau` (seconds), unpacked, and how far `tau` is past it;
    /// `None` when the final has not made that moment (or it is out of date).
    pub fn near(&mut self, tau: f64) -> anyhow::Result<Option<(TakeFrame, f64)>> {
        let k = (tau.max(0.0) * 60.0).floor() as u32;
        if k >= self.good {
            return Ok(None);
        }
        let mut best: Option<(u32, usize, f64)> = None;
        let mut consider = |second: u32, i: usize, t: f64| {
            if best.is_none_or(|b| (t - tau).abs() < (b.2 - tau).abs()) {
                best = Some((second, i, t));
            }
        };
        let second = k / 60;
        let here: Vec<(usize, f64)> = self
            .chunk(second)?
            .moments()
            .enumerate()
            .filter(|(_, m)| m.frame == k)
            .map(|(i, m)| (i, m.time()))
            .collect();
        for (i, t) in here {
            consider(second, i, t);
        }
        if k + 1 < self.good {
            let next = (k + 1) / 60;
            let there: Option<(usize, f64)> = self
                .chunk(next)?
                .moments()
                .enumerate()
                .find(|(_, m)| m.frame == k + 1 && m.steps == 0)
                .map(|(i, m)| (i, m.time()));
            if let Some((i, t)) = there {
                consider(next, i, t);
            }
        }
        let Some((second, i, t)) = best else { return Ok(None) };
        let bytes = self.chunk(second)?.bytes(i)?;
        let layout = self.layout;
        let frame = self.codec.decode(&bytes, &layout)?;
        Ok(Some((frame, tau - t)))
    }
}

/// The look drawing of a course, at the export's size.
struct Drawing {
    look: LookDraw,
    stored: StoredDrops,
    wet: HeldWet,
}

impl Drawing {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, field: &SolidField, rest: f32) -> Drawing {
        let gpu = LookGpu::new(device, queue);
        Drawing {
            look: LookDraw::new(gpu, field, rest),
            stored: StoredDrops::new(device, queue, &[]),
            wet: HeldWet::default(),
        }
    }
}

/// A moment's picture: the take's moment (or a draft body's frame) as the camera shows it then.
enum Source {
    Final { reader: TakeReader },
    Draft { setup: Arc<WaterSetup>, water: Box<bpe_water::Water>, field: SolidField, wet: WetField },
}

/// The final's status for `video`'s scene in `finals`, made first when `make`.
fn final_status(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    video: &WaterVideo,
    finals: &Finals,
    make: bool,
    making: &mut dyn FnMut(f32) -> bool,
) -> anyhow::Result<Status> {
    let fin = video.final_of(scene);
    let status = fin.status(finals, this_card());
    if status.complete() || !make {
        return Ok(status);
    }
    let gpu = WaterGpu::new(device, queue);
    let progress = MakeProgress::default();
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| -> anyhow::Result<()> {
        let job = s.spawn(|| fin.make(&gpu, finals, &progress, &stop));
        while !job.is_finished() {
            if !making(progress.share()) {
                stop.store(true, Ordering::Release);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        job.join().map_err(|_| anyhow::anyhow!("making the final stopped unexpectedly"))?.map(|_| ())
    })?;
    if stop.load(Ordering::Acquire) {
        anyhow::bail!(CANCELLED);
    }
    Ok(fin.status(finals, this_card()))
}

/// The finals folder an export reads.
pub fn finals_for(options: &ExportOptions) -> Finals {
    Finals::at(options.finals.clone().unwrap_or_else(Finals::default_dir))
}

/// Exports a Water scene to an mp4 (as [`crate::export_video`] does a Balls scene), its pictures
/// from `options.water`; a graphics card encoder that fails is tried again on the CPU. `making`
/// is told how far a make it asked for is (0 to 1; `false` stops it), `progress` the frames drawn.
/// Gives back the encoder that made it.
pub fn export(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    options: &ExportOptions,
    making: &mut dyn FnMut(f32) -> bool,
    progress: &mut dyn FnMut(u32, u32) -> bool,
) -> anyhow::Result<crate::Encoder> {
    let encoder = crate::resolve_encoder(options.encoder, &options.ffmpeg);
    match export_water(device, queue, scene, kit, options, encoder, making, progress) {
        Err(error) if crate::retry_on_cpu(encoder, &error) && error.to_string() != FINAL_MISSING => {
            export_water(device, queue, scene, kit, options, crate::Encoder::Cpu, making, progress)
                .with_context(|| format!("the {} failed first ({error:#})", encoder.name()))?;
            Ok(crate::Encoder::Cpu)
        }
        result => result.map(|()| encoder),
    }
}

/// One go of [`export`] with `encoder`.
#[allow(clippy::too_many_arguments)]
fn export_water(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    options: &ExportOptions,
    encoder: crate::Encoder,
    making: &mut dyn FnMut(f32) -> bool,
    progress: &mut dyn FnMut(u32, u32) -> bool,
) -> anyhow::Result<()> {
    let mut video = WaterVideo::of(scene, kit)?;
    if options.water == WaterSource::Draft {
        // (The draft plays forwards only: an exported draft leaves Un-mix out.)
        video.speeds = video.speeds.without_unmix();
    }
    let (width, height) = output_size(video.settings.width, video.settings.height, options.scale);
    let fps = video.settings.fps.max(1);
    let total = options.max_frames.map_or(video.frames(), |m| m.min(video.frames())).max(1);
    let finals = finals_for(options);
    let mut source = match options.water {
        WaterSource::Draft => {
            let setup = Arc::new(WaterSetup::of(scene, &video.resolved));
            let field = setup.field();
            let gpu = WaterGpu::new(device, queue);
            let water = Box::new(setup.start(gpu, &field, DRAFT_MOST));
            let wet = WetField::new(device, queue, field.layout);
            wet.mark(&water.drop_buffers(), water.frames());
            Source::Draft { setup, water, field, wet }
        }
        WaterSource::Final | WaterSource::MakeFinal => {
            let make = options.water == WaterSource::MakeFinal;
            let status = final_status(device, queue, scene, &video, &finals, make, making)?;
            // (The video's last frame shows the water at its water time.)
            let last = video.speeds.water_time(f64::from(total - 1) / f64::from(fps));
            if !status.good_at((last * 60.0).floor() as u32) {
                anyhow::bail!(FINAL_MISSING);
            }
            if let Some(take) = &status.take {
                finals.touch(take.print);
            }
            Source::Final { reader: TakeReader::open(&finals, &status).context(FINAL_MISSING)? }
        }
    };
    let fin = video.final_of(scene);
    let mut drawing = match &source {
        Source::Final { .. } => Drawing::new(device, queue, &fin.setup.field(), fin.setup.numbers().wall_off),
        Source::Draft { setup, field, .. } => Drawing::new(device, queue, field, setup.numbers().wall_off),
    };
    let picture = Picture { width, height, fps, total, encoder };
    let output = output_path(options);
    let made = crate::encode_frames(options, &picture, &output, true, |layout, send| {
        let mut readback = Readback::with_layout(device, width, height, FRAMES_IN_FLIGHT, layout);
        for index in 0..total {
            let tau = video.speeds.water_time(f64::from(index) / f64::from(fps));
            draw_moment(device, queue, scene, kit, &video.resolved, &mut source, &mut drawing, tau, (width, height))?;
            let (Some(texture), Some(view)) = (drawing.look.output(), drawing.look.output_view()) else {
                anyhow::bail!("the water was not drawn");
            };
            if let Some(pixels) = readback.push_texture(device, queue, texture, view)?
                && !send(pixels)
            {
                return Ok(true);
            }
            if !progress(index + 1, total) {
                return Ok(false);
            }
        }
        for pixels in readback.finish(device)? {
            if !send(pixels) {
                break;
            }
        }
        Ok(true)
    });
    if made.is_err() {
        let _ = std::fs::remove_file(&output);
    }
    made
}

fn output_path(options: &ExportOptions) -> PathBuf {
    let mut output = options.output.clone();
    if output.extension().is_none() {
        output.set_extension("mp4");
    }
    output
}

/// Draws water time `tau` from `source` into `drawing` at `size`.
#[allow(clippy::too_many_arguments)]
fn draw_moment(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    resolved: &Resolved,
    source: &mut Source,
    drawing: &mut Drawing,
    tau: f64,
    (width, height): (u32, u32),
) -> anyhow::Result<()> {
    let view = shot_view(scene, &kit.registry, tau, width, height);
    let look = look_of(scene, resolved, tau);
    match source {
        Source::Final { reader } => {
            let (frame, ahead) = reader.near(tau)?.context(FRAME_NOT_MADE)?;
            drawing.stored.set(device, queue, &frame.xv, &frame.kinds, &frame.mixes);
            let wet = reader.wet(tau);
            drawing.wet.give(device, queue, reader.layout(), wet.as_ref(), &mut drawing.look);
            drawing.look.draw_at(&drawing.stored.buffers(), &view, &look, ahead as f32);
        }
        Source::Draft { setup, water, field, wet } => {
            let k = (tau.max(0.0) * 60.0).round() as u32;
            while water.frames() < k {
                setup.set_inputs(water, field, DRAFT_MOST);
                water.frame();
                wet.mark(&water.drop_buffers(), water.frames());
            }
            let ahead = tau - f64::from(water.frames()) / 60.0;
            drawing.look.set_wet(Some(wet), water.frames());
            drawing.look.draw_at(&water.drop_buffers(), &view, &look, ahead as f32);
        }
    }
    Ok(())
}

/// Preview final frame of a Water scene: the final's moment at water time `time`, at `scale` of
/// the video's size: its RGBA pixels, its size and how long it took.
pub fn final_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    kit: &Kit,
    time: f64,
    scale: f32,
    finals: &Path,
) -> anyhow::Result<(Vec<u8>, (u32, u32), f32)> {
    let started = std::time::Instant::now();
    let video = WaterVideo::of(scene, kit)?;
    let (width, height) = output_size(video.settings.width, video.settings.height, scale);
    let finals = Finals::at(finals);
    let fin = video.final_of(scene);
    let status = fin.status(&finals, this_card());
    let reader = TakeReader::open(&finals, &status).context(FRAME_NOT_MADE)?;
    let mut source = Source::Final { reader };
    let mut drawing = Drawing::new(device, queue, &fin.setup.field(), fin.setup.numbers().wall_off);
    draw_moment(device, queue, scene, kit, &video.resolved, &mut source, &mut drawing, time, (width, height))?;
    let pixels = drawing.look.read();
    Ok((pixels, (width, height), started.elapsed().as_secs_f32()))
}
