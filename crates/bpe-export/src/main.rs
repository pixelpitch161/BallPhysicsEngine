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
  --gpu-frames N   frames to draw on the graphics card when profiling (default 30, 0 skips)";

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
        return export_still(&device, &queue, &scene, &options.effects, time, options.scale, &options.ffmpeg, &output);
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

/// A preset's name as typed on the command line: "Escape rings" is `escape-rings`.
fn slug(name: &str) -> String {
    name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
}
