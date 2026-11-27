//! Moving parts (Water mode part 5, `docs/superpowers/plans/water-mode/part5-moving-parts.md`): the
//! card lays them over the still field where they are, and they push the water.

use std::sync::Arc;

use bpe_water::drops::LEAST_WALL;
use bpe_water::scenes;
use bpe_water::{
    Drop, Feel, Layout, Mover, MoverShape, Movers, Numbers, Pose, STOPS, Solid, SolidField, WATER, Water, WaterGpu,
};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

const CELL: f32 = 8.0;

/// An open field 600 px across with nothing in it.
fn open() -> SolidField {
    let layout = Layout::cover(Vec2::splat(-300.0), Vec2::splat(300.0), CELL);
    SolidField::build_least(layout, &[], LEAST_WALL * CELL)
}

/// `solids` as one moving part laid on `still`, its grid aligned with the course's at `place`.
fn mover_of(still: &SolidField, solids: &[Solid], place: Vec2) -> Movers {
    let l = still.layout;
    let shape = MoverShape::of(solids, l.cell, LEAST_WALL * l.cell, l.origin - place).expect("a shape");
    Movers { still: Arc::new(still.clone()), movers: vec![Mover { shape: Arc::new(shape), wetting: 0.5, owner: 3 }] }
}

/// Checks the field on the card against the processor's twin at `pose`: the distances within 0.01
/// cells and the nodes marked as the part's well inside its grid, and the still field's own to the
/// bit far from it and from `was`.
fn check_field(water: &Water, movers: &Movers, pose: Pose, was: Vec2, inside: f32) {
    let card = water.solid_field_now();
    let l = movers.still.layout;
    let n = l.nodes();
    let want = movers.field_at(&[pose]);
    let want_card = want.for_card();
    let still = movers.still.for_card();
    let far = movers.movers[0].shape.radius * pose.scale + 2.0 * l.cell;
    let (mut near, mut marked) = (0, 0);
    for y in 0..l.rows {
        for x in 0..l.cols {
            let k = (y * l.cols + x) as usize;
            let p = l.to_world(Vec2::new(x as f32, y as f32));
            if p.distance(pose.at) < inside {
                near += 1;
                let (got, wanted) = (card[k], want_card[k]);
                assert!((got - wanted).abs() < 0.01, "node {x}, {y}: {got} cells is not {wanted}");
                assert_eq!(card[n + k], want_card[n + k], "node {x}, {y}: its Wetting");
                let mine = want.owner[k] == movers.movers[0].owner;
                assert_eq!(card[2 * n + k], if mine { 1.0 } else { 0.0 }, "node {x}, {y}: whose");
                marked += usize::from(mine);
            } else if p.distance(pose.at) > far && p.distance(was) > far {
                assert_eq!(card[k].to_bits(), still[k].to_bits(), "node {x}, {y}");
                assert_eq!(card[n + k].to_bits(), still[n + k].to_bits(), "node {x}, {y}: its Wetting");
                assert_eq!(card[2 * n + k], 0.0, "node {x}, {y}: whose");
            }
        }
    }
    assert!(near > 300 && marked > 300, "{near} nodes near, {marked} the part's");
}

/// A part set at a pose is there on the card, and one moved is at its new pose after a frame,
/// with drops (each step lays it) and without (the frame's end does).
#[test]
fn a_still_mover_is_where_it_is() {
    let Some(gpu) = card() else { return };
    let still = open();
    let block = [Solid::rect(Vec2::splat(-30.0), Vec2::splat(30.0), true, 0.0)];
    let place = Vec2::new(-40.0, -60.0);
    let movers = mover_of(&still, &block, place);
    let here = Pose::new(place, 0.3, 1.0);
    let there = Pose::new(place + Vec2::new(40.0, 0.0), 0.3, 1.0);
    for with_drops in [true, false] {
        let mut water = Water::new(gpu.clone(), &still, scenes::GRAVITY, Numbers::for_cell(CELL), &[Feel::default()]);
        water.set_movers(movers.clone(), &[here]);
        assert_eq!(water.poses(), &[here]);
        check_field(&water, &movers, here, here.at, 80.0);
        if with_drops {
            let l = water.layout();
            let pool: Vec<Drop> = (0..400)
                .map(|k| {
                    let at = Vec2::new(-200.0 + (k % 40) as f32 * 4.0, 200.0 + (k / 40) as f32 * 4.0);
                    Drop::new(l.to_grid(at), Vec2::ZERO, STOPS, k, WATER)
                })
                .collect();
            water.add(&pool);
        }
        water.move_to(&[there]);
        water.frame();
        assert_eq!(water.poses(), &[there]);
        check_field(&water, &movers, there, here.at, 80.0);
    }
}
