//! "Pulse with music" in the setting menu, the wave icon beside settings that pulse, and the
//! small window for a pulse's settings.
//!
//! Like the keyframe diamonds, the menu and icons can't change the scene while the panels are
//! drawn: they leave requests that the app applies afterwards with [`apply_requests`].

use super::Target;
use crate::theme;
use bpe_music::AudioFeatures;
use bpe_scene::{AudioBand, AudioFrom, Curve, Levels, Modulator, Property, Scene, Value};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Ui};
use std::collections::HashMap;
use std::sync::Arc;

/// What the menu and wave icons show this frame.
#[derive(Clone, Default)]
struct Status {
    /// Each pulsing setting, with whether it is on.
    pulsing: HashMap<Property, bool>,
    /// The finished soundtrack's analysis, once it is ready.
    mix: Option<Arc<AudioFeatures>>,
}

/// Something asked for while the panels were drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Pulse the setting with this part of the music, or switch its pulse over to it.
    Pulse(Property, AudioBand),
    /// Stop pulsing.
    Stop(Property),
    /// Open the pulse's settings.
    Edit(Property),
}

fn status_id() -> egui::Id {
    egui::Id::unique("pulse status")
}

fn requests_id() -> egui::Id {
    egui::Id::unique("pulse requests")
}

fn window_id() -> egui::Id {
    egui::Id::unique("pulse window")
}

/// Tells this frame's menus and icons which settings pulse.
pub fn publish(ctx: &egui::Context, scene: &Scene, mix: Option<Arc<AudioFeatures>>) {
    let pulsing = scene.modulators.iter().map(|m| (m.property.clone(), m.enabled)).collect();
    ctx.data_mut(|d| d.insert_temp(status_id(), Arc::new(Status { pulsing, mix })));
}

/// The finished soundtrack's analysis published this frame, if it is ready.
pub fn published_mix(ctx: &egui::Context) -> Option<Arc<AudioFeatures>> {
    status(ctx).mix.clone()
}

/// How strongly the music pushes `modulator` at `time`, from 0 to about 1, following `mix`.
pub fn strength(modulator: &Modulator, mix: &AudioFeatures, time: f64) -> f32 {
    let listener = |t: f64| bpe_render::audio_levels(mix, t);
    let listener: &dyn Fn(f64) -> Levels = &listener;
    let audio = bpe_scene::Audio { song: Some(listener), mix: Some(listener) };
    modulator.strength(time.max(0.0), &audio).unwrap_or(0.0)
}

fn status(ctx: &egui::Context) -> Arc<Status> {
    ctx.data(|d| d.get_temp::<Arc<Status>>(status_id())).unwrap_or_default()
}

pub fn request(ctx: &egui::Context, request: Request) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Request>>(requests_id()).push(request));
}

pub fn take_requests(ctx: &egui::Context) -> Vec<Request> {
    ctx.data_mut(|d| d.remove_temp::<Vec<Request>>(requests_id())).unwrap_or_default()
}

/// Whether this setting can pulse with the music: every setting that can be animated can.
pub fn available(target: &Target) -> bool {
    target.property.is_some()
}

/// The submenu: which part of the music to follow, plus the pulse's settings once it has one.
pub fn menu(ui: &mut Ui, target: &Target) {
    let Some(property) = &target.property else { return };
    let current = status(ui.ctx()).pulsing.get(property).copied();
    ui.set_min_width(210.0);
    ui.label(RichText::new("Follow the music's").small().color(theme::MUTED));
    for band in AudioBand::ALL {
        if ui.button(band.name()).on_hover_text(band_help(band)).clicked() {
            request(ui.ctx(), Request::Pulse(property.clone(), band));
            ui.close();
        }
    }
    if current.is_some() {
        ui.separator();
        if ui.button("Pulse settings…").clicked() {
            request(ui.ctx(), Request::Edit(property.clone()));
            ui.close();
        }
        if ui.button("Stop pulsing").clicked() {
            request(ui.ctx(), Request::Stop(property.clone()));
            ui.close();
        }
    }
    if property.affects_simulation() {
        ui.separator();
        ui.label(
            RichText::new("This changes how balls move, so it follows the song only, never the bounce sounds.")
                .small()
                .color(theme::MUTED),
        );
    }
}

fn band_help(band: AudioBand) -> &'static str {
    match band {
        AudioBand::Bass => "Kick drums and bass lines",
        AudioBand::Mid => "Vocals, chords and most instruments",
        AudioBand::Treble => "Hi-hats, cymbals and sparkle",
        AudioBand::Level => "How loud the music is overall",
        AudioBand::Beat => "A kick on every beat that fades quickly",
        AudioBand::Onset => "A jump whenever a new sound starts",
    }
}

/// The wave icon beside a setting that pulses with the music. Clicking it opens the pulse's
/// settings. Draws nothing for settings that don't pulse.
pub fn wave_icon(ui: &mut Ui, property: &Property) {
    let Some(on) = status(ui.ctx()).pulsing.get(property).copied() else { return };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), Sense::click());
    let color = match (on, response.hovered()) {
        (_, true) => Color32::WHITE,
        (true, false) => theme::ACCENT_TEXT,
        (false, false) => theme::MUTED,
    };
    paint_wave(ui.painter(), rect.shrink2(egui::vec2(2.0, 4.0)), color);
    let help = if on {
        "This setting pulses with the music. Click to change how."
    } else {
        "This setting's pulse is turned off. Click to change it."
    };
    if response.on_hover_text(help).clicked() {
        request(ui.ctx(), Request::Edit(property.clone()));
    }
}

/// A little sine wave filling `rect`.
pub fn paint_wave(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let points: Vec<Pos2> = (0..=16)
        .map(|i| {
            let t = i as f32 / 16.0;
            let y = (t * std::f32::consts::TAU * 1.5).sin();
            Pos2::new(rect.left() + t * rect.width(), rect.center().y - y * rect.height() * 0.5)
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.6, color)));
}

/// Applies the requests made while the panels were drawn. `fallback` gives a setting's value when
/// the scene has none of its own (an effect slider left at its default), so a new pulse can be
/// sized to it.
pub fn apply_requests(
    ctx: &egui::Context,
    scene: &mut Scene,
    requests: Vec<Request>,
    fallback: impl Fn(&Property) -> Option<Value>,
) {
    for request in requests {
        match request {
            Request::Pulse(property, band) => {
                if let Some(m) = scene.modulators.iter_mut().find(|m| m.property == property) {
                    m.source = band;
                    m.enabled = true;
                } else {
                    let base = property.get(scene).or_else(|| fallback(&property));
                    let modulator = new_pulse(scene, property.clone(), band, base);
                    scene.modulators.push(modulator);
                }
                ctx.data_mut(|d| d.insert_temp(window_id(), Some(property)));
            }
            Request::Stop(property) => {
                scene.modulators.retain(|m| m.property != property);
                close_window(ctx, &property);
            }
            Request::Edit(property) => {
                ctx.data_mut(|d| d.insert_temp(window_id(), Some(property)));
            }
        }
    }
}

fn close_window(ctx: &egui::Context, property: &Property) {
    let open = ctx.data(|d| d.get_temp::<Option<Property>>(window_id())).flatten();
    if open.as_ref() == Some(property) {
        ctx.data_mut(|d| d.insert_temp::<Option<Property>>(window_id(), None));
    }
}

/// A new pulse on `property`, sized so it is clearly visible without wrecking the scene.
/// Effect amounts that are already full are turned down so the music has room to push them.
fn new_pulse(scene: &mut Scene, property: Property, band: AudioBand, base: Option<Value>) -> Modulator {
    let mut modulator = Modulator::new(property.clone(), band);
    match base {
        Some(Value::Color(color)) => {
            // Towards white for dark colours, towards black for light ones.
            let light = color.r as u32 + color.g as u32 + color.b as u32 > 382;
            modulator.color = if light { bpe_scene::Color::rgb(0, 0, 0) } else { bpe_scene::Color::WHITE };
            modulator.amount = 0.6;
        }
        Some(Value::Number(value)) => modulator.amount = pulse_amount(&property, value),
        None => {}
    }
    if let (Property::EffectAmount(_), Some(Value::Number(value))) = (&property, base)
        && value > 0.85
    {
        property.set(scene, Value::Number(0.2));
        modulator.amount = 0.8;
    }
    modulator
}

/// How far a new pulse moves a number setting whose value is `value`.
pub fn pulse_amount(property: &Property, value: f32) -> f32 {
    let value = if value.is_finite() { value } else { 0.0 };
    match property {
        // Bounciness above 1 makes balls gain speed on every bounce, so stay gentle.
        Property::Bounciness => 0.08,
        Property::ArenaSize => 0.1,
        Property::EffectAmount(_) => (1.0 - value).clamp(0.2, 1.0),
        _ if value.abs() > 1e-3 => value.abs() * 0.5,
        Property::Gravity => 600.0,
        Property::SidewaysGravity => 300.0,
        Property::ArenaSpin => 45.0,
        Property::ShrinkSpeed => 10.0,
        Property::TrailWidth | Property::StringWidth | Property::FlashSize => 4.0,
        _ => 0.5,
    }
}

/// The open pulse window, if any. Edits the scene's pulse in place.
pub fn window(ctx: &egui::Context, scene: &mut Scene, mix: Option<&AudioFeatures>, time: f64) {
    let Some(property) = ctx.data(|d| d.get_temp::<Option<Property>>(window_id())).flatten() else { return };
    let Some(index) = scene.modulators.iter().position(|m| m.property == property) else {
        ctx.data_mut(|d| d.insert_temp::<Option<Property>>(window_id(), None));
        return;
    };
    let name = crate::keys::name(&property, scene);
    let mut open = true;
    let mut stop = false;
    egui::Window::new(format!("{name} pulses with the music"))
        .id(egui::Id::unique("pulse window").with(&property))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(320.0)
        .show(ctx, |ui| {
            let modulator = &mut scene.modulators[index];
            stop = settings(ui, modulator, mix, time);
        });
    if !open || stop {
        if stop {
            scene.modulators.remove(index);
        }
        ctx.data_mut(|d| d.insert_temp::<Option<Property>>(window_id(), None));
    }
}

/// The pulse's settings. Returns true when "Stop pulsing" was clicked.
fn settings(ui: &mut Ui, m: &mut Modulator, mix: Option<&AudioFeatures>, time: f64) -> bool {
    let mut stop = false;
    egui::Grid::new("pulse settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label("On");
        ui.checkbox(&mut m.enabled, "").on_hover_text("Turn the pulse off without losing its settings");
        ui.end_row();

        ui.label("Follow");
        egui::ComboBox::from_id_salt("band").selected_text(m.source.name()).show_ui(ui, |ui| {
            for band in AudioBand::ALL {
                ui.selectable_value(&mut m.source, band, band.name()).on_hover_text(band_help(band));
            }
        });
        ui.end_row();

        if m.property.is_color() {
            ui.label("Towards");
            let mut rgb = [m.color.r, m.color.g, m.color.b];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                m.color = bpe_scene::Color::rgb(rgb[0], rgb[1], rgb[2]);
            }
            ui.end_row();
            ui.label("How far");
            ui.add(egui::Slider::new(&mut m.amount, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)))
                .on_hover_text("How close to that colour the setting gets when the music is loudest");
        } else {
            ui.label("How much");
            let speed = (m.amount.abs() * 0.01).max(0.001);
            ui.add(egui::DragValue::new(&mut m.amount).speed(speed).max_decimals(3)).on_hover_text(
                "How far the setting moves at the music's loudest, in the setting's own units. Below 0 pulls it down instead.",
            );
        }
        ui.end_row();

        ui.label("Smoothing");
        ui.add(
            egui::Slider::new(&mut m.smoothing, 0.0..=1.0)
                .custom_formatter(|v, _| if v < 0.005 { "none".into() } else { format!("{v:.2} s") }),
        )
        .on_hover_text("Higher follows the music more gently; none follows every hit exactly");
        ui.end_row();

        ui.label("Response");
        egui::ComboBox::from_id_salt("curve").selected_text(m.curve.name()).show_ui(ui, |ui| {
            for curve in Curve::ALL {
                ui.selectable_value(&mut m.curve, curve, curve.name()).on_hover_text(curve_help(curve));
            }
        });
        ui.end_row();

        ui.label("Listen to");
        if m.property.affects_simulation() {
            ui.label(RichText::new("The song only").color(theme::MUTED)).on_hover_text(
                "This setting changes how balls move, so it never follows the bounce sounds the balls make.",
            );
        } else {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut m.from, AudioFrom::Mix, "Everything")
                    .on_hover_text("The finished soundtrack: the song plus the bounce sounds");
                ui.selectable_value(&mut m.from, AudioFrom::Song, "The song")
                    .on_hover_text("Only the loaded song, without the bounce sounds");
            });
        }
        ui.end_row();
    });
    if !m.smoothing.is_finite() {
        m.smoothing = 0.0;
    }
    if !m.amount.is_finite() {
        m.amount = 0.0;
    }
    ui.add_space(6.0);
    meter(ui, m, mix, time);
    ui.add_space(6.0);
    if ui.button("Stop pulsing").clicked() {
        stop = true;
    }
    stop
}

fn curve_help(curve: Curve) -> &'static str {
    match curve {
        Curve::Linear => "Moves in step with the music",
        Curve::Soft => "Reacts to quiet sounds too",
        Curve::Sharp => "Only reacts to loud hits",
        Curve::Gate => "Either off or fully on",
    }
}

/// A bar showing how strongly the music pushes the setting right now, and the last two seconds.
fn meter(ui: &mut Ui, m: &Modulator, mix: Option<&AudioFeatures>, time: f64) {
    let Some(mix) = mix else {
        ui.label(
            RichText::new("Once the soundtrack is ready, the wave below shows the pulse.").small().color(theme::MUTED),
        );
        return;
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, theme::CARD);
    let history = 2.0;
    let points: Vec<Pos2> = (0..=80)
        .map(|i| {
            let t = time - history + history * i as f64 / 80.0;
            let s = if t < 0.0 { 0.0 } else { strength(m, mix, t).clamp(0.0, 1.2) };
            Pos2::new(rect.left() + rect.width() * i as f32 / 80.0, rect.bottom() - 3.0 - s / 1.2 * (rect.height() - 6.0))
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5, theme::ACCENT_TEXT)));
    ui.label(RichText::new("How hard the music pushes, over the last two seconds").small().color(theme::MUTED));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pulses_are_sized_to_the_setting() {
        let mut scene = Scene::default();
        let gravity = Property::Gravity.get(&scene);
        let m = new_pulse(&mut scene, Property::Gravity, AudioBand::Bass, gravity);
        let g = gravity.unwrap().number().unwrap();
        assert!((m.amount - g.abs() * 0.5).abs() < 1e-3 || g == 0.0);
        assert_eq!(m.from, AudioFrom::Song);
        let m = new_pulse(&mut scene, Property::Bounciness, AudioBand::Treble, Some(Value::Number(0.9)));
        assert!(m.amount <= 0.1);
        let m = new_pulse(&mut scene, Property::Bloom, AudioBand::Level, Some(Value::Number(0.0)));
        assert!(m.amount > 0.0);
        assert_eq!(m.from, AudioFrom::Mix);
    }

    #[test]
    fn full_effects_are_turned_down_to_make_room() {
        let mut scene = Scene::default();
        scene.effects.push(bpe_scene::Effect::new("shockwave"));
        let base = Property::EffectAmount(0).get(&scene);
        let m = new_pulse(&mut scene, Property::EffectAmount(0), AudioBand::Beat, base);
        assert!(scene.effects[0].amount < 0.5);
        assert!(m.amount > 0.5);
    }

    #[test]
    fn colours_pulse_towards_a_contrasting_colour() {
        let mut scene = Scene::default();
        let dark = Some(Value::Color(bpe_scene::Color::rgb(0, 0, 0)));
        let m = new_pulse(&mut scene, Property::BackgroundTop, AudioBand::Bass, dark);
        assert_eq!(m.color, bpe_scene::Color::WHITE);
    }

    #[test]
    fn requests_add_switch_and_remove_pulses() {
        let ctx = egui::Context::default();
        let mut scene = Scene::default();
        let none = |_: &Property| None;
        apply_requests(&ctx, &mut scene, vec![Request::Pulse(Property::Bloom, AudioBand::Bass)], none);
        assert_eq!(scene.modulators.len(), 1);
        apply_requests(&ctx, &mut scene, vec![Request::Pulse(Property::Bloom, AudioBand::Beat)], none);
        assert_eq!(scene.modulators.len(), 1);
        assert_eq!(scene.modulators[0].source, AudioBand::Beat);
        apply_requests(&ctx, &mut scene, vec![Request::Stop(Property::Bloom)], none);
        assert!(scene.modulators.is_empty());
    }

    #[test]
    fn every_setting_can_pulse() {
        let target = Target { property: Some(Property::Exposure), label: String::new() };
        assert!(available(&target));
        assert!(!available(&Target::default()));
        for property in Property::PLAIN {
            assert!(pulse_amount(&property, 0.0).is_finite());
        }
    }
}
