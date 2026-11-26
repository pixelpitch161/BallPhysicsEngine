//! Drops put on the card in a body's own layout ([`crate::DropBuffers`]), so the drawing draws a
//! list of [`Drop`]s (hand-built sets, a dump of a run, another engine's drops read back) the same
//! way it draws a live body. And the drops files the measuring runs write and the look check reads.

use std::path::Path;

use crate::drop::Drop;
use crate::drops::DropBuffers;

/// Drops on the card, laid out as a body's: place and speed (`xv`), how full (`nf`, always full
/// here), kind, serial and colour (`id`).
pub struct StoredDrops {
    xv: wgpu::Buffer,
    nf: wgpu::Buffer,
    id: wgpu::Buffer,
    count: u32,
}

impl std::fmt::Debug for StoredDrops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StoredDrops({})", self.count)
    }
}

fn buffer(device: &wgpu::Device, queue: &wgpu::Queue, label: &'static str, bytes: &[u8]) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        // (Never empty: a binding needs room for one drop.)
        size: (bytes.len() as u64).max(16),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !bytes.is_empty() {
        queue.write_buffer(&buffer, 0, bytes);
    }
    buffer
}

impl StoredDrops {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, drops: &[Drop]) -> StoredDrops {
        let xv: Vec<[f32; 4]> = drops.iter().map(|d| [d.x[0], d.x[1], d.v[0], d.v[1]]).collect();
        let nf: Vec<[f32; 4]> = drops.iter().map(|_| [0.0, 0.0, 1.0, 0.0]).collect();
        let id: Vec<[u32; 4]> = drops.iter().map(|d| [d.kind, d.serial, d.colour, 0]).collect();
        StoredDrops {
            xv: buffer(device, queue, "stored drops places", bytemuck::cast_slice(&xv)),
            nf: buffer(device, queue, "stored drops ways", bytemuck::cast_slice(&nf)),
            id: buffer(device, queue, "stored drops kinds", bytemuck::cast_slice(&id)),
            count: drops.len() as u32,
        }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    /// The drops as the drawing reads a body's.
    pub fn buffers(&self) -> DropBuffers<'_> {
        DropBuffers { xv: &self.xv, nf: &self.nf, id: &self.id, count: self.count }
    }
}

/// The bytes a drop takes in a drops file: place and speed (grid units, f32), kind and colour (u32), little-endian.
const DROP_BYTES: usize = 24;
/// What a drops file starts with.
const MAGIC: &[u8; 8] = b"BPEDROP1";

/// Writes `drops` to a drops file (the measuring runs' `WATER_DUMP`).
pub fn save_drops(path: &Path, drops: &[Drop]) -> anyhow::Result<()> {
    let mut out = Vec::with_capacity(MAGIC.len() + drops.len() * DROP_BYTES);
    out.extend_from_slice(MAGIC);
    for d in drops {
        for f in [d.x[0], d.x[1], d.v[0], d.v[1]] {
            out.extend_from_slice(&f.to_le_bytes());
        }
        out.extend_from_slice(&d.kind.to_le_bytes());
        out.extend_from_slice(&d.colour.to_le_bytes());
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Reads a drops file written by [`save_drops`] (serials in file order).
pub fn load_drops(path: &Path) -> anyhow::Result<Vec<Drop>> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(bytes.starts_with(MAGIC), "{} is not a drops file", path.display());
    let body = &bytes[MAGIC.len()..];
    anyhow::ensure!(body.len() % DROP_BYTES == 0, "{} is cut short", path.display());
    let word = |c: &[u8], k: usize| [c[4 * k], c[4 * k + 1], c[4 * k + 2], c[4 * k + 3]];
    Ok(body
        .chunks_exact(DROP_BYTES)
        .enumerate()
        .map(|(i, c)| {
            let f = |k| f32::from_le_bytes(word(c, k));
            let u = |k| u32::from_le_bytes(word(c, k));
            Drop::new(glam::Vec2::new(f(0), f(1)), glam::Vec2::new(f(2), f(3)), u(4), i as u32, u(5))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drops_file_reads_back_what_was_written() {
        let drops = vec![
            Drop::new(glam::Vec2::new(1.5, 2.25), glam::Vec2::new(-3.0, 0.5), 1 | crate::STOPS, 0, 0x1234_5678),
            Drop::new(glam::Vec2::new(f32::NAN, 7.0), glam::Vec2::ZERO, 0, 1, crate::WATER),
        ];
        let path = std::env::temp_dir().join(format!("bpe-drops-{}.drops", std::process::id()));
        save_drops(&path, &drops).unwrap();
        let back = load_drops(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], drops[0]);
        assert!(back[1].x[0].is_nan() && back[1].x[1] == 7.0 && back[1].colour == crate::WATER);
    }
}
