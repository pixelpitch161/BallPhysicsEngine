//! Drawing streaks from the runtime's position history.

use bpe_core::{Blend, Settings, SlotId};
use bpe_gfx::{DrawCx, Frame, ModuleDraw, push_streak, push_streak_colours};
use glam::Vec2;

use super::{StreakTrail, TrailSource};
use crate::paint::{Follow, Inputs, beats_at, colour_at, seed_of};
use crate::things::balls::draw::ball_colors;

pub struct StreakDraw;

/// The margin each streak edge keeps for anti-aliasing, in video pixels. The preview shows the
/// video shrunk up to about four times, and the shader softens the edge over one output pixel.
pub const AA_MARGIN: f32 = 4.0;

impl ModuleDraw for StreakDraw {
    fn kind(&self) -> &'static str {
        "trail.streak"
    }

    fn draws_add_itself(&self) -> bool {
        true
    }

    fn draw(&self, cx: &DrawCx, slot: SlotId, frame: &mut Frame) {
        let Some(rslot) = cx.resolved.slots.get(&slot) else { return };
        let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
        let Some(history) = cx.world.history.get(&cx.thing.id) else { return };
        let trail = StreakTrail::from_params(&rslot.params);
        if trail.brightness <= 0.0 || trail.width <= 0.0 {
            return;
        }
        let interval = history.interval(cx.world.dt());
        let samples = (trail.seconds / interval).ceil().max(1.0) as usize;
        let ball_color = match trail.source {
            TrailSource::Ball => ball_colors(cx),
            TrailSource::Own => None,
        };
        let along = Along::new(cx, &trail, interval);
        let mut points: Vec<Vec2> = Vec::with_capacity(samples + 1);
        let mut colours: Vec<[f32; 4]> = Vec::with_capacity(samples + 1);
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            points.clear();
            points.push(bodies.position[i]);
            points.extend(history.points(i).take(samples));
            let half_width = trail.width * bodies.radius[i];
            if let Some(color) = &ball_color {
                let color = shade(&trail, color(i));
                push_streak(&mut frame.trails, &points, half_width, trail.taper, color, AA_MARGIN);
                continue;
            }
            let speed = bodies.velocity[i].length();
            colours.clear();
            let seed = seed_of(cx.thing.id, i);
            colours.extend((0..points.len()).map(|k| shade(&trail, along.colour(&points, k, speed, seed))));
            push_streak_colours(&mut frame.trails, &points, half_width, trail.taper, &colours, AA_MARGIN);
        }
    }
}

/// The streak's colour from `base` (linear light) at its brightness, faded by the blend amount:
/// covering with the brightness as opacity, or adding that much light. An additive colour glows
/// whatever the blend says. Other blends draw into a layer that is mixed in at its amount later,
/// so here they are plain: Normal at full amount.
fn shade(trail: &StreakTrail, base: [f32; 4]) -> [f32; 4] {
    let k = trail.brightness;
    let amount = match trail.blend {
        Blend::Normal | Blend::Add => trail.blend_amount.clamp(0.0, 1.0),
        _ => 1.0,
    };
    if trail.blend != Blend::Add && base[3] > 0.0 {
        return [base[0], base[1], base[2], (base[3] * k).min(1.0) * amount];
    }
    let k = k * amount;
    [base[0] * k, base[1] * k, base[2] * k, -1.0]
}

/// What the streak's own Colours read at each of its points, this frame.
struct Along<'a> {
    trail: &'a StreakTrail,
    camera: bpe_runtime::CameraView,
    /// Stuck to the screen: the points are screen places already.
    screen: bool,
    time: f64,
    /// Seconds between the history's samples.
    interval: f32,
    beats: f32,
}

impl<'a> Along<'a> {
    fn new(cx: &DrawCx, trail: &'a StreakTrail, interval: f32) -> Self {
        let beats = if trail.source == TrailSource::Own && trail.colour.follow == Follow::Beat {
            beats_at(cx.log, cx.video_time, cx.world.steps_per_second)
        } else {
            0.0
        };
        Self { trail, camera: cx.world.camera, screen: cx.resolved.screen, time: cx.time, interval, beats }
    }

    /// The colour at point `k` of `points` (newest first, the ball moving at `speed` at the
    /// first): Life runs from 0 at the ball to 1 at the tail end; Place reads where the point
    /// is, Speed how fast the ball went there, and Time when it was there, so each part of the
    /// streak keeps the colour it was drawn in. Beat is the same all along. `seed` is the ball's
    /// own, so the streak varies the way its ball does.
    fn colour(&self, points: &[Vec2], k: usize, speed: f32, seed: u32) -> [f32; 4] {
        let n = points.len();
        let place = points[k];
        let gap = |a: usize, b: usize| (points[a] - points[b]).length() / self.interval.max(1e-6);
        // The first point is the ball itself, nearer the next than a sample apart, so it reads
        // the ball's own speed; the others the segment behind them, the last the one before it.
        let speed = match k {
            0 => speed,
            _ if k + 1 < n => gap(k, k + 1),
            _ if k >= 2 => gap(k - 1, k),
            _ => speed,
        };
        let age = k as f32 * self.interval;
        let inputs = Inputs {
            count: 1,
            place,
            place_screen: if self.screen { place } else { self.camera.to_screen(place) },
            start: place,
            speed,
            life: if n > 1 { k as f32 / (n - 1) as f32 } else { 0.0 },
            beats: self.beats,
            time: self.time as f32 - if self.trail.colour.follow == Follow::Time { age } else { 0.0 },
            seed,
            ..Inputs::default()
        };
        colour_at(&self.trail.colour, &inputs)
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene, registry};
    use bpe_core::{SlotId, ThingId, Value};
    use bpe_gfx::{Frame, build_frame};

    #[test]
    fn a_streak_follows_the_ball_in_its_colour() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let trail = scene.add_module(&reg, "trail.streak", "Streak").unwrap();
        scene.link(&reg, ThingId(2), SlotId::Trail, trail).unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(2.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 2.0, &kit.draws, &mut frame);
        // Half a second at 60 samples a second: 30 segments of 6 vertices, less any the ball
        // didn't move for.
        assert!(!frame.trails.is_empty());
        assert!(frame.trails.len() <= 30 * 6);
        assert_eq!(frame.trails.len() % 6, 0);
        // The first vertex sits half a width plus the margin beside the ball, or beside the
        // newest sample a sixtieth of a second behind it.
        let ball = playback.world().bodies[&ThingId(2)].position[0];
        let head = glam::Vec2::from(frame.trails[0].position);
        let speed = playback.world().bodies[&ThingId(2)].velocity[0].length();
        let reach = 24.0 * 0.5 + super::AA_MARGIN + speed / 60.0;
        assert!((head - ball).length() <= reach + 1e-3, "{head} is not at the ball {ball}");
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        // The first vertex is at or just behind the head, where the fade has barely begun.
        let c = frame.trails[0].color;
        assert_eq!(c[3], -1.0);
        let fade = c[0] / (pink[0] * 0.5);
        assert!(fade > 0.9 && fade <= 1.0 + 1e-6, "{c:?}");
        assert!((c[2] / c[0] - pink[2] / pink[0]).abs() < 1e-4, "{c:?}");

        // Its own colour, covering, at the brightness as opacity.
        let m = scene.library.modules.get_mut(&trail).unwrap();
        m.params.set("source", Value::Choice("own".into()));
        m.params.set("blend", Value::Choice("normal".into()));
        m.params.set("colours", Value::Gradient(bpe_core::Gradient::solid(bpe_core::Color::rgb(0, 255, 0))));
        let resolved = bpe_core::resolve(&scene, &reg);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 2.0, &kit.draws, &mut frame);
        let c = frame.trails[0].color;
        assert_eq!(c[..3], [0.0, 1.0, 0.0]);
        assert!(c[3] > 0.45 && c[3] <= 0.5, "{c:?}");
        // Without a trail nothing is drawn and no history is kept.
        scene.set_slot_off(ThingId(2), SlotId::Trail);
        let resolved = bpe_core::resolve(&scene, &reg);
        let world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
        assert!(world.history.is_empty());
        build_frame(&scene, &resolved, &world, &bpe_runtime::Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.trails.is_empty());
    }

    #[test]
    fn the_blend_amount_fades_a_normal_streak_and_dims_an_added_one() {
        use super::{StreakTrail, shade};
        use bpe_core::Blend;
        assert_eq!(StreakTrail::default().blend, Blend::Add);
        assert_eq!(StreakTrail::default().blend_amount, 1.0);
        let base = [0.8, 0.4, 0.2, 1.0];
        let trail =
            |blend, blend_amount| StreakTrail { brightness: 0.5, blend, blend_amount, ..StreakTrail::default() };
        assert_eq!(shade(&trail(Blend::Normal, 1.0), base), [0.8, 0.4, 0.2, 0.5]);
        assert_eq!(shade(&trail(Blend::Normal, 0.5), base), [0.8, 0.4, 0.2, 0.25]);
        assert_eq!(shade(&trail(Blend::Add, 1.0), base), [0.4, 0.2, 0.1, -1.0]);
        assert_eq!(shade(&trail(Blend::Add, 0.5), base), [0.2, 0.1, 0.05, -1.0]);
        // An additive colour glows whatever the blend says, still at the amount.
        assert_eq!(shade(&trail(Blend::Normal, 0.5), [0.8, 0.4, 0.2, 0.0]), [0.2, 0.1, 0.05, -1.0]);
    }

    #[test]
    fn trail_life_runs_head_to_tail() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let trail = scene.add_module(&reg, "trail.streak", "Streak").unwrap();
        scene.link(&reg, ThingId(2), SlotId::Trail, trail).unwrap();
        let (red, blue) = (bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 0, 255));
        let m = scene.library.modules.get_mut(&trail).unwrap();
        m.params.set("source", Value::Choice("own".into()));
        m.params.set("blend", Value::Choice("normal".into()));
        m.params.set("colours", Value::Gradient(bpe_core::Gradient::even(&[red, blue])));
        m.params.set("follow", Value::Choice("life".into()));
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        // Between two history samples, so the ball is never on the newest one and the first
        // segment is drawn.
        playback.seek(2.01);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 2.01, &kit.draws, &mut frame);
        // Covering, so the colour itself is kept and only the alpha fades along the streak.
        let head = frame.trails[0].color;
        assert_eq!(head[..3], red.to_linear()[..3], "the ball's end is the first stop");
        let tail = frame.trails.last().unwrap().color;
        assert_eq!(tail[..3], blue.to_linear()[..3], "the tail's end is the last stop");
        // Halfway along it is halfway between.
        let mid = frame.trails[frame.trails.len() / 2].color;
        assert!(mid[0] > 0.1 && mid[2] > 0.1 && mid[1] == 0.0, "{mid:?}");

        // Following time, each part keeps the colour of when the ball was there: at a quarter
        // of a cycle a second, half a second back is an eighth of the way less.
        let m = scene.library.modules.get_mut(&trail).unwrap();
        m.params.set("follow", Value::Choice("time".into()));
        let resolved = bpe_core::resolve(&scene, &reg);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 2.01, &kit.draws, &mut frame);
        let at = |t: f32| [1.0 - t, 0.0, t];
        let near = |c: [f32; 4], want: [f32; 3]| (0..3).all(|k| (c[k] - want[k]).abs() < 1e-4);
        assert!(near(frame.trails[0].color, at(2.01 * 0.25)), "{:?}", frame.trails[0].color);
        let tail = frame.trails.last().unwrap().color;
        assert!(near(tail, at(2.01 * 0.25 - 30.0 / 60.0 * 0.25)), "{tail:?}");
    }

    #[test]
    fn each_ball_s_streak_varies_its_own_colour_like_the_ball() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(1));
        });
        let reg = &kit.registry;
        let trail = scene.add_module(reg, "trail.streak", "Streak").unwrap();
        scene.link(reg, ThingId(2), SlotId::Trail, trail).unwrap();
        let m = scene.library.modules.get_mut(&trail).unwrap();
        m.params.set("source", Value::Choice("own".into()));
        m.params.set("blend", Value::Choice("normal".into()));
        m.params.set("colours", Value::Gradient(bpe_core::Gradient::solid(bpe_core::Color::rgb(255, 0, 0))));
        m.params.set("vary_hue", Value::Number(60.0));
        let resolved = bpe_core::resolve(&scene, reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(1.01);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 1.01, &kit.draws, &mut frame);
        let mut heads: Vec<[f32; 4]> = Vec::new();
        for v in &frame.trails {
            if !heads.iter().any(|h| (0..3).all(|k| (h[k] - v.color[k]).abs() < 1e-4)) {
                heads.push(v.color);
            }
        }
        // One colour per streak, each its own.
        assert_eq!(heads.len(), 3, "{heads:?}");
    }
}
