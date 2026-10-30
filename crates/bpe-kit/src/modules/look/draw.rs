//! Reading a thing's Look and turning it into colours.

use std::cell::RefCell;
use std::collections::BTreeMap;

use bpe_core::{Color, Resolved, Settings, SlotId, ThingId};
use bpe_gfx::{DrawCx, GradientAtlas, PaintGpu};
use bpe_runtime::{Bodies, CameraView};
use glam::Vec2;

use super::ClassicLook;
use crate::paint::{Follow, Inputs, Paint, beats_at, colour_at, hits_on, paint_gpu};

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

/// What the paints of one thing's balls read this frame: the balls, their latest hits (found in
/// one pass over the log, only when a paint steps on them), the camera for screen places, and
/// the beats so far (counted once, only when a paint follows them).
pub struct BallInputs<'a> {
    bodies: &'a Bodies,
    camera: CameraView,
    /// Stuck to the screen: the balls' own places are screen places.
    screen: bool,
    time: f64,
    beats: f32,
    /// Per ball: its latest hit (when, what it hit, where); empty when no paint steps on hits.
    #[allow(clippy::type_complexity)]
    last: Vec<Option<(f64, Option<ThingId>, Vec2)>>,
    still: StillColours<'a>,
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
        let last = if paints.iter().any(|p| p.follow == Follow::Step) {
            hits_on(cx.log, cx.thing.id, bodies.len(), cx.time, steps_per_second).into_iter().map(|h| h.1).collect()
        } else {
            Vec::new()
        };
        Self {
            bodies,
            camera: cx.world.camera,
            screen: cx.resolved.screen,
            time: cx.time,
            beats,
            last,
            still: StillColours::new(cx),
        }
    }

    /// `world` on the screen.
    fn on_screen(&self, world: Vec2) -> Vec2 {
        if self.screen { world } else { self.camera.to_screen(world) }
    }

    /// What `p` reads about ball `i`. The latest hit only counts when `p` steps on hits and the
    /// ball has hit something.
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
            && let Some(&Some((at, other, point))) = self.last.get(i)
        {
            inputs.since_hit = (self.time - at) as f32;
            if p.take_hit_colour {
                inputs.hit_colour = other.and_then(|o| self.still.at(o, point));
            }
        }
        inputs
    }
}

#[cfg(test)]
thread_local! {
    /// How many times this thread has read another thing's look for its still colour.
    pub(crate) static LOOK_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The colours other things' looks paint as still things with no hits, for paints that take
/// the colour of what they hit. Each thing's look is read once, the first time it is asked for.
pub struct StillColours<'a> {
    scene: &'a Resolved,
    camera: CameraView,
    time: f64,
    /// Each thing's fill as a ball reads it and whether it is stuck to the screen; None for a
    /// thing with no look.
    looks: RefCell<BTreeMap<ThingId, Option<(Paint, bool)>>>,
}

impl<'a> StillColours<'a> {
    pub fn new(cx: &DrawCx<'a>) -> Self {
        Self { scene: cx.scene, camera: cx.world.camera, time: cx.time, looks: RefCell::default() }
    }

    /// The colour thing `other`'s look paints at `point`.
    pub fn at(&self, other: ThingId, point: Vec2) -> Option<[f32; 4]> {
        let mut looks = self.looks.borrow_mut();
        let look = looks.entry(other).or_insert_with(|| {
            let r = self.scene.things.get(&other)?;
            #[cfg(test)]
            LOOK_READS.with(|n| n.set(n.get() + 1));
            Some((for_balls(ClassicLook::from_params(&r.slots.get(&SlotId::Look)?.params).fill), r.screen))
        });
        let (fill, screen) = look.as_ref()?;
        let place_screen = if *screen { point } else { self.camera.to_screen(point) };
        let inputs =
            Inputs { count: 1, place: point, place_screen, start: point, time: self.time as f32, ..Inputs::default() };
        Some(colour_at(fill, &inputs))
    }
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
    still: StillColours<'a>,
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
            still: StillColours::new(cx),
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
                inputs.hit_colour = other.and_then(|o| self.still.at(o, point));
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
