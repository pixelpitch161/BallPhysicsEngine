//! Every play order on a fixed note set and hit sequence.

use bpe_core::NoteSlot;
use bpe_kit::sound::{Plays, StrumDir};
use bpe_sound::voices::{AnyPick, NotePicker, PickCx};

fn slot() -> NoteSlot {
    NoteSlot { notes: vec![60, 64, 67], order_clicked: vec![67, 60, 64] }
}

/// The first note each of `hits` hits plays, with `f` setting up each hit's context.
fn run(plays: Plays, hits: u64, f: impl Fn(&mut PickCx)) -> Vec<u8> {
    let slot = slot();
    let mut p = AnyPick::new(plays);
    (0..hits)
        .map(|i| {
            let mut cx = PickCx::plain(&slot, i);
            cx.seed = 0xC0FFEE;
            f(&mut cx);
            p.pick(&cx).notes().first().copied().unwrap_or(0)
        })
        .collect()
}

fn twelve(plays: Plays) -> Vec<u8> {
    run(plays, 12, |_| {})
}

#[test]
fn golden_lists_per_order() {
    assert_eq!(twelve(Plays::Up), [60, 64, 67, 60, 64, 67, 60, 64, 67, 60, 64, 67]);
    assert_eq!(twelve(Plays::Down), [67, 64, 60, 67, 64, 60, 67, 64, 60, 67, 64, 60]);
    assert_eq!(twelve(Plays::UpDown), [60, 64, 67, 64, 60, 64, 67, 64, 60, 64, 67, 64]);
    assert_eq!(twelve(Plays::DownUp), [67, 64, 60, 64, 67, 64, 60, 64, 67, 64, 60, 64]);
    assert_eq!(twelve(Plays::Converge), [60, 67, 64, 60, 67, 64, 60, 67, 64, 60, 67, 64]);
    assert_eq!(twelve(Plays::Diverge), [64, 67, 60, 64, 67, 60, 64, 67, 60, 64, 67, 60]);
    assert_eq!(twelve(Plays::OrderClicked), [67, 60, 64, 67, 60, 64, 67, 60, 64, 67, 60, 64]);
    assert_eq!(run(Plays::ByBall, 6, |cx| cx.ball_number = cx.hit_index as u32 * 2), [60, 67, 64, 60, 67, 64]);
    assert_eq!(run(Plays::BySide, 4, |cx| cx.side = 3 - cx.hit_index as u32), [60, 67, 64, 60]);
    assert_eq!(twelve(Plays::Random), [64, 67, 64, 64, 60, 64, 64, 67, 64, 67, 64, 67]);
    assert_eq!(twelve(Plays::Shuffle), [67, 60, 64, 67, 64, 60, 64, 67, 60, 67, 60, 64]);
    assert_eq!(twelve(Plays::RandomWalk), [60, 64, 60, 64, 60, 64, 67, 64, 60, 64, 60, 64]);
    // Shuffle plays every note once each time round, never the same note twice running.
    let s = twelve(Plays::Shuffle);
    assert!(s.windows(2).all(|w| w[0] != w[1]), "{s:?}");
    for round in s.chunks(3) {
        let mut r = round.to_vec();
        r.sort();
        assert_eq!(r, [60, 64, 67]);
    }
    // Random walk moves one step every time.
    let w = twelve(Plays::RandomWalk);
    let place = |n: u8| [60, 64, 67].iter().position(|&x| x == n).unwrap() as i32;
    assert!(w.windows(2).all(|p| (place(p[0]) - place(p[1])).abs() == 1), "{w:?}");
    // All at once plays the whole slot on every hit.
    let slot = slot();
    let pick = AnyPick::new(Plays::AllAtOnce).pick(&PickCx::plain(&slot, 5));
    assert_eq!(pick.notes(), [60, 64, 67]);
    assert_eq!(pick.strum, 0.0);
}

#[test]
fn strum_spreads_starts() {
    let slot = slot();
    let mut cx = PickCx::plain(&slot, 0);
    cx.strum_s = 0.1;
    let up = AnyPick::new(Plays::AllAtOnce).pick(&cx);
    assert_eq!(up.notes(), [60, 64, 67]);
    assert!((up.strum - 0.05).abs() < 1e-6, "100 ms over three notes: 50 ms apart");
    cx.strum_dir = StrumDir::Down;
    assert_eq!(AnyPick::new(Plays::AllAtOnce).pick(&cx).notes(), [67, 64, 60]);
    cx.strum_dir = StrumDir::Random;
    let mut r = AnyPick::new(Plays::AllAtOnce).pick(&cx).notes().to_vec();
    r.sort();
    assert_eq!(r, [60, 64, 67], "every note, in some order");
}

#[test]
fn repeat_each_holds_n_hits() {
    let up = run(Plays::Up, 8, |cx| cx.repeat = 2);
    assert_eq!(up, [60, 60, 64, 64, 67, 67, 60, 60]);
    let random = run(Plays::Random, 12, |cx| cx.repeat = 3);
    for held in random.chunks(3) {
        assert!(held.iter().all(|&n| n == held[0]), "{random:?}");
    }
}

#[test]
fn octave_range_extends_the_run() {
    let up = run(Plays::Up, 7, |cx| cx.octave_range = 1);
    assert_eq!(up, [60, 64, 67, 72, 76, 79, 60]);
    let slot = slot();
    let mut cx = PickCx::plain(&slot, 0);
    cx.octave_range = 2;
    assert_eq!(AnyPick::new(Plays::AllAtOnce).pick(&cx).notes(), [60, 64, 67, 72, 76, 79, 84, 88, 91]);
}

#[test]
fn empty_slot_plays_nothing() {
    let empty = NoteSlot::default();
    for plays in Plays::ALL {
        let mut p = AnyPick::new(plays);
        assert!(p.pick(&PickCx::plain(&empty, 3)).is_empty(), "{plays:?}");
    }
}

#[test]
fn deterministic_after_seek() {
    // Every order but Random walk depends on the step alone: picking hit 7 cold gives what the
    // run gave it.
    for plays in Plays::ALL.into_iter().filter(|p| *p != Plays::RandomWalk) {
        let all = twelve(plays);
        let slot = slot();
        let mut fresh = AnyPick::new(plays);
        let mut cx = PickCx::plain(&slot, 7);
        cx.seed = 0xC0FFEE;
        assert_eq!(fresh.pick(&cx).notes()[0], all[7], "{plays:?}");
    }
    // Random walk, reset and walked again from the start, gives the same.
    let slot = slot();
    let mut walk = AnyPick::new(Plays::RandomWalk);
    let walk_all = |w: &mut AnyPick| -> Vec<u8> {
        (0..12)
            .map(|i| {
                let mut cx = PickCx::plain(&slot, i);
                cx.seed = 0xC0FFEE;
                w.pick(&cx).notes()[0]
            })
            .collect()
    };
    let first = walk_all(&mut walk);
    walk.reset();
    assert_eq!(walk_all(&mut walk), first);
}
