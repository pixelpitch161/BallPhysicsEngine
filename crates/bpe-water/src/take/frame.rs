//! A kept moment's drops on disk: by the layout's cells, a byte a cell for how many drops it holds
//! (255 then four more bytes for 255 or more), then for each drop in cell order its place in its cell
//! (x and y, 1/256 of a cell), its speed (x and y, a signed byte each on a log scale), its kind's low
//! byte, and its colour mix (the share of another liquid it carries, of 255, and that liquid), each
//! as its own plane, the whole compressed with lz4.

use anyhow::{Context, bail, ensure};
use bpe_core::maths::Maths;

use crate::drop::STOPS;
use crate::solid::Layout;

const MAGIC: &[u8; 4] = b"BPF2";
/// The planes a drop has.
const PLANES: usize = 7;

/// A speed (pixels a second) as the pack pass stores it: `round(15 ln(1 + |v| / 4))`, at most 127,
/// its sign kept, as a two's-complement byte.
pub fn speed_byte(v: f32) -> u8 {
    if !v.is_finite() {
        return 0;
    }
    let m = (15.0 * (v.abs() / 4.0).det_ln_1p()).round().min(127.0) as i32;
    (if v < 0.0 { -m } else { m }) as i8 as u8
}

/// The speed (pixels a second) a speed byte stands for.
pub fn byte_speed(b: u8) -> f32 {
    let s = b as i8;
    let m = 4.0 * (f32::from(s.unsigned_abs()) / 15.0).det_exp_m1();
    if s < 0 { -m } else { m }
}

/// A moment's drops unpacked: place and speed (grid units, a second), kind, and colour mix (as
/// [`crate::Drop::mix`], its share kept to 1/255).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TakeFrame {
    pub xv: Vec<[f32; 4]>,
    pub kinds: Vec<u32>,
    pub mixes: Vec<u32>,
}

impl TakeFrame {
    pub fn len(&self) -> usize {
        self.xv.len()
    }

    pub fn is_empty(&self) -> bool {
        self.xv.is_empty()
    }
}

/// Packs and unpacks moments, keeping its working room between them.
#[derive(Default)]
pub struct FrameCodec {
    counts: Vec<u32>,
    order: Vec<u32>,
    planes: Vec<u8>,
}

impl FrameCodec {
    pub fn new() -> FrameCodec {
        FrameCodec::default()
    }

    /// A moment's bytes from the drops the card packed (`[cell | kind << 24, place and speed, mix]`,
    /// all ones for one not in the course) on a layout of `cells` cells.
    pub fn encode(&mut self, packed: &[[u32; 3]], cells: u32) -> Vec<u8> {
        let cells = cells as usize;
        self.counts.clear();
        self.counts.resize(cells + 1, 0);
        let mut live = 0u32;
        for d in packed {
            let c = (d[0] & 0xFF_FFFF) as usize;
            if d[0] != u32::MAX && c < cells {
                self.counts[c + 1] += 1;
                live += 1;
            }
        }
        self.planes.clear();
        for &n in &self.counts[1..] {
            if n < 255 {
                self.planes.push(n as u8);
            } else {
                self.planes.push(255);
                self.planes.extend_from_slice(&(n - 255).to_le_bytes());
            }
        }
        // (Where each cell's drops start, then each drop in cell order.)
        for c in 0..cells {
            self.counts[c + 1] += self.counts[c];
        }
        self.order.clear();
        self.order.resize(live as usize, 0);
        for (i, d) in packed.iter().enumerate() {
            let c = (d[0] & 0xFF_FFFF) as usize;
            if d[0] != u32::MAX && c < cells {
                self.order[self.counts[c] as usize] = i as u32;
                self.counts[c] += 1;
            }
        }
        let at = self.planes.len();
        self.planes.resize(at + PLANES * live as usize, 0);
        let n = live as usize;
        for (k, &i) in self.order.iter().enumerate() {
            let [w0, w1, mix] = packed[i as usize];
            let b = w1.to_le_bytes();
            self.planes[at + k] = b[0];
            self.planes[at + n + k] = b[1];
            self.planes[at + 2 * n + k] = b[2];
            self.planes[at + 3 * n + k] = b[3];
            self.planes[at + 4 * n + k] = (w0 >> 24) as u8;
            // (Its share of 65535 to a byte; a share that rounds to nothing is unmixed.)
            let share = ((mix >> 16) + 128) / 257;
            self.planes[at + 5 * n + k] = share as u8;
            self.planes[at + 6 * n + k] = if share == 0 { 0 } else { (mix & 15) as u8 };
        }
        let mut out = Vec::with_capacity(12 + self.planes.len() / 2);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&live.to_le_bytes());
        out.extend_from_slice(&(cells as u32).to_le_bytes());
        out.extend_from_slice(&lz4_flex::compress_prepend_size(&self.planes));
        out
    }

    /// The moment `bytes` holds, on `layout` (the take's). A broken moment is an error.
    pub fn decode(&mut self, bytes: &[u8], layout: &Layout) -> anyhow::Result<TakeFrame> {
        ensure!(bytes.len() >= 12 && &bytes[..4] == MAGIC, "this isn't a moment of a take");
        let live = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
        let cells = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        ensure!(live <= super::MOST_DROPS, "the moment is broken");
        let across = layout.cols.saturating_sub(1) as usize;
        ensure!(cells == across * layout.rows.saturating_sub(1) as usize, "the moment is of another course");
        // (A garbled size never asks for more room than a moment of these drops can take.)
        let size = bytes.get(12..16).map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")) as usize);
        ensure!(size.is_some_and(|s| s <= 5 * cells + PLANES * live), "the moment is broken");
        let planes = lz4_flex::decompress_size_prepended(&bytes[12..]).context("the moment is broken")?;
        let mut at = 0;
        let mut out = TakeFrame {
            xv: Vec::with_capacity(live),
            kinds: Vec::with_capacity(live),
            mixes: Vec::with_capacity(live),
        };
        let body = |at: usize| -> anyhow::Result<&[u8]> {
            let end =
                at.checked_add(PLANES * live).filter(|e| *e <= planes.len()).context("the moment is cut short")?;
            Ok(&planes[at..end])
        };
        // (First the counts, to find where the drops' planes start.)
        let mut by_cell = Vec::with_capacity(cells.min(planes.len()));
        for _ in 0..cells {
            let n = *planes.get(at).context("the moment is cut short")?;
            at += 1;
            let n = if n == 255 {
                let more = planes.get(at..at + 4).context("the moment is cut short")?;
                at += 4;
                255 + u32::from_le_bytes(more.try_into()?)
            } else {
                u32::from(n)
            };
            by_cell.push(n);
        }
        let total: u64 = by_cell.iter().map(|n| u64::from(*n)).sum();
        if total != live as u64 {
            bail!("the moment's counts don't add up");
        }
        let drops = body(at)?;
        let (px, py, sx, sy, kinds, shares, others) = (
            &drops[..live],
            &drops[live..2 * live],
            &drops[2 * live..3 * live],
            &drops[3 * live..4 * live],
            &drops[4 * live..5 * live],
            &drops[5 * live..6 * live],
            &drops[6 * live..],
        );
        let per_px = 1.0 / layout.cell.max(1e-6);
        let mut k = 0;
        for (c, &n) in by_cell.iter().enumerate() {
            let (cx, cy) = ((c % across.max(1)) as f32, (c / across.max(1)) as f32);
            for _ in 0..n {
                let x = cx + (f32::from(px[k]) + 0.5) / 256.0;
                let y = cy + (f32::from(py[k]) + 0.5) / 256.0;
                out.xv.push([x, y, byte_speed(sx[k]) * per_px, byte_speed(sy[k]) * per_px]);
                out.kinds.push(u32::from(kinds[k]) | STOPS);
                let share = u32::from(shares[k]) * 257;
                out.mixes.push(if share == 0 { 0 } else { (u32::from(others[k]) & 15) | (share << 16) });
                k += 1;
            }
        }
        Ok(out)
    }
}

/// The most cells a take's layout may have (a cell is kept in 24 bits).
pub const MOST_CELLS: u64 = 0xFF_FFFF;

/// Whether a take can be made on `layout`.
pub fn fits(layout: &Layout) -> bool {
    u64::from(layout.cols.saturating_sub(1)) * u64::from(layout.rows.saturating_sub(1)) <= MOST_CELLS
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    fn pack(x: f32, y: f32, vx: f32, vy: f32, kind: u32, layout: &Layout) -> [u32; 3] {
        let across = layout.cols - 1;
        let (cx, cy) = (x.floor().min((layout.cols - 2) as f32), y.floor().min((layout.rows - 2) as f32));
        let q = |f: f32| ((f * 256.0).clamp(0.0, 255.0)) as u32;
        let cell = cx as u32 + cy as u32 * across;
        let s = |v: f32| u32::from(speed_byte(v));
        [cell | ((kind & 255) << 24), q(x - cx) | (q(y - cy) << 8) | (s(vx) << 16) | (s(vy) << 24), 0]
    }

    #[test]
    fn speeds_keep_within_seven_percent() {
        assert_eq!(speed_byte(0.0), 0);
        assert_eq!(byte_speed(0), 0.0);
        for v in [0.5f32, 3.0, 40.0, 300.0, 2000.0, 15000.0] {
            for v in [v, -v] {
                let back = byte_speed(speed_byte(v));
                assert!((back - v).abs() <= 0.07 * v.abs() + 0.15, "{v} came back as {back}");
            }
        }
        assert_eq!(speed_byte(1e9), 127, "the fastest is held at the top");
        assert_eq!(speed_byte(f32::NAN), 0);
    }

    #[test]
    fn a_moment_reads_back_what_was_kept() {
        let layout = Layout::cover(Vec2::ZERO, Vec2::new(40.0, 30.0), 2.0);
        let (cols, rows) = ((layout.cols - 1) as f32, (layout.rows - 1) as f32);
        let mut drops = vec![
            pack(0.0, 0.0, 10.0, -20.0, 0, &layout),
            pack(cols, rows, 0.0, 0.0, 3 | crate::AIR, &layout),
            pack(5.99, 7.01, -400.0, 1.0, 1, &layout),
            [u32::MAX, u32::MAX, u32::MAX],
        ];
        // (A cell of 300 drops.)
        for k in 0..300 {
            drops.push(pack(3.0 + k as f32 / 300.0, 4.5, 0.0, 50.0, 2, &layout));
        }
        let cells = (layout.cols - 1) * (layout.rows - 1);
        let mut codec = FrameCodec::new();
        let bytes = codec.encode(&drops, cells);
        let back = codec.decode(&bytes, &layout).unwrap();
        assert_eq!(back.len(), drops.len() - 1, "the drop not in the course is left out");
        let has = |x: f32, y: f32, kind: u32| {
            back.xv
                .iter()
                .zip(&back.kinds)
                .any(|(p, k)| (p[0] - x).abs() < 0.004 && (p[1] - y).abs() < 0.004 && *k == kind | STOPS)
        };
        assert!(has(0.0, 0.0, 0) && has(cols, rows, 3 | crate::AIR) && has(5.99, 7.01, 1));
        let fast = back.xv.iter().find(|p| (p[0] - 5.99).abs() < 0.004).unwrap();
        assert!((fast[2] * layout.cell + 400.0).abs() < 28.0, "speed {}", fast[2] * layout.cell);
        assert_eq!(back.xv.iter().filter(|p| (p[1] - 4.5).abs() < 0.004).count(), 300);
        assert!(back.mixes.iter().all(|m| *m == 0), "unmixed drops read back unmixed");
        assert!(codec.decode(&bytes[..bytes.len() - 3], &layout).is_err(), "a moment cut short is an error");
        let mut garbled = bytes.clone();
        garbled[20] ^= 0x5A;
        let _ = codec.decode(&garbled, &layout); // (an error or other drops, never a panic)
        let other = Layout::cover(Vec2::ZERO, Vec2::new(60.0, 30.0), 2.0);
        assert!(codec.decode(&bytes, &other).is_err(), "a moment of another course is refused");
    }

    #[test]
    fn a_moment_keeps_each_drops_colour_mix() {
        let layout = Layout::cover(Vec2::ZERO, Vec2::new(40.0, 30.0), 2.0);
        let mixed = |x: f32, other: usize, share: f32| {
            let mut p = pack(x, 2.5, 0.0, 0.0, 0, &layout);
            p[2] = crate::mix_word(other, share);
            p
        };
        let drops = [mixed(1.5, 3, 0.5), mixed(2.5, 15, 1.0), mixed(3.5, 2, 0.001), mixed(4.5, 1, 0.3)];
        let cells = (layout.cols - 1) * (layout.rows - 1);
        let mut codec = FrameCodec::new();
        let bytes = codec.encode(&drops, cells);
        let back = codec.decode(&bytes, &layout).unwrap();
        let mix_at = |x: f32| {
            let k = back.xv.iter().position(|p| (p[0] - x).abs() < 0.01).unwrap();
            let m = back.mixes[k];
            (m & 15, f32::from((m >> 16) as u16) / 65535.0)
        };
        let (o, s) = mix_at(1.5);
        assert!(o == 3 && (s - 0.5).abs() <= 1.0 / 255.0, "{o} {s}");
        assert_eq!(mix_at(2.5), (15, 1.0));
        assert_eq!(mix_at(3.5), (0, 0.0), "a share under a 255th reads back unmixed");
        let (o, s) = mix_at(4.5);
        assert!(o == 1 && (s - 0.3).abs() <= 1.0 / 255.0, "{o} {s}");
    }
}
