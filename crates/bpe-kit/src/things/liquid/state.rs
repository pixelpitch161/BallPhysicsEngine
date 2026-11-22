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
use super::cards::{FillSettings, Forces, LookSettings, MeetsSettings, OtherLiquid, PourLayer};

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
    /// How many times the scene's gravity every liquid falls at: the first Liquid thing's Size
    /// (one for all, as Detail: liquids of different sizes sharing a body would fall at different
    /// gravities and layer the wrong way up).
    pub size: f32,
    /// Liquid steps taken.
    pub step: u32,
    /// The walls the band was last worked out from.
    pub walls_seen: BTreeMap<ThingId, Wall>,
    /// The band worked out from them, for a copied body to put on again without working it out.
    pub band: Option<Arc<Vec<WallNode>>>,
    /// In the main world: what the balls felt in the last liquid steps, on its way back, and the
    /// balls it is for (part 2). Shared by every copy of the world, so all read the same.
    pub pending: Option<(Arc<bpe_gfx::liquid_sim::Feel>, Arc<super::feel::Felt>)>,
    /// Which balls are in which liquid (thing, row, liquid number), and which touch it.
    pub inside: std::collections::BTreeSet<(ThingId, u32, u8)>,
    pub touching: std::collections::BTreeSet<(ThingId, u32, u8)>,
    /// The liquids' numbers on the card and their things, as last felt.
    pub numbers: Vec<(usize, ThingId)>,
    /// Events worked out from the feel, for the next after-step to log.
    pub news: Vec<bpe_runtime::Event>,
    /// The colour each ball has taken from the liquid (linear rgb, and how much).
    pub taken: Arc<BTreeMap<(ThingId, u32), [f32; 4]>>,
    /// Un-mix (part 4): liquid steps a rule still asks it to play back, and whether it is playing
    /// back now.
    pub unmix_left: u32,
    pub rewinding: bool,
    /// Each ball's size when a liquid that changes balls first touched it.
    pub sizes: Arc<BTreeMap<(ThingId, u32), f32>>,
    /// Balls dissolved, for the next after-step to remove (with the log).
    pub dissolved: Vec<(ThingId, u32)>,
    /// How each ball the last feel found touching the liquid is wet (liquid part 5).
    pub wet: Arc<BTreeMap<(ThingId, u32), super::sound::Wet>>,
    /// Every liquid's stat words from the last feel, by its number on the card (part 5).
    pub sums: Option<Arc<Vec<u32>>>,
    /// Whether some sound in the scene hears a liquid (`scene_hears_liquid`, read as the world is
    /// built): the feel then goes back with no balls too, for the liquids' sums (part 5).
    pub heard: bool,
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

    /// What the status line says of the liquid.
    pub fn reading(&self) -> Reading {
        Reading {
            drops: self.count_hint(),
            step_ms: self.step_ms(),
            substeps: self.body.as_ref().map_or(0, |b| b.substeps()),
            on_card: self.body.is_some(),
        }
    }
}

/// What the status line says of the liquid (faster liquid).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// How many drops the card last said there were.
    pub drops: u32,
    /// How long the last liquid step took, in milliseconds: on the card itself where the card can
    /// time its own work, else from when the step was sent to when its count came back.
    pub step_ms: f32,
    /// How many small steps that liquid step took (1 to 64: more the faster the liquid moves and
    /// the finer its Detail).
    pub substeps: u32,
    /// Whether there is a graphics card to move the liquid on.
    pub on_card: bool,
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
    /// Its Fill card's Heat, Drain, Current and Magnet layers.
    pub forces: Forces,
    /// The rain's share of a raindrop still owed.
    pub rain_owed: f32,
    /// The balls it is made from (From balls) as last seen: place, speed, size.
    pub seen: BTreeMap<(ThingId, u32), (Vec2, Vec2, f32)>,
    /// Which of its moments its sound logs (part 5).
    pub sound: super::sound::LiquidSound,
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
            forces: Forces::default(),
            rain_owed: 0.0,
            seen: BTreeMap::new(),
            sound: Default::default(),
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
        self.host.as_ref().and_then(|h| h.body.as_ref()).map_or(0, |b| b.bytes() + b.history_bytes())
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
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        st.settings,
        st.fill,
        st.pours,
        st.meets,
        st.others,
        st.look.stepped(),
        st.forces,
        st.sound
    );
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}
