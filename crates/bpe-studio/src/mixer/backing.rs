//! The Mixer's Backing track row (W11): a song that plays and exports with the mix. Load song…
//! (or drop a sound file on the window), ×, On, Starts at, Level, and the song's name and length.

use crate::sound_tab::sample_panel::{LoadedFile, pick_sound_file, read_sound_file};
use crate::style;
use bpe_core::{Address, Scene, SlotId, ThingId, Value};
use bpe_kit::Kit;
use bpe_sound::SampleBank;
use eframe::egui::{self, RichText, Ui};

/// The Mixer setting `key`.
fn at(key: &str) -> Address {
    Address::on(ThingId::ROOT, SlotId::Mixer, key)
}

fn value(scene: &Scene, kit: &Kit, key: &str) -> Option<Value> {
    scene.value_at_time(&kit.registry, &at(key), 0.0)
}

/// The backing track's name ("" for none).
pub fn backing_name(scene: &Scene, kit: &Kit) -> String {
    match value(scene, kit, "backing") {
        Some(Value::Text(t)) => t,
        _ => String::new(),
    }
}

/// Keeps `file` in the scene and makes it the backing track (turned on). False when the scene has
/// no Mixer to hold it.
pub fn set_backing(scene: &mut Scene, kit: &Kit, file: &LoadedFile) -> bool {
    bpe_kit::ensure_mixer(scene, &kit.registry);
    let name = scene.add_audio(&file.name(), file.bytes.clone());
    scene.set_at(&kit.registry, &at("backing"), Value::Text(name)).is_ok()
        && scene.set_at(&kit.registry, &at("backing_on"), Value::Bool(true)).is_ok()
}

/// What the row says under a song that can't play, or None.
pub fn backing_problem(scene: &Scene, name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    match scene.audio.get(name) {
        None => Some("Song not found: load it again with Load song…".into()),
        Some(a) => match SampleBank::ready(&a.bytes) {
            Some(Err(e)) => Some(e),
            _ => None,
        },
    }
}

/// A length in seconds as "3:25".
fn length(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Draws the row. A problem reading a chosen file goes in `note`.
pub fn row(ui: &mut Ui, scene: &mut Scene, kit: &Kit, note: &mut Option<String>) {
    let name = backing_name(scene, kit);
    let data = scene.audio.get(&name).and_then(|a| SampleBank::ready(&a.bytes));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new("Backing track").color(style::MUTED));
        if name.is_empty() {
            ui.label(RichText::new("None").color(style::MUTED));
        } else {
            let shown = match &data {
                Some(Ok(d)) => format!("{name}  {}", length(d.seconds())),
                _ => name.clone(),
            };
            ui.label(RichText::new(shown).color(style::TEXT));
        }
        let load = ui.button("Load song…").on_hover_text(
            "A song (WAV, FLAC, MP3, OGG or AIFF) that plays and exports with the mix. You can also drop one on \
                 the window",
        );
        if load.clicked()
            && let Some(path) = pick_sound_file("Load a backing track")
        {
            match read_sound_file(&path) {
                Ok(file) => {
                    set_backing(scene, kit, &file);
                    *note = None;
                }
                Err(e) => *note = Some(e),
            }
        }
        if name.is_empty() {
            return;
        }
        let remove = ui.button("×").on_hover_text("No backing track");
        crate::sound_tab::name_it(ui, &remove, "Remove the backing track");
        if remove.clicked() {
            let _ = scene.set_at(&kit.registry, &at("backing"), Value::Text(String::new()));
            *note = None;
        }
        ui.add_space(8.0);
        let mut on = matches!(value(scene, kit, "backing_on"), Some(Value::Bool(true)));
        let r = style::switch(ui, &mut on).on_hover_text("Play the backing track");
        crate::sound_tab::name_it(ui, &r, "Backing track on");
        if r.changed() {
            let _ = scene.set_at(&kit.registry, &at("backing_on"), Value::Bool(on));
        }
        ui.add_space(8.0);
        let label = ui.label(RichText::new("Starts at").color(style::MUTED));
        let mut start = value(scene, kit, "backing_at").as_ref().and_then(super::number).unwrap_or(0.0);
        let r = ui
            .add(egui::DragValue::new(&mut start).range(-600.0..=600.0).speed(0.005).max_decimals(3).suffix(" s"))
            .labelled_by(label.id)
            .on_hover_text("When the song starts in the video; below 0 the video starts partway into the song");
        if r.changed() {
            let _ = scene.set_at(&kit.registry, &at("backing_at"), Value::Number(start));
        }
        ui.add_space(8.0);
        let label = ui.label(RichText::new("Level").color(style::MUTED));
        let mut level = value(scene, kit, "backing_level").as_ref().and_then(super::number).unwrap_or(0.0);
        let r = ui
            .add(egui::DragValue::new(&mut level).range(-60.0..=12.0).speed(0.1).max_decimals(1).suffix(" dB"))
            .labelled_by(label.id);
        if r.changed() {
            let _ = scene.set_at(&kit.registry, &at("backing_level"), Value::Number(level));
        }
    });
    if data.is_none() && scene.audio.contains_key(&name) {
        ui.label(RichText::new("Reading the song…").small().color(style::MUTED));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    if let Some(problem) = note.clone().or_else(|| backing_problem(scene, &name)) {
        ui.label(RichText::new(problem).small().color(style::PROBLEM));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_song_is_kept_and_becomes_the_backing_track() {
        let kit = bpe_sound::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let file = LoadedFile { file: "My song.wav".into(), bytes: bpe_sound::audio::sine_wav(220.0, 0.1, 48_000, 1) };
        assert!(set_backing(&mut scene, &kit, &file));
        assert_eq!(backing_name(&scene, &kit), "My song");
        assert!(scene.audio.contains_key("My song"));
        assert_eq!(value(&scene, &kit, "backing_on"), Some(Value::Bool(true)));
        assert_eq!(backing_problem(&scene, "Gone").as_deref(), Some("Song not found: load it again with Load song…"));
        assert_eq!(length(205.4), "3:25");
    }

    #[test]
    fn the_row_shows_the_song_and_its_settings() {
        use egui_kittest::kittest::Queryable;
        let mut h = crate::mixer::strips::tests::mixer_harness();
        assert!(h.query_by_label("Load song…").is_some());
        assert!(h.query_by_label("Starts at").is_none(), "nothing to set without a song");
        let kit = h.state().kit.clone();
        let file = LoadedFile { file: "Song.wav".into(), bytes: bpe_sound::audio::sine_wav(220.0, 0.2, 48_000, 2) };
        // Read now, so the row's length doesn't move the buttons between frames.
        bpe_sound::SampleBank::get(&file.bytes).unwrap();
        assert!(set_backing(&mut h.state_mut().scene, &kit, &file));
        h.run();
        assert!(h.query_by_label_contains("0:00").is_some(), "the song's length");
        assert!(h.query_by_label("Starts at").is_some());
        assert!(h.query_by_label("Backing track on").is_some());
        h.get_by_label("Remove the backing track").click();
        h.run();
        assert_eq!(backing_name(&h.state().scene, &kit), "");
    }
}
