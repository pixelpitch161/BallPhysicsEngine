//! What a final's take is made of (Water mode part 8, `docs/superpowers/plans/water-mode/
//! part8-make-final-and-export.md`): restart points that play on as the body did, frames kept
//! inside a frame that change no drop, and the frames packed and unpacked.

use std::sync::Arc;

use bpe_water::drops::capture::Capture;
use bpe_water::scenes::{self, Running};
use bpe_water::take::FrameCodec;
use bpe_water::{Numbers, Water, WaterGpu, WaterState};

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// A pool with a tap pouring into it and a pocket of trapped air, at 12 px.
fn pool(gpu: &Arc<WaterGpu>) -> Running {
    let mut scene = scenes::pool_pour(12.0);
    scene.holes = vec![scenes::Hole { centre: glam::Vec2::new(0.0, 700.0), radius: 50.0, air: true }];
    scene.start(gpu, Numbers::for_cell(12.0))
}

fn same(a: &Water, b: &Water, what: &str) {
    assert_eq!(a.frames(), b.frames(), "{what}: frames");
    assert_eq!(a.count(), b.count(), "{what}: drops");
    assert_eq!(a.liquid(), b.liquid(), "{what}: liquid");
    let (da, db) = (a.read(), b.read());
    assert_eq!(da.len(), db.len(), "{what}: drops in the course");
    assert!(da == db, "{what}: every drop the same to the bit");
}

/// A body made from its state, through bytes, plays on exactly as the body and its copy do.
#[test]
fn a_body_from_its_state_plays_on_as_its_copy() {
    let Some(gpu) = card() else { return };
    let mut run = pool(&gpu);
    run.run_to(40.0 / 60.0);
    let bytes = run.water.state().to_bytes();
    let state = WaterState::from_bytes(&bytes).expect("the state reads back");
    assert_eq!(state, run.water.state(), "the bytes keep every part");
    let mut copy = run.copy();
    let w = &run.water;
    let restored = Water::from_state(gpu.clone(), &run.field, scenes::GRAVITY, *w.numbers(), w.feels(), &state)
        .expect("a body from the state");
    let mut restored = Running { water: restored, field: run.field.clone() };
    for _ in 0..30 {
        run.frame();
        copy.frame();
        restored.frame();
    }
    assert!(run.water.air_found() > 0 || run.water.read().iter().any(|d| d.is_air()), "air is in play");
    same(&run.water, &copy.water, "the copy");
    same(&run.water, &restored.water, "the body from its state");
    assert!(WaterState::from_bytes(&bytes[..bytes.len() - 5]).is_err(), "a state cut short is refused");
}

/// Keeping eight moments a frame changes no drop: the same water to the bit as frames kept whole.
#[test]
fn keeping_frames_changes_no_drop() {
    let Some(gpu) = card() else { return };
    let mut plain = pool(&gpu);
    let mut kept = plain.copy();
    let mut capture = Capture::new();
    let mut moments = Vec::new();
    for k in 0..30 {
        plain.frame();
        kept.water.frame_keeping(if k % 3 == 0 { 8 } else { 1 + k % 4 }, &mut capture);
        moments.extend(capture.take(gpu.device(), false));
    }
    moments.extend(capture.take(gpu.device(), true));
    same(&plain.water, &kept.water, "kept");
    let times: Vec<f64> = moments.iter().map(|c| c.moment.time()).collect();
    assert!(times.windows(2).all(|w| w[0] < w[1]), "the moments come in the order of time: {times:?}");
    assert_eq!(moments.iter().filter(|c| c.moment.steps == 0).count(), 30, "every frame's end");
    assert!(moments.len() > 60, "and moments inside them: {}", moments.len());
}

/// A kept moment, unpacked, is the body at that moment (within the packing's steps), and a
/// falling stream's drops move on from one moment to the next.
#[test]
fn a_kept_moment_is_the_body_at_its_moment() {
    let Some(gpu) = card() else { return };
    let mut run = pool(&gpu);
    run.run_to(0.5);
    let layout = run.water.layout();
    let cells = (layout.cols - 1) * (layout.rows - 1);
    let mut capture = Capture::new();
    run.water.capture_now(&mut capture);
    run.water.frame_keeping(4, &mut capture);
    let moments = capture.take(gpu.device(), true);
    assert_eq!(moments.len(), 5, "the start, three inside and the frame's end");
    let quarter: Vec<f64> = moments.iter().map(|c| c.moment.time() * 60.0).collect();
    for (k, t) in quarter.iter().enumerate() {
        assert!((t - (30.0 + k as f64 / 4.0)).abs() < 0.05, "moment {k} at frame {t}");
    }
    let mut codec = FrameCodec::new();
    let bytes = codec.encode(&moments[4].drops, cells);
    let last = codec.decode(&bytes, &layout).unwrap();
    let mut body: Vec<[f32; 2]> = run.water.read().iter().map(|d| d.x).collect();
    let mut back: Vec<[f32; 2]> = last.xv.iter().map(|p| [p[0], p[1]]).collect();
    assert_eq!(body.len(), back.len(), "every drop in the course");
    let key = |p: &[f32; 2]| ((p[1] * 64.0).floor() as i64, (p[0] * 64.0).floor() as i64);
    body.sort_by_key(key);
    back.sort_by_key(key);
    let near = body.iter().zip(&back).filter(|(a, b)| (a[0] - b[0]).abs() < 0.03 && (a[1] - b[1]).abs() < 0.03).count();
    assert!(near * 100 >= body.len() * 98, "the moment is the body: {near} of {} close", body.len());
    let mut most = 0.0f32;
    for d in run.water.read() {
        most = most.max(glam::Vec2::from(d.v).length() * layout.cell);
    }
    let fastest = last.xv.iter().map(|p| glam::Vec2::new(p[2], p[3]).length() * layout.cell).fold(0.0, f32::max);
    assert!((fastest - most).abs() < 0.08 * most + 1.0, "speeds kept: {fastest} against {most}");
}
