//! What an export can save beside the video: the notes and chords as MIDI, and each sound, bus
//! and the song as a WAV stem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bpe_core::{Resolved, Scene, SlotId};
use bpe_mix::{Soundtrack, Stem};
use bpe_runtime::Log;
use serde::{Deserialize, Serialize};

use crate::{Kit, mix_settings, music};

/// Which extras to save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extras {
    pub midi: bool,
    pub stems: bool,
    /// Channel stems hold their sends to the buses too.
    pub wet: bool,
}

impl Extras {
    pub fn any(self) -> bool {
        self.midi || self.stems
    }
}

/// One mix of a scene, with the extras asked for.
pub struct SceneExport {
    /// The finished soundtrack, the same as [`crate::mix_scene`] gives.
    pub soundtrack: Soundtrack,
    pub midi: Option<Vec<u8>>,
    /// Channels, buses, then the song.
    pub stems: Vec<Stem>,
}

/// "<module> (<things>)" for every sound module some thing plays through.
fn track_names(scene: &Scene, resolved: &Resolved) -> BTreeMap<u32, String> {
    let mut users: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (id, r) in &resolved.things {
        if let Some(slot) = r.slots.get(&SlotId::Sound) {
            let name = scene.things.get(id).map_or_else(|| format!("Thing {}", id.0), |t| t.name.clone());
            users.entry(slot.module.0).or_default().push(name);
        }
    }
    users
        .into_iter()
        .map(|(module, things)| {
            let base = scene
                .library
                .modules
                .get(&bpe_core::ModuleId(module))
                .map_or_else(|| format!("Sound {module}"), |m| m.name.clone());
            (module, format!("{base} ({})", things.join(", ")))
        })
        .collect()
}

/// Mixes the scene once, keeping what `extras` asks for.
pub fn mix_scene_export(scene: &Scene, resolved: &Resolved, log: &Log, kit: &Kit, extras: Extras) -> SceneExport {
    let animation = bpe_core::Animation::new(scene, resolved);
    let animation = (!animation.is_empty()).then_some(&animation);
    let warp = music::warp_for(scene, resolved, log);
    let settings = mix_settings(scene, resolved);
    let video_time = |t| warp.video_time(t);
    let physics = music::physics_inputs(scene, resolved, log, &settings, &video_time);
    let mut cache = bpe_mix::ChannelCache::default();
    let mut track = bpe_mix::mix_premaster(
        &settings,
        resolved,
        &scene.library,
        log,
        &kit.mixes,
        animation,
        &video_time,
        &physics,
        &mut cache,
    );
    let mut stems = if extras.stems { bpe_mix::stems(&settings, &track, &physics, extras.wet) } else { Vec::new() };
    let root = music::root_settings(resolved);
    if let Some(song) = music::load_song(&root.song) {
        let before = extras.stems.then(|| track.samples.clone());
        music::mix_in(&mut track.samples, &song, root.song_offset, root.song_volume, root.ducking, root.loudness);
        // The song as it sits in the mix, ducking and all: what it added.
        if let Some(before) = before {
            let samples = track.samples.iter().zip(&before).map(|(after, b)| after - b).collect();
            stems.push(Stem { name: "Song".into(), samples });
        }
    }
    let problems = bpe_mix::run_master_fx(&mut track.samples, &settings, &physics.with_levels(&track.levels));
    track.problems.extend(problems);
    track.report = Some(bpe_mix::master(&mut track.samples, &settings.master));
    let midi = extras.midi.then(|| {
        let notes = bpe_mix::notes_with(&settings, resolved, &scene.library, log, &kit.mixes, animation, &video_time);
        bpe_mix::midi_file(&settings, &notes, &track_names(scene, resolved))
    });
    SceneExport { soundtrack: track, midi, stems }
}

/// The stems folder for an export named `base`.
pub fn stems_folder(base: &Path) -> PathBuf {
    let mut name = base.file_name().unwrap_or_default().to_os_string();
    name.push("-stems");
    base.with_file_name(name)
}

/// The MIDI file for an export named `base`.
pub fn midi_path(base: &Path) -> PathBuf {
    let mut name = base.file_name().unwrap_or_default().to_os_string();
    name.push(".mid");
    base.with_file_name(name)
}

/// Writes `<base>.mid` and `<base>-stems/` (replacing an old stems folder) for what the export
/// holds. Gives the paths written.
pub fn write_extras(export: &SceneExport, base: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    if let Some(bytes) = &export.midi {
        let path = midi_path(base);
        std::fs::write(&path, bytes)?;
        written.push(path);
    }
    if !export.stems.is_empty() {
        let folder = stems_folder(base);
        if folder.exists() {
            std::fs::remove_dir_all(&folder)?;
        }
        std::fs::create_dir_all(&folder)?;
        for (stem, name) in export.stems.iter().zip(bpe_mix::stem_file_names(&export.stems)) {
            let path = folder.join(name);
            bpe_mix::write_wav24(&path, &stem.samples)?;
            written.push(path);
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::run;
    use crate::{kit, mix_scene, new_scene};
    use bpe_core::resolve;

    fn all() -> Extras {
        Extras { midi: true, stems: true, wet: false }
    }

    fn scene_and_log(seconds: f64) -> (Scene, Resolved, Kit, Log) {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, seconds);
        (scene, resolved, kit, log)
    }

    #[test]
    fn export_soundtrack_matches_mix_scene() {
        let (scene, resolved, kit, log) = scene_and_log(20.0);
        let export = mix_scene_export(&scene, &resolved, &log, &kit, all());
        assert_eq!(export.soundtrack.samples, mix_scene(&scene, &resolved, &log, &kit).samples);
    }

    #[test]
    fn stems_through_the_master_match_the_mix() {
        let (scene, resolved, kit, log) = scene_and_log(20.0);
        let export = mix_scene_export(&scene, &resolved, &log, &kit, all());
        assert!(!export.stems.is_empty());
        let mut sum = vec![0.0f32; export.soundtrack.samples.len()];
        for s in &export.stems {
            for (o, v) in sum.iter_mut().zip(&s.samples) {
                *o += v;
            }
        }
        let settings = mix_settings(&scene, &resolved);
        bpe_mix::run_master_fx(&mut sum, &settings, &Default::default());
        bpe_mix::master(&mut sum, &settings.master);
        let worst = sum.iter().zip(&export.soundtrack.samples).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst <= 0.001, "stems through the master are off by {worst} (over -60 dB)");
    }

    #[test]
    fn midi_names_tracks_after_modules_and_things() {
        let (scene, resolved, kit, log) = scene_and_log(10.0);
        let export = mix_scene_export(&scene, &resolved, &log, &kit, all());
        let file = bpe_music::midi::parse(export.midi.as_ref().unwrap()).unwrap();
        let names: Vec<&str> = file.parts.iter().map(|p| p.name.as_str()).collect();
        assert!(names.iter().any(|n| n.starts_with("Default sound (") && n.contains("Ball")), "{names:?}");
        assert!(!file.notes.is_empty());
    }

    #[test]
    fn stems_of_a_silent_scene() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.retain(|id, _| *id == bpe_core::ThingId::ROOT);
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 2.0);
        let export = mix_scene_export(&scene, &resolved, &log, &kit, all());
        let dir = std::env::temp_dir().join(format!("bpe-silent-extras-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let written = write_extras(&export, &dir.join("silent")).unwrap();
        assert!(written.iter().any(|p| p.extension().is_some_and(|e| e == "mid")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_extras_writes_the_files() {
        let (scene, resolved, kit, log) = scene_and_log(5.0);
        let export = mix_scene_export(&scene, &resolved, &log, &kit, all());
        let dir = std::env::temp_dir().join(format!("bpe-extras-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("clip");
        let written = write_extras(&export, &base).unwrap();
        assert!(bpe_music::midi::parse(&std::fs::read(dir.join("clip.mid")).unwrap()).is_ok());
        let stems: Vec<_> = written.iter().filter(|p| p.extension().is_some_and(|e| e == "wav")).collect();
        assert_eq!(stems.len(), export.stems.len());
        for p in stems {
            assert!(p.starts_with(dir.join("clip-stems")));
            assert_eq!(hound::WavReader::open(p).unwrap().spec().bits_per_sample, 24);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
