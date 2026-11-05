//! What every thing with ball rows (a Balls thing, an Emitter) keeps about each row beyond the
//! world's `Bodies`, and what the drawing and the sound read about such a thing's balls.

use bpe_core::{Params, Settings, ThingId};
use bpe_runtime::World;
use glam::Vec2;

use super::BallsSettings;
use super::sim::BallsState;
use crate::things::emitter::sim::EmitterState;

/// Each ball row's own facts, in row order (one entry per body row).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BallRows {
    /// Each row's ball number: its place in a group's shape, or its place in an emitter's firing
    /// order across every round.
    pub numbers: Vec<u32>,
    /// Each row's size, as a share of the Size setting (Variety).
    pub size_factor: Vec<f32>,
    /// The emitter round each row was fired in, from 1; 0 for a Balls thing's balls.
    pub round: Vec<u32>,
    /// The direction each row was fired or launched in, in radians clockwise from +x.
    pub launch: Vec<f32>,
    /// Each row's place in its group, from the thing: where a Balls group lays it out, or where
    /// one round of an emitter lays it out. What the group gradient's Place and Centre out read.
    pub place: Vec<Vec2>,
    /// How many times each row has been caught: its look's hit reactions count these as hits.
    pub arrivals: Vec<u32>,
    /// The step each row began fading out at (an emitter's Earlier rounds: Fade out).
    pub fade_from: Vec<Option<u64>>,
    /// Each row's place round its round: an emitter's ball's place in its round's directions, a
    /// group's ball's number. What a wavefront goes through the balls in.
    pub in_round: Vec<u32>,
}

impl BallRows {
    /// Adds a row.
    pub fn push(&mut self, number: u32, size_factor: f32, round: u32, launch: f32, place: Vec2, in_round: u32) {
        self.numbers.push(number);
        self.in_round.push(in_round);
        self.size_factor.push(size_factor);
        self.round.push(round);
        self.launch.push(launch);
        self.place.push(place);
        self.arrivals.push(0);
        self.fade_from.push(None);
    }

    /// The ball number of row `row` (the row itself for anything not listed).
    pub fn number(&self, row: usize) -> u32 {
        self.numbers.get(row).copied().unwrap_or(row as u32)
    }

    pub fn len(&self) -> usize {
        self.numbers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.numbers.is_empty()
    }

    /// Bytes the rows take, for playback budgets.
    pub fn memory(&self) -> usize {
        self.numbers.capacity() * 4 * 4 + self.place.capacity() * 8 + self.fade_from.capacity() * 16
    }
}

/// The rows of `thing`, when it is a thing with ball rows in `world`.
pub fn rows(world: &World, thing: ThingId) -> Option<&BallRows> {
    let st = world.states.get(&thing)?.as_any();
    st.downcast_ref::<BallsState>().map(|s| &s.rows).or_else(|| st.downcast_ref::<EmitterState>().map(|s| &s.rows))
}

/// [`rows`], to change.
pub fn rows_mut(world: &mut World, thing: ThingId) -> Option<&mut BallRows> {
    let st = world.states.get_mut(&thing)?.as_any_mut();
    if st.is::<BallsState>() {
        return st.downcast_mut::<BallsState>().map(|s| &mut s.rows);
    }
    st.downcast_mut::<EmitterState>().map(|s| &mut s.rows)
}

/// How far the group gradient's Round spread reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundSpan {
    /// So many rounds, first to last.
    Finite(u32),
    /// Rounds without end: the gradient comes round every ten.
    Endless,
}

impl Default for RoundSpan {
    fn default() -> Self {
        RoundSpan::Finite(1)
    }
}

/// What the drawing reads about the whole group of a thing with ball rows.
#[derive(Clone, Copy, Debug)]
pub struct GroupOf<'a> {
    pub rows: &'a BallRows,
    /// The group's places (what the gradient spreads Place and Centre out over).
    pub places: &'a [Vec2],
    /// How many balls the group counts for Order: a Balls group's places, an emitter's planned
    /// balls (or those fired so far when it can't know).
    pub count: usize,
    pub seed: i32,
    pub rounds: RoundSpan,
    /// How many steps a fading ball takes to fade out (an emitter's Fade time); 0 for none.
    pub fade_steps: u64,
    /// Whether the thing is an emitter.
    pub emitter: bool,
}

impl GroupOf<'_> {
    /// How much of row `row` shows at step `now` (fractional): 1 unless it is fading out.
    pub fn fade(&self, row: usize, now: f64) -> f32 {
        let Some(Some(from)) = self.rows.fade_from.get(row) else { return 1.0 };
        if self.fade_steps == 0 {
            return 0.0;
        }
        (1.0 - ((now - *from as f64) / self.fade_steps as f64) as f32).clamp(0.0, 1.0)
    }
}

/// The group of `thing`, when it is a thing with ball rows in `world`.
pub fn group_of(world: &World, thing: ThingId) -> Option<GroupOf<'_>> {
    let st = world.states.get(&thing)?.as_any();
    if let Some(s) = st.downcast_ref::<BallsState>() {
        return Some(GroupOf {
            rows: &s.rows,
            places: &s.offsets,
            count: s.offsets.len(),
            seed: s.settings.seed,
            rounds: RoundSpan::Finite(1),
            fade_steps: 0,
            emitter: false,
        });
    }
    let s = st.downcast_ref::<EmitterState>()?;
    Some(GroupOf {
        rows: &s.rows,
        places: &s.layout,
        count: if s.planned > 0 { s.planned } else { s.rows.len() },
        seed: s.settings.seed,
        rounds: s.settings.round_span(),
        fade_steps: s.settings.fade_steps(world.steps_per_second),
        emitter: true,
    })
}

/// The settings a ball's look-only variety reads (Size, Size spread, Colour spread, Turn spread,
/// Seed): a Balls thing's own, or, for an emitter, the same read from its settings as a group's.
pub fn variety(kind: &str, params: &Params) -> BallsSettings {
    if kind == crate::things::emitter::KIND {
        return crate::things::emitter::EmitterSettings::from_params(params).variety();
    }
    BallsSettings::from_params(params)
}

/// Whether things of `kind` have ball rows wearing a ball Look.
pub fn has_balls(kind: &str) -> bool {
    kind == "balls" || kind == crate::things::emitter::KIND
}
