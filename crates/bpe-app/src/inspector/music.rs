//! The Sound tab: which instrument plays, chords or a MIDI melody, the voices table that gives
//! kinds of event their own instrument, the song the video plays over, and beat sync.
//!
//! Clicking an instrument picks it and plays a few notes on it. Files (WAV samples, sample
//! folders, SoundFonts, MIDI files, songs) are read on demand and the results kept, so the tab
//! stays quick while it is open.

use super::{
    Roll, grid, heading, heading_dice, label, log_units, more, note_name, parse_note, percent, reset_tab, slider, units,
};
use crate::audio_out::{AudioOut, Handle};
use crate::theme;
use bpe_audio::instruments::{self, Builtin, Piece};
use bpe_audio::midi::MidiFile;
use bpe_scene::{
    Advance, BeatSync, ChordStep, EventFilter, Group, Instrument, Melody, Music, PitchMode, Scene, Sound, SyncMode,
    VoiceMap,
};
use bpe_sim::beat_sync;
use eframe::egui::{self, Popup, PopupCloseBehavior, RichText, Slider, Ui};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

/// The note a WAV file is assumed to play until the user says otherwise.
const SAMPLE_ROOT_NOTE: u8 = 60;

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {
    /// Where relative paths in the scene start from: the scene file's folder. Set by the app.
    pub base_dir: Option<PathBuf>,
    /// What the latest soundtrack found, such as the song's tempo. Set by the app.
    pub report: Report,
    midi: Option<Loaded<Arc<MidiFile>>>,
    font: Option<Loaded<Vec<(u32, u32, String)>>>,
    folder: Option<Loaded<Vec<u8>>>,
    /// The speakers instrument previews play on, opened at the first preview. None inside
    /// means there are none.
    speaker: Option<Option<AudioOut>>,
    nudge: Option<Receiver<NudgeResult>>,
    /// What the last "Fit to the beat" did.
    nudge_note: Option<String>,
}

/// A file read for the panel, kept until its path changes.
struct Loaded<T> {
    path: String,
    value: Result<T, String>,
}

/// What the app learned while making the soundtrack.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Share of hits within the beat sync tolerance of a beat, when there are beats.
    pub on_beat: Option<f32>,
    /// Tempo found in the song.
    pub song_bpm: Option<f32>,
    /// Length of the song in seconds.
    pub song_seconds: Option<f64>,
    pub song_error: Option<String>,
    /// Notes played over the whole video.
    pub notes: usize,
}

impl Report {
    pub fn new(sound: &bpe_audio::SceneSound) -> Self {
        Self {
            on_beat: sound.on_beat,
            song_bpm: sound.song.as_ref().map(|s| s.features.bpm).filter(|b| *b > 0.0),
            song_seconds: sound.song.as_ref().map(|s| s.samples.len() as f64 / 2.0 / bpe_audio::SAMPLE_RATE as f64),
            song_error: sound.song_error.clone(),
            notes: sound.mix.notes.len(),
        }
    }
}

impl State {
    fn base_dir(&self) -> Option<&Path> {
        self.base_dir.as_deref()
    }

    fn resolve(&self, path: &str) -> PathBuf {
        match self.base_dir() {
            Some(dir) if Path::new(path).is_relative() => dir.join(path),
            _ => PathBuf::from(path),
        }
    }

    /// Plays a few notes on `instrument`, made on another thread so the panel never waits.
    fn preview(&mut self, instrument: &Instrument) {
        let speaker = self.speaker.get_or_insert_with(|| AudioOut::new().ok());
        let Some(handle) = speaker.as_ref().map(AudioOut::handle) else { return };
        let (instrument, base_dir) = (instrument.clone(), self.base_dir.clone());
        std::thread::spawn(move || play_preview(&handle, &instrument, base_dir.as_deref()));
    }

    fn midi(&mut self, path: &str) -> &Result<Arc<MidiFile>, String> {
        if self.midi.as_ref().is_none_or(|l| l.path != path) {
            let value = bpe_audio::midi::load(path, self.base_dir()).map(Arc::new).map_err(|e| format!("{e:#}"));
            self.midi = Some(Loaded { path: path.into(), value });
        }
        &self.midi.as_ref().expect("just loaded").value
    }

    fn font(&mut self, path: &str) -> &Result<Vec<(u32, u32, String)>, String> {
        if self.font.as_ref().is_none_or(|l| l.path != path) {
            let value = instruments::soundfont_presets(&self.resolve(path)).map_err(|e| format!("{e:#}"));
            self.font = Some(Loaded { path: path.into(), value });
        }
        &self.font.as_ref().expect("just loaded").value
    }

    fn folder(&mut self, path: &str) -> &Result<Vec<u8>, String> {
        if self.folder.as_ref().is_none_or(|l| l.path != path) {
            let value = instruments::folder_notes(&self.resolve(path)).map_err(|e| format!("{e:#}"));
            self.folder = Some(Loaded { path: path.into(), value });
        }
        &self.folder.as_ref().expect("just loaded").value
    }
}

fn play_preview(handle: &Handle, instrument: &Instrument, base_dir: Option<&Path>) {
    match instruments::preview(instrument, base_dir) {
        Ok(samples) => handle.play(samples),
        Err(_) => handle.stop(),
    }
}

/// Draws the Sound tab. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State, roll: &mut Option<Roll>) {
    finish_nudge(scene, state);
    ui.checkbox(&mut scene.sound.enabled, "Play a note on every hit");
    ui.add_enabled_ui(scene.sound.enabled, |ui| {
        instrument_section(ui, &mut scene.sound, state);
        notes_section(ui, scene, state, roll);
        voices_section(ui, &mut scene.sound.voices, state);
        loudness_section(ui, &mut scene.sound);
    });
    song_section(ui, &mut scene.music, state);
    beat_sync_section(ui, scene, state);

    let fresh_sound = default_sound(&scene.sound);
    let fresh_music = default_music(&scene.music);
    if reset_tab(ui, scene.sound != fresh_sound || scene.music != fresh_music) {
        scene.sound = fresh_sound;
        scene.music = fresh_music;
    }
}

/// Default sound settings that keep the user's own chords, note shuffle, files and voices.
fn default_sound(current: &Sound) -> Sound {
    let instrument = match &current.instrument {
        Instrument::Pluck { .. } | Instrument::Library { .. } => Sound::default().instrument,
        Instrument::Sample { path, .. } => Instrument::Sample { path: path.clone(), root_note: SAMPLE_ROOT_NOTE },
        other => other.clone(),
    };
    Sound {
        progression: current.progression.clone(),
        seed: current.seed,
        instrument,
        voices: current.voices.clone(),
        ..Sound::default()
    }
}

/// Default music settings that keep the song and the melody's file.
fn default_music(current: &Music) -> Music {
    Music {
        song: current.song.clone(),
        melody: current.melody.as_ref().map(|m| Melody { path: m.path.clone(), ..Melody::default() }),
        ..Music::default()
    }
}

// ---------- instruments ----------

/// What the instrument buttons pick, in the order they're shown.
#[derive(Clone, Copy, PartialEq)]
enum Pick {
    /// The original soft pluck, which sounds exactly as scenes always have.
    Pluck,
    Builtin(Builtin),
    Wav,
    Folder,
    SoundFont,
}

impl Pick {
    fn of(instrument: &Instrument) -> Option<Pick> {
        Some(match instrument {
            Instrument::Pluck { .. } => Pick::Pluck,
            Instrument::Library { name } => Pick::Builtin(Builtin::from_name(name)?),
            Instrument::Sample { .. } => Pick::Wav,
            Instrument::SampleFolder { .. } => Pick::Folder,
            Instrument::SoundFont { .. } => Pick::SoundFont,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Pick::Pluck => "Pluck",
            Pick::Builtin(b) => b.name(),
            Pick::Wav => "WAV file",
            Pick::Folder => "Folder of notes",
            Pick::SoundFont => "SoundFont",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Pick::Pluck => "A soft plucked tone, the original bounce sound.",
            Pick::Builtin(b) => b.description(),
            Pick::Wav => "One recording of your own, tuned up and down to each note.",
            Pick::Folder => {
                "A folder of WAV files named by note, like C4.wav and G4.wav. Each note plays the nearest one."
            }
            Pick::SoundFont => "A SoundFont (.sf2) file, with hundreds of real instruments in one file.",
        }
    }

    /// An instrument of this kind, keeping what `current` has where it can.
    fn instrument(self, current: &Instrument) -> Option<Instrument> {
        Some(match self {
            Pick::Pluck => Instrument::Pluck { decay: 0.9, brightness: 0.5 },
            Pick::Builtin(b) => Instrument::library(b.name()),
            Pick::Wav => {
                let path = pick_file("WAV audio", &["wav"])?;
                let root_note = match current {
                    Instrument::Sample { root_note, .. } => *root_note,
                    _ => SAMPLE_ROOT_NOTE,
                };
                Instrument::Sample { path, root_note }
            }
            Pick::Folder => {
                Instrument::SampleFolder { path: rfd::FileDialog::new().pick_folder()?.display().to_string() }
            }
            Pick::SoundFont => Instrument::SoundFont { path: pick_file("SoundFont", &["sf2"])?, bank: 0, preset: 0 },
        })
    }
}

/// The instrument buttons in shelves, with your own sounds last.
fn shelves() -> Vec<(&'static str, Vec<Pick>)> {
    let mut shelves: Vec<(&'static str, Vec<Pick>)> = Vec::new();
    for builtin in Builtin::ALL {
        let pick = if builtin == Builtin::Pluck { Pick::Pluck } else { Pick::Builtin(builtin) };
        match shelves.iter_mut().find(|(family, _)| *family == builtin.family()) {
            Some((_, picks)) => picks.push(pick),
            None => shelves.push((builtin.family(), vec![pick])),
        }
    }
    shelves.push(("Your own sounds", vec![Pick::Wav, Pick::Folder, Pick::SoundFont]));
    shelves
}

fn pick_file(kind: &str, extensions: &[&str]) -> Option<String> {
    Some(rfd::FileDialog::new().add_filter(kind, extensions).pick_file()?.display().to_string())
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| "No file chosen".into(), |n| n.to_string_lossy().into_owned())
}

fn instrument_section(ui: &mut Ui, sound: &mut Sound, state: &mut State) {
    heading(ui, "Instrument");
    ui.label(RichText::new("Click one to pick it and hear it.").color(theme::MUTED));
    let current = Pick::of(&sound.instrument);
    egui::Grid::new("instrument-shelves").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        for (family, picks) in shelves() {
            ui.label(RichText::new(family).small().color(theme::MUTED));
            ui.horizontal_wrapped(|ui| {
                for pick in picks {
                    let response =
                        ui.add(egui::Button::selectable(current == Some(pick), pick.name())).on_hover_text(pick.help());
                    if response.clicked() {
                        if current != Some(pick)
                            && let Some(instrument) = pick.instrument(&sound.instrument)
                        {
                            sound.instrument = instrument;
                        }
                        if Pick::of(&sound.instrument) == Some(pick) {
                            state.preview(&sound.instrument);
                        }
                    }
                }
            });
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    grid(ui, "instrument", |ui| instrument_settings(ui, &mut sound.instrument, state));
}

/// The settings of the chosen instrument, as grid rows.
fn instrument_settings(ui: &mut Ui, instrument: &mut Instrument, state: &mut State) {
    match instrument {
        Instrument::Pluck { decay, brightness } => {
            slider(
                ui,
                "Ring time",
                "How long each note rings before fading out.",
                decay,
                0.9,
                0.05..=6.0,
                units(" s", 2),
            );
            slider(
                ui,
                "Brightness",
                "Higher sounds sharper, lower sounds softer.",
                brightness,
                0.5,
                0.0..=1.0,
                percent,
            );
        }
        Instrument::Library { name } => {
            label(ui, "Playing", "A built-in instrument. Pick another above.");
            ui.horizontal(|ui| {
                ui.label(name.as_str());
                hear_button(ui, state, &Instrument::Library { name: name.clone() });
            });
            ui.end_row();
        }
        Instrument::Sample { path, root_note } => {
            file_row(
                ui,
                "File",
                "The WAV file to play. It is tuned up or down to each note.",
                path,
                "WAV audio",
                &["wav"],
            );
            slider(
                ui,
                "Note in the file",
                "The note your recording plays, so it can be tuned to the chords. Middle C is C4.",
                root_note,
                SAMPLE_ROOT_NOTE,
                24..=108,
                note_names,
            );
        }
        Instrument::SampleFolder { path } => {
            label(ui, "Folder", "A folder of WAV files named by the note they play, like C4.wav or F#3.wav.");
            ui.horizontal(|ui| {
                if ui.button("Choose…").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    *path = folder.display().to_string();
                }
                ui.label(file_name(path)).on_hover_text(path.as_str());
            });
            ui.end_row();
            let found = state.folder(path).clone();
            label(ui, "Notes found", "");
            match found {
                Ok(notes) => {
                    let names: Vec<String> = notes.iter().take(12).map(|&n| note_name(n)).collect();
                    let more = if notes.len() > 12 { format!(" and {} more", notes.len() - 12) } else { String::new() };
                    ui.label(RichText::new(format!("{}{more}", names.join(" "))).color(theme::MUTED));
                }
                Err(error) => _ = ui.label(RichText::new(error).color(theme::ERROR)),
            }
            ui.end_row();
        }
        Instrument::SoundFont { path, bank, preset } => {
            file_row(ui, "File", "A SoundFont (.sf2) file.", path, "SoundFont", &["sf2"]);
            label(ui, "Sound", "Which instrument in the SoundFont plays. Drum kits are usually in bank 128.");
            match state.font(path).clone() {
                Ok(presets) => {
                    let shown = presets
                        .iter()
                        .find(|(b, p, _)| b == bank && p == preset)
                        .map_or_else(|| format!("Bank {bank}, sound {preset}"), |(_, _, name)| name.clone());
                    combo(
                        ui,
                        "SoundFont sound",
                        egui::ComboBox::from_id_salt("soundfont-preset").selected_text(shown).width(220.0),
                        |ui| {
                            for (b, p, name) in &presets {
                                let chosen = b == bank && p == preset;
                                if ui.selectable_label(chosen, format!("{name}  ({b}:{p})")).clicked() {
                                    (*bank, *preset) = (*b, *p);
                                }
                            }
                        },
                    );
                }
                Err(error) => _ = ui.label(RichText::new(error).color(theme::ERROR)),
            }
            ui.end_row();
        }
    }
}

/// A drop-down with a name for screen readers, since its label is in the grid's other column.
fn combo(ui: &mut Ui, name: &str, combo: egui::ComboBox, add: impl FnOnce(&mut Ui)) {
    let response = combo.show_ui(ui, add).response;
    named(&response, egui::Role::ComboBox, name);
}

/// Names a widget that shows no label of its own, for screen readers.
fn named(response: &egui::Response, kind: egui::Role, name: &str) {
    let enabled = response.enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(kind, enabled, name));
}

fn hear_button(ui: &mut Ui, state: &mut State, instrument: &Instrument) {
    if ui.small_button("▶ Hear it").on_hover_text("Play a few notes on this instrument").clicked() {
        state.preview(instrument);
    }
}

fn file_row(ui: &mut Ui, text: &str, help: &str, path: &mut String, kind: &str, extensions: &[&str]) {
    label(ui, text, help);
    ui.horizontal(|ui| {
        if ui.button("Choose…").clicked()
            && let Some(file) = pick_file(kind, extensions)
        {
            *path = file;
        }
        ui.label(file_name(path)).on_hover_text(path.as_str());
    });
    ui.end_row();
}

// ---------- notes: chords or a melody ----------

fn notes_section(ui: &mut Ui, scene: &mut Scene, state: &mut State, roll: &mut Option<Roll>) {
    heading(ui, "Notes come from");
    ui.horizontal(|ui| {
        let melody = scene.music.melody.is_some();
        if ui
            .add(egui::Button::selectable(!melody, "Chords"))
            .on_hover_text("Each hit plays a note from the current chord.")
            .clicked()
        {
            scene.music.melody = None;
        }
        if ui
            .add(egui::Button::selectable(melody, "A melody"))
            .on_hover_text("Each hit plays the next note of a tune from a MIDI file.")
            .clicked()
            && !melody
        {
            scene.music.melody = Some(Melody::default());
        }
    });
    match &mut scene.music.melody {
        None => chords_and_notes(ui, &mut scene.sound, roll),
        Some(melody) => melody_settings(ui, melody, state),
    }
}

fn chords_and_notes(ui: &mut Ui, sound: &mut Sound, roll: &mut Option<Roll>) {
    let d = Sound::default();
    heading_dice(ui, "Chords", Group::Chords, roll);
    ui.label(
        RichText::new("Each hit plays a note from the current chord. Click a chord to change it.").color(theme::MUTED),
    );
    chords(ui, &mut sound.progression);
    ui.add_space(6.0);
    heading_dice(ui, "Notes", Group::Notes, roll);
    grid(ui, "notes", |ui| {
        slider(
            ui,
            "Lowest note",
            "The lowest note a hit can play. Middle C is C4.",
            &mut sound.low_note,
            d.low_note,
            24..=108,
            note_names,
        );
        slider(
            ui,
            "Highest note",
            "The highest note a hit can play.",
            &mut sound.high_note,
            d.high_note,
            24..=108,
            note_names,
        );
        if sound.high_note < sound.low_note {
            sound.high_note = sound.low_note;
        }
    });
    ui.add_space(4.0);
    if ui
        .button("Shuffle notes")
        .on_hover_text("Pick a different set of notes for the same chords. Keep clicking until you like it.")
        .clicked()
    {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.subsec_nanos());
        sound.seed = sound.seed.wrapping_mul(6364136223846793005).wrapping_add(nanos as u64 | 1) % 1_000_000;
    }
}

fn melody_settings(ui: &mut Ui, melody: &mut Melody, state: &mut State) {
    let d = Melody::default();
    grid(ui, "melody", |ui| {
        label(ui, "Tune", "A tune that comes with the program, or a MIDI file of your own.");
        let builtin = melody.path.strip_prefix("builtin:").map(str::to_string);
        let shown = match &builtin {
            Some(name) => bpe_audio::midi::BUILTIN
                .iter()
                .find(|(n, ..)| n == name)
                .map_or_else(|| name.clone(), |(_, title, _)| title.to_string()),
            None => file_name(&melody.path),
        };
        combo(ui, "Tune", egui::ComboBox::from_id_salt("melody-file").selected_text(shown).width(220.0), |ui| {
            for (name, title, _) in bpe_audio::midi::BUILTIN {
                if ui.selectable_label(builtin.as_deref() == Some(*name), *title).clicked() {
                    melody.path = format!("builtin:{name}");
                    melody.track = None;
                }
            }
            ui.separator();
            if ui.selectable_label(builtin.is_none(), "Choose a MIDI file…").clicked()
                && let Some(file) = pick_file("MIDI", &["mid", "midi"])
            {
                melody.path = file;
                melody.track = None;
            }
        });
        ui.end_row();

        let file = state.midi(&melody.path).clone();
        match &file {
            Ok(file) => {
                label(
                    ui,
                    "Part",
                    "Which instrument's part of the file to play. Auto picks the busiest part that isn't drums.",
                );
                let auto = file.default_part();
                let describe = |i: u32| {
                    file.parts.get(i as usize).map_or_else(
                        || format!("Part {}", i + 1),
                        |p| {
                            let drums = if p.drums { ", drums" } else { "" };
                            format!("{} ({} notes{drums})", p.name, p.notes)
                        },
                    )
                };
                let shown = match melody.track {
                    None => format!("Auto: {}", auto.map_or_else(|| "none".into(), describe)),
                    Some(i) => describe(i),
                };
                combo(
                    ui,
                    "Part",
                    egui::ComboBox::from_id_salt("melody-part").selected_text(shown).width(220.0),
                    |ui| {
                        ui.selectable_value(&mut melody.track, None, "Auto");
                        for i in 0..file.parts.len() as u32 {
                            ui.selectable_value(&mut melody.track, Some(i), describe(i));
                        }
                    },
                );
                ui.end_row();
            }
            Err(error) => {
                label(ui, "Problem", "");
                ui.label(RichText::new(error).color(theme::ERROR));
                ui.end_row();
            }
        }
        slider(
            ui,
            "Transpose",
            "Shift every note up or down, in semitones. 12 is an octave.",
            &mut melody.transpose,
            d.transpose,
            -24..=24,
            units(" st", 0),
        );
        label(ui, "Next note on", "Which hits move the tune on to its next note.");
        ui.horizontal_wrapped(|ui| {
            for (advance, name, help) in [
                (Advance::AllHits, "Every hit", "Every hit that makes a sound plays the next note."),
                (
                    Advance::WallHits,
                    "Wall hits",
                    "Only hits on the arena's wall move on; other hits repeat the last note.",
                ),
                (Advance::PerBall, "Each ball", "Every ball plays the tune from the start on its own."),
            ] {
                let response = ui.add(egui::Button::selectable(melody.advance_on == advance, name)).on_hover_text(help);
                if response.clicked() {
                    melody.advance_on = advance;
                }
            }
        });
        ui.end_row();
        label(ui, "Harder hits louder", "On: harder hits play louder. Off: each note is as loud as the file says.");
        named(&ui.checkbox(&mut melody.velocity_from_impact, ""), egui::Role::CheckBox, "Harder hits louder");
        ui.end_row();
    });
    if let Ok(file) = state.midi(&melody.path).clone() {
        let steps = file.steps(melody.track, melody.transpose).len();
        let text = format!("{steps} notes, then it starts again from the top.");
        ui.label(RichText::new(text).small().color(theme::MUTED));
    }
}

const ROOTS: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
const QUALITIES: [(&str, &str); 10] = [
    ("", "Major"),
    ("m", "Minor"),
    ("7", "7"),
    ("m7", "m7"),
    ("maj7", "maj7"),
    ("sus2", "sus2"),
    ("sus4", "sus4"),
    ("add9", "add9"),
    ("dim", "dim"),
    ("aug", "aug"),
];

/// Splits a chord name into its root ("F#") and quality ("m7").
fn split_chord(chord: &str) -> (&str, &str) {
    let len = chord.chars().next().map_or(0, char::len_utf8);
    let len = match chord[len..].chars().next() {
        Some('#') | Some('b') => len + 1,
        _ => len,
    };
    chord.split_at(len.min(chord.len()))
}

fn note_names(slider: Slider<'_>) -> Slider<'_> {
    slider.custom_formatter(|n, _| note_name(n as u8)).custom_parser(parse_note)
}

/// Chord buttons. Clicking one opens a small editor for its root, type and length.
fn chords(ui: &mut Ui, progression: &mut Vec<ChordStep>) {
    let mut remove = None;
    let mut swap = None;
    let count = progression.len();
    ui.horizontal_wrapped(|ui| {
        for (i, step) in progression.iter_mut().enumerate() {
            let text = RichText::new(format!("{}  ", step.chord)).strong();
            let response = ui
                .add(egui::Button::new((
                    text,
                    RichText::new(format!("{:.1}s", step.seconds)).small().color(theme::MUTED),
                )))
                .on_hover_text("Click to change this chord");
            Popup::menu(&response).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(300.0);
                let (root, quality) = split_chord(&step.chord);
                let (root, quality) = (root.to_string(), quality.to_string());
                ui.label(RichText::new("Note").color(theme::MUTED));
                ui.horizontal_wrapped(|ui| {
                    for name in ROOTS {
                        if ui.add(egui::Button::selectable(root == name, name)).clicked() {
                            step.chord = format!("{name}{quality}");
                        }
                    }
                });
                ui.label(RichText::new("Type").color(theme::MUTED));
                ui.horizontal_wrapped(|ui| {
                    for (suffix, name) in QUALITIES {
                        if ui.add(egui::Button::selectable(quality == suffix, name)).clicked() {
                            step.chord = format!("{root}{suffix}");
                        }
                    }
                });
                ui.label(RichText::new("Length").color(theme::MUTED));
                ui.add(units(" s", 1)(Slider::new(&mut step.seconds, 0.25..=16.0)));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("Move earlier")).clicked() {
                        swap = Some((i - 1, i));
                    }
                    if ui.add_enabled(i + 1 < count, egui::Button::new("Move later")).clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.add_enabled(count > 1, egui::Button::new("Remove")).clicked() {
                        remove = Some(i);
                        ui.close();
                    }
                });
            });
        }
        if ui.button("+").on_hover_text("Add a chord").clicked() {
            let next = progression.last().cloned().unwrap_or(ChordStep { chord: "C".into(), seconds: 4.0 });
            progression.push(next);
        }
    });
    if let Some((a, b)) = swap {
        progression.swap(a, b);
    }
    if let Some(i) = remove {
        progression.remove(i);
    }
}

// ---------- voices ----------

/// Ready-made voices offered by "Add a voice".
fn voice_ideas() -> [(&'static str, VoiceMap); 5] {
    let drum = |on, key| VoiceMap {
        on,
        instrument: Instrument::library("Drum kit"),
        pitch: PitchMode::Fixed(key),
        ..VoiceMap::default()
    };
    [
        ("Kick drum on wall hits", drum(EventFilter::Walls, 36)),
        ("Snare when balls meet", drum(EventFilter::BallHits, 38)),
        ("Crash cymbal on escapes", drum(EventFilter::Escapes, 49)),
        (
            "Bass on every bar",
            VoiceMap {
                on: EventFilter::Downbeats,
                instrument: Instrument::library("Synth bass"),
                ..VoiceMap::default()
            },
        ),
        (
            "Bells on peg hits",
            VoiceMap {
                on: EventFilter::Obstacles,
                instrument: Instrument::library("Bells"),
                gain: -4.0,
                ..VoiceMap::default()
            },
        ),
    ]
}

fn voices_section(ui: &mut Ui, voices: &mut Vec<VoiceMap>, state: &mut State) {
    heading(ui, "Voices");
    ui.label(
        RichText::new("Give some kinds of event their own instrument. Everything else plays the instrument above.")
            .color(theme::MUTED),
    );
    let mut remove = None;
    for (i, voice) in voices.iter_mut().enumerate() {
        egui::Frame::new().fill(theme::CARD).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let on = ui.checkbox(&mut voice.enabled, "").on_hover_text("Turn this voice on or off");
                named(&on, egui::Role::CheckBox, "Voice on");
                combo(ui, "Plays on", egui::ComboBox::from_id_salt(("voice-on", i)).selected_text(voice.on.name()).width(120.0), |ui| {
                        for filter in EventFilter::ALL {
                            ui.selectable_value(&mut voice.on, filter, filter.name());
                        }
                    },
                );
                ui.label("plays");
                instrument_menu(ui, ("voice-instrument", i), &mut voice.instrument);
                hear_button(ui, state, &voice.instrument);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Remove this voice").clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.horizontal(|ui| {
                let drums = matches!(&voice.instrument, Instrument::Library { name } if Builtin::from_name(name) == Some(Builtin::DrumKit));
                let fixed = matches!(voice.pitch, PitchMode::Fixed(_));
                if ui.add(egui::Button::selectable(!fixed, "The tune")).on_hover_text("Play the melody or chord notes").clicked() {
                    voice.pitch = PitchMode::Tune;
                }
                let one = if drums { "One drum" } else { "One note" };
                if ui.add(egui::Button::selectable(fixed, one)).on_hover_text("Always play the same note").clicked() && !fixed {
                    voice.pitch = PitchMode::Fixed(if drums { 36 } else { 60 });
                }
                if let PitchMode::Fixed(key) = &mut voice.pitch {
                    if drums {
                        let piece = Piece::from_note(*key);
                        combo(ui, "Drum", egui::ComboBox::from_id_salt(("voice-drum", i)).selected_text(piece.name()).width(100.0), |ui| {
                                for piece in Piece::ALL {
                                    if ui.selectable_label(Piece::from_note(*key) == piece, piece.name()).clicked() {
                                        *key = piece.note();
                                    }
                                }
                            },
                        );
                    } else {
                        ui.add(note_names(Slider::new(key, 24..=108)).show_value(true));
                    }
                }
                ui.label(RichText::new("Volume").color(theme::MUTED));
                ui.add(egui::DragValue::new(&mut voice.gain).range(-30.0..=12.0).speed(0.2).suffix(" dB"))
                    .on_hover_text("How loud this voice is. 0 is as loud as the main instrument.");
            });
        });
        ui.add_space(3.0);
    }
    if let Some(i) = remove {
        voices.remove(i);
    }
    let add = ui.button("+ Add a voice").on_hover_text("Pick a ready-made voice, then change it however you like");
    Popup::menu(&add).show(|ui| {
        for (name, voice) in voice_ideas() {
            if ui.button(name).clicked() {
                voices.push(voice);
            }
        }
        ui.separator();
        if ui.button("An empty voice").clicked() {
            voices.push(VoiceMap::default());
        }
    });
}

/// A drop-down of every instrument, for a voice row.
fn instrument_menu(ui: &mut Ui, id: (&str, usize), instrument: &mut Instrument) {
    let shown = match &*instrument {
        Instrument::Sample { path, .. } | Instrument::SampleFolder { path } | Instrument::SoundFont { path, .. } => {
            file_name(path)
        }
        other => Pick::of(other).map_or("Unknown", Pick::name).to_string(),
    };
    combo(ui, "Instrument", egui::ComboBox::from_id_salt(id).selected_text(shown).width(120.0), |ui| {
        let current = Pick::of(instrument);
        for (family, picks) in shelves() {
            ui.label(RichText::new(family).small().color(theme::MUTED));
            for pick in picks {
                if ui.selectable_label(current == Some(pick), pick.name()).on_hover_text(pick.help()).clicked()
                    && let Some(new) = pick.instrument(instrument)
                {
                    *instrument = new;
                }
            }
        }
    });
}

// ---------- loudness ----------

fn loudness_section(ui: &mut Ui, sound: &mut Sound) {
    let d = Sound::default();
    ui.add_space(4.0);
    grid(ui, "loudness", |ui| {
        slider(
            ui,
            "Loudness",
            "Overall loudness of the soundtrack. -14 is the usual level for TikTok, Reels and Shorts.",
            &mut sound.target_loudness,
            d.target_loudness,
            -40.0..=-6.0,
            units(" dB", 0),
        );
    });
    more(ui, "sound-more", |ui| {
        grid(ui, "sound-more-grid", |ui| {
            slider(
                ui,
                "Quietest hit heard",
                "Gentle hits slower than this make no sound, which avoids a buzz when balls roll along the wall.",
                &mut sound.min_impact_speed,
                d.min_impact_speed,
                0.0..=2000.0,
                units(" px/s", 0),
            );
            slider(
                ui,
                "Full volume at",
                "Hits at this speed or faster play at full volume.",
                &mut sound.full_volume_speed,
                d.full_volume_speed,
                1.0..=10000.0,
                log_units(" px/s"),
            );
            slider(
                ui,
                "Stereo width",
                "Pans hits left and right by where they happen. 0% is mono.",
                &mut sound.stereo_width,
                d.stereo_width,
                0.0..=1.0,
                percent,
            );
        });
    });
}

// ---------- song ----------

fn song_section(ui: &mut Ui, music: &mut Music, state: &mut State) {
    let d = Music::default();
    heading(ui, "Song");
    grid(ui, "song", |ui| {
        label(ui, "Song file", "An mp3, wav, ogg or flac file played under the bounce sounds.");
        ui.horizontal(|ui| {
            if ui.button("Choose…").clicked()
                && let Some(file) = pick_file("Songs", &["mp3", "wav", "ogg", "flac"])
            {
                music.song = Some(file);
            }
            match &music.song {
                Some(path) => {
                    ui.label(file_name(path)).on_hover_text(path.as_str());
                    if ui.small_button("✕").on_hover_text("Play no song").clicked() {
                        music.song = None;
                    }
                }
                None => _ = ui.label(RichText::new("No song").color(theme::MUTED)),
            }
        });
        ui.end_row();
        if music.song.is_some() {
            let longest = state.report.song_seconds.map_or(600.0, |s| (s as f32).max(1.0)).max(music.offset);
            slider(
                ui,
                "Start at",
                "How far into the song the video starts, in seconds.",
                &mut music.offset,
                d.offset,
                0.0..=longest,
                units(" s", 2),
            );
            slider(
                ui,
                "Song volume",
                "How loud the song is next to the bounce sounds. 0 is about as loud.",
                &mut music.gain,
                d.gain,
                -24.0..=6.0,
                units(" dB", 1),
            );
            slider(
                ui,
                "Dip under hits",
                "How far the song gets quieter for a moment while bounces play, so they cut through.",
                &mut music.duck_amount,
                d.duck_amount,
                0.0..=1.0,
                percent,
            );
        }
        let help = if music.song.is_some() {
            "Beats per minute. Auto uses the tempo found in the song; anything else overrides it."
        } else {
            "Beats per minute for beat sync and beat events when there's no song. Auto is 120 when needed."
        };
        slider(ui, "Tempo", help, &mut music.bpm, d.bpm, 0.0..=240.0, |s| {
            s.custom_formatter(|v, _| if v <= 0.0 { "Auto".into() } else { format!("{v:.1} BPM") }).custom_parser(|t| {
                if t.trim().eq_ignore_ascii_case("auto") {
                    Some(0.0)
                } else {
                    t.trim().trim_end_matches("BPM").trim().parse().ok()
                }
            })
        });
    });
    if music.song.is_some() {
        if let Some(error) = &state.report.song_error {
            ui.label(RichText::new(format!("The song can't be used: {error}")).small().color(theme::ERROR));
        } else if let Some(bpm) = state.report.song_bpm {
            ui.label(RichText::new(format!("The song's tempo is about {bpm:.0} BPM.")).small().color(theme::MUTED));
        }
    }
}

// ---------- beat sync ----------

fn beat_sync_section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    let d = BeatSync::default();
    heading(ui, "Beat sync");
    let mut fit = false;
    let sync = &mut scene.music.beat_sync;
    ui.horizontal_wrapped(|ui| {
        for (mode, name, help) in [
            (SyncMode::Off, "Off", "Balls move freely."),
            (
                SyncMode::Lock,
                "Lock",
                "Each bounce slightly changes the lead ball's speed so its next bounce lands on a beat.",
            ),
            (
                SyncMode::Nudge,
                "Nudge",
                "Tune gravity, speed and arena size once so as many bounces as possible land on beats.",
            ),
            (
                SyncMode::Warp,
                "Warp",
                "Gently speed the video up and slow it down between bounces so they land on beats.",
            ),
        ] {
            if ui.add(egui::Button::selectable(sync.mode == mode, name)).on_hover_text(help).clicked() {
                sync.mode = mode;
            }
        }
    });
    let explain = match sync.mode {
        SyncMode::Off => "Pick a mode to make bounces land on the beat of the song or tempo.",
        SyncMode::Lock => "The lead balls are steered onto the beat. Works best with light gravity.",
        SyncMode::Nudge => "Click Fit to the beat to tune the scene. You can undo it with Ctrl+Z.",
        SyncMode::Warp => "The video's clock bends between bounces. The physics stays exactly the same.",
    };
    ui.label(RichText::new(explain).color(theme::MUTED));
    grid(ui, "beat-sync", |ui| match sync.mode {
        SyncMode::Off => {}
        SyncMode::Lock => {
            slider(
                ui,
                "Lead balls",
                "How many balls are steered onto the beat, starting with the first ones made.",
                &mut sync.lead_balls,
                d.lead_balls,
                1..=16,
                |s| s,
            );
            slider(
                ui,
                "Most speed change",
                "The most a ball's speed may change at one bounce. Higher locks on more often but looks less natural.",
                &mut sync.max_change,
                d.max_change,
                0.01..=0.4,
                percent,
            );
            label(
                ui,
                "Aim for",
                "Half or quarter beats give the ball more places to land when a whole beat is out of reach.",
            );
            ui.horizontal(|ui| {
                for (n, name) in [(1, "Beats"), (2, "Half beats"), (4, "Quarter beats")] {
                    if ui.add(egui::Button::selectable(sync.subdivisions == n, name)).clicked() {
                        sync.subdivisions = n;
                    }
                }
            });
            ui.end_row();
        }
        SyncMode::Nudge => {
            label(ui, "Tune the scene", "Try many mixes of gravity, launch speed and arena size and keep the best.");
            ui.horizontal(|ui| {
                if state.nudge.is_some() {
                    ui.spinner();
                    ui.label("Trying mixes…");
                } else if ui.button("Fit to the beat").clicked() {
                    fit = true;
                }
            });
            ui.end_row();
        }
        SyncMode::Warp => {
            slider(
                ui,
                "Warp strength",
                "How much the video may speed up or slow down between beats. Higher lands more bounces on beats.",
                &mut sync.warp_strength,
                d.warp_strength,
                0.0..=0.9,
                percent,
            );
        }
    });
    if sync.mode == SyncMode::Nudge
        && let Some(note) = &state.nudge_note
    {
        ui.label(RichText::new(note).small().color(theme::MUTED));
    }
    if sync.mode != SyncMode::Off {
        more(ui, "beat-sync-more", |ui| {
            grid(ui, "beat-sync-more-grid", |ui| {
                slider(
                    ui,
                    "Counts as on the beat",
                    "How close to a beat a hit must be to count as on it, in seconds.",
                    &mut sync.tolerance,
                    d.tolerance,
                    0.005..=0.15,
                    units(" s", 3),
                );
            });
        });
    }
    if let Some(share) = state.report.on_beat {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("On the beat");
            ui.add(
                egui::ProgressBar::new(share.clamp(0.0, 1.0))
                    .desired_width(160.0)
                    .text(format!("{:.0}%", share * 100.0)),
            )
            .on_hover_text("Share of the hits you hear that land on a beat (or half beat, with Lock's setting).");
        });
    }
    if fit {
        start_nudge(ui.ctx(), scene, state);
    }
}

/// What "Fit to the beat" found: the multipliers for gravity, launch speed and arena size, and
/// a line saying how it went.
type NudgeResult = Result<(Option<(f32, f32, f32)>, String), String>;

/// Starts "Fit to the beat" on other threads; [`finish_nudge`] applies it when it's done.
fn start_nudge(ctx: &egui::Context, scene: &Scene, state: &mut State) {
    let (sender, receiver) = channel();
    let (scene, base_dir, ctx) = (scene.clone(), state.base_dir.clone(), ctx.clone());
    std::thread::spawn(move || {
        let result: NudgeResult = (|| {
            let song = bpe_audio::load_song(&scene, base_dir.as_deref()).map_err(|e| format!("{e:#}"))?;
            let inputs = bpe_audio::sim_inputs(&scene, song.as_ref());
            let beats = beat_sync::subdivided(&inputs.beats, scene.music.beat_sync.subdivisions);
            let found = beat_sync::nudge(&scene, &beats);
            let (before, after) = (found.before * 100.0, found.after * 100.0);
            if found.after <= found.before + 1e-6 {
                return Ok((
                    None,
                    format!("No better mix was found ({before:.0}% of hits on the beat), so nothing changed."),
                ));
            }
            let note = format!(
                "Hits on the beat went from {before:.0}% to {after:.0}%: gravity ×{:.2}, launch speed ×{:.2}, arena size ×{:.2}.",
                found.gravity, found.speed, found.size
            );
            Ok((Some((found.gravity, found.speed, found.size)), note))
        })();
        let _ = sender.send(result);
        ctx.request_repaint();
    });
    state.nudge = Some(receiver);
    state.nudge_note = None;
}

/// Applies a finished "Fit to the beat" to the scene as it is now.
fn finish_nudge(scene: &mut Scene, state: &mut State) {
    let Some(receiver) = &state.nudge else { return };
    let result = match receiver.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("it stopped unexpectedly".into()),
    };
    state.nudge = None;
    match result {
        Ok((change, note)) => {
            if let Some((gravity, speed, size)) = change {
                let tuned = beat_sync::scaled(scene, gravity, speed, size);
                scene.physics.gravity = tuned.physics.gravity;
                scene.spawners = tuned.spawners;
                scene.container = tuned.container;
            }
            state.nudge_note = Some(note);
        }
        Err(error) => state.nudge_note = Some(format!("Fit to the beat didn't work: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_names_split_into_root_and_type() {
        assert_eq!(split_chord("F#m7"), ("F#", "m7"));
        assert_eq!(split_chord("Bb"), ("Bb", ""));
        assert_eq!(split_chord("Dsus4"), ("D", "sus4"));
        for root in ROOTS {
            for (quality, _) in QUALITIES {
                assert!(bpe_audio::chord::pitch_classes(&format!("{root}{quality}")).is_some(), "{root}{quality}");
            }
        }
    }

    #[test]
    fn every_builtin_has_a_button() {
        let shown: Vec<Pick> = shelves().into_iter().flat_map(|(_, picks)| picks).collect();
        for builtin in Builtin::ALL {
            let pick = if builtin == Builtin::Pluck { Pick::Pluck } else { Pick::Builtin(builtin) };
            assert!(shown.contains(&pick), "{} has no button", builtin.name());
        }
        assert_eq!(shown.len(), Builtin::ALL.len() + 3);
    }

    #[test]
    fn reset_keeps_files() {
        let music = Music {
            song: Some("song.mp3".into()),
            gain: -10.0,
            melody: Some(Melody { path: "tune.mid".into(), transpose: 5, ..Melody::default() }),
            ..Music::default()
        };
        let fresh = default_music(&music);
        assert_eq!(fresh.song, music.song);
        assert_eq!(fresh.gain, Music::default().gain);
        assert_eq!(fresh.melody, Some(Melody { path: "tune.mid".into(), ..Melody::default() }));
    }
}
