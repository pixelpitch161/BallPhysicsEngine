//! What a Liquid thing keeps between steps: its settings as read, its births' counters and, in
//! the first Liquid thing of the world (the host), the body of drops on the graphics card that
//! every Liquid thing shares.

use std::collections::BTreeMap;
use std::sync::Arc;

use bpe_core::ThingId;
use bpe_gfx::liquid_sim::{Body, Drop, WallNode};
use bpe_runtime::{Wall, World};
use glam::Vec2;

use super::LiquidSettings;
use super::cards::{FillSettings, LookSettings, MeetsSettings, OtherLiquid, PourLayer};

/// Where the grid sits in the world: node `(x, y)` is at `origin + (x, y) · cell`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub origin: Vec2,
    /// Pixels a cell.
    pub cell: f32,
    pub cols: u32,
    pub rows: u32,
}

impl Layout {
    /// The grid point at world point `p`.
    pub fn to_grid(&self, p: Vec2) -> Vec2 {
        (p - self.origin) / self.cell
    }

    /// The world point at grid point `g`.
    pub fn to_world(&self, g: Vec2) -> Vec2 {
        self.origin + g * self.cell
    }

    /// The part of the world drops can be in: two cells in from every edge of the grid.
    pub fn inner(&self) -> (Vec2, Vec2) {
        (self.to_world(Vec2::splat(2.0)), self.to_world(Vec2::new(self.cols as f32 - 2.0, self.rows as f32 - 2.0)))
    }
}

/// What the first Liquid thing of a world keeps for all of them.
#[derive(Clone, Debug)]
pub struct Host {
    /// The drops on the card: `None` where the thread its world was built on named no card. Clones
    /// share it until one of them steps, which copies it on the card first.
    pub body: Option<Arc<Body>>,
    pub layout: Layout,
    /// Liquid steps a second.
    pub rate: u32,
    /// Liquid steps taken.
    pub step: u32,
    /// The walls the band was last worked out from.
    pub walls_seen: BTreeMap<ThingId, Wall>,
    /// The band worked out from them, for a copied body to put on again without working it out.
    pub band: Option<Arc<Vec<WallNode>>>,
}

impl Host {
    /// How many drops the card last said there were (0 without a card).
    pub fn count_hint(&self) -> u32 {
        self.body.as_ref().map_or(0, |b| b.count_hint())
    }

    /// How long the card's last liquid step took, in milliseconds.
    pub fn step_ms(&self) -> f32 {
        self.body.as_ref().map_or(0.0, |b| b.step_ms())
    }
}

/// A Liquid thing's state.
#[derive(Clone, Debug)]
pub struct LiquidState {
    pub id: ThingId,
    pub parent: Option<ThingId>,
    pub settings: LiquidSettings,
    pub fill: FillSettings,
    pub pours: Vec<PourLayer>,
    pub meets: MeetsSettings,
    pub others: Vec<OtherLiquid>,
    pub look: LookSettings,
    /// Its liquid's number in the body (0 to 15), `None` past the 16th Liquid thing.
    pub index: Option<usize>,
    /// Drops born so far (the next one's serial).
    pub born: u32,
    /// Each pour's share of a drop still owed.
    pub owed: Vec<f32>,
    /// How full it has been filled (its Level as last filled), 0 before it fills.
    pub filled: f32,
    /// The Level fell this step: drops above the new level go.
    pub cut: Option<f32>,
    /// The most drops it keeps now: its Most drops, or its share of [`MOST_KEPT`] when every
    /// liquid's Most drops add up to more ([`share_most`]).
    pub kept: u32,
    /// The first Liquid thing's: the body every one shares.
    pub host: Option<Host>,
}

impl LiquidState {
    pub fn new(id: ThingId, parent: Option<ThingId>) -> LiquidState {
        LiquidState {
            id,
            parent,
            settings: LiquidSettings::default(),
            fill: FillSettings::default(),
            pours: Vec::new(),
            meets: MeetsSettings::default(),
            others: Vec::new(),
            look: LookSettings::default(),
            index: None,
            born: 0,
            owed: Vec::new(),
            filled: 0.0,
            cut: None,
            kept: 0,
            host: None,
        }
    }

    /// Bytes its body takes on the card.
    pub fn gpu_memory(&self) -> usize {
        self.host.as_ref().and_then(|h| h.body.as_ref()).map_or(0, |b| b.bytes())
    }
}

/// The most drops every liquid keeps between them, and the most born in one liquid step: together
/// under [`bpe_gfx::liquid_sim::MOST_DROPS`], so the card never runs out of room and how many are
/// kept never depends on when the card's count came back.
pub const MOST_KEPT: u32 = 800_000;
pub const MOST_BORN_A_STEP: usize = 200_000;

/// Sets each liquid's `kept`: its Most drops, scaled down evenly when they add up to more than
/// [`MOST_KEPT`].
pub fn share_most<'a>(states: impl IntoIterator<Item = &'a mut LiquidState>) {
    let mut all: Vec<&mut LiquidState> = states.into_iter().filter(|s| s.index.is_some()).collect();
    let sum: u64 = all.iter().map(|s| s.fill.most as u64).sum();
    let scale = if sum > MOST_KEPT as u64 { MOST_KEPT as f64 / sum as f64 } else { 1.0 };
    for s in all.iter_mut() {
        s.kept = ((s.fill.most as f64 * scale) as u32).max(1);
    }
}

/// The thing's Liquid state as the world has it now.
pub fn state_of(world: &World, id: ThingId) -> Option<&LiquidState> {
    (**world.states.get(&id)?).as_any().downcast_ref::<LiquidState>()
}

/// The host of the world's liquids (the first Liquid thing's state that has one).
pub fn host_of(world: &World) -> Option<&LiquidState> {
    world.order().iter().filter_map(|t| state_of(world, *t)).find(|s| s.host.is_some())
}

fn put(h: &mut u64, x: u32) {
    for b in x.to_le_bytes() {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

/// The hash of every drop's place, velocity and colour, read back from the card (the goldens');
/// 0 for a state that isn't the host, or has no card.
pub fn hash_of(st: &LiquidState) -> u64 {
    let Some(body) = st.host.as_ref().and_then(|h| h.body.as_ref()) else { return 0 };
    drops_hash(&body.read())
}

/// The hash of `drops`.
pub fn drops_hash(drops: &[Drop]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    put(&mut h, drops.len() as u32);
    for d in drops {
        for x in d.x.iter().chain(&d.v).chain(&d.colour) {
            put(&mut h, x.to_bits());
        }
        put(&mut h, d.kind);
        put(&mut h, d.serial);
    }
    h
}

/// The hash of everything the thing's stepping reads of its settings and cards, as read now:
/// what a restart test compares (cheap, unlike reading the drops back).
pub fn inputs_hash(st: &LiquidState) -> u64 {
    let text = format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        st.settings,
        st.fill,
        st.pours,
        st.meets,
        st.others,
        st.look.stepped()
    );
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}
