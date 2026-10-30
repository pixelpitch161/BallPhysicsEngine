//! Moving the camera along with the run: the shot that was set, following, fitting, shaking,
//! punching, and staying inside the world box. Its state lives in the world, so a rewound or
//! scrubbed run gives the same shot as one played straight through.

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{CameraView, EventKind, Log, ThingSim, World, state, state_mut};
use glam::Vec2;

use super::CameraSettings;

pub struct CameraSim;

/// What the camera keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraState {
    pub settings: CameraSettings,
    /// Where the smoothed follow is, and how fast it moves.
    pub pos: Vec2,
    pub vel: Vec2,
    /// The zoom Fit all has eased to.
    pub zoom: f32,
    /// How much shake is left, in pixels.
    pub trauma: f32,
    /// How much zoom punch is left, as a share of the zoom.
    pub punch: f32,
}

/// Hit speed that counts as a full-strength hit for shake and punch.
const FULL_HIT: f32 = 1500.0;
/// How often the shake changes direction, per second.
const SHAKE_RATE: f64 = 18.0;

/// The middle, average velocity and half size (radii included) of everything `me` follows, or
/// `None` when nothing it follows is there.
pub fn follow_point(world: &World, settings: &CameraSettings, me: ThingId) -> Option<(Vec2, Vec2, Vec2)> {
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    let (mut sum, mut vel, mut n) = (Vec2::ZERO, Vec2::ZERO, 0u32);
    for id in world.matching(&settings.follow_target, me) {
        if let Some(bodies) = world.bodies.get(&id) {
            for i in 0..bodies.len() {
                if !bodies.alive[i] || bodies.silent {
                    continue;
                }
                let (p, r) = (bodies.position[i], bodies.radius[i]);
                lo = lo.min(p - r);
                hi = hi.max(p + r);
                sum += p;
                vel += bodies.velocity[i];
                n += 1;
            }
        } else if id != me
            && let Some(pose) = world.poses.get(&id)
        {
            lo = lo.min(pose.position);
            hi = hi.max(pose.position);
            sum += pose.position;
            vel += pose.velocity;
            n += 1;
        }
    }
    (n > 0).then(|| (sum / n as f32, vel / n as f32, (hi - lo) * 0.5))
}

/// Smooth noise in -1..1 at `t`, the same for the same seed and salt.
fn noise(seed: u64, salt: u64, t: f64) -> f32 {
    let at = |i: i64| {
        let mut h = seed ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (i as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 31;
        h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 29;
        (h >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    let i = t.floor();
    let f = (t - i) as f32;
    let s = f * f * (3.0 - 2.0 * f);
    at(i as i64) * (1.0 - s) + at(i as i64 + 1) * s
}

/// The keyed shot, before following and the rest.
fn keyed(s: &CameraSettings) -> CameraView {
    CameraView { center: s.position, zoom: s.zoom.max(0.01), turn: s.turn.to_radians() }
}

/// Moves `c` so the turned shot stays inside the box; centres it on an axis where it can't fit.
fn clamp_to_box(mut view: CameraView, video: Vec2, s: &CameraSettings) -> CameraView {
    let half_box = s.box_size.abs() * 0.5;
    let shot = view.shot_bounds_half(video);
    for axis in 0..2 {
        let room = half_box[axis] - shot[axis];
        view.center[axis] = if room <= 0.0 {
            s.box_centre[axis]
        } else {
            view.center[axis].clamp(s.box_centre[axis] - room, s.box_centre[axis] + room)
        };
    }
    view
}

impl ThingSim for CameraSim {
    fn kind(&self) -> &'static str {
        "camera"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = CameraSettings::from_params(&resolved.params);
        let mut view = keyed(&settings);
        if settings.stay_inside {
            view = clamp_to_box(view, world.video, &settings);
        }
        let st = CameraState { pos: settings.position, vel: Vec2::ZERO, zoom: view.zoom, trauma: 0.0, punch: 0.0, settings };
        world.camera = view;
        world.states.insert(thing.id, Box::new(st));
    }

    fn settings_changed(&self, me: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = CameraSettings::from_params(&resolved.params);
        if let Some(st) = world.states.get_mut(&me) {
            state_mut::<CameraState>(st.as_mut()).settings = settings;
        }
    }

    fn after_step(&self, me: ThingId, world: &mut World, log: &mut Log) {
        let dt = world.dt();
        let video = world.video;
        let mut st = state::<CameraState>(world.states[&me].as_ref()).clone();
        let s = st.settings.clone();
        let key = keyed(&s);
        let mut view = key;

        if s.follow {
            let omega = 40.0 + (2.0 - 40.0) * s.smoothness.clamp(0.0, 1.0);
            match follow_point(world, &s, me) {
                Some((middle, velocity, half)) => {
                    let mut goal = middle + velocity * s.look_ahead;
                    // Slack: the target roams the middle of the shot without moving the camera.
                    let slack = key.shot_half(video) * s.slack;
                    let d = goal - st.pos;
                    goal = st.pos + Vec2::new(
                        d.x.signum() * (d.x.abs() - slack.x).max(0.0),
                        d.y.signum() * (d.y.abs() - slack.y).max(0.0),
                    );
                    let target = key.center.lerp(goal, s.follow_amount.clamp(0.0, 1.0));
                    if s.smoothness <= 0.0 {
                        st.pos = target;
                        st.vel = Vec2::ZERO;
                    } else {
                        let accel = (target - st.pos) * (omega * omega) - st.vel * (2.0 * omega);
                        st.vel += accel * dt;
                        st.pos += st.vel * dt;
                    }
                    if s.fit_all {
                        let need = (half + s.fit_margin).max(Vec2::splat(1.0));
                        let fit = (video.x * 0.5 / need.x).min(video.y * 0.5 / need.y);
                        let goal_zoom = key.zoom.min(fit);
                        st.zoom += (goal_zoom - st.zoom) * (1.0 - (-omega * dt).exp());
                        view.zoom = st.zoom;
                    } else {
                        st.zoom = key.zoom;
                    }
                }
                // Nothing to follow: hold where the follow left off.
                None => st.vel = Vec2::ZERO,
            }
            view.center = st.pos;
        } else {
            st.pos = key.center;
            st.vel = Vec2::ZERO;
            st.zoom = key.zoom;
        }

        // Hits add shake and punch; both die away.
        if s.shake > 0.0 || s.punch > 0.0 {
            for e in log.since(world.step) {
                if e.kind != EventKind::Hit {
                    continue;
                }
                let ours = world.matches(me, e.a.thing, &s.hits_from)
                    || e.b.is_some_and(|b| world.matches(me, b.thing, &s.hits_from));
                if !ours {
                    continue;
                }
                let strength = (e.speed / FULL_HIT).min(1.0);
                st.trauma = (st.trauma + s.shake * strength).min(s.shake * 2.0);
                st.punch = (st.punch + s.punch * strength).min(s.punch * 2.0);
            }
        }
        st.trauma *= (-dt * 3.0 / s.shake_fade.max(0.01)).exp();
        st.punch *= (-dt * 3.0 / s.punch_recovery.max(0.01)).exp();
        if st.trauma > 1e-4 {
            let t = world.time() * SHAKE_RATE;
            let seed = world.seed ^ me.0 as u64;
            view.center += Vec2::new(noise(seed, 1, t), noise(seed, 2, t)) * st.trauma;
        } else {
            st.trauma = 0.0;
        }
        if st.punch > 1e-6 {
            view.zoom *= 1.0 + st.punch;
        } else {
            st.punch = 0.0;
        }

        if s.stay_inside {
            view = clamp_to_box(view, video, &s);
        }
        world.camera = view;
        log.push_camera(world.step, view);
        *state_mut::<CameraState>(world.states.get_mut(&me).unwrap().as_mut()) = st;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{BALLS, run};
    use crate::{ensure_camera, kit, new_scene};
    use bpe_core::{Params, Scene, Target, Value, resolve};
    use bpe_runtime::Playback;

    /// The new scene with a camera set up by `f`, and the ball set up by `ball`.
    fn with_camera(f: impl FnOnce(&mut Params), ball: impl FnOnce(&mut bpe_core::Thing)) -> (Scene, crate::Kit, ThingId) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let cam = ensure_camera(&mut scene, &kit.registry);
        f(&mut scene.things.get_mut(&cam).unwrap().params);
        ball(scene.things.get_mut(&BALLS).unwrap());
        (scene, kit, cam)
    }

    /// No arena: the ball falls forever.
    fn falling(scene: &mut Scene) {
        scene.things.get_mut(&crate::testing::ARENA).unwrap().enabled = false;
    }

    fn video() -> Vec2 {
        Vec2::new(1080.0, 1920.0)
    }

    #[test]
    fn unfollowed_camera_is_its_keyed_shot() {
        let (scene, kit, _) = with_camera(
            |p| {
                p.set("position", Value::Vec2([200.0, -300.0]));
                p.set("zoom", Value::Number(2.0));
                p.set("turn", Value::Number(30.0));
            },
            |_| {},
        );
        let resolved = resolve(&scene, &kit.registry);
        let (world, log) = run(&scene, &resolved, &kit, 1.0);
        let want = CameraView { center: Vec2::new(200.0, -300.0), zoom: 2.0, turn: 30f32.to_radians() };
        assert_eq!(world.camera, want);
        assert_eq!(log.camera_at(world.step), want);
    }

    #[test]
    fn follow_converges_on_a_ball() {
        let (mut scene, kit, _) = with_camera(
            |p| {
                p.set("follow", Value::Bool(true));
                p.set("slack", Value::Number(0.0));
            },
            |b| {
                b.transform.position = Vec2::new(300.0, 400.0);
                b.params.set("velocity", Value::Vec2([0.0, 0.0]));
            },
        );
        falling(&mut scene);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let resolved = resolve(&scene, &kit.registry);
        let (world, _) = run(&scene, &resolved, &kit, 0.05);
        assert!(world.camera.center.length() < 200.0, "smoothed, not snapped: {:?}", world.camera.center);
        let (world, _) = run(&scene, &resolved, &kit, 3.0);
        let ball = world.bodies[&BALLS].position[0];
        assert!((world.camera.center - ball).length() < 1.0, "{:?} vs {ball:?}", world.camera.center);
    }

    #[test]
    fn follow_with_no_target_holds_still() {
        let (scene, kit, _) = with_camera(
            |p| {
                p.set("follow", Value::Bool(true));
                p.set("follow_target", Value::Target(Target::Tag("nothing".into())));
                p.set("position", Value::Vec2([40.0, 50.0]));
            },
            |_| {},
        );
        let resolved = resolve(&scene, &kit.registry);
        let (world, _) = run(&scene, &resolved, &kit, 2.0);
        assert_eq!(world.camera.center, Vec2::new(40.0, 50.0));
        assert!(world.camera.zoom.is_finite() && world.camera.turn.is_finite());
    }

    #[test]
    fn fit_all_keeps_spread_balls_in_shot() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        falling(&mut scene);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        for (id, x) in [(BALLS, -1500.0)] {
            let b = scene.things.get_mut(&id).unwrap();
            b.transform.position = Vec2::new(x, 0.0);
            b.params.set("velocity", Value::Vec2([0.0, 0.0]));
        }
        let other = scene.add_thing(&kit.registry, "balls", ThingId::ROOT).unwrap();
        let b = scene.things.get_mut(&other).unwrap();
        b.transform.position = Vec2::new(1500.0, 0.0);
        b.params.set("velocity", Value::Vec2([0.0, 0.0]));
        let cam = ensure_camera(&mut scene, &kit.registry);
        let p = &mut scene.things.get_mut(&cam).unwrap().params;
        p.set("follow", Value::Bool(true));
        p.set("fit_all", Value::Bool(true));
        let resolved = resolve(&scene, &kit.registry);
        let (world, _) = run(&scene, &resolved, &kit, 3.0);
        for bodies in world.bodies.values().filter(|b| !b.silent) {
            for i in 0..bodies.len() {
                assert!(world.camera.sees(video(), bodies.position[i], -bodies.radius[i]), "{:?}", world.camera);
            }
        }
    }

    #[test]
    fn scrub_equals_straight_play() {
        let (mut scene, kit, _) = with_camera(
            |p| {
                p.set("follow", Value::Bool(true));
                p.set("shake", Value::Number(30.0));
                p.set("punch", Value::Number(0.2));
                p.set("stay_inside", Value::Bool(true));
                p.set("box_size", Value::Vec2([1400.0, 2400.0]));
            },
            |_| {},
        );
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([300.0, 900.0]));
        let resolved = resolve(&scene, &kit.registry);
        let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
        straight.seek(4.0);
        let mut scrubbed = Playback::new(&scene, &resolved, kit.sims.clone());
        scrubbed.seek(6.0);
        scrubbed.seek(4.0);
        assert_eq!(straight.world().camera, scrubbed.world().camera);
        assert_eq!(straight.log().camera_at(straight.world().step), scrubbed.log().camera_at(scrubbed.world().step));
    }

    #[test]
    fn camera_adds_no_events() {
        let kit = kit();
        let (_, plain) = crate::scenes::all(&kit.registry).into_iter().next().unwrap();
        let mut with = plain.clone();
        let cam = ensure_camera(&mut with, &kit.registry);
        let p = &mut with.things.get_mut(&cam).unwrap().params;
        p.set("follow", Value::Bool(true));
        p.set("shake", Value::Number(20.0));
        let a = run(&plain, &resolve(&plain, &kit.registry), &kit, 5.0).1;
        let b = run(&with, &resolve(&with, &kit.registry), &kit, 5.0).1;
        assert_eq!(a.hash(), b.hash());
    }

    // ---- shake, punch and the world box ----

    /// A ball dropped onto the arena floor, so it hits hard soon after the start.
    fn hitting(f: impl FnOnce(&mut Params)) -> (Scene, crate::Kit) {
        let (scene, kit, _) = with_camera(f, |b| {
            b.transform.position = Vec2::new(0.0, 0.0);
            b.params.set("velocity", Value::Vec2([0.0, 2000.0]));
        });
        (scene, kit)
    }

    fn first_hit(log: &Log, rate: u32) -> f64 {
        log.events().iter().find(|e| e.kind == EventKind::Hit).expect("a hit").time(rate)
    }

    #[test]
    fn shake_fades_to_rest() {
        let (scene, kit) = hitting(|p| {
            p.set("shake", Value::Number(40.0));
            p.set("shake_fade", Value::Number(0.4));
        });
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 1.0);
        let hit = first_hit(&log, 240);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(hit + 0.1, &kit.sims, &mut log);
        assert!(world.camera.center.length() > 0.5, "{:?}", world.camera.center);
        // Take the ball away so no more hits come, then let the shake die down.
        world.bodies.get_mut(&BALLS).unwrap().alive[0] = false;
        world.advance_to(hit + 2.1, &kit.sims, &mut log);
        assert!(world.camera.center.length() < 0.01, "{:?}", world.camera.center);
    }

    #[test]
    fn punch_zooms_and_recovers() {
        let (scene, kit) = hitting(|p| {
            p.set("punch", Value::Number(0.2));
            p.set("punch_recovery", Value::Number(0.3));
        });
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 1.0);
        let hit = first_hit(&log, 240);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(hit + 1.0 / 240.0, &kit.sims, &mut log);
        assert!(world.camera.zoom > 1.05, "{}", world.camera.zoom);
        // Hold the ball so no more hits come, then let the punch ease back.
        world.bodies.get_mut(&BALLS).unwrap().alive[0] = false;
        world.advance_to(hit + 2.5, &kit.sims, &mut log);
        assert!((world.camera.zoom - 1.0).abs() < 1e-3, "{}", world.camera.zoom);
    }

    #[test]
    fn box_keeps_shot_inside() {
        let (mut scene, kit, _) = with_camera(
            |p| {
                p.set("follow", Value::Bool(true));
                p.set("smoothness", Value::Number(0.0));
                p.set("stay_inside", Value::Bool(true));
                p.set("box_size", Value::Vec2([2000.0, 4000.0]));
                p.set("turn", Value::Number(20.0));
            },
            |_| {},
        );
        falling(&mut scene);
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        for i in 1..=60 {
            world.advance_to(i as f64 * 0.1, &kit.sims, &mut log);
            let v = world.camera;
            let half = v.shot_bounds_half(video());
            assert!((v.center.abs() + half).cmple(Vec2::new(1000.0, 2000.0) + 1e-2).all(), "{v:?}");
        }
    }

    #[test]
    fn box_smaller_than_shot_centres() {
        let (mut scene, kit, _) = with_camera(
            |p| {
                p.set("follow", Value::Bool(true));
                p.set("stay_inside", Value::Bool(true));
                p.set("box_size", Value::Vec2([500.0, 500.0]));
                p.set("box_centre", Value::Vec2([100.0, 100.0]));
            },
            |_| {},
        );
        falling(&mut scene);
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        for i in 1..=30 {
            world.advance_to(i as f64 * 0.1, &kit.sims, &mut log);
            assert_eq!(world.camera.center, Vec2::new(100.0, 100.0));
        }
    }
}
