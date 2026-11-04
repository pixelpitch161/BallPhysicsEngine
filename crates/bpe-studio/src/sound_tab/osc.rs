//! An oscillator's panel: its on switch and Warp mode, the wave chips, the waveform view, the
//! knobs, Phase's Restart / Free, the sub, and the box its layers go in.

use super::{TabCx, db, panel, wave_view};
use crate::style;
use bpe_kit::sound::{NoiseColour, PhaseMode, SubOctave, SubWave, WarpMode, Wave};
use eframe::egui::{self, RichText, Ui};

/// One of the two oscillators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Osc {
    A,
    B,
}

impl Osc {
    /// Its settings' key prefix.
    pub fn prefix(self) -> &'static str {
        match self {
            Osc::A => "a_",
            Osc::B => "b_",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Osc::A => "Osc A",
            Osc::B => "Osc B",
        }
    }

    /// Its chain's lane (`Layer.lane`).
    pub fn lane(self) -> u8 {
        match self {
            Osc::A => 0,
            Osc::B => 1,
        }
    }
}

fn plain(v: f64) -> String {
    format!("{v:.0}")
}

fn signed(v: f64) -> String {
    if v.round() == 0.0 { "0".into() } else { format!("{:+.0}", v.round()) }
}

fn percent(v: f64) -> String {
    format!("{v:.0} %")
}

fn semis(v: f64) -> String {
    format!("{} st", signed(v))
}

fn cents(v: f64) -> String {
    format!("{} ct", signed(v))
}

fn degrees(v: f64) -> String {
    format!("{v:.0}°")
}

/// Pan, -100..100: L, C or R.
pub fn pan(v: f64) -> String {
    let x = v.round();
    if x == 0.0 {
        "C".into()
    } else if x < 0.0 {
        format!("L{:.0}", -x)
    } else {
        format!("R{x:.0}")
    }
}

/// Draws `which` oscillator's panel.
pub fn show(ui: &mut Ui, cx: &mut TabCx, which: Osc, tab: &mut super::SoundTab) {
    let p = which.prefix();
    let key = |k: &str| format!("{p}{k}");
    panel(ui, |ui| {
        let on = cx.flag(&key("on"));
        // The header: on, the name and Warp mode.
        ui.horizontal(|ui| {
            cx.switch(ui, &key("on"), &format!("{} on", which.name()));
            let name = RichText::new(which.name()).font(style::semibold(13.0));
            ui.label(if on { name } else { name.color(style::MUTED) });
            let wave: Wave = cx.choice(&key("wave"));
            if wave != Wave::Noise {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    warp_mode_menu(ui, cx, which);
                });
            }
        });
        if !on {
            ui.multiply_opacity(0.45);
        }
        let wave: Wave = cx.choice(&key("wave"));
        cx.chips(
            ui,
            &key("wave"),
            &[
                (Wave::Sine, "Sine"),
                (Wave::Triangle, "Tri"),
                (Wave::Saw, "Saw"),
                (Wave::Square, "Square"),
                (Wave::Noise, "Noise"),
            ],
        );
        if wave == Wave::Noise {
            cx.chips(ui, &key("noise"), &[(NoiseColour::White, "White"), (NoiseColour::Pink, "Pink")]);
        }
        let mode: WarpMode = cx.choice(&key("warp_mode"));
        let warp = (cx.number(&key("warp")) / 100.0) as f32;
        let id = egui::Id::unique(("wave view", cx.id, p));
        wave_view::show(ui, id, &format!("{} wave", which.name()), wave, mode, warp);
        let tonal = wave != Wave::Noise;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            cx.knob(ui, &key("gain"), "Gain", &db);
            cx.knob(ui, &key("level"), "Level", &db);
            if tonal {
                cx.knob(ui, &key("warp"), "Warp", &percent);
            }
            cx.knob(ui, &key("octave"), "Octave", &signed);
            cx.knob(ui, &key("semi"), "Semi", &semis);
            cx.knob(ui, &key("cents"), "Cents", &cents);
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if tonal {
                cx.knob(ui, &key("unison"), "Unison", &plain);
                cx.knob(ui, &key("detune"), "Detune", &cents);
                cx.knob(ui, &key("spread"), "Spread", &percent);
                cx.knob(ui, &key("blend"), "Blend", &percent);
                cx.knob(ui, &key("random_phase"), "Rand ph.", &percent);
            }
            cx.knob(ui, &key("pan"), "Pan", &pan);
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            cx.knob(ui, &key("phase"), "Phase", &degrees);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.add_space(14.0);
                cx.chips(ui, &key("phase_mode"), &[(PhaseMode::Restart, "Restart"), (PhaseMode::Free, "Free")]);
            });
            if tonal {
                ui.add_space(10.0);
                sub(ui, cx, which);
            }
        });
        super::chain_box::show(ui, cx, which.lane(), tab);
    });
}

/// The Warp mode menu.
fn warp_mode_menu(ui: &mut Ui, cx: &mut TabCx, which: Osc) {
    let key = format!("{}warp_mode", which.prefix());
    let now: WarpMode = cx.choice(&key);
    let mut picked = None;
    let menu = ui.menu_button(now.label(), |ui| {
        ui.set_min_width(120.0);
        for mode in WarpMode::ALL {
            if ui.selectable_label(mode == now, mode.label()).clicked() {
                picked = Some(mode);
                ui.close();
            }
        }
    });
    super::name_it(ui, &menu.response, &format!("{} warp mode", which.name()));
    menu.response.on_hover_text("How Warp bends the wave");
    if let Some(mode) = picked {
        cx.set(&key, bpe_core::Value::Choice(bpe_core::Choice::name(mode).into()));
    }
}

/// The sub: Off, Sine or Square, one or two octaves down, and its level.
fn sub(ui: &mut Ui, cx: &mut TabCx, which: Osc) {
    let p = which.prefix();
    ui.vertical(|ui| {
        ui.label(RichText::new("Sub").small().color(style::MUTED));
        cx.chips(
            ui,
            &format!("{p}sub"),
            &[(SubWave::Off, "Off"), (SubWave::Sine, "Sine"), (SubWave::Square, "Square")],
        );
        let off = cx.choice::<SubWave>(&format!("{p}sub")) == SubWave::Off;
        if !off {
            cx.chips(ui, &format!("{p}sub_octave"), &[(SubOctave::One, "-1 oct"), (SubOctave::Two, "-2 oct")]);
        }
    });
    if cx.choice::<SubWave>(&format!("{p}sub")) != SubWave::Off {
        cx.knob(ui, &format!("{p}sub_level"), "Sub level", &db);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{SHAPE, sounding};
    use super::*;
    use bpe_core::{Address, SlotId, Value};
    use eframe::egui::{Modifiers, PointerButton, accesskit::Role};
    use egui_kittest::kittest::{NodeT, Queryable};

    fn drag_up(h: &mut egui_kittest::Harness<'_, (crate::app::Studio, f32)>, at: egui::Pos2, dy: f32) {
        h.hover_at(at);
        h.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        for i in 1..=5 {
            h.event(egui::Event::PointerMoved(at - egui::vec2(0.0, dy * i as f32 / 5.0)));
            h.step();
        }
        let end = at - egui::vec2(0.0, dy);
        h.event(egui::Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    fn drawn(h: &egui_kittest::Harness<'_, (crate::app::Studio, f32)>) -> Vec<f32> {
        let id = wave_view::drawn_id(egui::Id::unique(("wave view", SHAPE, "a_")));
        h.ctx.data(|d| d.get_temp::<Vec<f32>>(id)).expect("Osc A's wave was drawn")
    }

    #[test]
    fn turning_warp_redraws_the_wave() {
        let mut h = sounding();
        let before = drawn(&h);
        let warp = h.get_all_by_role_and_label(Role::Slider, "Warp").next().unwrap().rect().center();
        drag_up(&mut h, warp, 60.0);
        let a = Address::on(SHAPE, SlotId::Sound, "a_warp");
        let reg = bpe_kit::registry();
        let now = h.state().0.scene.value_at_time(&reg, &a, 0.0);
        assert!(matches!(now, Some(Value::Number(v)) if v > 10.0), "Warp turned up: {now:?}");
        assert_ne!(drawn(&h), before, "the wave view follows Warp");
    }

    #[test]
    fn noise_shows_white_and_pink() {
        let mut h = sounding();
        assert!(h.query_by_label("Pink").is_none());
        h.get_all_by_label("Noise").next().unwrap().click();
        h.run();
        assert!(h.query_by_label("White").is_some());
        assert!(h.query_by_label("Pink").is_some());
        assert!(h.query_by_label("Osc A warp mode").is_none(), "noise has no Warp");
    }

    #[test]
    fn every_osc_knob_is_named() {
        let h = sounding();
        for name in [
            "Gain", "Level", "Warp", "Octave", "Semi", "Cents", "Unison", "Detune", "Spread", "Blend", "Rand ph.",
            "Pan", "Phase",
        ] {
            let n = h.get_all_by_role_and_label(Role::Slider, name).count();
            assert!(n >= 2, "{name}: {n} knobs (one on each oscillator)");
        }
    }

    #[test]
    fn right_click_a_knob_creates_a_keyframe() {
        let mut h = sounding();
        h.state_mut().0.time = 0.5;
        h.run();
        h.get_all_by_role_and_label(Role::Slider, "Cents").next().unwrap().click_secondary();
        h.run();
        h.get_by_label("Create Keyframe").click();
        h.run();
        let a = Address::on(SHAPE, SlotId::Sound, "a_cents");
        assert!(h.state().0.scene.keyed(&a), "a key at the playhead");
    }

    #[test]
    fn osc_b_starts_off_and_its_knobs_are_dimmed() {
        let h = sounding();
        let b = h.get_by_role_and_label(Role::CheckBox, "Osc B on");
        assert_eq!(b.accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
        let a = h.get_by_role_and_label(Role::CheckBox, "Osc A on");
        assert_eq!(a.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
        // Osc B's knobs are there to set up before it is turned on.
        assert_eq!(h.get_all_by_role_and_label(Role::Slider, "Unison").count(), 2);
    }
}
