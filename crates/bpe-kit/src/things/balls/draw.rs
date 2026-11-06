//! Drawing the balls and their hit flashes, through the layers of their Look.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bpe_core::{BallSpread, Blend, ResolvedThing, Settings, SlotId, ThingId};
use bpe_gfx::{Around, DrawCx, FlashInstance, Frame, HitMark, LayeredRun, ThingDraw, blend_alpha, thing_code};
use bpe_runtime::{Bodies, Event, EventKind};

use crate::modules::look::draw::{BallInputs, for_balls};
use crate::modules::look::layers::arcs::{self, ArcBall, ArcMoment, ArcsLayer};
use crate::modules::look::layers::around::{AroundKind, AroundLayer, around_layers};
use crate::modules::look::layers::aura::{self, AuraBall};
use crate::modules::look::layers::gpu::{Stack, SubLayer};
use crate::modules::look::layers::group::{BallInGroup, GroupShapeInfo, group_colour, mixed};
use crate::modules::look::layers::trail::{self, AroundPlace, TrailKind, draw::Balls};
use crate::modules::look::layers::vary::{VariedLook, spread_of};
use crate::modules::look::layers::wavefront::{self, Fronts, WavefrontLayer};
use crate::paint::{colour_at, vary_gpu};
use crate::things::balls::BallsSettings;
use crate::things::balls::rows::{GroupOf, RoundSpan, group_of, variety};
use crate::things::balls::sim::{colour_turn, look_turn};

pub struct BallsDraw;

/// The speed at which a flash is at full strength.
pub(crate) const FULL_FLASH_SPEED: f32 = 600.0;

/// How strong a hit at `speed` is, for flashes: 1 at [`FULL_FLASH_SPEED`], at most 1.5.
pub(crate) fn hit_strength(speed: f32) -> f32 {
    (speed / FULL_FLASH_SPEED).clamp(0.0, 1.5)
}

/// Each ball's last hit up to `now` seconds, no further back than `memory` seconds: how long ago
/// it was and how strong (`bpe_gfx::NO_HIT`'s age and 0 without one in that time), by row.
pub(crate) fn last_hits(cx: &DrawCx, count: usize, memory: f32) -> Vec<(f32, f32)> {
    last_hits_but(cx, count, memory, &|_, _| false)
}

/// [`last_hits`], leaving out the arrivals `skip` names (by the event and the row).
fn last_hits_but(cx: &DrawCx, count: usize, memory: f32, skip: &dyn Fn(&Event, usize) -> bool) -> Vec<(f32, f32)> {
    let mut last = vec![(bpe_gfx::NO_HIT[0], 0.0); count];
    if memory <= 0.0 {
        return last;
    }
    let id = cx.thing.id;
    let rate = cx.world.steps_per_second;
    let since = ((cx.time - memory as f64) * rate as f64).floor().max(0.0) as u64;
    for e in cx.log.since(since) {
        // A ball's arrival at a catcher counts as a hit for its reactions.
        let other = match e.kind {
            EventKind::Hit => e.b,
            EventKind::Enter => None,
            _ => continue,
        };
        let time = e.time(rate);
        if time > cx.time {
            break;
        }
        // Both balls of a ball-on-ball hit were hit.
        for side in std::iter::once(e.a).chain(other) {
            if side.thing == id
                && !(e.kind == EventKind::Enter && skip(e, side.item as usize))
                && let Some(l) = last.get_mut(side.item as usize)
            {
                *l = ((cx.time - time) as f32, hit_strength(e.speed));
            }
        }
    }
    last
}
/// How far back hits are listed for particles and shaders, in seconds.
pub const HIT_MEMORY: f64 = 3.0;

/// A ball's own number in 0 to 1, the same every frame and on every machine, for patterns and
/// effects that vary from ball to ball.
pub fn ball_seed(thing: ThingId, index: usize) -> f32 {
    let mut x = (index as u32).wrapping_mul(0x9e37_79b9) ^ thing.0.wrapping_mul(0x85eb_ca6b) ^ 0x27d4_eb2f;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x8a7d_9c3b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// What each ball of the thing being drawn wears: the Look card's own stack, or, when its
/// settings vary across the balls, the stack at the ball's place in their spreads (and the same
/// for its trails and particles).
pub(crate) struct Worn<'a> {
    look: VariedLook<'a>,
    /// The stacks the balls wear, the card's own first.
    stacks: Vec<Rc<Stack>>,
    /// Which of `stacks` each body row wears; empty when every ball wears the first.
    which: Vec<u32>,
    /// Each body row's place in the spreads, when its trails and particles vary.
    spreads: Vec<BallSpread>,
}

impl<'a> Worn<'a> {
    /// What the balls of the thing `cx` draws wear, when its Look card is a layered one.
    pub(crate) fn of(cx: &DrawCx<'a>) -> Option<Worn<'a>> {
        let look = VariedLook::of(cx.resolved.slots.get(&SlotId::Look)?)?;
        let rows = cx.world.bodies.get(&cx.thing.id).map_or(0, |b| b.len());
        let spreads: Vec<BallSpread> = if look.plain() && look.plain_around() {
            Vec::new()
        } else {
            (0..rows).map(|r| spread_of(cx, r)).collect()
        };
        let mut stacks = vec![look.base.clone()];
        let mut which = Vec::new();
        if !look.plain() {
            let mut index: HashMap<Vec<u16>, u32> = HashMap::new();
            which = spreads
                .iter()
                .map(|s| {
                    *index.entry(look.key(s)).or_insert_with_key(|key| {
                        stacks.push(look.stack_at(key));
                        stacks.len() as u32 - 1
                    })
                })
                .collect();
        }
        let spreads = if look.plain_around() { Vec::new() } else { spreads };
        Some(Worn { look, stacks, which, spreads })
    }

    /// The card's own stack, as every ball wears it when nothing varies.
    pub(crate) fn base(&self) -> &Stack {
        &self.stacks[0]
    }

    /// Which of the stacks body row `row` wears.
    fn index(&self, row: usize) -> usize {
        self.which.get(row).map_or(0, |&w| w as usize)
    }

    /// The stack body row `row` wears.
    fn stack(&self, row: usize) -> &Stack {
        &self.stacks[self.index(row)]
    }

    /// Body row `row`'s own trails and particles, when they vary across the balls.
    fn around(&self, row: usize) -> Option<Rc<Vec<AroundLayer>>> {
        let spread = self.spreads.get(row)?;
        Some(self.look.around_for(spread))
    }

    /// Whether the balls' trails and particles vary.
    fn around_varies(&self) -> bool {
        !self.spreads.is_empty()
    }
}

/// One stack's colour for each body row.
type ColourOf<'a> = Rc<dyn Fn(usize) -> [f32; 4] + 'a>;

/// Each ball's colour, as [`colours`] works it out for the stack it wears.
struct Colours<'w, 'a> {
    worn: &'w Worn<'a>,
    cx: &'w DrawCx<'a>,
    bodies: &'a Bodies,
    seen: bool,
    /// What every stack's colours share, made with the first.
    #[allow(clippy::type_complexity)]
    shared: RefCell<Option<(Rc<GroupCx<'a>>, Rc<BallInputs<'a>>)>>,
    /// Each stack's colours, made the first time a ball wearing it is asked about.
    made: RefCell<Vec<Option<ColourOf<'a>>>>,
}

impl<'w, 'a> Colours<'w, 'a> {
    fn new(worn: &'w Worn<'a>, cx: &'w DrawCx<'a>, bodies: &'a Bodies, seen: bool) -> Self {
        Colours {
            worn,
            cx,
            bodies,
            seen,
            shared: RefCell::new(None),
            made: RefCell::new(vec![None; worn.stacks.len()]),
        }
    }

    /// Body row `i`'s colour.
    fn get(&self, i: usize) -> [f32; 4] {
        let at = self.worn.index(i);
        let made = self.made.borrow()[at].clone();
        let f = made.unwrap_or_else(|| {
            let (group, inputs) = self
                .shared
                .borrow_mut()
                .get_or_insert_with(|| {
                    let fills: Vec<_> = self.worn.stacks.iter().map(|s| for_balls(s.body.fill.clone())).collect();
                    let inputs = BallInputs::new(self.cx, self.bodies, &fills.iter().collect::<Vec<_>>());
                    (Rc::new(group_cx(self.worn.base(), self.cx)), Rc::new(inputs))
                })
                .clone();
            let f: ColourOf<'a> =
                Rc::new(colours_with(&self.worn.stacks[at], self.cx, self.bodies, self.seen, group, inputs));
            self.made.borrow_mut()[at] = Some(f.clone());
            f
        });
        f(i)
    }
}

/// The linear-light colour of each ball of the thing being drawn: its Body's colours (painted by
/// place, start, order, speed, hits, steps or time), turned by its colour Variety, mixed with the
/// group gradient's colour, then turned by its colour reactions (Colour on hit and Colour over
/// time), when it has a layered look and bodies. Trails and particles take their colours from
/// here too, with the group's colour in even where the GPU mixes it into the ball.
pub fn ball_colors<'a>(cx: &DrawCx<'a>) -> Option<impl Fn(usize) -> [f32; 4] + 'a> {
    let worn = Worn::of(cx)?;
    let bodies = cx.world.bodies.get(&cx.thing.id)?;
    if worn.stacks.len() == 1 {
        let one = colours(worn.base(), cx, bodies, true);
        return Some(Box::new(one) as Box<dyn Fn(usize) -> [f32; 4] + 'a>);
    }
    let each: Vec<Box<dyn Fn(usize) -> [f32; 4] + 'a>> = worn
        .stacks
        .iter()
        .map(|s| Box::new(colours(s, cx, bodies, true)) as Box<dyn Fn(usize) -> [f32; 4] + 'a>)
        .collect();
    let which = worn.which.clone();
    Some(Box::new(move |i: usize| each[which.get(i).map_or(0, |&w| w as usize)](i)))
}

/// The group the thing's balls belong to, and what the gradient reads about it.
struct GroupCx<'a> {
    /// The rows and places of a thing with ball rows (a Balls thing, an emitter).
    of: Option<GroupOf<'a>>,
    info: GroupShapeInfo,
    /// The thing's settings as they are now, for the look-only variety (which doesn't rebuild
    /// the world, so the state's copy can be old).
    look: BallsSettings,
    /// The moment drawn, in steps, for fades.
    now: f64,
}

impl GroupCx<'_> {
    /// Body row `row`'s ball number.
    fn number(&self, row: usize) -> u32 {
        self.of.map_or(row as u32, |g| g.rows.number(row))
    }

    /// Whether its balls have a group's variety.
    fn varied(&self) -> bool {
        self.of.is_some()
    }

    /// How many balls the group has in all, let go or not.
    fn count(&self, bodies: &Bodies) -> usize {
        self.of.map_or(bodies.len(), |g| g.count)
    }

    /// How many hits body row `row` has had, its arrivals at catchers counted in.
    fn hits(&self, bodies: &Bodies, row: usize) -> u32 {
        let arrived = self.of.and_then(|g| g.rows.arrivals.get(row).copied()).unwrap_or(0);
        bodies.hits.get(row).copied().unwrap_or(0) + arrived
    }

    /// How much of body row `row` shows: 1 unless its round is fading out.
    fn fade(&self, row: usize) -> f32 {
        self.of.map_or(1.0, |g| g.fade(row, self.now))
    }

    /// What the group gradient reads about body row `row`.
    fn ball(&self, bodies: &Bodies, row: usize, time: f32) -> BallInGroup {
        let rows = self.of.map(|g| g.rows);
        BallInGroup {
            number: self.number(row),
            count: self.count(bodies) as u32,
            offset: rows.and_then(|r| r.place.get(row).copied()).unwrap_or_default(),
            speed: bodies.velocity.get(row).map_or(0.0, |v| v.length()),
            hits: self.hits(bodies, row),
            time,
            round: rows.and_then(|r| r.round.get(row).copied()).unwrap_or(0),
            rounds: self.of.map_or(RoundSpan::Finite(1), |g| g.rounds),
            launch: rows.and_then(|r| r.launch.get(row).copied()).unwrap_or(0.0),
        }
    }
}

fn group_cx<'a>(stack: &Stack, cx: &DrawCx<'a>) -> GroupCx<'a> {
    let of = group_of(cx.world, cx.thing.id);
    let (places, seed) = of.map_or((&[][..], 1), |g| (g.places, g.seed));
    let angle = stack.group.as_ref().map_or(0.0, |g| g.angle);
    let look = variety(&cx.thing.kind, &cx.resolved.params);
    let now = cx.time * cx.world.steps_per_second as f64;
    GroupCx { of, info: GroupShapeInfo::of(places, angle, seed), look, now }
}

/// [`ball_colors`] for `stack` and `bodies`; with `seen` false, the body colour the instance
/// carries (a group colour the GPU mixes in stays out).
fn colours<'a>(stack: &Stack, cx: &DrawCx<'a>, bodies: &'a Bodies, seen: bool) -> impl Fn(usize) -> [f32; 4] + use<'a> {
    let fill = for_balls(stack.body.fill.clone());
    let inputs = Rc::new(BallInputs::new(cx, bodies, &[&fill]));
    colours_with(stack, cx, bodies, seen, Rc::new(group_cx(stack, cx)), inputs)
}

/// [`colours`] with the group and the balls' paint inputs worked out already (shared by every
/// stack a varied look's balls wear: the group layers never vary, and `inputs` must have been
/// made for this stack's body paint among others).
fn colours_with<'a>(
    stack: &Stack,
    cx: &DrawCx<'a>,
    bodies: &'a Bodies,
    seen: bool,
    group: Rc<GroupCx<'a>>,
    inputs: Rc<BallInputs<'a>>,
) -> impl Fn(usize) -> [f32; 4] + use<'a> {
    let fill = for_balls(stack.body.fill.clone());
    let (hue_per_hit, turn) = (stack.hue_per_hit(), stack.rainbow() * cx.time as f32);
    let gradient = if seen { stack.group.clone() } else { stack.group_on_body().cloned() };
    let time = cx.time as f32;
    move |i| {
        // A fading ball's trails and particles fade with it (the ball itself by its opacity).
        let faded = |mut c: [f32; 4]| {
            if seen {
                c[3] *= group.fade(i);
            }
            c
        };
        let mut at = inputs.get(&fill, i);
        // Order counts the balls by their place in the group, let go yet or not.
        at.index = group.number(i) as usize;
        at.count = group.count(bodies);
        let mut base = colour_at(&fill, &at);
        if group.varied() {
            let spin = colour_turn(&group.look, group.number(i));
            if spin != 0.0 {
                base = crate::modules::look::basic::shift_linear_hue(base, spin);
            }
        }
        if let Some(g) = &gradient {
            base = mixed(base, group_colour(g, &group.info, &group.ball(bodies, i, time)), g.mix, g.amount);
        }
        faded(crate::modules::look::layers::gpu::recoloured(base, hue_per_hit, turn, group.hits(bodies, i)))
    }
}

impl ThingDraw for BallsDraw {
    fn kind(&self) -> &'static str {
        "balls"
    }

    fn blend(&self, resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        Stack::of(resolved.slots.get(&SlotId::Look)?).map(|s| s.whole_blend())
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(worn) = Worn::of(cx) else { return };
        let part = worn.base().split().unwrap_or(0);
        draw_part(cx, frame, &worn, part);
    }

    /// The layers above the Blend layer, painted normally over the blended rest.
    fn draw_over(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(worn) = Worn::of(cx) else { return };
        if let Some(cut) = worn.base().split() {
            draw_part(cx, frame, &worn, cut | OVER);
        }
    }

    /// The Look's trails and particles, but Ghosts (which the balls draw with themselves).
    fn around(&self, resolved: &ResolvedThing) -> Vec<Around> {
        own_around(resolved)
            .iter()
            .map(|a| {
                let (blend, amount) = a.blend();
                // Mini balls are balls: they draw as the balls do.
                let (blend, amount) = if a.is_mini_balls() { (Blend::Normal, 1.0) } else { (blend, amount) };
                // They have no Where either: they draw over the balls at every z-order.
                let behind = !a.is_mini_balls() && a.place() == AroundPlace::Behind;
                Around { blend, amount, behind }
            })
            .collect()
    }

    fn draw_around(&self, cx: &DrawCx, index: usize, frame: &mut Frame) {
        let Some(worn) = Worn::of(cx) else { return };
        let Some(layer) = own_around(cx.resolved).into_iter().nth(index) else { return };
        let memory = around_memory(cx, &worn, &layer);
        let drew = crate::modules::look::layers::around::Drew::from(frame);
        match &layer.kind {
            AroundKind::Trail(trail) => {
                with_balls(cx, &worn, memory, |balls| trail::draw::draw(cx, &layer, trail, balls, frame));
            }
            AroundKind::Wavefront(w) => draw_wavefront(cx, &worn, &layer, w, frame),
            AroundKind::Arcs(a) => {
                let balls = arc_balls(cx, &worn, &layer, a);
                arcs::draw(&layer, a, &balls, arc_moment(cx), frame);
            }
            AroundKind::Aura(a) => {
                let balls: Vec<AuraBall> = arc_balls(cx, &worn, &layer, &ArcsLayer::default())
                    .into_iter()
                    .map(|b| AuraBall { at: b.at, radius: b.radius, colour: b.colour })
                    .collect();
                aura::draw(&layer, a, &balls, cx.time as f32, frame);
            }
            AroundKind::Particles(particles) => {
                with_balls(cx, &worn, memory, |balls| {
                    crate::modules::look::layers::particles::draw::draw_balls(
                        cx,
                        &layer,
                        particles,
                        balls,
                        worn.base(),
                        frame,
                    )
                });
            }
        }
        drew.light(frame, &layer, cx);
    }

    /// The balls' surface and, for Stage, each ball as a sphere; then electric arcs that light
    /// the scene.
    fn light(&self, cx: &DrawCx, scene: &mut bpe_gfx::LightScene) {
        push_ball_light(cx, scene);
        push_stirs(cx, scene);
        let lighting: Vec<AroundLayer> = own_around(cx.resolved)
            .into_iter()
            .filter(|l| matches!(&l.kind, AroundKind::Arcs(a) if a.lights != 0.0))
            .collect();
        if lighting.is_empty() {
            return;
        }
        let Some(worn) = Worn::of(cx) else { return };
        for layer in &lighting {
            let AroundKind::Arcs(a) = &layer.kind else { continue };
            let balls = arc_balls(cx, &worn, layer, a);
            arcs::light(a, &balls, arc_moment(cx), scene);
        }
    }
}

/// Adds every live ball of the thing `cx` is drawing to the balls that stir a moving haze (lighting
/// part 4: Balls stir it), when the scene's haze moves and they stir it.
pub fn push_stirs(cx: &DrawCx, scene: &mut bpe_gfx::LightScene) {
    let s = &scene.settings;
    if !s.haze_moves() || s.haze_more[2] == 0.0 {
        return;
    }
    let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
    for r in (0..bodies.len()).filter(|&r| bodies.alive[r]) {
        let (p, v) = (bodies.position[r], bodies.velocity[r]);
        scene.stirs.push([[p.x, p.y, v.x, v.y], [bodies.radius[r], 0.0, 0.0, 0.0]]);
    }
}

/// The live balls that wear around layer `layer`, as electric arcs `a` see them (their last hits
/// read only when the arcs fire on hits).
fn arc_balls(cx: &DrawCx, worn: &Worn, layer: &AroundLayer, a: &ArcsLayer) -> Vec<ArcBall> {
    let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return Vec::new() };
    let colours = Colours::new(worn, cx, bodies, true);
    let group = group_cx(worn.base(), cx);
    let hits = if a.fire == arcs::ArcFire::WhenTheyHit {
        last_hits(cx, bodies.len(), a.fire_length.max(1e-3))
    } else {
        Vec::new()
    };
    (0..bodies.len())
        .filter(|&i| bodies.alive[i] && layer.applies(group.number(i)))
        .map(|i| {
            let fade = group.fade(i);
            let mut colour = colours.get(i);
            for c in &mut colour[..3] {
                *c *= fade;
            }
            ArcBall {
                at: bodies.position[i],
                radius: bodies.radius[i],
                number: group.number(i),
                colour,
                hit_age: hits.get(i).map_or(f32::MAX, |h| h.0),
            }
        })
        .collect()
}

/// The moment the arcs are drawn at: its time and the scene's beat.
fn arc_moment(cx: &DrawCx) -> ArcMoment {
    let beat = cx.scene.things.get(&ThingId::ROOT).map(|r| {
        let s = crate::things::scene::SceneSettings::from_params(&r.params);
        crate::things::scene::draw::beat_at(&s, cx.time)
    });
    ArcMoment { time: cx.time as f32, beat: beat.filter(|b| b[3] > 0.0 && b[2] > 0.0).map(|b| (b[0], 1.0 / b[2])) }
}

/// Adds the balls of the thing `cx` is drawing to a lit frame: its Look's surface at its surface
/// index, and every ball alive as a sphere (centre, radius, see-through and its Body's colour,
/// for the light it lets through).
pub fn push_ball_light(cx: &DrawCx, scene: &mut bpe_gfx::LightScene) {
    use crate::modules::look::surface;
    let Some(look) = cx.resolved.slots.get(&SlotId::Look).filter(|l| surface::has_surface(&l.kind)) else { return };
    let s = surface::of_card(look);
    let index = surface::surface_index(cx.scene, cx.thing.id);
    scene.set_surface(index, s.gpu(bpe_gfx::SurfaceGpu::BALL));
    if scene.settings.stage[0] < 0.5 {
        return;
    }
    let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
    let tint = match Stack::of(look) {
        // A Body that doesn't block light casts no shadow in Stage either.
        Some(stack) if stack.body_light[1] < 0.5 => return,
        Some(stack) => {
            let inputs = crate::paint::Inputs { time: cx.time as f32, ..Default::default() };
            crate::paint::colour_at(&stack.body.fill, &inputs)
        }
        None => [1.0; 4],
    };
    let see = s.see_through.clamp(0.0, 1.0);
    for i in 0..bodies.len() {
        if !bodies.alive[i] {
            continue;
        }
        let at = bodies.position[i];
        scene
            .spheres
            .push(bpe_gfx::SphereGpu { a: [at.x, at.y, bodies.radius[i], see], b: [tint[0], tint[1], tint[2], 0.0] });
    }
}

/// The thing's shown trails and particles, bottom first.
fn around_of(resolved: &ResolvedThing) -> Vec<AroundLayer> {
    match resolved.slots.get(&SlotId::Look) {
        Some(look) if look.kind == crate::modules::look::layers::CARD => around_layers(look, false),
        _ => Vec::new(),
    }
}

/// [`around_of`] but Ghosts, which draw with the balls and blend as they do.
fn own_around(resolved: &ResolvedThing) -> Vec<AroundLayer> {
    around_of(resolved).into_iter().filter(|a| !a.is_ghosts()).collect()
}

/// How far back `layer` reads the balls' hits: as far as its trail reaches when it restarts at
/// each bounce, the flash memory for a Flash on hit sub-layer, else not at all.
fn hit_memory(cx: &DrawCx, layer: &AroundLayer) -> f32 {
    let mut memory = 0.0f32;
    if let AroundKind::Trail(t) = &layer.kind
        && t.restart
    {
        memory = if t.stays() { cx.time as f32 } else { t.reach() };
    }
    for sub in &layer.subs {
        if let SubLayer::Flash(f) = sub {
            memory = memory.max(f.length * 4.0);
        }
    }
    memory
}

/// Draws the wavefront `w` (the around layer `layer`) through the balls of the thing `cx` draws.
fn draw_wavefront(cx: &DrawCx, worn: &Worn, layer: &AroundLayer, w: &WavefrontLayer, frame: &mut Frame) {
    let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
    let colours = Colours::new(worn, cx, bodies, true);
    let colour = |i: usize| colours.get(i);
    let group = group_cx(worn.base(), cx);
    let rows = group.of.map(|g| g.rows);
    let shown = |i: usize| bodies.alive[i] && layer.applies(group.number(i));
    let round = |i: usize| rows.and_then(|r| r.round.get(i).copied()).unwrap_or(0);
    let order = |i: usize| rows.and_then(|r| r.in_round.get(i).copied()).unwrap_or(i as u32);
    let fade = |i: usize| group.fade(i);
    let all_round = if group.of.is_some_and(|g| g.emitter) {
        crate::things::emitter::EmitterSettings::from_params(&cx.resolved.params).directions
            == crate::things::emitter::Directions::AllRound
    } else {
        group.look.group && group.look.shape == crate::things::balls::GroupShape::Ring
    };
    let fronts = Fronts {
        position: &bodies.position,
        shown: &shown,
        round: &round,
        order: &order,
        colour: &colour,
        fade: &fade,
        all_round,
        radius: group.look.radius,
    };
    wavefront::draw(layer, w, &fronts, frame);
}

/// How far back around layer `layer` reads the balls' hits, at most: as [`hit_memory`] says for
/// the layer as the card has it, or for any ball's own copy of it.
fn around_memory(cx: &DrawCx, worn: &Worn, layer: &AroundLayer) -> f32 {
    let mut memory = hit_memory(cx, layer);
    if worn.around_varies() {
        let mut seen: Vec<*const Vec<AroundLayer>> = Vec::new();
        for row in 0..worn.spreads.len() {
            let Some(own) = worn.around(row) else { continue };
            if seen.contains(&Rc::as_ptr(&own)) {
                continue;
            }
            seen.push(Rc::as_ptr(&own));
            if let Some(mine) = own.iter().find(|a| a.id == layer.id) {
                memory = memory.max(hit_memory(cx, mine));
            }
        }
    }
    memory
}

/// Runs `f` with what trails read about the balls of the thing `cx` draws, wearing `worn`,
/// their hits read `memory` seconds back; nothing without bodies or a history.
fn with_balls<R>(cx: &DrawCx, worn: &Worn, memory: f32, f: impl FnOnce(&Balls) -> R) -> Option<R> {
    let id = cx.thing.id;
    let bodies = cx.world.bodies.get(&id)?;
    let history = cx.world.history.get(&id)?;
    let dt = cx.world.dt();
    let newest = history.newest_step().map_or(0.0, |s| s as f64 * dt as f64);
    let colours = Colours::new(worn, cx, bodies, true);
    let colour = |i: usize| colours.get(i);
    let group = group_cx(worn.base(), cx);
    let number = |i: usize| group.number(i);
    let time = cx.time as f32;
    let own = |i: usize| worn.around(i).unwrap_or_default();
    let turned = |i: usize, ago: f32| {
        worn.stack(i).turning.as_ref().map(|t| {
            let own = if group.varied() { look_turn(&group.look, group.number(i)) } else { 0.0 };
            (t.turn + t.spin * (time - ago) + own).to_radians()
        })
    };
    let leaving = leaves_trails(cx);
    let rows = group_of(cx.world, id).map(|g| g.rows);
    let rate = cx.world.steps_per_second as f64;
    // The step ball `i` was caught at, when the catcher leaves its trail behind.
    let left = |i: usize| {
        if leaving.is_empty() {
            return None;
        }
        let c = rows?.caught.get(i).copied().flatten()?;
        leaving.contains(&c.by).then_some(c.step)
    };
    let gone = |i: usize| left(i).map(|step| (cx.time - step as f64 / rate).max(0.0) as f32);
    // The catch isn't a hit for a trail left behind: one that restarts at each bounce keeps
    // what it had.
    let last = last_hits_but(cx, bodies.len(), memory, &|e, i| left(i).is_some_and(|step| e.step >= step));
    let balls = Balls {
        bodies,
        history,
        interval: history.interval(dt),
        newest_ago: (cx.time - newest).max(0.0) as f32,
        colour: &colour,
        number: &number,
        turned: &turned,
        last_hits: &last,
        own: worn.around_varies().then_some(&own as &dyn Fn(usize) -> Rc<Vec<AroundLayer>>),
        gone: &gone,
    };
    Some(f(&balls))
}

/// The emitters whose caught balls leave their trails behind (Leave trails), as they are now.
fn leaves_trails(cx: &DrawCx) -> Vec<ThingId> {
    cx.scene
        .things
        .iter()
        .filter(|(_, r)| r.kind == crate::things::emitter::KIND)
        .filter(|(_, r)| matches!(r.params.get("leave_trails"), Some(bpe_core::Value::Bool(true))))
        .map(|(id, _)| *id)
        .collect()
}

/// Draws the balls `made` (by row) with the Ghosts their Look's Ghosts trails leave: copies of
/// each ball (of the same `part`) back along where it has been, those behind before every ball,
/// those in front after, with what their sub-layers add round them.
fn draw_ghosts(
    cx: &DrawCx,
    frame: &mut Frame,
    worn: &Worn,
    part: u32,
    made: &[(usize, bpe_gfx::LayeredInstance)],
    seen: &dyn Fn(usize) -> [f32; 4],
) {
    let ghosts: Vec<AroundLayer> = around_of(cx.resolved).into_iter().filter(|a| a.is_ghosts()).collect();
    let (mut behind, mut front) = (Vec::new(), Vec::new());
    let (mut under, mut over_behind, mut over_front) = (Vec::new(), Vec::new(), Vec::new());
    if !ghosts.is_empty() {
        let memory = ghosts.iter().map(|g| around_memory(cx, worn, g)).fold(0.0, f32::max);
        with_balls(cx, worn, memory, |balls| {
            for base in &ghosts {
                for (i, ball) in made {
                    // The ball's own copy of the layer, when its trails vary.
                    let own = balls.own_layers(*i);
                    let layer = own.as_ref().and_then(|o| o.iter().find(|a| a.id == base.id)).unwrap_or(base);
                    let AroundKind::Trail(t) = &layer.kind else { continue };
                    debug_assert_eq!(t.kind, TrailKind::Ghosts);
                    let in_front = t.place == AroundPlace::InFront;
                    for g in trail::draw::ghosts_of(layer, t, balls, *i) {
                        let mut copy = *ball;
                        copy.center = g.at.into();
                        copy.misc[0] *= g.opacity;
                        if in_front { &mut front } else { &mut behind }.push(copy);
                        if part & OVER == 0 {
                            let over = if in_front { &mut over_front } else { &mut over_behind };
                            trail::draw::ghost_extras(layer, g, ball.radius, seen(*i), &mut under, over);
                        }
                    }
                }
            }
        });
    }
    // Ghosts take the light as their trail layer does, flat: each ghost reads a copy of its
    // ball's records with that layer's Lighting.
    if frame.light.is_some() {
        let light = ghosts.first().map_or([0.0; 4], |g| [g.light[0], g.light[1], 0.0, 0.0]);
        let emit = ghosts.first().and_then(|g| g.emit);
        // And its UV, beams and link, not the ball's.
        let more = ghosts.first().and_then(|g| g.more_in(cx, frame.light.as_mut())).unwrap_or([0.0; 4]);
        let mut copies: Vec<(u32, u32)> = Vec::new();
        for g in behind.iter_mut().chain(front.iter_mut()) {
            let first = g.range[0];
            let at = match copies.iter().find(|(from, _)| *from == first) {
                Some((_, to)) => *to,
                None => {
                    let to = frame.look_layers.len() as u32;
                    let range = first as usize..(first + g.range[1]) as usize;
                    let mut records = frame.look_layers.get(range).map(<[_]>::to_vec).unwrap_or_default();
                    for r in &mut records {
                        r.light = light;
                        r.emit = crate::modules::look::layers::gives_light::gpu_steady(emit);
                        r.more = more;
                    }
                    frame.look_layers.extend(records);
                    copies.push((first, to));
                    to
                }
            };
            g.range[0] = at;
        }
        let stamp = |s: &mut bpe_gfx::Particle| {
            s.light = light;
            s.extra = more;
            if let Some(e) = emit {
                s.emit = e.at(s.color);
            }
        };
        under.iter_mut().chain(over_behind.iter_mut()).chain(over_front.iter_mut()).for_each(stamp);
    }
    frame.layered.extend(behind);
    frame.layered.extend(made.iter().map(|(_, b)| *b));
    frame.layered.extend(front);
    frame.particles.extend(under);
    frame.particles.extend(over_behind);
    frame.particles_front.extend(over_front);
}

/// A ball's seconds since its last hit when it hasn't hit anything lately.
const NEVER_HIT: f32 = 1e6;

/// The bit of a ball instance's `range.z` that draws the layers above its Blend layer.
const OVER: u32 = 0x1_0000;

/// Draws the balls of the thing `cx` is drawing, each wearing its stack of `worn`: whole (`part`
/// 0), the layers under the Blend layer with the flashes (`part` the first record above it), or
/// the layers above it (that with [`OVER`]): see layers_walk.wgsl. The layers' kinds and order
/// are the same for every ball, so the card's own stack says how the balls split and which
/// program draws them; only the settings in their records differ.
fn draw_part(cx: &DrawCx, frame: &mut Frame, worn: &Worn, part: u32) {
    let id = cx.thing.id;
    let Some(bodies) = cx.world.bodies.get(&id) else { return };
    let stack = worn.base();
    let color = Colours::new(worn, cx, bodies, false);
    let seen_by = Colours::new(worn, cx, bodies, true);
    let seen = |i: usize| seen_by.get(i);
    let time = cx.time as f32;
    let group = group_cx(stack, cx);
    let on_gpu = stack.group_on_gpu();
    // Each stack's records, written the first time a ball wearing it is drawn.
    let mut records: Vec<Option<Vec<bpe_gfx::LayerGpu>>> = vec![None; worn.stacks.len()];
    let link = crate::things::light::links::number_of(cx.scene, id);
    // Balls given layers of their own wear their own range of records: one range for each
    // stack and set of layers worn, most scenes one range for every ball.
    let mut ranges: HashMap<(usize, Vec<bool>), (u32, u32)> = HashMap::new();
    // Every record names the thing's surface (read only by a lit frame's light pass).
    let surface = crate::modules::look::surface::surface_index(cx.scene, id) as f32;
    let mut range_for = |frame: &mut Frame, at: usize, number: u32| -> (u32, u32) {
        let own = &worn.stacks[at];
        let wears: Vec<bool> =
            if own.has_only() { (0..own.drawn.len()).map(|k| own.applies(k, number)).collect() } else { Vec::new() };
        *ranges.entry((at, wears)).or_insert_with_key(|(_, wears)| {
            let all = records[at].get_or_insert_with(|| {
                let mut made = own.records(&mut frame.gradients, &mut frame.mask_tiles, time);
                crate::modules::look::layers::lighting::stamp(&mut made, link, frame.light.as_mut());
                made
            });
            let first = frame.look_layers.len() as u32;
            let picked = if wears.is_empty() { all.clone() } else { own.records_for(all, |k| wears[k]) };
            let count = picked.len() as u32;
            frame.look_layers.extend(picked.into_iter().map(|mut r| {
                r.light[3] = surface;
                r
            }));
            (first, count)
        })
    };
    // Each ball's own variation, only read when a pattern paints from its own gradient.
    let fills: Vec<_> = worn.stacks.iter().map(|s| for_balls(s.body.fill.clone())).collect();
    let vary =
        worn.stacks.iter().any(|s| s.varies()).then(|| BallInputs::new(cx, bodies, &fills.iter().collect::<Vec<_>>()));
    // Group shaders spread across the square round the group's places, a ball wide past them.
    let shaded = stack.group_shaders().next().is_some();
    // The biggest a ball of the group can be, whether let go yet or not.
    let widest =
        group.look.radius * if group.look.group { 1.0 + 0.5 * group.look.vary_size.clamp(0.0, 1.0) } else { 1.0 };
    // A Size varying across the balls can reach past it.
    let widest = match crate::things::balls::sim::size_to(&cx.resolved.params) {
        Some(to) => {
            widest.max(to * widest / group.look.radius.max(1e-3)).max(bodies.radius.iter().copied().fold(0.0, f32::max))
        }
        None => widest,
    };
    let side = 2.0 * (group.info.furthest + widest).max(1.0);
    // Flash and Colour on hit sub-layers read each ball's last hit and its hits so far.
    let hits = worn.stacks.iter().any(|s| s.needs_hits());
    let memory = worn.stacks.iter().map(|s| s.flash_memory()).fold(0.0, f32::max);
    let last = if hits { last_hits(cx, bodies.len(), memory) } else { Vec::new() };
    // Heat glow: each ball's heat from its hits (one heat a ball, by its first stack's heat).
    let heats = worn
        .stacks
        .iter()
        .find_map(|s| s.heat())
        .map(|h| crate::modules::look::layers::heat::ball_heats(cx, bodies.len(), h))
        .unwrap_or_default();
    let start = frame.layered.len() as u32;
    let mut made: Vec<(usize, bpe_gfx::LayeredInstance)> = Vec::with_capacity(bodies.len());
    for i in 0..bodies.len() {
        if !bodies.alive[i] {
            continue;
        }
        let seed = ball_seed(id, i);
        let code = thing_code(id) + seed * 0.5;
        let at = worn.index(i);
        let mine = &worn.stacks[at];
        let own = vary.as_ref().filter(|_| mine.varies()).map_or(bpe_gfx::NO_VARY, |v| {
            let fill = &fills[at];
            let paint = v.get(fill, i);
            vary_gpu(fill, paint.seed, paint.time)
        });
        let (center, velocity, radius) = (bodies.position[i], bodies.velocity[i], bodies.radius[i]);
        let body = blend_alpha(color.get(i));
        let number = group.number(i);
        let (first, count) = range_for(frame, at, number);
        let mut ball = mine.instance(body, center, velocity, radius, time, code, seed, own, first, count);
        ball.range[2] = part;
        if hits {
            let (age, strength) = last.get(i).copied().unwrap_or((bpe_gfx::NO_HIT[0], 0.0));
            ball.hit = [age, strength, group.hits(bodies, i) as f32, heats.get(i).copied().unwrap_or(0.0)];
        }
        if mine.body_each_ball() {
            // A gradient body turns by the ball's own hue turns, as a plain body's colour does.
            let hits = group.hits(bodies, i) as f32;
            let spin = if group.varied() { colour_turn(&group.look, number) } else { 0.0 };
            // The Variety turn is in turns already; the reaction's is degrees a hit.
            ball.vary[3] = spin + mine.hue_per_hit() * hits / 360.0;
        }
        if let Some(g) = on_gpu {
            let c = group_colour(g, &group.info, &group.ball(bodies, i, time));
            ball.group = [c[0], c[1], c[2], g.amount.clamp(0.0, 1.0)];
            ball.range[3] = g.mix.code();
        }
        if shaded {
            let at = group.ball(bodies, i, time).offset - group.info.middle;
            ball.place = [at.x, at.y, side, 0.0];
        }
        if group.varied() {
            ball.form[2] += look_turn(&group.look, number).to_radians();
        }
        let fade = group.fade(i);
        if fade < 1.0 {
            ball.misc[0] *= fade;
        }
        made.push((i, ball));
    }
    draw_ghosts(cx, frame, worn, part, &made, &seen);
    // The scene's effects see every ball drawn (once, with the whole or lower part).
    let effects = crate::modules::effects::draw::scene_has_effects(cx.scene);
    let mut mark_of = Vec::new();
    if effects && part & OVER == 0 {
        mark_of = vec![usize::MAX; bodies.len()];
        for (i, _) in &made {
            let c = seen(*i);
            mark_of[*i] = frame.ball_marks.len();
            frame.ball_marks.push(bpe_gfx::BallMark {
                center: bodies.position[*i].into(),
                radius: bodies.radius[*i],
                color: [c[0], c[1], c[2]],
                seed: ball_seed(id, *i),
                hit_age: NEVER_HIT,
            });
        }
    }
    // Which pipeline draws them: the stack's in-ball program, once per frame, or the plain one.
    let program = stack.program().map(|p| match frame.programs.iter().position(|q| q.key == p.key) {
        Some(i) => i as u32,
        None => {
            frame.programs.push(p);
            frame.programs.len() as u32 - 1
        }
    });
    let end = frame.layered.len() as u32;
    frame.layered_runs.push(LayeredRun { program, start, end });
    if part & OVER != 0 {
        return;
    }

    // Recent hits, for the flashes here and for the particles and the scene's effects. Only the
    // flashes' worth is scanned when nothing else wants them.
    let rate = cx.world.steps_per_second;
    let flashing = |s: &Stack| s.flash.as_ref().is_some_and(|f| f.flash > 0.0 && f.flash_seconds > 0.0);
    let flash = worn
        .stacks
        .iter()
        .any(|s| flashing(s))
        .then(|| worn.stacks.iter().filter_map(|s| s.flash.as_ref()).map(|f| f.flash_seconds).fold(0.0, f32::max));
    // The scene settings' effects aren't passed down but still pulse on every hit.
    let wants_hits = effects;
    let memory = if wants_hits {
        HIT_MEMORY
    } else if let Some(seconds) = flash {
        seconds as f64
    } else {
        return;
    };
    let since = ((cx.time - memory) * rate as f64).floor().max(0.0) as u64;
    for e in cx.log.since(since) {
        let hit = match e.kind {
            EventKind::Hit => true,
            // Arrivals at a catcher flash as hits do, but aren't hits for the scene's effects.
            EventKind::Enter => false,
            _ => continue,
        };
        let time = e.time(rate);
        if time > cx.time {
            continue;
        }
        // The effects' ball list: both balls of a ball-on-ball hit were hit.
        if hit && !mark_of.is_empty() {
            for side in std::iter::once(e.a).chain(e.b).filter(|s| s.thing == id) {
                if let Some(&m) = mark_of.get(side.item as usize).filter(|m| **m != usize::MAX) {
                    let age = (cx.time - time) as f32;
                    frame.ball_marks[m].hit_age = frame.ball_marks[m].hit_age.min(age);
                }
            }
        }
        if e.a.thing != id {
            continue;
        }
        if hit {
            frame.hits.push(HitMark {
                thing: id,
                other: e.b.map(|b| b.thing),
                point: e.point.into(),
                normal: e.normal.into(),
                time: time as f32,
                speed: e.speed,
                color: seen(e.a.item as usize),
            });
        }
        if flash.is_none() {
            continue;
        }
        let Some(f) = worn.stack(e.a.item as usize).flash.as_ref().filter(|f| f.flash > 0.0 && f.flash_seconds > 0.0)
        else {
            continue;
        };
        let age = (cx.time - time) as f32 / f.flash_seconds;
        if !(0.0..=1.0).contains(&age) {
            continue;
        }
        let intensity = hit_strength(e.speed) * (-3.0 * age).exp() * 0.35 * f.flash / 0.4;
        if intensity < 0.002 {
            continue;
        }
        let c = seen(e.a.item as usize);
        frame.flashes.push(FlashInstance {
            center: e.point.into(),
            size: f.flash_size,
            color: [
                (c[0] + 1.0) * 0.5 * intensity,
                (c[1] + 1.0) * 0.5 * intensity,
                (c[2] + 1.0) * 0.5 * intensity,
                1.0,
            ],
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::modules::look::layers::{BODY, HUE, PATTERN};
    use crate::testing::set_layer;
    use crate::{kit, new_scene, registry};
    use bpe_core::{Scene, Slot, SlotId, ThingId, Value};
    use bpe_gfx::{Frame, LayerGpu, build_frame};
    use bpe_runtime::{Log, World};

    const BALL: ThingId = ThingId(2);

    #[test]
    fn a_look_of_an_unknown_kind_is_reported_and_leaves_the_balls_undrawn() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.things.get_mut(&BALL).unwrap().slots.insert(SlotId::Look, Slot::new("look.hologram"));
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(!resolved.problems.is_empty());
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.layered.is_empty(), "no look, nothing to draw them with");
        assert_eq!(frame.look_layers.len(), 1, "only the shape's Body record");
        assert_eq!(frame.paths.len(), 1, "the shape still draws");
        assert_eq!((frame.width, frame.height), (1080, 1920));
    }

    /// The records of the frame's first layered ball (the shape's come before them).
    fn ball_records(frame: &Frame) -> &[LayerGpu] {
        let r = frame.layered[0].range;
        &frame.look_layers[r[0] as usize..(r[0] + r[1]) as usize]
    }

    /// The frame at `time` for the new scene's ball after `f`'s changes, played to `time`.
    fn frame_after(time: f64, f: impl FnOnce(&mut Scene)) -> Frame {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        f(&mut scene);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(time);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
        frame
    }

    #[test]
    fn the_classic_ball_draws_its_body_shine_and_glow_through_one_layered_instance() {
        let frame = frame_after(0.0, |_| {});
        assert!(frame.balls.is_empty() && frame.looks.is_empty(), "the old paths draw no ball");
        assert_eq!(frame.layered.len(), 1);
        let codes: Vec<u32> = ball_records(&frame).iter().map(|l| l.head[0]).collect();
        assert_eq!(codes, [LayerGpu::BODY, LayerGpu::SHINE, LayerGpu::GLOW]);
        assert_eq!(frame.layered[0].range[1], 3);
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(frame.layered[0].body, pink);
    }

    #[test]
    fn runs_cover_every_ball_in_order_and_name_one_program_per_distinct_stack() {
        let reg = registry();
        let frame = frame_after(0.0, |scene| {
            let look = crate::modules::look::layers::SHADER;
            let other = crate::add_thing(scene, &reg, "balls", ThingId::ROOT).unwrap();
            let third = crate::add_thing(scene, &reg, "balls", ThingId::ROOT).unwrap();
            scene.add_layer(&reg, other, SlotId::Look, look).unwrap();
            scene.add_layer(&reg, third, SlotId::Look, look).unwrap();
        });
        assert_eq!(frame.layered.len(), 3);
        let mut at = 0;
        for run in &frame.layered_runs {
            assert_eq!(run.start, at, "runs follow each other");
            at = run.end;
        }
        assert_eq!(at, 3, "every ball is in a run");
        let programs: Vec<Option<u32>> = frame.layered_runs.iter().map(|r| r.program).collect();
        assert_eq!(programs, [None, Some(0), Some(0)], "two lava stacks share one program");
        assert_eq!(frame.programs.len(), 1);
    }

    #[test]
    fn a_new_ball_is_one_white_body_record_and_does_not_flash() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.remove_thing(BALL).unwrap();
        let ball = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&ball).unwrap();
        thing.params.set("velocity", Value::Vec2([0.0, 900.0]));
        // The scene's effects list the hits.
        crate::testing::add_effect(&mut scene, &reg, "picture.kaleidoscope");
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered.len(), 1);
        assert_eq!(frame.layered[0].range[1], 1, "the body alone");
        assert_eq!(ball_records(&frame)[0].head[0], LayerGpu::BODY);
        assert_eq!(frame.layered[0].body, [1.0; 4], "white");
        assert!(!frame.hits.is_empty(), "the ball hit the arena");
        assert!(frame.flashes.is_empty(), "no Flash on hit, no flash");
    }

    #[test]
    fn a_ball_in_another_shape_still_flashes_and_carries_its_thing_code() {
        let frame = frame_after(3.0, |s| {
            set_layer(s, BALL, BODY, "ball_shape", Value::Choice("star".into()));
        });
        assert_eq!(frame.layered.len(), 1);
        let ball = frame.layered[0];
        assert_eq!(ball.form[0], 5.0);
        assert_eq!(ball.misc[1], bpe_gfx::thing_code(BALL) + ball.seed * 0.5);
        assert!(!frame.flashes.is_empty() || frame.hits.is_empty());
    }

    #[test]
    fn flashes_come_from_the_flash_on_hit_reaction_only() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(8.0);
        let rate = playback.world().steps_per_second;
        let first = playback.log().events().first().expect("the ball hits the arena").time(rate);
        // Just after the first hit, while it still flashes.
        let at = first + 0.05;
        playback.seek(at);
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > at - 0.25 && e.time(rate) <= at).count();
        assert!(recent > 0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), at, &kit.draws, &mut frame);
        assert_eq!(frame.flashes.len(), recent);
        // Without the reaction, no flash.
        let card = scene.card_mut(BALL, SlotId::Look).unwrap();
        let flash = card.layers.iter().find(|l| l.kind == crate::modules::look::layers::FLASH).unwrap().id;
        scene.remove_layer(&kit.registry, BALL, SlotId::Look, flash).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), at, &kit.draws, &mut frame);
        assert!(frame.flashes.is_empty());
    }

    #[test]
    fn hit_sub_layers_give_each_ball_its_last_hit_and_its_hits_so_far() {
        use crate::modules::look::layers::{OUTLINE, SUB_FLASH};
        let kit = kit();
        let reg = &kit.registry;
        let mut scene = new_scene(reg);
        let outline = scene.add_layer(reg, BALL, SlotId::Look, OUTLINE).unwrap();
        let resolved = bpe_core::resolve(&scene, reg);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(10.0);
        let rate = playback.world().steps_per_second;
        let mine: Vec<f64> = playback
            .log()
            .events()
            .iter()
            .filter(|e| e.kind == bpe_runtime::EventKind::Hit && e.a.thing == BALL && e.a.item == 0)
            .map(|e| e.time(rate))
            .collect();
        assert!(mine.len() >= 3, "the ball hits the arena three times: {mine:?}");
        let at = mine[2] + 0.01;
        playback.seek(at);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), at, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].hit, bpe_gfx::NO_HIT, "nothing reads hits yet");
        scene.add_sub_layer(reg, BALL, SlotId::Look, outline, SUB_FLASH).unwrap();
        let resolved = bpe_core::resolve(&scene, reg);
        build_frame(&scene, &resolved, playback.world(), playback.log(), at, &kit.draws, &mut frame);
        let hit = frame.layered[0].hit;
        assert_eq!(hit[2], playback.world().bodies[&BALL].hits[0] as f32);
        assert!(hit[2] >= 3.0, "{hit:?}");
        assert!((hit[0] - 0.01).abs() < 0.006, "just after its last hit: {hit:?}");
        assert!(hit[1] > 0.0 && hit[1] <= 1.5);
    }

    #[test]
    fn a_hidden_reaction_does_nothing() {
        let frame = frame_after(0.0, |s| {
            let reg = registry();
            let hue = set_layer(s, BALL, HUE, "hue_per_hit", Value::Number(120.0));
            s.set_layer_enabled(&reg, BALL, SlotId::Look, hue, false).unwrap();
            s.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(frame.layered[0].body, pink);
    }

    #[test]
    fn inside_linear_on_a_ball_adds_a_pattern_record() {
        let frame = frame_after(0.0, |s| {
            set_layer(s, BALL, PATTERN, "in_pattern", Value::Choice("linear".into()));
        });
        let pattern = frame.look_layers.iter().find(|l| l.head[0] == LayerGpu::PATTERN).unwrap();
        assert_eq!(pattern.paint.pattern, 1.0);
        assert_eq!(pattern.paint.row, -2.0, "a shade of the ball's own colour by default");
    }

    #[test]
    fn a_look_that_is_off_draws_nothing() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.remove_slot(ThingId(1), SlotId::Look);
        scene.remove_slot(BALL, SlotId::Look);
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.layered.is_empty() && frame.paths.is_empty());
    }

    #[test]
    fn colours_can_follow_hits_and_shift_hue_per_hit() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", bpe_core::Value::Int(2));
            p.set("rows", bpe_core::Value::Int(1));
            p.set("velocity", bpe_core::Value::Vec2([0.0, 0.0]));
        });
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("hits".into()));
        set_layer(&mut scene, BALL, BODY, "span", Value::Number(2.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        world.bodies.get_mut(&BALL).unwrap().hits[1] = 2;
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.layered[1].body, [0.0, 0.0, 1.0, 1.0]);
        // By order: first and last colour.
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("order".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!((frame.layered[0].body, frame.layered[1].body), ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]));
        // By speed: both still, so both red.
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("speed".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[1].body, [1.0, 0.0, 0.0, 1.0]);
        // Colour on hit: red turned 240° (two hits of 120°) is blue; the first ball with no hits
        // stays red. Its hit marks and trails take the same colour.
        set_layer(&mut scene, BALL, HUE, "hue_per_hit", Value::Number(120.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, [1.0, 0.0, 0.0, 1.0]);
        let b = frame.layered[1].body;
        assert!(b[2] > 0.9 && b[0] < 0.1, "{b:?}");
    }

    #[test]
    fn colour_over_time_turns_the_body_round_the_wheel() {
        let at = |time: f64| {
            frame_after(time, |s| {
                s.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
                set_layer(s, BALL, crate::modules::look::layers::RAINBOW, "rainbow", Value::Number(0.5));
            })
            .layered[0]
                .body
        };
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(at(0.0), pink);
        let half = crate::modules::look::basic::shift_linear_hue(pink, 0.5);
        assert_eq!(bpe_core::Color::from_linear(at(1.0)), bpe_core::Color::from_linear(half));
    }

    #[test]
    fn balls_leave_a_hit_mark_in_their_colour() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        // Hits are listed for what uses them: here the scene's effect.
        crate::testing::add_effect(&mut scene, &kit.registry, "picture.kaleidoscope");

        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let rate = playback.world().steps_per_second;
        let recent: Vec<_> = playback.log().events().iter().filter(|e| e.time(rate) > 4.0 - 3.0).collect();
        assert!(!recent.is_empty(), "the ball hit nothing in four seconds");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), recent.len());
        let mark = frame.hits[0];
        assert_eq!((mark.thing, mark.other), (BALL, Some(ThingId(1))));
        assert_eq!(mark.point, <[f32; 2]>::from(recent[0].point));
        assert_eq!(mark.speed, recent[0].speed);
        assert!((mark.time - recent[0].time(rate) as f32).abs() < 1e-6);
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(mark.color, pink);
        // A hit further back than the memory is not listed.
        playback.seek(10.0);
        let old: Vec<_> = playback.log().events().iter().filter(|e| e.time(rate) > 7.0).collect();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 10.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), old.len());
        // Without an effect that wants them, only the flashes' quarter second is listed.
        scene.remove_slot(ThingId::ROOT, SlotId::Shader);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 10.0, &kit.draws, &mut frame);
        let flash_worth = playback.log().events().iter().filter(|e| e.time(rate) > 10.0 - 0.25).count();
        assert_eq!(frame.hits.len(), flash_worth);
    }

    #[test]
    fn an_effect_on_the_scene_settings_still_gets_the_balls_hits() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        crate::testing::add_effect(&mut scene, &kit.registry, "picture.kaleidoscope");

        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(!resolved.things[&BALL].slots.contains_key(&SlotId::Shader), "the effect is not passed down");
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 4.0 - 3.0).count();
        assert!(recent > 0, "the ball hit nothing in four seconds");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), recent, "the scene's effect pulses on every hit it remembers");
    }

    #[test]
    fn stepping_on_hits_reads_the_log_and_the_hit_looks_once_a_frame() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(4));
            p.set("rows", Value::Int(3));
        });
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("step".into()));
        set_layer(&mut scene, BALL, BODY, "take_hit_colour", Value::Bool(true));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(6.0);
        let hit = playback.world().bodies[&BALL].hits.iter().filter(|&&h| h > 0).count();
        assert!(hit >= 4, "only {hit} balls hit something");
        let (searches, reads) = (&crate::paint::eval::LOG_SEARCHES, &crate::modules::look::draw::LOOK_READS);
        let (s0, r0) = (searches.with(|n| n.get()), reads.with(|n| n.get()));
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 6.0, &kit.draws, &mut frame);
        let (s1, r1) = (searches.with(|n| n.get()) - s0, reads.with(|n| n.get()) - r0);
        // The balls' draw reads their colours once; that reads the log once, and each other
        // thing's look once, whatever the number of balls.
        assert!(s1 <= 2, "{s1} searches of the log for {hit} balls that hit");
        assert!(r1 <= 2, "{r1} reads of the hit things' looks for {hit} balls that hit");
    }

    const RED: bpe_core::Color = bpe_core::Color::rgb(255, 0, 0);
    const BLUE: bpe_core::Color = bpe_core::Color::rgb(0, 0, 255);

    #[test]
    fn row_follow_is_place_from_spawn_at_90_degrees() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(3));
            p.set("spacing", Value::Number(40.0));
        });
        scene.things.get_mut(&BALL).unwrap().transform.position = glam::Vec2::new(30.0, -200.0);
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("start".into()));
        set_layer(&mut scene, BALL, BODY, "angle", Value::Number(90.0));
        set_layer(&mut scene, BALL, BODY, "centre", Value::Vec2([30.0, -200.0]));
        // From the first row's middle to the last's.
        set_layer(&mut scene, BALL, BODY, "span", Value::Number(80.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(3.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 3.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered.len(), 6);
        let body: Vec<[f32; 4]> = frame.layered.iter().map(|b| b.body).collect();
        let near = |a: [f32; 4], b: [f32; 4]| (0..4).all(|k| (a[k] - b[k]).abs() < 1e-5);
        let (red, blue) = (RED.to_linear(), BLUE.to_linear());
        // Where the balls started counts, not where they are now.
        assert!(near(body[0], red) && near(body[1], red), "{body:?}");
        let middle = bpe_core::Gradient::even(&[RED, BLUE]).sample(0.5);
        assert!(near(body[2], middle) && near(body[3], middle), "{body:?}");
        assert!(near(body[4], blue) && near(body[5], blue), "{body:?}");
        // Hits flash: the flashes match the log.
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 3.0 - 0.25).count();
        assert_eq!(frame.flashes.len(), recent, "{:?}", playback.log().events());
    }

    #[test]
    fn a_plain_look_bakes_nothing_and_a_pattern_paints_from_the_ball_s_colour_or_its_own() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(1));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.look_layers.iter().all(|l| l.head[0] != LayerGpu::PATTERN));
        // The shape's Body record, then one stack shared by the three balls.
        assert_eq!(frame.look_layers.len(), 1 + 3, "one stack of records shared by the three balls");
        assert!(frame.layered.iter().all(|b| b.range[..2] == [1, 3]));
        assert!(frame.gradients.len() == 1, "nothing is baked for a plain look");

        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("order".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_pattern", Value::Choice("rings".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_spin", Value::Number(90.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 2.0, &kit.draws, &mut frame);
        assert_eq!(frame.gradients.len(), 1, "a shade of each ball's own colour bakes nothing");
        let paint = frame.look_layers.iter().find(|l| l.head[0] == LayerGpu::PATTERN).unwrap().paint;
        assert_eq!((paint.row, paint.pattern, paint.angle), (-2.0, 6.0, 270.0));
        assert_eq!(paint.t0, -0.5, "the default shade");
        // Its own gradient takes one row the balls share.
        set_layer(&mut scene, BALL, PATTERN, "in_pattern", Value::Choice("dots".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_from", Value::Choice("own_gradient".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 2.0, &kit.draws, &mut frame);
        assert_eq!(frame.gradients.len(), 2, "the balls share one row");
        let paint = frame.look_layers.iter().find(|l| l.head[0] == LayerGpu::PATTERN).unwrap().paint;
        assert_eq!((paint.row, paint.pattern), (1.0, 7.0));
    }

    #[test]
    fn stepping_on_hits_can_take_the_colour_of_what_was_hit() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("step".into()));
        set_layer(&mut scene, BALL, BODY, "take_hit_colour", Value::Bool(true));
        // The shape is plain white.
        let white = bpe_core::Color::WHITE;
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, RED.to_linear(), "no hits yet: the first colour");
        playback.seek(4.0);
        assert!(playback.world().bodies[&BALL].hits[0] > 0, "the ball hit the wall");
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, white.to_linear());
        // Without taking it, the ball steps through its own colours.
        set_layer(&mut scene, BALL, BODY, "take_hit_colour", Value::Bool(false));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_ne!(frame.layered[0].body, white.to_linear());
    }

    #[test]
    fn a_ball_hit_by_a_layered_ball_takes_its_body_colour() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let other = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let green = bpe_core::Color::rgb(0, 255, 0);
        set_layer(&mut scene, other, BODY, "colours", Value::Gradient(bpe_core::Gradient::solid(green)));
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let cx = bpe_gfx::DrawCx {
            scene: &resolved,
            resolved: &resolved.things[&BALL],
            thing: &scene.things[&BALL],
            world: &world,
            log: &Log::default(),
            time: 0.0,
            tree: &scene,
        };
        let still = crate::modules::look::draw::StillColours::new(&cx);
        assert_eq!(still.at(other, glam::Vec2::ZERO), Some(green.to_linear()));
    }

    #[test]
    fn a_ball_group_with_vary_hue_has_different_colours() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(4));
            p.set("rows", Value::Int(2));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED])));
        set_layer(&mut scene, BALL, BODY, "vary_hue", Value::Number(40.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let build = || {
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
            frame.layered.iter().map(|b| b.body).collect::<Vec<_>>()
        };
        let colours = build();
        assert_eq!(colours.len(), 8);
        let mut distinct: Vec<[f32; 4]> = Vec::new();
        for c in &colours {
            if !distinct.iter().any(|d| (0..3).all(|k| (d[k] - c[k]).abs() < 1e-4)) {
                distinct.push(*c);
            }
        }
        assert!(distinct.len() >= 6, "only {} distinct colours: {colours:?}", distinct.len());
        // The same frame built twice is the same.
        assert_eq!(build(), colours);
    }

    #[test]
    fn an_inside_pattern_carries_each_ball_s_variation_to_the_gpu() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(1));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        set_layer(&mut scene, BALL, PATTERN, "in_pattern", Value::Choice("stripes".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_from", Value::Choice("own_gradient".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        assert!(frame.layered.iter().all(|b| b.vary == [0.0, 0.0, 1.0, 0.0]), "no variation is the identity");
        set_layer(&mut scene, BALL, BODY, "vary_hue", Value::Number(90.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        let turns: Vec<f32> = frame.layered.iter().map(|b| b.vary[0]).collect();
        assert!(turns.iter().all(|t| t.abs() <= 0.25), "{turns:?}");
        assert!(turns[0] != turns[1] && turns[1] != turns[2], "{turns:?}");
    }

    /// The frame at time 0 for a still group of `count` balls in a line after `f`'s changes.
    fn group_frame(count: i64, f: impl FnOnce(&mut Scene)) -> Frame {
        frame_after(0.0, |s| {
            let p = &mut s.things.get_mut(&BALL).unwrap().params;
            p.set("group", Value::Bool(true));
            p.set("shape", Value::Choice("line".into()));
            p.set("count", Value::Int(count));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
            f(s);
        })
    }

    #[test]
    fn a_group_shader_gives_each_ball_its_place_in_the_group_and_draws_with_a_program() {
        use crate::modules::look::layers::GROUP_SHADER;
        let plain = group_frame(5, |_| {});
        assert!(plain.layered.iter().all(|b| b.place == [0.0; 4]), "no group shader, no place");
        let frame = group_frame(5, |s| {
            set_layer(s, BALL, GROUP_SHADER, "amount", Value::Number(1.0));
        });
        let xs: Vec<f32> = frame.layered.iter().map(|b| b.place[0]).collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "along the line: {xs:?}");
        assert!(xs.iter().sum::<f32>().abs() < 1e-3, "from the group's middle: {xs:?}");
        let side = frame.layered[0].place[2];
        assert!(side > xs[4] - xs[0], "the square holds every ball: {side}");
        assert_eq!(frame.programs.len(), 1);
        assert_eq!(frame.layered_runs.last().unwrap().program, Some(0));
    }

    #[test]
    fn colour_spread_turns_a_gradient_body_by_whole_turns() {
        let frame = group_frame(6, |s| {
            s.things.get_mut(&BALL).unwrap().params.set("vary_colour", Value::Number(1.0));
            set_layer(s, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
            set_layer(s, BALL, BODY, "follow", Value::Choice("each_ball".into()));
        });
        let turns: Vec<f32> = frame.layered.iter().map(|b| b.vary[3]).collect();
        let (lo, hi) = turns.iter().fold((f32::MAX, f32::MIN), |(l, h), t| (l.min(*t), h.max(*t)));
        assert!(hi - lo > 0.2, "a full spread turns the balls far round the wheel: {turns:?}");
        assert!(turns.iter().all(|t| t.abs() <= 0.5), "{turns:?}");
    }

    #[test]
    fn a_group_gradient_by_order_gives_each_ball_its_own_body_colour_in_order() {
        use crate::modules::look::layers::GROUP_GRADIENT;
        let frame = group_frame(10, |s| {
            set_layer(s, BALL, GROUP_GRADIENT, "spread", Value::Choice("order".into()));
            set_layer(s, BALL, GROUP_GRADIENT, "amount", Value::Number(1.0));
        });
        assert_eq!(frame.layered.len(), 10);
        let blue = bpe_core::Color::rgb(0x3e, 0x9b, 0xff).to_linear();
        let pink = bpe_core::Color::rgb(0xff, 0x3e, 0xc8).to_linear();
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).take(3).all(|(x, y)| (x - y).abs() < 1e-3);
        assert!(close(frame.layered[0].body, blue) && close(frame.layered[9].body, pink));
        let reds: Vec<f32> = frame.layered.iter().map(|b| b.body[0]).collect();
        assert!(reds.windows(2).all(|w| w[1] > w[0]), "blue to pink in order: {reds:?}");
        assert!(frame.layered.iter().all(|b| b.range[3] == 0), "the body is mixed here, not on the GPU");
    }

    #[test]
    fn every_layer_puts_the_group_colour_on_the_instance_and_a_group_record_last() {
        use crate::modules::look::layers::GROUP_GRADIENT;
        let frame = group_frame(3, |s| {
            set_layer(s, BALL, GROUP_GRADIENT, "changes", Value::Choice("every_layer".into()));
            set_layer(s, BALL, GROUP_GRADIENT, "mix", Value::Choice("multiply".into()));
        });
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(frame.layered[0].body, pink, "the body keeps its own colour");
        let ball = frame.layered[2];
        assert_eq!(ball.range[3], bpe_core::Blend::Multiply.shader_code() + 1);
        assert!((ball.group[3] - 0.8).abs() < 1e-6, "the amount");
        let records = &frame.look_layers[ball.range[0] as usize..(ball.range[0] + ball.range[1]) as usize];
        assert_eq!(records.last().unwrap().head[0], LayerGpu::GROUP);
        assert_ne!(frame.layered[0].group, frame.layered[2].group, "each ball its own colour");
    }

    #[test]
    fn choose_flags_only_the_chosen_layers() {
        use crate::modules::look::layers::{GROUP_GRADIENT, OUTLINE};
        let frame = group_frame(2, |s| {
            let outline = set_layer(s, BALL, OUTLINE, "width", Value::Number(6.0));
            set_layer(s, BALL, GROUP_GRADIENT, "changes", Value::Choice("choose".into()));
            set_layer(s, BALL, GROUP_GRADIENT, "chosen", Value::Text(outline.0.to_string()));
        });
        let flagged: Vec<(u32, bool)> =
            ball_records(&frame).iter().map(|r| (r.head[0], r.head[2] & LayerGpu::GROUPED != 0)).collect();
        assert!(flagged.contains(&(LayerGpu::OUTLINE, true)), "{flagged:?}");
        assert!(flagged.iter().filter(|(_, g)| *g).count() == 1, "{flagged:?}");
        assert!(!flagged.iter().any(|(c, _)| *c == LayerGpu::GROUP), "no whole-ball record for Choose");
    }

    #[test]
    fn a_layer_given_to_one_ball_shows_only_in_its_range() {
        use crate::modules::look::layers::OUTLINE;
        let frame = group_frame(5, |s| {
            let reg = registry();
            let outline = set_layer(s, BALL, OUTLINE, "width", Value::Number(6.0));
            s.set_layer_only(&reg, BALL, SlotId::Look, outline, vec![3]).unwrap();
        });
        // Every range keeps every record, so masks and the group colour find theirs; the balls
        // that don't wear the outline have it at no opacity.
        let counts: Vec<u32> = frame.layered.iter().map(|b| b.range[1]).collect();
        assert!(counts.iter().all(|c| *c == counts[0]), "{counts:?}");
        let outline_of = |i: usize| {
            let r = frame.layered[i].range;
            *frame.look_layers[r[0] as usize..(r[0] + r[1]) as usize]
                .iter()
                .find(|l| l.head[0] == LayerGpu::OUTLINE)
                .unwrap()
        };
        assert_eq!(outline_of(3).amount[0], 1.0);
        assert!((0..5).filter(|i| *i != 3).all(|i| outline_of(i).amount[0] == 0.0));
        assert_eq!(outline_of(3).head[2] & LayerGpu::UNWORN, 0);
        assert!((0..5).filter(|i| *i != 3).all(|i| outline_of(i).head[2] & LayerGpu::UNWORN != 0));
        let firsts: std::collections::BTreeSet<u32> = frame.layered.iter().map(|b| b.range[0]).collect();
        assert_eq!(firsts.len(), 2, "two ranges in all: one with the outline, one without");
    }

    #[test]
    fn look_spreads_show_without_building_the_world_again() {
        // The editor keeps its world when only the look changes, so the spreads come from the
        // scene as it is now, not the world's copy of the settings.
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let p = &mut scene.things.get_mut(&BALL).unwrap().params;
        p.set("group", Value::Bool(true));
        p.set("count", Value::Int(6));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        let p = &mut scene.things.get_mut(&BALL).unwrap().params;
        p.set("vary_turn", Value::Number(90.0));
        p.set("vary_colour", Value::Number(1.0));
        let spread = bpe_core::resolve(&scene, &kit.registry);
        let draw = |resolved: &bpe_core::Resolved| {
            let mut frame = Frame::default();
            build_frame(&scene, resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
            frame
        };
        let (before, after) = (draw(&resolved), draw(&spread));
        assert!(before.layered.iter().all(|b| b.form[2] == 0.0));
        assert!(after.layered.iter().any(|b| b.form[2] != 0.0), "turned");
        assert!(before.layered.iter().zip(&after.layered).any(|(a, b)| a.body != b.body), "coloured");
    }

    #[test]
    fn turn_spread_turns_each_ball_the_same_way_every_time() {
        let turned = |seed: i64| {
            group_frame(6, |s| {
                let p = &mut s.things.get_mut(&BALL).unwrap().params;
                p.set("vary_turn", Value::Number(90.0));
                p.set("seed", Value::Int(seed));
            })
            .layered
            .iter()
            .map(|b| b.form[2])
            .collect::<Vec<f32>>()
        };
        let a = turned(1);
        assert_eq!(a, turned(1));
        assert_ne!(a, turned(2));
        assert!(a.iter().any(|t| t.abs() > 0.1) && a.iter().all(|t| t.abs() <= 90f32.to_radians() + 1e-5));
        let plain = group_frame(6, |_| {});
        assert!(plain.layered.iter().all(|b| b.form[2] == 0.0));
    }

    /// The new scene's ball made a line of `count`, its look given a layer of `kind` whose
    /// setting `key` varies by Number from `from` to `to`; with `vary` false, left at `from`.
    fn varied_line(
        count: u32,
        kind: &str,
        key: &str,
        from: Value,
        to: Value,
        vary: bool,
    ) -> (Scene, bpe_core::LayerId) {
        let kit = kit();
        let reg = &kit.registry;
        let mut scene = new_scene(reg);
        let ball = scene.things.get_mut(&BALL).unwrap();
        ball.params.set("group", Value::Bool(true));
        ball.params.set("shape", Value::Choice("line".into()));
        ball.params.set("count", Value::Int(count as i64));
        ball.params.set("radius", Value::Number(10.0));
        let layer = scene.add_layer(reg, BALL, SlotId::Look, kind).unwrap();
        let at = bpe_core::Address::on(BALL, SlotId::Look, &bpe_core::layered_key(layer, key));
        scene.set_at(reg, &at, from).unwrap();
        if vary {
            scene.start_varying(reg, &at).unwrap();
            let to_at = bpe_core::Address { key: bpe_core::vary::to_key(&at.key), ..at.clone() };
            scene.set_at(reg, &to_at, to).unwrap();
        }
        (scene, layer)
    }

    /// Each layered ball's (by row) record of `code`'s first number.
    fn record_values(frame: &Frame, code: u32) -> Vec<f32> {
        frame
            .layered
            .iter()
            .map(|b| {
                let r = b.range;
                let records = &frame.look_layers[r[0] as usize..(r[0] + r[1]) as usize];
                records.iter().find(|l| l.head[0] == code).map_or(-1.0, |l| l.p0[0])
            })
            .collect()
    }

    /// The frame of `scene` at `time`.
    fn frame_of(scene: &Scene, time: f64) -> Frame {
        let kit = kit();
        let resolved = bpe_core::resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = bpe_runtime::Playback::new(scene, &resolved, kit.sims.clone());
        playback.seek(time);
        let mut frame = Frame::default();
        build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
        frame
    }

    #[test]
    fn a_varied_layer_setting_gives_each_ball_its_own_records() {
        use crate::modules::look::layers::OUTLINE;
        let (scene, _) = varied_line(5, OUTLINE, "width", Value::Number(0.0), Value::Number(20.0), true);
        let frame = frame_of(&scene, 0.0);
        let widths = record_values(&frame, LayerGpu::OUTLINE);
        assert_eq!(widths.len(), 5);
        // Each ball by its number, the first at From and the last at To.
        let mut sorted = widths.clone();
        sorted.sort_by(f32::total_cmp);
        assert_eq!(sorted.first(), Some(&0.0));
        assert_eq!(sorted.last(), Some(&20.0));
        assert!((sorted[2] - 10.0).abs() < 0.05, "{widths:?}");
        // Unvaried, every ball shares one set of records.
        let (plain, _) = varied_line(5, OUTLINE, "width", Value::Number(0.0), Value::Number(20.0), false);
        let frame = frame_of(&plain, 0.0);
        assert!(record_values(&frame, LayerGpu::OUTLINE).iter().all(|w| *w == 0.0));
        let ranges: std::collections::BTreeSet<u32> = frame.layered.iter().map(|b| b.range[0]).collect();
        assert_eq!(ranges.len(), 1);
    }

    #[test]
    fn a_vary_to_the_same_value_draws_the_balls_as_before() {
        use crate::modules::look::layers::OUTLINE;
        let (same, _) = varied_line(4, OUTLINE, "width", Value::Number(6.0), Value::Number(6.0), true);
        let (plain, _) = varied_line(4, OUTLINE, "width", Value::Number(6.0), Value::Number(6.0), false);
        let (a, b) = (frame_of(&same, 0.5), frame_of(&plain, 0.5));
        assert_eq!(record_values(&a, LayerGpu::OUTLINE), record_values(&b, LayerGpu::OUTLINE));
        let strip = |f: &Frame| f.layered.iter().map(|l| (l.center, l.radius, l.body)).collect::<Vec<_>>();
        assert_eq!(strip(&a), strip(&b));
    }

    #[test]
    fn many_balls_share_at_most_one_set_of_records_per_step() {
        use crate::modules::look::layers::OUTLINE;
        let (scene, _) = varied_line(1500, OUTLINE, "width", Value::Number(0.0), Value::Number(20.0), true);
        let frame = frame_of(&scene, 0.0);
        assert_eq!(frame.layered.len(), 1500);
        let ranges: std::collections::BTreeSet<u32> = frame.layered.iter().map(|b| b.range[0]).collect();
        assert!(ranges.len() <= bpe_core::vary::VARY_LEVELS as usize, "{}", ranges.len());
        assert!(ranges.len() > 500, "the line's balls spread over many steps: {}", ranges.len());
    }

    #[test]
    fn a_varied_trail_setting_changes_each_ball_s_own_trail() {
        use crate::modules::look::layers::trail::TRAIL;
        // The first ball's trail is dark, so it draws none; the last's is bright.
        let (scene, _) = varied_line(2, TRAIL, "brightness", Value::Number(0.0), Value::Number(1.0), true);
        let varied = frame_of(&scene, 1.0);
        let (plain, _) = varied_line(2, TRAIL, "brightness", Value::Number(1.0), Value::Number(1.0), false);
        let both = frame_of(&plain, 1.0);
        let strips = |f: &Frame| f.trails.len() + f.trails_front.len();
        assert!(strips(&both) > 0);
        assert!(strips(&varied) > 0, "the bright ball still has its trail");
        assert!(strips(&varied) < strips(&both), "{} of {}", strips(&varied), strips(&both));
    }

    #[test]
    fn a_varied_particles_setting_changes_each_ball_s_own_particles() {
        use crate::modules::look::layers::particles::PARTICLES;
        let steady = |scene: &mut Scene, layer| {
            let at = bpe_core::Address::on(BALL, SlotId::Look, &bpe_core::layered_key(layer, "emit"));
            scene.set_at(&kit().registry, &at, Value::Choice("steady".into())).unwrap();
        };
        // The first ball lets off one a second, the last a hundred.
        let (mut varied, layer) = varied_line(2, PARTICLES, "rate", Value::Number(1.0), Value::Number(100.0), true);
        steady(&mut varied, layer);
        let (mut plain, layer) = varied_line(2, PARTICLES, "rate", Value::Number(100.0), Value::Number(100.0), false);
        steady(&mut plain, layer);
        let count = |f: &Frame| f.particles.len() + f.particles_front.len();
        let (some, both) = (count(&frame_of(&varied, 1.0)), count(&frame_of(&plain, 1.0)));
        assert!(both > 0);
        assert!(some > both / 3 && some < both * 2 / 3, "about half: {some} of {both}");
    }
}
