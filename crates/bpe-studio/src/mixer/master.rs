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
    let r = Knob::new(&mut v, lo..=hi, default, label).format(format).bipolar(bipolar).log(log).show(ui);
    let mut keyed = false;
    if r.changed {
        crate::inspector::set_or_key(scene, reg, &a, Value::Number(v), time, &mut keyed);
    }
    // The Master's keys take effect when every sound setting does (part 2); until then a key
    // that changes nothing isn't offered.
    let mark = Some(KeyMark { can_key: false, keyed: scene.keyed(&a), pulsed: false });
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

/// The Master strip.
pub fn strip(ui: &mut Ui, scene: &mut Scene, kit: &Kit, meters: Option<&MeterReading>, loudness: &[f32], time: f64) {
    let reading = meters.copied().unwrap_or(MeterReading::SILENT);
    ui.vertical(|ui| {
        ui.label(RichText::new("Master").font(style::semibold(14.0)).color(style::TEXT));
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            card_knob(ui, scene, kit, "master_input", "Input", &db, "dB", true, false, time);
            card_knob(ui, scene, kit, "master_output", "Output", &db, "dB", true, false, time);
            card_knob(ui, scene, kit, "master_ceiling", "Ceiling", &dbtp, "dBTP", false, false, time);
            card_knob(ui, scene, kit, "master_release", "Release", &ms, "ms", false, true, time);
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
