//! Live words (Text part 2): values typed into a Text's words as tokens in braces, read at the moment
//! drawn from the world, the log and the rules, so scrubbing and export agree. `{hits}`, `{hits of
//! Balls}`, `{balls left}`, `{time}`, `{countdown 30}`, `{beat}`, `{bar}`, `{counter score}`, `{health}`,
//! `{health percent of Shape}`, `{ball number}`, `{ball name}` and `{standings down 5}`; each may end in a
//! number format after `|` (`|000`, `|0.00`, `|m:ss`). Anything else in braces is shown as typed.

use std::collections::BTreeMap;

use bpe_core::{Scene, ThingId};
use bpe_runtime::{EventKind, Log, World};

/// A piece of words: plain text, or a value to read.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text(String),
    Value(Token, Format),
}

/// What a standings list ranks the balls by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankBy {
    /// How far below where it started.
    Down,
    Hits,
    Health,
    Size,
    Speed,
}

impl RankBy {
    /// Its word in a token.
    pub fn word(self) -> &'static str {
        match self {
            RankBy::Down => "down",
            RankBy::Hits => "hits",
            RankBy::Health => "health",
            RankBy::Size => "size",
            RankBy::Speed => "speed",
        }
    }

    fn of_word(words: &str) -> Option<RankBy> {
        Some(match words {
            "down" | "furthest down" => RankBy::Down,
            "hits" | "most hits" => RankBy::Hits,
            "health" => RankBy::Health,
            "size" => RankBy::Size,
            "speed" => RankBy::Speed,
            _ => return None,
        })
    }
}

/// A value a token reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// Hits: of a thing, else the ridden ball's or every hit.
    Hits { of: Option<String> },
    /// Live balls: of a thing, else of every thing.
    BallsLeft { of: Option<String> },
    /// Seconds since the start.
    Time,
    /// Seconds left of this many.
    Countdown(f64),
    /// The beat within its bar, from 1.
    Beat,
    /// The bar, from 1.
    Bar,
    /// A rule counter.
    Counter(String),
    /// Health points (or percent): of a thing, else of the ridden ball or the Text's parent.
    Health { of: Option<String>, percent: bool },
    /// The ridden ball's number.
    BallNumber,
    /// The ridden ball's name (its colour).
    BallName,
    /// A ranked list of balls, one line each.
    Standings { by: RankBy, rows: u32, of: Option<String> },
}

/// A clock format: `m:ss` (or `h:mm:ss`) with some decimals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub hours: bool,
    pub decimals: u32,
}

/// How a number is written: at least `whole` digits (leading zeros), `decimals` after the point
/// (`None`: the token's own), or as a clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Format {
    pub whole: u32,
    pub decimals: Option<u32>,
    pub clock: Option<Clock>,
}

impl Format {
    /// The format written after `|`; `None` when it isn't one.
    pub fn parse(s: &str) -> Option<Format> {
        let s = s.trim().to_lowercase();
        if s.is_empty() {
            return Some(Format::default());
        }
        if s.contains(':') {
            if !s.chars().all(|c| matches!(c, 'h' | 'm' | 's' | ':' | '.' | '0')) {
                return None;
            }
            let decimals = s.split_once('.').map_or(0, |(_, d)| d.chars().filter(|c| *c == '0').count() as u32);
            return Some(Format { clock: Some(Clock { hours: s.starts_with('h'), decimals }), ..Format::default() });
        }
        if !s.chars().all(|c| matches!(c, '0' | '.')) || s.matches('.').count() > 1 {
            return None;
        }
        let (whole, decimals) = match s.split_once('.') {
            Some((w, d)) => (w.len() as u32, Some(d.len() as u32)),
            None => (s.len() as u32, Some(0)),
        };
        Some(Format { whole: whole.min(12), decimals: decimals.map(|d| d.min(6)), clock: None })
    }
}

/// The most rows a standings list shows.
pub const MOST_ROWS: u32 = 50;

/// `words` cut into text and values.
pub fn parse(words: &str) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut text = String::new();
    let mut rest = words;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let inner = &after[..close];
        if inner.contains('{') {
            // An opening brace with another before its close: the first is plain text.
            text.push_str(&rest[..open + 1]);
            rest = after;
            continue;
        }
        match token_of(inner) {
            Some((token, format)) => {
                text.push_str(&rest[..open]);
                if !text.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                out.push(Piece::Value(token, format));
            }
            None => text.push_str(&rest[..open + 1 + close + 1]),
        }
        rest = &after[close + 1..];
    }
    text.push_str(rest);
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    out
}

/// Whether `words` hold any value to read.
pub fn has_tokens(words: &str) -> bool {
    words.contains('{') && parse(words).iter().any(|p| matches!(p, Piece::Value(..)))
}

/// The standings lists in `words`.
pub fn standings_in(words: &str) -> Vec<(RankBy, Option<String>)> {
    if !words.contains('{') {
        return Vec::new();
    }
    parse(words)
        .into_iter()
        .filter_map(|p| match p {
            Piece::Value(Token::Standings { by, of, .. }, _) => Some((by, of)),
            _ => None,
        })
        .collect()
}

/// The token in `inner` (what is between the braces) and its format.
fn token_of(inner: &str) -> Option<(Token, Format)> {
    let (body, format) = match inner.rsplit_once('|') {
        Some((b, f)) => (b, Format::parse(f)?),
        None => (inner, Format::default()),
    };
    let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = body.to_lowercase();
    // "<head> of <name>", the name as typed.
    let of = |head: &str| -> Option<Option<String>> {
        if lower == head {
            return Some(None);
        }
        let rest = lower.strip_prefix(head)?.strip_prefix(" of ")?;
        let name = body[body.len() - rest.len()..].trim();
        (!name.is_empty()).then(|| Some(name.to_string()))
    };
    let token = if let Some(of) = of("hits") {
        Token::Hits { of }
    } else if let Some(of) = of("balls left") {
        Token::BallsLeft { of }
    } else if lower == "time" {
        Token::Time
    } else if let Some(n) = lower.strip_prefix("countdown ") {
        let n: f64 = n.trim().parse().ok()?;
        if !n.is_finite() {
            return None;
        }
        Token::Countdown(n)
    } else if lower == "beat" {
        Token::Beat
    } else if lower == "bar" {
        Token::Bar
    } else if lower.starts_with("counter ") {
        let name = body["counter ".len()..].trim();
        if name.is_empty() {
            return None;
        }
        Token::Counter(name.to_string())
    } else if let Some(of) = of("health percent") {
        Token::Health { of, percent: true }
    } else if let Some(of) = of("health") {
        Token::Health { of, percent: false }
    } else if lower == "ball number" {
        Token::BallNumber
    } else if lower == "ball name" {
        Token::BallName
    } else if let Some(rest) = lower.strip_prefix("standings ") {
        let (spec, of) = match rest.split_once(" of ") {
            Some((s, _)) => {
                let name = body[body.len() - (rest.len() - s.len() - 4)..].trim();
                (s, (!name.is_empty()).then(|| name.to_string()))
            }
            None => (rest, None),
        };
        let words: Vec<&str> = spec.split_whitespace().collect();
        let (by, rows) = match words.last().and_then(|w| w.parse::<u32>().ok()) {
            Some(rows) => (words[..words.len() - 1].join(" "), rows),
            None => (words.join(" "), 5),
        };
        Token::Standings { by: RankBy::of_word(&by)?, rows: rows.clamp(1, MOST_ROWS), of }
    } else {
        return None;
    };
    Some((token, format))
}

/// `v` written as `format` says, with `decimals` when it doesn't say; `up` rounds up at the last digit
/// (a countdown).
pub fn format_value(v: f64, format: Format, decimals: u32, up: bool) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let round = |v: f64, d: u32| {
        let k = 10f64.powi(d as i32);
        // A hair under, so 2.0000001 rounds up to 2, not 3.
        if up { (v * k - 1e-6).ceil() / k } else { (v * k).round() / k }
    };
    if let Some(clock) = format.clock {
        let total = round(v.max(0.0), clock.decimals);
        let whole = total.floor();
        let part = total - whole;
        let secs = whole as u64;
        let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
        let mut out = if clock.hours { format!("{h}:{m:02}:{s:02}") } else { format!("{}:{s:02}", secs / 60) };
        if clock.decimals > 0 {
            let frac = format!("{:.*}", clock.decimals as usize, part);
            out.push_str(frac.trim_start_matches('0'));
        }
        return out;
    }
    let d = format.decimals.unwrap_or(decimals);
    let r = round(v, d);
    let text = format!("{:.*}", d as usize, r.abs());
    let digits = text.split('.').next().map_or(0, str::len) as u32;
    let zeros = "0".repeat(format.whole.saturating_sub(digits) as usize);
    let sign = if r < 0.0 && r != 0.0 { "-" } else { "" };
    format!("{sign}{zeros}{text}")
}

/// What words read their values with.
#[derive(Clone, Copy)]
pub struct LiveCx<'a> {
    /// The scene as saved (names, the tree).
    pub tree: &'a Scene,
    pub world: &'a World,
    pub log: Option<&'a Log>,
    /// The video's clock, seconds.
    pub time: f64,
    /// The Text.
    pub me: ThingId,
    /// The ball a riding copy sits on (thing, body row).
    pub ball: Option<(ThingId, usize)>,
    /// The balls' names, when worked out.
    pub names: Option<&'a BallNames>,
    /// How often standings re-sort, seconds.
    pub resort: f32,
}

/// The balls' names (their colours), by thing and body row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BallNames(pub BTreeMap<(ThingId, usize), String>);

impl BallNames {
    /// Ball `row` of `thing`'s name: its colour's, else "Ball N".
    pub fn of(&self, world: &World, thing: ThingId, row: usize) -> String {
        self.0.get(&(thing, row)).cloned().unwrap_or_else(|| {
            let n = world.bodies.get(&thing).and_then(|b| b.number.get(row)).map_or(row as u32, |n| *n);
            format!("Ball {}", n + 1)
        })
    }

    /// Names from each ball's colour: `colour_of(thing, row)` (linear), for the live balls of
    /// `things`. Two balls of a thing with the same colour name get their numbers after it.
    pub fn from_colours(world: &World, things: &[ThingId], colour_of: &dyn Fn(ThingId, usize) -> Option<[f32; 4]>) -> Self {
        let mut out = BTreeMap::new();
        for &thing in things {
            let Some(b) = world.bodies.get(&thing) else { continue };
            let named: Vec<(usize, &'static str)> = (0..b.len())
                .filter(|&r| b.alive[r])
                .filter_map(|r| colour_of(thing, r).map(|c| (r, colour_name([c[0], c[1], c[2]]))))
                .collect();
            for &(r, name) in &named {
                let twins = named.iter().filter(|(_, n)| *n == name).count();
                let n = b.number.get(r).copied().unwrap_or(r as u32) + 1;
                let name = if twins > 1 { format!("{name} {n}") } else { name.to_string() };
                out.insert((thing, r), name);
            }
        }
        BallNames(out)
    }
}

/// Words with their values read: the words to lay out, and how far (in lines) each paragraph
/// stands off its place while a standings list re-sorts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filled {
    pub words: String,
    pub moves: Vec<(u32, f32)>,
}

/// `pieces` with their values read in `cx`.
pub fn fill(pieces: &[Piece], cx: &LiveCx) -> Filled {
    let mut out = Filled::default();
    for piece in pieces {
        match piece {
            Piece::Text(t) => out.words.push_str(t),
            Piece::Value(Token::Standings { by, rows, of }, format) => {
                let paragraph = out.words.matches('\n').count() as u32;
                let lines = standings(cx, *by, *rows, of.as_deref(), *format);
                for (k, (line, offset)) in lines.iter().enumerate() {
                    if k > 0 {
                        out.words.push('\n');
                    }
                    out.words.push_str(line);
                    if offset.abs() > 1e-4 {
                        out.moves.push((paragraph + k as u32, *offset));
                    }
                }
            }
            Piece::Value(token, format) => out.words.push_str(&read(cx, token, *format)),
        }
    }
    out
}

/// What can't be read shows as this.
pub const UNREADABLE: &str = "-";

/// The first thing called `name` in tree order.
pub fn named(tree: &Scene, name: &str) -> Option<ThingId> {
    let name = name.trim();
    tree.order.iter().copied().find(|id| tree.things.get(id).is_some_and(|t| t.name.trim().eq_ignore_ascii_case(name)))
}

/// The step the world is at, the last whose events count.
fn now_step(cx: &LiveCx) -> u64 {
    cx.world.step
}

/// Hits so far with `thing` on either side (`None`: every hit).
fn hits_of(cx: &LiveCx, thing: Option<ThingId>) -> f64 {
    let Some(log) = cx.log else {
        // Without the log, the balls' own counts.
        return match thing {
            Some(t) => cx.world.bodies.get(&t).map_or(0, |b| b.hits.iter().map(|h| *h as u64).sum()) as f64,
            None => 0.0,
        };
    };
    let step = now_step(cx);
    let events = log.events();
    let end = events.partition_point(|e| e.step <= step);
    events[..end]
        .iter()
        .filter(|e| e.kind == EventKind::Hit)
        .filter(|e| thing.is_none_or(|t| e.a.thing == t || e.b.is_some_and(|b| b.thing == t)))
        .count() as f64
}

/// Reads one value.
fn read(cx: &LiveCx, token: &Token, format: Format) -> String {
    let number = |v: f64| format_value(v, format, 0, false);
    let thing_named = |of: &Option<String>| of.as_deref().map(|n| named(cx.tree, n));
    match token {
        Token::Hits { of } => match (thing_named(of), cx.ball) {
            (Some(None), _) => UNREADABLE.into(),
            (Some(Some(t)), _) => number(hits_of(cx, Some(t))),
            (None, Some((t, row))) => {
                number(cx.world.bodies.get(&t).and_then(|b| b.hits.get(row)).map_or(0.0, |h| *h as f64))
            }
            (None, None) => number(hits_of(cx, None)),
        },
        Token::BallsLeft { of } => {
            let count = |t: &ThingId| cx.world.bodies.get(t).map_or(0, |b| b.alive_count());
            match thing_named(of) {
                Some(None) => UNREADABLE.into(),
                Some(Some(t)) => number(count(&t) as f64),
                None => number(cx.world.bodies.keys().map(count).sum::<usize>() as f64),
            }
        }
        Token::Time => format_value(cx.time, format, 1, false),
        Token::Countdown(n) => format_value((n - cx.time).max(0.0), format, 0, true),
        Token::Beat | Token::Bar => {
            let beat = cx.world.beat_seconds;
            if !(beat.is_finite() && beat > 0.0) {
                return number(0.0);
            }
            let beats = (cx.time.max(0.0) / beat + 1e-9).floor() as u64;
            let per_bar = cx.world.beats_in_bar.max(1) as u64;
            number(if matches!(token, Token::Beat) { beats % per_bar + 1 } else { beats / per_bar + 1 } as f64)
        }
        Token::Counter(name) => number(cx.world.rules().map_or(0.0, |r| r.counter(name))),
        Token::Health { of, percent } => {
            let target = match thing_named(of) {
                Some(None) => return UNREADABLE.into(),
                Some(Some(t)) => Some((t, None)),
                None => match cx.ball {
                    Some((t, row)) => Some((t, Some(row))),
                    None => cx.tree.things.get(&cx.me).and_then(|t| t.parent).map(|p| (p, None)),
                },
            };
            match target.and_then(|(t, row)| health_of(cx.world, t, row)) {
                Some((now, start)) if *percent => {
                    let share = if start > 0.0 { now / start } else { 0.0 };
                    number(share.clamp(0.0, 1.0) * 100.0)
                }
                Some((now, _)) => number(now),
                None => UNREADABLE.into(),
            }
        }
        Token::BallNumber => match cx.ball {
            Some((t, row)) => {
                number(cx.world.bodies.get(&t).and_then(|b| b.number.get(row)).map_or(row as u32, |n| *n) as f64 + 1.0)
            }
            None => UNREADABLE.into(),
        },
        Token::BallName => match (cx.ball, cx.names) {
            (Some((t, row)), Some(names)) => names.of(cx.world, t, row),
            (Some((t, row)), None) => BallNames::default().of(cx.world, t, row),
            (None, _) => UNREADABLE.into(),
        },
        Token::Standings { .. } => String::new(),
    }
}

/// `thing`'s health now and at the start: its ball `row`'s, its own without balls, or the sum
/// over its live balls. `None` without a Health card.
fn health_of(world: &World, thing: ThingId, row: Option<usize>) -> Option<(f64, f64)> {
    if let Some(row) = row {
        return world.health(thing, Some(row as u32)).map(|hp| (hp.now, hp.start));
    }
    match world.bodies.get(&thing) {
        Some(b) => {
            let mut any = false;
            let (mut now, mut start) = (0.0, 0.0);
            for r in (0..b.len()).filter(|&r| b.alive[r]) {
                if let Some(hp) = world.health(thing, Some(r as u32)) {
                    any = true;
                    now += hp.now.max(0.0);
                    start += hp.start;
                }
            }
            any.then_some((now, start))
        }
        None => world.health(thing, None).map(|hp| (hp.now, hp.start)),
    }
}

/// Pixels in a metre, for standings' distances and speeds.
pub const PX_PER_METRE: f64 = 100.0;

/// `n` as a place: 1st, 2nd, 3rd, 4th … 11th, 12th, 13th … 21st.
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// A ball in a standings list.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ranked {
    thing: ThingId,
    row: usize,
    /// Higher ranks first.
    key: f64,
}

/// Where ball `row` of `thing` was `ago` seconds back: from its history when that reaches back so
/// far, else worked back along its velocity.
fn place_ago(world: &World, thing: ThingId, row: usize, ago: f64) -> glam::Vec2 {
    let Some(b) = world.bodies.get(&thing) else { return glam::Vec2::ZERO };
    let (p, v) = (b.position[row], b.velocity[row]);
    if ago <= 0.0 {
        return p;
    }
    let rate = world.steps_per_second as f64;
    let now = world.step as f64;
    let then = now - ago * rate;
    if let Some(h) = world.history.get(&thing)
        && let Some((first, _)) = h.first(row)
        && first <= then
        && let Some(at) = h.place(row, then, now, p)
    {
        return at;
    }
    p - v * ago as f32
}

/// The balls `standings` reads: of `of`, else of every thing with balls, in tree order.
fn ranked_things(cx: &LiveCx, of: Option<&str>) -> Vec<ThingId> {
    match of {
        Some(name) => named(cx.tree, name).filter(|t| cx.world.bodies.contains_key(t)).into_iter().collect(),
        None => cx.tree.order.iter().copied().filter(|t| cx.world.bodies.contains_key(t)).collect(),
    }
}

/// Every live ball of `things` ranked by `by` as it was `ago` seconds back (`hits` the hits each
/// had then), best first; ties in thing-then-row order.
fn rank(cx: &LiveCx, things: &[ThingId], by: RankBy, ago: f64, hits: &BTreeMap<(ThingId, usize), u64>) -> Vec<Ranked> {
    let mut out = Vec::new();
    for &thing in things {
        let Some(b) = cx.world.bodies.get(&thing) else { continue };
        for row in (0..b.len()).filter(|&r| b.alive[r]) {
            let key = match by {
                RankBy::Down => (place_ago(cx.world, thing, row, ago).y - b.start[row].y) as f64,
                RankBy::Hits => hits.get(&(thing, row)).copied().unwrap_or(0) as f64,
                RankBy::Health => health_of(cx.world, thing, Some(row)).map_or(0.0, |(now, _)| now),
                RankBy::Size => b.radius[row] as f64,
                RankBy::Speed => {
                    let dt = 1.0 / 60.0;
                    let (a, z) = (place_ago(cx.world, thing, row, ago + dt), place_ago(cx.world, thing, row, ago));
                    if ago <= 0.0 { b.velocity[row].length() as f64 } else { ((z - a).length() as f64) / dt }
                }
            };
            out.push(Ranked { thing, row, key: if key.is_finite() { key } else { 0.0 } });
        }
    }
    // Stable: ties keep tree and row order.
    out.sort_by(|a, b| b.key.total_cmp(&a.key));
    out
}

/// Each ball's hits by `steps` (one count list a step limit), from the log, counting a ball on
/// either side of a hit; without the log, the balls' own counts for every limit.
fn hits_by(cx: &LiveCx, things: &[ThingId], steps: &[u64]) -> Vec<BTreeMap<(ThingId, usize), u64>> {
    let mut out = vec![BTreeMap::new(); steps.len()];
    let Some(log) = cx.log else {
        for &t in things {
            if let Some(b) = cx.world.bodies.get(&t) {
                for (row, h) in b.hits.iter().enumerate() {
                    for m in &mut out {
                        m.insert((t, row), *h as u64);
                    }
                }
            }
        }
        return out;
    };
    let last = steps.iter().copied().max().unwrap_or(0);
    let events = log.events();
    let end = events.partition_point(|e| e.step <= last);
    for e in events[..end].iter().filter(|e| e.kind == EventKind::Hit) {
        for side in std::iter::once(e.a).chain(e.b) {
            if !things.contains(&side.thing) || !cx.world.bodies.contains_key(&side.thing) {
                continue;
            }
            for (k, &limit) in steps.iter().enumerate() {
                if e.step <= limit {
                    *out[k].entry((side.thing, side.item as usize)).or_default() += 1;
                }
            }
        }
    }
    out
}

/// A standings list's lines, best first, each with how many lines it stands off its place while
/// the list re-sorts (positive: below).
fn standings(cx: &LiveCx, by: RankBy, rows: u32, of: Option<&str>, format: Format) -> Vec<(String, f32)> {
    let things = ranked_things(cx, of);
    let resort = (cx.resort as f64).clamp(0.05, 1.0);
    let rate = cx.world.steps_per_second.max(1) as f64;
    let now = cx.time.max(0.0);
    // The grid moment the list last re-sorted at, and the one before.
    let g = (now / resort).floor() * resort;
    let ago_new = now - g;
    let ago_old = ago_new + resort;
    let step_now = now_step(cx);
    let step_ago = |ago: f64| step_now.saturating_sub((ago * rate).round() as u64);
    let hits = hits_by(cx, &things, &[step_now, step_ago(ago_new), step_ago(ago_old)]);
    let shown = rank(cx, &things, by, ago_new, &hits[1]);
    let before = if g - resort < 0.0 { shown.clone() } else { rank(cx, &things, by, ago_old, &hits[2]) };
    let t = ((now - g) / resort).clamp(0.0, 1.0) as f32;
    let glide = 1.0 - t * t * (3.0 - 2.0 * t);
    let rows = rows.clamp(1, MOST_ROWS) as usize;
    let names = cx.names.cloned().unwrap_or_default();
    shown
        .iter()
        .take(rows)
        .enumerate()
        .map(|(place, r)| {
            let b = &cx.world.bodies[&r.thing];
            let value = match by {
                RankBy::Down => {
                    let d = (b.position[r.row].y - b.start[r.row].y) as f64 / PX_PER_METRE;
                    format!("{}m", format_value(d, format, 1, false))
                }
                RankBy::Hits => format_value(
                    hits[0].get(&(r.thing, r.row)).copied().unwrap_or(0) as f64,
                    format,
                    0,
                    false,
                ),
                RankBy::Health => {
                    format_value(health_of(cx.world, r.thing, Some(r.row)).map_or(0.0, |h| h.0), format, 0, false)
                }
                RankBy::Size => format_value(b.radius[r.row] as f64, format, 0, false),
                RankBy::Speed => {
                    format!("{}m/s", format_value(b.velocity[r.row].length() as f64 / PX_PER_METRE, format, 1, false))
                }
            };
            let name = names.of(cx.world, r.thing, r.row);
            let was = before.iter().position(|o| o.thing == r.thing && o.row == r.row).unwrap_or(rows).min(rows);
            let offset = (was as f32 - place as f32) * glide;
            (format!("{} {name} {value}", ordinal(place + 1)), offset)
        })
        .collect()
}

/// The nearest of a few plain colour names to `rgb` (linear light).
pub fn colour_name(rgb: [f32; 3]) -> &'static str {
    use bpe_core::oklab::{linear_to_oklab, oklab_to_lch};
    let rgb = rgb.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 });
    let [l, c, h] = oklab_to_lch(linear_to_oklab(rgb));
    if c < 0.045 {
        return if l > 0.82 {
            "White"
        } else if l < 0.3 {
            "Black"
        } else {
            "Grey"
        };
    }
    if l < 0.2 {
        return "Black";
    }
    // OKLCH hues (turns) of each name's colour.
    const HUES: [(&str, f32); 10] = [
        ("Red", 0.081),
        ("Orange", 0.152),
        ("Yellow", 0.284),
        ("Lime", 0.36),
        ("Green", 0.395),
        ("Teal", 0.49),
        ("Cyan", 0.545),
        ("Blue", 0.733),
        ("Purple", 0.84),
        ("Pink", 0.96),
    ];
    let gap = |a: f32, b: f32| {
        let d = (a - b).abs();
        d.min(1.0 - d)
    };
    HUES.iter().min_by(|a, b| gap(a.1, h).total_cmp(&gap(b.1, h))).map_or("Grey", |(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(words: &str) -> (Token, Format) {
        match parse(words).as_slice() {
            [Piece::Value(t, f)] => (t.clone(), *f),
            other => panic!("{words}: {other:?}"),
        }
    }

    #[test]
    fn every_token_parses() {
        let table = [
            ("{hits}", Token::Hits { of: None }),
            ("{Hits of Big Balls}", Token::Hits { of: Some("Big Balls".into()) }),
            ("{balls left}", Token::BallsLeft { of: None }),
            ("{balls  left of Rain}", Token::BallsLeft { of: Some("Rain".into()) }),
            ("{time}", Token::Time),
            ("{countdown 30}", Token::Countdown(30.0)),
            ("{beat}", Token::Beat),
            ("{bar}", Token::Bar),
            ("{counter Score}", Token::Counter("Score".into())),
            ("{health}", Token::Health { of: None, percent: false }),
            ("{health of Box}", Token::Health { of: Some("Box".into()), percent: false }),
            ("{health percent of Box}", Token::Health { of: Some("Box".into()), percent: true }),
            ("{ball number}", Token::BallNumber),
            ("{ball name}", Token::BallName),
            ("{standings down 3}", Token::Standings { by: RankBy::Down, rows: 3, of: None }),
            ("{standings most hits 4 of Racers}", Token::Standings { by: RankBy::Hits, rows: 4, of: Some("Racers".into()) }),
            ("{standings speed}", Token::Standings { by: RankBy::Speed, rows: 5, of: None }),
        ];
        for (words, want) in table {
            assert_eq!(value(words).0, want, "{words}");
        }
    }

    #[test]
    fn unknown_braces_stay_as_typed() {
        for words in ["{nope}", "a {b c} d", "{", "}{", "{countdown x}", "{hits|abc}", "{counter }", "{{time}"] {
            let got: String = parse(words)
                .into_iter()
                .map(|p| match p {
                    Piece::Text(t) => t,
                    Piece::Value(..) => "<v>".into(),
                })
                .collect();
            let want = if words == "{{time}" { "{<v>" } else { words };
            assert_eq!(got, want, "{words}");
        }
        assert!(!has_tokens("plain {words}"));
        assert!(has_tokens("Hits: {hits}"));
        assert_eq!(parse("Hits: {hits}!").len(), 3);
    }

    #[test]
    fn formats_write_numbers() {
        let f = |s: &str| Format::parse(s).unwrap();
        assert_eq!(format_value(7.0, f("000"), 0, false), "007");
        assert_eq!(format_value(3.14159, f("0.00"), 0, false), "3.14");
        assert_eq!(format_value(-2.5, f("00.0"), 0, false), "-02.5");
        assert_eq!(format_value(12.34, Format::default(), 1, false), "12.3");
        assert_eq!(format_value(75.0, f("m:ss"), 0, false), "1:15");
        assert_eq!(format_value(75.25, f("m:ss.0"), 0, false), "1:15.3");
        assert_eq!(format_value(3725.0, f("h:mm:ss"), 0, false), "1:02:05");
        // A countdown rounds up, so it shows its start and 1 until it ends.
        assert_eq!(format_value(30.0, Format::default(), 0, true), "30");
        assert_eq!(format_value(29.2, Format::default(), 0, true), "30");
        assert_eq!(format_value(0.01, Format::default(), 0, true), "1");
        assert_eq!(format_value(0.0, Format::default(), 0, true), "0");
        assert_eq!(format_value(59.5, f("m:ss"), 0, true), "1:00");
        assert!(Format::parse("x").is_none());
        assert_eq!(value("{countdown 10|m:ss}").1.clock, Some(Clock { hours: false, decimals: 0 }));
    }

    #[test]
    fn places_read_as_ordinals() {
        let got: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111].iter().map(|n| ordinal(*n)).collect();
        assert_eq!(got, ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st", "111th"]);
    }

    #[test]
    fn colours_have_plain_names() {
        let lin = |r: u8, g: u8, b: u8| {
            let c = bpe_core::Color::rgb(r, g, b).to_linear();
            [c[0], c[1], c[2]]
        };
        let table = [
            ((230, 40, 40), "Red"),
            ((255, 140, 0), "Orange"),
            ((250, 220, 30), "Yellow"),
            ((40, 180, 60), "Green"),
            ((30, 200, 230), "Cyan"),
            ((40, 80, 230), "Blue"),
            ((150, 50, 220), "Purple"),
            ((255, 90, 180), "Pink"),
            ((255, 255, 255), "White"),
            ((10, 10, 10), "Black"),
            ((128, 128, 128), "Grey"),
        ];
        for ((r, g, b), name) in table {
            assert_eq!(colour_name(lin(r, g, b)), name, "{r} {g} {b}");
        }
    }
}
