use super::*;
use bpe_core::{Registry, Scene, SlotId, Stroke, Strokes, ThingId, Value};
use bpe_kit::things::water::{LIQUID, POUR, StartingLook, course_of, new_water_scene};
use glam::Vec2;

fn set(scene: &mut Scene, id: ThingId, slot: SlotId, key: &str, v: Value) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v);
}

/// A small chamber at coarse detail: a little water filled at the start and a tap pouring in.
fn chamber(reg: &Registry, detail: &str) -> Scene {
    let mut scene = new_water_scene(reg, StartingLook::Plain, "");
    set(&mut scene, ThingId::ROOT, SlotId::Water, "draft_detail", Value::Choice(detail.into()));
    let course = course_of(&scene).unwrap();
    let hole = Stroke { points: vec![[0.0, 0.0]], size: 200.0, strength: 1.0, square: true, ..Default::default() };
    set(&mut scene, course, SlotId::Form, "carved", Value::Strokes(Strokes { list: vec![hole] }));
    bpe_kit::add_thing(&mut scene, reg, LIQUID, ThingId::ROOT).unwrap();
    let fill = bpe_kit::add_thing(&mut scene, reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&fill).unwrap().transform.position = Vec2::new(0.0, 150.0);
    set(&mut scene, fill, SlotId::Main, "kind", Value::Choice("fill_at_the_start".into()));
    set(&mut scene, fill, SlotId::Main, "fill_size", Value::Vec2([300.0, 100.0]));
    let tap = bpe_kit::add_thing(&mut scene, reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&tap).unwrap().transform.position = Vec2::new(0.0, -150.0);
    scene
}

fn setup(scene: &Scene, reg: &Registry) -> Arc<WaterSetup> {
    Arc::new(WaterSetup::of(scene, &bpe_core::resolve(scene, reg)))
}

fn job(scene: &Scene, reg: &Registry, most: u32) -> DraftJob {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    DraftJob::start(&device, &queue, setup(scene, reg), most).expect("the thread starts")
}

/// Waits until the body is at `frame` with nothing left to do.
fn wait_for(job: &DraftJob, frame: u32) {
    job.follow(frame);
    let started = std::time::Instant::now();
    while job.reached() != Some(frame) || job.busy() {
        assert!(!job.failed(), "the draft stopped");
        assert!(started.elapsed().as_secs() < 300, "it gets to frame {frame}: at {:?}", job.reached());
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn drops(job: &DraftJob) -> Vec<bpe_water::Drop> {
    let live = job.live().lock().unwrap();
    let live = live.as_ref().expect("a body");
    live.water.read()
}

#[test]
fn the_draft_follows_the_playhead_and_goes_back() {
    let reg = bpe_kit::registry();
    let job = job(&chamber(&reg, "coarse"), &reg, 500_000);
    wait_for(&job, 90);
    let first = job.count();
    assert!(first > 0);
    wait_for(&job, 30);
    let live = job.live().lock().unwrap();
    assert_eq!(live.as_ref().unwrap().water.frames(), 30);
    assert!(job.count() < first, "the tap had poured less by then");
}

/// The draft marks where the water has been as it plays; going back keeps the marks (the frames
/// past the playhead's read dry against it), and a new body starts dry.
#[test]
fn the_draft_keeps_its_wet_marks_going_back() {
    let reg = bpe_kit::registry();
    let scene = chamber(&reg, "coarse");
    let job = job(&scene, &reg, 500_000);
    let marks = |job: &DraftJob| job.live().lock().unwrap().as_ref().unwrap().wet.read();
    wait_for(&job, 120);
    let played = marks(&job);
    let wet_by = |m: &[u32], frame: u32| m.iter().filter(|&&f| f <= frame).count();
    assert!(wet_by(&played, 0) > 0, "the fill is wet at the start");
    assert!(wet_by(&played, 120) > wet_by(&played, 30), "the tap wets more by 2 s");
    wait_for(&job, 30);
    assert_eq!(marks(&job), played, "going back keeps the marks (read against frame 30, those after are dry)");
    job.restart(setup(&scene, &reg));
    let started = std::time::Instant::now();
    while job.busy() || job.reached() != Some(30) {
        assert!(started.elapsed().as_secs() < 300, "the new body gets to 30");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(wet_by(&marks(&job), 30) <= wet_by(&played, 30), "a new body's marks start again");
    assert!(marks(&job).iter().all(|&f| f == bpe_water::draw::wet::DRY || f <= 30), "nothing past 30 yet");
}

#[test]
fn going_back_plays_the_same_water() {
    let reg = bpe_kit::registry();
    let job = job(&chamber(&reg, "coarse"), &reg, 500_000);
    wait_for(&job, 150);
    let once = drops(&job);
    wait_for(&job, 300);
    wait_for(&job, 150);
    assert_eq!(drops(&job), once, "from the copy at 2 s, the same water");
    wait_for(&job, 10);
    wait_for(&job, 150);
    assert_eq!(drops(&job), once, "and from the start");
}

#[test]
fn a_new_setup_replaces_the_body_and_its_drawing() {
    let reg = bpe_kit::registry();
    let job = job(&chamber(&reg, "coarse"), &reg, 500_000);
    wait_for(&job, 20);
    let (generation, print) = {
        let live = job.live().lock().unwrap();
        let live = live.as_ref().unwrap();
        assert_eq!(live.field.layout.cell, 12.0);
        (live.generation, job.plays())
    };
    let fine = setup(&chamber(&reg, "fine"), &reg);
    job.restart(fine.clone());
    wait_for(&job, 20);
    let live = job.live().lock().unwrap();
    let live = live.as_ref().unwrap();
    assert!(live.generation > generation);
    assert_eq!(job.plays(), fine.print);
    assert_ne!(job.plays(), print);
    assert_eq!(live.field.layout.cell, 6.0);
    assert_eq!(live.water.layout(), live.field.layout);
}

#[test]
fn the_stage_gets_the_body_between_frames_while_the_draft_catches_up() {
    let reg = bpe_kit::registry();
    let job = job(&chamber(&reg, "coarse"), &reg, 500_000);
    wait_for(&job, 0);
    job.follow(600);
    let mut seen = Vec::new();
    let started = std::time::Instant::now();
    while seen.len() < 5 && started.elapsed().as_secs() < 300 {
        let body = job.body(true).expect("the body");
        seen.push(body.as_ref().expect("a body").water.frames());
    }
    // Each wait is for the frame the thread is on at most, not for the whole way to frame 600.
    assert!(seen.iter().all(|f| *f < 600), "{seen:?}");
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert!(job.body(false).is_none() || job.reached().is_some());
}

#[test]
fn a_tap_in_the_rock_is_named_for_the_stage() {
    let reg = bpe_kit::registry();
    let mut scene = chamber(&reg, "coarse");
    let buried = bpe_kit::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&buried).unwrap().transform.position = Vec2::new(0.0, -700.0);
    let job = job(&scene, &reg, 500_000);
    wait_for(&job, 30);
    assert!(job.in_rock(buried));
    let mut others = scene.order.iter().filter(|id| **id != buried && scene.things[*id].kind == POUR);
    assert!(others.all(|id| !job.in_rock(*id)));
    let live = job.live().lock().unwrap();
    let live = live.as_ref().unwrap();
    for d in live.water.read() {
        let at = live.field.layout.to_world(Vec2::from(d.x));
        assert!(at.y > -300.0, "no drop from the buried tap: one at {at}");
    }
}

#[test]
fn the_draft_stops_pouring_at_its_most() {
    let reg = bpe_kit::registry();
    let job = job(&chamber(&reg, "coarse"), &reg, 1_000);
    wait_for(&job, 400);
    assert!(job.full());
    // A frame's births past the most at the very most (a coarse tap makes about six a frame).
    assert!((1_000..=1_020).contains(&job.count()), "{}", job.count());
}

#[test]
fn copies_are_kept_every_two_seconds_within_their_memory() {
    let mut copies = Copies::default();
    let mib = 1u64 << 20;
    for k in 0..10 {
        copies.keep(k * COPY_EVERY, 200 * mib, ());
        copies.prune(COPIES_MOST);
    }
    let kept = copies.frames();
    assert_eq!(kept.first(), Some(&0), "the start is always kept");
    assert_eq!(kept.last(), Some(&(9 * COPY_EVERY)), "and the newest");
    assert!(kept.len() as u64 * 200 * mib <= COPIES_MOST, "{kept:?}");
    assert!(kept.len() >= 3, "{kept:?}");
    copies.drop_after(5 * COPY_EVERY);
    assert!(copies.frames().iter().all(|f| *f <= 5 * COPY_EVERY));
    assert!(copies.newest_at_or_before(5 * COPY_EVERY - 1).is_some());
}

#[test]
fn the_playhead_frame_is_a_sixtieth_of_a_second() {
    assert_eq!([frame_of(0.0), frame_of(1.0), frame_of(0.999), frame_of(-1.0), frame_of(f64::NAN)], [0, 60, 59, 0, 0]);
}
