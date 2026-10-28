//! Analysis results saved next to the program, keyed by the song file's contents.
#![cfg_attr(not(feature = "decode"), allow(dead_code))]

use crate::AudioFeatures;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"BPEF";
/// Bump when the analysis changes, so old results are redone.
const VERSION: u32 = 1;

/// The `cache` folder next to the program.
pub fn cache_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("cache"))
}

/// Analyses a song file, or loads the saved analysis if this exact file was analysed before.
#[cfg(feature = "decode")]
pub fn analyze_file(path: impl AsRef<Path>) -> anyhow::Result<AudioFeatures> {
    analyze_file_in(path.as_ref(), cache_dir().as_deref())
}

#[cfg(not(feature = "decode"))]
pub fn analyze_file(path: impl AsRef<Path>) -> anyhow::Result<AudioFeatures> {
    let _ = path;
    anyhow::bail!("this build can't read song files")
}

#[cfg(feature = "decode")]
pub(crate) fn analyze_file_in(path: &Path, cache: Option<&Path>) -> anyhow::Result<AudioFeatures> {
    let bytes = std::fs::read(path)?;
    let key = format!("{:016x}-{}.bpef", fnv(&bytes), bytes.len());
    if let Some(dir) = cache
        && let Ok(features) = load(&dir.join(&key))
    {
        return Ok(features);
    }
    let features = crate::analyze(&crate::decode(path)?);
    if let Some(dir) = cache {
        // A cache that can't be written only costs time next run.
        let _ = std::fs::create_dir_all(dir).and_then(|_| save(&dir.join(&key), &features));
    }
    Ok(features)
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for chunk in bytes.chunks(8) {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        hash ^= u64::from_le_bytes(word);
        hash = hash.wrapping_mul(0x0100_0000_01b3).rotate_left(5);
    }
    hash
}

pub(crate) fn save(path: &Path, features: &AudioFeatures) -> std::io::Result<()> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&features.rate.to_le_bytes());
    out.extend_from_slice(&features.bpm.to_le_bytes());
    for list in [&features.bass, &features.mid, &features.treble, &features.level, &features.onset] {
        out.extend_from_slice(&(list.len() as u64).to_le_bytes());
        list.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    }
    for list in [&features.beats, &features.downbeats] {
        out.extend_from_slice(&(list.len() as u64).to_le_bytes());
        list.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    }
    let temporary = path.with_extension("part");
    std::fs::File::create(&temporary)?.write_all(&out)?;
    std::fs::rename(temporary, path)
}

pub(crate) fn load(path: &Path) -> std::io::Result<AudioFeatures> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not a saved analysis");
    let mut at = 0usize;
    let mut take = |n: usize| -> std::io::Result<&[u8]> {
        let slice = bytes.get(at..at + n).ok_or_else(bad)?;
        at += n;
        Ok(slice)
    };
    if take(4)? != MAGIC || u32::from_le_bytes(take(4)?.try_into().unwrap()) != VERSION {
        return Err(bad());
    }
    let rate = f32::from_le_bytes(take(4)?.try_into().unwrap());
    let bpm = f32::from_le_bytes(take(4)?.try_into().unwrap());
    let mut f32s = Vec::new();
    for _ in 0..5 {
        let len = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
        let raw = take(len.checked_mul(4).ok_or_else(bad)?)?;
        f32s.push(raw.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>());
    }
    let mut f64s = Vec::new();
    for _ in 0..2 {
        let len = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
        let raw = take(len.checked_mul(8).ok_or_else(bad)?)?;
        f64s.push(raw.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect::<Vec<_>>());
    }
    let [bass, mid, treble, level, onset]: [Vec<f32>; 5] = f32s.try_into().unwrap();
    let [beats, downbeats]: [Vec<f64>; 2] = f64s.try_into().unwrap();
    Ok(AudioFeatures { rate, bass, mid, treble, level, onset, beats, downbeats, bpm })
}
