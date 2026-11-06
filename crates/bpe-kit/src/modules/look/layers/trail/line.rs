//! A Line that stays, worked out from its ball's whole way since the start: simplified with a
//! [`Sleeve`] from the oldest point on, so a long line costs about one point a bounce. What the
//! fit has settled is remembered between frames (by thing, layer and ball), and each frame only
//! carries on from there with the samples that are new. The memory is checked against the
//! history every time (where the ball started and the last place it took in), and a frame
//! worked out from it is the same as one worked out afresh, so drawing a time again, scrubbing
//! or exporting draws the same.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use bpe_core::{LayerId, ThingId};
use bpe_runtime::History;
use glam::Vec2;

use super::kinds::{Path, Sleeve};

/// One point of a line: where, when (in steps), how far along the ball's way from its first
/// sample, and whether it is a bounce between samples.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mark {
    p: Vec2,
    t: f64,
    along: f64,
    corner: bool,
    /// The place before it (for still runs).
    before: Option<Vec2>,
}

/// What a line's fit has settled so far, and where it stopped.
#[derive(Clone, Debug)]
pub struct Memo {
    tol: f32,
    grid: u64,
    /// The ball's oldest sample: (the moment, in steps; where it was).
    start: (f64, Vec2),
    /// The last place taken in: (the moment, in steps; where it was); `None` before any.
    done: Option<(f64, Vec2)>,
    /// How far along its way the ball was at `done`.
    along: f64,
    /// The two places before (for still runs): the newest first.
    before: [Option<Vec2>; 2],
    sleeve: Sleeve<Mark>,
    /// The points kept for good, oldest first.
    kept: Vec<Mark>,
}

/// Which line a memo is for: the thing, its trail layer, the ball's row and how coarsely it is
/// simplified (the layer's level, see `draw::MOST_LEVEL`).
pub type Key = (ThingId, LayerId, usize, u32);

/// The most lines remembered at once; past it, everything is forgotten and worked out afresh.
const MOST_MEMOS: usize = 200_000;

/// The remembered lines, shared by every renderer (the memos check themselves, so it doesn't
/// matter whose they were).
pub fn memos() -> MutexGuard<'static, HashMap<Key, Memo>> {
    static MEMOS: OnceLock<Mutex<HashMap<Key, Memo>>> = OnceLock::new();
    MEMOS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The level each trail layer's Lines were last drawn at, where the next frame starts looking.
fn levels() -> MutexGuard<'static, HashMap<(ThingId, LayerId), u32>> {
    static LEVELS: OnceLock<Mutex<HashMap<(ThingId, LayerId), u32>>> = OnceLock::new();
    LEVELS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The level `layer`'s Lines were last drawn at (0 before any).
pub fn level_of(layer: (ThingId, LayerId)) -> u32 {
    levels().get(&layer).copied().unwrap_or(0)
}

/// Notes the level `layer`'s Lines were drawn at.
pub fn set_level(layer: (ThingId, LayerId), level: u32) {
    let mut levels = levels();
    if levels.len() >= MOST_MEMOS {
        levels.clear();
    }
    levels.insert(layer, level);
}

/// Ball `ball`'s whole line, newest (`head`, where it is now) first, simplified within `tol`
/// and always keeping every `grid`-th sample (see [`super::kinds::simplify`]) and where it
/// stopped each time it stood still. Carries on from `memo` when it still fits the history
/// (and puts what it settled back in it). `newest_ago` is how old the history's newest sample
/// is and `interval` the seconds between samples.
pub fn staying_line(
    memo: &mut Option<Memo>,
    history: &History,
    ball: usize,
    head: Vec2,
    newest_ago: f32,
    interval: f32,
    tol: f32,
    grid: u64,
) -> Path {
    let Some(start) = history.first(ball) else { return Path::default() };
    let same = |m: &Memo| m.tol == tol && m.grid == grid && m.start == start;
    let fits = |m: &Memo| same(m) && m.done.is_none_or(|(t, p)| history.item_at(ball, t) == Some(p));
    if let Some(m) = memo.as_mut()
        && same(m)
        && !fits(m)
    {
        rewind(m, history, ball);
    }
    if !memo.as_ref().is_some_and(fits) {
        *memo = Some(Memo {
            tol,
            grid,
            start,
            done: None,
            along: 0.0,
            before: [None, None],
            sleeve: Sleeve::new(tol),
            kept: Vec::new(),
        });
    }
    let Some(m) = memo.as_mut() else { return Path::default() };
    let every = history.every();
    let after = m.done.map_or(f64::NEG_INFINITY, |d| d.0);
    for (t, p) in history.trace_after(ball, after) {
        let corner = t % every as f64 != 0.0;
        if let Some(prev) = m.before[0] {
            m.along += (p - prev).length() as f64;
        }
        let on_grid = !corner && ((t as u64) / every).is_multiple_of(grid.max(1));
        let mark = Mark { p, t, along: m.along, corner, before: m.before[0] };
        m.sleeve.push(mark, p, on_grid || stops(&m.before, p), &mut m.kept);
        m.before = [Some(p), m.before[0]];
        m.done = Some((t, p));
    }
    // The ball where it is now ends the line; this part is worked out again each frame.
    let mut tail = Vec::new();
    let mut sleeve = m.sleeve.clone();
    let head_along = m.along + m.before[0].map_or(0.0, |prev| (head - prev).length() as f64);
    let head_mark = Mark { p: head, t: f64::INFINITY, along: head_along, corner: false, before: m.before[0] };
    sleeve.push(head_mark, head, false, &mut tail);
    sleeve.finish(&mut tail);
    // Seconds a step, and the newest sample's moment.
    let dt = interval as f64 / every as f64;
    let newest = history.newest_step().unwrap_or(0) as f64;
    let n = m.kept.len() + tail.len();
    let mut path = Path {
        points: Vec::with_capacity(n),
        ago: Vec::with_capacity(n),
        arc: Vec::with_capacity(n),
        corner: Vec::with_capacity(n),
    };
    for mark in tail.iter().rev().chain(m.kept.iter().rev()) {
        let ago = if mark.t.is_infinite() { 0.0 } else { (newest_ago as f64 + (newest - mark.t) * dt) as f32 };
        path.points.push(mark.p);
        path.ago.push(ago.max(0.0));
        path.arc.push((head_along - mark.along) as f32);
        path.corner.push(mark.corner);
    }
    path
}

/// Takes `m` back to its newest grid sample the history still has (the history is from an
/// earlier moment than `m` got to, as after scrubbing back), so it carries on from there: the
/// fit starts again at every grid sample, so what it had settled before one is still right.
/// Leaves it as it was when it has none.
fn rewind(m: &mut Memo, history: &History, ball: usize) {
    let every = history.every();
    let newest = history.newest_step().unwrap_or(0) as f64;
    let upto = m.kept.partition_point(|k| k.t <= newest);
    let on_grid = |k: &Mark| !k.corner && ((k.t as u64) / every).is_multiple_of(m.grid.max(1));
    let Some(at) =
        (0..upto).rev().find(|&j| on_grid(&m.kept[j]) && history.item_at(ball, m.kept[j].t) == Some(m.kept[j].p))
    else {
        return;
    };
    let g = m.kept[at];
    m.kept.truncate(at + 1);
    m.sleeve = Sleeve::from_anchor(m.tol, g.p);
    m.along = g.along;
    m.before = [Some(g.p), g.before];
    m.done = Some((g.t, g.p));
}

/// Whether the place `p` begins a stop: it is where the place before was, which wasn't itself
/// a stop. Kept, so a caught ball's line ends where it stopped (see [`Path::drop_still_head`]).
fn stops(before: &[Option<Vec2>; 2], p: Vec2) -> bool {
    before[0] == Some(p) && before[1] != Some(p)
}

/// Looks up `key`'s memo, works out its line and puts the memo back, forgetting every memo
/// once there are too many.
#[allow(clippy::too_many_arguments)]
pub fn remembered_line(
    memos: &mut HashMap<Key, Memo>,
    key: Key,
    history: &History,
    ball: usize,
    head: Vec2,
    newest_ago: f32,
    interval: f32,
    tol: f32,
    grid: u64,
) -> Path {
    let mut memo = memos.remove(&key);
    let path = staying_line(&mut memo, history, ball, head, newest_ago, interval, tol, grid);
    if memos.len() >= MOST_MEMOS {
        memos.clear();
    }
    if let Some(m) = memo {
        memos.insert(key, m);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_runtime::HistoryWant;

    /// A ball going round and bouncing out every so often, for `steps` steps at 240 a second.
    fn history(steps: u64) -> History {
        let mut h = History::new(240, HistoryWant { seconds: 0.0, forever: true });
        h.note(0, 1);
        for step in 0..=steps {
            let t = step as f32 / 240.0;
            let p = Vec2::from_angle(t) * 300.0;
            if step % 97 == 13 {
                h.corner(0, step as f64 - 0.5, p * 1.1);
            }
            h.record(step, &[p]);
        }
        h
    }

    fn head(h: &History) -> Vec2 {
        h.points(0).next().unwrap()
    }

    #[test]
    fn a_remembered_line_is_the_same_as_one_worked_out_afresh() {
        let i = 1.0 / 60.0;
        let mut memo = None;
        // On, back (as when scrubbing) and on again.
        for steps in [400u64, 2000, 2003, 2004, 2100, 9000, 1000, 5001, 4999, 4000, 9000] {
            let h = history(steps);
            let carried = staying_line(&mut memo, &h, 0, head(&h), 0.0, i, 0.15, 16);
            let fresh = staying_line(&mut None, &h, 0, head(&h), 0.0, i, 0.15, 16);
            assert_eq!(carried, fresh, "at {steps}");
            assert_eq!(carried.points.last(), h.points(0).last().as_ref(), "it reaches the start");
            assert!(carried.len() < h.len() / 2, "{} of {}", carried.len(), h.len());
        }
        // Gone back in time, the memo carries on from a grid sample before.
        let h = history(1000);
        let mut m = memo.clone().unwrap();
        rewind(&mut m, &h, 0);
        assert!(m.done.is_some_and(|d| d.0 <= 1000.0) && !m.kept.is_empty());
        let back = staying_line(&mut memo, &h, 0, head(&h), 0.0, i, 0.15, 16);
        assert_eq!(back, staying_line(&mut None, &h, 0, head(&h), 0.0, i, 0.15, 16));
        // Another tolerance or grid starts again.
        let other = staying_line(&mut memo, &h, 0, head(&h), 0.0, i, 1.0, 16);
        assert!(other.len() < back.len());
    }

    #[test]
    fn a_line_keeps_where_its_ball_stopped() {
        let mut h = History::new(240, HistoryWant { seconds: 0.0, forever: true });
        h.note(0, 1);
        for step in 0..=480u64 {
            let x = step.min(240) as f32;
            h.record(step, &[Vec2::new(x, 0.0)]);
        }
        let mut path = staying_line(&mut None, &h, 0, Vec2::new(240.0, 0.0), 0.0, 1.0 / 60.0, 0.15, 1 << 20);
        assert_eq!(path.drop_still_head(), 1);
        assert_eq!(path.points, vec![Vec2::new(240.0, 0.0), Vec2::ZERO]);
        // It stopped one sample after it got there: a second ago, near enough.
        assert!((path.ago[0] - (1.0 - 1.0 / 60.0)).abs() < 1e-4, "{}", path.ago[0]);
    }
}
