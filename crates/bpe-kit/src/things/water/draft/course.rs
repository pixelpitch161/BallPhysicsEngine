//! The course's Pieces and Walls as the water sees them (Water mode part 3): each one's Shape or
//! Line, its place and turn, and its Material's Wetting, laid over the carved rock in the scene's
//! order ([`super::WaterSetup::field_with`]).

use bpe_core::{Scene, SlotId, ThingId, Value};
use bpe_water::parts::SCALE_RANGE;
use bpe_water::{Part, Pose, Solid};
use glam::Vec2;

use bpe_core::maths::Maths;

use super::Keyed;
use crate::modules::water::pieces::{corners, outlines, placed};
use crate::modules::water::{MaterialCard, MovesCard, MovesKind, PieceShape, WallLine};
use crate::things::water::{PIECE, WALL};

/// One Piece or Wall as the water sees it.
#[derive(Clone, Debug)]
pub struct SolidSetup {
    pub id: ThingId,
    /// [`PIECE`] or [`WALL`].
    pub kind: &'static str,
    /// Its place (pixels) and turn (radians clockwise).
    pub at: Vec2,
    pub turn: f32,
    pub(super) form: Keyed,
    pub(super) material: Keyed,
    pub(super) moves: Keyed,
}

/// What a solid thing is: a Piece's Shape or a Wall's Line.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    Piece(PieceShape),
    Wall(WallLine),
}

impl SolidSetup {
    /// Piece or Wall `id` of `scene`, with its cards as resolved (missing cards are the defaults).
    pub fn of(scene: &Scene, resolved: &bpe_core::Resolved, id: ThingId) -> Option<SolidSetup> {
        let thing = scene.things.get(&id)?;
        let kind = match thing.kind.as_str() {
            PIECE => PIECE,
            WALL => WALL,
            _ => return None,
        };
        let card = |slot| Keyed::of(scene, resolved, id, slot).unwrap_or_default();
        Some(SolidSetup {
            id,
            kind,
            at: thing.transform.position,
            turn: thing.transform.rotation,
            form: card(SlotId::Form),
            material: card(SlotId::Material),
            moves: card(SlotId::Moves),
        })
    }

    /// Its Shape or Line at the start.
    pub fn form(&self) -> Form {
        if self.kind == PIECE { Form::Piece(self.form.card(0.0)) } else { Form::Wall(self.form.card(0.0)) }
    }

    /// Its Material's Wetting.
    pub fn wetting(&self) -> f32 {
        self.material.card::<MaterialCard>(0.0).wetting
    }

    /// Its Material at `time` seconds.
    pub fn material_at(&self, time: f64) -> MaterialCard {
        self.material.card(time)
    }

    /// How its Moves card says it moves.
    pub fn motion(&self) -> MovesKind {
        self.moves.card::<MovesCard>(0.0).moves
    }

    /// Whether it cuts the course (its Shape's or Line's Cuts).
    pub fn cuts(&self) -> bool {
        match self.form() {
            Form::Piece(shape) => shape.cuts,
            Form::Wall(line) => line.cuts,
        }
    }

    /// Where a Keyframed part is at water time `time` (seconds): its place plus its Position, its
    /// turn plus its Turn, and its Scale (decision 5 of the part 5 plan).
    pub fn pose_at(&self, time: f64) -> Pose {
        let c: MovesCard = self.moves.card(time);
        let offset = if c.offset.is_finite() { c.offset } else { Vec2::ZERO };
        let turn = if c.turn.is_finite() { c.turn.to_radians() } else { 0.0 };
        Pose::new(self.place() + offset, self.turn() + turn, c.scale).sane()
    }

    /// What it is as a moving part, in its own frame (its place at the middle, not turned, Scale 1):
    /// a Piece's walls or its filled shape, a Wall's line, its Cuts left out (a moving part never cuts).
    /// `None` when it has nothing to it.
    pub fn moving_part(&self) -> Option<Vec<Solid>> {
        let solids = match self.form() {
            Form::Piece(shape) => shape.parts(),
            Form::Wall(line) => line.solid().into_iter().collect(),
        };
        let solids: Vec<Solid> = solids.into_iter().filter(Solid::is_sound).collect();
        (!solids.is_empty()).then_some(solids)
    }

    /// The smallest Scale its Moves card takes, its base value's and every key's, at most 1 (a part
    /// keyed small has its thin walls taken as thicker in its own frame, so they stay thick enough).
    pub fn least_scale(&self) -> f32 {
        let base = self.moves.card::<MovesCard>(0.0).scale;
        let keys = self.moves.tracks.iter().filter(|(k, _)| k == "scale").flat_map(|(_, t)| &t.keys);
        let keyed = keys.filter_map(|k| match k.value {
            Value::Number(v) => Some(v as f32),
            _ => None,
        });
        std::iter::once(base).chain(keyed).filter(|v| v.is_finite()).fold(1.0f32, f32::min).clamp(SCALE_RANGE.0, 1.0)
    }

    /// What it adds and what it cuts, in its own frame: `(solids, cut)`.
    fn own(&self) -> (Vec<Solid>, Vec<Solid>) {
        match self.form() {
            Form::Piece(shape) if shape.cuts => {
                (if shape.is_hollow() { shape.parts() } else { Vec::new() }, shape.cut())
            }
            Form::Piece(shape) => (shape.parts(), Vec::new()),
            Form::Wall(line) => {
                let solid: Vec<Solid> = line.solid().into_iter().collect();
                if line.cuts { (Vec::new(), solid) } else { (solid, Vec::new()) }
            }
        }
    }

    fn world(&self, solids: Vec<Solid>) -> Vec<Solid> {
        let (at, turn) = (self.place(), self.turn());
        solids.iter().map(|s| placed(s, at, turn)).collect()
    }

    fn place(&self) -> Vec2 {
        if self.at.is_finite() { self.at } else { Vec2::ZERO }
    }

    fn turn(&self) -> f32 {
        if self.turn.is_finite() { self.turn } else { 0.0 }
    }

    /// It as a part of the course, in world pixels (`None` when it has nothing to it).
    pub fn part(&self) -> Option<Part> {
        let (solids, cut) = self.own();
        let part = Part { solids: self.world(solids), cut: self.world(cut), wetting: self.wetting() };
        (!part.solids.is_empty() || !part.cut.is_empty()).then_some(part)
    }

    /// Its body in world pixels: what it adds, or with Cuts on what it takes away.
    fn body(&self) -> Vec<Solid> {
        let (solids, cut) = self.own();
        self.world(if solids.is_empty() { cut } else { solids.into_iter().chain(cut).collect() })
    }

    /// How far world point `p` is from its body (below 0 inside).
    pub fn distance(&self, p: Vec2) -> f32 {
        self.body().iter().filter(|s| s.is_sound()).map(|s| s.distance(p, 0.0)).fold(f32::INFINITY, f32::min)
    }

    /// Its outlines to draw on the stage, in world pixels ([`outlines`]).
    pub fn outline(&self) -> Vec<Vec<Vec2>> {
        outlines(&self.body())
    }

    /// Where other solids and pen points snap to it (world pixels): a piece's hull's corners (a
    /// wheel's top, bottom and sides), a wall's own points.
    pub fn corners(&self) -> Vec<Vec2> {
        match self.form() {
            Form::Piece(shape) => corners(&self.world(shape.hull())),
            Form::Wall(line) => {
                line.points.iter().map(|p| Vec2::from(*p)).filter(|p| p.is_finite()).map(|p| self.to_world(p)).collect()
            }
        }
    }

    /// World point `p` in its own frame (its place at the middle, not turned).
    pub fn to_frame(&self, p: Vec2) -> Vec2 {
        let (s, c) = self.turn().det_sin_cos();
        let d = p - self.place();
        Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c)
    }

    /// A point of its own frame in the world.
    pub fn to_world(&self, p: Vec2) -> Vec2 {
        let (s, c) = self.turn().det_sin_cos();
        self.place() + Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
    }

    /// Its thinnest part (pixels).
    pub fn thinnest(&self) -> f32 {
        match self.form() {
            Form::Piece(shape) => shape.thinnest(),
            Form::Wall(line) => line.thinnest(),
        }
    }
}
