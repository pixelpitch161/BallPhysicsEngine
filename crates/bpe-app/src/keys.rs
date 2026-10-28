//! Keyframes in the editor: the diamond button beside each setting that can be animated, and
//! keeping keyframes in step with edits.
//!
//! While a scene has animated settings, the panels edit a copy of it with those settings at
//! their values at the playhead, so sliders show what is on screen. [`write_back`] then turns a
//! change to an animated setting into a keyframe at the playhead and puts the scene's own plain
//! value back.

use crate::inspector::Tab;
use crate::theme;
use bpe_scene::{Property, SAME_TIME, Scene, Track, Value};
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Ui};
use std::collections::HashMap;
use std::sync::Arc;

/// What the diamond buttons show this frame: the playhead time and, for each animated setting,
/// whether it has a keyframe at the playhead.
#[derive(Clone, Default)]
struct Status {
    time: f32,
    animated: HashMap<Property, bool>,
}

/// Something a diamond button asked for, done after the panels are drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Add a keyframe at the playhead, or remove the one there.
    Toggle(Property),
    /// Stop animating: keep the value at the playhead and drop every keyframe.
    Stop(Property),
}

fn status_id() -> egui::Id {
    egui::Id::unique("keyframe status")
}

fn requests_id() -> egui::Id {
    egui::Id::unique("keyframe requests")
}

fn next_id() -> egui::Id {
    egui::Id::unique("keyframe next row")
}

/// Tells this frame's diamond buttons where the playhead is and which settings are animated.
pub fn publish(ctx: &egui::Context, scene: &Scene, time: f32) {
    let animated =
        scene.animation.iter().map(|t| (t.property.clone(), t.key_near(time, SAME_TIME).is_some())).collect();
    ctx.data_mut(|d| {
        d.insert_temp(status_id(), Arc::new(Status { time, animated }));
        d.remove::<Option<Property>>(next_id());
    });
}

/// Puts a keyframe diamond on the next setting row drawn in `ui`'s panel.
pub fn mark_next(ui: &Ui, property: Property) {
    ui.data_mut(|d| d.insert_temp(next_id(), Some(property)));
}

/// The setting the next row should get a diamond for, if any. Called by the row's label.
pub fn take_next(ui: &Ui) -> Option<Property> {
    ui.data_mut(|d| d.remove_temp::<Option<Property>>(next_id())).flatten()
}

pub fn take_requests(ctx: &egui::Context) -> Vec<Request> {
    ctx.data_mut(|d| d.remove_temp::<Vec<Request>>(requests_id())).unwrap_or_default()
}

pub fn request(ctx: &egui::Context, request: Request) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Request>>(requests_id()).push(request));
}

/// Whether a setting is not animated, animated, or has a keyframe at the playhead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyState {
    Still,
    Animated,
    KeyHere,
}

/// Draws a keyframe diamond centred on `center`.
pub fn paint_diamond(painter: &egui::Painter, center: Pos2, size: f32, state: KeyState, color: Color32) {
    let points = vec![
        center + egui::vec2(0.0, -size),
        center + egui::vec2(size, 0.0),
        center + egui::vec2(0.0, size),
        center + egui::vec2(-size, 0.0),
    ];
    let fill = if state == KeyState::KeyHere { color } else { Color32::TRANSPARENT };
    painter.add(egui::Shape::convex_polygon(points, fill, Stroke::new(1.3, color)));
}

/// The keyframe button beside a setting's name.
pub fn diamond(ui: &mut Ui, property: Property) {
    let status = ui.data(|d| d.get_temp::<Arc<Status>>(status_id())).unwrap_or_default();
    let state = match status.animated.get(&property) {
        None => KeyState::Still,
        Some(false) => KeyState::Animated,
        Some(true) => KeyState::KeyHere,
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
    let color = match (state, response.hovered()) {
        (KeyState::Still, false) => theme::MUTED.gamma_multiply(0.55),
        (KeyState::Still, true) => theme::TEXT,
        (_, false) => theme::ACCENT_TEXT,
        (_, true) => Color32::WHITE,
    };
    paint_diamond(ui.painter(), rect.center(), 5.0, state, color);
    let when = crate::app::clock(status.time as f64);
    let help = match state {
        KeyState::Still => format!(
            "Animate this setting: adds a keyframe at {when}. Then move the playhead and change the setting to add more."
        ),
        KeyState::Animated => {
            format!("This setting is animated. Click to add a keyframe at {when}, or just change the setting here.")
        }
        KeyState::KeyHere => format!("There is a keyframe at {when}. Click to remove it."),
    };
    let response = response.on_hover_text(help);
    if response.clicked() {
        request(ui.ctx(), Request::Toggle(property.clone()));
    }
    response.context_menu(|ui| {
        let add = if state == KeyState::KeyHere { "Remove keyframe here" } else { "Add keyframe here" };
        if ui.button(add).clicked() {
            request(ui.ctx(), Request::Toggle(property.clone()));
            ui.close();
        }
        if ui.add_enabled(state != KeyState::Still, egui::Button::new("Stop animating")).clicked() {
            request(ui.ctx(), Request::Stop(property.clone()));
            ui.close();
        }
    });
}

/// Applies the diamond buttons' requests to `scene` at `time`. `fallback` gives a setting's
/// value when the scene doesn't hold one, such as an effect slider left at its default.
pub fn apply_requests(
    scene: &mut Scene,
    requests: Vec<Request>,
    time: f32,
    fallback: impl Fn(&Property) -> Option<Value>,
) {
    for request in requests {
        match request {
            Request::Toggle(property) => {
                let shown = scene.at_time(time);
                let Some(value) = property.get(&shown).or_else(|| fallback(&property)) else { continue };
                match scene.track_mut(&property) {
                    Some(track) => match track.key_near(time, SAME_TIME) {
                        Some(i) => {
                            let removed = track.keys.remove(i);
                            if track.keys.is_empty() {
                                scene.animation.retain(|t| t.property != property);
                                property.set(scene, removed.value);
                            }
                        }
                        None => {
                            track.set_key(time, value);
                        }
                    },
                    None => {
                        let mut track = Track::new(property.clone());
                        track.set_key(time, value);
                        scene.animation.push(track);
                        property.set(scene, value);
                    }
                }
            }
            Request::Stop(property) => stop_animating(scene, &property, time),
        }
    }
}

/// Drops a setting's keyframes, keeping the value it has at `time`.
pub fn stop_animating(scene: &mut Scene, property: &Property, time: f32) {
    let value = scene.track(property).and_then(|t| t.value_at(time));
    scene.animation.retain(|t| t.property != *property);
    if let Some(value) = value {
        property.set(scene, value);
    }
}

/// Takes the panels' edits of `shown` (the scene at `time`, made from `base`) back into the
/// scene: an animated setting that changed gets a keyframe at `time`, and every animated
/// setting gets its plain value from `base` again.
pub fn write_back(base: &Scene, shown: &Scene, mut edited: Scene, time: f32) -> Scene {
    // When effects were added, removed or reordered, effect indices no longer line up between
    // the copies, so effect settings are left as they are this frame.
    let restructured = shown.animation != edited.animation || shown.effects.len() != edited.effects.len();
    let properties: Vec<Property> = edited.animation.iter().map(|t| t.property.clone()).collect();
    for property in properties {
        if restructured && property.effect().is_some() {
            continue;
        }
        let now = property.get(&edited);
        if let Some(value) = now
            && now != property.get(shown)
            && let Some(track) = edited.track_mut(&property)
        {
            track.set_key(time, value);
        }
        if base.track(&property).is_some() {
            property.copy(base, &mut edited);
        }
    }
    edited
}

/// A setting's name as the timeline shows it.
pub fn name(property: &Property, scene: &Scene) -> String {
    let effect_name = |i: &u32| {
        let shader = scene.effects.get(*i as usize).map_or("Missing effect", |e| e.shader.as_str());
        crate::inspector::pretty_name(shader)
    };
    match property {
        Property::Gravity => "Gravity".into(),
        Property::SidewaysGravity => "Sideways gravity".into(),
        Property::Bounciness => "Bounciness".into(),
        Property::AirDrag => "Air drag".into(),
        Property::SpeedGain => "Speed up".into(),
        Property::ArenaSize => "Arena size".into(),
        Property::ArenaSpin => "Arena spin".into(),
        Property::ShrinkSpeed => "Shrink".into(),
        Property::ArenaColor => "Arena colour".into(),
        Property::BackgroundTop => "Background top".into(),
        Property::BackgroundBottom => "Background bottom".into(),
        Property::TrailWidth => "Trail width".into(),
        Property::TrailOpacity => "Trail brightness".into(),
        Property::BallGlow => "Ball glow".into(),
        Property::FlashStrength => "Flash strength".into(),
        Property::FlashSize => "Flash size".into(),
        Property::Bloom => "Bloom".into(),
        Property::Exposure => "Brightness".into(),
        Property::StringOpacity => "String brightness".into(),
        Property::StringWidth => "String width".into(),
        Property::EffectAmount(i) => format!("{} amount", effect_name(i)),
        Property::EffectParam(i, param) => {
            format!("{} {}", effect_name(i), crate::inspector::pretty_name(param).to_lowercase())
        }
    }
}

/// The inspector tab a setting lives in.
pub fn tab(property: &Property) -> Tab {
    match property {
        Property::SpeedGain => Tab::Bounces,
        Property::EffectAmount(_) | Property::EffectParam(..) => Tab::Effects,
        Property::BackgroundTop
        | Property::BackgroundBottom
        | Property::TrailWidth
        | Property::TrailOpacity
        | Property::BallGlow
        | Property::FlashStrength
        | Property::FlashSize
        | Property::Bloom
        | Property::Exposure
        | Property::StringOpacity
        | Property::StringWidth => Tab::Look,
        _ => Tab::Arena,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::Effect;

    #[test]
    fn editing_an_animated_setting_adds_a_keyframe_and_keeps_the_plain_value() {
        let mut base = Scene::default();
        base.look.bloom_strength = 0.15;
        let mut track = Track::new(Property::Bloom);
        track.set_key(0.0, Value::Number(0.0));
        track.set_key(2.0, Value::Number(1.0));
        base.animation.push(track);

        let shown = base.at_time(1.0);
        assert_eq!(shown.look.bloom_strength, 0.5);
        // Nothing edited: the scene comes back exactly as it was.
        assert_eq!(write_back(&base, &shown, shown.clone(), 1.0), base);

        let mut edited = shown.clone();
        edited.look.bloom_strength = 0.9;
        edited.look.exposure = 1.4;
        let result = write_back(&base, &shown, edited, 1.0);
        assert_eq!(result.look.bloom_strength, 0.15);
        assert_eq!(result.look.exposure, 1.4);
        let keys = &result.track(&Property::Bloom).unwrap().keys;
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[1].value, Value::Number(0.9));
        assert_eq!(result.at_time(1.0).look.bloom_strength, 0.9);
    }

    #[test]
    fn diamond_requests_start_add_remove_and_stop_animation() {
        let mut scene = Scene::default();
        scene.look.bloom_strength = 0.3;
        let none = |_: &Property| None;
        apply_requests(&mut scene, vec![Request::Toggle(Property::Bloom)], 1.0, none);
        assert_eq!(scene.track(&Property::Bloom).unwrap().keys.len(), 1);

        // A second keyframe at another time with another value.
        scene.track_mut(&Property::Bloom).unwrap().set_key(3.0, Value::Number(0.9));
        apply_requests(&mut scene, vec![Request::Toggle(Property::Bloom)], 2.0, none);
        let keys = &scene.track(&Property::Bloom).unwrap().keys;
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[1].time, 2.0);

        // Clicking at a keyframe removes it.
        apply_requests(&mut scene, vec![Request::Toggle(Property::Bloom)], 2.0, none);
        assert_eq!(scene.track(&Property::Bloom).unwrap().keys.len(), 2);

        // Stopping keeps the value at the playhead.
        let at_three = scene.at_time(3.0).look.bloom_strength;
        apply_requests(&mut scene, vec![Request::Stop(Property::Bloom)], 3.0, none);
        assert!(scene.animation.is_empty());
        assert_eq!(scene.look.bloom_strength, at_three);
    }

    #[test]
    fn removing_the_last_keyframe_keeps_its_value() {
        let mut scene = Scene::default();
        let mut track = Track::new(Property::Exposure);
        track.set_key(1.0, Value::Number(2.0));
        scene.animation.push(track);
        apply_requests(&mut scene, vec![Request::Toggle(Property::Exposure)], 1.0, |_| None);
        assert!(scene.animation.is_empty());
        assert_eq!(scene.look.exposure, 2.0);
    }

    #[test]
    fn effect_sliders_at_their_default_animate_from_the_default() {
        let mut scene = Scene::default();
        scene.effects.push(Effect::new("echo"));
        let property = Property::EffectParam(0, "zoom".into());
        apply_requests(&mut scene, vec![Request::Toggle(property.clone())], 0.5, |_| Some(Value::Number(1.0)));
        assert_eq!(scene.track(&property).unwrap().keys[0].value, Value::Number(1.0));

        // A frame in which the effect list changed leaves effect settings alone.
        let shown = scene.at_time(0.5);
        let mut edited = shown.clone();
        edited.effects.insert(0, Effect::new("film"));
        edited.remap_effect_tracks(|i| Some(i + 1));
        let result = write_back(&scene, &shown, edited.clone(), 0.5);
        assert_eq!(result, edited);
    }

    #[test]
    fn every_setting_has_a_name_and_a_tab() {
        let scene = Scene::default();
        for property in Property::PLAIN {
            assert!(!name(&property, &scene).is_empty());
            let _ = tab(&property);
        }
        assert_eq!(name(&Property::EffectParam(3, "fade".into()), &scene), "Missing effect fade");
    }
}
