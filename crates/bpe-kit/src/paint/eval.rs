//! Turning a paint into colours: where along the gradient each ball sits (from what the paint
//! follows, its scroll and its repeat), and the colour found there.

use super::{Follow, Paint, PlaceShape, Repeat, Space};
use bpe_core::{Color, Gradient, Stop, ThingId, Transition};
use bpe_runtime::{EventKind, Log};
use glam::Vec2;

/// What a paint reads about one ball. Fill in only what the paint follows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub index: usize,
    pub count: usize,
    pub place: Vec2,
    pub place_screen: Vec2,
    pub start: Vec2,
    pub speed: f32,
    pub hits: u32,
    pub since_hit: f32,
    pub life: f32,
    pub path: f32,
    pub nearest: Option<[f32; 4]>,
    pub hit_colour: Option<[f32; 4]>,
    pub beats: f32,
    pub time: f32,
}

/// How fast `Follow::Time` runs when the paint's scroll is still, in cycles per second.
const DEFAULT_TIME_SPEED: f32 = 0.25;

/// `x` if it is a real number, else 0.
fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// The unit vector pointing `degrees` round from +x towards +y (y points down).
fn dir(degrees: f32) -> Vec2 {
    let r = degrees.to_radians();
    Vec2::new(r.cos(), r.sin())
}

/// `x` brought into 0 to 1 the way `repeat` says.
fn wrap(x: f32, repeat: Repeat) -> f32 {
    let x = finite(x);
    let fract = |v: f32| v - v.floor();
    let t = match repeat {
        Repeat::Clamp => x,
        Repeat::Repeat => fract(x),
        Repeat::Mirror => 1.0 - (fract(x / 2.0) * 2.0 - 1.0).abs(),
    };
    t.clamp(0.0, 1.0)
}

/// The gradient position a ball is painted at, 0 to 1, after the follow, the scroll and the
/// repeat. Follows that pick a colour straight from the stops (Step, NearestBall) give the place
/// of that stop. Never NaN, whatever the inputs.
pub fn offset(p: &Paint, i: &Inputs) -> f32 {
    let s = finite(p.span).max(1e-3);
    let stops = &p.colours.stops;
    let n = stops.len().max(1);
    let mut repeat = p.repeat;
    let mut scrolls = true;
    let raw = match p.follow {
        Follow::Place | Follow::Start => {
            let pos = match (p.follow, p.space) {
                (Follow::Start, _) => i.start,
                (_, Space::World) => i.place,
                (_, Space::Screen) => i.place_screen,
            };
            match p.shape {
                PlaceShape::Direction => (pos - p.centre).dot(dir(p.angle)) / s + 0.5,
                PlaceShape::Centre => (pos - p.centre).length() / s,
            }
        }
        Follow::Order => {
            if i.count <= 1 {
                0.0
            } else {
                i.index as f32 / (i.count - 1) as f32
            }
        }
        Follow::Speed => i.speed / s,
        Follow::Hits => i.hits as f32 / s,
        Follow::Life => i.life,
        Follow::Path => i.path,
        Follow::Step => {
            return stops.get(i.hits as usize % n).map_or(0.0, |st| finite(st.at).clamp(0.0, 1.0));
        }
        Follow::Beat => {
            repeat = Repeat::Repeat;
            scrolls = false;
            i.beats / n as f32
        }
        Follow::Time => {
            scrolls = false;
            i.time * if p.scroll == 0.0 { DEFAULT_TIME_SPEED } else { p.scroll }
        }
        Follow::NearestBall => return 0.0,
    };
    let t = if scrolls { finite(raw) + finite(i.time) * finite(p.scroll) } else { finite(raw) };
    wrap(t, repeat)
}

/// The colour of stop `k`, white when the gradient has none.
fn stop_from(p: &Paint, k: usize) -> Color {
    p.colours.stops.get(k).map_or(Color::WHITE, |s| s.color)
}

/// The same in linear light.
fn stop_colour(p: &Paint, k: usize) -> [f32; 4] {
    stop_from(p, k).to_linear()
}

/// The colour of stop `k - 1` blending into stop `k` as `since_hit` runs to `step_time`; stop 0
/// until the first hit.
fn step_colour(p: &Paint, hits: u32, since_hit: f32) -> [f32; 4] {
    let n = p.colours.stops.len().max(1);
    let k = hits as usize % n;
    if hits == 0 {
        return stop_colour(p, 0);
    }
    let instant = p.colours.transition == Transition::Steps || p.step_time <= 0.0 || p.step_time.is_nan();
    let f = if instant { 1.0 } else { finite(since_hit) / p.step_time };
    if f >= 1.0 {
        return stop_colour(p, k);
    }
    let prev = (hits as usize - 1) % n;
    if f <= 0.0 {
        return stop_colour(p, prev);
    }
    let pair = Gradient {
        stops: vec![Stop { at: 0.0, color: stop_from(p, prev) }, Stop { at: 1.0, color: stop_from(p, k) }],
        transition: p.colours.transition,
    };
    pair.sample(f)
}

/// The colour a ball is painted, in linear light. Never NaN, whatever the inputs.
pub fn colour_at(p: &Paint, i: &Inputs) -> [f32; 4] {
    match p.follow {
        Follow::NearestBall => i.nearest.unwrap_or_else(|| stop_colour(p, 0)),
        Follow::Step => match i.hit_colour {
            Some(c) if p.take_hit_colour => c,
            _ => step_colour(p, i.hits, i.since_hit),
        },
        // Hard bands step one stop per beat, whatever the stops' places.
        Follow::Beat if p.colours.transition == Transition::Steps => {
            let n = p.colours.stops.len().max(1);
            stop_colour(p, finite(i.beats).max(0.0).floor() as usize % n)
        }
        _ => p.colours.sample(offset(p, i)),
    }
}

/// The video-clock time of event step `step`.
fn seconds(step: u64, sps: u32) -> f64 {
    step as f64 / sps.max(1) as f64
}

/// The last step at or before `time` seconds.
fn step_at(time: f64, sps: u32) -> u64 {
    (time * sps.max(1) as f64).floor().max(0.0) as u64
}

/// Whole beats so far at `video_time` (seconds) plus how far it is to the next beat, 0 to 1;
/// no fraction after the last beat. Beat events sit on the video clock, `steps_per_second` to
/// the second. Costs one pass over the events up to `video_time`, so call it once a frame, not
/// once a ball.
pub fn beats_at(log: &Log, video_time: f64, steps_per_second: u32) -> f32 {
    let now = step_at(if video_time.is_finite() { video_time } else { 0.0 }, steps_per_second);
    let upto = log.events().partition_point(|e| e.step <= now);
    let mut whole = 0u32;
    let mut last = 0.0f64;
    for e in &log.events()[..upto] {
        if e.kind == EventKind::Beat {
            whole += 1;
            last = seconds(e.step, steps_per_second);
        }
    }
    let next = log.events()[upto..].iter().find(|e| e.kind == EventKind::Beat);
    let fraction = match next {
        Some(e) => {
            let (a, b) = (last, seconds(e.step, steps_per_second));
            if b > a { ((video_time - a) / (b - a)).clamp(0.0, 1.0) as f32 } else { 0.0 }
        }
        None => 0.0,
    };
    whole as f32 + if fraction.is_finite() { fraction } else { 0.0 }
}

/// The latest hit on ball `index` of `thing` at or before `time` seconds on the simulation
/// clock: when it happened and what the ball hit (None when the hit names no other thing). A
/// ball hit by another counts too, with the other as the mover. Walks back from `time` and stops
/// at the first match, so it costs the hits since that one, not the whole log.
pub fn last_hit(
    log: &Log,
    thing: ThingId,
    index: usize,
    time: f64,
    steps_per_second: u32,
) -> Option<(f64, Option<ThingId>)> {
    last_hit_at(log, thing, index, time, steps_per_second).map(|(at, other, _)| (at, other))
}

/// [`last_hit`] with where the hit happened, for reading the colour of what was hit there.
pub fn last_hit_at(
    log: &Log,
    thing: ThingId,
    index: usize,
    time: f64,
    steps_per_second: u32,
) -> Option<(f64, Option<ThingId>, Vec2)> {
    let now = step_at(if time.is_finite() { time } else { 0.0 }, steps_per_second);
    let events = log.events();
    let upto = events.partition_point(|e| e.step <= now);
    let me = (thing, index as u32);
    events[..upto].iter().rev().filter(|e| e.kind == EventKind::Hit).find_map(|e| {
        let at = seconds(e.step, steps_per_second);
        if (e.a.thing, e.a.item) == me {
            Some((at, e.b.map(|b| b.thing), e.point))
        } else if e.b.is_some_and(|b| (b.thing, b.item) == me) {
            Some((at, Some(e.a.thing), e.point))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Choice, Color, Gradient, Transition};
    use bpe_runtime::{Event, EventKind, Party};

    const RED: Color = Color::rgb(255, 0, 0);
    const GREEN: Color = Color::rgb(0, 255, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    fn paint(follow: Follow) -> Paint {
        Paint { follow, colours: Gradient::even(&[RED, BLUE]), ..Paint::default() }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn near4(a: [f32; 4], b: [f32; 4]) -> bool {
        (0..4).all(|k| near(a[k], b[k]))
    }

    #[test]
    fn place_direction_runs_along_the_angle() {
        let p = Paint { angle: 0.0, span: 1000.0, ..paint(Follow::Place) };
        let at = |x: f32| offset(&p, &Inputs { place: Vec2::new(x, 0.0), ..Inputs::default() });
        assert!(near(at(-500.0), 0.0) && near(at(500.0), 1.0) && near(at(0.0), 0.5));
        // 90 degrees runs downwards (+y).
        let down = Paint { angle: 90.0, ..p };
        let t = offset(&down, &Inputs { place: Vec2::new(0.0, 500.0), ..Inputs::default() });
        assert!(near(t, 1.0));
    }

    #[test]
    fn centre_shape_measures_distance() {
        let p = Paint { shape: PlaceShape::Centre, centre: Vec2::new(10.0, 0.0), span: 100.0, ..paint(Follow::Place) };
        assert!(near(offset(&p, &Inputs { place: Vec2::new(60.0, 0.0), ..Inputs::default() }), 0.5));
    }

    #[test]
    fn screen_space_reads_place_screen() {
        let p = Paint { angle: 0.0, span: 1000.0, space: Space::Screen, ..paint(Follow::Place) };
        let i = Inputs { place: Vec2::new(-500.0, 0.0), place_screen: Vec2::new(500.0, 0.0), ..Inputs::default() };
        assert!(near(offset(&p, &i), 1.0));
    }

    #[test]
    fn start_uses_spawn_position() {
        let p = Paint { angle: 0.0, span: 1000.0, ..paint(Follow::Start) };
        let i = Inputs { place: Vec2::new(500.0, 0.0), start: Vec2::new(-500.0, 0.0), ..Inputs::default() };
        assert!(near(offset(&p, &i), 0.0));
    }

    #[test]
    fn order_spreads_first_to_last() {
        let p = paint(Follow::Order);
        let at = |index| offset(&p, &Inputs { index, count: 5, ..Inputs::default() });
        assert!(near(at(0), 0.0) && near(at(2), 0.5) && near(at(4), 1.0));
    }

    #[test]
    fn zero_spans_and_single_balls_give_the_first_colour() {
        let single = Inputs { index: 0, count: 1, ..Inputs::default() };
        assert!(near(offset(&paint(Follow::Order), &single), 0.0));
        assert!(near4(colour_at(&paint(Follow::Order), &single), RED.to_linear()));
        for follow in [Follow::Place, Follow::Start, Follow::Speed, Follow::Hits] {
            let p = Paint { span: 0.0, ..paint(follow) };
            let i =
                Inputs { place: Vec2::splat(7.0), start: Vec2::splat(-3.0), speed: 50.0, hits: 3, ..Inputs::default() };
            assert!(colour_at(&p, &i).iter().all(|c| c.is_finite()), "{follow:?}");
            let o = offset(&p, &i);
            assert!((0.0..=1.0).contains(&o), "{follow:?} gave {o}");
        }
        let nan = Inputs { speed: f32::NAN, place: Vec2::NAN, ..Inputs::default() };
        for name in Follow::NAMES {
            let follow = Follow::from_name(name).unwrap();
            assert!(colour_at(&paint(follow), &nan).iter().all(|c| c.is_finite()), "{follow:?}");
        }
        let none = Inputs { count: 0, ..Inputs::default() };
        assert!(near(offset(&paint(Follow::Order), &none), 0.0));
    }

    #[test]
    fn repeat_and_mirror_wrap() {
        let i = Inputs { life: 1.25, ..Inputs::default() };
        let with = |repeat| offset(&Paint { repeat, ..paint(Follow::Life) }, &i);
        assert!(near(with(Repeat::Clamp), 1.0));
        assert!(near(with(Repeat::Repeat), 0.25));
        assert!(near(with(Repeat::Mirror), 0.75));
        let neg = Inputs { life: -0.25, ..Inputs::default() };
        assert!(near(offset(&Paint { repeat: Repeat::Repeat, ..paint(Follow::Life) }, &neg), 0.75));
    }

    #[test]
    fn scroll_moves_the_offset_over_time() {
        let p = Paint { scroll: 0.5, repeat: Repeat::Repeat, ..paint(Follow::Life) };
        let at = |time| offset(&p, &Inputs { life: 0.1, time, ..Inputs::default() });
        assert!(near(at(0.0), 0.1) && near(at(1.0), 0.6));
        // Time follows by itself at its scroll speed, and at a default speed when still.
        let t = Paint { scroll: 0.5, repeat: Repeat::Repeat, ..paint(Follow::Time) };
        assert!(near(offset(&t, &Inputs { time: 1.0, ..Inputs::default() }), 0.5));
        let still = Paint { scroll: 0.0, repeat: Repeat::Repeat, ..paint(Follow::Time) };
        assert!(near(offset(&still, &Inputs { time: 2.0, ..Inputs::default() }), 0.5));
    }

    fn three() -> Paint {
        Paint { follow: Follow::Step, colours: Gradient::even(&[RED, GREEN, BLUE]), step_time: 0.2, ..Paint::default() }
    }

    #[test]
    fn step_on_hit_walks_the_stops() {
        let p = three();
        let at = |hits, since_hit| colour_at(&p, &Inputs { hits, since_hit, ..Inputs::default() });
        assert!(near4(at(0, 5.0), RED.to_linear()), "no hits yet shows the first stop");
        assert!(near4(at(4, 1.0), GREEN.to_linear()), "4 mod 3 = stop 1 after step_time");
        assert!(near4(at(4, 0.0), at(3, 5.0)), "just after the hit it is still the previous stop (stop 0)");
        assert!(near4(at(4, 0.0), RED.to_linear()));
        let mid = at(4, 0.1);
        let want =
            [(RED.to_linear()[0] + GREEN.to_linear()[0]) / 2.0, (RED.to_linear()[1] + GREEN.to_linear()[1]) / 2.0];
        assert!(near(mid[0], want[0]) && near(mid[1], want[1]));
        // Wrapping round: hit 3 goes from the last stop back to the first.
        assert!(near4(at(3, 0.0), BLUE.to_linear()) && near4(at(3, 1.0), RED.to_linear()));
    }

    #[test]
    fn steps_transition_and_zero_step_time_are_instant() {
        let mut p = three();
        p.colours.transition = Transition::Steps;
        assert!(near4(colour_at(&p, &Inputs { hits: 1, since_hit: 0.0, ..Inputs::default() }), GREEN.to_linear()));
        let q = Paint { step_time: 0.0, ..three() };
        assert!(near4(colour_at(&q, &Inputs { hits: 1, since_hit: 0.0, ..Inputs::default() }), GREEN.to_linear()));
    }

    #[test]
    fn take_hit_colour_wins_when_known() {
        let p = Paint { take_hit_colour: true, ..three() };
        let c = [0.1, 0.2, 0.3, 1.0];
        let i = Inputs { hits: 1, since_hit: 5.0, hit_colour: Some(c), ..Inputs::default() };
        assert_eq!(colour_at(&p, &i), c);
        let unknown = Inputs { hit_colour: None, ..i };
        assert!(near4(colour_at(&p, &unknown), GREEN.to_linear()));
        let off = Paint { take_hit_colour: false, ..p };
        assert!(near4(colour_at(&off, &i), GREEN.to_linear()));
    }

    #[test]
    fn beat_steps_one_stop_per_beat() {
        let mut p = Paint { follow: Follow::Beat, colours: Gradient::even(&[RED, GREEN, BLUE]), ..Paint::default() };
        p.colours.transition = Transition::Steps;
        let at = |beats| colour_at(&p, &Inputs { beats, ..Inputs::default() });
        assert!(
            near4(at(0.0), RED.to_linear()) && near4(at(1.0), GREEN.to_linear()) && near4(at(2.0), BLUE.to_linear())
        );
        assert!(near4(at(3.0), RED.to_linear()), "wraps after the last stop");
        assert!(near4(at(1.9), GREEN.to_linear()));
        // Smooth: a whole cycle takes n beats and repeats, whatever the repeat setting.
        let s = Paint { follow: Follow::Beat, repeat: Repeat::Clamp, ..paint(Follow::Beat) };
        assert!(near(offset(&s, &Inputs { beats: 1.0, ..Inputs::default() }), 0.5));
        assert!(near(offset(&s, &Inputs { beats: 2.5, ..Inputs::default() }), 0.25));
    }

    #[test]
    fn nearest_ball_returns_it_or_the_first_stop() {
        let p = paint(Follow::NearestBall);
        let c = [0.5, 0.5, 0.5, 1.0];
        assert_eq!(colour_at(&p, &Inputs { nearest: Some(c), ..Inputs::default() }), c);
        assert!(near4(colour_at(&p, &Inputs::default()), RED.to_linear()));
    }

    fn ev(step: u64, kind: EventKind, a: (u32, u32), b: Option<(u32, u32)>) -> Event {
        let party = |(t, item)| Party { thing: ThingId(t), item };
        Event { step, kind, a: party(a), b: b.map(party), point: Vec2::ZERO, normal: Vec2::ZERO, speed: 0.0 }
    }

    #[test]
    fn beats_at_counts_beat_events_on_the_video_clock() {
        let mut log = Log::default();
        log.push(ev(120, EventKind::Beat, (0, 0), None));
        log.push(ev(120, EventKind::Hit, (1, 0), Some((2, 0))));
        log.push(ev(240, EventKind::Beat, (0, 0), None));
        assert_eq!(beats_at(&log, 0.75, 240), 1.5);
        assert_eq!(beats_at(&log, 0.25, 240), 0.5);
        assert_eq!(beats_at(&log, 0.5, 240), 1.0);
        assert_eq!(beats_at(&log, 1.0, 240), 2.0);
        assert_eq!(beats_at(&log, 5.0, 240), 2.0, "no fraction after the last beat");
        assert_eq!(beats_at(&Log::default(), 3.0, 240), 0.0);
        assert!(beats_at(&log, f64::NAN, 240).is_finite());
    }

    #[test]
    fn last_hit_finds_the_latest_hit_on_a_ball() {
        let mut log = Log::default();
        log.push(ev(10, EventKind::Hit, (1, 3), Some((9, 0))));
        log.push(ev(20, EventKind::Hit, (1, 4), Some((9, 0))));
        // Ball 3 was the one hit, here.
        log.push(Event { point: Vec2::new(7.0, 8.0), ..ev(30, EventKind::Hit, (2, 0), Some((1, 3))) });
        log.push(ev(40, EventKind::Spawn, (1, 3), None));
        log.push(ev(50, EventKind::Hit, (1, 3), None));
        let sps = 10;
        assert_eq!(last_hit(&log, ThingId(1), 3, 10.0, sps), Some((5.0, None)));
        assert_eq!(last_hit(&log, ThingId(1), 3, 4.0, sps), Some((3.0, Some(ThingId(2)))));
        assert_eq!(last_hit(&log, ThingId(1), 3, 2.5, sps), Some((1.0, Some(ThingId(9)))));
        assert_eq!(last_hit(&log, ThingId(1), 3, 0.5, sps), None);
        assert_eq!(last_hit(&log, ThingId(1), 7, 10.0, sps), None);
        let (_, _, point) = last_hit_at(&log, ThingId(1), 3, 4.0, sps).unwrap();
        assert_eq!(point, Vec2::new(7.0, 8.0));
    }
}
