//! A Text's layers laid over each letter or each line, and worn by only some letters (Text part 4):
//! the Text writes its whole records once, then a patched copy for each distinct unit, each letter's
//! instance naming its copy. Shapes never come here.

use bpe_core::{LayerId, ResolvedSlot};
use bpe_gfx::LayerGpu;
use glam::Vec2;

use super::gpu::ShapeDrawn;
use super::{Across, ShapeStack};

/// The most patched copies of a Text's records a frame: past that, the other letters share the
/// whole records.
pub const MOST_UNIT_SETS: usize = 2048;

/// A drawn layer that isn't laid whole or worn by every letter: where its record is, how its own
/// colours are laid and which letters wear it (empty: all).
#[derive(Clone, Debug, PartialEq)]
pub struct UnitLayer {
    pub id: LayerId,
    pub record: usize,
    pub across: Across,
    pub only: Vec<u32>,
}

impl ShapeDrawn {
    /// How its own colours are laid on a Text: a Shader always whole, an Outline or Glow taking the
    /// colour of the layers under it has none of its own.
    pub fn across(&self) -> Across {
        use crate::modules::look::layers::ColourFrom;
        match self {
            ShapeDrawn::Paint(p) => p.across,
            ShapeDrawn::Outline(o) if o.colour_from == ColourFrom::Own => o.across,
            ShapeDrawn::Glow(g) if g.colour_from == ColourFrom::Own => g.across,
            _ => Across::WholeText,
        }
    }
}

impl ShapeStack {
    /// The layers of `slot` (this stack's card) laid over each letter or line or worn by only some
    /// letters, each with its record; empty when the whole text wears every layer whole.
    pub fn unit_layers(&self, slot: &ResolvedSlot) -> Vec<UnitLayer> {
        self.drawn
            .iter()
            .filter_map(|(id, d, _)| {
                let record = self.record_of(*id)?;
                let only = slot.layers.iter().find(|l| l.id == *id).map(|l| l.only.clone()).unwrap_or_default();
                let across = d.across();
                (across != Across::WholeText || !only.is_empty()).then_some(UnitLayer { id: *id, record, across, only })
            })
            .collect()
    }
}

/// Where one letter or line sits in the text's own frame: its points (for a straight gradient's
/// extent), its middle and its turn (radians).
#[derive(Clone, Copy, Debug)]
pub struct UnitPlace<'a> {
    pub points: &'a [Vec2],
    pub middle: Vec2,
    pub turn: f32,
}

/// `record` (a paint, outline or glow record with its own gradient) laid over `unit` instead of the
/// whole text: a Straight gradient's extent along its turned way, Round's own middle and reach, any
/// other form's centre and turn. One colour, and the Along the edge form, stay as they are.
pub fn lay_over(record: &mut LayerGpu, unit: &UnitPlace) {
    if record.paint.row < 0.0 || unit.points.is_empty() {
        return;
    }
    let turn = if unit.turn.is_finite() { unit.turn } else { 0.0 };
    let rot = Vec2::from_angle(turn);
    let code = (record.paint.pattern + 0.5) as u32;
    match code {
        0 | 4 => {}
        1 => {
            let dir = rot.rotate(Vec2::new(record.p1[0], record.p1[1]));
            let (lo, hi) = unit.points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                let d = p.dot(dir);
                (lo.min(d), hi.max(d))
            });
            record.p1 = [dir.x, dir.y, lo, hi];
            let place = rot.rotate(Vec2::new(record.paint.place[0], record.paint.place[1]));
            record.paint.place[0] = place.x;
            record.paint.place[1] = place.y;
        }
        2 => {
            let reach = unit.points.iter().map(|p| p.distance(unit.middle)).fold(0.0, f32::max).max(1e-3);
            record.paint.place[2] = unit.middle.x;
            record.paint.place[3] = unit.middle.y;
            record.p0[2] = reach;
        }
        _ => {
            let centre = unit.middle + rot.rotate(Vec2::new(record.paint.place[0], record.paint.place[1]));
            record.paint.place[0] = centre.x;
            record.paint.place[1] = centre.y;
            record.paint.angle += turn.to_degrees();
        }
    }
}

/// `layer`'s record not worn: passed over as if it weren't there, keeping its place.
pub fn unworn(record: &mut LayerGpu) {
    record.amount[0] = 0.0;
    record.head[2] |= LayerGpu::UNWORN;
}

/// `all` (the whole text's records) for one letter: each of `layers` laid over the letter's
/// `letter` or `line` unit as its Across says, and left off when `number` isn't one it names.
pub fn records_for_unit(
    all: &[LayerGpu],
    layers: &[UnitLayer],
    number: u32,
    letter: &UnitPlace,
    line: &UnitPlace,
) -> Vec<LayerGpu> {
    let mut out = all.to_vec();
    for l in layers {
        let Some(r) = out.get_mut(l.record) else { continue };
        match l.across {
            Across::WholeText => {}
            Across::EachLetter => lay_over(r, letter),
            Across::EachLine => lay_over(r, line),
        }
        if !l.only.is_empty() && !l.only.contains(&number) {
            unworn(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_gfx::PaintGpu;

    fn paint(pattern: f32) -> LayerGpu {
        LayerGpu {
            head: [LayerGpu::SHAPE_PAINT, 0, 0, 0],
            amount: [1.0, 0.0, 0.0, 0.0],
            p1: [1.0, 0.0, -100.0, 100.0],
            paint: PaintGpu { row: 0.0, pattern, scale: 1.0, place: [5.0, 0.0, 0.0, 0.0], ..PaintGpu::default() },
            ..LayerGpu::ZERO
        }
    }

    const BOX: [Vec2; 4] = [Vec2::new(10.0, -5.0), Vec2::new(30.0, -5.0), Vec2::new(30.0, 5.0), Vec2::new(10.0, 5.0)];

    #[test]
    fn a_straight_gradient_spans_the_unit_along_its_turned_way() {
        let mut r = paint(1.0);
        lay_over(&mut r, &UnitPlace { points: &BOX, middle: Vec2::new(20.0, 0.0), turn: 0.0 });
        assert_eq!(r.p1, [1.0, 0.0, 10.0, 30.0]);
        let mut r = paint(1.0);
        lay_over(&mut r, &UnitPlace { points: &BOX, middle: Vec2::new(20.0, 0.0), turn: std::f32::consts::FRAC_PI_2 });
        assert!(r.p1[0].abs() < 1e-6 && (r.p1[1] - 1.0).abs() < 1e-6);
        assert!((r.p1[2] + 5.0).abs() < 1e-4 && (r.p1[3] - 5.0).abs() < 1e-4, "{:?}", r.p1);
        assert!(r.paint.place[0].abs() < 1e-5 && (r.paint.place[1] - 5.0).abs() < 1e-5, "the offset turns too");
    }

    #[test]
    fn round_runs_from_the_units_own_middle_to_its_reach() {
        let mut r = paint(2.0);
        lay_over(&mut r, &UnitPlace { points: &BOX, middle: Vec2::new(20.0, 0.0), turn: 0.0 });
        assert_eq!([r.paint.place[2], r.paint.place[3]], [20.0, 0.0]);
        assert!((r.p0[2] - (125.0f32).sqrt()).abs() < 1e-4);
    }

    #[test]
    fn other_forms_move_their_centre_and_turn() {
        let mut r = paint(3.0);
        lay_over(&mut r, &UnitPlace { points: &BOX, middle: Vec2::new(20.0, 0.0), turn: std::f32::consts::PI });
        assert!((r.paint.place[0] - 15.0).abs() < 1e-4 && r.paint.place[1].abs() < 1e-4);
        assert!((r.paint.angle - 180.0).abs() < 1e-3);
    }

    #[test]
    fn one_colour_and_along_the_edge_stay() {
        let mut one = paint(1.0);
        one.paint.row = -1.0;
        let before = one;
        lay_over(&mut one, &UnitPlace { points: &BOX, middle: Vec2::ZERO, turn: 1.0 });
        assert_eq!(one, before);
        let mut edge = paint(4.0);
        let before = edge;
        lay_over(&mut edge, &UnitPlace { points: &BOX, middle: Vec2::ZERO, turn: 1.0 });
        assert_eq!(edge, before);
    }

    #[test]
    fn a_letter_not_named_leaves_the_layer_off() {
        let all = vec![LayerGpu::ZERO, paint(1.0)];
        let layers = vec![UnitLayer { id: LayerId(1), record: 1, across: Across::WholeText, only: vec![0, 2] }];
        let place = UnitPlace { points: &BOX, middle: Vec2::ZERO, turn: 0.0 };
        let worn = records_for_unit(&all, &layers, 2, &place, &place);
        assert_eq!(worn, all);
        let off = records_for_unit(&all, &layers, 1, &place, &place);
        assert_eq!(off[1].amount[0], 0.0);
        assert_eq!(off[1].head[2] & LayerGpu::UNWORN, LayerGpu::UNWORN);
    }
}
