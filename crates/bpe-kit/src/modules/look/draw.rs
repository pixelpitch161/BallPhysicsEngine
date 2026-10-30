//! Reading a thing's Look and turning it into colours.

use bpe_core::{Color, Resolved, Settings, SlotId, ThingId};
use bpe_gfx::{DrawCx, GradientAtlas, PaintGpu};
use bpe_runtime::{Bodies, CameraView, Log};
use glam::Vec2;

use super::ClassicLook;
use crate::paint::{Follow, Inputs, Paint, beats_at, colour_at, hits_on, last_hit_at, paint_gpu};

/// The thing's classic look, when its Look slot is filled.
pub fn look_of(cx: &DrawCx) -> Option<ClassicLook> {
    cx.resolved.slots.get(&SlotId::Look).map(|s| ClassicLook::from_params(&s.params))
}

/// `p` as a ball reads it: a ball has no path of its own, so Path reads as Place.
pub fn for_balls(mut p: Paint) -> Paint {
    if p.follow == Follow::Path {
        p.follow = Follow::Place;
    }
    p
}

/// What the paints of one thing's balls read this frame: the balls, the log for their hits,
/// the camera for screen places, and the beats so far (counted once, only when a paint follows
/// them).
pub struct BallInputs<'a> {
    thing: ThingId,
    bodies: &'a Bodies,
    log: &'a Log,
    scene: &'a Resolved,
    camera: CameraView,
    /// Stuck to the screen: the balls' own places are screen places.
    screen: bool,
    time: f64,
    steps_per_second: u32,
    beats: f32,
}

impl<'a> BallInputs<'a> {
    /// The inputs for `bodies`, the balls of the thing `cx` draws, ready for `paints`.
    pub fn new(cx: &DrawCx<'a>, bodies: &'a Bodies, paints: &[&Paint]) -> Self {
        let steps_per_second = cx.world.steps_per_second;
        let beats = if paints.iter().any(|p| p.follow == Follow::Beat) {
            beats_at(cx.log, cx.video_time, steps_per_second)
        } else {
            0.0
        };
        Self {
            thing: cx.thing.id,
            bodies,
            log: cx.log,
            scene: cx.scene,
            camera: cx.world.camera,
            screen: cx.resolved.screen,
            time: cx.time,
            steps_per_second,
            beats,
        }
    }

    /// `world` on the screen.
    fn on_screen(&self, world: Vec2) -> Vec2 {
        if self.screen { world } else { self.camera.to_screen(world) }
    }

    /// What `p` reads about ball `i`. The log is only searched for the last hit when `p` steps
    /// on hits and the ball has hit something.
    pub fn get(&self, p: &Paint, i: usize) -> Inputs {
        let b = self.bodies;
        let place = b.position.get(i).copied().unwrap_or_default();
        let hits = b.hits.get(i).copied().unwrap_or(0);
        let mut inputs = Inputs {
            index: i,
            count: b.len(),
            place,
            place_screen: self.on_screen(place),
            start: b.spawn.get(i).copied().unwrap_or(place),
            speed: b.velocity.get(i).map_or(0.0, |v| v.length()),
            hits,
            beats: self.beats,
            time: self.time as f32,
            ..Inputs::default()
        };
        if p.follow == Follow::Step
            && hits > 0
            && let Some((at, other, point)) = last_hit_at(self.log, self.thing, i, self.time, self.steps_per_second)
        {
            inputs.since_hit = (self.time - at) as f32;
            if p.take_hit_colour {
                inputs.hit_colour = other.and_then(|o| self.colour_of(o, point));
            }
        }
        inputs
    }

    /// The colour thing `other`'s look paints at `point`, as a still thing with no hits.
    fn colour_of(&self, other: ThingId, point: Vec2) -> Option<[f32; 4]> {
        still_colour(self.scene, self.camera, self.time, other, point)
    }
}

/// The colour thing `other`'s look paints at `point` at `time`, as a still thing with no hits.
fn still_colour(scene: &Resolved, camera: CameraView, time: f64, other: ThingId, point: Vec2) -> Option<[f32; 4]> {
    let r = scene.things.get(&other)?;
    let fill = for_balls(ClassicLook::from_params(&r.slots.get(&SlotId::Look)?.params).fill);
    let place_screen = if r.screen { point } else { camera.to_screen(point) };
    let inputs = Inputs { count: 1, place: point, place_screen, start: point, time: time as f32, ..Inputs::default() };
    Some(colour_at(&fill, &inputs))
}

/// `p` as a wall or obstacle reads it: a wall never spawns, so Start reads as Place.
pub fn for_walls(mut p: Paint) -> Paint {
    if p.follow == Follow::Start {
        p.follow = Follow::Place;
    }
    p
}

/// What the paint of one wall or obstacle reads this frame, for each of its `count` pieces (the
/// pegs of a grid, the bars of a set; a wall is one piece): the hits on each piece (counted in one
/// pass over the log, only when the paint follows hits or steps), the beats so far (only when it
/// follows them), the camera for screen places, and how fast the thing moves. A wall has no life
/// and its place along its own line is worked out on the GPU, so both read 0.
pub struct WallInputs<'a> {
    scene: &'a Resolved,
    camera: CameraView,
    /// Stuck to the screen: the pieces' own places are screen places.
    screen: bool,
    time: f64,
    count: usize,
    speed: f32,
    beats: f32,
    /// Per piece: its hits so far, and the latest one (when, what hit it, where).
    #[allow(clippy::type_complexity)]
    hits: Vec<(u32, Option<(f64, Option<ThingId>, Vec2)>)>,
}

impl<'a> WallInputs<'a> {
    /// The inputs for the `count` pieces of the thing `cx` draws, moving at `speed` pixels per
    /// second, ready for `p` (already [`for_walls`]).
    pub fn new(cx: &DrawCx<'a>, p: &Paint, count: usize, speed: f32) -> Self {
        let steps_per_second = cx.world.steps_per_second;
        let beats = if p.follow == Follow::Beat { beats_at(cx.log, cx.video_time, steps_per_second) } else { 0.0 };
        let hits = if matches!(p.follow, Follow::Hits | Follow::Step) {
            hits_on(cx.log, cx.thing.id, count, cx.time, steps_per_second)
        } else {
            Vec::new()
        };
        Self {
            scene: cx.scene,
            camera: cx.world.camera,
            screen: cx.resolved.screen,
            time: cx.time,
            count,
            speed: if speed.is_finite() { speed } else { 0.0 },
            beats,
            hits,
        }
    }

    /// What `p` reads about piece `i`, centred at `place`.
    pub fn get(&self, p: &Paint, i: usize, place: Vec2) -> Inputs {
        let (hits, last) = self.hits.get(i).copied().unwrap_or((0, None));
        let mut inputs = Inputs {
            index: i,
            count: self.count,
            place,
            place_screen: if self.screen { place } else { self.camera.to_screen(place) },
            start: place,
            speed: self.speed,
            hits,
            beats: self.beats,
            time: self.time as f32,
            ..Inputs::default()
        };
        if p.follow == Follow::Step
            && let Some((at, other, point)) = last
        {
            inputs.since_hit = (self.time - at) as f32;
            if p.take_hit_colour {
                inputs.hit_colour = other.and_then(|o| still_colour(self.scene, self.camera, self.time, o, point));
            }
        }
        inputs
    }

    /// How `p` paints piece `i`, centred at `place`: its colour (what the glow shows, and the line
    /// where nothing is painted on it) and the gradient painted on its line, baked into `atlas`.
    pub fn paint(&self, p: &Paint, i: usize, place: Vec2, atlas: &mut GradientAtlas) -> ([f32; 4], PaintGpu) {
        let inputs = self.get(p, i, place);
        (colour_at(p, &inputs), paint_gpu(p, &inputs, atlas, self.time as f32))
    }
}

/// The linear-light colour of each ball in `bodies`: the look's Colours painted the way it
/// follows (place, start, order, speed, hits, steps, beats or time), then turned round the colour
/// wheel by "Hue shift per hit" for every hit the ball has taken. The closure owns the look and
/// borrows the bodies and what `cx` borrows.
pub fn colors<'a>(look: ClassicLook, cx: &DrawCx<'a>, bodies: &'a Bodies) -> impl Fn(usize) -> [f32; 4] + 'a {
    let fill = for_balls(look.fill);
    let inputs = BallInputs::new(cx, bodies, &[&fill]);
    let hue_per_hit = look.hue_per_hit;
    move |i| {
        let base = colour_at(&fill, &inputs.get(&fill, i));
        let hits = bodies.hits.get(i).copied().unwrap_or(0);
        if hue_per_hit == 0.0 || hits == 0 {
            base
        } else {
            Color::from_linear(base).shift_hue(hue_per_hit * hits as f32 / 360.0).to_linear()
        }
    }
}

/// The glow a wall gets from a look: the old renderer's wall glow at the default ball glow.
pub fn wall_glow(look: &ClassicLook) -> f32 {
    0.015 * look.glow / 0.12
}
