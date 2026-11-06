//! A ball Look whose layer settings vary across the balls (see [`bpe_core::vary`]): each ball
//! wears the stack worked out at its own place in the spreads. Balls whose places fall in the same
//! of [`bpe_core::vary::VARY_LEVELS`] steps share a stack, so a frame works a look out at most once
//! per step, however many balls wear it; nothing is kept from one frame to the next.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use bpe_core::vary::{self, BallSpread, Vary, VaryUse};
use bpe_core::{LayerId, ResolvedSlot, SettingDef};
use bpe_gfx::DrawCx;

use super::around::{AroundLayer, around_layers};
use super::gpu::Stack;
use super::particles::PARTICLES;
use super::trail::TRAIL;
use super::{BLEND, GROUP_GRADIENT, GROUP_SHADER};

/// Trail settings that set how far back the whole layer reads the balls' past: the same for every
/// ball.
const TRAIL_WHOLE: [&str; 6] = ["seconds", "pixels", "faster", "ghosts", "ghost_every", "blend_amount"];
/// Particle settings that set how far back the whole layer looks, or how it is mixed in.
const PARTICLES_WHOLE: [&str; 2] = ["life", "blend_amount"];

/// Whether setting `def` of ball layer kind `kind` can vary across the balls: every number,
/// pair, colour and gradient, but those of shape layers, of the group layers (which spread across
/// the group already), of Blend (which mixes the whole thing) and the trail and particle settings
/// that the whole layer shares.
pub fn layer_vary(kind: &str, def: &SettingDef) -> Option<VaryUse> {
    (vary::can_vary_type(&def.ty) && !whole(kind, def.key)).then_some(VaryUse::Live)
}

/// Whether layer kind `kind`'s setting `key` is one for the whole thing, never a ball's own.
fn whole(kind: &str, key: &str) -> bool {
    match kind {
        GROUP_GRADIENT | GROUP_SHADER | BLEND => true,
        TRAIL => TRAIL_WHOLE.contains(&key),
        PARTICLES => PARTICLES_WHOLE.contains(&key),
        k => k.starts_with("shape."),
    }
}

/// [`spread_of`], with the ball's light and shadow from the frame's light probe when `lit` (some
/// varied setting spreads by Light or Shadow) and the frame has one.
pub fn spread_lit(cx: &DrawCx, row: usize, lit: bool) -> BallSpread {
    let mut spread = spread_of(cx, row);
    if lit && let Some(light) = cx.light {
        let b = light.ball(cx.world, cx.log, cx.thing.id, row, cx.time);
        (spread.light, spread.shadow) = (b.light, b.shadow);
    }
    spread
}

/// Where body row `row` of the thing being drawn sits in the spreads, as it moves now.
pub fn spread_of(cx: &DrawCx, row: usize) -> BallSpread {
    // How much of its health is gone (rules part 4): 0 without a Health card.
    let hurt = cx.world.hurt(cx.thing.id, row);
    if let Some(s) = crate::things::balls::sim::spread_in(cx.world, cx.thing.id, row)
        .or_else(|| crate::things::emitter::sim::spread_in(cx.world, cx.thing.id, row))
    {
        return BallSpread { hurt, ..s };
    }
    let bodies = cx.world.bodies.get(&cx.thing.id);
    BallSpread {
        hurt,
        number: bodies.and_then(|b| b.number.get(row).copied()).unwrap_or(row as u32),
        count: bodies.map_or(1, |b| b.len() as u32),
        speed: bodies.and_then(|b| b.velocity.get(row)).map_or(0.0, |v| v.length()),
        hits: bodies.and_then(|b| b.hits.get(row).copied()).unwrap_or(0),
        seed: cx.world.seed as u32,
        ..BallSpread::default()
    }
}

/// One group of the look's varied settings, with what its balls share per step.
struct Part<T> {
    varies: Vec<Vary>,
    salts: Vec<u32>,
    made: RefCell<HashMap<Vec<u16>, Rc<T>>>,
}

impl<T> Part<T> {
    fn new(varies: Vec<Vary>) -> Self {
        Part { salts: varies.iter().map(Vary::salt).collect(), varies, made: RefCell::new(HashMap::new()) }
    }

    /// Every varied setting with its salt.
    fn varies_with_salts(&self) -> Vec<(&Vary, u32)> {
        self.varies.iter().zip(self.salts.iter().copied()).collect()
    }

    /// The steps a ball at `spread` is at, one per varied setting (empty when nothing varies).
    fn key(&self, spread: &BallSpread) -> Vec<u16> {
        self.varies.iter().zip(&self.salts).map(|(v, &s)| vary::level(spread.t(v.by, s))).collect()
    }

    /// What a ball at `key` wears, made by `make` from the look's settings at those steps the
    /// first time it is asked.
    fn get(&self, slot: &ResolvedSlot, key: Vec<u16>, make: impl FnOnce(&ResolvedSlot) -> T) -> Rc<T> {
        if let Some(t) = self.made.borrow().get(&key) {
            return t.clone();
        }
        let ts: Vec<f32> = key.iter().map(|&q| vary::at_level(q)).collect();
        let own =
            ResolvedSlot { params: vary::with_varies(&slot.params, &self.varies, &ts, &|_| None), ..slot.clone() };
        let made = Rc::new(make(&own));
        self.made.borrow_mut().insert(key, made.clone());
        made
    }
}

/// A thing's ball Look, with the stack and the trails and particles each ball wears.
pub struct VariedLook<'a> {
    slot: &'a ResolvedSlot,
    /// The stack as the card says, for balls whose look doesn't vary.
    pub base: Rc<Stack>,
    stack: Part<Stack>,
    around: Part<Vec<AroundLayer>>,
}

impl<'a> VariedLook<'a> {
    /// The look of a `look.layers` card `slot`; `None` for any other card.
    pub fn of(slot: &'a ResolvedSlot) -> Option<VariedLook<'a>> {
        let base = Rc::new(Stack::of(slot)?);
        let (mut on, mut round) = (Vec::new(), Vec::new());
        for v in vary::varies_in(&slot.params).into_iter().filter(|v| v.to.is_some()) {
            let Some((id, key)) = bpe_core::split_layered(&v.key) else { continue };
            match layer_place(slot, id, key) {
                Some(Place::Stack) => on.push(v),
                Some(Place::Around) => round.push(v),
                None => {}
            }
        }
        Some(VariedLook { slot, base, stack: Part::new(on), around: Part::new(round) })
    }

    /// Whether some varied setting spreads by Light or Shadow.
    pub fn needs_light(&self) -> bool {
        self.stack.varies.iter().chain(&self.around.varies).any(|v| v.by.reads_light())
    }

    /// Whether every ball wears the same stack.
    pub fn plain(&self) -> bool {
        self.stack.varies.is_empty()
    }

    /// Whether every ball wears the same trails and particles.
    pub fn plain_around(&self) -> bool {
        self.around.varies.is_empty()
    }

    /// The step of each varied stack setting a ball at `spread` is at (empty when none vary).
    pub fn key(&self, spread: &BallSpread) -> Vec<u16> {
        self.stack.key(spread)
    }

    /// The step of each varied trail and particle setting a ball at `spread` is at.
    pub fn around_key(&self, spread: &BallSpread) -> Vec<u16> {
        self.around.key(spread)
    }

    /// The stack a ball at `key` wears.
    pub fn stack_at(&self, key: &[u16]) -> Rc<Stack> {
        if key.is_empty() {
            return self.base.clone();
        }
        let base = &self.base;
        self.stack.get(self.slot, key.to_vec(), |s| Stack::of(s).unwrap_or_else(|| (**base).clone()))
    }

    /// The stack a ball at `spread` wears.
    pub fn stack_for(&self, spread: &BallSpread) -> Rc<Stack> {
        self.stack_at(&self.key(spread))
    }

    /// The trails and particles (and Ghosts) a ball at `spread` wears, bottom first.
    pub fn around_for(&self, spread: &BallSpread) -> Rc<Vec<AroundLayer>> {
        let key = self.around.key(spread);
        self.around.get(self.slot, key, |s| around_layers(s, false))
    }

    /// The look settings of a ball at `spread` with every varied setting at its step, then
    /// `change` on top: a rule's changes to this one ball (rules part 1).
    pub fn own_slot(&self, spread: &BallSpread, change: impl FnOnce(&mut bpe_core::Params)) -> ResolvedSlot {
        let (mut varies, mut ts) = (Vec::new(), Vec::new());
        for part in [&self.stack.varies_with_salts(), &self.around.varies_with_salts()] {
            for (v, salt) in part.iter() {
                ts.push(vary::at_level(vary::level(spread.t(v.by, *salt))));
                varies.push((*v).clone());
            }
        }
        let mut params = vary::with_varies(&self.slot.params, &varies, &ts, &|_| None);
        change(&mut params);
        ResolvedSlot { params, ..self.slot.clone() }
    }

    /// The stack a ball with its own settings `own` (from [`VariedLook::own_slot`]) wears.
    pub fn own_stack(&self, own: &ResolvedSlot) -> Rc<Stack> {
        Rc::new(Stack::of(own).unwrap_or_else(|| (*self.base).clone()))
    }

    /// The trails and particles a ball with its own settings `own` wears.
    pub fn own_around(&self, own: &ResolvedSlot) -> Rc<Vec<AroundLayer>> {
        Rc::new(around_layers(own, false))
    }
}

/// What a varied layer setting changes: the stack on the ball, or its trails and particles.
enum Place {
    Stack,
    Around,
}

/// Where layer `id`'s setting `key` acts, when it can vary.
fn layer_place(slot: &ResolvedSlot, id: LayerId, key: &str) -> Option<Place> {
    let layer = slot.layers.iter().find(|l| l.id == id)?;
    if whole(layer.kind.as_str(), key) {
        return None;
    }
    // A sub-layer acts where its layer does.
    let top = match layer.parent {
        Some(p) => slot.layers.iter().find(|l| l.id == p)?,
        None => layer,
    };
    Some(if matches!(top.kind.as_str(), TRAIL | PARTICLES) { Place::Around } else { Place::Stack })
}
