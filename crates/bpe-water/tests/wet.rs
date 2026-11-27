//! Wet marks (`bpe_water::draw::wet`, Water mode part 6b): a drop marks the four nodes round it
//! with its frame and a later frame never moves them, air marks nothing, and the field is the same
//! whatever order the drops are in. On the software card.

use bpe_water::draw::wet::DRY;
use bpe_water::draw::{StoredDrops, WetField};
use bpe_water::{AIR, Drop, Layout, WATER};
use glam::Vec2;

fn layout() -> Layout {
    Layout::cover(Vec2::ZERO, Vec2::splat(80.0), 8.0)
}

fn drop_at(l: &Layout, p: Vec2, kind: u32) -> Drop {
    Drop::new(l.to_grid(p), Vec2::ZERO, kind, 0, WATER)
}

/// The nodes round world point `p`.
fn round(l: &Layout, p: Vec2) -> [usize; 4] {
    let g = l.to_grid(p).floor();
    let k = g.y as usize * l.cols as usize + g.x as usize;
    [k, k + 1, k + l.cols as usize, k + l.cols as usize + 1]
}

#[test]
fn a_drop_marks_the_nodes_round_it_first_and_air_marks_none() {
    let Ok((device, queue)) = bpe_gfx::test_device() else { return };
    let l = layout();
    let wet = WetField::new(&device, &queue, l);
    assert!(wet.read().iter().all(|&m| m == DRY), "a new field is dry");
    let (a, b, air) = (Vec2::new(20.0, 20.0), Vec2::new(60.0, 44.0), Vec2::new(36.0, 70.0));
    let first = StoredDrops::new(&device, &queue, &[drop_at(&l, a, 0), drop_at(&l, air, AIR)]);
    wet.mark(&first.buffers(), 5);
    let later = StoredDrops::new(&device, &queue, &[drop_at(&l, a, 0), drop_at(&l, b, 1)]);
    wet.mark(&later.buffers(), 9);
    let marks = wet.read();
    let (at_a, at_b) = (round(&l, a), round(&l, b));
    for k in at_a {
        assert_eq!(marks[k], 5, "first wet at 5, and a later frame doesn't move it");
    }
    for k in at_b {
        assert_eq!(marks[k], 9);
    }
    let marked = marks.iter().filter(|&&m| m != DRY).count();
    assert_eq!(marked, 8, "only the nodes round the liquid drops (the air marks none)");
    wet.clear();
    assert!(wet.read().iter().all(|&m| m == DRY), "cleared");
}

#[test]
fn the_marks_are_the_same_whatever_order_the_drops_are_in() {
    let Ok((device, queue)) = bpe_gfx::test_device() else { return };
    let l = layout();
    let drops: Vec<Drop> =
        (0..40).map(|k| drop_at(&l, Vec2::new(10.0 + 1.7 * k as f32, 12.0 + (k * 7 % 50) as f32), 0)).collect();
    let mut backwards = drops.clone();
    backwards.reverse();
    // Half the drops at frame 4, all of them at 2 and then at 7.
    let field = |list: &[Drop], half: &[Drop]| {
        let wet = WetField::new(&device, &queue, l);
        wet.mark(&StoredDrops::new(&device, &queue, half).buffers(), 4);
        wet.mark(&StoredDrops::new(&device, &queue, list).buffers(), 2);
        wet.mark(&StoredDrops::new(&device, &queue, list).buffers(), 7);
        wet.read()
    };
    let one = field(&drops, &drops[..20]);
    assert!(one.iter().all(|&m| m == DRY || m == 2), "every node touched first at 2");
    assert_eq!(one, field(&backwards, &backwards[..20]), "the same field");
    let mut again = WetField::new(&device, &queue, l);
    again.upload(&one);
    assert_eq!(again.read(), one, "uploaded marks read back");
    again = WetField::new(&device, &queue, l);
    again.upload(&[0; 3]);
    assert!(again.read().iter().all(|&m| m == DRY), "marks for another layout leave it dry");
}
