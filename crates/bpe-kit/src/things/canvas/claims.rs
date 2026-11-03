//! Colour war: the claim map beside a Paint canvas's grid. Each cell holds the team that claimed
//! it, balls claim ground along their path and at their hits, and claimed ground can spread and be
//! taken over on the 30 Hz tick. Teams are handed to the world as they appear.

use std::collections::BTreeMap;
use std::sync::Arc;

use bpe_core::{Color, ThingId};
use bpe_runtime::Team;
use glam::Vec2;

use super::sim::mix;

/// Cells along one side of a block of the map.
const BLOCK: u32 = 32;

/// Who owns each cell of a canvas: 0 nobody, else team index + 1.
#[derive(Clone)]
pub struct ClaimMap {
    cell: f32,
    w: u32,
    h: u32,
    cells: Arc<Vec<u8>>,
    /// Per team (index = team number − 1): cells held.
    counts: Vec<u32>,
    /// Per block of 32×32 cells: claimed cells.
    blocks: Vec<u32>,
}

impl ClaimMap {
    /// An unclaimed map covering `size` canvas px with square cells of `cell` px.
    pub fn new(size: Vec2, cell: f32) -> Self {
        let cell = cell.max(0.5);
        let w = ((size.x / cell).ceil() as u32).max(1);
        let h = ((size.y / cell).ceil() as u32).max(1);
        let blocks = (w.div_ceil(BLOCK) * h.div_ceil(BLOCK)) as usize;
        ClaimMap { cell, w, h, cells: Arc::new(vec![0; (w * h) as usize]), counts: Vec::new(), blocks: vec![0; blocks] }
    }

    /// Cells across and down.
    pub fn dims(&self) -> (u32, u32) {
        (self.w, self.h)
    }

    /// Canvas px along one side of a cell.
    pub fn cell_size(&self) -> f32 {
        self.cell
    }

    /// The team number of cell (x, y): 0 nobody, else team index + 1.
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.cells[(y * self.w + x) as usize]
    }

    /// The cell holding canvas point `p`, or `None` off the map.
    pub fn cell_at(&self, p: Vec2) -> Option<(u32, u32)> {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return None;
        }
        let x = (p.x / self.cell + self.w as f32 / 2.0).floor();
        let y = (p.y / self.cell + self.h as f32 / 2.0).floor();
        (x >= 0.0 && y >= 0.0 && x < self.w as f32 && y < self.h as f32).then_some((x as u32, y as u32))
    }

    /// The canvas point at the middle of cell (x, y).
    pub fn centre_of(&self, x: u32, y: u32) -> Vec2 {
        Vec2::new(
            (x as f32 + 0.5 - self.w as f32 / 2.0) * self.cell,
            (y as f32 + 0.5 - self.h as f32 / 2.0) * self.cell,
        )
    }

    fn block_of(&self, x: u32, y: u32) -> usize {
        ((y / BLOCK) * self.w.div_ceil(BLOCK) + x / BLOCK) as usize
    }

    /// The only place a cell changes, so the counts stay exact.
    fn set(&mut self, x: u32, y: u32, team: u8) {
        let i = (y * self.w + x) as usize;
        let old = self.cells[i];
        if old == team {
            return;
        }
        if self.counts.len() < team as usize {
            self.counts.resize(team as usize, 0);
        }
        if old > 0 {
            self.counts[old as usize - 1] -= 1;
        } else {
            let b = self.block_of(x, y);
            self.blocks[b] += 1;
        }
        if team > 0 {
            self.counts[team as usize - 1] += 1;
        }
        Arc::make_mut(&mut self.cells)[i] = team;
    }

    /// Claims cell (x, y) for `team` (1-based). Without `take_over`, only an unclaimed cell.
    pub fn claim_cell(&mut self, x: u32, y: u32, team: u8, take_over: bool) {
        let old = self.get(x, y);
        if old == team || (old != 0 && !take_over) {
            return;
        }
        self.set(x, y, team);
    }

    /// Claims for `team` (1-based) every cell whose centre is within `r` (+ half a cell when
    /// `tiles`) of the segment a→b, and the cell holding `b`. Without `take_over`, only
    /// unclaimed cells.
    pub fn claim(&mut self, a: Vec2, b: Vec2, r: f32, team: u8, take_over: bool, tiles: bool) {
        let r = r.max(0.0);
        let reach = r + if tiles { self.cell * 0.5 } else { 0.0 };
        let lo = a.min(b) - Vec2::splat(reach + self.cell);
        let hi = a.max(b) + Vec2::splat(reach + self.cell);
        let cell_of = |p: Vec2| {
            let x = (p.x / self.cell + self.w as f32 / 2.0).floor().clamp(0.0, self.w as f32 - 1.0);
            let y = (p.y / self.cell + self.h as f32 / 2.0).floor().clamp(0.0, self.h as f32 - 1.0);
            (x as u32, y as u32)
        };
        let (min, max) = (
            cell_of(lo.clamp(Vec2::splat(-1e7), Vec2::splat(1e7))),
            cell_of(hi.clamp(Vec2::splat(-1e7), Vec2::splat(1e7))),
        );
        let d = b - a;
        let dd = d.length_squared();
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                let p = self.centre_of(x, y);
                let t = if dd > 0.0 { ((p - a).dot(d) / dd).clamp(0.0, 1.0) } else { 0.0 };
                if (p - (a + d * t)).length() <= reach {
                    self.claim_cell(x, y, team, take_over);
                }
            }
        }
        if let Some((x, y)) = self.cell_at(b) {
            self.claim_cell(x, y, team, take_over);
        }
    }

    /// Spreads claimed ground, reading a copy of the map so the order of cells does not matter.
    /// An unclaimed cell with claimed neighbours (left, right, up, down) joins the most common
    /// neighbouring team with chance `grow × 0.5 × count / 4`. With `take_over`, a claimed cell
    /// whose neighbours hold more cells of another team than of its own joins that team with
    /// chance `grow × 0.25 × (theirs − its own) / 4`. Ties go to the lower team number. Only
    /// blocks holding claims, and their neighbours, are visited.
    pub fn grow(&mut self, grow: f32, take_over: bool, tick: u32) {
        if grow <= 0.0 || self.counts.iter().all(|c| *c == 0) {
            return;
        }
        let old = self.cells.clone();
        let (bw, bh) = (self.w.div_ceil(BLOCK), self.h.div_ceil(BLOCK));
        let mut active: Vec<usize> = Vec::new();
        for by in 0..bh {
            for bx in 0..bw {
                if self.blocks[(by * bw + bx) as usize] == 0 {
                    continue;
                }
                for (dx, dy) in [(0i64, 0i64), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (bx as i64 + dx, by as i64 + dy);
                    if nx >= 0 && ny >= 0 && nx < bw as i64 && ny < bh as i64 {
                        active.push((ny as u32 * bw + nx as u32) as usize);
                    }
                }
            }
        }
        active.sort_unstable();
        active.dedup();
        let teams = self.counts.len();
        for block in active {
            let (bx, by) = (block as u32 % bw, block as u32 / bw);
            for y in by * BLOCK..((by + 1) * BLOCK).min(self.h) {
                for x in bx * BLOCK..((bx + 1) * BLOCK).min(self.w) {
                    let me = old[(y * self.w + x) as usize];
                    let mut tally = vec![0u8; teams + 1];
                    for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                        if nx < 0 || ny < 0 || nx >= self.w as i64 || ny >= self.h as i64 {
                            continue;
                        }
                        let t = old[(ny as u32 * self.w + nx as u32) as usize];
                        if t > 0 {
                            tally[t as usize] += 1;
                        }
                    }
                    // The most common neighbouring team other than mine; ties to the lower number.
                    let mut best = 0usize;
                    for t in 1..=teams {
                        if t != me as usize && tally[t] > tally[best] {
                            best = t;
                        }
                    }
                    if best == 0 {
                        continue;
                    }
                    let p = if me == 0 {
                        grow * 0.5 * tally[best] as f32 / 4.0
                    } else if take_over && tally[best] > tally[me as usize] {
                        grow * 0.25 * (tally[best] - tally[me as usize]) as f32 / 4.0
                    } else {
                        continue;
                    };
                    let u = (mix(tick as u64, y * self.w + x) >> 40) as f32 / (1u64 << 24) as f32;
                    if u < p {
                        self.set(x, y, best as u8);
                    }
                }
            }
        }
    }

    /// Each of the first `teams` teams' share of the whole map.
    pub fn shares(&self, teams: usize) -> Vec<f32> {
        let all = (self.w as f32) * (self.h as f32);
        (0..teams).map(|i| self.counts.get(i).copied().unwrap_or(0) as f32 / all).collect()
    }

    /// The map as an sRGB RGBA8 image: a claimed cell is its team's colour with alpha 255,
    /// anything else `[0, 0, 0, 0]`. A team number past `teams` draws as unclaimed.
    pub fn image(&self, teams: &[Team]) -> Vec<u8> {
        let colours: Vec<[u8; 4]> = teams
            .iter()
            .map(|t| {
                let c = Color::from_linear(t.colour);
                [c.r, c.g, c.b, 255]
            })
            .collect();
        let mut out = Vec::with_capacity(self.cells.len() * 4);
        for &c in self.cells.iter() {
            out.extend_from_slice(colours.get((c as usize).wrapping_sub(1)).filter(|_| c > 0).unwrap_or(&[0, 0, 0, 0]));
        }
        out
    }

    /// A 64-bit FNV-1a hash of the map size and every cell.
    pub fn hash(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for b in self.w.to_le_bytes().into_iter().chain(self.h.to_le_bytes()).chain(self.cells.iter().copied()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// A plain name for an sRGB colour: Red, Orange, Brown, Yellow, Lime, Green, Teal, Cyan, Blue,
/// Purple, Pink, White, Grey or Black.
pub fn colour_name(srgb: [u8; 3]) -> &'static str {
    let [r, g, b] = srgb.map(|c| c as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let v = max;
    let s = if max > 0.0 { (max - min) / max } else { 0.0 };
    if v < 0.15 {
        return "Black";
    }
    if s < 0.18 {
        return if v > 0.85 { "White" } else { "Grey" };
    }
    let d = max - min;
    let mut hue = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    if hue >= 360.0 {
        hue -= 360.0;
    }
    match hue {
        h if !(15.0..345.0).contains(&h) => "Red",
        h if h < 45.0 => {
            if v < 0.6 {
                "Brown"
            } else {
                "Orange"
            }
        }
        h if h < 70.0 => "Yellow",
        h if h < 100.0 => "Lime",
        h if h < 160.0 => "Green",
        h if h < 180.0 => "Teal",
        h if h < 200.0 => "Cyan",
        h if h < 255.0 => "Blue",
        h if h < 290.0 => "Purple",
        _ => "Pink",
    }
}

/// What a team is made of.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TeamKey {
    /// One team per claiming thing.
    Thing(ThingId),
    /// One team per colour: r5 << 10 | g5 << 5 | b5 of its sRGB.
    Colour(u16),
}

/// A canvas's colour war.
#[derive(Clone)]
pub struct Claims {
    pub map: ClaimMap,
    pub teams: Vec<Team>,
    /// What each team in `teams` is made of, in the same order.
    pub keys: Vec<TeamKey>,
    /// Per brush thing and item: where its path claim last reached.
    pub last: BTreeMap<(ThingId, u32), Vec2>,
    /// Whether a team was added since the world was last told.
    pub changed: bool,
}

impl Claims {
    pub fn new(map: ClaimMap) -> Self {
        Claims { map, teams: Vec::new(), keys: Vec::new(), last: BTreeMap::new(), changed: false }
    }

    /// The 1-based team number for `key`, adding the team when it is new (named `thing_name` for a
    /// thing, else by its colour; coloured `colour`, linear). `None` when 255 teams exist already.
    pub fn team(&mut self, key: TeamKey, thing_name: &str, colour: [f32; 4]) -> Option<u8> {
        if let Some(i) = self.keys.iter().position(|k| *k == key) {
            return Some(i as u8 + 1);
        }
        if self.teams.len() >= 255 {
            return None;
        }
        let base = match &key {
            TeamKey::Thing(_) => thing_name.trim().to_string(),
            TeamKey::Colour(_) => {
                let c = Color::from_linear(colour);
                colour_name([c.r, c.g, c.b]).to_string()
            }
        };
        let base = if base.is_empty() { "Team".to_string() } else { base };
        let taken = |n: &str| self.teams.iter().any(|t| t.name.eq_ignore_ascii_case(n));
        let mut name = base.clone();
        let mut k = 2;
        while taken(&name) {
            name = format!("{base} {k}");
            k += 1;
        }
        self.teams.push(Team { name, colour: [colour[0], colour[1], colour[2], 1.0] });
        self.keys.push(key);
        self.changed = true;
        Some(self.teams.len() as u8)
    }
}

/// The key of a colour team: the sRGB colour rounded to 5 bits a channel.
pub fn colour_key(linear: [f32; 4]) -> TeamKey {
    let c = Color::from_linear(linear);
    TeamKey::Colour(((c.r as u16 >> 3) << 10) | ((c.g as u16 >> 3) << 5) | (c.b as u16 >> 3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> ClaimMap {
        ClaimMap::new(Vec2::splat(400.0), 4.0)
    }

    fn scan(m: &ClaimMap, team: u8) -> u32 {
        let (w, h) = m.dims();
        (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| m.get(x, y) == team).count() as u32
    }

    #[test]
    fn claim_takes_cells_near_the_path() {
        let mut smooth = ClaimMap::new(Vec2::splat(400.0), 4.0);
        smooth.claim(Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0), 10.0, 1, true, false);
        let at = |m: &ClaimMap, p: Vec2| m.get(m.cell_at(p).unwrap().0, m.cell_at(p).unwrap().1);
        assert_eq!(at(&smooth, Vec2::new(0.0, 5.0)), 1);
        assert_eq!(at(&smooth, Vec2::new(0.0, 25.0)), 0);
        // A 40 px tile whose centre is 25 px from a 10 px path flips with tiles, not without.
        let (mut tiles, mut plain) = (ClaimMap::new(Vec2::splat(400.0), 40.0), ClaimMap::new(Vec2::splat(400.0), 40.0));
        let target = tiles.centre_of(5, 5) + Vec2::new(0.0, 25.0);
        tiles.claim(target - Vec2::new(30.0, 0.0), target + Vec2::new(30.0, 0.0), 10.0, 1, true, true);
        plain.claim(target - Vec2::new(30.0, 0.0), target + Vec2::new(30.0, 0.0), 10.0, 1, true, false);
        assert_eq!(tiles.get(5, 5), 1);
        assert_eq!(plain.get(5, 5), 0);
    }

    #[test]
    fn take_over_off_keeps_ground() {
        let mut m = map();
        m.claim(Vec2::ZERO, Vec2::ZERO, 20.0, 1, false, false);
        let one = m.counts[0];
        m.claim(Vec2::ZERO, Vec2::ZERO, 20.0, 2, false, false);
        assert_eq!(m.counts.get(1).copied().unwrap_or(0), 0);
        assert_eq!(m.counts[0], one);
        m.claim(Vec2::ZERO, Vec2::ZERO, 20.0, 2, true, false);
        assert!(m.counts[1] > 0 && m.counts[0] == 0);
        for t in 1..=2u8 {
            assert_eq!(m.counts[t as usize - 1], scan(&m, t), "team {t}");
        }
    }

    #[test]
    fn grow_spreads_and_is_order_free() {
        let mut a = map();
        a.claim(Vec2::ZERO, Vec2::ZERO, 1.0, 1, true, false);
        let start = a.counts[0];
        for tick in 0..30 {
            a.grow(1.0, true, tick);
        }
        assert!(a.counts[0] > start);
        let cells = |m: &mut ClaimMap, order: &[(u32, u32)]| {
            for &(x, y) in order {
                m.claim_cell(x, y, 1, true);
            }
            for tick in 0..30 {
                m.grow(1.0, true, tick);
            }
        };
        let seed: Vec<(u32, u32)> = vec![(50, 50), (51, 50), (50, 51), (60, 60)];
        let mut rev = seed.clone();
        rev.reverse();
        let (mut p, mut q) = (map(), map());
        cells(&mut p, &seed);
        cells(&mut q, &rev);
        assert_eq!(p.hash(), q.hash());
    }

    #[test]
    fn grow_takes_over_where_outnumbered() {
        let flips = |take: bool| {
            let mut m = map();
            m.claim_cell(50, 50, 1, true);
            for (x, y) in [(49, 50), (51, 50), (50, 49), (50, 51)] {
                m.claim_cell(x, y, 2, true);
            }
            for tick in 0..40 {
                m.grow(1.0, take, tick);
            }
            m.get(50, 50)
        };
        assert_eq!(flips(true), 2);
        assert_eq!(flips(false), 1);
    }

    #[test]
    fn colour_names() {
        let hex = |s: &str| {
            let c = Color::from_hex(s).unwrap();
            [c.r, c.g, c.b]
        };
        for (h, n) in [
            ("#ff3e5c", "Red"),
            ("#3e9bff", "Blue"),
            ("#3ee07a", "Green"),
            ("#ffd23e", "Yellow"),
            ("#ff5ca8", "Pink"),
            ("#3ee0d0", "Teal"),
            ("#b6ff3b", "Lime"),
            ("#ffffff", "White"),
            ("#808080", "Grey"),
            ("#000000", "Black"),
            ("#8b4513", "Brown"),
            ("#ff8c1a", "Orange"),
            ("#a04dff", "Purple"),
            ("#29f0ff", "Cyan"),
        ] {
            assert_eq!(colour_name(hex(h)), n, "{h}");
        }
    }

    #[test]
    fn teams_by_colour_get_names_and_repeats() {
        let mut c = Claims::new(map());
        let lin = |h: &str| Color::from_hex(h).unwrap().to_linear();
        assert_eq!(c.team(colour_key(lin("#ff3e5c")), "", lin("#ff3e5c")), Some(1));
        assert_eq!(c.team(colour_key(lin("#ff2020")), "", lin("#ff2020")), Some(2));
        assert_eq!(c.team(colour_key(lin("#ff3e5c")), "", lin("#ff3e5c")), Some(1));
        assert_eq!(c.teams.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Red", "Red 2"]);
        assert!(c.changed);
    }

    #[test]
    fn teams_by_thing_use_the_things_names() {
        let mut c = Claims::new(map());
        assert_eq!(c.team(TeamKey::Thing(ThingId(2)), "Balls", [1.0, 0.0, 0.0, 1.0]), Some(1));
        assert_eq!(c.team(TeamKey::Thing(ThingId(5)), "balls", [0.0, 1.0, 0.0, 1.0]), Some(2));
        assert_eq!(c.teams[0].name, "Balls");
        assert_eq!(c.teams[1].name, "balls 2");
    }

    #[test]
    fn shares_and_image() {
        let mut m = map();
        m.claim(Vec2::ZERO, Vec2::ZERO, 20.0, 1, true, false);
        let s = m.shares(2);
        assert!(s[0] > 0.0 && s[1] == 0.0);
        let teams = [Team { name: "Red".into(), colour: [1.0, 0.0, 0.0, 1.0] }];
        let img = m.image(&teams);
        let (w, _) = m.dims();
        let (x, y) = m.cell_at(Vec2::ZERO).unwrap();
        let i = ((y * w + x) * 4) as usize;
        assert_eq!(&img[i..i + 4], &[255, 0, 0, 255]);
        assert_eq!(&img[0..4], &[0, 0, 0, 0]);
    }
}
