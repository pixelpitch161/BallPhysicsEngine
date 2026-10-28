//! Command-line renderer: `bpe-render scene.ron out.mp4`, without opening the editor.

use anyhow::{Context, bail};
use bpe_export::{ExportOptions, export_still, export_video};
use bpe_scene::Scene;
use std::io::Write;
use std::path::PathBuf;

const USAGE: &str = "\
Usage:
  bpe-render <scene.ron | default | growing-ball> <out.mp4> [options]
  bpe-render <scene.ron | default | growing-ball> --still <seconds> <out.png> [--scale F]
  bpe-render <scene.ron | default | growing-ball> --wav <out.wav>
  bpe-render default --save-scene <out.ron>

Options:
  --scale F        render at a fraction of the scene's size, for quick drafts
  --frames N       render only the first N frames
  --crf N          H.264 quality, lower is better (default 18)
  --preset NAME    x264 speed preset (default medium)
  --ffmpeg PATH    ffmpeg executable (default: ffmpeg on PATH)";

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
    let (scene, base_dir) = if args[0] == "default" {
        (Scene::default(), None)
    } else if args[0] == "growing-ball" {
        (Scene::growing_ball(), None)
    } else {
        let path = PathBuf::from(&args[0]);
        let scene = Scene::load(&path).with_context(|| format!("loading {}", path.display()))?;
        (scene, path.parent().map(PathBuf::from))
    };

    let mut options = ExportOptions::new("out.mp4");
    options.base_dir = base_dir.clone();
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
            "--still" => still = Some(value()?.parse::<f64>()?),
            "--save-scene" => {
                scene.save(value()?)?;
                return Ok(());
            }
            "--wav" => {
                let hits = bpe_sim::collect_wall_hits(&scene);
                bpe_audio::render_soundtrack(&scene, &hits, base_dir.as_deref())?.write_wav(value()?)?;
                return Ok(());
            }
            other if other.starts_with("--") => bail!("unknown option {other}\n\n{USAGE}"),
            other => output = Some(PathBuf::from(other)),
        }
    }
    let output = output.context("missing output path")?;
    let (device, queue) = bpe_render::headless_device().context("no usable GPU found")?;

    if let Some(time) = still {
        return export_still(&device, &queue, &scene, time, options.scale, &options.ffmpeg, &output);
    }

    options.output = output;
    let started = std::time::Instant::now();
    export_video(&device, &queue, &scene, &options, |done, total| {
        if done % 30 == 0 || done == total {
            print!("\rframe {done}/{total}");
            let _ = std::io::stdout().flush();
        }
        true
    })?;
    println!("\nwrote {} in {:.1} s", options.output.display(), started.elapsed().as_secs_f32());
    Ok(())
}
