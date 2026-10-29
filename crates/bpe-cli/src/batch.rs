//! Batch rendering: many seeds of one scene, pre-screened by score, rendered, and the best kept.
//! The record of a batch lives in its folder as `batch.ron`, so a stopped batch carries on.

use crate::{Encoder, ExportOptions, export_video};
use anyhow::{Context, bail};
use bpe_core::{Registry, Scene};
use bpe_gfx::wgpu;
use bpe_kit::random::{Roll, Scope, roll};
use bpe_kit::score::{Parts, sample, score};
use bpe_kit::{Kit, resolve_scene};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The record of a batch, in its folder.
pub const MANIFEST: &str = "batch.ron";
const LOCK: &str = "batch.lock";

/// How good and how slow the rendered videos are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    /// Half size, fast encoding: for checking a batch before running it overnight.
    Draft,
    /// Full size, normal encoding.
    #[default]
    Good,
    /// Full size, slowest encoding and highest quality.
    Best,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Draft, Quality::Good, Quality::Best];

    pub fn name(self) -> &'static str {
        match self {
            Quality::Draft => "Draft",
            Quality::Good => "Good",
            Quality::Best => "Best",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.name().eq_ignore_ascii_case(name))
    }

    /// Sets size and encoder settings on `options`.
    pub fn apply(self, options: &mut ExportOptions) {
        let (scale, preset, crf) = match self {
            Quality::Draft => (0.5, "veryfast", 23),
            Quality::Good => (1.0, "medium", 20),
            Quality::Best => (1.0, "slow", 18),
        };
        options.scale = scale;
        options.preset = preset.into();
        options.crf = crf;
    }
}

/// What happens to rendered videos that didn't make the cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rejected {
    /// Moved into a "rejected" folder inside the batch folder.
    #[default]
    Move,
    Delete,
}

/// Everything that defines a batch. Starting a batch with the same settings in the same folder
/// carries on with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchSpec {
    /// How many videos to render.
    pub count: u32,
    /// How many of the rendered videos to keep.
    pub keep: u32,
    /// The first seed; the batch uses the seeds after it.
    pub first_seed: u32,
    /// Seeds simulated per video rendered, to pick the most promising ones.
    pub prescreen: u32,
    /// How far from the scene the variations go, 0 to 1.
    pub amount: f32,
    pub quality: Quality,
    pub rejected: Rejected,
}

impl Default for BatchSpec {
    fn default() -> Self {
        Self {
            count: 50,
            keep: 10,
            first_seed: 1,
            prescreen: 4,
            amount: 0.5,
            quality: Quality::Good,
            rejected: Rejected::Move,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    /// Not simulated yet.
    #[default]
    Waiting,
    /// Simulated and scored, waiting to see whether it is rendered.
    Screened,
    /// Didn't score well enough in the pre-screen to be rendered.
    Skipped,
    /// Picked for rendering.
    Chosen,
    Rendered,
    Kept,
    Rejected,
    /// Rendering failed; see the job's error.
    Failed,
}

impl Status {
    /// The word a job card shows.
    pub fn word(self) -> &'static str {
        match self {
            Status::Waiting => "Waiting",
            Status::Screened => "Screened",
            Status::Skipped => "Skipped",
            Status::Chosen => "Chosen",
            Status::Rendered => "Rendered",
            Status::Kept => "Kept",
            Status::Rejected => "Rejected",
            Status::Failed => "Failed",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub seed: u32,
    pub status: Status,
    /// The total score, 0 to 1, once screened.
    pub score: Option<f32>,
    pub parts: Option<Parts>,
    /// The video's file name in the batch folder (or in `rejected`).
    pub file: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    #[default]
    Prescreen,
    Render,
    Pick,
    Done,
}

impl Phase {
    pub fn word(self) -> &'static str {
        match self {
            Phase::Prescreen => "Trying seeds",
            Phase::Render => "Rendering",
            Phase::Pick => "Picking the best",
            Phase::Done => "Finished",
        }
    }
}

/// The record of a batch, saved as `batch.ron` in its folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub spec: BatchSpec,
    /// The scene as it was when the batch started, so editing it later doesn't change a batch
    /// that is half done.
    pub base: Scene,
    pub phase: Phase,
    pub jobs: Vec<Job>,
}

impl Manifest {
    pub fn load(folder: &Path) -> anyhow::Result<Self> {
        let path = folder.join(MANIFEST);
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }

    /// Writes the record next to itself first and then swaps it in, so a crash mid-write never
    /// leaves a broken file.
    pub fn save(&self, folder: &Path) -> anyhow::Result<()> {
        let path = folder.join(MANIFEST);
        let temporary = folder.join(format!("{MANIFEST}.new"));
        let pretty = ron::ser::PrettyConfig::new().depth_limit(4);
        std::fs::write(&temporary, ron::ser::to_string_pretty(self, pretty)?)?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    }

    /// Jobs in a given state.
    pub fn count(&self, status: Status) -> usize {
        self.jobs.iter().filter(|j| j.status == status).count()
    }

    /// The kept videos, best first.
    pub fn kept(&self) -> Vec<&Job> {
        let mut kept: Vec<&Job> = self.jobs.iter().filter(|j| j.status == Status::Kept).collect();
        kept.sort_by(|a, b| total(b).total_cmp(&total(a)));
        kept
    }
}

fn total(job: &Job) -> f32 {
    job.score.unwrap_or(-1.0)
}

/// Where a batch is, for progress bars.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    /// Jobs finished in this phase, and how many it has.
    pub done: u32,
    pub total: u32,
    /// Frames of the video being rendered, and how many it has.
    pub frame: u32,
    pub frames: u32,
}

/// Opens the batch in `folder`: the one already there when its settings match, else a new one.
pub fn open(folder: &Path, spec: &BatchSpec, base: &Scene) -> anyhow::Result<Manifest> {
    std::fs::create_dir_all(folder).with_context(|| format!("making the folder {}", folder.display()))?;
    if folder.join(MANIFEST).is_file() {
        let manifest = Manifest::load(folder)?;
        if manifest.spec == *spec {
            return Ok(manifest);
        }
        bail!("This folder holds a batch with other settings. Choose another folder, or use its settings.");
    }
    if spec.count == 0 {
        bail!("A batch needs at least one video");
    }
    if spec.keep == 0 {
        bail!("A batch needs to keep at least one video");
    }
    let screened = spec.count.saturating_mul(spec.prescreen.max(1));
    let jobs = (0..screened).map(|i| Job { seed: spec.first_seed.wrapping_add(i), ..Job::default() }).collect();
    let manifest = Manifest { spec: spec.clone(), base: base.clone(), phase: Phase::Prescreen, jobs };
    manifest.save(folder)?;
    Ok(manifest)
}

/// Keeps other programs out of a batch folder while it is worked on. Two programs working on
/// one folder would delete each other's half-made videos. Goes away with the program.
#[derive(Debug)]
pub struct Lock {
    _file: std::fs::File,
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn lock(folder: &Path) -> anyhow::Result<Lock> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(LOCK);
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path)?;
    if file.try_lock().is_err() {
        bail!("Another batch is running in this folder");
    }
    Ok(Lock { _file: file, path })
}

/// The scene a seed makes: the whole scene rolled, named after the seed.
pub fn scene_for(base: &Scene, reg: &Registry, seed: u32, amount: f32) -> Scene {
    let mut scene = base.clone();
    roll(&mut scene, reg, &Scope::Scene, Roll { seed, amount });
    scene.name = format!("{} seed {seed}", base.name.trim()).trim().to_owned();
    scene
}

/// A file name for a seed's video: the scene's name as a slug, then the seed.
pub fn video_name(scene_name: &str, seed: u32) -> String {
    let slug: String = scene_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() { "scene".to_owned() } else { slug };
    format!("{slug}-seed-{seed}.mp4")
}

/// Picks the jobs to render: the best screened scores, ties going to the lower seed.
pub fn pick_chosen(jobs: &mut [Job], count: u32) {
    let mut order: Vec<usize> = (0..jobs.len()).filter(|&i| jobs[i].status == Status::Screened).collect();
    order.sort_by(|&a, &b| total(&jobs[b]).total_cmp(&total(&jobs[a])).then(jobs[a].seed.cmp(&jobs[b].seed)));
    for (rank, index) in order.into_iter().enumerate() {
        jobs[index].status = if rank < count as usize { Status::Chosen } else { Status::Skipped };
    }
}

/// Keeps the best rendered videos; the rest are rejected.
pub fn pick_kept(jobs: &mut [Job], keep: u32) {
    let mut order: Vec<usize> = (0..jobs.len()).filter(|&i| jobs[i].status == Status::Rendered).collect();
    order.sort_by(|&a, &b| total(&jobs[b]).total_cmp(&total(&jobs[a])).then(jobs[a].seed.cmp(&jobs[b].seed)));
    for (rank, index) in order.into_iter().enumerate() {
        jobs[index].status = if rank < keep as usize { Status::Kept } else { Status::Rejected };
    }
}

/// Runs the batch in `folder` to the end, saving its record after every step. `progress` is
/// called after every job and every few frames; returning false stops after the current step,
/// and `run` returns Ok so the batch can carry on later. Takes the folder's lock while it runs.
#[allow(clippy::too_many_arguments)]
pub fn run(
    folder: &Path,
    manifest: &mut Manifest,
    kit: &Kit,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ffmpeg: &Path,
    weights: &Parts,
    progress: &mut dyn FnMut(&Manifest, Progress) -> bool,
) -> anyhow::Result<()> {
    let _lock = lock(folder)?;
    let stop = AtomicBool::new(false);
    if manifest.phase == Phase::Prescreen {
        prescreen(folder, manifest, kit, weights, &stop, progress)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        pick_chosen(&mut manifest.jobs, manifest.spec.count);
        manifest.phase = Phase::Render;
        manifest.save(folder)?;
    }
    if manifest.phase == Phase::Render {
        render(folder, manifest, kit, device, queue, ffmpeg, &stop, progress)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        manifest.phase = Phase::Pick;
        manifest.save(folder)?;
    }
    if manifest.phase == Phase::Pick {
        pick(folder, manifest)?;
        manifest.phase = Phase::Done;
        manifest.save(folder)?;
    }
    let total = manifest.jobs.len() as u32;
    progress(manifest, Progress { phase: Phase::Done, done: total, total, ..Progress::default() });
    Ok(())
}

/// Simulates and scores every waiting seed on all CPU cores.
fn prescreen(
    folder: &Path,
    manifest: &mut Manifest,
    kit: &Kit,
    weights: &Parts,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(&Manifest, Progress) -> bool,
) -> anyhow::Result<()> {
    let waiting: Vec<usize> =
        (0..manifest.jobs.len()).filter(|&i| manifest.jobs[i].status == Status::Waiting).collect();
    let total = manifest.jobs.len() as u32;
    let done_before = total - waiting.len() as u32;
    let (sender, receiver) = std::sync::mpsc::channel();
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(waiting.len().max(1));
    let spec = manifest.spec.clone();
    let seeds: Vec<u32> = manifest.jobs.iter().map(|j| j.seed).collect();
    let base = manifest.base.clone();
    let base = &base;
    std::thread::scope(|scope| -> anyhow::Result<()> {
        for _ in 0..threads {
            let sender = sender.clone();
            let (next, waiting, seeds, spec) = (&next, &waiting, &seeds, &spec);
            scope.spawn(move || {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&index) = waiting.get(at) else { return };
                    let scene = scene_for(base, &kit.registry, seeds[index], spec.amount);
                    let resolved = resolve_scene(&scene, &kit.registry);
                    let Some(sampled) = sample(&scene, &resolved, &kit.sims, stop) else { return };
                    if sender.send((index, score(&sampled, weights))).is_err() {
                        return;
                    }
                }
            });
        }
        drop(sender);
        let mut last_save = Instant::now();
        for (count, (index, score)) in receiver.into_iter().enumerate() {
            let job = &mut manifest.jobs[index];
            job.score = Some(score.total);
            job.parts = Some(score.parts);
            job.status = Status::Screened;
            if last_save.elapsed() > Duration::from_secs(2) {
                manifest.save(folder)?;
                last_save = Instant::now();
            }
            let done = done_before + count as u32 + 1;
            if !progress(manifest, Progress { phase: Phase::Prescreen, done, total, ..Progress::default() }) {
                stop.store(true, Ordering::Relaxed);
            }
        }
        Ok(())
    })?;
    manifest.save(folder)
}

/// Renders every chosen job, best first.
#[allow(clippy::too_many_arguments)]
fn render(
    folder: &Path,
    manifest: &mut Manifest,
    kit: &Kit,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    ffmpeg: &Path,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(&Manifest, Progress) -> bool,
) -> anyhow::Result<()> {
    let mut queue_of_jobs: Vec<usize> =
        (0..manifest.jobs.len()).filter(|&i| manifest.jobs[i].status == Status::Chosen).collect();
    queue_of_jobs.sort_by(|&a, &b| total(&manifest.jobs[b]).total_cmp(&total(&manifest.jobs[a])));
    let total_jobs =
        manifest.jobs.iter().filter(|j| matches!(j.status, Status::Chosen | Status::Rendered | Status::Failed)).count()
            as u32;
    let done_before = total_jobs - queue_of_jobs.len() as u32;
    let mut options = ExportOptions::new(folder.join("unused.mp4"));
    options.ffmpeg = ffmpeg.to_path_buf();
    manifest.spec.quality.apply(&mut options);
    for (position, &index) in queue_of_jobs.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let seed = manifest.jobs[index].seed;
        let scene = scene_for(&manifest.base, &kit.registry, seed, manifest.spec.amount);
        let name = video_name(&manifest.base.name, seed);
        let partial = folder.join(format!("{name}.partial.mp4"));
        let _ = std::fs::remove_file(&partial);
        options.output = partial.clone();
        let done = done_before + position as u32;
        let mut stopped = false;
        let result = export_video(device, queue, &scene, kit, &options, |frame, frames| {
            if frame % 10 == 0 || frame == frames {
                let p = Progress { phase: Phase::Render, done, total: total_jobs, frame, frames };
                if !progress(manifest, p) {
                    stopped = true;
                }
            }
            !stopped
        });
        if stopped {
            let _ = std::fs::remove_file(&partial);
            stop.store(true, Ordering::Relaxed);
            break;
        }
        let job = &mut manifest.jobs[index];
        match result {
            Ok(encoder) => {
                // A graphics card encoder that failed is not tried again on every video.
                if encoder == Encoder::Cpu {
                    options.encoder = Encoder::Cpu;
                }
                std::fs::rename(&partial, folder.join(&name))?;
                std::fs::write(folder.join(name.replace(".mp4", ".ron")), bpe_core::save(&scene))?;
                job.file = Some(PathBuf::from(&name));
                job.status = Status::Rendered;
            }
            Err(error) => {
                job.status = Status::Failed;
                job.error = Some(format!("{error:#}"));
            }
        }
        manifest.save(folder)?;
        let p = Progress { phase: Phase::Render, done: done + 1, total: total_jobs, ..Progress::default() };
        if !progress(manifest, p) {
            stop.store(true, Ordering::Relaxed);
            break;
        }
    }
    Ok(())
}

/// Keeps the best rendered videos and moves or deletes the rest.
fn pick(folder: &Path, manifest: &mut Manifest) -> anyhow::Result<()> {
    pick_kept(&mut manifest.jobs, manifest.spec.keep);
    let rejected_dir = folder.join("rejected");
    for job in manifest.jobs.iter_mut().filter(|j| j.status == Status::Rejected) {
        let Some(file) = job.file.clone() else { continue };
        let scene_file = file.with_extension("ron");
        for name in [file, scene_file] {
            let from = folder.join(&name);
            if !from.exists() {
                continue;
            }
            match manifest.spec.rejected {
                Rejected::Delete => std::fs::remove_file(&from)?,
                Rejected::Move => {
                    std::fs::create_dir_all(&rejected_dir)?;
                    std::fs::rename(&from, rejected_dir.join(&name))?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::{kit, new_scene};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-batch-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn video_names_are_slugs() {
        assert_eq!(video_name("Peg board!", 7), "peg-board-seed-7.mp4");
        assert_eq!(video_name("", 1), "scene-seed-1.mp4");
        assert_eq!(video_name("  Ünïcode  name ", 12), "ünïcode-name-seed-12.mp4");
    }

    #[test]
    fn open_makes_resumes_and_refuses() {
        let kit = kit();
        let base = new_scene(&kit.registry);
        let dir = temp_dir("open");
        let spec = BatchSpec { count: 2, keep: 1, prescreen: 3, first_seed: 10, ..BatchSpec::default() };
        let manifest = open(&dir, &spec, &base).unwrap();
        assert_eq!(manifest.jobs.len(), 6, "count × candidates per video");
        assert_eq!(manifest.jobs.iter().map(|j| j.seed).collect::<Vec<_>>(), [10, 11, 12, 13, 14, 15]);
        assert!(manifest.jobs.iter().all(|j| j.status == Status::Waiting));
        assert_eq!(manifest.phase, Phase::Prescreen);
        assert!(dir.join(MANIFEST).is_file());
        assert_eq!(manifest.base, base);

        let again = open(&dir, &spec, &base).unwrap();
        assert_eq!(again, manifest, "the same settings carry on with the batch in the folder");

        let other = BatchSpec { count: 3, ..spec.clone() };
        let error = open(&dir, &other, &base).unwrap_err().to_string();
        assert!(error.contains("other settings"), "{error}");

        let empty = temp_dir("open-empty");
        assert!(open(&empty, &BatchSpec { count: 0, ..spec.clone() }, &base).is_err());
        assert!(open(&empty, &BatchSpec { keep: 0, ..spec }, &base).is_err());
    }

    #[test]
    fn the_lock_keeps_a_second_batch_out() {
        let dir = temp_dir("lock");
        let first = lock(&dir).unwrap();
        let error = lock(&dir).unwrap_err().to_string();
        assert!(error.contains("Another batch"), "{error}");
        drop(first);
        assert!(lock(&dir).is_ok());
    }

    fn job(seed: u32, status: Status, score: f32) -> Job {
        Job { seed, status, score: Some(score), ..Job::default() }
    }

    #[test]
    fn prescreen_chooses_the_best_count_seeds_and_pick_keeps_the_best_rendered() {
        let mut jobs = vec![
            job(1, Status::Screened, 0.3),
            job(2, Status::Screened, 0.9),
            job(3, Status::Screened, 0.6),
            job(4, Status::Screened, 0.6),
        ];
        pick_chosen(&mut jobs, 2);
        let statuses: Vec<Status> = jobs.iter().map(|j| j.status).collect();
        assert_eq!(
            statuses,
            [Status::Skipped, Status::Chosen, Status::Chosen, Status::Skipped],
            "ties go to the lower seed"
        );

        let mut jobs = vec![job(1, Status::Rendered, 0.3), job(2, Status::Rendered, 0.9), job(3, Status::Failed, 0.0)];
        pick_kept(&mut jobs, 1);
        let statuses: Vec<Status> = jobs.iter().map(|j| j.status).collect();
        assert_eq!(statuses, [Status::Rejected, Status::Kept, Status::Failed]);
    }

    #[test]
    fn a_seed_makes_a_different_scene_named_after_it() {
        let kit = kit();
        let base = new_scene(&kit.registry);
        let one = scene_for(&base, &kit.registry, 1, 0.5);
        let two = scene_for(&base, &kit.registry, 2, 0.5);
        assert_ne!(one, base);
        assert_ne!(one, two);
        assert_eq!(scene_for(&base, &kit.registry, 1, 0.5), one);
        assert_eq!(one.name, "New scene seed 1");
    }

    #[test]
    fn quality_sets_size_and_encoding() {
        let mut options = ExportOptions::new("x.mp4");
        Quality::Draft.apply(&mut options);
        assert_eq!((options.scale, options.preset.as_str(), options.crf), (0.5, "veryfast", 23));
        Quality::Good.apply(&mut options);
        assert_eq!((options.scale, options.preset.as_str(), options.crf), (1.0, "medium", 20));
        Quality::Best.apply(&mut options);
        assert_eq!((options.scale, options.preset.as_str(), options.crf), (1.0, "slow", 18));
        assert_eq!(Quality::parse("best"), Some(Quality::Best));
        assert_eq!(Quality::parse("nope"), None);
    }
}
