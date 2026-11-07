//! Showing a Text a letter, word or line at a time (Text part 2): when each one appears, timed by
//! seconds, the scene's beats or the hits of a thing, and how it moves as it comes and goes. Pure in
//! time: worked out from the world and the log at the moment drawn.

use bpe_core::ThingId;
use bpe_runtime::{EventKind, Log, World};
use glam::Vec2;

use super::{ShowBy, TextMove, TextSettings, TimedBy};
use crate::text::Letter;

/// How one letter shows at a moment: scaled about its middle, moved, and how much of it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LetterMove {
    pub scale: f32,
    pub offset: Vec2,
    pub opacity: f32,
}

impl LetterMove {
    /// As laid out.
    pub const WHOLE: LetterMove = LetterMove { scale: 1.0, offset: Vec2::ZERO, opacity: 1.0 };

    pub fn is_whole(&self) -> bool {
        *self == Self::WHOLE
    }
}

/// Which unit (one appearing together) `letter` belongs to.
pub fn unit_of(letter: &Letter, by: ShowBy) -> u32 {
    match by {
        ShowBy::AllAtOnce => 0,
        ShowBy::LetterByLetter => letter.index,
        ShowBy::WordByWord => letter.word,
        ShowBy::LineByLine => letter.line,
    }
}

/// Whether the settings show the text whole all along (nothing to work out).
pub fn shows_whole(s: &TextSettings) -> bool {
    s.show == ShowBy::AllAtOnce
        && !moves(s.appear)
        && s.timed_by == TimedBy::Seconds && s.show_start <= 0.0 && s.stays <= 0.0
}

/// The most hits looked for to time a show.
const MOST_UNITS: usize = 100_000;

/// When each of `count` units appears (`None`: not yet), as `s` times them; `me` the Text (a target
/// may name it), `world` and `log` at the moment drawn.
pub fn appear_times(s: &TextSettings, count: usize, world: &World, log: Option<&Log>, me: ThingId) -> Vec<Option<f64>> {
    let count = count.min(MOST_UNITS);
    let start = if s.show_start.is_finite() { s.show_start.max(0.0) as f64 } else { 0.0 };
    match s.timed_by {
        TimedBy::Seconds => {
            let each = if s.each.is_finite() { s.each.max(0.0) as f64 } else { 0.0 };
            (0..count).map(|k| Some(start + k as f64 * each)).collect()
        }
        TimedBy::Beats => {
            let beat = world.beat_seconds;
            if !(beat.is_finite() && beat > 0.0) {
                return vec![Some(start); count];
            }
            let every = if s.every.is_finite() && s.every > 0.0 { s.every as f64 } else { 1.0 };
            let first = (start / beat - 1e-9).ceil();
            (0..count).map(|k| Some((first + k as f64 * every) * beat)).collect()
        }
        TimedBy::Hits => {
            let mut out = vec![None; count];
            let Some(log) = log else { return out };
            let things = world.matching(&s.hits_of, me);
            let rate = world.steps_per_second.max(1);
            let from = (start * rate as f64).ceil() as u64;
            let events = log.events();
            let begin = events.partition_point(|e| e.step < from);
            let end = events.partition_point(|e| e.step <= world.step);
            let hits = events[begin..end.max(begin)]
                .iter()
                .filter(|e| e.kind == EventKind::Hit)
                .filter(|e| things.contains(&e.a.thing) || e.b.is_some_and(|b| things.contains(&b.thing)))
                .map(|e| e.time(rate));
            for (slot, t) in out.iter_mut().zip(hits) {
                *slot = Some(t);
            }
            out
        }
    }
}

/// `t` (0 to 1) eased out with a little overshoot.
fn back_out(t: f32) -> f32 {
    let (c1, c3) = (1.70158, 2.70158);
    let u = t - 1.0;
    1.0 + c3 * u * u * u + c1 * u * u
}

/// `t` (0 to 1) eased out with bounces, as a ball dropped on the floor.
fn bounce_out(t: f32) -> f32 {
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// Whether `m` moves over time (else it shows or goes at once).
fn moves(m: TextMove) -> bool {
    !matches!(m, TextMove::None | TextMove::Typewriter)
}

/// A letter `t` (0 to 1) of the way in by `m`, for letters `size` px high.
fn coming(m: TextMove, t: f32, size: f32) -> LetterMove {
    let t = t.clamp(0.0, 1.0);
    let w = LetterMove::WHOLE;
    match m {
        TextMove::None | TextMove::Typewriter => w,
        TextMove::Pop => LetterMove { scale: back_out(t).max(0.0), ..w },
        TextMove::Fade => LetterMove { opacity: t, ..w },
        TextMove::Slide => {
            let e = 1.0 - (1.0 - t) * (1.0 - t);
            LetterMove { offset: Vec2::new(0.0, (1.0 - e) * 0.6 * size), opacity: t, ..w }
        }
        TextMove::Drop => {
            LetterMove { offset: Vec2::new(0.0, -(1.0 - bounce_out(t)) * 1.2 * size), opacity: (t / 0.3).min(1.0), ..w }
        }
    }
}

/// A letter `p` (0 to 1) of the way out by `m`: the move backwards, but a drop falls on down.
fn going(m: TextMove, p: f32, size: f32) -> LetterMove {
    let p = p.clamp(0.0, 1.0);
    match m {
        TextMove::Drop => LetterMove { offset: Vec2::new(0.0, p * p * 1.5 * size), opacity: 1.0 - p, ..LetterMove::WHOLE },
        _ => coming(m, 1.0 - p, size),
    }
}

/// How a letter that appears at `appear` shows at `now` (`None`: not shown), for letters `size`
/// px high.
pub fn letter_move(s: &TextSettings, appear: Option<f64>, now: f64, size: f32) -> Option<LetterMove> {
    let a = appear?;
    if now < a {
        return None;
    }
    let takes = if s.move_time.is_finite() { s.move_time.max(0.01) as f64 } else { 0.35 };
    let came = if moves(s.appear) { takes } else { 0.0 };
    let mut shown = if now < a + came { coming(s.appear, ((now - a) / takes) as f32, size) } else { LetterMove::WHOLE };
    if s.stays.is_finite() && s.stays > 0.0 {
        let leave = a + came + s.stays as f64;
        if now >= leave {
            if !moves(s.disappear) {
                return None;
            }
            let p = (now - leave) / takes;
            if p >= 1.0 {
                return None;
            }
            shown = going(s.disappear, p as f32, size);
        }
    }
    Some(shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(appear: TextMove, disappear: TextMove, stays: f32) -> TextSettings {
        TextSettings { appear, disappear, stays, move_time: 0.5, ..TextSettings::default() }
    }

    #[test]
    fn a_letter_shows_from_its_moment() {
        let s = settings(TextMove::None, TextMove::None, 0.0);
        assert_eq!(letter_move(&s, None, 5.0, 100.0), None, "not yet");
        assert_eq!(letter_move(&s, Some(1.0), 0.99, 100.0), None);
        assert_eq!(letter_move(&s, Some(1.0), 1.0, 100.0), Some(LetterMove::WHOLE));
        assert_eq!(letter_move(&s, Some(1.0), 1000.0, 100.0), Some(LetterMove::WHOLE), "stays");
    }

    #[test]
    fn each_move_starts_small_or_away_and_ends_whole() {
        for m in [TextMove::Pop, TextMove::Fade, TextMove::Slide, TextMove::Drop] {
            let s = settings(m, TextMove::None, 0.0);
            let start = letter_move(&s, Some(0.0), 0.0, 100.0).unwrap();
            assert!(start.scale < 0.01 || start.opacity < 0.01, "{m:?} starts hidden: {start:?}");
            let end = letter_move(&s, Some(0.0), 0.5, 100.0).unwrap();
            assert!((end.scale - 1.0).abs() < 1e-4 && end.offset.length() < 1e-3 && end.opacity == 1.0, "{m:?} {end:?}");
        }
        // A pop overshoots on the way.
        let s = settings(TextMove::Pop, TextMove::None, 0.0);
        let peak = (1..50).map(|k| letter_move(&s, Some(0.0), k as f64 / 100.0, 100.0).unwrap().scale).fold(0.0, f32::max);
        assert!(peak > 1.05, "{peak}");
        // A drop comes from above, a slide from below.
        let drop = coming(TextMove::Drop, 0.1, 100.0);
        let slide = coming(TextMove::Slide, 0.1, 100.0);
        assert!(drop.offset.y < 0.0 && slide.offset.y > 0.0);
    }

    #[test]
    fn stays_for_then_goes() {
        let s = settings(TextMove::Fade, TextMove::Fade, 2.0);
        // In over 0.5 s, whole for 2 s, out over 0.5 s.
        assert_eq!(letter_move(&s, Some(1.0), 3.4, 100.0), Some(LetterMove::WHOLE));
        let out = letter_move(&s, Some(1.0), 3.75, 100.0).unwrap();
        assert!((out.opacity - 0.5).abs() < 1e-3, "{out:?}");
        assert_eq!(letter_move(&s, Some(1.0), 4.0, 100.0), None);
        // Gone at once without a disappearing move; a drop falls on down.
        let s = settings(TextMove::None, TextMove::None, 2.0);
        assert_eq!(letter_move(&s, Some(0.0), 2.0, 100.0), None);
        let s = settings(TextMove::None, TextMove::Drop, 1.0);
        assert!(letter_move(&s, Some(0.0), 1.25, 100.0).unwrap().offset.y > 0.0);
    }

    #[test]
    fn seconds_and_beats_time_each_one() {
        let kit = crate::kit();
        let scene = crate::new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let s = TextSettings { show_start: 1.0, each: 0.25, ..TextSettings::default() };
        let t = appear_times(&s, 3, &world, None, ThingId::ROOT);
        assert_eq!(t, [Some(1.0), Some(1.25), Some(1.5)]);
        // 120 beats a minute: a beat every 0.5 s; the first at or after 1.1 s is at 1.5 s.
        let beat = world.beat_seconds;
        assert!(beat > 0.0);
        let s = TextSettings { timed_by: TimedBy::Beats, show_start: 1.1, every: 2.0, ..TextSettings::default() };
        let t = appear_times(&s, 2, &world, None, ThingId::ROOT);
        let first = (1.1 / beat).ceil() * beat;
        assert!((t[0].unwrap() - first).abs() < 1e-9 && (t[1].unwrap() - (first + 2.0 * beat)).abs() < 1e-9, "{t:?}");
        // Hits with no log: none yet.
        let s = TextSettings { timed_by: TimedBy::Hits, ..TextSettings::default() };
        assert_eq!(appear_times(&s, 2, &world, None, ThingId::ROOT), [None, None]);
    }
}
