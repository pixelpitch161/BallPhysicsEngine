//! The inspector in Video mode (part 2 plan, Task 4): the Video's own settings, its song and
//! Master; a clip; a transition; a kept scene. Each edit is one undo step, and a number dragged is
//! one step for the whole drag.

use super::{VideoPick, arrange, scenes};
use crate::app::{Studio, clock};
use crate::style;
use bpe_core::video::{ClipId, Direction, KeptId, Song, TransitionKind, TransitionSound};
use eframe::egui::{self, RichText, Ui};

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).size(12.0).color(style::MUTED));
    ui.add_space(2.0);
}

/// A number box named by the label before it.
fn number<T: egui::emath::Numeric>(
    ui: &mut Ui,
    name: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    speed: f64,
    suffix: &str,
    hint: &str,
) {
    let label = ui.label(name).on_hover_text(hint);
    style::wells(ui, |ui| ui.add(egui::DragValue::new(value).range(range).speed(speed).suffix(suffix)))
        .labelled_by(label.id)
        .on_hover_text(hint);
    ui.end_row();
}

/// Chips for one of a few choices, named by the label before them.
fn chips<T: Copy + PartialEq>(ui: &mut Ui, name: &str, value: &mut T, choices: &[(T, &str)]) {
    ui.label(name);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for &(choice, words) in choices {
            if ui.add(style::toggle_button(words, *value == choice)).clicked() {
                *value = choice;
            }
        }
    });
    ui.end_row();
}

/// What the inspector shows.
pub fn show(ui: &mut Ui, studio: &mut Studio) {
    // Edits made while the pointer is held (a number dragged) are one undo step.
    let merge = ui.input(|i| i.pointer.any_down()) || ui.ctx().text_edit_focused();
    if !merge {
        studio.video.history.end_merging();
    }
    match studio.video.pick() {
        VideoPick::Video => video(ui, studio, merge),
        VideoPick::Clip(id) => clip(ui, studio, id, merge),
        VideoPick::Transition(id) => transition(ui, studio, id, merge),
        VideoPick::Scene(id) => scene(ui, studio, id, merge),
    }
}

fn video(ui: &mut Ui, studio: &mut Studio, merge: bool) {
    ui.label(RichText::new(studio.video.name()).size(15.0).color(style::TEXT_STRONG));
    ui.label(
        RichText::new(format!("{} clips · {} long", studio.video.video.clips.len(), clock(studio.video.video.end())))
            .small()
            .color(style::MUTED),
    );
    let mut s = studio.video.video.settings.clone();
    heading(ui, "PICTURE");
    egui::Grid::new("video picture").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        number(
            ui,
            "Width",
            &mut s.width,
            16..=4096,
            2.0,
            " px",
            "The video's width. Scenes of another shape are fitted to cover it.",
        );
        number(ui, "Height", &mut s.height, 16..=4096, 2.0, " px", "The video's height.");
        chips(ui, "Frame rate", &mut s.fps, &[(24, "24"), (25, "25"), (30, "30"), (50, "50"), (60, "60")]);
    });
    heading(ui, "BEAT GRID");
    egui::Grid::new("video beat").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        number(
            ui,
            "Tempo",
            &mut s.tempo,
            20.0..=400.0,
            0.2,
            " BPM",
            "The song's tempo: the bars on the timeline and every clip that follows the video's tempo.",
        );
        number(ui, "Beats in a bar", &mut s.time_top, 1..=16, 0.05, "", "The time signature's top number.");
        chips(ui, "Beat note", &mut s.time_bottom, &[(2, "2"), (4, "4"), (8, "8"), (16, "16")]);
        number(
            ui,
            "First beat at",
            &mut s.first_beat,
            0.0..=600.0,
            0.005,
            " s",
            "Where bar 1 starts, so the bars line up with a song that doesn't start on one.",
        );
        chips(
            ui,
            "Snap",
            &mut s.snap,
            &[
                (bpe_core::video::Snap::Bar, "Bar"),
                (bpe_core::video::Snap::Beat, "Beat"),
                (bpe_core::video::Snap::Off, "Off"),
            ],
        );
    });
    heading(ui, "MASTER");
    egui::Grid::new("video master").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        let m = &mut s.master;
        number(ui, "Input", &mut m.input, -24.0..=24.0, 0.05, " dB", "Gain into the video's limiter.");
        number(
            ui,
            "Ceiling",
            &mut m.ceiling,
            -12.0..=0.0,
            0.02,
            " dB",
            "The loudest the limiter lets through (true peak).",
        );
        number(ui, "Release", &mut m.release, 1.0..=1000.0, 1.0, " ms", "How fast the limiter lets go.");
        number(ui, "Output", &mut m.output, -24.0..=24.0, 0.05, " dB", "Gain after the limiter.");
        number(ui, "End fade", &mut m.end_fade, 0.0..=30.0, 0.02, " s", "Fades the whole video out at its end.");
    });
    if s != studio.video.video.settings {
        studio.video.change(merge, |v| v.settings = s);
    }
    song(ui, studio, merge);
}

fn song(ui: &mut Ui, studio: &mut Studio, merge: bool) {
    heading(ui, "SONG");
    let mut song = studio.video.video.song.clone();
    ui.horizontal(|ui| {
        match &song {
            Some(s) => {
                let length = bpe_sound::SampleBank::ready(&s.audio.bytes)
                    .and_then(Result::ok)
                    .map(|d| format!(" · {}", scenes::short_length(d.seconds())))
                    .unwrap_or_default();
                ui.label(RichText::new(format!("{}{length}", s.name)).color(style::TEXT));
            }
            None => _ = ui.label(RichText::new("No song").color(style::MUTED)),
        }
        let load = ui.button("Load song…").on_hover_text("A song (WAV, FLAC, MP3, OGG or AIFF) under the whole video");
        if load.clicked()
            && let Some(path) = crate::sound_tab::sample_panel::pick_sound_file("Load the video's song")
        {
            match crate::sound_tab::sample_panel::read_sound_file(&path) {
                Ok(file) => {
                    let mut new = song.clone().unwrap_or_default();
                    new.name = file.name();
                    new.audio = bpe_core::scene_audio::SceneAudio::new(file.bytes);
                    song = Some(new);
                }
                Err(problem) => studio.report(crate::app::ProblemKind::File, problem),
            }
        }
        if song.is_some() && ui.button("×").on_hover_text("Take the song out of the video").clicked() {
            song = None;
        }
    });
    if let Some(s) = &mut song {
        egui::Grid::new("video song").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            let on = ui.label("On");
            style::switch(ui, &mut s.on).labelled_by(on.id);
            ui.end_row();
            number(
                ui,
                "Starts at",
                &mut s.starts_at,
                -600.0..=1800.0,
                0.01,
                " s",
                "Where the song starts in the video; below 0 skips its intro.",
            );
            number(ui, "Level", &mut s.level_db, -60.0..=12.0, 0.05, " dB", "The song's level in the mix.");
        });
    }
    if song != studio.video.video.song {
        let song: Option<Song> = song;
        studio.video.change(merge, |v| v.song = song);
    }
}

fn clip(ui: &mut Ui, studio: &mut Studio, id: ClipId, merge: bool) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let Some(mut c) = arrange::clip(&studio.video.video, id).cloned() else { return };
    let before = c.clone();
    let scene_name = studio.video.video.scenes.get(&c.scene).map(|k| k.name.clone()).unwrap_or_default();
    ui.label(RichText::new(format!("Clip of {scene_name}")).size(15.0).color(style::TEXT_STRONG));
    let s = studio.video.video.settings.clone();
    let (bar, beat) = arrange::bar_beat(&s, c.start);
    ui.label(
        RichText::new(format!("Starts at bar {bar}.{beat} · ends at {}", clock(c.end()))).small().color(style::MUTED),
    );
    heading(ui, "CLIP");
    egui::Grid::new("video clip").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        let label = ui.label("Scene");
        ui.menu_button(scene_name.clone(), |ui| {
            for (k, kept) in &studio.video.video.scenes {
                if ui.button(&kept.name).clicked() {
                    c.scene = *k;
                    ui.close();
                }
            }
        })
        .response
        .labelled_by(label.id);
        ui.end_row();
        number(ui, "Start", &mut c.start, 0.0..=1800.0, 0.01, " s", "Where the clip begins in the video.");
        number(
            ui,
            "Length",
            &mut c.length,
            0.0..=1800.0,
            0.01,
            " s",
            "How long it plays (never past its scene's end).",
        );
        number(
            ui,
            "Starts in the scene at",
            &mut c.offset,
            0.0..=600.0,
            0.01,
            " s",
            "How far into its scene the clip begins.",
        );
        number(ui, "Level", &mut c.level_db, -60.0..=12.0, 0.05, " dB", "The scene's sound in the video's mix.");
        let follow = ui
            .label("Follow video tempo")
            .on_hover_text("The scene plays at the video's tempo and beat grid, so its beats land on the song's");
        style::switch(ui, &mut c.follow_video_tempo).labelled_by(follow.id);
        ui.end_row();
        let song = ui.label("Scene song").on_hover_text("Play the scene's own backing track as part of its sound");
        style::switch(ui, &mut c.scene_song).labelled_by(song.id);
        ui.end_row();
    });
    if c.follow_video_tempo {
        ui.label(
            RichText::new(format!("This scene plays at the video's {} BPM.", trim_number(s.tempo)))
                .small()
                .color(style::MUTED),
        );
    }
    let has_song = studio
        .video
        .video
        .scenes
        .get(&c.scene)
        .is_some_and(|k| !crate::mixer::backing::backing_name(&k.scene, &kit).is_empty());
    if c.scene_song && c.follow_video_tempo && has_song {
        ui.label(RichText::new("The scene's song keeps its own speed.").small().color(style::MUTED));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Duplicate").on_hover_text("A copy just after it (Ctrl+D)").clicked() {
            let mut made = None;
            studio.video.change(false, |v| made = arrange::duplicate_clip(v, reg, id));
            if let Some(made) = made {
                studio.video.pick = VideoPick::Clip(made);
            }
        }
        if ui.button("Delete clip").on_hover_text("Delete").clicked() {
            studio.video.change(false, |v| _ = arrange::delete_clip(v, id));
        }
    });
    if c != before {
        studio.video.change(merge, |v| {
            if let Some(slot) = v.clips.iter_mut().find(|x| x.id == id) {
                *slot = c;
            }
            arrange::tidy(v, reg);
        });
    }
}

/// A number without trailing zeros: "128", "92.5".
fn trim_number(v: f32) -> String {
    let text = format!("{v:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn direction_chips(ui: &mut Ui, direction: &mut Direction) {
    chips(
        ui,
        "Direction",
        direction,
        &[(Direction::Left, "Left"), (Direction::Right, "Right"), (Direction::Up, "Up"), (Direction::Down, "Down")],
    );
}

/// The kind of transition, without its settings, for the chips.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Cut,
    Crossfade,
    FadeThrough,
    Flash,
    Wipe,
    Push,
    ZoomThrough,
}

fn kind_of(kind: TransitionKind) -> Kind {
    match kind {
        TransitionKind::Cut => Kind::Cut,
        TransitionKind::Crossfade => Kind::Crossfade,
        TransitionKind::FadeThrough { .. } => Kind::FadeThrough,
        TransitionKind::Flash { .. } => Kind::Flash,
        TransitionKind::Wipe { .. } => Kind::Wipe,
        TransitionKind::Push { .. } => Kind::Push,
        TransitionKind::ZoomThrough => Kind::ZoomThrough,
    }
}

fn kind_from(kind: Kind) -> TransitionKind {
    match kind {
        Kind::Cut => TransitionKind::Cut,
        Kind::Crossfade => TransitionKind::Crossfade,
        Kind::FadeThrough => TransitionKind::FADE_THROUGH,
        Kind::Flash => TransitionKind::FLASH,
        Kind::Wipe => TransitionKind::WIPE,
        Kind::Push => TransitionKind::Push { direction: Direction::Left },
        Kind::ZoomThrough => TransitionKind::ZoomThrough,
    }
}

fn transition(ui: &mut Ui, studio: &mut Studio, id: ClipId, merge: bool) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let video = &studio.video.video;
    let Some(i) = arrange::index_of(video, id) else { return };
    let (from, to) = (
        video.scenes.get(&video.clips[i - 1].scene).map(|k| k.name.clone()).unwrap_or_default(),
        video.scenes.get(&video.clips[i].scene).map(|k| k.name.clone()).unwrap_or_default(),
    );
    let overlap = video.overlap(i);
    let beat = video.settings.beat_seconds();
    let mut t = video.clips[i].transition_in;
    let before = t;
    ui.label(RichText::new("Transition").size(15.0).color(style::TEXT_STRONG));
    ui.label(RichText::new(format!("From {from} into {to}")).small().color(style::MUTED));
    heading(ui, "PICTURE");
    let mut beats = overlap / beat;
    egui::Grid::new("video transition").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        let mut kind = kind_of(t.kind);
        chips(
            ui,
            "Kind",
            &mut kind,
            &[
                (Kind::Cut, "Cut"),
                (Kind::Crossfade, "Crossfade"),
                (Kind::FadeThrough, "Fade through colour"),
                (Kind::Flash, "Flash"),
                (Kind::Wipe, "Wipe"),
                (Kind::Push, "Push"),
                (Kind::ZoomThrough, "Zoom through"),
            ],
        );
        if kind != kind_of(t.kind) {
            t.kind = kind_from(kind);
        }
        match &mut t.kind {
            TransitionKind::FadeThrough { colour } | TransitionKind::Flash { colour } => {
                let label = ui.label("Colour");
                ui.color_edit_button_srgb(colour).labelled_by(label.id);
                ui.end_row();
            }
            TransitionKind::Wipe { direction, softness } => {
                direction_chips(ui, direction);
                let mut percent = *softness * 100.0;
                number(
                    ui,
                    "Soft edge",
                    &mut percent,
                    0.0..=30.0,
                    0.1,
                    " %",
                    "How soft the wipe's edge is, as a share of the screen.",
                );
                *softness = percent / 100.0;
            }
            TransitionKind::Push { direction } => direction_chips(ui, direction),
            _ => {}
        }
        number(
            ui,
            "Length",
            &mut beats,
            0.0..=64.0,
            0.02,
            " beats",
            "How long the two clips overlap. At most half the shorter clip.",
        );
        chips(
            ui,
            "Sound",
            &mut t.sound,
            &[(TransitionSound::Crossfade, "Crossfade"), (TransitionSound::KeepBoth, "Keep both")],
        );
    });
    if t.kind == TransitionKind::Cut {
        ui.label(RichText::new("A cut over an overlap shows the clip coming in alone.").small().color(style::MUTED));
    }
    ui.label(RichText::new(format!("{} s", trim_number((overlap) as f32))).small().color(style::MUTED));
    if t != before {
        studio.video.change(merge, |v| {
            if let Some(c) = v.clips.iter_mut().find(|c| c.id == id) {
                c.transition_in = t;
            }
        });
    }
    if (beats - overlap / beat).abs() > 1e-9 {
        studio.video.change(merge, |v| arrange::set_overlap(v, reg, id, beats * beat));
    }
}

fn scene(ui: &mut Ui, studio: &mut Studio, id: KeptId, merge: bool) {
    let kit = studio.kit.clone();
    let Some(kept) = studio.video.video.scenes.get(&id) else { return };
    let mut name = kept.name.clone();
    let mut colour = kept.colour;
    let length = bpe_kit::video::scene_length(&kept.scene, &kit.registry);
    let clips = arrange::clips_of(&studio.video.video, id);
    ui.label(RichText::new("Scene in the video").size(15.0).color(style::TEXT_STRONG));
    ui.label(
        RichText::new("A copy kept inside the video: changing it never changes the scene file it came from.")
            .small()
            .color(style::MUTED),
    );
    egui::Grid::new("video kept scene").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        let label = ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0)).labelled_by(label.id);
        ui.end_row();
        let label = ui.label("Colour");
        let swatch = style::swatch_button(ui, Some(colour), "Scene colour").labelled_by(label.id);
        egui::Popup::menu(&swatch).show(|ui| {
            if let Some(picked) = style::track_picker(ui, Some(colour)) {
                colour = picked;
            }
        });
        ui.end_row();
        ui.label("Length");
        ui.label(RichText::new(clock(length)).color(style::MUTED));
        ui.end_row();
        ui.label("Clips");
        ui.label(RichText::new(clips.to_string()).color(style::MUTED));
        ui.end_row();
    });
    if clips == 0 {
        ui.label(RichText::new("Not in the video yet: left out when saved.").small().color(style::MUTED));
    }
    ui.add_space(8.0);
    let mut remove = false;
    ui.horizontal(|ui| {
        if ui.button("Add to the end").clicked() {
            scenes::add_to_end(studio, id);
        }
        remove = ui.button("Remove from the video").clicked();
    });
    let changed = studio.video.video.scenes.get(&id).is_some_and(|k| k.name != name || k.colour != colour);
    if changed {
        studio.video.change(merge, |v| {
            if let Some(k) = v.scenes.get_mut(&id) {
                k.name = name;
                k.colour = colour;
            }
        });
    }
    if remove {
        studio.video.change(false, |v| arrange::remove_scene(v, id));
    }
}
