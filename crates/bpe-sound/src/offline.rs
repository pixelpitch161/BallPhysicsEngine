//! Rendering a scene's whole sound ahead of time, for the export: the same engine and blocks
//! as the editor plays.

use crate::wav::WavOut;
use crate::{BLOCK, Engine, SAMPLE_RATE};
use anyhow::bail;
use bpe_core::{Scene, SlotId, Value};
use bpe_kit::{Kit, resolve_scene};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Blocks in one written chunk: about a second.
const CHUNK_BLOCKS: usize = 750;

static LARGEST_CHUNK: AtomicUsize = AtomicUsize::new(0);

/// The largest chunk buffer any render to a file has held, in bytes (for checks that memory
/// stays flat however long the sound is).
pub fn largest_chunk_bytes() -> usize {
    LARGEST_CHUNK.load(Ordering::Relaxed)
}

/// Whether anything in the scene makes sound: an enabled thing with a sound card that is on.
pub fn has_sound(scene: &Scene) -> bool {
    scene.things.values().filter(|t| t.enabled).any(|t| {
        let card = t.slots.get(&SlotId::Sound).is_some_and(|c| c.params.get("enabled") != Some(&Value::Bool(false)));
        #[cfg(feature = "test-tone")]
        let card = card || t.tags.iter().any(|tag| tag.starts_with("test-tone"));
        card
    })
}

/// Frames in `seconds` of sound.
fn frames_in(seconds: f64) -> usize {
    (seconds.max(0.0) * SAMPLE_RATE as f64).round() as usize
}

/// Renders `seconds` of the scene's sound to `path` as a 32-bit float stereo WAV at 48 kHz,
/// a second at a time so memory stays flat. `progress` hears how far it is (0 to 1) after each
/// second; returning `false` cancels, which deletes the file and gives an error.
pub fn render_to_wav(
    scene: &Scene,
    kit: &Kit,
    path: &Path,
    seconds: f64,
    mut progress: impl FnMut(f64) -> bool,
) -> anyhow::Result<()> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, kit);
    let total = frames_in(seconds);
    let mut out = WavOut::create(path)?;
    let mut chunk: Vec<f32> = Vec::with_capacity(CHUNK_BLOCKS * BLOCK * 2);
    let mut block = [0.0f32; BLOCK * 2];
    let mut written = 0;
    while written < total {
        chunk.clear();
        while chunk.len() < CHUNK_BLOCKS * BLOCK * 2 && written + chunk.len() / 2 < total {
            engine.render(&mut block);
            let frames = BLOCK.min(total - written - chunk.len() / 2);
            chunk.extend_from_slice(&block[..frames * 2]);
        }
        LARGEST_CHUNK.fetch_max(chunk.capacity() * size_of::<f32>(), Ordering::Relaxed);
        if let Err(e) = out.write(&chunk) {
            drop(out);
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
        written += chunk.len() / 2;
        if !progress(written as f64 / total.max(1) as f64) {
            drop(out);
            let _ = std::fs::remove_file(path);
            bail!("sound render cancelled");
        }
    }
    out.finish()
}

/// `seconds` of the scene's sound, interleaved stereo, all in memory (for tests and short
/// clips).
pub fn render_samples(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<f32> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, kit);
    let total = frames_in(seconds);
    let mut out = Vec::with_capacity(total * 2);
    let mut block = [0.0f32; BLOCK * 2];
    while out.len() < total * 2 {
        engine.render(&mut block);
        let frames = BLOCK.min(total - out.len() / 2);
        out.extend_from_slice(&block[..frames * 2]);
    }
    out
}
