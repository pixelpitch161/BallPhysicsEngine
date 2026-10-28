//! The scene property panel. Every widget edits the [`Scene`] in place; the app notices the
//! change and rebuilds the preview and soundtrack.

use bpe_scene::{
    BounceRules, ChordStep, Color, Container, Instrument, Look, Physics, Scene, Sound, Spawner, Vec2, Video,
};
use eframe::egui::{self, Color32, DragValue, Ui};

pub fn show(ui: &mut Ui, scene: &mut Scene, progression_text: &mut String) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut scene.name);
    });
    ui.add_space(4.0);
    section(ui, "Video", true, |ui| video(ui, &mut scene.video));
    section(ui, "Container", true, |ui| container(ui, &mut scene.container));
    section(ui, "Balls", true, |ui| spawners(ui, &mut scene.spawners));
    section(ui, "Physics", false, |ui| physics(ui, &mut scene.physics));
    section(ui, "On each wall hit", false, |ui| rules(ui, &mut scene.rules));
    section(ui, "Look", true, |ui| look(ui, &mut scene.look));
    section(ui, "Sound", true, |ui| sound(ui, &mut scene.sound, progression_text));
}

fn section(ui: &mut Ui, title: &str, open: bool, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong()).default_open(open).show(ui, add);
}

fn grid(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([12.0, 6.0]).striped(false).show(ui, add);
}

fn number(ui: &mut Ui, label: &str, value: &mut f32, speed: f64, range: std::ops::RangeInclusive<f32>, suffix: &str) {
    ui.label(label);
    ui.add(DragValue::new(value).speed(speed).range(range).suffix(suffix));
    ui.end_row();
}

fn integer(ui: &mut Ui, label: &str, value: &mut u32, range: std::ops::RangeInclusive<u32>) {
    ui.label(label);
    ui.add(DragValue::new(value).range(range));
    ui.end_row();
}

fn vec2(ui: &mut Ui, label: &str, value: &mut Vec2, speed: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(DragValue::new(&mut value.x).speed(speed).prefix("x "));
        ui.add(DragValue::new(&mut value.y).speed(speed).prefix("y "));
    });
    ui.end_row();
}

fn color(ui: &mut Ui, label: &str, value: &mut Color) {
    ui.label(label);
    color_button(ui, value);
    ui.end_row();
}

fn color_button(ui: &mut Ui, value: &mut Color) {
    let mut c = Color32::from_rgba_unmultiplied(value.r, value.g, value.b, value.a);
    if ui.color_edit_button_srgba(&mut c).changed() {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        *value = Color { r, g, b, a };
    }
}

fn video(ui: &mut Ui, video: &mut Video) {
    grid(ui, "video", |ui| {
        integer(ui, "Width", &mut video.width, 16..=4096);
        integer(ui, "Height", &mut video.height, 16..=4096);
        integer(ui, "Frames per second", &mut video.fps, 1..=240);
        number(ui, "Length", &mut video.duration, 0.1, 0.5..=600.0, " s");
    });
    if ui.small_button("Vertical 1080 x 1920").clicked() {
        (video.width, video.height) = (1080, 1920);
    }
}

fn container(ui: &mut Ui, container: &mut Container) {
    let is_circle = matches!(container, Container::Circle { .. });
    ui.horizontal(|ui| {
        if ui.selectable_label(is_circle, "Ring").clicked() && !is_circle {
            *container = Container::Circle {
                center: Vec2::ZERO,
                radius: 508.0,
                thickness: 3.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
            };
        }
        if ui.selectable_label(!is_circle, "Box").clicked() && is_circle {
            *container = Container::Rect {
                center: Vec2::ZERO,
                size: Vec2::new(1000.0, 1000.0),
                thickness: 3.0,
                color: Color::rgb(0xd8, 0xdc, 0xd0),
            };
        }
    });
    grid(ui, "container", |ui| match container {
        Container::Circle { center, radius, thickness, color: c } => {
            vec2(ui, "Centre", center, 1.0);
            number(ui, "Radius", radius, 1.0, 10.0..=4000.0, " px");
            number(ui, "Line width", thickness, 0.1, 0.0..=100.0, " px");
            color(ui, "Colour", c);
        }
        Container::Rect { center, size, thickness, color: c } => {
            vec2(ui, "Centre", center, 1.0);
            vec2(ui, "Size", size, 1.0);
            number(ui, "Line width", thickness, 0.1, 0.0..=100.0, " px");
            color(ui, "Colour", c);
        }
    });
}

fn spawners(ui: &mut Ui, spawners: &mut Vec<Spawner>) {
    let mut remove = None;
    for (index, spawner) in spawners.iter_mut().enumerate() {
        let title = match spawner {
            Spawner::Grid { columns, rows, .. } => format!("Grid {columns} x {rows}"),
            Spawner::Single { .. } => "Single ball".to_string(),
        };
        ui.push_id(index, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(title).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                });
                grid(ui, "spawner", |ui| match spawner {
                    Spawner::Grid { center, columns, rows, spacing, radius, velocity, gradient } => {
                        vec2(ui, "Centre", center, 1.0);
                        integer(ui, "Columns", columns, 1..=200);
                        integer(ui, "Rows", rows, 1..=200);
                        number(ui, "Spacing", spacing, 0.1, 0.0..=500.0, " px");
                        number(ui, "Ball radius", radius, 0.1, 0.5..=200.0, " px");
                        vec2(ui, "Velocity", velocity, 1.0);
                        ui.label("Gradient (top to bottom)");
                        color_list(ui, gradient);
                        ui.end_row();
                    }
                    Spawner::Single { position, velocity, radius, color: c } => {
                        vec2(ui, "Position", position, 1.0);
                        vec2(ui, "Velocity", velocity, 1.0);
                        number(ui, "Radius", radius, 0.1, 0.5..=500.0, " px");
                        color(ui, "Colour", c);
                    }
                });
            });
        });
    }
    if let Some(index) = remove {
        spawners.remove(index);
    }
    ui.horizontal(|ui| {
        if ui.button("Add grid").clicked() {
            spawners.push(Spawner::Grid {
                center: Vec2::ZERO,
                columns: 10,
                rows: 10,
                spacing: 20.0,
                radius: 5.0,
                velocity: Vec2::new(300.0, 0.0),
                gradient: vec![Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(0xff, 0xb8, 0x2e)],
            });
        }
        if ui.button("Add ball").clicked() {
            spawners.push(Spawner::Single {
                position: Vec2::ZERO,
                velocity: Vec2::new(300.0, -200.0),
                radius: 20.0,
                color: Color::rgb(0xff, 0x5c, 0xa8),
            });
        }
    });
}

fn color_list(ui: &mut Ui, colors: &mut Vec<Color>) {
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        for (i, c) in colors.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                let response = ui.scope(|ui| color_button(ui, c)).response;
                response.context_menu(|ui| {
                    if ui.button("Remove colour").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            colors.remove(i);
        }
        if ui.small_button("+").on_hover_text("Add a colour (right-click a colour to remove it)").clicked() {
            colors.push(colors.last().copied().unwrap_or(Color::WHITE));
        }
    });
}

fn physics(ui: &mut Ui, physics: &mut Physics) {
    grid(ui, "physics", |ui| {
        vec2(ui, "Gravity", &mut physics.gravity, 1.0);
        number(ui, "Bounciness", &mut physics.restitution, 0.005, 0.0..=1.5, "");
        number(ui, "Air drag", &mut physics.air_drag, 0.005, 0.0..=10.0, " /s");
        integer(ui, "Steps per second", &mut physics.steps_per_second, 30..=2000);
        ui.label("Balls hit each other");
        ui.checkbox(&mut physics.ball_collisions, "");
        ui.end_row();
    });
}

fn rules(ui: &mut Ui, rules: &mut BounceRules) {
    grid(ui, "rules", |ui| {
        number(ui, "Speed multiplier", &mut rules.speed_gain, 0.001, 0.5..=2.0, "x");
        number(ui, "Speed limit", &mut rules.max_speed, 10.0, 10.0..=20000.0, " px/s");
        number(ui, "Grow by", &mut rules.grow, 0.05, -50.0..=50.0, " px");
        number(ui, "Largest radius", &mut rules.max_radius, 1.0, 1.0..=2000.0, " px");
        ui.label("Change colour to");
        color_list(ui, &mut rules.recolor);
        ui.end_row();
    });
}

fn look(ui: &mut Ui, look: &mut Look) {
    grid(ui, "look", |ui| {
        color(ui, "Background top", &mut look.background_top);
        color(ui, "Background bottom", &mut look.background_bottom);
        number(ui, "Trail length", &mut look.trail_seconds, 0.01, 0.0..=2.0, " s");
        number(ui, "Trail width", &mut look.trail_width, 0.01, 0.0..=2.0, "");
        number(ui, "Trail brightness", &mut look.trail_opacity, 0.01, 0.0..=1.0, "");
        number(ui, "Ball glow", &mut look.ball_glow, 0.01, 0.0..=2.0, "");
        number(ui, "Hit flash", &mut look.flash_strength, 0.01, 0.0..=5.0, "");
        number(ui, "Flash length", &mut look.flash_seconds, 0.01, 0.0..=2.0, " s");
        number(ui, "Flash size", &mut look.flash_size, 0.5, 1.0..=300.0, " px");
        number(ui, "Bloom", &mut look.bloom_strength, 0.01, 0.0..=3.0, "");
        number(ui, "Exposure", &mut look.exposure, 0.01, 0.1..=4.0, "");
    });
}

fn sound(ui: &mut Ui, sound: &mut Sound, progression_text: &mut String) {
    ui.checkbox(&mut sound.enabled, "Play a note on every wall hit");
    ui.label("Chords, each with its length in seconds:");
    let response = ui.add(
        egui::TextEdit::singleline(progression_text).hint_text("C:4, G:4, Am:4, F:4").desired_width(f32::INFINITY),
    );
    match parse_progression(progression_text) {
        Ok(progression) => {
            if response.changed() {
                sound.progression = progression;
            }
        }
        Err(message) => {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
    }
    grid(ui, "sound", |ui| {
        ui.label("Note range");
        ui.horizontal(|ui| {
            ui.add(DragValue::new(&mut sound.low_note).range(0..=127).custom_formatter(|n, _| note_name(n as u8)));
            ui.label("to");
            ui.add(DragValue::new(&mut sound.high_note).range(0..=127).custom_formatter(|n, _| note_name(n as u8)));
        });
        ui.end_row();
        ui.label("Instrument");
        ui.horizontal(|ui| {
            let is_pluck = matches!(sound.instrument, Instrument::Pluck { .. });
            if ui.selectable_label(is_pluck, "Pluck").clicked() && !is_pluck {
                sound.instrument = Instrument::Pluck { decay: 0.9, brightness: 0.5 };
            }
            if ui.selectable_label(!is_pluck, "WAV sample").clicked() && is_pluck {
                sound.instrument = Instrument::Sample { path: String::new(), root_note: 60 };
            }
        });
        ui.end_row();
        match &mut sound.instrument {
            Instrument::Pluck { decay, brightness } => {
                number(ui, "Ring time", decay, 0.01, 0.05..=6.0, " s");
                number(ui, "Brightness", brightness, 0.01, 0.0..=1.0, "");
            }
            Instrument::Sample { path, root_note } => {
                ui.label("File");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(path).desired_width(160.0));
                    if ui.button("Browse").clicked()
                        && let Some(file) = rfd::FileDialog::new().add_filter("WAV audio", &["wav"]).pick_file()
                    {
                        *path = file.display().to_string();
                    }
                });
                ui.end_row();
                ui.label("Sample's own note");
                ui.add(DragValue::new(root_note).range(0..=127).custom_formatter(|n, _| note_name(n as u8)));
                ui.end_row();
            }
        }
        number(ui, "Quietest hit heard", &mut sound.min_impact_speed, 1.0, 0.0..=5000.0, " px/s");
        number(ui, "Full volume at", &mut sound.full_volume_speed, 1.0, 1.0..=10000.0, " px/s");
        number(ui, "Stereo width", &mut sound.stereo_width, 0.01, 0.0..=1.0, "");
        number(ui, "Loudness", &mut sound.target_loudness, 0.1, -40.0..=-6.0, " dB");
        ui.label("Note seed");
        ui.add(DragValue::new(&mut sound.seed));
        ui.end_row();
    });
}

pub fn progression_to_text(progression: &[ChordStep]) -> String {
    progression.iter().map(|s| format!("{}:{}", s.chord, s.seconds)).collect::<Vec<_>>().join(", ")
}

fn parse_progression(text: &str) -> Result<Vec<ChordStep>, String> {
    text.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (chord, seconds) = part.split_once(':').unwrap_or((part, "4"));
            let chord = chord.trim();
            if bpe_audio::chord::pitch_classes(chord).is_none() {
                return Err(format!("I don't know the chord \"{chord}\""));
            }
            let seconds: f32 =
                seconds.trim().parse().map_err(|_| format!("\"{seconds}\" is not a number of seconds"))?;
            Ok(ChordStep { chord: chord.to_string(), seconds })
        })
        .collect()
}

fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[note as usize % 12], note as i32 / 12 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progression_text_round_trips() {
        let progression = Sound::default().progression;
        assert_eq!(parse_progression(&progression_to_text(&progression)).unwrap(), progression);
        assert!(parse_progression("C:4, Xm:2").is_err());
    }

    #[test]
    fn note_names() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
    }
}
