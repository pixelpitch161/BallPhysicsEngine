//! TEMPORARY probe (not committed).
use bpe_water::probe::Stats;
use bpe_water::scenes;
use bpe_water::{Numbers, WaterGpu};
use glam::Vec2;

#[test]
#[ignore]
fn unrest_by_depth() {
    let (device, queue) = bpe_gfx::test_device().unwrap();
    let gpu = WaterGpu::new(&device, &queue);
    let cell: f32 = std::env::var("CELL").ok().and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let mut run = scenes::still_box(cell, 0.6).start(&gpu, Numbers::from_env_for(cell));
    let layout = run.water.layout();
    let bands = [0.0f32, 12.0, 24.0, 48.0, 96.0, 1e9];
    for t in [2.0f32, 4.0, 6.0, 8.0] {
        run.run_to(t);
        let d = run.water.read();
        let s = Stats::of(&d, &layout);
        let mut sum = [[0.0f64; 2]; 5];
        let mut n = [[0usize; 2]; 5];
        for x in &d {
            let p = layout.to_world(Vec2::from(x.x));
            let depth = p.y - s.top;
            let b = bands.windows(2).position(|w| depth >= w[0] && depth < w[1]).unwrap_or(4);
            let side = usize::from(p.x.abs() > 300.0 - 40.0);
            let v = Vec2::from(x.v) * layout.cell;
            sum[b][side] += f64::from(v.length_squared());
            n[b][side] += 1;
        }
        let cells: Vec<String> = (0..5)
            .map(|b| {
                let r = |k: usize| if n[b][k] == 0 { 0.0 } else { (sum[b][k] / n[b][k] as f64).sqrt() };
                format!("{:.1}/{:.1}", r(0), r(1))
            })
            .collect();
        println!(
            "t {t}: rms {:.2} top {:.1}  by depth (middle/by the walls) 0-12 12-24 24-48 48-96 deeper: {}",
            s.rms,
            s.top,
            cells.join("  ")
        );
    }
}

#[test]
#[ignore]
fn surface_over_time() {
    let (device, queue) = bpe_gfx::test_device().unwrap();
    let gpu = WaterGpu::new(&device, &queue);
    let cell: f32 = std::env::var("CELL").ok().and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let until: f32 = std::env::var("UNTIL").ok().and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let mut run = scenes::still_box(cell, 0.6).start(&gpu, Numbers::from_env_for(cell));
    let layout = run.water.layout();
    let mut t = 0.0f32;
    while t < until {
        t += 0.1;
        run.run_to(t);
        let d = run.water.read();
        let s = Stats::of(&d, &layout);
        // The top in 12 columns 50 px wide, and the mean speed (x, y) of the top 24 px.
        let mut tops = [f32::MAX; 12];
        let (mut vx, mut vy, mut n) = (0.0f64, 0.0f64, 0usize);
        for x in &d {
            let p = layout.to_world(Vec2::from(x.x));
            let k = (((p.x + 300.0) / 50.0).floor() as i32).clamp(0, 11) as usize;
            tops[k] = tops[k].min(p.y);
            if p.y < s.top + 24.0 {
                let v = Vec2::from(x.v) * layout.cell;
                vx += f64::from(v.x);
                vy += f64::from(v.y);
                n += 1;
            }
        }
        let profile: Vec<String> = tops.iter().map(|y| format!("{:.1}", -81.0 - y)).collect();
        println!(
            "t {t:.1} rms {:.1} top layer mean v ({:.1}, {:.1}) tops above -81: {}",
            s.rms,
            vx / n.max(1) as f64,
            vy / n.max(1) as f64,
            profile.join(" ")
        );
    }
}

#[test]
#[ignore]
fn top_layer_waves() {
    let (device, queue) = bpe_gfx::test_device().unwrap();
    let gpu = WaterGpu::new(&device, &queue);
    let cell: f32 = std::env::var("CELL").ok().and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let mut run = scenes::still_box(cell, 0.6).start(&gpu, Numbers::from_env_for(cell));
    let layout = run.water.layout();
    for t in [1.0f32, 2.0, 2.5] {
        run.run_to(t);
        let d = run.water.read();
        let s = Stats::of(&d, &layout);
        let mut sum = [Vec2::ZERO; 12];
        let mut n = [0usize; 12];
        let mut pts = Vec::new();
        for x in &d {
            let p = layout.to_world(Vec2::from(x.x));
            if p.y < s.top + 24.0 {
                let k = (((p.x + 300.0) / 50.0).floor() as i32).clamp(0, 11) as usize;
                let v = Vec2::from(x.v) * layout.cell;
                sum[k] += v;
                n[k] += 1;
                pts.push((k, v));
            }
        }
        let means: Vec<Vec2> = (0..12).map(|k| sum[k] / n[k].max(1) as f32).collect();
        let organised = (means.iter().map(|m| m.length_squared()).sum::<f32>() / 12.0).sqrt();
        let random =
            (pts.iter().map(|(k, v)| (*v - means[*k]).length_squared()).sum::<f32>() / pts.len() as f32).sqrt();
        let cols: Vec<String> = means.iter().map(|m| format!("({:.0},{:.0})", m.x, m.y)).collect();
        println!("t {t}: top layer column means rms {organised:.1}, about them {random:.1}: {}", cols.join(" "));
    }
}

#[test]
#[ignore]
fn fullness_by_depth() {
    let (device, queue) = bpe_gfx::test_device().unwrap();
    let gpu = WaterGpu::new(&device, &queue);
    let cell: f32 = std::env::var("CELL").ok().and_then(|s| s.parse().ok()).unwrap_or(6.0);
    let mut run = scenes::still_box(cell, 0.6).start(&gpu, Numbers::from_env_for(cell));
    let layout = run.water.layout();
    let bands = [0.0f32, 6.0, 12.0, 18.0, 24.0, 30.0, 36.0, 42.0, 48.0, 72.0, 96.0, 200.0, 1e9];
    for t in [0.3f32, 1.0] {
        run.run_to(t);
        let d = run.water.read();
        let s = Stats::of(&d, &layout);
        let mut sum = vec![0.0f64; bands.len() - 1];
        let mut n = vec![0usize; bands.len() - 1];
        for x in &d {
            let p = layout.to_world(Vec2::from(x.x));
            if p.x.abs() > 200.0 {
                continue;
            }
            let depth = p.y - s.top;
            if let Some(b) = bands.windows(2).position(|w| depth >= w[0] && depth < w[1]) {
                sum[b] += f64::from(x.c[0]);
                n[b] += 1;
            }
        }
        let row: Vec<String> = (0..sum.len())
            .map(|b| format!("{}-{}: {:.3}", bands[b], bands[b + 1].min(999.0), sum[b] / n[b].max(1) as f64))
            .collect();
        println!("t {t} cell {cell}: fullness by depth {}", row.join("  "));
    }
}

/// Task 8: a thin pour at 2 px with the strongest skin at `MOST` steps a frame (with gravity, then
/// none): the highest a drop gets above the spout and the fastest sideways, frame by frame (the
/// worst over each 5 frames). `BPE_WATER_BEND_STEPS` sets how many steps a frame the bend is on.
#[test]
#[ignore]
fn pour_calm() {
    let (device, queue) = bpe_gfx::test_device().unwrap();
    let gpu = WaterGpu::new(&device, &queue);
    let most: u32 = std::env::var("MOST").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
    let frames: u32 = std::env::var("FRAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    for zero_g in [false, true] {
        let mut scene = scenes::thin_pour(2.0, 7.0);
        for feel in &mut scene.feels {
            feel.surface_pull = 1.0;
        }
        if zero_g {
            scene.gravity = Vec2::ZERO;
        }
        let pour = scene.pours[0];
        let numbers = Numbers { floor: 1024, most_steps: most, ..Numbers::from_env_for(2.0) };
        let mut run = scene.start(&gpu, numbers);
        let (mut up, mut side) = (f32::MIN, 0.0f32);
        for frame in 1..=frames {
            run.frame();
            let layout = run.water.layout();
            for d in run.water.read() {
                let at = layout.to_world(Vec2::from(d.x));
                up = up.max(pour.place.y - at.y);
                side = side.max(d.v[0].abs() * layout.cell / pour.speed);
            }
            if frame % 5 == 0 {
                println!(
                    "most {most} zero_g {zero_g} frame {frame}: steps {} drops {} highest above the spout {up:.1} px, fastest sideways {side:.2} of the pour's speed",
                    run.water.steps_last_frame(),
                    run.water.count()
                );
                (up, side) = (f32::MIN, 0.0);
            }
        }
    }
}
