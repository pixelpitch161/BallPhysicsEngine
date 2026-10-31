//! Command-line renderer for the modular studio: `bpe-render-next scene.ron out.mp4`.

use anyhow::{Context, bail};
use bpe_cli::{Encoder, ExportOptions, export_video_reporting};
use std::io::Write;
use std::path::PathBuf;

const USAGE: &str = "\
Usage:
  bpe-render-next <scene.ron> [out.mp4] [options]
  bpe-render-next --new <out.mp4> [options]
  bpe-render-next batch <scene.ron> <folder> [batch options]

Without an output name the video is written next to the scene file, with the same name.
`--new` renders the scene a brand-new document starts with: a circle arena and one ball.

Options:
  --scale F        render at a fraction of the scene's size, for quick drafts
  --frames N       render only the first N frames
  --midi           also save the notes and chords as <out>.mid
  --stems          also save each sound, bus and the song as WAVs in <out>-stems/
  --wet-stems      stems include each sound's sends to the buses
  --crf N          H.264 quality, lower is better (default 18)
  --preset NAME    x264 speed preset (default medium)
  --ffmpeg PATH    ffmpeg executable (default: found automatically)
  --encoder NAME   auto (default), cpu, nvidia, amd or intel. auto uses the graphics card's
                   encoder when ffmpeg can, which is much faster, and the CPU otherwise

Batch options (many seeds of one scene, the best kept; a stopped batch carries on):
  --count N        videos to render (default 50)
  --keep N         videos to keep (default 10)
  --seed N         the first seed (default 1)
  --candidates N   seeds tried per video rendered (default 4)
  --amount F       how far the variations go, 0 to 1 (default 0.5)
  --quality NAME   draft, good (default) or best
  --delete-rejected  delete the videos that didn't make the cut instead of moving them
  --midi, --stems, --wet-stems  as for a single video, for every video kept
  --ffmpeg PATH    ffmpeg executable (default: found automatically)";

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
    if args[0] == "batch" {
        return run_batch(&args[1..]);
    }
    let kit = bpe_kit::kit();
    let mut scene_path: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut new_scene = false;
    let mut scale = 1.0f32;
    let mut frames = None;
    let mut crf = 18u8;
    let mut preset = String::from("medium");
    let mut ffmpeg = None;
    let mut encoder = Encoder::Auto;
    let mut extras = bpe_kit::Extras::default();

    let mut i = 0;
    let value = |i: &mut usize, flag: &str| -> anyhow::Result<String> {
        *i += 1;
        args.get(*i).cloned().with_context(|| format!("{flag} needs a value\n\n{USAGE}"))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--new" => new_scene = true,
            flag if bpe_cli::extras_flag(flag, &mut extras) => {}
            "--scale" => scale = value(&mut i, "--scale")?.parse().context("--scale needs a number")?,
            "--frames" => frames = Some(value(&mut i, "--frames")?.parse().context("--frames needs a whole number")?),
            "--crf" => crf = value(&mut i, "--crf")?.parse().context("--crf needs a number from 0 to 51")?,
            "--preset" => preset = value(&mut i, "--preset")?,
            "--ffmpeg" => ffmpeg = Some(PathBuf::from(value(&mut i, "--ffmpeg")?)),
            "--encoder" => {
                let name = value(&mut i, "--encoder")?;
                encoder = Encoder::from_name(&name)
                    .with_context(|| format!("unknown encoder {name}: use auto, cpu, nvidia, amd or intel"))?;
            }
            flag if flag.starts_with("--") => bail!("unknown option {flag}\n\n{USAGE}"),
            path => {
                if !new_scene && scene_path.is_none() {
                    scene_path = Some(path.into());
                } else if output.is_none() {
                    output = Some(path.into());
                } else {
                    bail!("too many arguments\n\n{USAGE}");
                }
            }
        }
        i += 1;
    }

    let scene = if new_scene {
        bpe_kit::new_studio_scene(&kit.registry)
    } else {
        let path = scene_path.as_ref().with_context(|| format!("no scene file given\n\n{USAGE}"))?;
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        bpe_core::load(&text, &kit.registry).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
    };
    let output = match (output, &scene_path) {
        (Some(o), _) => o,
        (None, Some(path)) => path.with_extension("mp4"),
        (None, None) => bail!("--new needs an output file\n\n{USAGE}"),
    };

    let mut options = ExportOptions::new(output.clone());
    options.scale = scale;
    options.max_frames = frames;
    options.crf = crf;
    options.preset = preset;
    options.encoder = encoder;
    options.extras = extras;
    if let Some(ffmpeg) = ffmpeg {
        options.ffmpeg = ffmpeg;
    }

    let (device, queue) = bpe_gfx::headless_device().context("no graphics device for rendering")?;
    let started = std::time::Instant::now();
    let report = export_video_reporting(&device, &queue, &scene, &kit, &options, |done, total| {
        if done % 60 == 0 || done == total {
            print!("\r{done}/{total} frames");
            let _ = std::io::stdout().flush();
        }
        true
    })?;
    println!(
        "\nwrote {} with the {} in {:.1} s",
        output.display(),
        report.encoder.name(),
        started.elapsed().as_secs_f32()
    );
    for path in &report.extras {
        println!("also wrote {}", path.display());
    }
    Ok(())
}

fn load_scene(path: &std::path::Path, kit: &bpe_kit::Kit) -> anyhow::Result<bpe_core::Scene> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    bpe_core::load(&text, &kit.registry).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

fn run_batch(args: &[String]) -> anyhow::Result<()> {
    use bpe_cli::batch::{self, BatchSpec, Phase, Quality, Rejected};
    let kit = bpe_kit::kit();
    let mut spec = BatchSpec::default();
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut ffmpeg = None;
    let mut i = 0;
    let value = |i: &mut usize, flag: &str| -> anyhow::Result<String> {
        *i += 1;
        args.get(*i).cloned().with_context(|| format!("{flag} needs a value\n\n{USAGE}"))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--count" => spec.count = value(&mut i, "--count")?.parse().context("--count needs a whole number")?,
            "--keep" => spec.keep = value(&mut i, "--keep")?.parse().context("--keep needs a whole number")?,
            "--seed" => spec.first_seed = value(&mut i, "--seed")?.parse().context("--seed needs a whole number")?,
            "--candidates" => {
                spec.prescreen = value(&mut i, "--candidates")?.parse().context("--candidates needs a whole number")?
            }
            "--amount" => spec.amount = value(&mut i, "--amount")?.parse().context("--amount needs a number")?,
            "--quality" => {
                let name = value(&mut i, "--quality")?;
                spec.quality = Quality::parse(&name)
                    .with_context(|| format!("unknown quality {name}: use draft, good or best"))?;
            }
            "--delete-rejected" => spec.rejected = Rejected::Delete,
            flag if bpe_cli::extras_flag(flag, &mut spec.extras) => {}
            "--ffmpeg" => ffmpeg = Some(PathBuf::from(value(&mut i, "--ffmpeg")?)),
            flag if flag.starts_with("--") => bail!("unknown option {flag}\n\n{USAGE}"),
            path => paths.push(path.into()),
        }
        i += 1;
    }
    let [scene_path, folder] = paths.as_slice() else { bail!("batch needs a scene file and a folder\n\n{USAGE}") };
    let scene = load_scene(scene_path, &kit)?;
    let ffmpeg = ffmpeg.or_else(bpe_cli::find_ffmpeg).context("ffmpeg not found: install it or pass --ffmpeg")?;
    let (device, queue) = bpe_gfx::headless_device().context("no graphics device for rendering")?;
    let mut manifest = batch::open(folder, &spec, &scene)?;
    let started = std::time::Instant::now();
    let mut last_phase = None;
    batch::run(folder, &mut manifest, &kit, &device, &queue, &ffmpeg, &bpe_kit::score::Parts::WEIGHTS, &mut |m, p| {
        if last_phase != Some(p.phase) {
            last_phase = Some(p.phase);
            println!("{}", p.phase.word());
        }
        if p.phase == Phase::Render && p.frames > 0 {
            print!("\rvideo {}/{}: {}/{} frames", p.done + 1, p.total, p.frame, p.frames);
            let _ = std::io::stdout().flush();
        } else if p.phase != Phase::Done {
            print!("\r{}/{}", p.done, p.total);
            let _ = std::io::stdout().flush();
        }
        if p.phase == Phase::Done {
            println!();
            for job in &m.jobs {
                let score = job.score.map_or(String::from("-"), |s| format!("{:.0}", s * 100.0));
                let file = job.file.as_ref().map_or(String::new(), |f| format!("  {}", f.display()));
                println!("seed {:>6}  {:<9} score {score:>3}{file}", job.seed, job.status.word());
            }
        }
        true
    })?;
    let kept = manifest.count(batch::Status::Kept);
    println!("kept {kept} video(s) in {} after {:.1} s", folder.display(), started.elapsed().as_secs_f32());
    Ok(())
}
