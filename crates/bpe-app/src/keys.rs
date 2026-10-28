//! Keyframes in the editor: the diamond button beside each setting that can be animated, and
//! keeping keyframes in step with edits.
//!
//! While a scene has animated settings, the panels edit a copy of it with those settings at
//! their values at the playhead, so sliders show what is on screen. [`write_back`] then turns a
//! change to an animated setting into a keyframe at the playhead and puts the scene's own plain
//! value back.

use crate::inspector::Tab;
use crate::theme;
use bpe_scene::{List, Property, SAME_TIME, Scene, Track, Value};
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

/// What the blocks on the open node canvas belong to, so their settings can be keyframed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canvas {
    /// The rule set at this index in the scene's rules.
    Rule(u32),
    /// A node effect: a block's value is keyframed by making it one of the effect's sliders.
    Shader,
}

fn canvas_id() -> egui::Id {
    egui::Id::unique("keyframe canvas")
}

fn shader_keys_id() -> egui::Id {
    egui::Id::unique("keyframe shader inputs")
}

/// Tells the node canvas drawn next what its blocks belong to.
pub fn set_canvas(ctx: &egui::Context, canvas: Option<Canvas>) {
    ctx.data_mut(|d| d.insert_temp(canvas_id(), canvas));
}

pub fn canvas(ctx: &egui::Context) -> Option<Canvas> {
    ctx.data(|d| d.get_temp::<Option<Canvas>>(canvas_id())).flatten()
}

/// Asks for a keyframe on a node effect block's value, as (block id, input name), once it has
/// become one of the effect's sliders.
pub fn request_shader_key(ctx: &egui::Context, node: u32, input: String) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<(u32, String)>>(shader_keys_id()).push((node, input)));
}

pub fn take_shader_keys(ctx: &egui::Context) -> Vec<(u32, String)> {
    ctx.data_mut(|d| d.remove_temp::<Vec<(u32, String)>>(shader_keys_id())).unwrap_or_default()
}

/// Whether a setting is animated, from what [`publish`] said this frame.
pub fn is_animated(ctx: &egui::Context, property: &Property) -> bool {
    ctx.data(|d| d.get_temp::<Arc<Status>>(status_id())).is_some_and(|s| s.animated.contains_key(property))
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
    // When effects, ball groups, captions or rules were added, removed or reordered, indices no
    // longer line up between the copies, so their settings are left as they are this frame.
    let tracks_changed = shown.animation != edited.animation;
    let lengths = |s: &Scene| [s.effects.len(), s.spawners.len(), s.captions.len(), s.logic.len()];
    let (before, after) = (lengths(shown), lengths(&edited));
    let restructured = |list: List| {
        let i = match list {
            List::Effects => 0,
            List::BallGroups => 1,
            List::Captions => 2,
            List::Rules => 3,
        };
        tracks_changed || before[i] != after[i]
    };
    let properties: Vec<Property> = edited.animation.iter().map(|t| t.property.clone()).collect();
    for property in properties {
        if property.item().is_some_and(|(list, _)| restructured(list)) {
            continue;
        }
        let now = property.get(&edited);
        // Rounding errors are not edits: a 3D camera angle can read back a hair off (WS4).
        if let Some(value) = now
            && !property.get(shown).is_some_and(|shown| shown.same(value))
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
        let Some(effect) = scene.effects.get(*i as usize) else { return "Missing effect".to_string() };
        let name = crate::inspector::pretty_name(&effect.shader);
        // Number effects used more than once, counting from the top.
        let same: Vec<usize> =
            scene.effects.iter().enumerate().filter(|(_, e)| e.shader == effect.shader).map(|(j, _)| j).collect();
        match same.iter().position(|&j| j == *i as usize) {
            Some(n) if same.len() > 1 => format!("{name} {}", n + 1),
            _ => name,
        }
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
        // 3D (WS4)
        Property::CameraTurn => "Camera turn".into(),
        Property::CameraHeight => "Camera height".into(),
        Property::CameraDistance => "Camera distance".into(),
        Property::CameraFov => "Zoom".into(),
        Property::LightTurn => "Light direction".into(),
        Property::LightHeight => "Light height".into(),
        Property::LightBrightness => "Light brightness".into(),
        Property::Glass => "Glass".into(),
        Property::EdgeGlow => "Edge glow".into(),
        Property::SongVolume => "Song volume".into(),
        Property::SongDucking => "Song dip under hits".into(),
        Property::StereoWidth => "Stereo width".into(),
        Property::QuietestHit => "Quietest hit heard".into(),
        Property::LoudestHit => "Full volume at".into(),
        Property::Ball(i, setting) => {
            let group = match scene.spawners.get(*i as usize) {
                Some(bpe_scene::Spawner::Grid { .. }) => "Block of balls",
                Some(bpe_scene::Spawner::Single { .. }) => "Single ball",
                Some(bpe_scene::Spawner::Stream { .. }) => "Stream of balls",
                None => "Missing balls",
            };
            format!("{group} {} {}", i + 1, setting.name())
        }
        Property::Caption(i, setting) => {
            let caption = scene.captions.get(*i as usize).map_or("Missing caption".to_string(), |c| {
                let name = c.default_name();
                let name = if c.name.trim().is_empty() { name } else { c.name.clone() };
                format!("\"{}\"", short(&name, 18))
            });
            format!("{caption} {}", setting.name())
        }
        Property::Rule(rule, id, param) => {
            let Some(graph) = scene.logic.get(*rule as usize) else { return "Missing rule".into() };
            let spec = graph.node(*id).and_then(|n| bpe_graph::Catalog::spec(bpe_sim::rules::catalog(), &n.kind));
            let block = spec.map_or("block".to_string(), |s| s.title.to_lowercase());
            let label = spec.and_then(|s| s.param_spec(param)).map_or(param.clone(), |p| p.label.to_lowercase());
            format!("{}: {block} {label}", short(&graph.name, 20))
        }
    }
}

/// `text` cut to `most` letters, with "…" when cut.
fn short(text: &str, most: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= most {
        return text.to_string();
    }
    let cut: String = text.chars().take(most.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// A rule block setting's value when the block leaves it at the catalog's default.
pub fn rule_default(scene: &Scene, property: &Property) -> Option<Value> {
    let Property::Rule(rule, id, name) = property else { return None };
    let node = scene.logic.get(*rule as usize)?.node(*id)?;
    let spec = bpe_graph::Catalog::spec(bpe_sim::rules::catalog(), &node.kind)?;
    match spec.param_spec(name)?.default {
        bpe_graph::Param::Number(v) => Some(Value::Number(v)),
        bpe_graph::Param::Color(c) => Some(Value::Color(bpe_scene::Color::from_linear(c))),
        _ => None,
    }
}

/// The inspector tab a setting lives in.
pub fn tab(property: &Property) -> Tab {
    match property {
        Property::SpeedGain => Tab::Bounces,
        Property::EffectAmount(_) | Property::EffectParam(..) => Tab::Effects,
        Property::Ball(..) => Tab::Balls,
        Property::Caption(..) => Tab::Text,
        Property::Rule(..) => Tab::Rules,
        Property::SongVolume
        | Property::SongDucking
        | Property::StereoWidth
        | Property::QuietestHit
        | Property::LoudestHit => Tab::Sound,
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
        let twice = Scene { effects: vec![Effect::new("echo"), Effect::new("film"), Effect::new("echo")], ..scene };
        assert_eq!(name(&Property::EffectAmount(2), &twice), "Echo 2 amount");
        assert_eq!(name(&Property::EffectAmount(1), &twice), "Film amount");
    }

    #[test]
    fn new_settings_have_names_and_tabs() {
        let mut scene = Scene::default();
        scene.captions.push(bpe_scene::Caption::new("Can it escape?", bpe_scene::CaptionStyle::Hook));
        scene.logic.push(bpe_sim::rules::recipes().remove(0).graph);
        let flash = scene.logic[0].nodes.iter().find(|n| n.kind == "flash").unwrap().id;
        let cases = [
            (Property::Ball(0, bpe_scene::BallSetting::Glow), Tab::Balls, "1 glow"),
            (Property::Caption(0, bpe_scene::CaptionSetting::Size), Tab::Text, "\"Can it escape?\" size"),
            (Property::Rule(0, flash, "strength".into()), Tab::Rules, "Escape and multiply: flash the screen strength"),
            (Property::SongVolume, Tab::Sound, "Song volume"),
        ];
        for (property, want_tab, part) in cases {
            let name = name(&property, &scene);
            assert!(name.contains(part), "{name}");
            assert_eq!(tab(&property), want_tab, "{property:?}");
        }
        assert_eq!(name(&Property::Caption(5, bpe_scene::CaptionSetting::X), &scene), "Missing caption left and right");
        // A rule setting left at its default starts from the catalog's value.
        let strength = Property::Rule(0, flash, "strength".into());
        let mut plain = scene.clone();
        plain.logic[0].node_mut(flash).unwrap().params.remove("strength");
        assert!(matches!(rule_default(&plain, &strength), Some(Value::Number(_))));
        apply_requests(&mut plain, vec![Request::Toggle(strength.clone())], 1.0, |p| rule_default(&scene, p));
        assert_eq!(plain.track(&strength).unwrap().keys.len(), 1);
    }

    #[test]
    fn editing_an_animated_ball_look_adds_a_keyframe() {
        let mut base = Scene::default();
        *base.spawners[0].style_mut() = Some(bpe_scene::BallStyle::default());
        let glow = Property::Ball(0, bpe_scene::BallSetting::Glow);
        apply_requests(&mut base, vec![Request::Toggle(glow.clone())], 0.0, |_| None);
        let shown = base.at_time(2.0);
        let mut edited = shown.clone();
        edited.spawners[0].style_mut().as_mut().unwrap().glow = 0.7;
        let result = write_back(&base, &shown, edited, 2.0);
        assert_eq!(result.track(&glow).unwrap().keys.len(), 2);
        assert_eq!(result.at_time(2.0).spawners[0].style().unwrap().glow, 0.7);
        assert_eq!(result.spawners[0].style().unwrap().glow, 0.0, "the plain value stays");

        // A frame that removed a ball group leaves ball settings alone.
        let shown = result.at_time(2.0);
        let mut edited = shown.clone();
        edited.spawners.remove(0);
        edited.removed_item(List::BallGroups, 0);
        assert_eq!(write_back(&result, &shown, edited.clone(), 2.0), edited);
    }
}
