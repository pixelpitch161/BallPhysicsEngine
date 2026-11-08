//! A light in the picture: nothing drawn of its own, only its light, through the
//! [`ThingDraw::light`] hook. It reads where it is from its Position, or from what it rides when
//! its parent in the tree is a Balls, Emitter or Shape thing.

use bpe_core::{Settings, ThingId};
use bpe_gfx::{DrawCx, Frame, LightGpu, LightScene, ThingDraw};
use bpe_runtime::{EventKind, Outline, World};
use glam::Vec2;

use super::laser::{self, Meets};
use super::{
    AreaShape, AtThings, Falloff, FlickerStyle, LightColour, LightSettings, LightType, LightsUp, Points, RidesOn,
    StripAlong,
};
use crate::things::rig::pattern::Drive;

pub struct LightDraw;

/// The most rays a light beam's band is traced with.
const MOST_RAYS: u32 = 64;
/// How many points a round outline is cut into for a strip running along it.
const ROUND_POINTS: usize = 96;
/// A hit this fast or faster flares a light fully.
const FULL_HIT: f32 = 600.0;

impl ThingDraw for LightDraw {
    fn kind(&self) -> &'static str {
        super::LIGHT
    }

    fn draw(&self, _cx: &DrawCx, _frame: &mut Frame) {}

    fn light(&self, cx: &DrawCx, scene: &mut LightScene) {
        // The showing lighting scene stands in for the light's own settings, then its rigs play
        // their patterns on each lamp.
        let scenes = crate::modules::light_scenes::now(cx);
        let s = shown_settings(cx, &scenes);
        let spots = places(cx, &s);
        let drives = crate::things::rig::fixtures::drives(cx, &scenes, spots.len());
        let picked = Picked::of(cx, &s);
        // Lights up: the link numbers it names, shared by all its lamps.
        let links = if s.lights_up == LightsUp::Everything {
            [0.0; 4]
        } else {
            let named = super::links::named(cx.scene, cx.world, &s, cx.resolved.id);
            let first = scene.links.len() as f32;
            scene.links.extend_from_slice(&named);
            [s.lights_up.code(), first, named.len() as f32, 0.0]
        };
        // What a laser's beam meets, worked out once for all its lamps.
        let scenery = s.kind.is_beam().then(|| laser::scenery(cx.world));
        // A broken light dims (and a popped one flares) as its Break look plays out.
        let broken = crate::rules::remains::whole_now(cx).map_or(1.0, |w| w.alpha * w.scale);
        for (spot, mut drive) in spots.iter().zip(drives) {
            drive.strength *= broken;
            let colour = drive.colour.or_else(|| picked.colour(spot));
            push_light(cx, &s, spot, &drive, colour, links, scenery.as_ref(), scene);
        }
    }
}

/// The light's settings as it shows now: the showing lighting scene's in place of its own.
pub fn shown_settings(cx: &DrawCx, scenes: &crate::modules::light_scenes::ScenesNow<'_>) -> LightSettings {
    LightSettings::from_params(&scenes.params(cx.resolved.id, &cx.resolved.params))
}

/// One place a light shines from: a light riding each ball is one per ball.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    pub at: Vec2,
    /// How the ridden thing is moving (for Points: Where it's going).
    pub velocity: Vec2,
    /// The radius of the ridden ball, so its own shadow doesn't swallow its light.
    pub skip: f32,
    /// The ridden ball (thing, row), for its own flare.
    pub ball: Option<(ThingId, u32)>,
    /// What it points at (Points: At the pen, where a beam stops, or At a thing).
    pub aim: Option<Vec2>,
}

/// Where the light shines from at this moment, each place aimed at its thing when it points at one.
pub fn places(cx: &DrawCx, s: &LightSettings) -> Vec<Place> {
    // A rig only groups lights: a light rides what its rig sits in.
    let parent = crate::things::rig::fixtures::ridden(cx.tree, cx.resolved.id);
    let mut out = if let Some(text) =
        parent.filter(|p| cx.tree.things.get(p).is_some_and(|t| t.kind == crate::things::text::KIND))
    {
        text_places(cx, text, s)
    } else {
        places_in(cx.world, parent, s)
    };
    aim_places(cx.world, s, cx.resolved.id, &mut out);
    out
}

/// What the light `me` points at when it points At a thing: the point of the first thing its Aims
/// at names that has one (itself left out). `None` otherwise, or when nothing named is there.
pub fn aim_at(world: &World, s: &LightSettings, me: ThingId) -> Option<Vec2> {
    if s.points != Points::AtAThing || s.kind == LightType::Point {
        return None;
    }
    world
        .matching(&s.aim_at, me)
        .into_iter()
        .filter(|&id| id != me)
        .find_map(|id| crate::things::point::point_of(world, id, s.aim_ball))
}

/// Aims every place of the light `me` at its thing, when it points at one.
pub fn aim_places(world: &World, s: &LightSettings, me: ThingId, places: &mut [Place]) {
    if let Some(at) = aim_at(world, s, me) {
        for p in places {
            p.aim = Some(at);
        }
    }
}

/// Where a light inside Text `text` shines from (Text part 4): on its Draw on pen, its middle, or
/// its own Position, aimed at the pen when it points there. Nothing where it needs a pen and there
/// is none.
fn text_places(cx: &DrawCx, text: ThingId, s: &LightSettings) -> Vec<Place> {
    use crate::things::text::draw::{TextAt, text_now};
    let own = Place { at: s.position, ..Place::default() };
    let Some(r) = cx.scene.things.get(&text) else { return vec![own] };
    let at = TextAt {
        scene: Some(cx.scene),
        log: Some(cx.log),
        sound: cx.sound,
        look: r.slots.get(&bpe_core::SlotId::Look),
        ..TextAt::new(cx.tree, &r.params, cx.world, text, cx.time)
    };
    let wants_pen = s.rides_on == RidesOn::ThePen || (s.kind != LightType::Point && s.points == Points::AtThePen);
    // A Text in the world that is a wall of its letters: ridden like any wall (as the editor's handles read it).
    let walled = cx.world.walls.contains_key(&text);
    if !wants_pen && (walled || s.rides_on != RidesOn::TheMiddle) {
        return places_in(cx.world, Some(text), s);
    }
    let pen = if wants_pen { crate::things::text::moves::pen(&at) } else { None };
    if wants_pen && pen.is_none() {
        return Vec::new();
    }
    let place = match s.rides_on {
        RidesOn::ThePen => pen.map_or(own, |p| Place { at: p + s.position, ..own }),
        RidesOn::TheMiddle if walled => places_in(cx.world, Some(text), s).into_iter().next().unwrap_or(own),
        RidesOn::TheMiddle => match text_now(&at).and_then(|now| now.bounds().map(|(lo, hi)| (now, 0.5 * (lo + hi)))) {
            Some((now, mid)) => {
                let screen = cx.tree.things.get(&text).is_none_or(|t| t.screen.unwrap_or(true));
                let p = now.to_world(mid);
                Place { at: if screen { cx.world.camera.to_world(p) } else { p } + s.position, ..own }
            }
            None => own,
        },
        _ => places_in(cx.world, Some(text), s).into_iter().next().unwrap_or(own),
    };
    let aim = if s.points == Points::AtThePen { pen } else { None };
    vec![Place { aim, ..place }]
}

/// Where a light riding `parent` (its tree parent, past any rigs) shines from in `world`: its Position, or what it
/// rides moved by its Position. The editor's handles use this too, so they sit on the light.
pub fn places_in(world: &World, parent: Option<ThingId>, s: &LightSettings) -> Vec<Place> {
    let own = Place { at: s.position, ..Place::default() };
    let Some(parent) = parent.filter(|&p| p != ThingId::ROOT) else { return vec![own] };
    if let Some(bodies) = world.bodies.get(&parent) {
        let ball = |row: usize| Place {
            at: bodies.position[row] + s.position,
            velocity: bodies.velocity[row],
            skip: bodies.radius[row],
            ball: Some((parent, row as u32)),
            aim: None,
        };
        let alive = || (0..bodies.len()).filter(|&r| bodies.alive[r]);
        return match s.rides_on {
            RidesOn::EachBall => alive().map(ball).collect(),
            RidesOn::BallNumber => {
                let row = bodies.row_of(s.ball_number.saturating_sub(1)).filter(|&r| bodies.alive[r]);
                row.map(ball).into_iter().collect()
            }
            // A ball group has no edge: the middle of its balls.
            RidesOn::TheMiddle | RidesOn::APointOnTheEdge | RidesOn::ThePen => {
                let n = alive().count();
                if n == 0 {
                    return Vec::new();
                }
                let (sum, vel) = alive()
                    .fold((Vec2::ZERO, Vec2::ZERO), |(p, v), r| (p + bodies.position[r], v + bodies.velocity[r]));
                vec![Place { at: sum / n as f32 + s.position, velocity: vel / n as f32, ..Place::default() }]
            }
        };
    }
    if let Some(wall) = world.walls.get(&parent) {
        let pose = world.poses.get(&parent).copied().unwrap_or_default();
        let at = match s.rides_on {
            RidesOn::APointOnTheEdge => point_round(&outline_points(&wall.outline), s.edge_at),
            _ => outline_centre(&wall.outline),
        };
        return vec![Place {
            at: at + s.position,
            velocity: if wall.velocity != Vec2::ZERO { wall.velocity } else { pose.velocity },
            ..Place::default()
        }];
    }
    // A parent the world doesn't move (a folder): the light sits at its Position.
    vec![own]
}

/// Adds the light at `place` to the scene, as its rigs drive it.
#[allow(clippy::too_many_arguments)]
fn push_light(
    cx: &DrawCx,
    s: &LightSettings,
    place: &Place,
    drive: &Drive,
    colour: Option<[f32; 3]>,
    links: [f32; 4],
    scenery: Option<&laser::Scenery>,
    scene: &mut LightScene,
) {
    let colour = colour.unwrap_or_else(|| s.linear_colour());
    let strength = s.strength
        * (1.0 + flare(cx, s, place))
        * drive.strength
        * flicker(cx.time, s.flicker, s.flicker_speed, s.flicker_style, s.flicker_seed);
    let place = &Place { at: place.at + Vec2::new(drive.sway, 0.0), ..*place };
    let strength = if s.negative { -strength } else { strength };
    let dir = Vec2::from_angle(aim_angle(s, place, cx.time) + drive.turn.to_radians());
    let height = if s.kind == LightType::Sun { s.elevation.to_radians().sin() } else { s.height };
    let shadow = s.shadow_colour.to_linear();
    let mut size = s.size.max(0.0);
    // Each run of segments the light lights from, with its share of the light's colour and
    // whether it ends on a hit: one for a strip, one a colour for a beam that split (glass and
    // light part 2), none for a lamp.
    let mut runs: Vec<([f32; 3], f32, f32, f32)> = Vec::new();
    if s.kind.is_beam() {
        let shapes = meets_of(s.at_shapes);
        let balls = meets_of(s.at_balls);
        // Glass bends the beam through it (lighting part 2b's See-through and Bends light), unless
        // the beam is to stop there; its Rainbow split splits it into colours.
        // Which things are glass, read once (a beam of many rays meets them very often).
        let plain_glass = scene.plain_glass;
        let mut glass_of = std::collections::HashMap::new();
        for thing in scenery.iter().flat_map(|s| s.things()) {
            glass_of.entry(thing).or_insert_with(|| {
                cx.scene
                    .things
                    .get(&thing)
                    .and_then(|t| t.slots.get(&bpe_core::SlotId::Look))
                    .and_then(crate::modules::look::surface::glass_of)
            });
        }
        let meets = |thing: ThingId, ball: bool| {
            let plain = if ball { balls } else { shapes };
            match glass_of.get(&thing).copied().flatten() {
                // The light probe sees dichroic glass as plain glass (part 4b, one-way).
                Some(g) if plain != Meets::Stop => Meets::Bend {
                    index: g.index,
                    rainbow: g.rainbow,
                    dichroic: if plain_glass { Default::default() } else { g.dichroic },
                },
                _ => plain,
            }
        };
        let empty = laser::Scenery::default();
        let scenery = scenery.unwrap_or(&empty);
        // A beam pointing at the pen stops there.
        let length = match place.aim {
            Some(a) if s.points == Points::AtThePen => s.beam_length.max(0.0).min(a.distance(place.at)),
            _ => s.beam_length.max(0.0),
        };
        let colours = s.colours.clamp(1, laser::MOST_COLOURS);
        let trace = |from: Vec2, most: usize| {
            laser::trace_split(scenery, from, dir, length, s.bounces, place.ball, &meets, colours, most)
        };
        let middle = trace(place.at, laser::MOST_BEAM_SEGMENTS);
        let mut push_run = |share: [f32; 3], path: &laser::Path| {
            if share.iter().all(|c| c.abs() < 1e-6) || path.points.len() < 2 {
                return;
            }
            let first = scene.segments.len() as f32;
            for pair in path.points.windows(2) {
                scene.segments.push([pair[0].x, pair[0].y, pair[1].x, pair[1].y]);
            }
            let count = (path.points.len() - 1) as f32;
            runs.push((share, first, count, if path.ended_on_hit { 1.0 } else { 0.0 }));
        };
        push_run([1.0; 3], &middle.white);
        for c in &middle.colours {
            push_run(c.share, &c.path);
        }
        if s.kind == LightType::Beam {
            // The band: rays across it, edge to edge, joined by ribbons.
            let width = s.beam_width.max(0.0);
            size += 0.5 * width;
            let rays = s.rays_across.clamp(2, MOST_RAYS);
            if width >= 0.5 {
                let side = dir.perp();
                let across: Vec<f32> = (0..rays).map(|j| j as f32 / (rays - 1) as f32).collect();
                let per = laser::MOST_RIBBON_SEGMENTS / rays as usize;
                let traced: Vec<laser::Split> =
                    across.iter().map(|u| trace(place.at + side * ((u - 0.5) * width), per)).collect();
                let glow = strength.max(0.0) * s.laser_glow.max(0.0) * laser::BEAM_SHOW;
                let shown = [colour[0] * glow, colour[1] * glow, colour[2] * glow];
                let gap = width / (rays - 1) as f32;
                let ribbons = laser::ribbons(&traced, &across, gap, shown, s.beam_softness, laser::MOST_RIBBONS);
                scene.ribbons.extend(ribbons);
            }
        }
    }
    if s.kind == LightType::Strip {
        size += 0.5 * s.width.max(0.0);
        let points = match s.strip_along {
            StripAlong::Line => {
                let half = dir * (0.5 * s.length);
                vec![place.at - half, place.at + half]
            }
            StripAlong::AShapesOutline => strip_outline(cx.world, s, cx.resolved.id),
        };
        let first = scene.segments.len() as f32;
        let closed = s.strip_along == StripAlong::AShapesOutline;
        let n = points.len();
        let pairs = if closed { n } else { n.saturating_sub(1) };
        for k in 0..pairs {
            let (a, b) = (points[k], points[(k + 1) % n]);
            scene.segments.push([a.x, a.y, b.x, b.y]);
        }
        runs.push(([1.0; 3], first, pairs as f32, 0.0));
    }
    if !s.kind.is_beam() && s.kind != LightType::Strip {
        runs.push(([1.0; 3], 0.0, 0.0, 0.0));
    }
    // A laser's line is its own Beam width; a light beam draws no line (its band is its glow) and
    // its hit spots are half the band wide (`laser_beams` makes a spot three times `p11.x`).
    let (line_width, line_glow) = match s.kind {
        LightType::Laser => (s.laser_width.max(0.0), s.laser_glow.max(0.0)),
        LightType::Beam => (s.beam_width.max(0.0) / 6.0, 0.0),
        _ => (s.laser_width.max(0.0), 0.0),
    };
    for (share, first, count, ended_on_hit) in runs {
        let c = [colour[0] * share[0], colour[1] * share[1], colour[2] * share[2]];
        push_one(
            cx,
            s,
            place,
            c,
            strength,
            dir,
            height,
            size,
            shadow,
            links,
            [first, count],
            ended_on_hit,
            line_width,
            line_glow,
            scene,
        );
    }
}

/// Pushes one of the light's lights: `colour` its colour (linear, before `strength`), `run` the
/// first segment and how many it lights from.
#[allow(clippy::too_many_arguments)]
fn push_one(
    cx: &DrawCx,
    s: &LightSettings,
    place: &Place,
    colour: [f32; 3],
    strength: f32,
    dir: Vec2,
    height: f32,
    size: f32,
    shadow: [f32; 4],
    links: [f32; 4],
    run: [f32; 2],
    ended_on_hit: f32,
    line_width: f32,
    line_glow: f32,
    scene: &mut LightScene,
) {
    let half_cone = (0.5 * s.cone).to_radians();
    let edge = s.cone_edge.max(0.0).to_radians();
    let (cos_inner, cos_outer) = ((half_cone - edge).max(0.0).cos(), half_cone.cos());
    let [first, count] = run;
    scene.lights.push(LightGpu {
        p0: [place.at.x, place.at.y, height, s.kind.code()],
        p1: [colour[0] * strength, colour[1] * strength, colour[2] * strength, 0.0],
        p2: [s.reach.max(0.0), s.falloff.code(), size, s.shadow_strength],
        p3: [dir.x, dir.y, cos_inner, cos_outer],
        p4: [s.width.max(0.0), s.length.max(0.0), first, count],
        p5: [shadow[0], shadow[1], shadow[2], if s.casts { 1.0 } else { 0.0 }],
        p6: [
            place.skip,
            s.sun_size.max(0.0).to_radians().tan(),
            if s.area_shape == AreaShape::Disc { 1.0 } else { 0.0 },
            s.softness.max(0.0),
        ],
        p7: [
            // A beam in the air is its own line or band (Beam glow), not a strip's wide glow.
            if s.beam.is_finite() && !s.kind.is_beam() { s.beam } else { 0.0 },
            if s.ultraviolet { 1.0 } else { 0.0 },
            s.uv_visible.max(0.0),
            line_glow,
        ],
        p8: [s.gobo.code(), s.gobo_size.max(1e-3), s.gobo_sharpness, s.gobo_depth],
        p9: gobo_turn(s, dir, cx.time),
        p10: links,
        p11: [line_width, s.hit_glow.max(0.0), ended_on_hit, 0.0],
    });
}

/// How a laser meets a thing its At shapes or At balls answers.
fn meets_of(at: AtThings) -> Meets {
    match at {
        AtThings::Bounce => Meets::Bounce,
        AtThings::Stop => Meets::Stop,
        AtThings::PassThrough => Meets::Pass,
    }
}

/// A gobo's turn (cos, sin) and drift (the offset its pattern has moved) at `time`: a lamp's
/// pattern turns with where it points (a sun's lies on the world), by Gobo turn and Spin.
fn gobo_turn(s: &LightSettings, dir: Vec2, time: f64) -> [f32; 4] {
    let t = time as f32;
    let aim = if s.kind == LightType::Point || s.kind == LightType::Sun { 0.0 } else { dir.y.atan2(dir.x) };
    let turn = aim + (s.gobo_turn + s.gobo_spin * t).to_radians();
    let drift = s.gobo_drift * t;
    [turn.cos(), turn.sin(), drift.x, drift.y]
}

/// How much of its strength a flickering light gives at `time` (1 with no Flicker): pure in
/// the time and the seed, so every frame drawn again, scrubbed or exported is the same.
pub fn flicker(time: f64, amount: f32, speed: f32, style: FlickerStyle, seed: u32) -> f32 {
    if amount == 0.0 || !amount.is_finite() {
        return 1.0;
    }
    let x = time * speed.max(0.0) as f64;
    let n = match style {
        // Smooth wandering noise, two speeds over each other.
        FlickerStyle::Candle => 0.65 * smooth_noise(x, seed) + 0.35 * smooth_noise(2.7 * x + 13.0, seed ^ 0x9e37),
        // Mostly on, now and then a short dropout (about one beat in six).
        FlickerStyle::Faulty => {
            let k = x.floor();
            let r = hash(k as i64, seed);
            if r > 0.83 {
                let f = (x - k) as f32;
                // Out for the middle of the beat, with a stutter.
                if (0.15..0.75).contains(&f) {
                    if hash((x * 7.0).floor() as i64, seed ^ 0x51) > 0.3 { 1.0 } else { 0.4 }
                } else {
                    0.0
                }
            } else {
                0.0
            }
        }
        FlickerStyle::Pulse => {
            let w = (std::f64::consts::TAU * x).sin() as f32;
            0.5 - 0.5 * w
        }
    };
    (1.0 - amount * n).max(0.0)
}

/// A number from 0 to 1 for whole number `k` and `seed`.
pub(crate) fn hash(k: i64, seed: u32) -> f32 {
    let mut h = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (seed as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 29;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Value noise from 0 to 1, smooth between whole numbers.
pub(crate) fn smooth_noise(x: f64, seed: u32) -> f32 {
    let k = x.floor();
    let f = (x - k) as f32;
    let f = f * f * (3.0 - 2.0 * f);
    let (a, b) = (hash(k as i64, seed), hash(k as i64 + 1, seed));
    a + (b - a) * f
}

/// What a light with Colour from Picked takes its colour from: a thing's layer's main colour, or
/// for a Body of balls each ball's own colour.
enum Picked<'a> {
    None,
    One([f32; 3]),
    EachBall { thing: ThingId, colours: Box<dyn Fn(usize) -> [f32; 4] + 'a>, first: usize },
}

impl<'a> Picked<'a> {
    fn of(cx: &DrawCx<'a>, s: &LightSettings) -> Picked<'a> {
        if s.colour_from != LightColour::Picked {
            return Picked::None;
        }
        let Some(id) = cx.world.matching(&s.pick_from, cx.resolved.id).into_iter().find(|&id| id != cx.resolved.id)
        else {
            return Picked::None;
        };
        let (Some(thing), Some(resolved)) = (cx.tree.things.get(&id), cx.scene.things.get(&id)) else {
            return Picked::None;
        };
        // A Body of balls: each ball's own colour, as it is drawn.
        if s.pick_layer <= 1
            && let Some(bodies) = cx.world.bodies.get(&id)
        {
            let theirs = DrawCx { thing, resolved, ..*cx };
            if let Some(colours) = crate::things::balls::draw::ball_colors(&theirs) {
                let first = (0..bodies.len()).find(|&r| bodies.alive[r]).unwrap_or(0);
                return Picked::EachBall { thing: id, colours: Box::new(colours), first };
            }
        }
        layer_colour(resolved, s.pick_layer).map_or(Picked::None, Picked::One)
    }

    /// The colour for the lamp at `place`: the ball it rides when it rides one of the picked
    /// balls, else the first ball's.
    fn colour(&self, place: &Place) -> Option<[f32; 3]> {
        match self {
            Picked::None => None,
            Picked::One(c) => Some(*c),
            Picked::EachBall { thing, colours, first } => {
                let row = match place.ball {
                    Some((t, row)) if t == *thing => row as usize,
                    _ => *first,
                };
                let c = colours(row);
                Some([c[0], c[1], c[2]])
            }
        }
    }
}

/// The main colour (linear) of layer number `number` (1 the base layer, sub-layers not counted) of
/// a thing's Look: its Colour, or its gradient at the middle.
fn layer_colour(resolved: &bpe_core::ResolvedThing, number: u32) -> Option<[f32; 3]> {
    let look = resolved.slots.get(&bpe_core::SlotId::Look)?;
    let layer = look.layers.iter().filter(|l| l.parent.is_none()).nth(number.max(1) as usize - 1)?;
    let params = look.layer_params(layer.id);
    let c = ["colour", "colours", "color"].iter().find_map(|k| match params.get(k) {
        Some(bpe_core::Value::Color(c)) => Some(c.to_linear()),
        Some(bpe_core::Value::Gradient(g)) => Some(g.sample(0.5)),
        _ => None,
    })?;
    Some([c[0], c[1], c[2]])
}

/// Which way the light at `place` points at `time`, in radians clockwise from the right: its
/// Direction, turned by Spin, the way the ball it rides is going, or at its pen or thing.
pub fn aim_angle(s: &LightSettings, place: &Place, time: f64) -> f32 {
    match s.points {
        Points::FixedAngle => s.direction.to_radians(),
        Points::WhereItsGoing if place.velocity.length_squared() > 1e-6 => place.velocity.y.atan2(place.velocity.x),
        Points::WhereItsGoing => s.direction.to_radians(),
        Points::Spins => (s.direction + s.spin * time as f32).to_radians(),
        Points::AtThePen | Points::AtAThing => {
            match place.aim.map(|a| a - place.at).filter(|d| d.length_squared() > 1e-6) {
                Some(d) => d.y.atan2(d.x),
                None => s.direction.to_radians(),
            }
        }
    }
}

/// Adds a strip light along `points` (an open line) to the scene: `colour` its colour times
/// strength (linear; below 0 it takes light away), `reach` how far it lights, `width` how wide the
/// strip is. It casts no shadows. Electric arcs light the scene with these.
pub fn push_strip(scene: &mut LightScene, points: &[Vec2], colour: [f32; 3], reach: f32, width: f32) {
    if points.len() < 2 {
        return;
    }
    let first = scene.segments.len() as f32;
    for pair in points.windows(2) {
        scene.segments.push([pair[0].x, pair[0].y, pair[1].x, pair[1].y]);
    }
    let middle = points.iter().copied().sum::<Vec2>() / points.len() as f32;
    let size = 0.5 * width.max(0.0);
    scene.lights.push(LightGpu {
        p0: [middle.x, middle.y, ARC_HEIGHT, LightType::Strip.code()],
        p1: [colour[0], colour[1], colour[2], 0.0],
        p2: [reach.max(0.0), Falloff::Soft.code(), size, 0.0],
        p3: [1.0, 0.0, 1.0, 1.0],
        p4: [width.max(0.0), 0.0, first, (points.len() - 1) as f32],
        p5: [0.0; 4],
        p6: [0.0; 4],
        p7: [0.0; 4],
        ..LightGpu::ZERO
    });
}

/// How high above the picture a strip light from [`push_strip`] hangs.
const ARC_HEIGHT: f32 = 60.0;

/// How much the light flares now, on top of its strength (1 doubles it): its Flare times the hits
/// it hears, each dying away over its Flare length; plus a rule's Flash on it or on a rig it is in
/// (W12), each as strong as its Strength says, times its Flare (or once, when Flare is 0).
fn flare(cx: &DrawCx, s: &LightSettings, place: &Place) -> f32 {
    let world = cx.world;
    let rate = world.steps_per_second.max(1);
    let length = s.flare_length.max(1e-3) as f64;
    let first = ((cx.time - length) * rate as f64).ceil().max(0.0) as u64;
    let last = (cx.time * rate as f64 + 1e-6).floor().max(0.0) as u64 + 1;
    let me = cx.resolved.id;
    let hits_heard = s.flare != 0.0;
    let heard = if hits_heard && place.ball.is_none() { world.matching(&s.flare_from, me) } else { Vec::new() };
    let mut rigs: Option<Vec<ThingId>> = None;
    let (mut hits, mut flashes) = (0.0f32, 0.0f32);
    for e in cx.log.between(first, last) {
        let age = ((cx.time - e.time(rate)) / length).clamp(0.0, 1.0) as f32;
        let fade = (1.0 - age) * (1.0 - age);
        match e.kind {
            EventKind::Hit if hits_heard => {
                let by = match place.ball {
                    Some((thing, row)) => {
                        (e.a.thing == thing && e.a.item == row)
                            || e.b.is_some_and(|b| b.thing == thing && b.item == row)
                    }
                    None => heard.contains(&e.a.thing),
                };
                if by {
                    hits += (e.speed / FULL_HIT).min(1.0) * fade;
                }
            }
            EventKind::Flash => {
                let on_me = e.a.thing == me
                    || rigs
                        .get_or_insert_with(|| crate::things::rig::fixtures::rigs_of(cx.tree, me))
                        .contains(&e.a.thing);
                if on_me && e.speed.is_finite() {
                    flashes += (e.speed / bpe_runtime::rules::FULL_FLASH_SPEED).max(0.0) * fade;
                }
            }
            _ => {}
        }
    }
    let gain = if s.flare != 0.0 { s.flare } else { 1.0 };
    s.flare * hits + gain * flashes
}

/// The outline a strip runs along: the first thing its Shape setting names that is a wall.
fn strip_outline(world: &World, s: &LightSettings, me: ThingId) -> Vec<Vec2> {
    world
        .matching(&s.strip_shape, me)
        .into_iter()
        .find_map(|id| world.walls.get(&id))
        .map(|w| outline_points(&w.outline))
        .unwrap_or_default()
}

/// The middle of an outline.
pub fn outline_centre(outline: &Outline) -> Vec2 {
    match outline {
        Outline::Ring { center, .. }
        | Outline::Box { center, .. }
        | Outline::Ellipse { center, .. }
        | Outline::Path { center, .. }
        | Outline::Letters { center, .. } => *center,
    }
}

/// An outline's centre line in the world as a closed list of points, starting at its top.
pub fn outline_points(outline: &Outline) -> Vec<Vec2> {
    let round = |center: Vec2, half: Vec2, angle: f32| -> Vec<Vec2> {
        let turn = Vec2::from_angle(angle);
        (0..ROUND_POINTS)
            .map(|k| {
                let t = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * k as f32 / ROUND_POINTS as f32;
                center + turn.rotate(Vec2::new(half.x * t.cos(), half.y * t.sin()))
            })
            .collect()
    };
    match outline {
        Outline::Ring { center, radius, sides, .. } if *sides < 3 => round(*center, Vec2::splat(*radius), 0.0),
        Outline::Ring { .. } | Outline::Box { .. } => outline.corners(),
        Outline::Ellipse { center, half, angle, .. } => round(*center, *half, *angle),
        Outline::Path { center, angle, scale, points, .. } => {
            let turn = Vec2::from_angle(*angle);
            points.iter().map(|p| *center + turn.rotate(*p * *scale)).collect()
        }
        // A Text's letters: its longest contour (what wants one closed list gets one letter's
        // outline; what wants every side reads `outline_contours`).
        Outline::Letters { .. } => outline_contours(outline).into_iter().max_by_key(Vec::len).unwrap_or_default(),
    }
}

/// Every closed contour of an outline's centre line in the world: one for every outline but a
/// Text's letters, which give each letter's every contour.
pub fn outline_contours(outline: &Outline) -> Vec<Vec<Vec2>> {
    match outline {
        Outline::Letters { center, angle, letters, .. } => {
            let turn = Vec2::from_angle(*angle);
            letters
                .list
                .iter()
                .flat_map(|l| l.contours())
                .map(|c| c.iter().map(|p| *center + turn.rotate(*p)).collect())
                .collect()
        }
        _ => vec![outline_points(outline)],
    }
}

/// The point `share` of the way round a closed list of points (0 and 1 its first point).
pub fn point_round(points: &[Vec2], share: f32) -> Vec2 {
    let n = points.len();
    if n == 0 {
        return Vec2::ZERO;
    }
    let side = |k: usize| points[k].distance(points[(k + 1) % n]);
    let total: f32 = (0..n).map(side).sum();
    if total <= 0.0 {
        return points[0];
    }
    let mut left = share.rem_euclid(1.0) * total;
    for k in 0..n {
        let d = side(k);
        if left <= d {
            return points[k].lerp(points[(k + 1) % n], left / d.max(1e-9));
        }
        left -= d;
    }
    points[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_goes_round_by_its_share() {
        let square = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        assert_eq!(point_round(&square, 0.0), Vec2::ZERO);
        assert_eq!(point_round(&square, 0.125), Vec2::new(5.0, 0.0));
        assert_eq!(point_round(&square, 0.5), Vec2::new(10.0, 10.0));
        assert_eq!(point_round(&square, 1.25), Vec2::new(10.0, 0.0));
    }

    #[test]
    fn a_light_points_by_its_direction_its_spin_or_its_ball() {
        let s = LightSettings { direction: 90.0, spin: 30.0, ..LightSettings::default() };
        let place = Place { velocity: Vec2::new(-5.0, 0.0), ..Place::default() };
        let close = |a: f32, b: f32| (a - b.to_radians()).abs() < 1e-5;
        assert!(close(aim_angle(&s, &place, 2.0), 90.0));
        let spins = LightSettings { points: Points::Spins, ..s.clone() };
        assert!(close(aim_angle(&spins, &place, 2.0), 150.0));
        let going = LightSettings { points: Points::WhereItsGoing, ..s.clone() };
        assert!(close(aim_angle(&going, &place, 2.0), 180.0));
        assert!(close(aim_angle(&going, &Place::default(), 2.0), 90.0), "standing still: its Direction");
    }

    #[test]
    fn a_light_points_at_a_thing_or_one_of_its_balls() {
        use crate::testing::{BALLS, SHAPE, scene_with_balls};
        let (scene, resolved, kit) = scene_with_balls("ring", |_| {});
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let me = ThingId(99);
        let s = LightSettings {
            kind: LightType::Spot,
            points: Points::AtAThing,
            aim_at: bpe_core::Target::Thing(BALLS),
            aim_ball: 2,
            ..LightSettings::default()
        };
        let b = world.bodies.get_mut(&BALLS).unwrap();
        let row = b.row_of(1).unwrap();
        b.position[row] = Vec2::new(100.0, 100.0);
        assert_eq!(aim_at(&world, &s, me), Some(Vec2::new(100.0, 100.0)), "ball number 2");
        let mut places = vec![Place::default()];
        aim_places(&world, &s, me, &mut places);
        let close = |a: f32, b: f32| (a - b.to_radians()).abs() < 1e-5;
        assert!(close(aim_angle(&s, &places[0], 0.0), 45.0), "{}", aim_angle(&s, &places[0], 0.0).to_degrees());
        // 0: the middle of the live balls.
        let b = &world.bodies[&BALLS];
        let middle = (0..b.len()).map(|r| b.position[r]).sum::<Vec2>() / b.len() as f32;
        let mid = LightSettings { aim_ball: 0, ..s.clone() };
        assert!(aim_at(&world, &mid, me).unwrap().distance(middle) < 1e-3);
        // A shape: its outline's middle.
        let at_shape = LightSettings { aim_at: bpe_core::Target::Thing(SHAPE), ..s.clone() };
        let centre = outline_centre(&world.walls[&SHAPE].outline);
        assert_eq!(aim_at(&world, &at_shape, me), Some(centre));
        // Nothing named there: its Direction; a point light, or another Points, aims nowhere.
        let gone = LightSettings { aim_at: bpe_core::Target::Thing(ThingId(77)), direction: 30.0, ..s.clone() };
        assert_eq!(aim_at(&world, &gone, me), None);
        assert!(close(aim_angle(&gone, &Place::default(), 0.0), 30.0));
        assert_eq!(aim_at(&world, &LightSettings { kind: LightType::Point, ..s.clone() }, me), None);
        assert_eq!(aim_at(&world, &LightSettings { points: Points::FixedAngle, ..s.clone() }, me), None);
        // Every ball gone: no point.
        world.bodies.get_mut(&BALLS).unwrap().alive.iter_mut().for_each(|a| *a = false);
        assert_eq!(aim_at(&world, &s, me), None);
    }

    #[test]
    fn a_light_at_the_root_sits_at_its_position() {
        let kit = crate::kit();
        let scene = crate::new_studio_scene(&kit.registry);
        let world = World::build(&scene, &bpe_core::resolve(&scene, &kit.registry), &kit.sims);
        let s = LightSettings { position: Vec2::new(3.0, 4.0), ..LightSettings::default() };
        let at = places_in(&world, Some(ThingId::ROOT), &s);
        assert_eq!(at.len(), 1);
        assert_eq!(at[0].at, Vec2::new(3.0, 4.0));
    }

    #[test]
    fn flicker_stays_between_dark_and_full_and_is_pure_in_time() {
        for style in [FlickerStyle::Candle, FlickerStyle::Faulty, FlickerStyle::Pulse] {
            let at = |t: f64| flicker(t, 0.7, 8.0, style, 3);
            let seen: Vec<f32> = (0..400).map(|k| at(k as f64 / 60.0)).collect();
            assert!(seen.iter().all(|f| (0.0..=1.0).contains(f)), "{style:?}");
            assert!(seen.iter().any(|f| *f < 0.9), "{style:?} flickers");
            assert_eq!(seen, (0..400).map(|k| at(k as f64 / 60.0)).collect::<Vec<_>>());
        }
        assert_eq!(flicker(1.3, 0.0, 8.0, FlickerStyle::Candle, 3), 1.0, "no flicker: full");
        assert_ne!(
            (0..50).map(|k| flicker(k as f64 / 10.0, 1.0, 4.0, FlickerStyle::Candle, 1)).collect::<Vec<_>>(),
            (0..50).map(|k| flicker(k as f64 / 10.0, 1.0, 4.0, FlickerStyle::Candle, 2)).collect::<Vec<_>>(),
            "another seed, another flicker"
        );
    }

    #[test]
    fn a_round_outline_starts_at_its_top() {
        let ring = Outline::Ring { center: Vec2::new(5.0, 5.0), radius: 10.0, sides: 0, angle: 0.0, thickness: 4.0 };
        let points = outline_points(&ring);
        assert_eq!(points.len(), ROUND_POINTS);
        assert!(points[0].distance(Vec2::new(5.0, -5.0)) < 1e-4, "{:?}", points[0]);
    }
}
