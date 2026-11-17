//! What the balls feel from the liquid (liquid part 2). In the main world each batch of liquid
//! steps records, per ball it met, the push its drops gave it, how much liquid touched it and of
//! what colour, and how much of a ring just outside it is wet ([`bpe_gfx::liquid_sim::Feel`]). The
//! next world step's first substep takes it ([`apply`]): the balls get the push, the drag and the
//! carry once, the balls' taken colours move, and Enter liquid / Leave liquid / Splash events are
//! worked out for the next after-step to log. Everything here is worked out from integers the card
//! summed, so it is the same on every play.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bpe_core::ThingId;
use bpe_gfx::liquid_sim::{FEEL_WORDS, MOST_LIQUIDS, RING, S_PUSH, S_TOUCH, feel_at};
use bpe_runtime::{Event, EventKind, Party, World};
use glam::Vec2;

use super::state::{Host, LiquidState};

/// Under this share of a ball's ring in a liquid, a ball in it has left it.
pub const OUT_AT: f32 = 0.15;
/// Over this share, a ball out of it has gone in: about its middle under (a ball floating as
/// high as a ball half as heavy as the liquid is in it).
pub const IN_AT: f32 = 0.35;
/// How fast a ball takes the liquid's colour, a second, at Balls take its colour 1 and fully under.
pub const TAKE_RATE: f32 = 3.0;

/// What a Liquid event's party `b` carries as its item.
pub const ENTER: u32 = 0;
pub const LEAVE: u32 = 1;
pub const SPLASH: u32 = 2;

/// The balls a batch of liquid steps was told about, in the order the card numbered them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Felt {
    /// Each ball: its thing and body row.
    pub balls: Vec<(ThingId, u32)>,
    /// Each liquid's thing and how fast a ball must go in to splash (px/s), by liquid number.
    pub liquids: Vec<(ThingId, f32)>,
    /// Pixels a grid cell.
    pub cell: f32,
    /// The seconds of liquid the batch covered.
    pub span: f32,
}

fn word(words: &[i32], k: usize) -> f32 {
    words.get(k).copied().unwrap_or(0) as f32
}

fn float(words: &[i32], k: usize) -> f32 {
    let x = f32::from_bits(words.get(k).copied().unwrap_or(0) as u32);
    if x.is_finite() { x } else { 0.0 }
}

/// `v` no longer than `most`; zero when not finite.
fn clamp_len(v: Vec2, most: f32) -> Vec2 {
    if !v.is_finite() {
        return Vec2::ZERO;
    }
    let l = v.length();
    if l > most && l > 0.0 { v * (most / l) } else { v }
}

/// One liquid event at the world's step.
fn event(world: &World, ball: (ThingId, u32), liquid: ThingId, code: u32, place: Vec2, v: Vec2) -> Event {
    Event {
        step: world.step,
        kind: EventKind::Liquid,
        a: Party { thing: ball.0, item: ball.1 },
        b: Some(Party { thing: liquid, item: code }),
        point: place,
        normal: v.normalize_or_zero(),
        speed: v.length(),
        at: 0.0,
    }
}

/// Gives the balls what `host` felt in its last batch (waiting for the card when it isn't back
/// yet), and works out the events and taken colours. Nothing when nothing is on its way.
pub fn apply(host: &mut Host, world: &mut World) {
    let Some((feel, felt)) = host.pending.take() else { return };
    let words = feel.wait();
    let ceiling = world.limits().ceiling;
    let cell = felt.cell.max(1e-3);
    let span = felt.span.max(0.0);
    let mut inside: BTreeSet<(ThingId, u32, u8)> = BTreeSet::new();
    let mut touching: BTreeSet<(ThingId, u32, u8)> = BTreeSet::new();
    let mut news = Vec::new();
    let mut taken: Option<BTreeMap<(ThingId, u32), [f32; 4]>> = None;
    for (i, &(thing, row)) in felt.balls.iter().enumerate() {
        let Some(w) = words.get(i * FEEL_WORDS..(i + 1) * FEEL_WORDS) else { break };
        let Some(bodies) = world.bodies.get_mut(&thing) else { continue };
        let r = row as usize;
        if r >= bodies.len() || !bodies.alive[r] {
            continue;
        }
        let (place, v) = (bodies.position[r], bodies.velocity[r]);
        let radius = (bodies.radius[r] / cell).max(0.05);
        let area = std::f32::consts::PI * radius * radius;
        let push = Vec2::new(word(w, feel_at::PUSH), word(w, feel_at::PUSH + 1)) / S_PUSH / area * cell;
        let touch = word(w, feel_at::TOUCH) / S_TOUCH;
        let wet = word(w, feel_at::WET) / RING as f32;
        let mut dv = clamp_len(push, ceiling);
        if touch > 0.0 {
            let drag = word(w, feel_at::DRAG) / S_TOUCH / touch;
            let carry = word(w, feel_at::CARRY) / S_TOUCH / touch;
            let flow = Vec2::new(float(w, feel_at::FLOW), float(w, feel_at::FLOW + 1)) * cell;
            let k = (drag * wet * span).max(0.0);
            dv += clamp_len(-(v - carry * flow) * (1.0 - (-k).exp()), ceiling);
        }
        if !bodies.held[r] {
            let after = v + dv;
            if after.is_finite() {
                bodies.velocity[r] = clamp_len(after, ceiling);
            }
        }
        // Which liquids it is in: each liquid's share of the wet ring by its share of the contacts.
        let counts: Vec<f32> = (0..MOST_LIQUIDS).map(|l| word(w, feel_at::LIQUIDS + l).max(0.0)).collect();
        let all: f32 = counts.iter().sum();
        for (l, &(liquid, splash_from)) in felt.liquids.iter().enumerate().take(MOST_LIQUIDS) {
            let share = if all > 0.0 { wet * counts[l] / all } else { 0.0 };
            let key = (thing, row, l as u8);
            let was = host.inside.contains(&key);
            if (was && share >= OUT_AT) || (!was && share >= IN_AT) {
                inside.insert(key);
            }
            if !was && share >= IN_AT {
                news.push(event(world, (thing, row), liquid, ENTER, place, v));
            }
            // It splashes as it first touches the liquid, fast enough.
            if share > 0.0 {
                touching.insert(key);
                if !host.touching.contains(&key) && !was && v.length() >= splash_from {
                    news.push(event(world, (thing, row), liquid, SPLASH, place, v));
                }
            }
        }
        // Its colour moves to the colour of the liquid touching it.
        let take = if touch > 0.0 { word(w, feel_at::TAKE) / S_TOUCH / touch } else { 0.0 };
        if take > 0.0 && wet > 0.0 {
            let colour = [0, 1, 2].map(|c| word(w, feel_at::COLOUR + c) / S_TOUCH / touch);
            if colour.iter().all(|c| c.is_finite()) {
                let t = 1.0 - (-TAKE_RATE * take * wet * span).exp();
                let map = taken.get_or_insert_with(|| (*host.taken).clone());
                let was = map.get(&(thing, row)).copied().unwrap_or([0.0; 4]);
                let mixed = if was[3] <= 0.0 { colour } else { [0, 1, 2].map(|c| was[c] + (colour[c] - was[c]) * t) };
                map.insert((thing, row), [mixed[0], mixed[1], mixed[2], was[3] + (1.0 - was[3]) * t]);
            }
        }
    }
    // Balls that were in a liquid and aren't now (out of it, or no longer met), while still there.
    for &(thing, row, l) in host.inside.difference(&inside) {
        let Some(&(liquid, _)) = felt.liquids.get(l as usize) else { continue };
        let Some(bodies) = world.bodies.get(&thing) else { continue };
        let r = row as usize;
        if r < bodies.len() && bodies.alive[r] {
            news.push(event(world, (thing, row), liquid, LEAVE, bodies.position[r], bodies.velocity[r]));
        }
    }
    // In ball, then liquid, then kind order whatever order they were found in.
    news.sort_by_key(|e| (e.a, e.b));
    host.inside = inside;
    host.touching = touching;
    host.news.extend(news);
    if let Some(map) = taken {
        host.taken = Arc::new(map);
    }
}

/// The colours the balls of `thing` took from the world's liquid (linear rgb and how much, by
/// body row), when some did.
pub fn taken_colours(world: &World, thing: ThingId) -> Option<BTreeMap<u32, [f32; 4]>> {
    let host = super::state::host_of(world)?.host.as_ref()?;
    let of: BTreeMap<u32, [f32; 4]> =
        host.taken.range((thing, 0)..=(thing, u32::MAX)).map(|(&(_, row), c)| (row, *c)).collect();
    (!of.is_empty()).then_some(of)
}

/// Whether `st` (a Liquid thing's state) is the host of a liquid that steps with the physics.
pub fn in_main_world(world: &World, st: &LiquidState) -> bool {
    st.host.is_some() && !world.beside.contains(&st.id)
}
