//! Command-line renderer: `bpe-render scene.ron out.mp4`, without opening the editor.

use anyhow::{Context, bail};
use bpe_export::{ExportOptions, export_still, export_video};
use bpe_scene::Scene;
use std::io::Write;
use std::path::PathBuf;

const USAGE: &str = "\
Usage:
  bpe-render <scene.ron | preset> <out.mp4> [options]
  bpe-render <scene.ron | preset> --still <seconds> <out.png> [--scale F]
  bpe-render <scene.ron | preset> --wav <out.wav>
  bpe-render <preset> --save-scene <out.ron>
  bpe-render <scene.ron | preset | all> --profile [--scale F] [--gpu-frames N]
  bpe-render batch <scene.ron | preset> --out <folder> [batch options]

A preset is a built-in scene named in lower case with dashes, such as escape-rings or plinko.
`default` is the reference reel. `bpe-render --presets` lists them all.

Options:
  --scale F        render at a fraction of the scene's size, for quick drafts
  --frames N       render only the first N frames
  --crf N          H.264 quality, lower is better (default 18)
  --preset NAME    x264 speed preset (default medium)
  --ffmpeg PATH    ffmpeg executable (default: found automatically)
  --encoder NAME   auto (default), cpu, nvidia, amd or intel. auto uses the graphics card's
                   encoder when ffmpeg can, which is much faster, and the CPU otherwise
  --profile        time the simulation, sound, frames and drawing, and print a fingerprint
                   of the result (`all` profiles every preset plus some extra-heavy scenes)
  --gpu-frames N   frames to draw on the graphics card when profiling (default 30, 0 skips)

Batch options (render many random variations and keep the best; run the same command again
to carry on with a batch that was stopped):
  --out DIR        folder for the videos and the batch record (required)
  --count N        videos to render (default 50)
  --keep N         best videos to keep (default 10)
  --vary LIST      what to change, comma-separated (default balls,arena,bounces,look):
                   balls, arena, bounces, look, effects, sound, song, instrument, captions, all
  --amount F       how different the variations are, 0 to 1 (default 0.5)
  --songs DIR      folder of songs to pick from when varying the song
  --quality Q      draft, good or best (default good)
  --seed N         first seed (default 1)
  --prescreen N    seeds simulated per video rendered, to find the best (default 4)
  --rejected WHAT  move (default) or delete the videos that don't make the cut
  --seconds S      make each video at most S seconds long
  --scale F        size compared with the quality's size
  --encoder NAME   as above";

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if args[0] == "--presets" {
        for preset in Scene::presets() {
            println!("{:22} {}", slug(preset.name), preset.description);
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--profile") {
        return profile(&args);
    }
    if args[0] == "batch" {
        return batch(&args[1..]);
    }
    let preset = Scene::presets().into_iter().find(|p| slug(p.name) == args[0]);
    let (scene, base_dir) = if args[0] == "default" {
        (Scene::default(), None)
    } else if let Some(preset) = preset {
        (preset.scene, None)
    } else {
        let path = PathBuf::from(&args[0]);
        let scene = Scene::load(&path).with_context(|| format!("loading {}", path.display()))?;
        (scene, path.parent().map(PathBuf::from))
    };

    let mut options = ExportOptions::new("out.mp4");
    options.base_dir = base_dir.clone();
    options.effects.set_folders(bpe_render::effects::default_folders(base_dir.as_deref()));
    options.effects.sync_graphs(&scene.shader_graphs);
    for info in options.effects.list() {
        let used = scene.effects.iter().any(|e| e.enabled && e.shader == info.id);
        if let (true, Some(error)) = (used, info.error()) {
            eprintln!("warning: effect {} is skipped because it has an error:\n{error}", info.id);
        }
    }
    for effect in scene.effects.iter().filter(|e| e.enabled) {
        if options.effects.get(&effect.shader).is_none() {
            eprintln!("warning: effect {} not found, skipping it", effect.shader);
        }
    }
    let mut output = None;
    let mut still = None;
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        let mut value = || rest.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--scale" => options.scale = value()?.parse()?,
            "--frames" => options.max_frames = Some(value()?.parse()?),
            "--crf" => options.crf = value()?.parse()?,
            "--preset" => options.preset = value()?.clone(),
            "--ffmpeg" => options.ffmpeg = value()?.into(),
            "--encoder" => {
                let name = value()?;
                options.encoder = bpe_export::Encoder::from_name(name)
                    .with_context(|| format!("unknown encoder {name}: use auto, cpu, nvidia, amd or intel"))?;
            }
            "--still" => still = Some(value()?.parse::<f64>()?),
            "--save-scene" => {
                scene.save(value()?)?;
                return Ok(());
            }
            "--wav" => {
                let sound = bpe_audio::render_scene(&scene, base_dir.as_deref())?;
                if let Some(error) = sound.song_error {
                    bail!("the song can't be used: {error}");
                }
                sound.mix.track.write_wav(value()?)?;
                return Ok(());
            }
            other if other.starts_with("--") => bail!("unknown option {other}\n\n{USAGE}"),
            other => output = Some(PathBuf::from(other)),
        }
    }
    let output = output.context("missing output path")?;
    let (device, queue) = bpe_render::headless_device().context("no usable GPU found")?;

    if let Some(time) = still {
        return export_still(
            &device,
            &queue,
            &scene,
            &options.effects,
            time,
            options.scale,
            &options.ffmpeg,
            &output,
            base_dir.as_deref(),
        );
    }

    options.output = output;
    let started = std::time::Instant::now();
    let encoder = export_video(&device, &queue, &scene, &options, |done, total| {
        if done % 30 == 0 || done == total {
            print!("\rframe {done}/{total}");
            let _ = std::io::stdout().flush();
        }
        true
    })?;
    println!(
        "\nwrote {} in {:.1} s, encoded with the {}",
        options.output.display(),
        started.elapsed().as_secs_f32(),
        encoder.name()
    );
    Ok(())
}

fn profile(args: &[String]) -> anyhow::Result<()> {
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
    let scale: f32 = value("--scale").map_or(Ok(0.5), |v| v.parse())?;
    let gpu_frames: u32 = value("--gpu-frames").map_or(Ok(30), |v| v.parse())?;
    let scenes = if args[0] == "all" {
        bpe_export::profile::heavy_scenes()
    } else if args[0] == "default" {
        vec![("Reference reel".to_string(), Scene::default())]
    } else if let Some(preset) = Scene::presets().into_iter().find(|p| slug(p.name) == args[0]) {
        vec![(preset.name.to_string(), preset.scene)]
    } else {
        let path = PathBuf::from(&args[0]);
        vec![(args[0].clone(), Scene::load(&path).with_context(|| format!("loading {}", path.display()))?)]
    };
    let device = if gpu_frames > 0 { bpe_render::headless_device().ok() } else { None };
    if gpu_frames > 0 && device.is_none() {
        eprintln!("no GPU found, skipping drawing");
    }
    println!("{}", bpe_export::profile::Report::header());
    for (name, scene) in scenes {
        let gpu = device.as_ref().map(|(d, q)| (d, q));
        let report = bpe_export::profile::profile_scene(&name, &scene, gpu, scale, gpu_frames)?;
        println!("{}", report.row());
    }
    Ok(())
}

fn batch(args: &[String]) -> anyhow::Result<()> {
    use bpe_export::batch::{self, BatchSpec, Phase, Quality, Rejected, Status, Vary};
    let base = args.first().filter(|a| !a.starts_with("--")).context("batch needs a scene file or preset")?;
    let (scene, base_dir) = load_scene(base)?;
    let mut spec = BatchSpec { base: base.clone(), ..BatchSpec::default() };
    let mut options = ExportOptions::new("unused.mp4");
    options.base_dir = base_dir.clone();
    options.effects.set_folders(bpe_render::effects::default_folders(base_dir.as_deref()));
    let mut folder = None;
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        let mut value = || rest.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => folder = Some(PathBuf::from(value()?)),
            "--count" => spec.count = value()?.parse()?,
            "--keep" => spec.keep = value()?.parse()?,
            "--vary" => spec.vary = Vary::parse(value()?)?,
            "--amount" => spec.amount = value()?.parse::<f32>()?.clamp(0.0, 1.0),
            "--songs" => spec.songs = Some(PathBuf::from(value()?)),
            "--quality" => {
                let name = value()?;
                spec.quality =
                    Quality::parse(name).with_context(|| format!("unknown quality {name}: use draft, good or best"))?;
            }
            "--seed" => spec.first_seed = value()?.parse()?,
            "--prescreen" => spec.prescreen = value()?.parse::<u32>()?.max(1),
            "--rejected" => {
                spec.rejected = match value()?.as_str() {
                    "move" => Rejected::Move,
                    "delete" => Rejected::Delete,
                    other => bail!("unknown --rejected {other}: use move or delete"),
                }
            }
            "--seconds" => spec.max_seconds = Some(value()?.parse()?),
            "--scale" => spec.scale = value()?.parse()?,
            "--ffmpeg" => options.ffmpeg = value()?.into(),
            "--encoder" => {
                let name = value()?;
                options.encoder = bpe_export::Encoder::from_name(name)
                    .with_context(|| format!("unknown encoder {name}: use auto, cpu, nvidia, amd or intel"))?;
            }
            other => bail!("unknown batch option {other}\n\n{USAGE}"),
        }
    }
    let folder = folder.context("batch needs --out <folder>")?;
    if !spec.vary.any() {
        bail!("--vary needs at least one thing to change");
    }
    let weights = batch::load_weights();
    let mut manifest = batch::open(&folder, &spec, &scene, weights)?;
    if manifest.phase == Phase::Done {
        println!("This batch is already finished.");
    } else if manifest.jobs.iter().any(|j| j.status != Status::Waiting) {
        println!("Carrying on with the batch in {}.", folder.display());
    }
    let (device, queue) = bpe_render::headless_device().context("no usable GPU found")?;
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let runner = batch::Runner { device: &device, queue: &queue, folder: folder.clone(), options, cancel: &cancel };
    let started = std::time::Instant::now();
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    runner.run(&mut manifest, |_, progress| {
        if last.elapsed().as_millis() < 250 && progress.done < progress.total {
            return;
        }
        last = std::time::Instant::now();
        let eta = progress.eta.map_or(String::new(), |eta| format!(", about {} left", minutes(eta.as_secs())));
        let line = match progress.phase {
            Phase::Prescreen => format!("Trying seeds: {}/{}", progress.done, progress.total),
            Phase::Render => match progress.seed {
                Some(seed) => format!(
                    "Rendering video {}/{} (seed {seed}): frame {}/{}{eta}",
                    progress.done + 1,
                    progress.total,
                    progress.frame.0,
                    progress.frame.1
                ),
                None => format!("Rendered {}/{}{eta}", progress.done, progress.total),
            },
            Phase::Pick | Phase::Done => "Picking the best".into(),
        };
        print!("\r{line:<78}");
        let _ = std::io::stdout().flush();
    })?;
    println!();
    let kept = manifest.kept();
    if manifest.phase == Phase::Done {
        println!("Kept {} videos in {} ({}):", kept.len(), folder.display(), minutes(started.elapsed().as_secs()));
        for job in kept {
            let score = job.score.unwrap_or_default();
            println!(
                "  {:5.1}  {}  ({})",
                score.total * 100.0,
                job.video.as_deref().unwrap_or("?"),
                batch::describe(&score.parts)
            );
        }
        let failed: Vec<_> = manifest.jobs.iter().filter(|j| j.status == Status::Failed).collect();
        for job in failed {
            println!("  seed {} failed: {}", job.seed, job.error.as_deref().unwrap_or("?"));
        }
    }
    Ok(())
}

fn minutes(seconds: u64) -> String {
    if seconds < 90 { format!("{seconds} s") } else { format!("{} min", seconds.div_ceil(60)) }
}

/// A scene from a file or a preset's name, with the folder its relative paths start from.
fn load_scene(name: &str) -> anyhow::Result<(Scene, Option<PathBuf>)> {
    if name == "default" {
        return Ok((Scene::default(), None));
    }
    if let Some(preset) = Scene::presets().into_iter().find(|p| slug(p.name) == name) {
        return Ok((preset.scene, None));
    }
    let path = PathBuf::from(name);
    let scene = Scene::load(&path).with_context(|| format!("loading {}", path.display()))?;
    Ok((scene, path.parent().map(PathBuf::from)))
}

/// A preset's name as typed on the command line: "Escape rings" is `escape-rings`.
fn slug(name: &str) -> String {
    // Brackets and other marks are left out: "Escape and multiply (rules)" is escape-and-multiply-rules.
    let plain: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    plain.split_whitespace().collect::<Vec<_>>().join("-")
}
