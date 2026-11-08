//! Tests of the swarm step on the software GPU; skipped without a graphics adapter.

use super::*;

const DT: f32 = 1.0 / 120.0;

fn gpu() -> Option<Arc<SwarmGpu>> {
    match crate::test_device() {
        Ok((device, queue)) => Some(SwarmGpu::for_device(&device, &queue)),
        Err(_) => {
            eprintln!("no GPU adapter available, skipping");
            None
        }
    }
}

fn plain_step(now: u32, cap: u32) -> StepGpu {
    StepGpu { counts: [now, 0, cap, 0], time: [now as f32 * DT, DT, 1.0e9, 0.0], ..StepGpu::default() }
}

fn speck(i: u32, born: u32, life: f32) -> (SpeckMain, SpeckMore) {
    (
        SpeckMain { place: [i as f32, 0.0], velocity: [0.0, 0.0], born, life, seed: i, turn: 0.0 },
        SpeckMore { size: 1.0, ball: i, ..SpeckMore::default() },
    )
}

#[test]
fn births_go_after_the_living_in_order_and_most_particles_caps_them() {
    let Some(gpu) = gpu() else { return };
    let mut swarm = SwarmBuffers::new(gpu.clone());
    let first: Vec<_> = (0..10).map(|i| speck(i, 0, 100.0)).collect();
    swarm.step(&StepIn { step: plain_step(0, 25), words: &[], births: &first });
    let next: Vec<_> = (10..30).map(|i| speck(i, 1, 100.0)).collect();
    swarm.step(&StepIn { step: plain_step(1, 25), words: &[], births: &next });
    let got = swarm.read();
    assert_eq!(got.len(), 25);
    for (k, (_, more)) in got.iter().enumerate() {
        assert_eq!(more.ball, k as u32, "oldest first");
    }
}

#[test]
fn the_dead_go_and_the_rest_keep_their_order_across_many_workgroups() {
    let Some(gpu) = gpu() else { return };
    let mut swarm = SwarmBuffers::new(gpu);
    let n = 300_000u32;
    // Every third particle lives one step, the rest long.
    let births: Vec<_> = (0..n).map(|i| speck(i, 0, if i % 3 == 1 { DT * 0.5 } else { 100.0 })).collect();
    swarm.step(&StepIn { step: plain_step(0, MOST_SWARM), words: &[], births: &births });
    swarm.step(&StepIn { step: plain_step(1, MOST_SWARM), words: &[], births: &[] });
    let got = swarm.read();
    let want: Vec<u32> = (0..n).filter(|i| i % 3 != 1).collect();
    assert_eq!(got.len(), want.len());
    assert!(got.iter().map(|(_, m)| m.ball).eq(want.iter().copied()), "kept in order");
}

#[test]
fn gravity_moves_them_by_semi_implicit_euler() {
    let Some(gpu) = gpu() else { return };
    let mut swarm = SwarmBuffers::new(gpu);
    let mut step = plain_step(0, 100);
    step.more[0] = 1;
    step.forces[0] = ForceGpu {
        kind: [FORCE_GRAVITY, 0, 0, 0],
        a: [1.0, 0.0, 0.0, 0.0],
        b: [0.0, 900.0, 0.0, 0.0],
        ..ForceGpu::default()
    };
    let births = vec![speck(0, 0, 100.0)];
    let (mut p, mut v) = (0.0f32, 0.0f32);
    for s in 0..120u32 {
        step.counts[0] = s;
        step.time[0] = s as f32 * DT;
        swarm.step(&StepIn { step, words: &[], births: if s == 0 { &births } else { &[] } });
        v += 900.0 * DT;
        p += v * DT;
    }
    let got = swarm.read();
    assert_eq!(got.len(), 1);
    assert!((got[0].0.velocity[1] - v).abs() < 1e-2, "{} against {v}", got[0].0.velocity[1]);
    assert!((got[0].0.place[1] - p).abs() < 1e-2, "{} against {p}", got[0].0.place[1]);
}

#[test]
fn a_copy_keeps_the_particles_as_they_were_and_runs_are_the_same() {
    let Some(gpu) = gpu() else { return };
    let run = |copy_at: Option<u32>| {
        let mut swarm = SwarmBuffers::new(gpu.clone());
        let mut step = plain_step(0, 10_000);
        step.more[0] = 2;
        step.forces[0] = ForceGpu {
            kind: [FORCE_GRAVITY, 0, 0, 0],
            a: [1.0, 0.0, 0.0, 0.0],
            b: [0.0, 400.0, 0.0, 0.0],
            ..ForceGpu::default()
        };
        step.forces[1] = ForceGpu {
            kind: [FORCE_WANDER, 0, 0, 0],
            a: [1.0, 0.0, 0.0, 0.0],
            b: [300.0, 2.0, 0.0, 0.0],
            ..ForceGpu::default()
        };
        let mut copy = None;
        for s in 0..60u32 {
            step.counts[0] = s;
            step.time[0] = s as f32 * DT;
            let births: Vec<_> = (0..100).map(|i| speck(s * 100 + i, s, 0.2 + (i % 7) as f32 * 0.05)).collect();
            swarm.step(&StepIn { step, words: &[], births: &births });
            if copy_at == Some(s) {
                copy = Some((swarm.clone(), swarm.read()));
            }
        }
        (swarm.read(), copy)
    };
    let (a, copy) = run(Some(20));
    let (b, _) = run(None);
    assert!(!a.is_empty());
    assert_eq!(a, b, "the same each run");
    let (copied, then) = copy.expect("copied");
    assert_eq!(copied.read(), then, "the copy is as it was when made");
    assert_ne!(copied.read(), a);
}

#[test]
fn a_crowd_takes_only_as_many_births_as_it_is_short_of() {
    let Some(gpu) = gpu() else { return };
    let mut swarm = SwarmBuffers::new(gpu);
    let mut step = plain_step(0, 1000);
    step.more[2] = STEP_CROWD;
    step.counts[3] = 50;
    let births: Vec<_> = (0..80).map(|i| speck(i, 0, 100.0)).collect();
    swarm.step(&StepIn { step, words: &[], births: &births });
    assert_eq!(swarm.read().len(), 50);
    step.counts[0] = 1;
    swarm.step(&StepIn { step, words: &[], births: &births });
    assert_eq!(swarm.read().len(), 50, "full: none taken");
}

/// Draws `frame` on `gpu`'s own device.
fn render_on(gpu: &SwarmGpu, frame: &crate::Frame, w: u32, h: u32) -> Vec<u8> {
    let mut renderer = crate::Renderer::new(gpu.device(), w, h);
    renderer.render(gpu.device(), gpu.queue(), frame);
    renderer.read_output(gpu.device(), gpu.queue()).unwrap()
}

fn pixel(px: &[u8], w: u32, h: u32, at: [f32; 2]) -> [u8; 4] {
    let x = (((at[0] / 1080.0 + 0.5) * w as f32) as usize).min(w as usize - 1);
    let y = (((at[1] / 1920.0 + 0.5) * h as f32) as usize).min(h as usize - 1);
    let i = (y * w as usize + x) * 4;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn dots_at(gpu: &Arc<SwarmGpu>, places: &[[f32; 2]], size: f32) -> SwarmBuffers {
    let mut swarm = SwarmBuffers::new(gpu.clone());
    let births: Vec<_> = places
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            (
                SpeckMain { place: p, velocity: [0.0; 2], born: 0, life: 100.0, seed: i as u32, turn: 0.0 },
                SpeckMore { size, ..SpeckMore::default() },
            )
        })
        .collect();
    swarm.step(&StepIn { step: plain_step(0, 1000), words: &[], births: &births });
    swarm
}

fn red_dots() -> draw::SwarmLook {
    let mut look = draw::SwarmLook { k0: [0, 6, draw::ROLE_OWN, 0], ..draw::SwarmLook::default() };
    // Fade: none.
    look.k1[3] = 3;
    look.c0 = [1.0, 0.0, 0.0, 1.0];
    look
}

#[test]
fn a_swarm_draws_its_particles_where_they_are() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (270, 480);
    let swarm = dots_at(&gpu, &[[-200.0, 0.0], [200.0, 300.0]], 30.0);
    let mut frame = crate::Frame { bloom: 0.0, ..crate::Frame::default() };
    frame.swarms.push(draw::SwarmRun {
        at: 0,
        source: swarm.source(),
        look: Arc::new(red_dots()),
        balls: Arc::from(Vec::new()),
    });
    let px = render_on(&gpu, &frame, w, h);
    for at in [[-200.0, 0.0], [200.0, 300.0]] {
        let p = pixel(&px, w, h, at);
        assert!(p[0] > 200 && p[1] < 40, "a red dot at {at:?}: {p:?}");
    }
    let empty = pixel(&px, w, h, [0.0, -500.0]);
    assert!(empty[0] < 20, "nothing elsewhere: {empty:?}");
}

#[test]
fn a_swarm_sits_between_the_sprites_its_at_says_and_a_glow_adds_round_it() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (270, 480);
    let swarm = dots_at(&gpu, &[[0.0, 0.0]], 40.0);
    let mut frame = crate::Frame { bloom: 0.0, ..crate::Frame::default() };
    // A blue dot under the swarm, a green one over it.
    frame.particles.push(crate::Particle::new(6, glam::Vec2::new(-30.0, 0.0), 0.0, 40.0, [0.0, 0.0, 1.0, 1.0]));
    frame.particles.push(crate::Particle::new(6, glam::Vec2::new(30.0, 0.0), 0.0, 20.0, [0.0, 1.0, 0.0, 1.0]));
    let mut glow = red_dots();
    glow.k0[2] = draw::ROLE_GLOW;
    glow.s0 = [1.0, 60.0, 0.0, 0.0];
    let run =
        |look| draw::SwarmRun { at: 1, source: swarm.source(), look: Arc::new(look), balls: Arc::from(Vec::new()) };
    frame.swarms.push(run(glow));
    frame.swarms.push(run(red_dots()));
    let px = render_on(&gpu, &frame, w, h);
    let under = pixel(&px, w, h, [-20.0, 0.0]);
    assert!(under[0] > 200 && under[2] < 60, "the swarm covers the blue dot: {under:?}");
    let over = pixel(&px, w, h, [30.0, 0.0]);
    assert!(over[1] > 200 && over[0] < 60, "the green dot is over it: {over:?}");
    let round = pixel(&px, w, h, [0.0, -70.0]);
    assert!(round[0] > 10, "the glow adds light round it: {round:?}");
    // Culling keeps the runs between the same sprites.
    let mut culled = frame.clone();
    culled.camera = bpe_runtime::CameraView { zoom: 1.001, ..Default::default() };
    culled.particles.insert(0, crate::Particle::new(6, glam::Vec2::new(5000.0, 0.0), 0.0, 5.0, [1.0; 4]));
    culled.swarms.iter_mut().for_each(|r| r.at = 2);
    culled.cull();
    assert_eq!(culled.particles.len(), 2);
    assert!(culled.swarms.iter().all(|r| r.at == 1));
}
