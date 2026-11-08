//! Rendering a scene's whole sound ahead of time, for the export: the same engine and blocks
//! as the editor plays.

use crate::wav::WavOut;
use crate::{BLOCK, Engine, SAMPLE_RATE};
use anyhow::bail;
use bpe_core::{Scene, Settings, SlotId, Value};
use bpe_kit::sound::{BUSES, MixerSettings};
use bpe_kit::{Kit, resolve_scene};
use std::path::{Path, PathBuf};
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
    progress: impl FnMut(f64) -> bool,
) -> anyhow::Result<()> {
    render_sound(scene, kit, path, seconds, &Extras::default(), progress)
}

/// Files the export may save beside the mix.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    /// One 32-bit float WAV per bus (after its fader, mute and solo, so nothing is cut off at
    /// full scale) and one of the master (after the limiter), in that order.
    pub stems: Option<([PathBuf; BUSES], PathBuf)>,
    /// A MIDI file of the notes the shapes played.
    pub midi: Option<PathBuf>,
}

/// The MIDI file for a video saved as `video`: `<name>.mid` beside it.
pub fn midi_path(video: &Path) -> PathBuf {
    video.with_extension("mid")
}

/// The stem files for a video saved as `video`: `<name> - <bus name>.wav` per bus and
/// `<name> - Master.wav`, beside it.
pub fn stem_paths(scene: &Scene, video: &Path) -> ([PathBuf; BUSES], PathBuf) {
    let dir = video.parent().map(Path::to_path_buf).unwrap_or_default();
    let base = video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "sound".into());
    let mixer = scene
        .card(bpe_core::ThingId::ROOT, SlotId::Mixer)
        .map(|c| MixerSettings::from_params(&c.params))
        .unwrap_or_default();
    let file = |name: &str| dir.join(format!("{base} - {name}.wav"));
    let names: [String; BUSES] = std::array::from_fn(|i| safe_name(&mixer.bus(i as u8 + 1).name));
    // Two buses of one name (or one called Master) get their numbers, so no file is written twice.
    let taken = |i: usize| {
        let n = names[i].to_lowercase();
        n == "master" || names.iter().enumerate().any(|(j, m)| j != i && m.to_lowercase() == n)
    };
    let buses =
        std::array::from_fn(
            |i| {
                if taken(i) { file(&format!("{} (Bus {})", names[i], i + 1)) } else { file(&names[i]) }
            },
        );
    (buses, file("Master"))
}

/// `name` with the characters Windows won't take in a file name replaced.
fn safe_name(name: &str) -> String {
    let s: String =
        name.trim().chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    if s.is_empty() { "Bus".into() } else { s }
}

/// Renders the mix to `path` (as [`render_to_wav`]) and the `extras` beside it.
pub fn render_sound(
    scene: &Scene,
    kit: &Kit,
    path: &Path,
    seconds: f64,
    extras: &Extras,
    mut progress: impl FnMut(f64) -> bool,
) -> anyhow::Result<()> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, kit);
    engine.set_end(seconds);
    let total = frames_in(seconds);
    let mut written_files: Vec<PathBuf> = vec![path.to_path_buf()];
    let remove_all = |files: &[PathBuf]| {
        for f in files {
            let _ = std::fs::remove_file(f);
        }
    };
    let mut out = WavOut::create(path)?;
    let mut stems = Vec::new();
    if let Some((buses, master)) = &extras.stems {
        engine.record_stems(true);
        for p in buses.iter().chain([master]) {
            match WavOut::create(p) {
                Ok(w) => {
                    stems.push(w);
                    written_files.push(p.clone());
                }
                Err(e) => {
                    drop(out);
                    drop(stems);
                    remove_all(&written_files);
                    return Err(e);
                }
            }
        }
    }
    if extras.midi.is_some() {
        engine.record_notes(true);
    }
    let mut chunk: Vec<f32> = Vec::with_capacity(CHUNK_BLOCKS * BLOCK * 2);
    let mut stem_chunks: Vec<Vec<f32>> =
        (0..stems.len()).map(|_| Vec::with_capacity(CHUNK_BLOCKS * BLOCK * 2)).collect();
    let mut block = [0.0f32; BLOCK * 2];
    let mut written = 0;
    while written < total {
        chunk.clear();
        stem_chunks.iter_mut().for_each(Vec::clear);
        while chunk.len() < CHUNK_BLOCKS * BLOCK * 2 && written + chunk.len() / 2 < total {
            engine.render(&mut block);
            let frames = BLOCK.min(total - written - chunk.len() / 2);
            chunk.extend_from_slice(&block[..frames * 2]);
            if !stem_chunks.is_empty() {
                for (b, stem) in engine.stems().iter().enumerate() {
                    stem_chunks[b].extend_from_slice(&stem[..frames * 2]);
                }
                stem_chunks[BUSES].extend_from_slice(&block[..frames * 2]);
            }
        }
        LARGEST_CHUNK.fetch_max(chunk.capacity() * size_of::<f32>(), Ordering::Relaxed);
        let mut result = out.write(&chunk);
        for (w, c) in stems.iter_mut().zip(&stem_chunks) {
            result = result.and_then(|()| w.write(c));
        }
        if let Err(e) = result {
            drop(out);
            drop(stems);
            remove_all(&written_files);
            return Err(e);
        }
        written += chunk.len() / 2;
        if !progress(written as f64 / total.max(1) as f64) {
            drop(out);
            drop(stems);
            remove_all(&written_files);
            bail!("sound render cancelled");
        }
    }
    let mut result = Ok(());
    for w in stems {
        result = result.and_then(|()| w.finish());
    }
    if let Some(midi) = &extras.midi {
        written_files.push(midi.clone());
        result =
            result.and_then(|()| crate::midi::write_midi(midi, scene, engine.note_log(), engine.tempo(), total as u64));
    }
    result = result.and_then(|()| out.finish());
    if result.is_err() {
        remove_all(&written_files);
    }
    result
}

/// `seconds` of the scene's sound, interleaved stereo, all in memory (for tests and short
/// clips).
pub fn render_samples(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<f32> {
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, kit);
    engine.set_end(seconds);
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
