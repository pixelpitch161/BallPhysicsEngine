//! Command-line renderer for the modular studio: `bpe-render-next scene.ron out.mp4`.

use anyhow::{Context, bail};
use bpe_cli::{Encoder, ExportOptions, export_video};
use std::io::Write;
use std::path::PathBuf;

const USAGE: &str = "\
Usage:
  bpe-render-next <scene.ron> [out.mp4] [options]
  bpe-render-next --new <out.mp4> [options]

Without an output name the video is written next to the scene file, with the same name.
`--new` renders the scene a brand-new document starts with: a circle arena and one ball.

Options:
  --scale F        render at a fraction of the scene's size, for quick drafts
  --frames N       render only the first N frames
  --crf N          H.264 quality, lower is better (default 18)
  --preset NAME    x264 speed preset (default medium)
  --ffmpeg PATH    ffmpeg executable (default: found automatically)
  --encoder NAME   auto (default), cpu, nvidia, amd or intel. auto uses the graphics card's
                   encoder when ffmpeg can, which is much faster, and the CPU otherwise";

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

    let mut i = 0;
    let value = |i: &mut usize, flag: &str| -> anyhow::Result<String> {
        *i += 1;
        args.get(*i).cloned().with_context(|| format!("{flag} needs a value\n\n{USAGE}"))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--new" => new_scene = true,
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
        bpe_kit::new_scene(&kit.registry)
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
    if let Some(ffmpeg) = ffmpeg {
        options.ffmpeg = ffmpeg;
    }

    let (device, queue) = bpe_gfx::headless_device().context("no graphics device for rendering")?;
    let started = std::time::Instant::now();
    let used = export_video(&device, &queue, &scene, &kit, &options, |done, total| {
        if done % 60 == 0 || done == total {
            print!("\r{done}/{total} frames");
            let _ = std::io::stdout().flush();
        }
        true
    })?;
    println!("\nwrote {} with the {} in {:.1} s", output.display(), used.name(), started.elapsed().as_secs_f32());
    Ok(())
}
