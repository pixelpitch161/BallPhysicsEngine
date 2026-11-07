//! Drawing a Particles thing: nothing of its own, then each shown Look layer as an around layer
//! with a blend of its own (so other blends route through the frame's layers as trails' do),
//! every particle it draws as a sprite with what its sub-layers add (Soft smoke and Flame as one
//! soft field the GPU splats and shades, Web lines as lines between them, Trails and Marks as
//! strips and sprites from the state, Liquid as blobs the GPU runs together), then the layer's
//! Experimental modifiers worked over what it drew. Particles carry no lighting yet (their
//! `light`, `emit` and `extra` stay 0).

use std::f32::consts::TAU;

use bpe_core::{Blend, ResolvedThing, SlotId, ThingId};
use bpe_gfx::{
    Around, DrawCx, FIELD_FLAME, FIELD_HEAT_STOPS, FIELD_SMOKE, Field, FieldLook, Frame, Liquid, MOST_BLOBS, Particle,
    Splat, ThingDraw,
};
use bpe_runtime::EventKind;
use glam::Vec2;

use super::KIND;
use super::births::{signed, unit};
use super::liquid::LiquidLayer;
use super::look::{
    ColourFrom, Common, Drawn, FineDetail, FlameLayer, GlitterLayer, LookLayer, MoreShape, Only, PointAlong,
    ShapesLayer, SmokeLayer, WebLayer, layers_of,
};
use super::marks::Mark;
use super::modifiers::{
    MirrorRound, Modifiers, Tears, glitch, glitch_blobs, glitch_splats, mirror, mirror_blobs, mirror_splats, split_into,
};
use super::sim::state_of;
use super::state::{ParticlesState, Speck};
use super::trail::draw_one;
use super::web::{MOST_WEB_LINES, ball_links, links};
use crate::modules::look::layers::around_subs::{Extras, HitInfo, OWN, Pieces, coloured, particle_with_extras};
use crate::modules::look::layers::gpu::SubLayer;
use crate::modules::look::layers::particles::ShapeMadeOf;
use crate::modules::look::layers::trail::AroundPlace;
use crate::paint::{Inputs, colour_at};
use crate::things::balls::draw::hit_strength;

pub struct ParticlesDraw;

/// The thing's shown Look layers, bottom first.
fn layers(resolved: &ResolvedThing) -> Vec<LookLayer> {
    match resolved.slots.get(&SlotId::Look) {
        Some(look) if look.kind == super::look::CARD => layers_of(look),
        _ => Vec::new(),
    }
}

impl ThingDraw for ParticlesDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// The particles draw as around layers; the thing itself draws nothing.
    fn draw(&self, _cx: &DrawCx, _frame: &mut Frame) {}

    /// The thing itself never blends (its layers do, each on its own).
    fn blend(&self, _resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        Some((Blend::Normal, 1.0, false))
    }

    fn around(&self, resolved: &ResolvedThing) -> Vec<Around> {
        layers(resolved)
            .iter()
            .map(|l| Around {
                blend: l.common.blend,
                amount: l.common.blend_amount,
                behind: l.common.place == AroundPlace::Behind,
            })
            .collect()
    }

    fn draw_around(&self, cx: &DrawCx, index: usize, frame: &mut Frame) {
        let Some(layer) = layers(cx.resolved).into_iter().nth(index) else { return };
        let Some(st) = state_of(cx.world, cx.thing.id) else { return };
        if let Drawn::Liquid(l) = &layer.drawn {
            frame.liquids.extend(liquid_of(cx, st, &layer, l));
            return;
        }
        let mut pieces = Pieces::default();
        let field = draw_layer(cx, st, &layer, &mut pieces);
        pieces.into_frame_with_field(frame, layer.common.place == AroundPlace::InFront, field);
    }
}

/// Whether `layer` draws particle `s` at particle step `now`.
pub fn draws(c: &Common, s: &Speck, now: u32) -> bool {
    match c.only {
        Only::All => true,
        Only::YoungerThan => s.age(now) < c.age,
        Only::OlderThan => s.age(now) >= c.age,
        Only::AShare => unit(s.seed, 61) < c.share,
        Only::TheRest => unit(s.seed, 61) >= c.share,
    }
}

/// A particle's colour from `base` (linear light) at `alpha`, mixed in by the blend amount:
/// covering, or adding its light. A colour with no alpha, or a layer that adds, adds its light.
pub fn shade(c: &Common, base: [f32; 4], alpha: f32) -> [f32; 4] {
    let amount = match c.blend {
        Blend::Normal | Blend::Add => c.blend_amount.clamp(0.0, 1.0),
        _ => 1.0,
    };
    if c.blend != Blend::Add && base[3] > 0.0 {
        return [base[0], base[1], base[2], (base[3] * alpha * amount).min(1.0)];
    }
    [base[0], base[1], base[2], -(alpha * amount)]
}

/// The hits of the thing a Particles thing sits in (its parent), as its colour sub-layers read
/// them: the last before `cx.time` and how many so far.
fn parent_hits(cx: &DrawCx, parent: Option<ThingId>) -> HitInfo {
    let Some(parent) = parent else { return HitInfo::default() };
    let rate = cx.world.steps_per_second;
    let mut info = HitInfo::default();
    for e in cx.log.since(0) {
        let ours = e.a.thing == parent || e.b.is_some_and(|b| b.thing == parent);
        if e.kind != EventKind::Hit || !ours {
            continue;
        }
        let time = e.time(rate);
        if time > cx.time {
            break;
        }
        info.last = Some(((cx.time - time) as f32, hit_strength(e.speed)));
        info.hits += 1;
    }
    info
}

/// One particle as a layer sees it at the moment drawn.
pub struct Seen<'a> {
    pub s: &'a Speck,
    /// How far through its life it is, 0 to 1.
    pub t: f32,
    /// How much of it shows: opacity, fade and twinkle.
    pub alpha: f32,
    /// Its half size, in pixels.
    pub size: f32,
    /// Its colour before the sub-layers (linear light).
    pub base: [f32; 4],
    /// Its colour after them, drawn as the layer draws (see [`shade`]).
    pub colour: [f32; 4],
}

/// Every particle of `st` that `layer` draws, as it sees them.
pub fn seen<'a>(cx: &DrawCx, st: &'a ParticlesState, layer: &LookLayer, hit: HitInfo) -> Vec<Seen<'a>> {
    let c = &layer.common;
    let mut out = Vec::new();
    if c.opacity <= 0.0 {
        return out;
    }
    let screen = cx.resolved.screen;
    // The colours of the parent's balls, when a layer takes them.
    let parent = st.parent.and_then(|p| Some((cx.tree.things.get(&p)?, cx.scene.things.get(&p)?)));
    let ball_cx = match (c.colour_from, parent) {
        (ColourFrom::ParentBall, Some((thing, resolved))) => Some(DrawCx { thing, resolved, ..*cx }),
        _ => None,
    };
    let ball_colour = ball_cx.as_ref().and_then(crate::things::balls::draw::ball_colors);
    let now = st.step;
    let twinkle = c.twinkle.clamp(0.0, 1.0);
    for s in &st.specks {
        if !draws(c, s, now) {
            continue;
        }
        let t = s.life_at(now);
        let Some(grow) = c.size_curve.at(t) else { continue };
        let age = s.age(now);
        let flicker = 1.0 - twinkle * (0.5 + 0.5 * (c.twinkle_speed * TAU * age + unit(s.seed, 67) * TAU).sin());
        let alpha = (c.opacity * c.fade.at(t) * flicker).max(0.0);
        if alpha <= 0.0 {
            continue;
        }
        let own = || {
            let inputs = Inputs {
                // A stop picked once for each particle, for Follow: Order.
                index: (unit(s.seed, 41) * 1024.0) as usize,
                count: 1024,
                place: s.place,
                place_screen: if screen { s.place } else { cx.world.camera.to_screen(s.place) },
                start: s.home,
                speed: s.velocity.length(),
                life: t,
                time: cx.time as f32,
                seed: s.seed,
                ..Inputs::default()
            };
            colour_at(&c.colour, &inputs)
        };
        let base = match (&ball_colour, s.ball) {
            (Some(of), b) if b > 0 => of(b as usize - 1),
            _ => own(),
        };
        let size = s.size * c.size.max(0.0) * grow;
        if size <= 0.0 {
            continue;
        }
        let subs = &layer.subs;
        let colour = shade(c, coloured(subs, subs.len(), base, hit), alpha);
        out.push(Seen { s, t, alpha, size, base, colour });
    }
    out
}

/// Where Fine detail's copy `k` of a particle of half size `size` sits from it at `time`.
pub fn copy_offset(d: &FineDetail, s: &Speck, k: u32, size: f32, time: f32) -> Vec2 {
    offset_of_seed(d, s.seed, k, size, time)
}

/// [`copy_offset`] for anything with dice `seed` (a mark keeps its particle's).
pub fn offset_of_seed(d: &FineDetail, seed: u32, k: u32, size: f32, time: f32) -> Vec2 {
    let v = Vec2::new(signed(seed, 300 + k), signed(seed, 400 + k)) * d.spread * size;
    if d.shimmer == 0.0 {
        return v;
    }
    Vec2::from_angle(d.shimmer * time * TAU * signed(seed, 500 + k)).rotate(v)
}

/// The most copies Fine detail draws a layer in all: past it each particle gets fewer.
pub const MOST_COPY_SPRITES: usize = 400_000;

/// `d` with no more copies than keep `count` particles' copies within [`MOST_COPY_SPRITES`].
pub fn capped(d: FineDetail, count: usize) -> FineDetail {
    let most = (MOST_COPY_SPRITES / count.max(1)).min(u32::MAX as usize) as u32;
    FineDetail { copies: d.copies.min(most), ..d }
}

/// `p` moved by `by` and sized `k` times, its colour's alpha `fade` times (an added colour's
/// light too).
fn copied(p: &Particle, by: Vec2, k: f32, fade: f32) -> Particle {
    let mut q = *p;
    q.place[0] += by.x;
    q.place[1] += by.y;
    q.place[3] *= k;
    q.style[0] *= k;
    q.color[3] *= fade;
    q
}

/// What a layer reads to colour one particle (or one mark).
pub struct Item {
    pub place: Vec2,
    pub velocity: Vec2,
    pub home: Vec2,
    pub seed: u32,
    /// The parent ball it came from (body row + 1), 0 when none.
    pub ball: u32,
    /// How far through its life it is, 0 to 1.
    pub life: f32,
    /// How old it is, in seconds.
    pub age: f32,
}

impl Item {
    fn of(s: &Speck, now: u32) -> Item {
        Item {
            place: s.place,
            velocity: s.velocity,
            home: s.home,
            seed: s.seed,
            ball: s.ball,
            life: s.life_at(now),
            age: s.age(now),
        }
    }
}

/// How much of an item shows by the layer's opacity, fade and twinkle (before Size over life).
fn alpha_of(c: &Common, it: &Item) -> f32 {
    let twinkle = c.twinkle.clamp(0.0, 1.0);
    let tau = std::f32::consts::TAU;
    let flicker = 1.0 - twinkle * (0.5 + 0.5 * (c.twinkle_speed * tau * it.age + unit(it.seed, 67) * tau).sin());
    (c.opacity * c.fade.at(it.life) * flicker).max(0.0)
}

/// An item's own colour from the layer's Colours.
fn own_colour(cx: &DrawCx, c: &Common, it: &Item) -> [f32; 4] {
    let inputs = Inputs {
        // A stop picked once for each particle, for Follow: Order.
        index: (unit(it.seed, 41) * 1024.0) as usize,
        count: 1024,
        place: it.place,
        place_screen: if cx.resolved.screen { it.place } else { cx.world.camera.to_screen(it.place) },
        start: it.home,
        speed: it.velocity.length(),
        life: it.life,
        time: cx.time as f32,
        seed: it.seed,
        ..Inputs::default()
    };
    colour_at(&c.colour, &inputs)
}

/// Whether a Marks layer draws mark `m` (by age or a share, as Only says).
fn draws_mark(c: &Common, m: &Mark, now: u32) -> bool {
    match c.only {
        Only::All => true,
        Only::YoungerThan => m.age(now) < c.age,
        Only::OlderThan => m.age(now) >= c.age,
        Only::AShare => unit(m.seed, 61) < c.share,
        Only::TheRest => unit(m.seed, 61) >= c.share,
    }
}

/// The longest step a trail is drawn across: longer is a wrap or a leap.
fn longest_jump(cx: &DrawCx) -> f32 {
    let (_, half) = super::births::screen(cx.world, 0.0);
    (half.min_element() * 0.5).max(8.0)
}

/// The view the drawn places are in: the camera's, or the screen's own for a thing stuck to the
/// screen (its places are screen places already).
fn view_of(cx: &DrawCx) -> bpe_runtime::CameraView {
    if cx.resolved.screen { bpe_runtime::CameraView::default() } else { cx.world.camera }
}

/// What Mirror turns its copies round, in the places drawn.
pub fn mirror_centre(cx: &DrawCx, st: &ParticlesState, m: &Modifiers) -> Vec2 {
    let middle = view_of(cx).center;
    let seen = |world: Vec2| if cx.resolved.screen { cx.world.camera.to_screen(world) } else { world };
    match m.round {
        MirrorRound::TheMiddleOfThePicture => middle,
        MirrorRound::APoint => m.centre,
        MirrorRound::TheThingTheyRide => {
            let Some(parent) = st.parent else { return middle };
            if let Some(b) = cx.world.bodies.get(&parent) {
                let alive: Vec<Vec2> = (0..b.len()).filter(|&r| b.alive[r]).map(|r| b.position[r]).collect();
                if !alive.is_empty() {
                    return seen(alive.iter().sum::<Vec2>() / alive.len() as f32);
                }
            }
            cx.world.poses.get(&parent).map_or(middle, |p| seen(p.position))
        }
    }
}

/// Mirror and Glitch over everything `layer` drew (and its field's splats, for Soft smoke and
/// Flame).
fn finish(cx: &DrawCx, st: &ParticlesState, layer: &LookLayer, pieces: &mut Pieces, field: Option<&mut Field>) {
    let m = &layer.modifiers;
    let mut field = field;
    if m.mirror.ways() > 1 {
        let centre = mirror_centre(cx, st, m);
        mirror(pieces, m, centre);
        if let Some(f) = field.as_deref_mut() {
            mirror_splats(&mut f.splats, m, centre);
        }
    }
    if m.glitches() {
        let view = view_of(cx);
        let tears = Tears::new(m, &view, cx.time, layer.id.0);
        glitch(pieces, &tears);
        if let Some(f) = field {
            glitch_splats(&mut f.splats, &tears);
        }
    }
}

/// Draws `layer`'s particles of `st` into `pieces`; Soft smoke and Flame give their field. Its
/// modifiers are worked over what it drew.
pub fn draw_layer(cx: &DrawCx, st: &ParticlesState, layer: &LookLayer, pieces: &mut Pieces) -> Option<Field> {
    let hit = if layer.subs.iter().any(|s| s.reads_hits()) { parent_hits(cx, st.parent) } else { HitInfo::default() };
    let seen = seen(cx, st, layer, hit);
    let c = &layer.common;
    let time = cx.time as f32;
    let extras = |v: &Seen, fade: f32| Extras {
        subs: &layer.subs,
        base: v.base,
        s: v.t,
        fade: v.alpha * fade,
        unit: 1.0,
        hit,
        outline: 1.0,
        added: 1.0,
    };
    match &layer.drawn {
        Drawn::Smoke(_) | Drawn::Flame(_) => {
            let mut field = field_of(cx, layer, &seen, hit, pieces);
            finish(cx, st, layer, pieces, Some(&mut field));
            return Some(field);
        }
        Drawn::Web(w) => draw_web(cx, layer, w, &seen, hit, pieces),
        Drawn::Trails(_) | Drawn::Marks(_) => draw_from_state(cx, st, layer, hit, pieces),
        // Drawn through the frame's liquids (see `liquid_of`).
        Drawn::Liquid(_) => return None,
        _ => {
            let flutters = st.forces.flips();
            let now_s = st.step as f32 * super::step::DT;
            let d = capped(c.detail, seen.len());
            let splits = layer.modifiers.splits();
            let mut scratch = Pieces::default();
            for v in &seen {
                let (own, star) = sprite(&layer.drawn, v.s, v.size, v.colour, flutters, now_s, time);
                // Colour split draws it (and its copies) apart first, then splits it.
                let into = if splits { &mut scratch } else { &mut *pieces };
                particle_with_extras(own, &extras(v, 1.0), into);
                into.particles[OWN].extend(star);
                for k in 0..d.copies {
                    let by = copy_offset(&d, v.s, k, v.size, time);
                    let (size, opacity) = (d.size.max(0.0), d.opacity.clamp(0.0, 1.0));
                    particle_with_extras(copied(&own, by, size, opacity), &extras(v, d.opacity), into);
                    into.particles[OWN].extend(star.map(|s| copied(&s, by, size, opacity)));
                }
                if splits {
                    split_into(&mut scratch, pieces, layer.modifiers.split_of(v.s.velocity));
                }
            }
        }
    }
    finish(cx, st, layer, pieces, None);
    None
}

/// Draws a Trails or Marks layer, which read the state's trail ring and marks.
fn draw_from_state(cx: &DrawCx, st: &ParticlesState, layer: &LookLayer, hit: HitInfo, pieces: &mut Pieces) {
    let c = &layer.common;
    // The colours of the parent's balls, when a layer takes them.
    let parent = st.parent.and_then(|p| Some((cx.tree.things.get(&p)?, cx.scene.things.get(&p)?)));
    let ball_cx = match (c.colour_from, parent) {
        (ColourFrom::ParentBall, Some((thing, resolved))) => Some(DrawCx { thing, resolved, ..*cx }),
        _ => None,
    };
    let ball_colour = ball_cx.as_ref().and_then(crate::things::balls::draw::ball_colors);
    let base_of = |it: &Item| match (&ball_colour, it.ball) {
        (Some(of), b) if b > 0 => of(b as usize - 1),
        _ => own_colour(cx, c, it),
    };
    let now = st.step;
    let subs = &layer.subs;
    let splits = layer.modifiers.splits();
    let time = cx.time as f32;
    let (copy_size, copy_opacity) = (c.detail.size.max(0.0), c.detail.opacity.clamp(0.0, 1.0));
    let mut scratch = Pieces::default();
    // Draws one item through `draw`, split into colours when Colour split asks.
    let mut put = |pieces: &mut Pieces, velocity: Vec2, draw: &mut dyn FnMut(&mut Pieces)| {
        if splits {
            draw(&mut scratch);
            split_into(&mut scratch, pieces, layer.modifiers.split_of(velocity));
        } else {
            draw(pieces);
        }
    };
    match &layer.drawn {
        Drawn::Trails(t) => {
            let jump = longest_jump(cx);
            let d = capped(c.detail, st.specks.len());
            for (i, s) in st.specks.iter().enumerate() {
                if !draws(c, s, now) {
                    continue;
                }
                let it = Item::of(s, now);
                let Some(grow) = c.size_curve.at(it.life) else { continue };
                let alpha = alpha_of(c, &it);
                let size = s.size * c.size.max(0.0) * grow;
                if alpha <= 0.0 || size <= 0.0 {
                    continue;
                }
                let points = st.trail.recent(i, s, now, t.every, t.length);
                if points.len() < 2 {
                    continue;
                }
                let base = base_of(&it);
                put(pieces, s.velocity, &mut |p| draw_one(t, c, subs, &points, base, alpha, size, hit, jump, p));
                // Fine detail: the whole trail again, moved, thinner and fainter.
                for k in 0..d.copies {
                    let by = copy_offset(&d, s, k, size, time);
                    let moved: Vec<Vec2> = points.iter().map(|p| *p + by).collect();
                    let (a, z) = (alpha * copy_opacity, size * copy_size);
                    put(pieces, s.velocity, &mut |p| draw_one(t, c, subs, &moved, base, a, z, hit, jump, p));
                }
            }
        }
        Drawn::Marks(m) => {
            let why = m.why();
            // The screen's down, in the world.
            let down = Vec2::from_angle(cx.world.camera.turn).rotate(Vec2::Y);
            let d = capped(c.detail, st.marks.len());
            for mark in &st.marks {
                if mark.why & why == 0 || !draws_mark(c, mark, now) {
                    continue;
                }
                let age = mark.age(now);
                let shows = m.shows(age);
                let life = m.life(age);
                let it = Item {
                    place: mark.place,
                    velocity: mark.velocity,
                    home: mark.home,
                    seed: mark.seed,
                    ball: mark.ball,
                    life,
                    age,
                };
                let Some(grow) = c.size_curve.at(life) else { continue };
                let alpha = alpha_of(c, &it) * shows;
                let size = mark.size * c.size.max(0.0) * grow;
                if alpha <= 0.0 || size <= 0.0 {
                    continue;
                }
                let base = base_of(&it);
                let colour = shade(c, coloured(subs, subs.len(), base, hit), alpha);
                let extras = Extras { subs, base, s: life, fade: alpha, unit: 1.0, hit, outline: 1.0, added: 1.0 };
                let sprites = super::marks::sprites_of(m, mark, age, size, colour, down);
                let copy_extras = Extras { fade: alpha * copy_opacity, ..extras };
                put(pieces, Vec2::ZERO, &mut |p| {
                    for own in &sprites {
                        particle_with_extras(*own, &extras, p);
                    }
                    for k in 0..d.copies {
                        let by = offset_of_seed(&d, mark.seed, k, size, time);
                        for own in &sprites {
                            particle_with_extras(copied(own, by, copy_size, copy_opacity), &copy_extras, p);
                        }
                    }
                });
            }
        }
        _ => {}
    }
}

/// The angle a shape points at: its own turn or the way it goes, turned `by` more.
fn pointing(s: &Speck, along: PointAlong, by: f32) -> f32 {
    let v = s.velocity;
    let base = match along {
        PointAlong::OwnTurn => s.turn,
        PointAlong::WayItGoes if v.length() > 1e-3 => v.y.atan2(v.x),
        PointAlong::WayItGoes => s.turn,
    };
    base + by.to_radians()
}

/// The sprite for particle `s` drawn by `drawn` at `size` (half size), coloured `colour`, and a
/// star over it when glitter flashes; `flutters` when a force flips the particles over. `now` is
/// the particle step's time and `time` the drawing's, both in seconds.
pub fn sprite(
    drawn: &Drawn,
    s: &Speck,
    size: f32,
    colour: [f32; 4],
    flutters: bool,
    now: f32,
    time: f32,
) -> (Particle, Option<Particle>) {
    let speed = s.velocity.length();
    // Flutter: a flat thing flipping over shows edge on, and darker on its back.
    let flutter = |p: &mut Particle| {
        if flutters {
            let face = s.flip.cos();
            p.place[3] = size * face.abs().max(0.12);
            p.style[1] = 0.7 + 0.3 * face;
        }
    };
    match drawn {
        Drawn::Sprites { made_of, softness } => {
            let mut p = Particle::new(made_of.as_ball().code(), s.place, s.turn, size, colour);
            p.style[0] = match made_of {
                ShapeMadeOf::Sparks => size + speed * 0.03,
                ShapeMadeOf::Streaks => size * 2.0 + speed * 0.08,
                ShapeMadeOf::Confetti => size * 1.6,
                ShapeMadeOf::Shards => size * 1.4,
                _ => size,
            };
            p.style[2] = softness.clamp(0.0, 1.0);
            flutter(&mut p);
            (p, None)
        }
        Drawn::Streaks { stretch, least, width } => {
            let half_width = (size * width).max(0.0);
            let length = (speed * stretch).max(*least).max(0.0);
            let angle = if speed > 1e-3 { s.velocity.y.atan2(s.velocity.x) } else { s.turn };
            let mut p = Particle::new(ShapeMadeOf::Streaks.as_ball().code(), s.place, angle, half_width, colour);
            p.style[0] = (length * 0.5).max(half_width);
            (p, None)
        }
        Drawn::Shapes(l) => (shape_sprite(l, s, size, colour, now, flutter), None),
        Drawn::Glitter(l) => glitter_sprite(l, s, size, colour, time),
        // Fields and lines are drawn their own way; a plain dot stands in.
        Drawn::Smoke(_) | Drawn::Flame(_) | Drawn::Web(_) | Drawn::Trails(_) | Drawn::Marks(_) | Drawn::Liquid(_) => {
            (Particle::new(ShapeMadeOf::Dots.as_ball().code(), s.place, s.turn, size, colour), None)
        }
    }
}

/// More shapes' sprite for `s`.
fn shape_sprite(
    l: &ShapesLayer,
    s: &Speck,
    size: f32,
    colour: [f32; 4],
    now: f32,
    flutter: impl Fn(&mut Particle),
) -> Particle {
    let (code, length) = l.shape.code_and_length();
    if l.shape == MoreShape::Pixels {
        let place = if l.grid > 0.0 { (s.place / l.grid).round() * l.grid } else { s.place };
        let mut p = Particle::new(code, place, 0.0, size, colour);
        p.style[2] = l.softness.clamp(0.0, 1.0);
        return p;
    }
    let angle = match (l.shape, l.point_along) {
        // A bird seen from the front leans into the way it goes rather than pointing along it.
        (MoreShape::Birds, PointAlong::WayItGoes) => {
            let v = s.velocity;
            0.5 * (v.x / v.length().max(1.0)) + l.turn_by.to_radians()
        }
        _ => pointing(s, l.point_along, l.turn_by),
    };
    let mut p = Particle::new(code, s.place, angle, size, colour);
    p.style[0] = if l.shape == MoreShape::Flakes { size } else { size * length * l.length.max(0.0) };
    p.style[2] = l.softness.clamp(0.0, 1.0);
    if matches!(l.shape, MoreShape::Birds | MoreShape::Fish) {
        let age = (now - s.born as f32 * super::step::DT).max(0.0);
        let pose = (age * l.flap_speed * TAU + unit(s.seed, 73) * TAU).sin();
        p.shape[1] = ((pose * 0.5 + 0.5).clamp(0.0, 1.0) * 65535.0) as u32;
    }
    flutter(&mut p);
    p
}

/// How brightly glitter particle `s`'s facet flashes at `time`, 0 to 1.
pub fn glint(l: &GlitterLayer, s: &Speck, time: f32) -> f32 {
    let facet = s.turn + unit(s.seed, 71) * TAU + l.shimmer * time * TAU * signed(s.seed, 72);
    let facing = (facet - l.light_from.to_radians()).cos().max(0.0);
    facing.powf(l.sharpness.max(1.0))
}

/// Glitter's facet for `s`, and the star of its flash.
fn glitter_sprite(l: &GlitterLayer, s: &Speck, size: f32, colour: [f32; 4], time: f32) -> (Particle, Option<Particle>) {
    let flash = glint(l, s, time);
    let bright = l.base.clamp(0.0, 1.0) + flash;
    let lit = [colour[0] * bright, colour[1] * bright, colour[2] * bright, colour[3]];
    // A diamond a little longer than it is wide.
    let mut facet = Particle::new(16, s.place, s.turn, size * 0.75, lit);
    facet.style[0] = size;
    let star = (flash > 0.02 && l.star > 0.0).then(|| {
        let f = l.flash_colour.to_linear();
        let alpha = colour[3].abs() * flash;
        let mut p = Particle::new(3, s.place, s.turn, size * l.star * flash, [f[0], f[1], f[2], -alpha]);
        p.style[2] = 0.0;
        p
    });
    (facet, star)
}

/// The sub-layers that add sprites round a particle of their own (a shadow, a glow, an outline).
fn adds_round(subs: &[SubLayer]) -> bool {
    subs.iter().any(|s| matches!(s, SubLayer::Shadow(_) | SubLayer::Glow(_) | SubLayer::Outline(_)))
}

/// Soft smoke's or Flame's field of `seen`, with what its sub-layers add round each particle in
/// `pieces` (shadows and glows under it, outlines over it).
fn field_of(cx: &DrawCx, layer: &LookLayer, seen: &[Seen], hit: HitInfo, pieces: &mut Pieces) -> Field {
    let c = &layer.common;
    let time = cx.time as f32;
    let d = capped(c.detail, seen.len());
    let amount = match c.blend {
        Blend::Normal | Blend::Add => c.blend_amount.clamp(0.0, 1.0),
        _ => 1.0,
    };
    // A layer that adds, or its own colours all with no alpha, add their light (the layer
    // decides, so the field never flips with which particle is oldest).
    let adds = c.blend == Blend::Add
        || (c.colour_from == ColourFrom::Own && c.colour.colours.stops.iter().all(|s| s.color.a == 0));
    let (look, puff) = match &layer.drawn {
        Drawn::Flame(f) => (flame_look(f, adds, amount), f.puff),
        Drawn::Smoke(s) => (smoke_look(s, adds, amount), s.puff),
        _ => unreachable!("only smoke and flame are fields"),
    };
    let cools = match &layer.drawn {
        Drawn::Flame(f) => f.cools.clamp(0.0, 1.0),
        _ => 0.0,
    };
    let flame = matches!(layer.drawn, Drawn::Flame(_));
    let mut splats = Vec::with_capacity(seen.len() * (1 + d.copies as usize));
    let subs = adds_round(&layer.subs);
    for v in seen {
        let tint = coloured(&layer.subs, layer.subs.len(), v.base, hit);
        let weight =
            if flame { v.alpha * (1.0 - cools * v.t) } else { v.alpha * if tint[3] > 0.0 { tint[3] } else { 1.0 } };
        if weight <= 0.0 {
            continue;
        }
        let radius = v.size * puff.max(0.0);
        let splat =
            |at: Vec2, r: f32, w: f32| Splat { place: [at.x, at.y, r, w], color: [tint[0], tint[1], tint[2], 0.0] };
        splats.push(splat(v.s.place, radius, weight));
        for k in 0..d.copies {
            let by = copy_offset(&d, v.s, k, v.size, time);
            splats.push(splat(v.s.place + by, radius * d.size.max(0.0), weight * d.opacity.clamp(0.0, 1.0)));
        }
        if subs {
            // The particle as a soft puff, only for what its sub-layers draw round it.
            let mut puff_sprite = Particle::new(ShapeMadeOf::Smoke.as_ball().code(), v.s.place, 0.0, radius, v.colour);
            puff_sprite.style[2] = 1.0;
            let e = Extras {
                subs: &layer.subs,
                base: v.base,
                s: v.t,
                fade: v.alpha,
                unit: 1.0,
                hit,
                outline: 1.0,
                added: 1.0,
            };
            particle_with_extras(puff_sprite, &e, pieces);
        }
    }
    // The field stands for what the layer itself draws.
    pieces.particles[OWN].clear();
    Field { at: 0, splats, look }
}

fn smoke_look(s: &SmokeLayer, adds: bool, amount: f32) -> FieldLook {
    let way = Vec2::from_angle(s.drift_way.to_radians()) * s.drift;
    FieldLook {
        kind: [FIELD_SMOKE, if adds { 1.0 } else { 0.0 }, amount, s.thickness.max(0.0)],
        detail: [s.detail.clamp(0.0, 1.0), s.detail_size.max(1.0), way.x, way.y],
        shade: [s.shading.clamp(0.0, 1.0), s.light_from.to_radians(), s.dark_inside.clamp(0.0, 1.0), 0.0],
        ..FieldLook::default()
    }
}

fn flame_look(f: &FlameLayer, adds: bool, amount: f32) -> FieldLook {
    let mut heat = [[0.0; 4]; FIELD_HEAT_STOPS];
    for (h, c) in heat.iter_mut().zip(f.heat_colours.bake(FIELD_HEAT_STOPS)) {
        *h = c;
    }
    FieldLook {
        kind: [FIELD_FLAME, if adds { 1.0 } else { 0.0 }, amount, f.heat.max(0.0)],
        detail: [f.detail.clamp(0.0, 1.0), f.detail_size.max(1.0), 0.0, -f.rise],
        shade: [f.bend.max(0.0), f.flicker.clamp(0.0, 1.0), f.flicker_speed, f.brightness.max(0.0)],
        heat,
        ..FieldLook::default()
    }
}

/// Web lines: lines between the particles `seen` within Reach (and to balls), then a dot at each.
fn draw_web(cx: &DrawCx, layer: &LookLayer, w: &WebLayer, seen: &[Seen], hit: HitInfo, pieces: &mut Pieces) {
    let c = &layer.common;
    let places: Vec<Vec2> = seen.iter().map(|v| v.s.place).collect();
    let reach = w.reach.max(1.0);
    let half_width = (w.line_width * 0.5).max(0.0);
    let line_alpha = |d: f32, of: f32| {
        let near = (1.0 - d / of).clamp(0.0, 1.0);
        w.line_opacity.clamp(0.0, 1.0) * if w.fade_power > 0.0 { near.powf(w.fade_power) } else { 1.0 }
    };
    let line = |a: Vec2, b: Vec2, colour: [f32; 4]| {
        let along = b - a;
        let mut p = Particle::new(9, (a + b) * 0.5, along.y.atan2(along.x), half_width, colour);
        p.style[0] = (along.length() * 0.5).max(half_width);
        p
    };
    let mean =
        |x: [f32; 4], y: [f32; 4]| [(x[0] + y[0]) * 0.5, (x[1] + y[1]) * 0.5, (x[2] + y[2]) * 0.5, (x[3] + y[3]) * 0.5];
    let extras = |base: [f32; 4], t: f32, fade: f32| Extras {
        subs: &layer.subs,
        base,
        s: t,
        fade,
        unit: 1.0,
        hit,
        outline: 1.0,
        added: 1.0,
    };
    if half_width > 0.0 && w.line_opacity > 0.0 {
        for (i, j, d) in links(&places, reach, w.links as usize) {
            let (a, b) = (&seen[i], &seen[j]);
            let k = line_alpha(d, reach);
            if k <= 0.0 {
                continue;
            }
            let mut colour = mean(a.colour, b.colour);
            colour[3] *= k;
            let fade = (a.alpha + b.alpha) * 0.5 * k;
            particle_with_extras(line(a.s.place, b.s.place, colour), &extras(mean(a.base, b.base), a.t, fade), pieces);
        }
        if w.to_balls && w.ball_reach > 0.0 {
            let balls = super::step::balls_in(cx.world);
            for (i, edge, gap) in ball_links(&places, &balls, w.ball_reach, MOST_WEB_LINES) {
                let v = &seen[i];
                let k = line_alpha(gap, w.ball_reach);
                if k <= 0.0 {
                    continue;
                }
                let colour = [v.colour[0], v.colour[1], v.colour[2], v.colour[3] * k];
                particle_with_extras(line(v.s.place, edge, colour), &extras(v.base, v.t, v.alpha * k), pieces);
            }
        }
    }
    if w.dots > 0.0 {
        let d = capped(c.detail, seen.len());
        let time = cx.time as f32;
        for v in seen {
            let size = v.size * w.dots;
            let dot = Particle::new(ShapeMadeOf::Dots.as_ball().code(), v.s.place, 0.0, size, v.colour);
            particle_with_extras(dot, &extras(v.base, v.t, v.alpha), pieces);
            for k in 0..d.copies {
                let by = copy_offset(&d, v.s, k, size, time);
                let copy = copied(&dot, by, d.size.max(0.0), d.opacity.clamp(0.0, 1.0));
                particle_with_extras(copy, &extras(v.base, v.t, v.alpha * d.opacity), pieces);
            }
        }
    }
}

/// The liquid `layer` (whose own settings are `l`) makes of `st`'s particles: a blob each, its
/// modifiers worked over them.
pub fn liquid_of(cx: &DrawCx, st: &ParticlesState, layer: &LookLayer, l: &LiquidLayer) -> Option<Liquid> {
    let c = &layer.common;
    if c.opacity <= 0.0 {
        return None;
    }
    let parent = st.parent.and_then(|p| Some((cx.tree.things.get(&p)?, cx.scene.things.get(&p)?)));
    let ball_cx = match (c.colour_from, parent) {
        (ColourFrom::ParentBall, Some((thing, resolved))) => Some(DrawCx { thing, resolved, ..*cx }),
        _ => None,
    };
    let ball_colour = ball_cx.as_ref().and_then(crate::things::balls::draw::ball_colors);
    let now = st.step;
    // Mirror's copies count against the cap too, so every copy shows whole.
    let most = MOST_BLOBS / layer.modifiers.mirror.ways().max(1) as usize;
    let d = capped(c.detail, st.specks.len());
    let time = cx.time as f32;
    let mut blobs = Vec::new();
    for s in &st.specks {
        if !draws(c, s, now) {
            continue;
        }
        let it = Item::of(s, now);
        let Some(grow) = c.size_curve.at(it.life) else { continue };
        // The layer's opacity is the surface's; each blob counts as much as it shows.
        let weight = alpha_of(c, &it) / c.opacity;
        let size = s.size * c.size.max(0.0) * grow;
        if weight <= 0.0 || size <= 0.0 {
            continue;
        }
        let base = match (&ball_colour, it.ball) {
            (Some(of), b) if b > 0 => of(b as usize - 1),
            _ => own_colour(cx, c, &it),
        };
        blobs.push(l.blob(s.place, size, weight, base));
        for k in 0..d.copies {
            let by = copy_offset(&d, s, k, size, time);
            blobs.push(l.blob(s.place + by, size * d.size.max(0.0), weight * d.opacity.clamp(0.0, 1.0), base));
        }
        if blobs.len() >= most {
            blobs.truncate(most);
            break;
        }
    }
    let m = &layer.modifiers;
    if m.mirror.ways() > 1 {
        mirror_blobs(&mut blobs, m, mirror_centre(cx, st, m));
    }
    if m.glitches() {
        glitch_blobs(&mut blobs, &Tears::new(m, &view_of(cx), cx.time, layer.id.0));
    }
    (!blobs.is_empty()).then(|| Liquid { blobs, style: l.style(c.opacity), front: c.place == AroundPlace::InFront })
}
