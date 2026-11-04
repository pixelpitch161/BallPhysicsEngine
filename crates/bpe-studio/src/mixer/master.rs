//! The Master strip: the last stop before the speakers and the export. Input and Output gain,
//! the true-peak Ceiling and its Release, the level meter, the loudness readouts and the
//! loudness graph with the -14 LUFS mark the apps aim for.

use crate::style::{self, Knob};
use crate::widgets::{self, KeyMark, TargetChoices, ValueMenu};
use bpe_core::{Address, Scene, SlotId, ThingId, Value};
use bpe_kit::Kit;
use bpe_sound::MeterReading;
use eframe::egui::{RichText, Ui};

/// The loudness the apps play clips at.
pub const TARGET_LUFS: f32 = -14.0;

fn address(key: &str) -> Address {
    Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key: key.into() }
}

fn db(v: f64) -> String {
    format!("{v:+.1} dB")
}

fn dbtp(v: f64) -> String {
    format!("{v:.1} dBTP")
}

fn ms(v: f64) -> String {
    format!("{v:.0} ms")
}

/// One knob on a mixer card setting, through the usual edit path (a key at the playhead when it
/// has keys) with the usual right-click menu.
#[allow(clippy::too_many_arguments)]
fn card_knob(
    ui: &mut Ui,
    scene: &mut Scene,
    kit: &Kit,
    key: &str,
    label: &str,
    format: &dyn Fn(f64) -> String,
    unit: &str,
    bipolar: bool,
    log: bool,
    time: f64,
    width: f32,
) {
    let reg = &kit.registry;
    let a = address(key);
    let Some(def) = scene.def_at(reg, &a) else { return };
    let (lo, hi) = match def.range {
        Some((lo, hi)) => (lo, hi),
        None => (0.0, 1.0),
    };
    let default = super::number(&def.default).unwrap_or(0.0);
    let now = scene.value_at_time(reg, &a, time).unwrap_or_else(|| def.default.clone());
    let mut v = super::number(&now).unwrap_or(default);
    let r = Knob::new(&mut v, lo..=hi, default, label).format(format).bipolar(bipolar).log(log).width(width).show(ui);
    let mut keyed = false;
    if r.changed {
        crate::inspector::set_or_key(scene, reg, &a, Value::Number(v), time, &mut keyed);
    }
    // Input and Output take keys (read every block); Ceiling and Release don't.
    let mark = Some(KeyMark { can_key: def.flags.keyframe, keyed: scene.keyed(&a), pulsed: false });
    let choices = TargetChoices::default();
    match widgets::value_menu(&r.response, def, unit, &now, mark, &choices) {
        ValueMenu::Nothing => {}
        ValueMenu::Paste(pasted) => {
            crate::inspector::set_or_key(scene, reg, &a, pasted, time, &mut keyed);
        }
        ValueMenu::CreateKey => {
            let _ = scene.set_key(reg, &a, time, now);
        }
    }
}

/// The Master's knobs, in the order the strips show them.
pub const KNOBS: [&str; 4] = ["master_input", "master_output", "master_ceiling", "master_release"];

/// The Master's knob for setting `key` (one of [`KNOBS`]), `width` wide.
pub fn knob(ui: &mut Ui, scene: &mut Scene, kit: &Kit, key: &str, time: f64, width: f32) {
    match key {
        "master_input" => card_knob(ui, scene, kit, key, "Input", &db, "dB", true, false, time, width),
        "master_output" => card_knob(ui, scene, kit, key, "Output", &db, "dB", true, false, time, width),
        "master_ceiling" => card_knob(ui, scene, kit, key, "Ceiling", &dbtp, "dBTP", false, false, time, width),
        "master_release" => card_knob(ui, scene, kit, key, "Release", &ms, "ms", false, true, time, width),
        _ => {}
    }
}

/// The loudness readouts and the loudness graph with its -14 LUFS mark.
pub fn loudness(ui: &mut Ui, reading: &MeterReading, history: &[f32]) {
    style::loudness_row(ui, reading);
    ui.add_space(4.0);
    ui.label(RichText::new("Loudness (short-term), -14 LUFS is where the apps play it").small().color(style::MUTED));
    style::loudness_graph(ui, history, TARGET_LUFS);
}

/// The Master strip.
pub fn strip(ui: &mut Ui, scene: &mut Scene, kit: &Kit, meters: Option<&MeterReading>, loudness: &[f32], time: f64) {
    let reading = meters.copied().unwrap_or(MeterReading::SILENT);
    ui.vertical(|ui| {
        ui.label(RichText::new("Master").font(style::semibold(14.0)).color(style::TEXT));
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            for key in KNOBS {
                knob(ui, scene, kit, key, time, 64.0);
            }
            ui.add_space(6.0);
            style::level_meter(ui, reading.peak_l, reading.peak_r, 72.0).on_hover_text("Peak level, left and right");
        });
        ui.add_space(6.0);
        style::loudness_row(ui, &reading);
        ui.add_space(4.0);
        ui.label(
            RichText::new("Loudness (short-term), -14 LUFS is where the apps play it").small().color(style::MUTED),
        );
        style::loudness_graph(ui, loudness, TARGET_LUFS);
    });
}
