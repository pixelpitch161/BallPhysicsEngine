//! A water second's kept moments in one file, and restart points. Every file is written whole to
//! a `.part` file and then renamed, so a make that stops (or a crash) leaves every whole file
//! there and never half of one, and a file another take shares (a hard link) is never written in
//! place.

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::drops::WaterState;
use crate::drops::capture::Moment;

const MAGIC: &[u8; 8] = b"BPECHNK1";
const POINT_MAGIC: &[u8; 8] = b"BPEPNT01";

/// What a take's head keeps of a whole chunk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChunkInfo {
    /// The water second it holds (its frames `60 * second` to `60 * second + 59`).
    pub second: u32,
    /// How many moments each of its frames keeps (its own frame and those inside it).
    pub keep: Vec<u8>,
    pub bytes: u64,
}

/// The chunk file of water second `second` in `dir`.
pub fn chunk_path(dir: &Path, second: u32) -> PathBuf {
    dir.join(format!("chunk-{second:06}.bin"))
}

/// The restart point at water frame `frame` in `dir`.
pub fn point_path(dir: &Path, frame: u32) -> PathBuf {
    dir.join(format!("point-{frame:06}.bin"))
}

fn part(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".part");
    path.with_file_name(name)
}

/// Writes a chunk as its moments come.
pub struct ChunkWriter {
    path: PathBuf,
    file: BufWriter<File>,
    at: u64,
    second: u32,
    index: Vec<(Moment, u64, u64)>,
}

impl ChunkWriter {
    pub fn new(dir: &Path, second: u32) -> anyhow::Result<ChunkWriter> {
        let path = chunk_path(dir, second);
        let file = File::create(part(&path)).with_context(|| format!("couldn't write {}", path.display()))?;
        let mut file = BufWriter::with_capacity(1 << 20, file);
        file.write_all(MAGIC)?;
        Ok(ChunkWriter { path, file, at: MAGIC.len() as u64, second, index: Vec::new() })
    }

    pub fn second(&self) -> u32 {
        self.second
    }

    /// Adds a moment (in the order of time).
    pub fn push(&mut self, moment: Moment, bytes: &[u8]) -> anyhow::Result<()> {
        self.file.write_all(bytes)?;
        self.index.push((moment, self.at, bytes.len() as u64));
        self.at += bytes.len() as u64;
        Ok(())
    }

    /// Writes the index, closes the file and gives it its name.
    pub fn finish(mut self) -> anyhow::Result<ChunkInfo> {
        let index_at = self.at;
        let mut keep = vec![0u8; 60];
        for (m, at, len) in &self.index {
            for v in [m.frame, m.steps, m.of] {
                self.file.write_all(&v.to_le_bytes())?;
            }
            self.file.write_all(&at.to_le_bytes())?;
            self.file.write_all(&len.to_le_bytes())?;
            if let Some(k) = m.frame.checked_sub(60 * self.second).and_then(|f| keep.get_mut(f as usize)) {
                *k = k.saturating_add(1);
            }
        }
        self.file.write_all(&(self.index.len() as u64).to_le_bytes())?;
        self.file.write_all(&index_at.to_le_bytes())?;
        self.file.write_all(MAGIC)?;
        let file = self.file.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(part(&self.path), &self.path)?;
        let bytes = std::fs::metadata(&self.path)?.len();
        Ok(ChunkInfo { second: self.second, keep, bytes })
    }

    /// Gives up on the chunk (its `.part` file removed).
    pub fn abandon(self) {
        let path = part(&self.path);
        drop(self.file);
        std::fs::remove_file(path).ok();
    }
}

/// A whole chunk, opened to read its moments.
pub struct Chunk {
    file: File,
    index: Vec<(Moment, u64, u64)>,
}

impl Chunk {
    /// Chunk `second` of the take in `dir`.
    pub fn open(dir: &Path, second: u32) -> anyhow::Result<Chunk> {
        let path = chunk_path(dir, second);
        let mut file = File::open(&path).with_context(|| format!("no {}", path.display()))?;
        let len = file.metadata()?.len();
        ensure!(len >= 32, "{} is cut short", path.display());
        let mut tail = [0u8; 24];
        file.seek(SeekFrom::End(-24))?;
        file.read_exact(&mut tail)?;
        ensure!(&tail[16..] == MAGIC, "{} isn't a whole chunk", path.display());
        let n = u64::from_le_bytes(tail[..8].try_into()?);
        let index_at = u64::from_le_bytes(tail[8..16].try_into()?);
        ensure!(n <= 60 * 8 && index_at.checked_add(n * 28 + 24) == Some(len), "{} is broken", path.display());
        file.seek(SeekFrom::Start(index_at))?;
        let mut raw = vec![0u8; (n * 28) as usize];
        file.read_exact(&mut raw)?;
        let mut index = Vec::with_capacity(n as usize);
        for e in raw.chunks_exact(28) {
            let u = |k: usize| u32::from_le_bytes(e[k..k + 4].try_into().expect("four bytes"));
            let w = |k: usize| u64::from_le_bytes(e[k..k + 8].try_into().expect("eight bytes"));
            let (at, size) = (w(12), w(20));
            if at.checked_add(size).is_none_or(|end| end > index_at) {
                bail!("{} is broken", path.display());
            }
            index.push((Moment { frame: u(0), steps: u(4), of: u(8) }, at, size));
        }
        Ok(Chunk { file, index })
    }

    /// Its moments, in the order of time.
    pub fn moments(&self) -> impl Iterator<Item = Moment> + '_ {
        self.index.iter().map(|e| e.0)
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Moment `i`'s bytes.
    pub fn bytes(&mut self, i: usize) -> anyhow::Result<Vec<u8>> {
        let (_, at, size) = *self.index.get(i).context("no such moment")?;
        self.file.seek(SeekFrom::Start(at))?;
        let mut out = vec![0u8; size as usize];
        self.file.read_exact(&mut out)?;
        Ok(out)
    }
}

/// Writes `state` as the restart point at water frame `frame` in `dir`; returns its bytes.
pub fn write_point(dir: &Path, frame: u32, state: &WaterState) -> anyhow::Result<u64> {
    let path = point_path(dir, frame);
    let mut bytes = POINT_MAGIC.to_vec();
    bytes.extend_from_slice(&lz4_flex::compress_prepend_size(&state.to_bytes()));
    let mut file = File::create(part(&path)).with_context(|| format!("couldn't write {}", path.display()))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(part(&path), &path)?;
    Ok(bytes.len() as u64)
}

/// The restart point at water frame `frame` in `dir`.
pub fn read_point(dir: &Path, frame: u32) -> anyhow::Result<WaterState> {
    let path = point_path(dir, frame);
    let bytes = std::fs::read(&path).with_context(|| format!("no {}", path.display()))?;
    ensure!(bytes.starts_with(POINT_MAGIC), "{} isn't a restart point", path.display());
    // (A garbled size never asks for more room than a body of the most drops a take holds.)
    let size = bytes.get(8..12).map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")) as u64);
    ensure!(size.is_some_and(|s| s <= super::MOST_DROPS as u64 * 200 + (64 << 20)), "the restart point is broken");
    let raw = lz4_flex::decompress_size_prepended(&bytes[8..]).context("the restart point is broken")?;
    WaterState::from_bytes(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bpe-chunk-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_chunk_reads_back_and_a_part_is_never_one() {
        let d = dir("whole");
        let mut w = ChunkWriter::new(&d, 2).unwrap();
        let m = |frame, steps, of| Moment { frame, steps, of };
        w.push(m(120, 0, 1), b"first").unwrap();
        w.push(m(120, 16, 32), b"in between").unwrap();
        w.push(m(121, 0, 1), b"").unwrap();
        assert!(Chunk::open(&d, 2).is_err(), "a chunk being written isn't a chunk");
        let info = w.finish().unwrap();
        assert_eq!(info.second, 2);
        assert_eq!(&info.keep[..3], &[2, 1, 0]);
        let mut c = Chunk::open(&d, 2).unwrap();
        assert_eq!(c.moments().collect::<Vec<_>>(), vec![m(120, 0, 1), m(120, 16, 32), m(121, 0, 1)]);
        assert_eq!(c.bytes(1).unwrap(), b"in between");
        assert_eq!(c.bytes(2).unwrap(), b"");
        let w = ChunkWriter::new(&d, 3).unwrap();
        w.abandon();
        assert!(!part(&chunk_path(&d, 3)).exists() && !chunk_path(&d, 3).exists());
        let path = chunk_path(&d, 2);
        let all = std::fs::read(&path).unwrap();
        std::fs::write(&path, &all[..all.len() - 1]).unwrap();
        assert!(Chunk::open(&d, 2).is_err(), "a chunk cut short is refused");
        std::fs::remove_dir_all(&d).ok();
    }
}
