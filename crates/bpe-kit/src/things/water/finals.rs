//! The finals folder (Water mode part 8): every take Make final has made, each in a folder named by
//! its print, with its head (what it was made from, on what card, how far it is made), its chunks
//! and its restart points (`bpe_water::take`). A take is a cache: anything that doesn't read, was
//! made on another card or by another engine counts as not made.
//!
//! A scene whose physics changed after some time finds the take of the scene as it was: its
//! moments are good up to the change, and a new take of the changed scene starts from the restart
//! point before it, its earlier chunks and points hard links to the old take's (copies where the
//! disk can't link), so nothing is made twice and the old take stays whole.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use bpe_water::take::{ChunkInfo, TAKE_FORMAT, chunk};
use serde::{Deserialize, Serialize};

use super::draft::{Speeds, WaterInputs};

/// What the program is (a take made by another version is remade).
pub const PROGRAM: &str = concat!("ball-studio ", env!("CARGO_PKG_VERSION"));
/// Water frames between restart points (2 s).
pub const POINT_EVERY: u32 = 120;
/// A make that wrote nothing for this long has stopped (its lock is taken over).
const STALE: Duration = Duration::from_secs(120);
const HEAD: &str = "head.ron";
const LOCK: &str = "make.lock";

/// A take's head.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TakeHead {
    pub format: u32,
    pub engine: u64,
    pub program: String,
    /// The card and driver it was made on.
    pub card: String,
    pub print: u64,
    /// Pixels a cell, and the layout's corner and nodes.
    pub cell: f32,
    pub origin: [f32; 2],
    pub cols: u32,
    pub rows: u32,
    pub inputs: WaterInputs,
    /// Water frames asked for (the make plays them all: its moments run to the last one's end).
    pub frames: u32,
    /// The whole chunks, by water second.
    pub chunks: Vec<ChunkInfo>,
    /// The restart points' frames.
    pub points: Vec<u32>,
    /// The most drops it held.
    pub drops: u32,
    /// Wall seconds its makes took, and the water frames they made.
    pub seconds_making: f64,
    pub frames_made: u32,
    /// When it was last used (seconds since 1970).
    pub last_used: u64,
}

impl TakeHead {
    /// A head for a take not made yet.
    pub fn new(
        print: u64,
        cell: f32,
        layout: &bpe_water::Layout,
        inputs: WaterInputs,
        frames: u32,
        card: &str,
    ) -> TakeHead {
        TakeHead {
            format: TAKE_FORMAT,
            engine: bpe_water::take::engine_print(),
            program: PROGRAM.into(),
            card: card.into(),
            print,
            cell,
            origin: layout.origin.to_array(),
            cols: layout.cols,
            rows: layout.rows,
            inputs,
            frames,
            chunks: Vec::new(),
            points: Vec::new(),
            drops: 0,
            seconds_making: 0.0,
            frames_made: 0,
            last_used: now(),
        }
    }

    /// The layout its moments are on.
    pub fn layout(&self) -> bpe_water::Layout {
        bpe_water::Layout { origin: glam::Vec2::from(self.origin), cell: self.cell, cols: self.cols, rows: self.rows }
    }

    /// Whether this program can use it on `card`.
    fn usable(&self, card: &str) -> bool {
        self.format == TAKE_FORMAT
            && self.engine == bpe_water::take::engine_print()
            && self.program == PROGRAM
            && self.card == card
    }

    /// How many moments frame `frame` keeps in its chunks (0: not made).
    pub fn kept(&self, frame: u32) -> u8 {
        let second = frame / 60;
        self.chunks
            .iter()
            .find(|c| c.second == second)
            .and_then(|c| c.keep.get((frame % 60) as usize).copied())
            .unwrap_or(0)
    }

    /// The frames made from the start, each with as many moments as `speeds` asks (or more).
    pub fn good_for(&self, speeds: &Speeds) -> u32 {
        let mut k = 0;
        // (The last frame is kept alone: no frame follows it.)
        let want = |k: u32| if k == self.frames { 1 } else { speeds.keep_for(k).min(8) };
        while k <= self.frames && u32::from(self.kept(k)) >= want(k) {
            k += 1;
        }
        k
    }

    /// The frames made from the start, however many moments each keeps.
    pub fn made(&self) -> u32 {
        let mut k = 0;
        while k <= self.frames && self.kept(k) > 0 {
            k += 1;
        }
        k
    }

    /// The bytes its chunks take.
    pub fn bytes(&self) -> u64 {
        self.chunks.iter().map(|c| c.bytes).sum()
    }

    /// Bytes a drop-moment took (for the room a make needs).
    pub fn bytes_a_drop(&self) -> Option<f64> {
        let moments: u64 = self.chunks.iter().flat_map(|c| c.keep.iter().map(|k| u64::from(*k))).sum();
        (moments > 0 && self.drops > 0).then(|| self.bytes() as f64 / (moments as f64 * f64::from(self.drops)))
    }

    /// Wall seconds a water frame took to make.
    pub fn seconds_a_frame(&self) -> Option<f64> {
        (self.frames_made > 30).then(|| self.seconds_making / f64::from(self.frames_made))
    }
}

/// What a scene's final is now.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// The take its moments come from (this scene's, or one of the scene as it was).
    pub take: Option<TakeHead>,
    /// Whether that take is this scene's own (its print).
    pub own: bool,
    /// The water frames from the start that are right for the scene as it is (moments kept as
    /// thickly as its slow motion asks).
    pub good: u32,
    /// The frames the take has made, right or not.
    pub made: u32,
    /// The frame a make starts from: a restart point at or before `good`, or 0.
    pub restart: u32,
    /// The frames the scene needs (its Length's water frames and the last one's end).
    pub needs: u32,
}

impl Status {
    /// Whether the whole of the scene's final is made and right.
    pub fn complete(&self) -> bool {
        self.good > self.needs
    }

    /// Whether water frame `frame`'s moments are right.
    pub fn good_at(&self, frame: u32) -> bool {
        frame < self.good
    }
}

/// What a find looks for.
#[derive(Clone, Copy, Debug)]
pub struct Wanted<'a> {
    pub print: u64,
    pub inputs: &'a WaterInputs,
    pub cell: f32,
    pub card: &'a str,
    /// Water frames the scene plays.
    pub frames: u32,
    pub speeds: &'a Speeds,
}

/// The finals folder.
#[derive(Clone, Debug)]
pub struct Finals {
    dir: PathBuf,
}

/// Seconds since 1970.
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Writes `bytes` to `path` whole: to a `.part` file, then renamed.
fn write_whole(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut part = path.as_os_str().to_os_string();
    part.push(".part");
    std::fs::write(&part, bytes).with_context(|| format!("couldn't write {}", path.display()))?;
    std::fs::rename(&part, path)?;
    Ok(())
}

impl Finals {
    /// Where finals go: `BPE_FINALS` if set, else the computer's local app data (Windows) or cache
    /// folder: takes are big, and never need to travel with the person's settings.
    pub fn default_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("BPE_FINALS").filter(|d| !d.is_empty()) {
            return PathBuf::from(dir);
        }
        if cfg!(windows)
            && let Some(local) = std::env::var_os("LOCALAPPDATA")
        {
            return PathBuf::from(local).join("Ball Studio").join("Finals");
        }
        if let Some(cache) = std::env::var_os("XDG_CACHE_HOME").filter(|d| !d.is_empty()) {
            return PathBuf::from(cache).join("ball-studio").join("finals");
        }
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(".cache").join("ball-studio").join("finals");
        }
        std::env::temp_dir().join("ball-studio-finals")
    }

    pub fn at(dir: impl Into<PathBuf>) -> Finals {
        Finals { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The folder of the take with `print`.
    pub fn take_dir(&self, print: u64) -> PathBuf {
        self.dir.join(format!("{print:016x}"))
    }

    /// The head of the take with `print`, if it reads.
    pub fn head(&self, print: u64) -> Option<TakeHead> {
        read_head(&self.take_dir(print))
    }

    /// Every take's head that reads.
    pub fn heads(&self) -> Vec<TakeHead> {
        let Ok(list) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        list.filter_map(|e| e.ok()).filter_map(|e| read_head(&e.path())).collect()
    }

    /// What the final of the scene `wanted` describes is now: its own take, or the take of the
    /// scene as it was whose moments are right the longest.
    pub fn find(&self, wanted: &Wanted) -> Status {
        let needs = wanted.frames;
        let mut best: Option<(TakeHead, bool, u32)> = None;
        for head in self.heads() {
            if !head.usable(wanted.card) || head.cell.to_bits() != wanted.cell.to_bits() {
                continue;
            }
            let own = head.print == wanted.print;
            let mut good = head.good_for(wanted.speeds);
            if !own {
                // (Frames read their inputs at their start: a change at t leaves the frames before it.)
                if let Some(t) = head.inputs.earliest_change(wanted.inputs) {
                    good = good.min((t * 60.0).floor().max(0.0) as u32);
                }
            }
            let better = match &best {
                None => true,
                Some((_, b_own, b_good)) => good > *b_good || (good == *b_good && own && !b_own),
            };
            if better {
                best = Some((head, own, good));
            }
        }
        match best {
            Some((head, own, good)) => {
                let restart = head.points.iter().copied().filter(|p| *p <= good).max().unwrap_or(0);
                // (Another take is no help before its first restart point.)
                if !own && restart == 0 {
                    return Status { take: None, own: false, good: 0, made: 0, restart: 0, needs };
                }
                let made = head.made();
                Status { take: Some(head), own, good, made, restart, needs }
            }
            None => Status { take: None, own: false, good: 0, made: 0, restart: 0, needs },
        }
    }

    /// Starts writing the take `head` describes from frame `from`: its folder made, the lock taken,
    /// and (when `from` is another take's) that take's chunks and restart points before `from`
    /// linked in. A take another maker is writing is refused.
    pub fn begin(&self, mut head: TakeHead, from: Option<(&TakeHead, u32)>) -> anyhow::Result<TakeDir> {
        let dir = self.take_dir(head.print);
        std::fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
        let lock = dir.join(LOCK);
        if let Ok(meta) = std::fs::metadata(&lock) {
            let fresh = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age < STALE);
            if fresh {
                anyhow::bail!("This final is being made by another window or the command line.");
            }
        }
        write_whole(&lock, format!("{}", std::process::id()).as_bytes())?;
        let at = from.map_or(0, |(_, f)| f);
        head.chunks.clear();
        head.points.clear();
        if let Some((old, at)) = from {
            let old_dir = self.take_dir(old.print);
            let share = |name: &Path, to: &Path| -> anyhow::Result<()> {
                if old.print == head.print {
                    return Ok(());
                }
                std::fs::remove_file(to).ok();
                if std::fs::hard_link(name, to).is_err() {
                    std::fs::copy(name, to).with_context(|| format!("couldn't copy {}", name.display()))?;
                }
                Ok(())
            };
            for c in old.chunks.iter().filter(|c| (c.second + 1) * 60 <= at) {
                share(&chunk::chunk_path(&old_dir, c.second), &chunk::chunk_path(&dir, c.second))?;
                head.chunks.push(c.clone());
            }
            for p in old.points.iter().copied().filter(|p| *p <= at) {
                share(&chunk::point_path(&old_dir, p), &chunk::point_path(&dir, p))?;
                head.points.push(p);
            }
            head.drops = head.drops.max(old.drops);
            if old.print == head.print {
                head.seconds_making = old.seconds_making;
                head.frames_made = old.frames_made;
            }
        }
        head.last_used = now();
        let take = TakeDir { dir, head, from: at };
        take.write_head()?;
        Ok(take)
    }

    /// Notes that the take with `print` was used now (the 30-day clean-up keeps it).
    pub fn touch(&self, print: u64) {
        let dir = self.take_dir(print);
        if let Some(mut head) = read_head(&dir)
            && now().saturating_sub(head.last_used) > 3600
        {
            head.last_used = now();
            if let Ok(text) = ron::to_string(&head) {
                write_whole(&dir.join(HEAD), text.as_bytes()).ok();
            }
        }
    }

    /// Removes the take with `print`.
    pub fn clear(&self, print: u64) -> anyhow::Result<()> {
        let dir = self.take_dir(print);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Removes every take; returns the bytes freed.
    pub fn clear_all(&self) -> anyhow::Result<u64> {
        let size = self.size();
        if self.dir.exists() {
            for e in std::fs::read_dir(&self.dir)?.filter_map(|e| e.ok()) {
                if e.path().is_dir() {
                    std::fs::remove_dir_all(e.path())?;
                }
            }
        }
        Ok(size)
    }

    /// Removes the takes not used for `days` days (and folders with no head that are a day old);
    /// returns how many.
    pub fn sweep(&self, days: u64) -> usize {
        let Ok(list) = std::fs::read_dir(&self.dir) else { return 0 };
        let mut gone = 0;
        for e in list.filter_map(|e| e.ok()) {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            let old = match read_head(&path) {
                Some(head) => now().saturating_sub(head.last_used) > days * 86_400,
                None => e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|a| a.as_secs() > 86_400),
            };
            if old && std::fs::remove_dir_all(&path).is_ok() {
                gone += 1;
            }
        }
        gone
    }

    /// The bytes every take's files take.
    pub fn size(&self) -> u64 {
        fn walk(dir: &Path) -> u64 {
            let Ok(list) = std::fs::read_dir(dir) else { return 0 };
            list.filter_map(|e| e.ok())
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => walk(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        }
        walk(&self.dir)
    }
}

fn read_head(dir: &Path) -> Option<TakeHead> {
    let text = std::fs::read_to_string(dir.join(HEAD)).ok()?;
    ron::from_str(&text).ok()
}

/// A take being written (its lock held).
pub struct TakeDir {
    pub dir: PathBuf,
    pub head: TakeHead,
    /// The frame this make started from.
    pub from: u32,
}

impl TakeDir {
    fn write_head(&self) -> anyhow::Result<()> {
        let text = ron::to_string(&self.head)?;
        write_whole(&self.dir.join(HEAD), text.as_bytes())
    }

    /// A chunk is whole: the head counts it (and the lock is kept fresh).
    pub fn chunk_done(&mut self, info: ChunkInfo, drops: u32, seconds: f64, frames: u32) -> anyhow::Result<()> {
        self.head.chunks.retain(|c| c.second != info.second);
        self.head.chunks.push(info);
        self.head.chunks.sort_by_key(|c| c.second);
        self.head.drops = self.head.drops.max(drops);
        self.head.seconds_making += seconds;
        self.head.frames_made += frames;
        self.write_head()?;
        write_whole(&self.dir.join(LOCK), format!("{}", std::process::id()).as_bytes())
    }

    /// A restart point is written.
    pub fn point_done(&mut self, frame: u32) -> anyhow::Result<()> {
        if !self.head.points.contains(&frame) {
            self.head.points.push(frame);
            self.head.points.sort_unstable();
        }
        self.write_head()
    }

    /// Lets the lock go.
    pub fn release(self) -> TakeHead {
        self.head.clone()
    }
}

impl Drop for TakeDir {
    fn drop(&mut self) {
        std::fs::remove_file(self.dir.join(LOCK)).ok();
    }
}

/// The room (bytes) a make of `frames` water frames with about `drops` drops needs, at
/// `bytes_a_drop` a drop-moment (3.5 before any make), its moments `moments` in all.
pub fn room_needed(moments: u64, drops: u32, frames: u32, bytes_a_drop: Option<f64>) -> u64 {
    let a_drop = bytes_a_drop.unwrap_or(3.5);
    let chunks = moments as f64 * f64::from(drops) * a_drop;
    // (A restart point every 2 s: about 40 bytes a drop compressed.)
    let points = f64::from(frames / POINT_EVERY + 1) * f64::from(drops) * 40.0;
    (chunks + points) as u64
}

/// What a make keeps clear on the disk.
pub const ROOM_LEFT: u64 = 2 << 30;

/// The free room (bytes) on the disk `dir` is on (or the nearest folder above it that is there).
pub fn free_space(dir: &Path) -> Option<u64> {
    let mut at = dir;
    while !at.exists() {
        at = at.parent()?;
    }
    free_at(at)
}

#[cfg(windows)]
fn free_at(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is a NUL-ended path and `free` a u64 the call writes; the other outputs may be null.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(unix)]
fn free_at(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is NUL-ended and `stat` a statvfs the call fills.
    let ok = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    #[allow(clippy::unnecessary_cast)]
    (ok == 0).then(|| stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(not(any(windows, unix)))]
fn free_at(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::things::water::draft::InputPart;

    fn folder(name: &str) -> Finals {
        let d = std::env::temp_dir().join(format!("bpe-finals-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        Finals::at(d)
    }

    fn inputs(rate: u64) -> WaterInputs {
        let part = |name: &str, from: f64, fixed: u64| InputPart { name: name.into(), from, fixed, keys: Vec::new() };
        WaterInputs { parts: vec![part("world", 0.0, 1), part("tap:0", 4.95, rate)] }
    }

    /// A take of `seconds` whole chunks (moments one a frame) and points every 2 s.
    fn made(finals: &Finals, print: u64, inputs: WaterInputs, seconds: u32, card: &str) -> TakeHead {
        let layout = bpe_water::Layout::cover(glam::Vec2::ZERO, glam::Vec2::splat(100.0), 8.0);
        let head = TakeHead::new(print, 8.0, &layout, inputs, seconds * 60, card);
        let mut take = finals.begin(head, None).unwrap();
        for s in 0..=seconds {
            let path = chunk::chunk_path(&take.dir, s);
            std::fs::write(&path, format!("chunk {s} of {print}")).unwrap();
            let mut keep = vec![1u8; 60];
            if s == seconds {
                keep = vec![0u8; 60];
                keep[0] = 1;
            }
            take.chunk_done(ChunkInfo { second: s, keep, bytes: 100 }, 1000, 1.0, 60).unwrap();
            if s > 0 && (s * 60) % POINT_EVERY == 0 {
                std::fs::write(chunk::point_path(&take.dir, s * 60), "point").unwrap();
                take.point_done(s * 60).unwrap();
            }
        }
        take.release()
    }

    fn wanted<'a>(print: u64, inputs: &'a WaterInputs, card: &'a str, speeds: &'a Speeds) -> Wanted<'a> {
        Wanted { print, inputs, cell: 8.0, card, frames: 360, speeds }
    }

    #[test]
    fn a_take_is_found_by_its_print_and_another_cards_is_not() {
        let finals = folder("found");
        let speeds = Speeds::real_time(6.0);
        let head = made(&finals, 7, inputs(1), 6, "card A");
        let a = inputs(1);
        let found = finals.find(&wanted(7, &a, "card A", &speeds));
        assert!(found.own && found.complete(), "{found:?}");
        assert_eq!(found.take.as_ref(), Some(&head));
        let other = finals.find(&wanted(7, &a, "card B", &speeds));
        assert!(other.take.is_none() && !other.complete(), "a take from another card is not used");
        // (Slow motion asks for more moments: the frames from 2 s on are too thin.)
        let mut slow = crate::things::water::draft::slow::tests_support::constant(0.5, 6.0);
        let thin = finals.find(&wanted(7, &a, "card A", &slow));
        assert_eq!(thin.good, 0, "{thin:?}");
        slow = Speeds::real_time(6.0);
        assert!(finals.find(&wanted(7, &a, "card A", &slow)).complete());
        std::fs::write(finals.take_dir(7).join(HEAD), "not a head").unwrap();
        assert!(finals.find(&wanted(7, &a, "card A", &speeds)).take.is_none(), "a broken head is skipped");
        std::fs::remove_dir_all(finals.dir()).ok();
    }

    #[test]
    fn a_changed_scene_starts_from_the_point_before_its_change() {
        let finals = folder("prefix");
        let speeds = Speeds::real_time(6.0);
        let old = made(&finals, 7, inputs(1), 6, "card A");
        let changed = inputs(2);
        let found = finals.find(&wanted(8, &changed, "card A", &speeds));
        assert!(!found.own);
        assert_eq!(found.good, 297, "good up to the tap's change at 4.95 s");
        assert_eq!(found.restart, 240, "from the point at 4 s");
        let layout = old.layout();
        let head = TakeHead::new(8, 8.0, &layout, changed.clone(), 360, "card A");
        let take = finals.begin(head, Some((found.take.as_ref().unwrap(), found.restart))).unwrap();
        assert_eq!(take.head.chunks.len(), 4, "its first four seconds come from the old take");
        assert_eq!(take.head.points, vec![120, 240]);
        let new_dir = take.dir.clone();
        assert_eq!(std::fs::read_to_string(chunk::chunk_path(&new_dir, 3)).unwrap(), "chunk 3 of 7");
        assert!(!chunk::chunk_path(&new_dir, 4).exists());
        drop(take);
        assert!(finals.find(&wanted(7, &inputs(1), "card A", &speeds)).complete(), "the old take stays whole");
        // (A change before the first point is no help.)
        let mut early = inputs(1);
        early.parts[0].fixed = 99;
        assert!(finals.find(&wanted(9, &early, "card A", &speeds)).take.is_none());
        std::fs::remove_dir_all(finals.dir()).ok();
    }

    #[test]
    fn a_lock_is_held_while_a_take_is_made() {
        let finals = folder("lock");
        let layout = bpe_water::Layout::cover(glam::Vec2::ZERO, glam::Vec2::splat(100.0), 8.0);
        let head = TakeHead::new(5, 8.0, &layout, inputs(1), 60, "card A");
        let take = finals.begin(head.clone(), None).unwrap();
        assert!(finals.begin(head.clone(), None).is_err(), "a take being made is refused");
        drop(take);
        assert!(finals.begin(head, None).is_ok(), "and taken once let go");
        std::fs::remove_dir_all(finals.dir()).ok();
    }

    #[test]
    fn old_takes_are_swept_and_clear_empties() {
        let finals = folder("sweep");
        made(&finals, 1, inputs(1), 1, "card A");
        let mut old = made(&finals, 2, inputs(1), 1, "card A");
        old.last_used = now() - 31 * 86_400;
        std::fs::write(finals.take_dir(2).join(HEAD), ron::to_string(&old).unwrap()).unwrap();
        assert!(finals.size() > 0);
        assert_eq!(finals.sweep(30), 1);
        assert!(finals.head(1).is_some() && finals.head(2).is_none());
        assert!(free_space(finals.dir()).is_some_and(|f| f > 0), "the disk's free room reads");
        assert!(finals.clear_all().unwrap() > 0);
        assert!(finals.heads().is_empty());
        std::fs::remove_dir_all(finals.dir()).ok();
    }
}
