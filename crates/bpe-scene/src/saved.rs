//! The user's own presets: a ball look, a Look tab, an effect stack, rules, bounce settings,
//! sound, captions or a whole scene, saved under a name and reused in other scenes.
//!
//! Each preset is one RON file in a folder per kind, so presets can be copied between computers
//! by copying files. File paths in a preset (songs, samples, fonts) are made absolute when it is
//! saved, so it still finds them from any scene.

use crate::{
    BallContact, BallStyle, BounceRules, Caption, Effect, Emitter, Font, Graph, Instrument, Look, Music, Scene, Sound,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Effects named like this use the node effect of that name in `Scene::shader_graphs`.
const GRAPH_PREFIX: &str = "graph:";

/// What a preset holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PresetKind {
    BallLook,
    Look,
    Effects,
    Rules,
    Bounces,
    Sound,
    Captions,
    Scene,
}

impl PresetKind {
    pub const ALL: [PresetKind; 8] = [
        PresetKind::BallLook,
        PresetKind::Look,
        PresetKind::Effects,
        PresetKind::Rules,
        PresetKind::Bounces,
        PresetKind::Sound,
        PresetKind::Captions,
        PresetKind::Scene,
    ];

    /// The kind's name in a sentence, such as "Save this ball look".
    pub fn name(self) -> &'static str {
        match self {
            PresetKind::BallLook => "ball look",
            PresetKind::Look => "look",
            PresetKind::Effects => "effect stack",
            PresetKind::Rules => "rule set",
            PresetKind::Bounces => "bounce setup",
            PresetKind::Sound => "sound setup",
            PresetKind::Captions => "caption set",
            PresetKind::Scene => "scene",
        }
    }

    /// What applying a preset of this kind replaces, for hover text.
    pub fn replaces(self) -> &'static str {
        match self {
            PresetKind::BallLook => "these balls' look",
            PresetKind::Look => "the background, trails, glow, string art and hit particles",
            PresetKind::Effects => "the effect stack",
            PresetKind::Rules => "every rule",
            PresetKind::Bounces => "what happens on each bounce and how balls bump into each other",
            PresetKind::Sound => "the instrument, notes, chords, voices, song and beat sync",
            PresetKind::Captions => "every caption",
            PresetKind::Scene => "the whole scene",
        }
    }

    /// The folder for this kind inside the presets folder.
    pub fn folder(self) -> &'static str {
        match self {
            PresetKind::BallLook => "ball-looks",
            PresetKind::Look => "looks",
            PresetKind::Effects => "effects",
            PresetKind::Rules => "rules",
            PresetKind::Bounces => "bounces",
            PresetKind::Sound => "sound",
            PresetKind::Captions => "captions",
            PresetKind::Scene => "scenes",
        }
    }
}

/// The settings a preset holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PresetData {
    BallLook(BallStyle),
    Look {
        look: Look,
        particles: Vec<Emitter>,
    },
    /// The effects, and the node effects they use.
    Effects {
        effects: Vec<Effect>,
        graphs: Vec<Graph>,
    },
    Rules(Vec<Graph>),
    Bounces {
        rules: BounceRules,
        ball_collisions: bool,
        contact: BallContact,
    },
    Sound {
        sound: Sound,
        music: Music,
    },
    Captions(Vec<Caption>),
    Scene(Box<Scene>),
}

/// A preset the user saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPreset {
    pub name: String,
    pub data: PresetData,
}

impl PresetData {
    pub fn kind(&self) -> PresetKind {
        match self {
            PresetData::BallLook(_) => PresetKind::BallLook,
            PresetData::Look { .. } => PresetKind::Look,
            PresetData::Effects { .. } => PresetKind::Effects,
            PresetData::Rules(_) => PresetKind::Rules,
            PresetData::Bounces { .. } => PresetKind::Bounces,
            PresetData::Sound { .. } => PresetKind::Sound,
            PresetData::Captions(_) => PresetKind::Captions,
            PresetData::Scene(_) => PresetKind::Scene,
        }
    }

    /// The part of `scene` a preset of `kind` holds. A ball look comes from one ball group, so
    /// use [`PresetData::BallLook`] for it; this returns None for it.
    pub fn capture(scene: &Scene, kind: PresetKind) -> Option<Self> {
        Some(match kind {
            PresetKind::BallLook => return None,
            PresetKind::Look => PresetData::Look { look: scene.look.clone(), particles: scene.particles.clone() },
            PresetKind::Effects => {
                let graphs = scene
                    .shader_graphs
                    .iter()
                    .filter(|g| scene.effects.iter().any(|e| e.shader.strip_prefix(GRAPH_PREFIX) == Some(&g.name)))
                    .cloned()
                    .collect();
                PresetData::Effects { effects: scene.effects.clone(), graphs }
            }
            PresetKind::Rules => PresetData::Rules(scene.logic.clone()),
            PresetKind::Bounces => PresetData::Bounces {
                rules: scene.rules.clone(),
                ball_collisions: scene.physics.ball_collisions,
                contact: scene.physics.contact.clone(),
            },
            PresetKind::Sound => PresetData::Sound { sound: scene.sound.clone(), music: scene.music.clone() },
            PresetKind::Captions => PresetData::Captions(scene.captions.clone()),
            PresetKind::Scene => PresetData::Scene(Box::new(scene.clone())),
        })
    }

    /// Puts the preset's settings into `scene`, replacing what was there. A ball look goes to
    /// ball group `ball`; other kinds ignore it.
    pub fn apply(self, scene: &mut Scene, ball: Option<usize>) {
        match self {
            PresetData::BallLook(style) => {
                if let Some(spawner) = ball.and_then(|i| scene.spawners.get_mut(i)) {
                    *spawner.style_mut() = Some(style);
                }
            }
            PresetData::Look { look, particles } => {
                scene.look = look;
                scene.particles = particles;
            }
            PresetData::Effects { effects, graphs } => {
                // Animations and pulses belonged to the old effects.
                scene.remap_effect_tracks(|_| None);
                scene.effects = effects;
                for graph in graphs {
                    match scene.shader_graphs.iter_mut().find(|g| g.name == graph.name) {
                        Some(old) => *old = graph,
                        None => scene.shader_graphs.push(graph),
                    }
                }
            }
            PresetData::Rules(logic) => scene.logic = logic,
            PresetData::Bounces { rules, ball_collisions, contact } => {
                scene.rules = rules;
                scene.physics.ball_collisions = ball_collisions;
                scene.physics.contact = contact;
            }
            PresetData::Sound { sound, music } => {
                scene.sound = sound;
                scene.music = music;
            }
            PresetData::Captions(captions) => scene.captions = captions,
            PresetData::Scene(saved) => *scene = *saved,
        }
    }

    /// Makes relative file paths absolute, starting from `base` (the scene file's folder).
    pub fn absolute_paths(&mut self, base: &Path) {
        match self {
            PresetData::Sound { sound, music } => sound_paths(sound, music, base),
            PresetData::Captions(captions) => caption_paths(captions, base),
            PresetData::Scene(scene) => {
                sound_paths(&mut scene.sound, &mut scene.music, base);
                caption_paths(&mut scene.captions, base);
            }
            _ => {}
        }
    }
}

fn absolute(path: &mut String, base: &Path) {
    if !path.is_empty() && Path::new(path.as_str()).is_relative() {
        *path = base.join(path.as_str()).display().to_string();
    }
}

fn instrument_paths(instrument: &mut Instrument, base: &Path) {
    match instrument {
        Instrument::Sample { path, .. } | Instrument::SampleFolder { path } | Instrument::SoundFont { path, .. } => {
            absolute(path, base)
        }
        Instrument::Pluck { .. } | Instrument::Library { .. } => {}
    }
}

fn sound_paths(sound: &mut Sound, music: &mut Music, base: &Path) {
    instrument_paths(&mut sound.instrument, base);
    for voice in &mut sound.voices {
        instrument_paths(&mut voice.instrument, base);
    }
    if let Some(song) = &mut music.song {
        absolute(song, base);
    }
    if let Some(melody) = &mut music.melody
        && !melody.path.starts_with("builtin:")
    {
        absolute(&mut melody.path, base);
    }
}

fn caption_paths(captions: &mut [Caption], base: &Path) {
    for caption in captions {
        if let Font::File(path) = &mut caption.font {
            absolute(path, base);
        }
    }
}

/// A file name for a preset called `name`: letters, digits, spaces, dashes and underscores kept,
/// anything else dropped.
pub fn file_name(name: &str) -> String {
    let kept: String = name.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect::<String>();
    let kept = kept.trim();
    format!("{}.ron", if kept.is_empty() { "preset" } else { kept })
}

impl SavedPreset {
    pub fn kind(&self) -> PresetKind {
        self.data.kind()
    }

    fn path(&self, dir: &Path) -> PathBuf {
        dir.join(self.kind().folder()).join(file_name(&self.name))
    }

    pub fn to_ron(&self) -> anyhow::Result<String> {
        let pretty = ron::ser::PrettyConfig::new().struct_names(true);
        Ok(ron::ser::to_string_pretty(self, pretty)?)
    }

    pub fn from_ron(text: &str) -> anyhow::Result<Self> {
        let mut preset: Self = ron::from_str(text)?;
        if let PresetData::Scene(scene) = &mut preset.data {
            for track in &mut scene.animation {
                track.sort();
            }
        }
        Ok(preset)
    }

    /// Saves the preset in the presets folder `dir`, replacing one of the same kind and name.
    pub fn save(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        let path = self.path(dir);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&path, self.to_ron()?)?;
        Ok(path)
    }

    /// Removes the preset's file from the presets folder `dir`.
    pub fn delete(&self, dir: &Path) -> anyhow::Result<()> {
        Ok(std::fs::remove_file(self.path(dir))?)
    }

    /// Whether a preset of this kind called `name` is saved in `dir`.
    pub fn exists(dir: &Path, kind: PresetKind, name: &str) -> bool {
        dir.join(kind.folder()).join(file_name(name)).is_file()
    }

    /// Every readable preset of `kind` in the presets folder `dir`, sorted by name. Files that
    /// don't read are left out.
    pub fn list(dir: &Path, kind: PresetKind) -> Vec<SavedPreset> {
        let Ok(entries) = std::fs::read_dir(dir.join(kind.folder())) else { return Vec::new() };
        let mut presets: Vec<SavedPreset> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ron")))
            .filter_map(|p| Self::from_ron(&std::fs::read_to_string(p).ok()?).ok())
            .filter(|p| p.kind() == kind)
            .collect();
        presets.sort_by_key(|p| p.name.to_lowercase());
        presets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Melody, Spawner};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-saved-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn every_kind_saves_loads_and_applies() {
        let dir = temp_dir("kinds");
        let mut source = Scene::presets().into_iter().find(|p| !p.scene.effects.is_empty()).unwrap().scene;
        source.captions.push(Caption::default());
        source.look.trail_seconds = 0.77;
        source.rules.grow = 1.5;
        source.sound.low_note = 41;
        for kind in PresetKind::ALL.into_iter().filter(|&k| k != PresetKind::BallLook) {
            let preset =
                SavedPreset { name: format!("My {}", kind.name()), data: PresetData::capture(&source, kind).unwrap() };
            preset.save(&dir).unwrap();
            let listed = SavedPreset::list(&dir, kind);
            assert_eq!(listed, vec![preset.clone()], "{kind:?}");
            let mut target = Scene::default();
            listed[0].data.clone().apply(&mut target, None);
            // Applying again from the target gives back the same settings.
            assert_eq!(PresetData::capture(&target, kind), Some(preset.data), "{kind:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ball_look_goes_to_one_group() {
        let mut scene = Scene::default();
        scene.spawners.push(scene.spawners[0].clone());
        let style = BallStyle { opacity: 0.5, ..BallStyle::default() };
        PresetData::BallLook(style.clone()).apply(&mut scene, Some(1));
        assert_eq!(scene.spawners[1].style(), Some(&style));
        assert_ne!(scene.spawners[0].style(), Some(&style));
        assert!(matches!(scene.spawners[1], Spawner::Grid { .. } | Spawner::Single { .. } | Spawner::Stream { .. }));
    }

    #[test]
    fn effect_stack_brings_its_node_effects_and_drops_old_animations() {
        let source = Scene {
            effects: vec![Effect::new("graph:Swirl"), Effect::new("kaleidoscope")],
            shader_graphs: vec![Graph::new("Swirl"), Graph::new("Unused")],
            ..Scene::default()
        };
        let data = PresetData::capture(&source, PresetKind::Effects).unwrap();
        let PresetData::Effects { graphs, .. } = &data else { panic!() };
        assert_eq!(graphs.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Swirl"]);

        let mut target = Scene { effects: vec![Effect::new("bloom")], ..Scene::default() };
        target.animation.push(crate::Track::new(crate::Property::EffectAmount(0)));
        target.shader_graphs = vec![Graph::new("Swirl"), Graph::new("Mine")];
        data.apply(&mut target, None);
        assert_eq!(target.effects, source.effects);
        assert!(target.animation.is_empty());
        assert_eq!(target.shader_graphs.len(), 2, "same-named node effect is replaced, others kept");
    }

    #[test]
    fn saved_paths_are_absolute() {
        let base = std::env::temp_dir().join("my scenes");
        let mut scene = Scene::default();
        scene.music.song = Some("songs/tune.mp3".into());
        scene.music.melody = Some(Melody { path: "builtin:Ode".into(), ..Melody::default() });
        scene.sound.instrument = Instrument::Sample { path: "ping.wav".into(), root_note: 60 };
        let mut data = PresetData::Scene(Box::new(scene));
        data.absolute_paths(&base);
        let PresetData::Scene(scene) = data else { panic!() };
        assert_eq!(scene.music.song.as_deref(), Some(base.join("songs/tune.mp3").display().to_string().as_str()));
        assert_eq!(scene.music.melody.as_ref().unwrap().path, "builtin:Ode");
        assert!(matches!(scene.sound.instrument, Instrument::Sample { ref path, .. } if Path::new(path).is_absolute()));
    }

    #[test]
    fn names_make_safe_file_names() {
        assert_eq!(file_name("Neon / glow?"), "Neon  glow.ron");
        assert_eq!(file_name("../.."), "preset.ron");
        assert_eq!(file_name("  "), "preset.ron");
    }

    #[test]
    fn same_name_replaces_and_delete_removes() {
        let dir = temp_dir("replace");
        let mut preset = SavedPreset { name: "Calm".into(), data: PresetData::Rules(vec![Graph::new("a")]) };
        preset.save(&dir).unwrap();
        preset.data = PresetData::Rules(vec![Graph::new("b")]);
        preset.save(&dir).unwrap();
        assert_eq!(SavedPreset::list(&dir, PresetKind::Rules), vec![preset.clone()]);
        assert!(SavedPreset::exists(&dir, PresetKind::Rules, "Calm"));
        preset.delete(&dir).unwrap();
        assert!(SavedPreset::list(&dir, PresetKind::Rules).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
