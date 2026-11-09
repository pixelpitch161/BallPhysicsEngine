//! A Sample oscillator's panel (W11): the sample's name and Load…, a note when it can't be found
//! or read, its waveform with Start and the loop marked, the knobs (Gain, Level, Octave, Semi,
//! Cents, Pan; Root, Start, the loop's ends and fade), the Loop chips and the Follow notes and
//! Reverse switches.

use std::sync::Arc;

use super::osc::{Osc, pan};
use super::{TabCx, TabEdit, db, name_it};
use crate::style;
use bpe_core::Value;
use bpe_kit::sound::SampleLoop;
use bpe_sound::SampleBank;
use bpe_sound::audio::{EXTENSIONS, NOT_A_SOUND};
use eframe::egui::{self, RichText, Sense, Stroke, Ui, Vec2};

/// The waveform's height.
const HEIGHT: f32 = 64.0;

/// What the note under the name says when the sample isn't kept in the scene.
pub const NOT_FOUND: &str = "Sample not found: load it again with Load…";

/// A sound file chosen with Load…, read but not yet kept.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedFile {
    /// The file's name without its folder (and the name to keep it under, without the extension).
    pub file: String,
    pub bytes: Arc<[u8]>,
}

impl LoadedFile {
    /// The name to keep it under: the file name without its extension.
    pub fn name(&self) -> String {
        std::path::Path::new(&self.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// Reads `path` for keeping, or says in plain words why it can't be.
pub fn read_sound_file(path: &std::path::Path) -> Result<LoadedFile, String> {
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes: Arc<[u8]> = std::fs::read(path).map_err(|e| format!("Couldn't read {file}: {e}"))?.into();
    bpe_sound::audio::probe(&bytes).map_err(|_| format!("{file} {NOT_A_SOUND}"))?;
    Ok(LoadedFile { file, bytes })
}

/// A file picker for sound files.
pub fn pick_sound_file(title: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new().set_title(title).add_filter("Sound files", &EXTENSIONS).pick_file()
}

/// What `cx` keeps under the text setting `key`.
fn text(cx: &TabCx, key: &str) -> String {
    match cx.get(key) {
        Value::Text(t) => t,
        _ => String::new(),
    }
}

fn percent(v: f64) -> String {
    format!("{v:.1} %")
}

fn ms(v: f64) -> String {
    format!("{v:.0} ms")
}

fn signed(v: f64) -> String {
    if v.round() == 0.0 { "0".into() } else { format!("{:+.0}", v.round()) }
}

fn note(v: f64) -> String {
    crate::sound_tab::keys::note_name(v.round().clamp(0.0, 127.0) as u8)
}

/// Draws the panel's sample part (the oscillator's header and chips are already drawn).
pub fn show(ui: &mut Ui, cx: &mut TabCx, which: Osc, tab: &mut super::SoundTab) {
    let p = which.prefix();
    let key = |k: &str| format!("{p}{k}");
    let name = text(cx, &key("sample"));
    let kept = cx.scene.audio.get(&name).map(|a| a.bytes.clone());
    ui.horizontal(|ui| {
        let shown = if name.is_empty() { "No sample loaded".to_owned() } else { name.clone() };
        let colour = if name.is_empty() { style::MUTED } else { style::TEXT };
        ui.label(RichText::new(shown).color(colour));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let load = ui.button("Load…").on_hover_text(
                "Choose a sound file (WAV, FLAC, MP3, OGG or AIFF). The scene keeps a copy, so moving the file later \
                 doesn't lose it",
            );
            name_it(ui, &load, &format!("Load {} sample", which.name()));
            if load.clicked()
                && let Some(path) = pick_sound_file("Load a sample")
            {
                match read_sound_file(&path) {
                    Ok(file) => {
                        cx.edits.push(TabEdit::OwnStep);
                        cx.edits.push(TabEdit::LoadSample { key: key("sample"), file });
                    }
                    Err(text) => cx.edits.push(TabEdit::Note { text, problem: true }),
                }
            }
        });
    });
    // What can be said about it: missing, unreadable or still being read.
    let data = kept.as_ref().map(SampleBank::ready);
    let problem = match (&kept, &data) {
        _ if name.is_empty() => None,
        (None, _) => Some(NOT_FOUND.to_owned()),
        (Some(_), Some(Some(Err(e)))) => Some(e.clone()),
        _ => None,
    };
    if let Some(problem) = problem {
        ui.label(RichText::new(problem).small().color(style::PROBLEM));
    } else if matches!(data, Some(None)) {
        ui.label(RichText::new("Reading the sound file…").small().color(style::MUTED));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    let sample = match data {
        Some(Some(Ok(d))) => Some(d),
        _ => None,
    };
    let loop_mode: SampleLoop = cx.choice(&key("sample_loop"));
    let marks = Marks {
        start: cx.number(&key("sample_start")) as f32 / 100.0,
        looped: (loop_mode != SampleLoop::Off).then(|| {
            (cx.number(&key("sample_loop_start")) as f32 / 100.0, cx.number(&key("sample_loop_end")) as f32 / 100.0)
        }),
        reverse: cx.flag(&key("sample_reverse")),
        colour: cx.colour,
    };
    waveform(ui, &format!("{} sample", which.name()), sample.as_deref(), &marks);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        cx.knob(ui, &key("gain"), "Gain", &db);
        cx.knob(ui, &key("level"), "Level", &db);
        cx.knob(ui, &key("octave"), "Octave", &signed);
        cx.knob(ui, &key("semi"), "Semi", &|v| format!("{} st", signed(v)));
        cx.knob(ui, &key("cents"), "Cents", &|v| format!("{} ct", signed(v)));
        cx.knob(ui, &key("pan"), "Pan", &pan);
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        cx.knob(ui, &key("sample_root"), "Root", &note).on_hover_text("The note the sample sounds at as recorded");
        cx.knob(ui, &key("sample_start"), "Start", &percent);
        if loop_mode != SampleLoop::Off {
            cx.knob(ui, &key("sample_loop_start"), "Loop start", &percent);
            cx.knob(ui, &key("sample_loop_end"), "Loop end", &percent);
        }
        if loop_mode == SampleLoop::Forward {
            cx.knob(ui, &key("sample_loop_fade"), "Loop fade", &ms);
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Loop").small().color(style::MUTED));
        cx.chips(
            ui,
            &key("sample_loop"),
            &[(SampleLoop::Off, "Off"), (SampleLoop::Forward, "Forward"), (SampleLoop::PingPong, "Ping-pong")],
        );
    });
    ui.horizontal(|ui| {
        let r = cx.switch(ui, &key("sample_follow"), &format!("{} follow notes", which.name()));
        r.on_hover_text("Each note plays the sample at its own pitch; off, every note plays it as recorded (drums)");
        ui.label(RichText::new("Follow notes").small().color(style::MUTED));
        ui.add_space(12.0);
        cx.switch(ui, &key("sample_reverse"), &format!("{} reverse", which.name()));
        ui.label(RichText::new("Reverse").small().color(style::MUTED));
    });
    let _ = tab;
}

/// Where Start and the loop sit on the waveform (shares of its length, on the sound as it plays).
struct Marks {
    start: f32,
    looped: Option<(f32, f32)>,
    reverse: bool,
    colour: egui::Color32,
}

/// The sample's waveform, the whole width of `ui`, with Start (a line) and the loop (a shaded
/// span) marked. Empty while there is no sound to draw.
fn waveform(ui: &mut Ui, name: &str, data: Option<&bpe_dsp::SampleData>, marks: &Marks) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::hover());
    name_it(ui, &response, name);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, style::LINE_SOFT), egui::StrokeKind::Inside);
    let mid = rect.center().y;
    painter.line_segment([egui::pos2(rect.left(), mid), egui::pos2(rect.right(), mid)], Stroke::new(1.0, style::LINE));
    let Some(data) = data else { return response };
    let x_at = |share: f32| rect.left() + share.clamp(0.0, 1.0) * rect.width();
    if let Some((a, b)) = marks.looped
        && b > a
    {
        let span = egui::Rect::from_x_y_ranges(x_at(a)..=x_at(b), rect.y_range());
        painter.rect_filled(span, 0.0, marks.colour.gamma_multiply(0.18));
    }
    // One column of peaks per pixel, the sound as it plays (reversed when Reverse is on).
    let peaks = data.peaks();
    let columns = rect.width().max(1.0) as usize;
    let half = rect.height() * 0.45;
    for c in 0..columns {
        let (mut from, mut to) =
            (c * peaks.len() / columns, ((c + 1) * peaks.len() / columns).max(c * peaks.len() / columns + 1));
        if marks.reverse {
            (from, to) = (peaks.len().saturating_sub(to), peaks.len().saturating_sub(from));
        }
        let peak = peaks.get(from..to.min(peaks.len())).map_or(0.0, |s| s.iter().fold(0.0f32, |m, v| m.max(*v)));
        let x = rect.left() + c as f32 + 0.5;
        let h = (peak.min(1.0) * half).max(0.5);
        painter.line_segment([egui::pos2(x, mid - h), egui::pos2(x, mid + h)], Stroke::new(1.0, style::MUTED));
    }
    let x = x_at(marks.start);
    painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], Stroke::new(1.5, marks.colour));
    response
}
