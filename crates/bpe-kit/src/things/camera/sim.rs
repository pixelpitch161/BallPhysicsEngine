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
/// `None` when nothing it follows is there. A ball counts once each; a shape or a walled Text once
/// at its place, with its whole outline kept in the shot; a Particles thing once at the middle of
/// its particles, all of them kept in; a Light once a lamp; a Rig each lamp of its lights.
pub fn follow_point(world: &World, settings: &CameraSettings, me: ThingId) -> Option<(Vec2, Vec2, Vec2)> {
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    let (mut sum, mut vel, mut n) = (Vec2::ZERO, Vec2::ZERO, 0u32);
    let mut add = |at: Vec2, velocity: Vec2, low: Vec2, high: Vec2| {
        if !(at.is_finite() && low.is_finite() && high.is_finite()) {
            return;
        }
        lo = lo.min(low);
        hi = hi.max(high);
        sum += at;
        vel += if velocity.is_finite() { velocity } else { Vec2::ZERO };
        n += 1;
    };
    let mut followed = world.matching(&settings.follow_target, me);
    // Following Balls follows every thing's balls: emitters' too.
    if settings.follow_target == bpe_core::Target::Kind(bpe_core::KindId::from("balls")) {
        let emitters = bpe_core::Target::Kind(bpe_core::KindId::from(crate::things::emitter::KIND));
        followed.extend(world.matching(&emitters, me));
        followed.sort();
    }
    for id in followed {
        if let Some(bodies) = world.bodies.get(&id) {
            for i in 0..bodies.len() {
                if !bodies.alive[i] {
                    continue;
                }
                let (p, r) = (bodies.position[i], bodies.radius[i]);
                add(p, bodies.velocity[i], p - r, p + r);
            }
            continue;
        }
        if id == me {
            continue;
        }
        if let Some(wall) = world.walls.get(&id) {
            let pose = world.poses.get(&id).copied();
            let at = pose.map_or_else(|| crate::things::light::draw::outline_centre(&wall.outline), |p| p.position);
            let velocity = pose.map_or(wall.velocity, |p| p.velocity);
            // Only Fit all reads the box (the outline is walked once a step).
            let (low, high) = if settings.fit_all { outline_box(wall).unwrap_or((at, at)) } else { (at, at) };
            add(at, velocity, low, high);
            continue;
        }
        if let Some((at, velocity, low, high)) = particles_box(world, id) {
            add(at, velocity, low, high);
            continue;
        }
        match world.kind_of(id) {
            Some(crate::things::light::LIGHT | crate::things::rig::RIG) => {
                for at in lamps_of(world, id) {
                    add(at, Vec2::ZERO, at, at);
                }
            }
            _ => {
                if let Some(pose) = world.poses.get(&id) {
                    add(pose.position, pose.velocity, pose.position, pose.position);
                }
            }
        }
    }
    (n > 0).then(|| (sum / n as f32, vel / n as f32, (hi - lo) * 0.5))
}

/// The box round a wall's outline, its band included.
fn outline_box(wall: &bpe_runtime::Wall) -> Option<(Vec2, Vec2)> {
    let band = if wall.solid { 0.0 } else { (wall.outline.thickness() * 0.5).max(0.0) };
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for p in crate::things::light::draw::outline_contours(&wall.outline).iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    (lo.x <= hi.x && lo.y <= hi.y).then_some((lo - band, hi + band))
}

/// A Particles thing's live particles: their middle, mean velocity and the box round them (their
/// sizes included). `None` with none (or a Huge swarm's, which live on the graphics card), or when
/// the world doesn't step it.
fn particles_box(world: &World, id: ThingId) -> Option<(Vec2, Vec2, Vec2, Vec2)> {
    let st = crate::things::particles::sim::particles_in(world.states.get(&id)?.as_ref())?;
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    let (mut sum, mut vel, mut count) = (Vec2::ZERO, Vec2::ZERO, 0usize);
    for s in st.specks.iter().filter(|s| s.place.is_finite()) {
        let r = if s.size.is_finite() { s.size.max(0.0) } else { 0.0 };
        lo = lo.min(s.place - r);
        hi = hi.max(s.place + r);
        sum += s.place;
        vel += if s.velocity.is_finite() { s.velocity } else { Vec2::ZERO };
        count += 1;
    }
    let k = count as f32;
    (count > 0).then(|| (sum / k, vel / k, lo, hi))
}

/// The Light things a followed Light or Rig shines from: the light itself, or a rig's lights.
fn followed_lights(tree: &bpe_core::Scene, id: ThingId) -> Vec<ThingId> {
    use crate::things::light::LIGHT;
    match tree.things.get(&id).map(|t| t.kind.as_str()) {
        Some(LIGHT) => vec![id],
        Some(crate::things::rig::RIG) => tree
            .descendants(id)
            .into_iter()
            .filter(|d| tree.things.get(d).is_some_and(|t| t.enabled && t.kind == LIGHT))
            .collect(),
        _ => Vec::new(),
    }
}

/// Where the lamps of a followed Light or Rig are now: each light at its Position (as keyed now)
/// or on what it rides, read from the saved scene (lighting scenes aren't read).
fn lamps_of(world: &World, id: ThingId) -> Vec<Vec2> {
    use crate::things::light::LightSettings;
    let Some(tree) = world.tree.as_deref() else { return Vec::new() };
    let time = world.time();
    followed_lights(tree, id)
        .into_iter()
        .filter_map(|light| tree.things.get(&light).map(|t| (light, t)))
        .flat_map(|(light, t)| {
            let mut s = LightSettings::from_params(&t.params);
            let key = bpe_core::Address::own(light, "position");
            if let Some(bpe_core::Value::Vec2([x, y])) = tree.tracks.get(&key).and_then(|tr| tr.value_at(time)) {
                s.position = Vec2::new(x, y);
            }
            let parent = crate::things::rig::fixtures::ridden(tree, light);
            crate::things::light::draw::places_in(world, parent, &s).into_iter().map(|p| p.at).collect::<Vec<_>>()
        })
        .collect()
}

/// A hash of what the camera reads from the Lights and Rigs it follows (their settings, keys and
/// places in the tree), so an edit to one starts the run over; 0 when it follows none.
fn followed_lights_hash(scene: &bpe_core::Scene, me: ThingId, s: &CameraSettings) -> u64 {
    use std::hash::{Hash, Hasher};
    if !s.follow {
        return 0;
    }
    let mut lights: Vec<ThingId> = scene
        .order
        .iter()
        .filter(|&&id| id != me && scene.target_matches(&s.follow_target, me, id))
        .flat_map(|&id| followed_lights(scene, id))
        .collect();
    if lights.is_empty() {
        return 0;
    }
    lights.sort();
    lights.dedup();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for id in lights {
        let Some(t) = scene.things.get(&id) else { continue };
        id.0.hash(&mut h);
        format!("{:?}", t.params).hash(&mut h);
        format!("{:?}", crate::things::rig::fixtures::ridden(scene, id)).hash(&mut h);
        for (a, track) in scene.tracks.iter().filter(|(a, _)| a.thing == id) {
            format!("{a:?}{track:?}").hash(&mut h);
        }
    }
    h.finish().max(1)
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
        let st =
            CameraState { pos: settings.position, vel: Vec2::ZERO, zoom: view.zoom, trauma: 0.0, punch: 0.0, settings };
        world.camera = view;
        world.states.insert(thing.id, Box::new(st));
    }

    /// A followed thing stepped beside the physics (a Particles thing) steps in the camera's world.
    fn keeps(&self, scene: &bpe_core::Scene, me: &Thing, other: ThingId) -> bool {
        let s = CameraSettings::from_params(&me.params);
        // A Huge swarm's particles live on the graphics card and give no point to follow.
        let swarm =
            bpe_core::Value::Choice(bpe_core::Choice::name(crate::things::particles::look::Engine::HugeSwarm).into());
        let huge = scene
            .things
            .get(&other)
            .and_then(|t| t.slots.get(&bpe_core::SlotId::Look))
            .is_some_and(|l| l.params.get("engine") == Some(&swarm));
        s.follow && !huge && scene.target_matches(&s.follow_target, me.id, other)
    }

    /// Following, it reads followed Lights from the saved scene.
    fn reads_tree(&self, resolved: &ResolvedThing) -> bool {
        CameraSettings::from_params(&resolved.params).follow
    }

    fn fingerprint_more(&self, scene: &bpe_core::Scene, thing: &Thing, resolved: &ResolvedThing) -> u64 {
        followed_lights_hash(scene, thing.id, &CameraSettings::from_params(&resolved.params))
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
                    goal = st.pos
                        + Vec2::new(
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
                // Nothing to follow: hold where the follow left off, and how far out it fitted.
                None => st.vel = Vec2::ZERO,
            }
            view.center = st.pos;
            if s.fit_all {
                view.zoom = st.zoom;
            }
        } else {
            st.pos = key.center;
            st.vel = Vec2::ZERO;
            st.zoom = key.zoom;
        }

        // Hits add shake and punch; both die away. The last step's hits are all in the log by
        // now, whichever things logged them after the camera's turn.
        if s.shake > 0.0 || s.punch > 0.0 {
            for e in log.between(world.step.saturating_sub(1), world.step) {
                if e.kind != EventKind::Hit {
                    continue;
                }
                let ours = world.names(&s.hits_from, me, e.a.thing)
                    || e.b.is_some_and(|b| world.names(&s.hits_from, me, b.thing));
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
        // A rule's shakes and zoom punches (rules part 6), each dying away over its own time.
        if let Some(rules) = world.rules() {
            let time = world.time();
            for k in rules.kicks().iter().filter(|k| k.thing == me) {
                let Some(age) = k.age(time, world.steps_per_second) else { continue };
                let w = k.weight(age);
                if k.punch {
                    view.zoom = (view.zoom * (1.0 + k.amount * w)).max(0.01);
                } else {
                    // Each kick its own wobble, so two never move together.
                    let seed = world.seed ^ me.0 as u64 ^ k.start.wrapping_mul(0x9e37_79b9_7f4a_7c15);
                    let t = age * k.rate as f64;
                    view.center += Vec2::new(noise(seed, 3, t), noise(seed, 4, t)) * (k.amount * w);
                }
            }
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
    fn with_camera(
        f: impl FnOnce(&mut Params),
        ball: impl FnOnce(&mut bpe_core::Thing),
    ) -> (Scene, crate::Kit, ThingId) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let cam = ensure_camera(&mut scene, &kit.registry);
        f(&mut scene.things.get_mut(&cam).unwrap().params);
        ball(scene.things.get_mut(&BALLS).unwrap());
        (scene, kit, cam)
    }

    /// No arena: the ball falls forever.
    fn falling(scene: &mut Scene) {
        scene.things.get_mut(&crate::testing::SHAPE).unwrap().enabled = false;
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

    /// A Zoom key at 0 s is the shot from the first frame on.
    #[test]
    fn a_zoom_key_at_the_start_is_the_first_shot() {
        let (mut scene, kit, cam) = with_camera(|_| {}, |_| {});
        let zoom = bpe_core::Address { thing: cam, slot: None, key: "zoom".into() };
        scene.set_key(&kit.registry, &zoom, 0.0, Value::Number(3.0)).unwrap();
        scene.set_key(&kit.registry, &zoom, 2.0, Value::Number(3.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(world.camera.zoom, 3.0);
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

    /// The new scene with no arena or gravity and a camera following `target` with no slack or
    /// smoothing, run `seconds`.
    fn following(scene: &mut Scene, kit: &crate::Kit, target: Target, fit: bool) -> ThingId {
        falling(scene);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let cam = ensure_camera(scene, &kit.registry);
        let p = &mut scene.things.get_mut(&cam).unwrap().params;
        p.set("follow", Value::Bool(true));
        p.set("follow_target", Value::Target(target));
        p.set("slack", Value::Number(0.0));
        p.set("smoothness", Value::Number(0.0));
        p.set("fit_all", Value::Bool(fit));
        cam
    }

    #[test]
    fn the_camera_follows_a_light_and_a_rigs_lights() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let light = crate::add_thing(&mut scene, &kit.registry, crate::things::light::LIGHT, ThingId::ROOT).unwrap();
        scene.things.get_mut(&light).unwrap().params.set("position", Value::Vec2([500.0, 300.0]));
        following(&mut scene, &kit, Target::Thing(light), false);
        let resolved = resolve(&scene, &kit.registry);
        let (world, _) = run(&scene, &resolved, &kit, 0.5);
        assert!(world.camera.center.distance(Vec2::new(500.0, 300.0)) < 1e-3, "{:?}", world.camera.center);
        // Moving the light starts the camera's run over.
        let before = bpe_runtime::world::fingerprint(&scene, &resolved, &kit.sims);
        let mut moved = scene.clone();
        moved.things.get_mut(&light).unwrap().params.set("position", Value::Vec2([0.0, 0.0]));
        let after = bpe_runtime::world::fingerprint(&moved, &resolve(&moved, &kit.registry), &kit.sims);
        assert_ne!(before, after);

        // A rig: the middle of its lights.
        let mut scene = new_scene(&kit.registry);
        let rig = crate::add_thing(&mut scene, &kit.registry, crate::things::rig::RIG, ThingId::ROOT).unwrap();
        for x in [100.0, 300.0] {
            let l = crate::add_thing(&mut scene, &kit.registry, crate::things::light::LIGHT, rig).unwrap();
            scene.things.get_mut(&l).unwrap().params.set("position", Value::Vec2([x, -50.0]));
        }
        following(&mut scene, &kit, Target::Thing(rig), false);
        let resolved = resolve(&scene, &kit.registry);
        let (world, _) = run(&scene, &resolved, &kit, 0.5);
        assert!(world.camera.center.distance(Vec2::new(200.0, -50.0)) < 1e-3, "{:?}", world.camera.center);
    }

    #[test]
    fn the_camera_follows_particles_which_then_step_with_the_physics() {
        use crate::things::particles::looks::{Look, add_particles};
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let id = add_particles(&mut scene, &kit.registry, Look::SparkFountain, ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        assert!(bpe_runtime::is_beside(&scene, &kit.sims, id, &resolved.things[&id]), "beside while unfollowed");
        let cam = following(&mut scene, &kit, Target::Thing(id), true);
        let resolved = resolve(&scene, &kit.registry);
        assert!(!bpe_runtime::is_beside(&scene, &kit.sims, id, &resolved.things[&id]), "followed: in the main world");
        let main = bpe_runtime::main_part(&scene, &resolved, &kit.sims);
        assert!(main.things.contains_key(&id));
        let (world, _) = run(&scene, &resolved, &kit, 1.5);
        let (middle, _, low, high) = particles_box(&world, id).expect("sparks by now");
        let s = CameraSettings::from_params(&resolved.things[&cam].params);
        let (at, _, half) = follow_point(&world, &s, cam).unwrap();
        assert_eq!(at, middle, "one point: the sparks' middle");
        assert_eq!(half, (high - low) * 0.5);
        // Fit all keeps every spark in the shot.
        let st = crate::things::particles::sim::particles_in(world.states[&id].as_ref()).unwrap();
        assert!(st.specks.iter().all(|p| world.camera.sees(video(), p.place, 0.0)), "{:?}", world.camera);
    }

    #[test]
    fn fit_all_keeps_a_whole_shape_in_the_shot() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let shape = crate::add_thing(&mut scene, &kit.registry, crate::things::shape::KIND, ThingId::ROOT).unwrap();
        scene.things.get_mut(&shape).unwrap().params = crate::ShapeSettings::circle(3000.0, 12.0).to_params();
        following(&mut scene, &kit, Target::Thing(shape), true);
        let resolved = resolve(&scene, &kit.registry);
        let (world, _) = run(&scene, &resolved, &kit, 3.0);
        let wall = &world.walls[&shape];
        for p in crate::things::light::draw::outline_contours(&wall.outline).iter().flatten() {
            assert!(world.camera.sees(video(), *p, 0.0), "{p:?} {:?}", world.camera);
        }
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
        let b = scene.things.get_mut(&BALLS).unwrap();
        b.transform.position = Vec2::new(-1500.0, 0.0);
        b.params.set("velocity", Value::Vec2([0.0, 0.0]));
        let other = crate::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let b = scene.things.get_mut(&other).unwrap();
        b.transform.position = Vec2::new(1500.0, 0.0);
        b.params.set("velocity", Value::Vec2([0.0, 0.0]));
        let cam = ensure_camera(&mut scene, &kit.registry);
        let p = &mut scene.things.get_mut(&cam).unwrap().params;
        p.set("follow", Value::Bool(true));
        p.set("fit_all", Value::Bool(true));
        let resolved = resolve(&scene, &kit.registry);
        let (world, _) = run(&scene, &resolved, &kit, 3.0);
        for bodies in world.bodies.values() {
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

    #[test]
    fn a_hit_logged_after_the_camera_still_shakes_it() {
        let (scene, kit, cam) = with_camera(|p| p.set("shake", Value::Number(40.0)), |_| {});
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(0.1, &kit.sims, &mut log);
        // A hit the camera's own step never saw: logged after its turn.
        log.push(bpe_runtime::Event {
            step: world.step,
            kind: EventKind::Hit,
            a: bpe_runtime::Party { thing: BALLS, item: 0 },
            b: None,
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed: 3000.0,
            at: 0.0,
        });
        world.step(&kit.sims, &mut log);
        assert!(state::<CameraState>(world.states[&cam].as_ref()).trauma > 0.0);
    }

    #[test]
    fn fit_all_holds_its_zoom_when_the_balls_go() {
        let (mut scene, kit, cam) = with_camera(
            |p| {
                p.set("follow", Value::Bool(true));
                p.set("fit_all", Value::Bool(true));
                p.set("fit_margin", Value::Number(1000.0));
            },
            |_| {},
        );
        falling(&mut scene);
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(2.0, &kit.sims, &mut log);
        let zoom = world.camera.zoom;
        assert!(zoom < 1.0, "{zoom}");
        world.bodies.get_mut(&BALLS).unwrap().alive[0] = false;
        world.advance_to(2.1, &kit.sims, &mut log);
        assert_eq!(world.camera.zoom, zoom);
        let _ = cam;
    }
}
