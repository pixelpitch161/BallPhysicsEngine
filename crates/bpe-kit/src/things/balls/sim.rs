//! Placing the balls, letting them go and keeping their settings.

use bpe_core::vary::{self, BallSpread, Vary};
use bpe_core::{Params, ResolvedThing, Settings, Thing, ThingId, Value};
use bpe_runtime::rules::Ruled;
use bpe_runtime::{Bodies, Log, NewBall, Pose, ThingSim, World, state, state_mut};
use glam::Vec2;

use super::rows::BallRows;
use super::shapes::offsets;
use super::{BallsSettings, Launch, Release, ReleaseOrder};
use crate::rng::Rng;

pub use super::shapes::offsets as group_offsets;

pub struct BallsSim;

/// What the balls keep between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct BallsState {
    pub settings: BallsSettings,
    /// Each body row's own facts. A row's ball number is its place in the group's shape: balls
    /// let go one by one are pushed in the order they go, so a row isn't always its number.
    pub rows: BallRows,
    /// Where each ball starts from the thing, by number.
    pub offsets: Vec<Vec2>,
    /// The balls still to be let go: the step each goes on and its number, soonest last.
    pub pending: Vec<(u64, u32)>,
    /// The thing's own settings that vary across the balls (Size, Launch speed), with a To.
    pub varies: Vec<Vary>,
    /// Each ball's place from left to right in the group (0 to 1), by number.
    pub place: Vec<f32>,
    /// Which way each ball is launched, round the circle (0 to 1), by number.
    pub launch: Vec<f32>,
    /// Each body row's own size: its Size, varied and with its Variety share, before any growth.
    pub own_size: Vec<f32>,
    /// A rule changed some ball's Size the last time the settings changed, so the sizes are
    /// worked out again when that change ends.
    pub ruled_size: bool,
}

impl BallsState {
    /// The ball number of body row `row` (the row itself for anything not listed).
    pub fn number(&self, row: usize) -> u32 {
        self.rows.number(row)
    }

    /// Where ball `number` sits in the spreads, before it moves (no speed, no hits).
    pub fn spread_of_number(&self, number: u32) -> BallSpread {
        let n = number as usize;
        BallSpread {
            number,
            count: self.offsets.len().max(1) as u32,
            round: 0.0,
            launch: self.launch.get(n).copied().unwrap_or(0.0),
            place: self.place.get(n).copied().unwrap_or(0.0),
            speed: 0.0,
            hits: 0,
            seed: self.settings.seed as u32,
            alone: !self.settings.group,
            ..BallSpread::default()
        }
    }

    /// Where body row `row` of `bodies` sits in the spreads, as it moves now.
    pub fn spread_of(&self, bodies: &Bodies, row: usize) -> BallSpread {
        BallSpread {
            speed: bodies.velocity.get(row).map_or(0.0, |v| v.length()),
            hits: bodies.hits.get(row).copied().unwrap_or(0),
            ..self.spread_of_number(self.number(row))
        }
    }

    /// `key`'s value for a ball at `spread`: `base`, mixed towards its To when it varies.
    fn own_value(&self, key: &str, base: Value, spread: &BallSpread) -> Value {
        match self.varies.iter().find(|v| v.key == key) {
            Some(v) => {
                let to = v.to.as_ref().expect("only varies with a To are kept");
                vary::mixed(&base, to, v.amount(spread.t(v.by, v.salt())))
            }
            None => base,
        }
    }

    /// Ball `spread`'s Size, varied (before its Variety share).
    fn size_at(&self, spread: &BallSpread) -> f32 {
        match self.own_value("radius", Value::Number(self.settings.radius as f64), spread) {
            Value::Number(r) => r as f32,
            _ => self.settings.radius,
        }
    }

    /// Ball `spread`'s Launch speed, varied.
    fn velocity_at(&self, spread: &BallSpread) -> Vec2 {
        let v = self.settings.velocity;
        match self.own_value("velocity", Value::Vec2([v.x, v.y]), spread) {
            Value::Vec2([x, y]) => Vec2::new(x, y),
            _ => v,
        }
    }

    /// Whether the Size varies across the balls.
    fn size_varies(&self) -> bool {
        self.varies.iter().any(|v| v.key == "radius")
    }
}

/// The thing's own settings in `params` that vary across its balls: Size and Launch speed.
fn own_varies(params: &Params) -> Vec<Vary> {
    vary::varies_in(params)
        .into_iter()
        .filter(|v| matches!(v.key.as_str(), "radius" | "velocity") && v.to.is_some())
        .collect()
}

/// The settings a group is laid out by: when its Size varies up to `size_to` (bigger than Size),
/// its places are spaced for that bigger end, so no ball starts overlapping its neighbours.
pub fn laid_out(s: &BallsSettings, size_to: Option<f32>) -> std::borrow::Cow<'_, BallsSettings> {
    match size_to {
        Some(to) if to > s.radius => std::borrow::Cow::Owned(BallsSettings { radius: to, ..s.clone() }),
        _ => std::borrow::Cow::Borrowed(s),
    }
}

/// The far end of a Size that varies across the balls, in `params`.
pub fn size_to(params: &Params) -> Option<f32> {
    match vary::vary_of(params, "radius")?.to? {
        Value::Number(x) => Some(x as f32),
        _ => None,
    }
}

/// Each ball's place from left to right across `offsets` (0 to 1; all 0 when they line up).
fn places_across(offsets: &[Vec2]) -> Vec<f32> {
    let (lo, hi) = offsets.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    let width = hi - lo;
    offsets.iter().map(|p| if width > 1e-4 { ((p.x - lo) / width).clamp(0.0, 1.0) } else { 0.0 }).collect()
}

/// The ball numbers of `thing`'s body rows' spreads, when it is a Balls thing in `world`.
pub fn spread_in(world: &World, thing: ThingId, row: usize) -> Option<BallSpread> {
    let st = world.states.get(&thing)?.as_any().downcast_ref::<BallsState>()?;
    let bodies = world.bodies.get(&thing)?;
    Some(st.spread_of(bodies, row))
}

/// The ball numbers of `thing`'s body rows, when it is a thing with ball rows (Balls, Emitter)
/// in `world`.
pub fn numbers(world: &World, thing: ThingId) -> Option<&[u32]> {
    super::rows::rows(world, thing).map(|r| r.numbers.as_slice())
}

/// The group number of `thing`'s ball in body row `item` (the row itself when `thing` has no
/// ball rows).
pub fn ball_number(world: &World, thing: ThingId, item: u32) -> u32 {
    numbers(world, thing).and_then(|n| n.get(item as usize).copied()).unwrap_or(item)
}

/// What Variety rolls for: a ball's size, its launch speed, its colour and its turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Roll {
    Size = 1,
    Speed = 2,
    Colour = 3,
    Turn = 4,
    Launch = 5,
    Order = 6,
}

/// Ball `number`'s own random number in 0 to 1 for `roll`, the same every run for one `seed`.
pub fn ball_unit(seed: i32, number: u32, roll: Roll) -> f32 {
    Rng::new(seed as u32, number ^ ((roll as u32) << 24)).unit()
}

/// `amount` of spread (0 to 1) as a factor round 1: 0.5 to 1.5 at full spread.
fn spread(seed: i32, number: u32, roll: Roll, amount: f32) -> f32 {
    if amount == 0.0 { 1.0 } else { 1.0 + amount.clamp(0.0, 1.0) * (ball_unit(seed, number, roll) - 0.5) }
}

/// Ball `number`'s size as a share of the Size setting.
pub fn size_factor(s: &BallsSettings, number: u32) -> f32 {
    if s.group { spread(s.seed, number, Roll::Size, s.vary_size) } else { 1.0 }
}

/// How far round the colour wheel ball `number`'s colours turn, in turns (Variety).
pub fn colour_turn(s: &BallsSettings, number: u32) -> f32 {
    if !s.group || s.vary_colour == 0.0 {
        return 0.0;
    }
    s.vary_colour.clamp(0.0, 1.0) * (ball_unit(s.seed, number, Roll::Colour) - 0.5)
}

/// How far ball `number`'s look is turned, in degrees (Variety).
pub fn look_turn(s: &BallsSettings, number: u32) -> f32 {
    if !s.group || s.vary_turn == 0.0 {
        return 0.0;
    }
    s.vary_turn * (2.0 * ball_unit(s.seed, number, Roll::Turn) - 1.0)
}

/// The ball numbers in the order they are let go.
pub fn release_order(s: &BallsSettings, offsets: &[Vec2]) -> Vec<u32> {
    let n = offsets.len() as u32;
    let mut order: Vec<u32> = (0..n).collect();
    // Let go all at once, the balls keep their numbers' order.
    if !s.group || s.release == Release::AllAtOnce {
        return order;
    }
    match s.release_order {
        ReleaseOrder::FirstToLast => {}
        ReleaseOrder::CentreOut => {
            let middle = offsets.iter().copied().sum::<Vec2>() / n.max(1) as f32;
            let d = |i: &u32| offsets[*i as usize].distance_squared(middle);
            order.sort_by(|a, b| d(a).total_cmp(&d(b)).then(a.cmp(b)));
        }
        ReleaseOrder::Random => {
            let mut rng = Rng::new(s.seed as u32, (Roll::Order as u32) << 24);
            for i in (1..order.len()).rev() {
                let j = (rng.next_u32() as usize) % (i + 1);
                order.swap(i, j);
            }
        }
    }
    order
}

/// The step the `k`th ball let go goes on, at `rate` steps a second.
pub fn release_step(s: &BallsSettings, k: usize, rate: u32) -> u64 {
    if !s.group {
        return 0;
    }
    let slot = match s.release {
        Release::AllAtOnce => return 0,
        Release::OneByOne => k as f64,
        Release::Waves => (k / s.wave_size.max(1) as usize) as f64,
    };
    (slot * s.release_gap.max(0.0) as f64 * rate as f64).round() as u64
}

/// Ball `number`'s launch velocity (before what it rides on adds), `offset` from the middle of a
/// group whose places' centre is `middle`, of `count` balls.
pub fn launch_velocity(s: &BallsSettings, number: u32, count: usize, offset: Vec2, middle: Vec2) -> Vec2 {
    launch_velocity_from(s, s.velocity, number, count, offset, middle)
}

/// [`launch_velocity`] from the launch speed `v` (the ball's own, when it varies).
pub fn launch_velocity_from(s: &BallsSettings, v: Vec2, number: u32, count: usize, offset: Vec2, middle: Vec2) -> Vec2 {
    if !s.group {
        return v;
    }
    let speed = v.length();
    let way = if speed > 0.0 { v / speed } else { Vec2::X };
    let launched = match s.launch {
        Launch::Same => v,
        Launch::Fan => {
            let t = if count > 1 { number as f32 / (count - 1) as f32 } else { 0.5 };
            Vec2::from_angle((s.fan_angle * (t - 0.5)).to_radians()).rotate(v)
        }
        Launch::Outward => {
            let out = offset - middle;
            let dir = if out.length_squared() > 1e-8 { out.normalize() } else { way };
            dir * speed
        }
        Launch::Random => Vec2::from_angle(std::f32::consts::TAU * ball_unit(s.seed, number, Roll::Launch)) * speed,
    };
    let factor = spread(s.seed, number, Roll::Speed, s.vary_speed);
    if factor == 1.0 { launched } else { launched * factor }
}

/// Pushes ball `number` of the group, `offset` from the thing, as the thing stands at `at`.
fn push_ball(bodies: &mut Bodies, st: &mut BallsState, at: Pose, number: u32) {
    let s = &st.settings;
    let offset = st.offsets[number as usize];
    let turn = Vec2::from_angle(at.angle);
    let position = at.position + turn.rotate(offset);
    let middle = turn.rotate(st.offsets.iter().copied().sum::<Vec2>() / st.offsets.len().max(1) as f32);
    let spread = st.spread_of_number(number);
    let own_velocity = if st.varies.is_empty() { s.velocity } else { st.velocity_at(&spread) };
    let launched = launch_velocity_from(s, own_velocity, number, st.offsets.len(), turn.rotate(offset), middle);
    let velocity = launched + at.velocity + (position - at.position).perp() * at.spin;
    let factor = size_factor(s, number);
    let size = if st.varies.is_empty() { s.radius } else { st.size_at(&spread) };
    let radius = if factor == 1.0 { size } else { size * factor };
    let row = bodies.push(position, velocity, radius) as usize;
    bodies.number[row] = number;
    let launch = if launched.length_squared() > 0.0 { launched.to_angle() } else { 0.0 };
    st.rows.push(number, factor, 0, launch, offset, number);
    st.own_size.push(radius);
}

/// Moves every ball of `bodies` from its own size in `st` to `own` (by row): a ball still at its
/// own size takes the new one exactly; a grown one keeps its growth in proportion.
fn resize_to(st: &mut BallsState, bodies: &mut Bodies, own: Vec<f32>) {
    resize_rows(&mut st.own_size, bodies, own);
}

/// Moves every ball of `bodies` from its own size in `own_size` to `own` (by row): a ball still
/// at its own size takes the new one exactly; a grown one keeps its growth in proportion.
pub fn resize_rows(own_size: &mut Vec<f32>, bodies: &mut Bodies, own: Vec<f32>) {
    for (row, r) in bodies.radius.iter_mut().enumerate() {
        let (Some(&old), Some(&new)) = (own_size.get(row), own.get(row)) else { continue };
        if new != old {
            *r = if *r == old || old <= 0.0 { new } else { *r * (new / old) };
        }
    }
    *own_size = own;
}

/// Whether `s` is a group whose Word is made of a Text thing.
fn word_from_text(s: &BallsSettings) -> bool {
    s.group && s.shape == super::GroupShape::Word && s.made_of == super::WordFrom::ATextThing
}

/// Whether the balls spell a typed Word in a font other than the built-in one.
fn typed_in_a_font(s: &BallsSettings) -> bool {
    s.group
        && s.shape == super::GroupShape::Word
        && s.made_of == super::WordFrom::Typed
        && s.word_font != crate::text::fonts::FALLBACK_NAME
}

/// Each body row's own size in `st` as its settings, spreads and the rules' changes to single
/// balls (`ruled`) say now; `hurts` is how much of each row's health is gone (rules part 4).
fn own_sizes(st: &BallsState, bodies: &Bodies, ruled: Option<&Ruled>, hurts: &[f32]) -> Vec<f32> {
    (0..bodies.len().min(st.rows.size_factor.len()))
        .map(|row| {
            let spread = BallSpread { hurt: hurts.get(row).copied().unwrap_or(0.0), ..st.spread_of(bodies, row) };
            let size = st.size_at(&spread);
            let size = ruled.map_or(size, |r| r.number(row, size));
            let factor = st.rows.size_factor[row];
            if factor == 1.0 { size } else { size * factor }
        })
        .collect()
}

impl ThingSim for BallsSim {
    fn kind(&self) -> &'static str {
        "balls"
    }

    /// A Word made of a Text thing reads that Text from the scene (Text part 4).
    fn reads_tree(&self, resolved: &ResolvedThing) -> bool {
        let s = BallsSettings::from_params(&resolved.params);
        word_from_text(&s) || typed_in_a_font(&s)
    }

    /// The Text a Word is made of: its saved settings and font place the balls. A typed Word in
    /// another font: that font's file (a font loaded into the scene can change under its name).
    fn fingerprint_more(&self, scene: &bpe_core::Scene, thing: &Thing, resolved: &ResolvedThing) -> u64 {
        let s = BallsSettings::from_params(&resolved.params);
        if typed_in_a_font(&s) {
            return super::shapes::typed_font(&s, Some(scene)).map_or(0, |f| f.id);
        }
        if !word_from_text(&s) {
            return 0;
        }
        crate::things::text::shape::text_named(scene, &s.word_text, thing.id)
            .map_or(1, |text| crate::things::text::shape::letters_hash(scene, text))
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = BallsSettings::from_params(&resolved.params);
        // Riders start where their parent is and move with it; everything else sits where its
        // own transform says.
        let at = world.anchor(thing.id);
        // Sized first, so a word filled from a Text is spaced for the largest ball.
        let sized = laid_out(&settings, size_to(&resolved.params));
        let places = offsets(&super::shapes::with_text_word(&sized, world.tree.as_deref(), thing.id));
        let order = release_order(&settings, &places);
        let rate = world.steps_per_second;
        let middle = places.iter().copied().sum::<Vec2>() / places.len().max(1) as f32;
        let launch = (0..places.len())
            .map(|n| {
                let v = launch_velocity(&settings, n as u32, places.len(), places[n], middle);
                vary::launch_turn(v)
            })
            .collect();
        let mut st = BallsState {
            varies: own_varies(&resolved.params),
            place: places_across(&places),
            launch,
            own_size: Vec::with_capacity(places.len()),
            ruled_size: false,
            settings,
            rows: BallRows::default(),
            offsets: places,
            pending: Vec::new(),
        };
        let mut bodies = Bodies::default();
        for (k, number) in order.into_iter().enumerate() {
            match release_step(&st.settings, k, rate) {
                0 => push_ball(&mut bodies, &mut st, at, number),
                step => st.pending.push((step, number)),
            }
        }
        // Soonest last, so letting one go pops it.
        st.pending.reverse();
        world.bodies.insert(thing.id, bodies);
        world.states.insert(thing.id, Box::new(st));
    }

    /// What its trail and particle layers need of where the balls have been.
    fn history(&self, resolved: &ResolvedThing) -> Option<bpe_runtime::HistoryWant> {
        crate::modules::look::layers::around_history(resolved)
    }

    /// Lets go the balls whose time has come, where the thing is now.
    fn spawn(&self, thing: ThingId, world: &mut World, _log: &mut Log) {
        let Some(st) = world.states.get(&thing) else { return };
        let due = |st: &BallsState| st.pending.last().is_some_and(|(at, _)| *at <= world.step);
        if !due(state::<BallsState>(st.as_ref())) {
            return;
        }
        let at = world.anchor(thing);
        let step = world.step;
        let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) else { return };
        let st = state_mut::<BallsState>(st.as_mut());
        while let Some(&(when, number)) = st.pending.last() {
            if when > step {
                break;
            }
            st.pending.pop();
            push_ball(bodies, st, at, number);
        }
    }

    /// A keyframe moved one of the balls' settings. When it moved the size, every ball grows or
    /// shrinks by as much, keeping its own share (Variety) and what it has grown by on its hits.
    /// Placement and launch speed only matter at the start, so they stay.
    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = BallsSettings::from_params(&resolved.params);
        let ruled = world.ruled(thing, None, "radius");
        let hurts = world.hurts(thing);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<BallsState>(st.as_mut());
        let varies = own_varies(&resolved.params);
        if st.size_varies() || varies.iter().any(|v| v.key == "radius") || ruled.is_some() || st.ruled_size {
            // Each ball has a size of its own: worked out again from the new ends.
            st.settings = settings;
            st.varies = varies;
            st.ruled_size = ruled.is_some();
            if let Some(bodies) = world.bodies.get_mut(&thing) {
                let own = own_sizes(st, bodies, ruled.as_ref(), &hurts);
                resize_to(st, bodies, own);
            }
            return;
        }
        st.varies = varies;
        let (old, new) = (st.settings.radius, settings.radius);
        if new != old
            && let Some(bodies) = world.bodies.get_mut(&thing)
        {
            let sized = |size: f32, factor: f32| if factor == 1.0 { size } else { size * factor };
            for (row, r) in bodies.radius.iter_mut().enumerate() {
                let factor = st.rows.size_factor.get(row).copied().unwrap_or(1.0);
                // A ball still at its own size takes the new one exactly; a grown one keeps its
                // growth in proportion.
                *r = if *r == sized(old, factor) || old <= 0.0 { sized(new, factor) } else { *r * (new / old) };
            }
            st.own_size = st.rows.size_factor.iter().map(|&f| sized(new, f)).collect();
        }
        st.settings = settings;
    }

    /// A Size that varies by the balls' speed or hits follows them every step.
    fn scripted(&self, thing: ThingId, world: &mut World) {
        let Some(st) = world.states.get(&thing) else { return };
        let st = state::<BallsState>(st.as_ref());
        if !st.varies.iter().any(|v| v.key == "radius" && v.by.changes()) {
            return;
        }
        let ruled = world.ruled(thing, None, "radius");
        let hurts = world.hurts(thing);
        let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) else { return };
        let st = state_mut::<BallsState>(st.as_mut());
        let own = own_sizes(st, bodies, ruled.as_ref(), &hurts);
        resize_to(st, bodies, own);
    }

    fn ball_spread(&self, thing: ThingId, world: &World, row: usize) -> Option<BallSpread> {
        spread_in(world, thing, row)
    }

    /// Colour and turn spread only change how the balls look.
    fn affects_physics(&self, key: &str) -> bool {
        !matches!(key, "vary_colour" | "vary_turn")
    }

    /// A rule's ball (Split, Spawn balls): a split one takes the ball it came from's number and
    /// facts; a new one the next number, its Size and the way it goes.
    fn add_ball(&self, thing: ThingId, world: &mut World, ball: &NewBall) -> Option<u32> {
        let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) else {
            return None;
        };
        let st = st.as_any_mut().downcast_mut::<BallsState>()?;
        let fresh = (next_number(&st.offsets, &st.rows), st.settings.radius);
        Some(push_rule_ball(&mut st.rows, &mut st.own_size, bodies, ball, fresh))
    }
}

/// The number a ball a rule adds to a group takes: after every place and every number so far.
fn next_number(offsets: &[Vec2], rows: &BallRows) -> u32 {
    let most = rows.numbers.iter().copied().max().map_or(0, |n| n + 1);
    most.max(offsets.len() as u32)
}

/// Pushes a rule's ball into `bodies` and `rows` (with its own size in `own_size`): a split one
/// (`ball.like`) copies that row's number and facts, a new one takes `fresh`'s number and size.
/// Its row.
pub fn push_rule_ball(
    rows: &mut BallRows,
    own_size: &mut Vec<f32>,
    bodies: &mut Bodies,
    ball: &NewBall,
    fresh: (u32, f32),
) -> u32 {
    let launch = if ball.velocity.length_squared() > 0.0 { ball.velocity.to_angle() } else { 0.0 };
    match ball.like.map(|r| r as usize).filter(|&r| r < rows.len() && r < bodies.len()) {
        Some(from) => {
            let radius = ball.radius.unwrap_or(bodies.radius[from]);
            let row = bodies.push(ball.position, ball.velocity, radius) as usize;
            bodies.number[row] = bodies.number[from];
            let (number, factor, round, place, in_round) =
                (rows.numbers[from], rows.size_factor[from], rows.round[from], rows.place[from], rows.in_round[from]);
            rows.push(number, factor, round, rows.launch[from], place, in_round);
            rows.fade_from[row] = rows.fade_from[from];
            own_size.push(own_size.get(from).copied().unwrap_or(radius));
            row as u32
        }
        None => {
            let (number, size) = fresh;
            let radius = ball.radius.unwrap_or(size);
            let row = bodies.push(ball.position, ball.velocity, radius) as usize;
            bodies.number[row] = number;
            rows.push(number, 1.0, 0, launch, Vec2::ZERO, number);
            own_size.push(radius);
            row as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{BALLS, SHAPE, scene_with_balls};
    use crate::{kit, new_scene};
    use bpe_core::{Address, Attach, Value, resolve};
    use bpe_runtime::{Log, World};
    use glam::Vec2;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn balls_riding_a_spinning_arena_start_with_its_rim_speed() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&SHAPE).unwrap().params.set("spin", Value::Number(90.0));
        scene
            .set_parent(
                BALLS,
                SHAPE,
                Attach::Ride { offset: Vec2::new(0.0, -300.0), turn_with_parent: true, item: None },
            )
            .unwrap();
        scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position[0], Vec2::new(0.0, -300.0));
        let v = w.bodies[&BALLS].velocity[0];
        // The rim at (0, -300) turning clockwise (y down) moves in +x at 300 * pi/2 px/s.
        assert!((v.x - 300.0 * FRAC_PI_2).abs() < 0.5 && v.y.abs() < 1e-3, "{v:?}");
    }

    #[test]
    fn a_turned_block_turns_its_layout() {
        let (mut scene, _, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
            p.set("spacing", Value::Number(10.0));
        });
        scene.things.get_mut(&BALLS).unwrap().transform.rotation = FRAC_PI_2;
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        let p = &w.bodies[&BALLS].position;
        assert!((p[0] - Vec2::new(0.0, -5.0)).length() < 1e-4 && (p[1] - Vec2::new(0.0, 5.0)).length() < 1e-4, "{p:?}");
    }

    #[test]
    fn a_single_ball_sits_at_the_thing_with_its_launch_speed() {
        let (scene, resolved, kit) = scene_with_balls("single", |p| p.set("velocity", Value::Vec2([5.0, 6.0])));
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position, vec![Vec2::ZERO]);
        assert_eq!(w.bodies[&BALLS].velocity, vec![Vec2::new(5.0, 6.0)]);
        assert_eq!(w.bodies[&BALLS].radius, vec![24.0]);
    }

    #[test]
    fn a_size_key_resizes_the_balls() {
        let (mut scene, _, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
        });
        let size = Address { thing: BALLS, slot: None, key: "radius".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(24.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(48.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].radius, vec![24.0, 24.0]);
        w.advance_to(1.5, &kit.sims, &mut Log::default());
        assert_eq!(w.bodies[&BALLS].radius, vec![48.0, 48.0]);
    }

    /// A key on Size leaves Grow per hit growing the balls, and a Size that moves scales what
    /// they grew by with it.
    #[test]
    fn a_size_key_keeps_what_the_balls_grew_by() {
        let (mut scene, _, kit) = scene_with_balls("single", |_| {});
        scene.card_mut(BALLS, bpe_core::SlotId::Bounce).unwrap().params.set("grow", Value::Number(2.0));
        let size = Address { thing: BALLS, slot: None, key: "radius".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(24.0)).unwrap();
        scene.set_key(&kit.registry, &size, 2.0, Value::Number(24.0)).unwrap();
        scene.set_key(&kit.registry, &size, 3.0, Value::Number(48.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(2.0, &kit.sims, &mut log);
        let hits = w.bodies[&BALLS].hits[0];
        assert!(hits > 0, "it hits the shape in 2 s");
        let grown = w.bodies[&BALLS].radius[0];
        assert!(grown > 24.0 + 1.0, "it grew: {grown} after {hits} hits");
        let mut quiet = w.clone();
        // Without hits from here on, doubling the Size doubles the grown ball.
        quiet.states.remove(&SHAPE);
        quiet.walls.clear();
        quiet.advance_to(3.5, &kit.sims, &mut log);
        let r = quiet.bodies[&BALLS].radius[0];
        assert!((r / grown - 2.0).abs() < 1e-3, "{r} against {grown}");
    }

    /// Riding one ball of a group rides the ball with that number, wherever its row is.
    #[test]
    fn riding_an_item_rides_the_ball_of_that_number() {
        let (mut scene, _, kit) = group(12, |p| {
            p.set("release", Value::Choice("one_by_one".into()));
            p.set("release_gap", Value::Number(0.0));
            p.set("release_order", Value::Choice("random".into()));
        });
        let shape = SHAPE;
        scene
            .set_parent(shape, BALLS, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: false, item: Some(5) })
            .unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        let numbers = super::numbers(&w, BALLS).unwrap();
        let row = numbers.iter().position(|&n| n == 5).unwrap();
        assert_ne!(row, 5, "a random order puts ball 5 elsewhere");
        assert_eq!(w.anchor(shape).position, w.bodies[&BALLS].position[row]);
    }

    /// A grid group of `count` balls, at rest unless `f` says otherwise.
    fn group(count: i64, f: impl FnOnce(&mut bpe_core::Params)) -> (bpe_core::Scene, bpe_core::Resolved, crate::Kit) {
        scene_with_balls("single", |p| {
            p.set("group", Value::Bool(true));
            p.set("count", Value::Int(count));
            p.set("columns", Value::Int(4));
            f(p);
        })
    }

    #[test]
    fn group_defaults_leave_every_ball_as_it_was() {
        let (scene, resolved, kit) = group(8, |_| {});
        let w = World::build(&scene, &resolved, &kit.sims);
        let b = &w.bodies[&BALLS];
        assert_eq!(b.position, super::super::shapes::grid(8, 4, 56.0));
        assert!(b.velocity.iter().all(|v| *v == Vec2::new(300.0, -200.0)));
        assert!(b.radius.iter().all(|r| *r == 24.0));
        let st = bpe_runtime::state::<super::BallsState>(w.states[&BALLS].as_ref());
        assert_eq!(st.rows.numbers, (0..8).collect::<Vec<u32>>());
    }

    #[test]
    fn one_by_one_lets_a_ball_go_every_gap_in_order() {
        let (scene, resolved, kit) = group(5, |p| p.set("release", Value::Choice("one_by_one".into())));
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].len(), 1, "the first goes at once");
        let mut log = Log::default();
        let mut counts = Vec::new();
        for _ in 0..100 {
            w.step(&kit.sims, &mut log);
            counts.push(w.bodies[&BALLS].len());
        }
        // A gap of 0.1 s is 24 steps.
        assert_eq!(counts[22], 1);
        assert_eq!(counts[23], 2, "the second goes on step 24");
        assert_eq!(counts[47], 3);
        assert_eq!(counts[99], 5);
        assert_eq!(super::numbers(&w, BALLS).unwrap(), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn centre_out_and_waves_set_who_goes_when() {
        let (scene, resolved, kit) = group(9, |p| {
            p.set("columns", Value::Int(3));
            p.set("release", Value::Choice("waves".into()));
            p.set("wave_size", Value::Int(4));
            p.set("release_order", Value::Choice("centre_out".into()));
        });
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let first = super::numbers(&w, BALLS).unwrap().to_vec();
        assert_eq!(first.len(), 4, "one wave of four");
        assert_eq!(first[0], 4, "the middle of a 3 by 3 grid goes first");
        let mut log = Log::default();
        w.advance_to(0.15, &kit.sims, &mut log);
        assert_eq!(w.bodies[&BALLS].len(), 8);
        w.advance_to(0.25, &kit.sims, &mut log);
        assert_eq!(w.bodies[&BALLS].len(), 9);
        // A wave bigger than the group is one wave.
        let (scene, resolved, kit) = group(5, |p| {
            p.set("release", Value::Choice("waves".into()));
            p.set("wave_size", Value::Int(50));
        });
        assert_eq!(World::build(&scene, &resolved, &kit.sims).bodies[&BALLS].len(), 5);
    }

    #[test]
    fn a_fan_spreads_round_the_launch_and_outward_points_away() {
        let (scene, resolved, kit) = group(5, |p| {
            p.set("launch", Value::Choice("fan".into()));
            p.set("fan_angle", Value::Number(90.0));
            p.set("velocity", Value::Vec2([100.0, 0.0]));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        let v = &w.bodies[&BALLS].velocity;
        let deg = |v: Vec2| v.to_angle().to_degrees();
        assert!((deg(v[0]) + 45.0).abs() < 1e-3 && (deg(v[4]) - 45.0).abs() < 1e-3, "{v:?}");
        assert!(deg(v[2]).abs() < 1e-3, "the middle ball goes the launch way");
        assert!(v.iter().all(|v| (v.length() - 100.0).abs() < 1e-3));
        let (scene, resolved, kit) = group(4, |p| {
            p.set("shape", Value::Choice("ring".into()));
            p.set("launch", Value::Choice("outward".into()));
            p.set("velocity", Value::Vec2([0.0, 50.0]));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        let b = &w.bodies[&BALLS];
        for i in 0..4 {
            assert!(b.velocity[i].normalize().dot(b.position[i].normalize()) > 0.999, "ball {i} goes outward");
            assert!((b.velocity[i].length() - 50.0).abs() < 1e-3);
        }
    }

    #[test]
    fn varied_sizes_are_the_same_every_run_and_survive_a_size_key() {
        let (mut scene, _, kit) = group(6, |p| p.set("vary_size", Value::Number(1.0)));
        // Room to grow: a hit holds a ball to the Max size.
        scene.card_mut(BALLS, bpe_core::SlotId::Bounce).unwrap().params.set("max_size", Value::Number(500.0));
        let size = Address { thing: BALLS, slot: None, key: "radius".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(24.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(48.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let before = w.bodies[&BALLS].radius.clone();
        assert_eq!(before, World::build(&scene, &resolved, &kit.sims).bodies[&BALLS].radius);
        assert!(before.iter().any(|r| *r != 24.0) && before.iter().all(|r| (12.0..=36.0).contains(r)), "{before:?}");
        w.advance_to(1.5, &kit.sims, &mut Log::default());
        let after = &w.bodies[&BALLS].radius;
        for (a, b) in after.iter().zip(&before) {
            assert!((a / b - 2.0).abs() < 1e-5, "each keeps its own share: {a} {b}");
        }
    }

    #[test]
    fn seeking_back_replays_late_balls_the_same() {
        let (scene, resolved, kit) = group(12, |p| {
            p.set("release", Value::Choice("one_by_one".into()));
            p.set("release_order", Value::Choice("random".into()));
            p.set("vary_speed", Value::Number(0.8));
        });
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(2.0);
        let bodies = playback.world().bodies[&BALLS].clone();
        let numbers = super::numbers(playback.world(), BALLS).unwrap().to_vec();
        assert_eq!(bodies.len(), 12);
        playback.seek(0.3);
        playback.seek(2.0);
        assert_eq!(playback.world().bodies[&BALLS], bodies);
        assert_eq!(super::numbers(playback.world(), BALLS).unwrap(), numbers);
        let mut sorted = numbers.clone();
        sorted.sort();
        assert_eq!(sorted, (0..12).collect::<Vec<u32>>());
        assert_ne!(numbers, sorted, "a random order");
    }

    #[test]
    fn a_block_is_laid_out_row_by_row_and_a_ring_clockwise_from_the_top() {
        let (scene, resolved, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(2));
            p.set("spacing", Value::Number(10.0));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(
            w.bodies[&BALLS].position,
            vec![Vec2::new(-5.0, -5.0), Vec2::new(5.0, -5.0), Vec2::new(-5.0, 5.0), Vec2::new(5.0, 5.0)]
        );
        let (scene, resolved, kit) = scene_with_balls("ring", |p| {
            p.set("count", Value::Int(4));
            p.set("ring_radius", Value::Number(10.0));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        let p = &w.bodies[&BALLS].position;
        assert_eq!(p.len(), 4);
        assert!((p[0] - Vec2::new(0.0, -10.0)).length() < 1e-4, "{p:?}");
        assert!((p[1] - Vec2::new(10.0, 0.0)).length() < 1e-4, "{p:?}");
    }

    /// Sets `key` (of the thing, or of a card with `slot`) varying to `to` by `by`.
    fn vary(scene: &mut bpe_core::Scene, slot: Option<bpe_core::SlotId>, key: &str, to: Value, by: &str) {
        let thing = scene.things.get_mut(&BALLS).unwrap();
        let params = match slot {
            None => &mut thing.params,
            Some(slot) => &mut thing.slots.get_mut(&slot).unwrap().params,
        };
        params.set(&format!("{key}~to"), to);
        params.set(&format!("{key}~by"), Value::Choice(by.into()));
    }

    #[test]
    fn a_size_varied_by_number_runs_from_the_value_to_to() {
        let (mut scene, _, kit) = group(4, |p| p.set("radius", Value::Number(10.0)));
        vary(&mut scene, None, "radius", Value::Number(40.0), "number");
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].radius, vec![10.0, 20.0, 30.0, 40.0]);
        // Easing bunches them towards the start.
        scene.things.get_mut(&BALLS).unwrap().params.set("radius~ease", Value::Choice("ease_in".into()));
        let resolved = resolve(&scene, &kit.registry);
        let r = World::build(&scene, &resolved, &kit.sims).bodies[&BALLS].radius.clone();
        assert!(r[1] < 20.0 && r[0] == 10.0 && r[3] == 40.0, "{r:?}");
    }

    #[test]
    fn keys_on_either_end_resize_the_balls() {
        let (mut scene, _, kit) = group(2, |p| {
            p.set("radius", Value::Number(10.0));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        vary(&mut scene, None, "radius", Value::Number(20.0), "number");
        let to = Address { thing: BALLS, slot: None, key: "radius~to".into() };
        scene.set_key(&kit.registry, &to, 0.0, Value::Number(20.0)).unwrap();
        scene.set_key(&kit.registry, &to, 1.0, Value::Number(30.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].radius, vec![10.0, 20.0]);
        w.advance_to(1.5, &kit.sims, &mut Log::default());
        assert_eq!(w.bodies[&BALLS].radius, vec![10.0, 30.0]);
    }

    #[test]
    fn a_launch_speed_varied_by_number_launches_each_ball_its_own_way() {
        let (mut scene, _, kit) = group(3, |p| p.set("velocity", Value::Vec2([100.0, 0.0])));
        vary(&mut scene, None, "velocity", Value::Vec2([300.0, 0.0]), "number");
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        let v = &w.bodies[&BALLS].velocity;
        assert_eq!(v, &vec![Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(300.0, 0.0)]);
    }

    #[test]
    fn bounciness_varied_by_number_gives_each_ball_its_own_bounce() {
        // Four balls falling together onto the floor of the ring; the first keeps all its speed,
        // the last none.
        let (mut scene, _, kit) = group(4, |p| {
            p.set("velocity", Value::Vec2([0.0, 300.0]));
            p.set("space", Value::Number(10.0));
        });
        vary(&mut scene, Some(bpe_core::SlotId::Bounce), "bounciness", Value::Number(0.0), "number");
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(3.0, &kit.sims, &mut log);
        let speeds: Vec<f32> = w.bodies[&BALLS].velocity.iter().map(|v| v.length()).collect();
        assert!(speeds[0] > 250.0, "the first bounces on: {speeds:?}");
        assert!(speeds[3] < 50.0, "the last lies dead: {speeds:?}");
        assert!(speeds[1] > speeds[2], "{speeds:?}");
        // Stepping the same scene again gives the same balls.
        let mut again = World::build(&scene, &resolved, &kit.sims);
        again.advance_to(3.0, &kit.sims, &mut Log::default());
        assert_eq!(again.bodies[&BALLS], w.bodies[&BALLS]);
    }

    #[test]
    fn gravity_varied_by_number_pulls_later_balls_harder() {
        let (mut scene, _, kit) = group(3, |p| p.set("velocity", Value::Vec2([0.0, 0.0])));
        scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 500.0]));
        let motion = scene.card_mut(BALLS, bpe_core::SlotId::Motion).unwrap();
        motion.params.set("gravity_scale", Value::Number(0.0));
        vary(&mut scene, Some(bpe_core::SlotId::Motion), "gravity_scale", Value::Number(2.0), "number");
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let start = w.bodies[&BALLS].position.clone();
        w.advance_to(0.2, &kit.sims, &mut Log::default());
        let fell: Vec<f32> = w.bodies[&BALLS].position.iter().zip(&start).map(|(p, s)| p.y - s.y).collect();
        assert!(fell[0].abs() < 1e-3, "no pull on the first: {fell:?}");
        assert!((fell[2] - 2.0 * fell[1]).abs() < 0.5, "twice the pull on the last: {fell:?}");
    }

    #[test]
    fn a_size_varied_by_hits_grows_as_the_balls_hit() {
        let (mut scene, _, kit) = scene_with_balls("single", |p| p.set("radius", Value::Number(10.0)));
        scene.things.get_mut(&BALLS).unwrap().params.set("group", Value::Bool(true));
        scene.things.get_mut(&BALLS).unwrap().params.set("count", Value::Int(1));
        vary(&mut scene, None, "radius", Value::Number(30.0), "hits");
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].radius, vec![10.0]);
        let mut log = Log::default();
        w.advance_to(6.0, &kit.sims, &mut log);
        let hits = w.bodies[&BALLS].hits[0];
        assert!(hits >= 2, "{hits}");
        let r = w.bodies[&BALLS].radius[0];
        let want = 10.0 + 20.0 * (hits.min(10) as f32 / 10.0);
        assert!((r - want).abs() < 1.5, "{r} against {want} after {hits} hits");
    }
}
