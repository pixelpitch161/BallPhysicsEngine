//! The batch factory: makes many random variations of a scene, keeps the best, and renders them
//! while nobody is watching.
//!
//! A batch runs in three steps:
//!
//! 1. **Pre-screen.** Several times more seeds than videos wanted are simulated without drawing
//!    or sound, on every CPU core, and scored on rhythm, build-up, climax and motion.
//! 2. **Render.** The best of those are rendered to mp4, each with its `.ron` scene beside it, and
//!    scored again with sampled frames and the mixed soundtrack. While one video encodes, the next
//!    one is simulated and mixed.
//! 3. **Pick.** The best `keep` videos stay; the others are deleted or moved to `rejected`.
//!
//! Everything about the batch, including each job's status and score, is written to
//! `batch.json` in the output folder after every step. Starting the same batch again picks up
//! where it stopped, so a crash, a closed laptop or a killed process loses at most the video
//! that was being rendered.

use crate::score::{self, Parts, SampledFrame, Score, ScoreInputs, Weights};
use crate::{ExportOptions, export_video_with};
use anyhow::{Context, bail};
use bpe_render::{EffectLibrary, effects::ParamKind, wgpu};
use bpe_scene::{Dice, Effect, Group, Scene};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The file in the output folder that records the batch.
pub const MANIFEST: &str = "batch.json";
/// How many frames are sampled from each rendered video to judge its variety.
pub const SAMPLED_FRAMES: u32 = 16;
/// Sampled frames are shrunk to this width in pixels.
const SAMPLE_WIDTH: u32 = 54;
/// File types that count as songs in a songs folder.
pub const SONG_TYPES: [&str; 4] = ["mp3", "wav", "ogg", "flac"];

/// Which parts of the scene each variation changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vary {
    pub balls: bool,
    pub arena: bool,
    pub bounces: bool,
    pub look: bool,
    pub effects: bool,
    pub sound: bool,
    pub song: bool,
    pub instrument: bool,
    pub captions: bool,
}

impl Vary {
    /// Names accepted by [`Vary::parse`], with what each changes.
    pub const NAMES: [(&str, &str); 9] = [
        ("balls", "where the balls start, how many, their size, speed and colours"),
        ("arena", "the arena's size, line, spin and shrinking, and gravity"),
        ("bounces", "speeding up, growing, colour changes and new balls on each bounce"),
        ("look", "background, trails, glow and string art"),
        ("effects", "the sliders of the scene's shader effects"),
        ("sound", "chords and notes"),
        ("song", "which song from the songs folder plays"),
        ("instrument", "the instrument's sound"),
        ("captions", "caption words, colours and animations, or a new hook"),
    ];

    /// Reads a comma-separated list such as `balls,look,song`. `all` turns everything on.
    pub fn parse(list: &str) -> anyhow::Result<Self> {
        let mut vary = Vary::default();
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            match name {
                "all" => {
                    vary = Vary {
                        balls: true,
                        arena: true,
                        bounces: true,
                        look: true,
                        effects: true,
                        sound: true,
                        song: true,
                        instrument: true,
                        captions: true,
                    }
                }
                "balls" => vary.balls = true,
                "arena" => vary.arena = true,
                "bounces" => vary.bounces = true,
                "look" | "colours" | "colors" => vary.look = true,
                "effects" => vary.effects = true,
                "sound" => vary.sound = true,
                "song" | "songs" => vary.song = true,
                "instrument" | "instruments" => vary.instrument = true,
                "captions" | "text" => vary.captions = true,
                other => {
                    bail!("don't know how to vary \"{other}\". Choose from: {}", Self::NAMES.map(|(n, _)| n).join(", "))
                }
            }
        }
        Ok(vary)
    }

    /// The setting groups to roll, in the order the explorer always rolled them.
    pub fn groups(&self, scene: &Scene) -> Vec<Group> {
        let mut groups = Vec::new();
        if self.balls {
            groups.extend((0..scene.spawners.len()).map(Group::Balls));
        }
        if self.arena {
            groups.extend([Group::Shape, Group::Movement, Group::World]);
        }
        if self.bounces {
            groups.extend([Group::Bounces, Group::NewBalls]);
        }
        if self.look {
            groups.extend([Group::Background, Group::Trails, Group::Glow, Group::Strings]);
        }
        if self.sound {
            groups.extend([Group::Chords, Group::Notes]);
        }
        if self.instrument {
            groups.push(Group::Instrument);
        }
        if self.captions {
            groups.push(Group::Captions);
        }
        groups
    }

    pub fn any(&self) -> bool {
        *self != Vary::default()
    }
}

/// Rolls new slider values for an effect, for the dice on an effect card, the explorer and
/// batches. Values land nearer the effect's defaults than its slider ends, which are often
/// extreme, and some sliders are left alone because most values look broken.
pub fn roll_effect(effect: &mut Effect, library: &EffectLibrary, dice: &mut Dice, amount: f32) {
    let Some(info) = library.get(&effect.shader) else { return };
    // Echo's zoom smears anything that doesn't move, like the arena's wall, into a grey disc.
    const KEEP: [&str; 1] = ["zoom"];
    for param in &info.params {
        let rolled = dice.range(param.min, param.max);
        let flip = dice.chance(0.3);
        if KEEP.contains(&param.name.as_str()) {
            continue;
        }
        let current = effect.params.get(&param.name).copied().filter(|v| v.is_finite()).unwrap_or(param.default);
        let value = match param.kind {
            ParamKind::Toggle => {
                if flip && dice.chance(amount) {
                    1.0 - current.round()
                } else {
                    current
                }
            }
            ParamKind::Whole | ParamKind::Slider => {
                let target = param.default + (rolled - param.default) * 0.6;
                let value = current + (target - current) * amount;
                if param.kind == ParamKind::Whole { value.round() } else { value }
            }
        };
        effect.params.insert(param.name.clone(), value.clamp(param.min.min(param.max), param.max.max(param.min)));
    }
}

/// The variation of `base` made from `seed`. The same base, settings and seed always give the
/// same scene, which is what the explorer shows for that seed too.
pub fn variation(
    base: &Scene,
    vary: &Vary,
    seed: u64,
    amount: f32,
    library: &EffectLibrary,
    songs: &[String],
) -> (Scene, Option<String>) {
    let mut scene = base.variation(&vary.groups(base), seed, amount);
    if vary.effects {
        for (index, effect) in scene.effects.iter_mut().enumerate() {
            roll_effect(effect, library, &mut Dice::mixed(seed, 1000 + index as u64), amount);
        }
    }
    let song =
        (vary.song && !songs.is_empty()).then(|| Dice::mixed(seed, 2000).int(0, songs.len() as u32 - 1) as usize);
    let song = song.map(|i| songs[i].clone());
    if let Some(song) = &song {
        apply_song(&mut scene, song);
    }
    // "Day {seed}" becomes "Day 42" in the saved scene, so the video can be made again from it.
    for caption in &mut scene.captions {
        caption.text = caption.text.replace("{seed}", &seed.to_string());
    }
    (scene, song)
}

/// Makes `scene` play `song`. Scenes gain a song track with the music workstream; until then the
/// song is only recorded in the batch.
fn apply_song(_scene: &mut Scene, _song: &str) {}

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
    /// Moved into a "rejected" folder inside the output folder.
    #[default]
    Move,
    Delete,
}

/// Everything that defines a batch. Starting a batch with the same settings in the same folder
/// resumes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchSpec {
    /// The scene the variations are made from, as a file path or a preset's name.
    pub base: String,
    pub vary: Vary,
    /// How different the variations are, 0 to 1.
    pub amount: f32,
    /// How many videos to render.
    pub count: u32,
    /// How many of the rendered videos to keep.
    pub keep: u32,
    /// The first seed; the batch uses the seeds after it.
    pub first_seed: u64,
    /// Folder of songs to pick from when varying the song.
    pub songs: Option<PathBuf>,
    pub quality: Quality,
    pub rejected: Rejected,
    /// Seeds simulated per video rendered, to pick the most promising ones.
    pub prescreen: u32,
    /// Make each video at most this many seconds long (mostly for quick tests).
    pub max_seconds: Option<f32>,
    /// Size of the video compared with the quality's own size, for tests.
    pub scale: f32,
}

impl Default for BatchSpec {
    fn default() -> Self {
        Self {
            base: String::new(),
            vary: Vary { balls: true, arena: true, bounces: true, look: true, ..Vary::default() },
            amount: 0.5,
            count: 50,
            keep: 10,
            first_seed: 1,
            songs: None,
            quality: Quality::Good,
            rejected: Rejected::Move,
            prescreen: 4,
            max_seconds: None,
            scale: 1.0,
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
    /// Rendered and fully scored.
    Rendered,
    Kept,
    Rejected,
    /// Rendering failed; see the job's error.
    Failed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub seed: u64,
    /// The song's file name, when the song is varied.
    pub song: Option<String>,
    /// Score from simulating only.
    pub prescore: Option<Score>,
    /// Score from the rendered video, with frames and sound.
    pub score: Option<Score>,
    pub status: Status,
    /// The video's file name in the output folder (or in `rejected`).
    pub video: Option<String>,
    /// Seconds it took to render.
    pub render_seconds: Option<f32>,
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

/// The record of a batch, saved as `batch.json` in its output folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub spec: BatchSpec,
    /// The base scene as it was when the batch started, so editing the file later doesn't
    /// change a batch that is half done.
    pub base_scene: String,
    pub weights: Weights,
    /// Song file names, from the songs folder, sorted.
    pub songs: Vec<String>,
    pub phase: Phase,
    pub jobs: Vec<Job>,
}

impl Manifest {
    pub fn load(folder: &Path) -> anyhow::Result<Self> {
        let path = folder.join(MANIFEST);
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }

    /// Writes the manifest next to itself first and then swaps it in, so a crash mid-write
    /// never leaves a broken file.
    pub fn save(&self, folder: &Path) -> anyhow::Result<()> {
        let path = folder.join(MANIFEST);
        let temporary = folder.join(format!("{MANIFEST}.new"));
        std::fs::write(&temporary, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    }

    pub fn base(&self) -> anyhow::Result<Scene> {
        Scene::from_ron(&self.base_scene)
    }

    /// Jobs in a given state.
    pub fn count(&self, status: Status) -> usize {
        self.jobs.iter().filter(|j| j.status == status).count()
    }

    /// The kept videos, best first.
    pub fn kept(&self) -> Vec<&Job> {
        let mut kept: Vec<&Job> = self.jobs.iter().filter(|j| j.status == Status::Kept).collect();
        kept.sort_by(|a, b| total(b.score).total_cmp(&total(a.score)));
        kept
    }
}

fn total(score: Option<Score>) -> f32 {
    score.map_or(-1.0, |s| s.total)
}

/// Where a batch is, for progress bars.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    /// Jobs finished in this phase, and how many it has.
    pub done: usize,
    pub total: usize,
    /// The seed being rendered, and how many of its frames are done.
    pub seed: Option<u64>,
    pub frame: (u32, u32),
    /// Rough time left for the whole batch.
    pub eta: Option<Duration>,
}

/// Song files in `folder`, sorted by name.
pub fn list_songs(folder: &Path) -> anyhow::Result<Vec<String>> {
    let mut songs: Vec<String> = std::fs::read_dir(folder)
        .with_context(|| format!("reading the songs folder {}", folder.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|e| SONG_TYPES.iter().any(|t| e.to_string_lossy().eq_ignore_ascii_case(t)))
        })
        .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    songs.sort();
    Ok(songs)
}

/// Starts a batch in `folder`, or resumes the one already there if it has the same settings.
/// A finished batch with the same settings is returned as it is.
pub fn open(folder: &Path, spec: &BatchSpec, base: &Scene, weights: Weights) -> anyhow::Result<Manifest> {
    std::fs::create_dir_all(folder).with_context(|| format!("making the folder {}", folder.display()))?;
    if folder.join(MANIFEST).is_file() {
        let manifest = Manifest::load(folder)?;
        if manifest.spec == *spec {
            return Ok(manifest);
        }
        bail!(
            "{} already holds a different batch. Choose an empty folder, or the same settings to carry on with it",
            folder.display()
        );
    }
    if spec.count == 0 {
        bail!("a batch needs at least one video");
    }
    if spec.keep == 0 {
        bail!("a batch needs to keep at least one video");
    }
    let songs = match (&spec.songs, spec.vary.song) {
        (Some(folder), true) => list_songs(folder)?,
        _ => Vec::new(),
    };
    if spec.vary.song && songs.is_empty() {
        bail!("varying the song needs a songs folder with mp3, wav, ogg or flac files in it");
    }
    let mut base = base.clone();
    if let Some(seconds) = spec.max_seconds {
        base.video.duration = base.video.duration.min(seconds.max(0.1));
    }
    let screened = spec.count.saturating_mul(spec.prescreen.max(1));
    let jobs = (0..screened as u64).map(|i| Job { seed: spec.first_seed.wrapping_add(i), ..Job::default() }).collect();
    let manifest = Manifest {
        version: 1,
        spec: spec.clone(),
        base_scene: base.to_ron()?,
        weights,
        songs,
        phase: Phase::Prescreen,
        jobs,
    };
    manifest.save(folder)?;
    Ok(manifest)
}

/// What a batch needs besides its manifest.
pub struct Runner<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub folder: PathBuf,
    /// Encoder, ffmpeg and effect settings for every video. Size and quality come from the spec.
    pub options: ExportOptions,
    /// Set from another thread to stop after the current step. The batch can be resumed.
    pub cancel: &'a AtomicBool,
}

/// One variation, simulated and mixed, ready to render.
struct Prepared {
    index: usize,
    scene: Scene,
    stats: score::SimStats,
    soundtrack: anyhow::Result<bpe_audio::Soundtrack>,
}

impl Runner<'_> {
    /// Runs the batch in `manifest` to the end (or until cancelled), saving after every step.
    /// `progress` is called often with the manifest and where the batch is.
    pub fn run(&self, manifest: &mut Manifest, mut progress: impl FnMut(&Manifest, &Progress)) -> anyhow::Result<()> {
        // Two programs working on one folder would delete each other's half-made videos. The
        // lock goes away by itself if this program is killed.
        let lock = std::fs::File::create(self.folder.join("batch.lock"))?;
        if lock.try_lock().is_err() {
            bail!("another Ball Studio or bpe-render is already working on the batch in {}", self.folder.display());
        }
        let _awake = KeepAwake::new();
        let base = manifest.base()?;
        if manifest.phase == Phase::Prescreen {
            self.prescreen(manifest, &base, &mut progress)?;
            if self.cancelled() {
                return Ok(());
            }
            choose(manifest);
            manifest.phase = Phase::Render;
            manifest.save(&self.folder)?;
        }
        if manifest.phase == Phase::Render {
            self.render(manifest, &base, &mut progress)?;
            if self.cancelled() {
                return Ok(());
            }
            manifest.phase = Phase::Pick;
            manifest.save(&self.folder)?;
        }
        if manifest.phase == Phase::Pick {
            self.pick(manifest)?;
            manifest.phase = Phase::Done;
            manifest.save(&self.folder)?;
        }
        let total = manifest.jobs.len();
        progress(manifest, &Progress { phase: Phase::Done, done: total, total, ..Progress::default() });
        drop(lock);
        let _ = std::fs::remove_file(self.folder.join("batch.lock"));
        Ok(())
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Simulates and scores every waiting seed on all CPU cores.
    fn prescreen(
        &self,
        manifest: &mut Manifest,
        base: &Scene,
        progress: &mut impl FnMut(&Manifest, &Progress),
    ) -> anyhow::Result<()> {
        let waiting: Vec<usize> = (0..manifest.jobs.len()).filter(|&i| manifest.jobs[i].prescore.is_none()).collect();
        let total = manifest.jobs.len();
        let done_before = total - waiting.len();
        let (sender, receiver) = std::sync::mpsc::channel();
        let next = AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(waiting.len().max(1));
        let spec = manifest.spec.clone();
        let songs = manifest.songs.clone();
        let weights = manifest.weights;
        let seeds: Vec<u64> = manifest.jobs.iter().map(|j| j.seed).collect();
        let library = &self.options.effects;
        std::thread::scope(|scope| -> anyhow::Result<()> {
            for _ in 0..threads {
                let sender = sender.clone();
                let (next, waiting, seeds, spec, songs) = (&next, &waiting, &seeds, &spec, &songs);
                scope.spawn(move || {
                    loop {
                        if self.cancelled() {
                            return;
                        }
                        let at = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&index) = waiting.get(at) else { return };
                        let seed = seeds[index];
                        let (scene, song) = variation(base, &spec.vary, seed, spec.amount, library, songs);
                        let stats = score::simulate(&scene);
                        let parts = score::parts(&scene, &stats, &ScoreInputs::default());
                        if sender.send((index, song, weights.score(parts))).is_err() {
                            return;
                        }
                    }
                });
            }
            drop(sender);
            let mut last_save = Instant::now();
            for (count, (index, song, score)) in receiver.into_iter().enumerate() {
                let job = &mut manifest.jobs[index];
                job.song = song;
                job.prescore = Some(score);
                job.status = Status::Screened;
                if last_save.elapsed() > Duration::from_secs(2) {
                    manifest.save(&self.folder)?;
                    last_save = Instant::now();
                }
                let done = done_before + count + 1;
                progress(manifest, &Progress { phase: Phase::Prescreen, done, total, ..Progress::default() });
            }
            Ok(())
        })?;
        manifest.save(&self.folder)
    }

    /// Renders every chosen job, simulating and mixing the next one while the current one
    /// encodes.
    fn render(
        &self,
        manifest: &mut Manifest,
        base: &Scene,
        progress: &mut impl FnMut(&Manifest, &Progress),
    ) -> anyhow::Result<()> {
        let mut queue: Vec<usize> =
            (0..manifest.jobs.len()).filter(|&i| manifest.jobs[i].status == Status::Chosen).collect();
        queue.sort_by(|&a, &b| total(manifest.jobs[b].prescore).total_cmp(&total(manifest.jobs[a].prescore)));
        let total_jobs = manifest
            .jobs
            .iter()
            .filter(|j| matches!(j.status, Status::Chosen | Status::Rendered | Status::Failed))
            .count();
        let mut options = self.options.clone();
        manifest.spec.quality.apply(&mut options);
        options.scale *= manifest.spec.scale;
        let spec = manifest.spec.clone();
        let songs = manifest.songs.clone();
        let base_dir = options.base_dir.clone();
        let library = options.effects.clone();
        let prepare = move |index: usize, seed: u64| {
            let base = base.clone();
            let (spec, songs, base_dir, library) = (spec.clone(), songs.clone(), base_dir.clone(), library.clone());
            std::thread::spawn(move || {
                let (scene, _) = variation(&base, &spec.vary, seed, spec.amount, &library, &songs);
                let stats = score::simulate(&scene);
                let soundtrack = bpe_audio::render_soundtrack(&scene, &stats.hits, base_dir.as_deref());
                Prepared { index, scene, stats, soundtrack }
            })
        };
        let mut rendered_seconds = Vec::new();
        let mut next = queue.first().map(|&i| prepare(i, manifest.jobs[i].seed));
        for (position, &index) in queue.iter().enumerate() {
            if self.cancelled() {
                break;
            }
            let prepared = next
                .take()
                .context("nothing prepared")?
                .join()
                .map_err(|_| anyhow::anyhow!("preparing a video crashed"))?;
            debug_assert_eq!(prepared.index, index);
            next = queue.get(position + 1).map(|&i| prepare(i, manifest.jobs[i].seed));
            let seed = manifest.jobs[index].seed;
            let name = video_name(&prepared.scene.name, seed);
            let partial = self.folder.join(format!("{name}.partial.mp4"));
            let _ = std::fs::remove_file(&partial);
            options.output = partial.clone();
            let soundtrack = match &prepared.soundtrack {
                Ok(track) => track,
                Err(error) => {
                    fail(&mut manifest.jobs[index], error);
                    manifest.save(&self.folder)?;
                    continue;
                }
            };
            let frame_count =
                options.max_frames.map_or(prepared.scene.frame_count(), |m| m.min(prepared.scene.frame_count()));
            let wanted = sample_frames(frame_count);
            let (width, height) = crate::output_size(&prepared.scene, options.scale);
            let mut frames = Vec::new();
            let done = position + (total_jobs - queue.len());
            let render_started = Instant::now();
            let eta = |rendered: &[f32]| {
                let per_video =
                    if rendered.is_empty() { None } else { Some(rendered.iter().sum::<f32>() / rendered.len() as f32) };
                per_video.map(|s| Duration::from_secs_f32(s * (queue.len() - position) as f32))
            };
            let mut stopped = false;
            let result = export_video_with(
                self.device,
                self.queue,
                &prepared.scene,
                &options,
                Some(soundtrack),
                |frame, of| {
                    if frame % 10 == 0 || frame == of {
                        progress(
                            manifest,
                            &Progress {
                                phase: Phase::Render,
                                done,
                                total: total_jobs,
                                seed: Some(seed),
                                frame: (frame, of),
                                eta: eta(&rendered_seconds).or_else(|| {
                                    // Before the first video is done, guess from this one's pace.
                                    let elapsed = render_started.elapsed().as_secs_f32();
                                    (frame > 30).then(|| {
                                        Duration::from_secs_f32(
                                            elapsed / frame as f32 * of as f32 * (queue.len() - position) as f32,
                                        )
                                    })
                                }),
                            },
                        );
                    }
                    stopped = self.cancelled();
                    !stopped
                },
                |frame, pixels| {
                    // A graphics card encoder that fails starts again on the CPU from frame 0.
                    if frame == 0 {
                        frames.clear();
                    }
                    if wanted.binary_search(&frame).is_ok() {
                        frames.push(SampledFrame::shrink(pixels, width, height, SAMPLE_WIDTH));
                    }
                },
            );
            if stopped {
                let _ = std::fs::remove_file(&partial);
                break;
            }
            let job = &mut manifest.jobs[index];
            match result {
                Ok(encoder) => {
                    // The graphics card encoder failed and the CPU took over: use the CPU for the
                    // rest of the batch rather than failing on every video.
                    if encoder == crate::Encoder::Cpu {
                        options.encoder = crate::Encoder::Cpu;
                    }
                    let video = format!("{name}.mp4");
                    std::fs::rename(&partial, self.folder.join(&video))?;
                    prepared.scene.save(self.folder.join(format!("{name}.ron")))?;
                    let inputs = ScoreInputs { beats: &[], soundtrack: Some(soundtrack), frames: &frames };
                    let parts = score::parts(&prepared.scene, &prepared.stats, &inputs);
                    job.score = Some(manifest.weights.score(parts));
                    job.video = Some(video);
                    job.status = Status::Rendered;
                    let seconds = render_started.elapsed().as_secs_f32();
                    job.render_seconds = Some(seconds);
                    rendered_seconds.push(seconds);
                }
                Err(error) => fail(job, &error),
            }
            manifest.save(&self.folder)?;
            progress(
                manifest,
                &Progress {
                    phase: Phase::Render,
                    done: done + 1,
                    total: total_jobs,
                    // Time for the videos after this one.
                    eta: eta(&rendered_seconds)
                        .filter(|_| position + 1 < queue.len())
                        .map(|e| e.mul_f32((queue.len() - position - 1) as f32 / (queue.len() - position) as f32)),
                    ..Progress::default()
                },
            );
        }
        if let Some(next) = next {
            let _ = next.join();
        }
        Ok(())
    }

    /// Keeps the best rendered videos and moves or deletes the rest.
    fn pick(&self, manifest: &mut Manifest) -> anyhow::Result<()> {
        let mut rendered: Vec<usize> =
            (0..manifest.jobs.len()).filter(|&i| manifest.jobs[i].status == Status::Rendered).collect();
        rendered.sort_by(|&a, &b| {
            total(manifest.jobs[b].score)
                .total_cmp(&total(manifest.jobs[a].score))
                .then(manifest.jobs[a].seed.cmp(&manifest.jobs[b].seed))
        });
        let keep = manifest.spec.keep as usize;
        let rejected_dir = self.folder.join("rejected");
        for (rank, &index) in rendered.iter().enumerate() {
            let job = &mut manifest.jobs[index];
            if rank < keep {
                job.status = Status::Kept;
                continue;
            }
            job.status = Status::Rejected;
            let Some(video) = job.video.clone() else { continue };
            let files = [video.clone(), video.replace(".mp4", ".ron")];
            for file in files {
                let from = self.folder.join(&file);
                if !from.exists() {
                    continue;
                }
                match manifest.spec.rejected {
                    Rejected::Delete => std::fs::remove_file(&from)?,
                    Rejected::Move => {
                        std::fs::create_dir_all(&rejected_dir)?;
                        std::fs::rename(&from, rejected_dir.join(&file))?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Picks the jobs to render: the best pre-screen scores, ties going to the lower seed.
fn choose(manifest: &mut Manifest) {
    if manifest.jobs.iter().any(|j| matches!(j.status, Status::Chosen | Status::Rendered | Status::Failed)) {
        return;
    }
    let mut order: Vec<usize> = (0..manifest.jobs.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&manifest.jobs[a], &manifest.jobs[b]);
        total(b.prescore).total_cmp(&total(a.prescore)).then(a.seed.cmp(&b.seed))
    });
    for (rank, index) in order.into_iter().enumerate() {
        let job = &mut manifest.jobs[index];
        job.status = if rank < manifest.spec.count as usize { Status::Chosen } else { Status::Skipped };
    }
}

fn fail(job: &mut Job, error: &anyhow::Error) {
    job.status = Status::Failed;
    job.error = Some(format!("{error:#}"));
}

/// Frame numbers to sample, spread evenly from the first frame to the last, sorted.
pub fn sample_frames(frame_count: u32) -> Vec<u32> {
    let n = SAMPLED_FRAMES.min(frame_count);
    let mut frames: Vec<u32> = (0..n).map(|i| ((i as f64 + 0.5) / n as f64 * frame_count as f64) as u32).collect();
    frames.dedup();
    frames
}

/// A file name for a variation's video, without extension: the scene's name and the seed.
pub fn video_name(scene_name: &str, seed: u64) -> String {
    let mut slug: String = scene_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        slug = "video".into();
    }
    slug.truncate(40);
    format!("{slug}-seed-{seed}")
}

/// The folder Ball Studio keeps its settings in.
pub fn settings_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Ball Studio"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("ball-studio"))
    }
}

fn weights_file() -> Option<PathBuf> {
    settings_dir().map(|d| d.join("score-weights.json"))
}

/// The score weights the user set or taught with thumbs, or the defaults.
pub fn load_weights() -> Weights {
    weights_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_weights(weights: &Weights) {
    if let Some(file) = weights_file() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(weights) {
            let _ = std::fs::write(file, text);
        }
    }
}

/// Asks Windows not to go to sleep while a batch runs. Does nothing elsewhere.
struct KeepAwake;

#[cfg(windows)]
mod power {
    pub const ES_CONTINUOUS: u32 = 0x8000_0000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn SetThreadExecutionState(flags: u32) -> u32;
    }
}

impl KeepAwake {
    fn new() -> Self {
        #[cfg(windows)]
        // SAFETY: a plain Win32 call with no pointers; it only sets this thread's power request.
        unsafe {
            power::SetThreadExecutionState(power::ES_CONTINUOUS | power::ES_SYSTEM_REQUIRED);
        }
        Self
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: as above; this clears the request again.
        unsafe {
            power::SetThreadExecutionState(power::ES_CONTINUOUS);
        }
    }
}

/// The parts of a score in one line, like "Rhythm 80, Build-up 55", for showing why a video
/// ranked where it did.
pub fn describe(parts: &Parts) -> String {
    parts
        .list()
        .iter()
        .filter_map(|(name, value, _)| value.map(|v| format!("{name} {:.0}", v * 100.0)))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(base: &str) -> BatchSpec {
        BatchSpec {
            base: base.into(),
            count: 3,
            keep: 2,
            prescreen: 2,
            max_seconds: Some(1.0),
            scale: 0.1,
            quality: Quality::Draft,
            ..BatchSpec::default()
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-batch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn vary_lists_parse() {
        let vary = Vary::parse("balls, look,song").unwrap();
        assert!(vary.balls && vary.look && vary.song && !vary.arena);
        assert!(Vary::parse("all").unwrap().captions);
        assert!(Vary::parse("balls,wobble").is_err());
        assert!(!Vary::parse("").unwrap().any());
    }

    #[test]
    fn variations_match_the_explorer_and_fill_in_the_seed() {
        let mut base = Scene::default();
        base.captions.push(bpe_scene::Caption::new("Day {seed}", bpe_scene::CaptionStyle::Hook));
        let vary = Vary { look: true, ..Vary::default() };
        let library = EffectLibrary::new();
        let (scene, song) = variation(&base, &vary, 42, 0.5, &library, &[]);
        assert_eq!(song, None);
        assert_eq!(scene.captions[0].text, "Day 42");
        let explorer = base.variation(&vary.groups(&base), 42, 0.5);
        assert_eq!(scene.look, explorer.look);
        // Songs are picked by seed from the list.
        let songs = vec!["a.mp3".to_string(), "b.mp3".to_string(), "c.mp3".to_string()];
        let vary = Vary { song: true, ..Vary::default() };
        let picks: Vec<_> =
            (0..30).map(|seed| variation(&base, &vary, seed, 0.5, &library, &songs).1.unwrap()).collect();
        assert!(songs.iter().all(|s| picks.contains(s)));
        assert_eq!(
            variation(&base, &vary, 7, 0.5, &library, &songs).1,
            variation(&base, &vary, 7, 0.5, &library, &songs).1
        );
    }

    #[test]
    fn effects_vary_through_the_library() {
        let base = Scene { effects: vec![Effect::new("kaleidoscope")], ..Scene::default() };
        let vary = Vary { effects: true, ..Vary::default() };
        let (scene, _) = variation(&base, &vary, 3, 1.0, &EffectLibrary::new(), &[]);
        assert_ne!(scene.effects, base.effects);
    }

    #[test]
    fn sample_frames_spread_through_the_video() {
        let frames = sample_frames(600);
        assert_eq!(frames.len(), 16);
        assert!(frames[0] < 40 && *frames.last().unwrap() > 560);
        assert!(frames.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(sample_frames(3), vec![0, 1, 2]);
        assert!(sample_frames(0).is_empty());
    }

    #[test]
    fn video_names_are_safe_file_names() {
        assert_eq!(video_name("Escape rings", 7), "escape-rings-seed-7");
        assert_eq!(video_name("a/b\\c: d?", 1), "a-b-c-d-seed-1");
        assert_eq!(video_name("", 3), "video-seed-3");
    }

    #[test]
    fn choosing_takes_the_best_prescores() {
        let mut manifest = Manifest {
            version: 1,
            spec: BatchSpec { count: 2, ..BatchSpec::default() },
            base_scene: String::new(),
            weights: Weights::default(),
            songs: Vec::new(),
            phase: Phase::Prescreen,
            jobs: [0.2, 0.9, 0.5, 0.9]
                .iter()
                .enumerate()
                .map(|(i, &t)| Job {
                    seed: i as u64,
                    prescore: Some(Score { total: t, ..Score::default() }),
                    ..Job::default()
                })
                .collect(),
        };
        choose(&mut manifest);
        let statuses: Vec<Status> = manifest.jobs.iter().map(|j| j.status).collect();
        assert_eq!(statuses, [Status::Skipped, Status::Chosen, Status::Skipped, Status::Chosen]);
    }

    #[test]
    fn opening_a_folder_with_another_batch_is_refused() {
        let dir = temp_dir("refuse");
        let base = Scene::default();
        open(&dir, &spec("a"), &base, Weights::default()).unwrap();
        // Same settings resume.
        assert_eq!(open(&dir, &spec("a"), &base, Weights::default()).unwrap().jobs.len(), 6);
        assert!(open(&dir, &spec("b"), &base, Weights::default()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn song_batches_need_songs() {
        let dir = temp_dir("songs");
        let songs = dir.join("songs");
        std::fs::create_dir_all(&songs).unwrap();
        let spec = BatchSpec { vary: Vary { song: true, ..Vary::default() }, songs: Some(songs.clone()), ..spec("s") };
        assert!(open(&dir.join("out"), &spec, &Scene::default(), Weights::default()).is_err());
        std::fs::write(songs.join("b.mp3"), b"").unwrap();
        std::fs::write(songs.join("a.WAV"), b"").unwrap();
        std::fs::write(songs.join("notes.txt"), b"").unwrap();
        let manifest = open(&dir.join("out"), &spec, &Scene::default(), Weights::default()).unwrap();
        assert_eq!(manifest.songs, ["a.WAV", "b.mp3"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Runs a tiny batch, stops it partway through rendering, and resumes it: the result must be
    /// the same as running it straight through.
    #[test]
    fn a_stopped_batch_resumes_to_the_same_result() {
        let Ok((device, queue)) = bpe_render::headless_device() else {
            assert!(std::env::var_os("BPE_REQUIRE_GPU").is_none(), "no GPU adapter");
            eprintln!("no GPU adapter available, skipping");
            return;
        };
        let options = ExportOptions::new("unused.mp4");
        if Command::new(&options.ffmpeg).arg("-version").output().is_err() {
            eprintln!("no ffmpeg, skipping");
            return;
        }
        let base = Scene::escape_rings();
        let run = |dir: &Path, stop_after: Option<usize>| {
            let cancel = AtomicBool::new(false);
            let mut manifest = open(dir, &spec("rings"), &base, Weights::default()).unwrap();
            let runner = Runner {
                device: &device,
                queue: &queue,
                folder: dir.to_path_buf(),
                options: options.clone(),
                cancel: &cancel,
            };
            runner
                .run(&mut manifest, |m, p| {
                    // Stop in the middle of rendering a video, like a kill would.
                    if let Some(after) = stop_after
                        && m.count(Status::Rendered) >= after
                        && p.frame.0 >= 20
                    {
                        cancel.store(true, Ordering::Relaxed);
                    }
                })
                .unwrap();
            manifest
        };
        let straight = temp_dir("straight");
        let whole = run(&straight, None);
        assert_eq!(whole.phase, Phase::Done);
        assert_eq!(whole.count(Status::Kept), 2);
        assert_eq!(whole.count(Status::Rejected), 1);
        assert_eq!(whole.count(Status::Skipped), 3);
        for job in whole.kept() {
            let video = job.video.as_ref().unwrap();
            assert!(straight.join(video).is_file());
            assert!(straight.join(video.replace(".mp4", ".ron")).is_file());
            let parts = job.score.unwrap().parts;
            assert!(parts.variety.is_some() && parts.sound.is_some());
        }
        assert_eq!(std::fs::read_dir(straight.join("rejected")).unwrap().count(), 2);

        let stopped = temp_dir("stopped");
        let half = run(&stopped, Some(1));
        assert_eq!(half.phase, Phase::Render);
        assert_eq!(half.count(Status::Rendered), 1);
        // No half-written video is left behind.
        assert!(
            std::fs::read_dir(&stopped)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().contains("partial"))
        );
        let resumed = run(&stopped, None);
        assert_eq!(resumed.phase, Phase::Done);
        let seeds = |m: &Manifest| m.kept().iter().map(|j| j.seed).collect::<Vec<_>>();
        assert_eq!(seeds(&resumed), seeds(&whole));
        let scores = |m: &Manifest| m.jobs.iter().map(|j| j.score.map(|s| s.total)).collect::<Vec<_>>();
        assert_eq!(scores(&resumed), scores(&whole));
        let _ = std::fs::remove_dir_all(&straight);
        let _ = std::fs::remove_dir_all(&stopped);
    }

    use std::process::Command;
}
