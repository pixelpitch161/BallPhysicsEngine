//! Lighting scenes (lighting spec, part 6): named snapshots of every Light's and Rig's settings,
//! kept on the scene's Lighting card. While a scene shows, its saved settings stand in for the
//! lights' own; a change crossfades from the scene before. Which scene shows is worked out from
//! the moment drawn (the timeline's Show scene keys, the bars, or the hits), so nothing is kept
//! between frames.

use std::borrow::Cow;

use bpe_core::{
    Address, Choice, Color, EditError, LIGHT_SCENES_KEY, LightingScene, Params, Registry, Scene, SceneLight,
    SettingValue, Settings, SlotId, Target, ThingId, Value,
};
use bpe_gfx::DrawCx;

use crate::things::light::LIGHT;
use crate::things::rig::{RIG, pattern};
use crate::things::scene::SceneSettings;

/// What shows the lighting scenes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ScenesPlay {
    /// The lights keep their own settings.
    #[default]
    Off,
    /// Show scene picks one; its keys on the timeline switch scenes.
    OnTheTimeline,
    /// A new scene every so many bars.
    InTurn,
    /// A new scene on the hits of the things heard.
    OnHits,
}

/// The order scenes come in, in turn or on hits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SceneOrder {
    /// First to last, then round again.
    #[default]
    InOrder,
    /// First to last and back again.
    BackAndForth,
    /// Any scene but the one showing (change the Seed for another run).
    Random,
}

/// A light's settings a scene doesn't hold: what the light is and what it rides, not how it looks.
pub const STRUCTURE: [&str; 7] =
    ["kind", "strip_along", "strip_shape", "area_shape", "rides_on", "ball_number", "flare_from"];

/// The setting that picks the scene on the timeline.
pub const SHOW_SCENE: &str = "show_scene";

/// Which scenes show at the moment drawn: `from` crossfading to `to` (`mix` 0 to 1), each 0 for
/// the lights' own settings or a scene's number from 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScenesNow<'a> {
    pub scenes: &'a [LightingScene],
    pub from: usize,
    pub to: usize,
    pub mix: f32,
}

impl<'a> ScenesNow<'a> {
    /// Nothing showing: every light keeps its own settings.
    pub const OFF: ScenesNow<'static> = ScenesNow { scenes: &[], from: 0, to: 0, mix: 1.0 };

    /// `thing`'s settings as the showing scenes have them, from its own (resolved) `own`.
    pub fn params<'p>(&self, thing: ThingId, own: &'p Params) -> Cow<'p, Params> {
        let to = self.saved(self.to, thing, own);
        if self.mix >= 1.0 || self.from == self.to {
            return to;
        }
        let from = self.saved(self.from, thing, own);
        if self.mix <= 0.0 {
            return from;
        }
        if matches!((&from, &to), (Cow::Borrowed(_), Cow::Borrowed(_))) {
            return to;
        }
        Cow::Owned(blend_params(&from, &to, self.mix))
    }

    /// `thing`'s settings in scene `index` (from 1): its own, with what the scene saved over them.
    fn saved<'p>(&self, index: usize, thing: ThingId, own: &'p Params) -> Cow<'p, Params> {
        let Some(saved) = index.checked_sub(1).and_then(|i| self.scenes.get(i)).and_then(|s| s.of(thing)) else {
            return Cow::Borrowed(own);
        };
        let mut out = own.clone();
        for (key, value) in &saved.0 {
            if let Some(slot) = out.0.get_mut(key) {
                *slot = value.clone();
            }
        }
        Cow::Owned(out)
    }

    /// "Scene 2", "Scene 1 → Scene 2 · 40%", "The lights' own": what shows, for the editor.
    pub fn words(&self) -> String {
        let name = |i: usize| match i.checked_sub(1).and_then(|i| self.scenes.get(i)) {
            Some(s) => s.name.clone(),
            None => "The lights' own".to_owned(),
        };
        if self.mix >= 1.0 || self.from == self.to {
            name(self.to)
        } else {
            format!("{} → {} · {:.0}%", name(self.from), name(self.to), self.mix * 100.0)
        }
    }
}

/// Every setting of `to`, blended from `from` by `mix`.
pub fn blend_params(from: &Params, to: &Params, mix: f32) -> Params {
    Params(
        to.0.iter()
            .map(|(k, b)| {
                let v = from.0.get(k).map_or_else(|| b.clone(), |a| blend_value(a, b, mix));
                (k.clone(), v)
            })
            .collect(),
    )
}

/// `a` blended to `b` by `t`: numbers, points and colours (in linear light) smoothly, whole
/// numbers rounded, and anything else switching half way.
pub fn blend_value(a: &Value, b: &Value, t: f32) -> Value {
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let colour = |x: Color, y: Color| {
        let (p, q) = (x.to_linear(), y.to_linear());
        Color::from_linear([lerp(p[0], q[0]), lerp(p[1], q[1]), lerp(p[2], q[2]), lerp(p[3], q[3])])
    };
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Value::Number(x + (y - x) * t as f64),
        (Value::Int(x), Value::Int(y)) => Value::Int((*x as f64 + (*y - *x) as f64 * t as f64).round() as i64),
        (Value::Vec2([ax, ay]), Value::Vec2([bx, by])) => Value::Vec2([lerp(*ax, *bx), lerp(*ay, *by)]),
        (Value::Color(x), Value::Color(y)) => Value::Color(colour(*x, *y)),
        (Value::Gradient(x), Value::Gradient(y)) if x.stops.len() == y.stops.len() => {
            let mut g = if t < 0.5 { x.clone() } else { y.clone() };
            for (s, (p, q)) in g.stops.iter_mut().zip(x.stops.iter().zip(&y.stops)) {
                s.at = lerp(p.at, q.at);
                s.color = colour(p.color, q.color);
            }
            Value::Gradient(g)
        }
        _ if t < 0.5 => a.clone(),
        _ => b.clone(),
    }
}

/// Which scenes show at the moment drawn.
pub fn now<'a>(cx: &DrawCx<'a>) -> ScenesNow<'a> {
    let Some(root) = cx.scene.things.get(&ThingId::ROOT) else { return ScenesNow::OFF };
    let Some(card) = root.slots.get(&SlotId::Lighting) else { return ScenesNow::OFF };
    let Some(Value::LightScenes(scenes)) = card.params.get(LIGHT_SCENES_KEY) else { return ScenesNow::OFF };
    let get = |key: &str| card.params.get(key).cloned();
    let keys = cx.tree.tracks.get(&show_address()).map(|t| t.keys.as_slice()).unwrap_or_default();
    let tempo = SceneSettings::from_params(&root.params);
    let hits = |target: &Target, gap: f64| {
        let heard = cx.world.matching(target, ThingId::ROOT);
        Some(pattern::hits_heard(cx.log, cx.world.steps_per_second, &heard, cx.time, gap))
    };
    let card = |time: f64| pick(&get, scenes.len(), &tempo, keys, time, &hits);
    // A rule's scene (rules part 6) shows from its moment on, crossfading from what showed then.
    let shown = cx.world.rules().and_then(|r| r.light_scene().copied());
    let picked = match shown {
        Some(sh) => {
            let at = sh.step as f64 / cx.world.steps_per_second.max(1) as f64;
            let showing = |t: f64| card(t).map_or(0, |(_, to, _)| to);
            let from = sh.from.map_or_else(|| showing(at), |n| n as usize);
            match sh.to {
                Some(to) => {
                    let fade = match sh.fade {
                        Some(f) => f as f64,
                        None => number_of(&get, "scene_fade", 0.0).max(0.0),
                    };
                    let mix = if fade <= 0.0 { 1.0 } else { ((cx.time - at) / fade).clamp(0.0, 1.0) as f32 };
                    Some((from, to as usize, mix))
                }
                // Back to the card: what it shows now, crossfading from the rule's scene.
                None => {
                    let fade = sh.fade.map_or_else(|| number_of(&get, "scene_fade", 0.0).max(0.0), |f| f as f64);
                    let mix = if fade <= 0.0 { 1.0 } else { ((cx.time - at) / fade).clamp(0.0, 1.0) as f32 };
                    let now = card(cx.time);
                    if mix >= 1.0 { now } else { Some((from, now.map_or(0, |(_, to, _)| to), mix)) }
                }
            }
        }
        None => card(cx.time),
    };
    match picked {
        Some((from, to, mix)) if !scenes.is_empty() => ScenesNow { scenes, from, to, mix },
        _ => ScenesNow::OFF,
    }
}

/// A number setting of the Lighting card, `or` without it.
fn number_of(get: &dyn Fn(&str) -> Option<Value>, key: &str, or: f64) -> f64 {
    match get(key) {
        Some(Value::Number(x)) => x,
        Some(Value::Int(x)) => x as f64,
        _ => or,
    }
}

/// What shows at `time`, for the editor's readout ("Scene 1 → Scene 2 · 40%"); `None` when
/// scenes are off or move on with the hits (which need the playback).
pub fn words_at(scene: &Scene, reg: &Registry, time: f64) -> Option<String> {
    let scenes = scenes_of(scene);
    let get = |key: &str| scene.value_at_time(reg, &Address::on(ThingId::ROOT, SlotId::Lighting, key), time);
    let keys = scene.tracks.get(&show_address()).map(|t| t.keys.as_slice()).unwrap_or_default();
    let root = scene.things.get(&ThingId::ROOT)?;
    let tempo = SceneSettings::from_params(&root.params);
    let (from, to, mix) = pick(&get, scenes.len(), &tempo, keys, time, &|_, _| None)?;
    Some(ScenesNow { scenes: &scenes, from, to, mix }.words())
}

fn show_address() -> Address {
    Address::on(ThingId::ROOT, SlotId::Lighting, SHOW_SCENE)
}

/// Which scenes show at `time` from the Lighting card's settings (`get`), as (from, to, mix);
/// `None` when they are off, there are none, or (`hits` giving `None`) they wait on hits.
fn pick(
    get: &dyn Fn(&str) -> Option<Value>,
    count: usize,
    tempo: &SceneSettings,
    keys: &[bpe_core::Key],
    time: f64,
    hits: &dyn Fn(&Target, f64) -> Option<Vec<f64>>,
) -> Option<(usize, usize, f32)> {
    let play = get("scenes_play").and_then(|v| ScenesPlay::from_value(&v)).unwrap_or_default();
    if count == 0 || play == ScenesPlay::Off {
        return None;
    }
    let number = |key: &str, or: f64| match get(key) {
        Some(Value::Number(x)) => x,
        Some(Value::Int(x)) => x as f64,
        _ => or,
    };
    let fade = number("scene_fade", 0.0).max(0.0);
    let order = get("scene_order").and_then(|v| SceneOrder::from_value(&v)).unwrap_or_default();
    let seed = number("scene_seed", 1.0) as u32;
    let (from, to, start) = match play {
        ScenesPlay::Off => return None,
        ScenesPlay::OnTheTimeline => {
            let in_range = |v: i64| if v >= 1 && v as usize <= count { v as usize } else { 0 };
            let to = in_range(number(SHOW_SCENE, 0.0) as i64);
            // The latest key at or before now that changed the scene starts its crossfade.
            let change = keys
                .windows(2)
                .rev()
                .find(|w| w[1].time <= time && w[1].value != w[0].value)
                .map(|w| (as_int(&w[0].value), w[1].time));
            match change {
                Some((before, at)) => (in_range(before), to, at),
                None => (to, to, f64::NEG_INFINITY),
            }
        }
        ScenesPlay::InTurn => {
            let (beat, top) = pattern::beat_seconds(tempo);
            let length = beat * top as f64 * number("scene_every", 4.0).max(1e-3);
            let k = (time / length).floor().max(0.0) as u64;
            let before = order_at(order, k.saturating_sub(1), count, seed);
            (before, order_at(order, k, count, seed), k as f64 * length)
        }
        ScenesPlay::OnHits => {
            let target = get("scene_heard").and_then(|v| Target::from_value(&v)).unwrap_or_default();
            let hits = hits(&target, number("scene_gap", 0.0).max(0.0))?;
            let k = hits.len() as u64;
            match hits.last() {
                Some(&at) => (order_at(order, k - 1, count, seed), order_at(order, k, count, seed), at),
                None => (1, 1, f64::NEG_INFINITY),
            }
        }
    };
    let mix = if fade <= 0.0 { 1.0 } else { ((time - start) / fade).clamp(0.0, 1.0) as f32 };
    Some((from, to, mix))
}

fn as_int(v: &Value) -> i64 {
    match v {
        Value::Int(x) => *x,
        Value::Number(x) => x.round() as i64,
        _ => 0,
    }
}

/// The scene (from 1) showing at change `k` (0 the first) of `count` scenes in `order`.
pub fn order_at(order: SceneOrder, k: u64, count: usize, seed: u32) -> usize {
    let count = count.max(1) as u64;
    match order {
        SceneOrder::InOrder => (k % count) as usize + 1,
        SceneOrder::BackAndForth if count == 1 => 1,
        SceneOrder::BackAndForth => {
            let period = 2 * (count - 1);
            let r = k % period;
            (if r < count { r } else { period - r }) as usize + 1
        }
        SceneOrder::Random if count == 1 => 1,
        // Each change moves on by 1 to count − 1 scenes, so the same scene never shows twice running.
        SceneOrder::Random => {
            let mut at = 0u64;
            for step in 1..=k {
                at = (at + 1 + mix(seed, step) % (count - 1)) % count;
            }
            at as usize + 1
        }
    }
}

fn mix(seed: u32, step: u64) -> u64 {
    let mut x = (seed as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ step.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 31;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 29)
}

// ---------- Editing (each one undo step in the editor) ----------

fn scenes_address() -> Address {
    Address::on(ThingId::ROOT, SlotId::Lighting, LIGHT_SCENES_KEY)
}

/// The scene's lighting scenes, saved on its Lighting card (none without the card).
pub fn scenes_of(scene: &Scene) -> Vec<LightingScene> {
    let card = scene.things.get(&ThingId::ROOT).and_then(|r| r.slots.get(&SlotId::Lighting));
    match card.and_then(|c| c.params.get(LIGHT_SCENES_KEY)) {
        Some(Value::LightScenes(list)) => list.clone(),
        _ => Vec::new(),
    }
}

fn set_scenes(scene: &mut Scene, reg: &Registry, list: Vec<LightingScene>) -> Result<(), EditError> {
    scene.set_at(reg, &scenes_address(), Value::LightScenes(list))
}

/// Every Light's and Rig's settings at `time` (keyframes applied), as a scene saves them.
pub fn capture(scene: &Scene, reg: &Registry, time: f64) -> Vec<SceneLight> {
    let mut out = Vec::new();
    for &id in &scene.order {
        let Some(thing) = scene.things.get(&id) else { continue };
        if thing.kind != LIGHT && thing.kind != RIG {
            continue;
        }
        let Some(kind) = reg.thing(&thing.kind) else { continue };
        let mut params = Params::default();
        for d in kind.settings() {
            if thing.kind == LIGHT && STRUCTURE.contains(&d.key) {
                continue;
            }
            if let Some(v) = scene.value_at_time(reg, &Address::own(id, d.key), time) {
                params.set(d.key, v);
            }
        }
        out.push(SceneLight { thing: id, params });
    }
    out
}

/// A name no scene in `list` has: "Scene 3".
fn free_name(list: &[LightingScene], stem: &str) -> String {
    (1..).map(|n| format!("{stem} {n}")).find(|name| !list.iter().any(|s| &s.name == name)).expect("a free name")
}

/// Saves the lights as they are at `time` as a new scene at the end; its number (from 1).
pub fn add_scene(scene: &mut Scene, reg: &Registry, time: f64) -> Result<usize, EditError> {
    let mut list = scenes_of(scene);
    let name = free_name(&list, "Scene");
    list.push(LightingScene { name, lights: capture(scene, reg, time) });
    let n = list.len();
    set_scenes(scene, reg, list)?;
    Ok(n)
}

/// Saves the lights as they are at `time` over scene `index` (from 0), keeping its name.
pub fn update_scene(scene: &mut Scene, reg: &Registry, index: usize, time: f64) -> Result<(), EditError> {
    let mut list = scenes_of(scene);
    let lights = capture(scene, reg, time);
    list.get_mut(index).ok_or(EditError::NotAccepted)?.lights = lights;
    set_scenes(scene, reg, list)
}

/// Renames scene `index`.
pub fn rename_scene(scene: &mut Scene, reg: &Registry, index: usize, name: &str) -> Result<(), EditError> {
    let mut list = scenes_of(scene);
    list.get_mut(index).ok_or(EditError::NotAccepted)?.name = name.trim().to_owned();
    set_scenes(scene, reg, list)
}

/// A copy of scene `index` right after it, named "<name> copy".
pub fn duplicate_scene(scene: &mut Scene, reg: &Registry, index: usize) -> Result<(), EditError> {
    let mut list = scenes_of(scene);
    let mut copy = list.get(index).ok_or(EditError::NotAccepted)?.clone();
    copy.name = format!("{} copy", copy.name);
    list.insert(index + 1, copy);
    // Scenes after it move up one: Show scene keeps naming them.
    renumber(scene, reg, |n| if n > index as i64 + 1 { n + 1 } else { n });
    set_scenes(scene, reg, list)
}

/// Moves scene `from` to `to` (both from 0); Show scene keeps naming the same scenes.
pub fn move_scene(scene: &mut Scene, reg: &Registry, from: usize, to: usize) -> Result<(), EditError> {
    let mut list = scenes_of(scene);
    if from >= list.len() || to >= list.len() {
        return Err(EditError::NotAccepted);
    }
    let mut order: Vec<usize> = (0..list.len()).collect();
    let moved = list.remove(from);
    list.insert(to, moved);
    let o = order.remove(from);
    order.insert(to, o);
    renumber(scene, reg, |n| match order.iter().position(|&old| old as i64 + 1 == n) {
        Some(new) if n >= 1 => new as i64 + 1,
        _ => n,
    });
    set_scenes(scene, reg, list)
}

/// Takes scene `index` away; Show scene keys that named it show the lights' own settings.
pub fn remove_scene(scene: &mut Scene, reg: &Registry, index: usize) -> Result<(), EditError> {
    let mut list = scenes_of(scene);
    if index >= list.len() {
        return Err(EditError::NotAccepted);
    }
    list.remove(index);
    let gone = index as i64 + 1;
    renumber(scene, reg, |n| match n {
        n if n == gone => 0,
        n if n > gone => n - 1,
        n => n,
    });
    set_scenes(scene, reg, list)
}

/// Writes scene `index`'s saved settings back onto its lights: as keys at `time` where a setting
/// has keys, else as the plain value. Lights no longer in the scene are skipped.
pub fn put_on_lights(scene: &mut Scene, reg: &Registry, index: usize, time: f64) -> Result<(), EditError> {
    let list = scenes_of(scene);
    let chosen = list.get(index).ok_or(EditError::NotAccepted)?;
    for light in &chosen.lights {
        if !scene.things.contains_key(&light.thing) {
            continue;
        }
        for (key, value) in &light.params.0 {
            let a = Address::own(light.thing, key);
            if scene.def_at(reg, &a).is_none() {
                continue;
            }
            if scene.keyed(&a) {
                scene.set_key(reg, &a, time, value.clone())?;
            } else {
                scene.set_at(reg, &a, value.clone())?;
            }
        }
    }
    Ok(())
}

/// Maps Show scene's value and every key of it through `f`.
fn renumber(scene: &mut Scene, reg: &Registry, f: impl Fn(i64) -> i64) {
    let a = show_address();
    if let Some(Value::Int(n)) = scene.value_at(reg, &a) {
        let _ = scene.set_at(reg, &a, Value::Int(f(n)));
    }
    if let Some(track) = scene.tracks.get_mut(&a) {
        for k in &mut track.keys {
            if let Value::Int(n) = k.value {
                k.value = Value::Int(f(n));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Ease, Key};

    fn own(pairs: &[(&str, Value)]) -> Params {
        let mut p = Params::default();
        for (k, v) in pairs {
            p.set(k, v.clone());
        }
        p
    }

    #[test]
    fn blending_mixes_numbers_and_colours_and_switches_the_rest_half_way() {
        let a = own(&[
            ("strength", Value::Number(1.0)),
            ("colour", Value::Color(Color::rgb(255, 0, 0))),
            ("falloff", Value::Choice("real".into())),
            ("seed", Value::Int(1)),
        ]);
        let b = own(&[
            ("strength", Value::Number(3.0)),
            ("colour", Value::Color(Color::rgb(0, 0, 255))),
            ("falloff", Value::Choice("soft".into())),
            ("seed", Value::Int(4)),
        ]);
        let m = blend_params(&a, &b, 0.25);
        assert_eq!(m.get("strength"), Some(&Value::Number(1.5)));
        assert_eq!(m.get("falloff"), Some(&Value::Choice("real".into())));
        assert_eq!(m.get("seed"), Some(&Value::Int(2)), "1.75 rounds to 2");
        let Some(Value::Color(c)) = m.get("colour") else { panic!() };
        let lin = c.to_linear();
        assert!((lin[0] - 0.75).abs() < 0.01 && (lin[2] - 0.25).abs() < 0.01, "linear mix: {lin:?}");
        assert_eq!(blend_params(&a, &b, 0.5).get("falloff"), Some(&Value::Choice("soft".into())));
    }

    #[test]
    fn a_showing_scene_stands_in_for_what_it_saved_and_nothing_else() {
        let saved = own(&[("strength", Value::Number(4.0)), ("gone", Value::Number(1.0))]);
        let scenes =
            vec![LightingScene { name: "Hot".into(), lights: vec![SceneLight { thing: ThingId(5), params: saved }] }];
        let mine = own(&[("strength", Value::Number(1.0)), ("reach", Value::Number(900.0))]);
        let now = ScenesNow { scenes: &scenes, from: 0, to: 1, mix: 1.0 };
        let p = now.params(ThingId(5), &mine);
        assert_eq!(p.get("strength"), Some(&Value::Number(4.0)));
        assert_eq!(p.get("reach"), Some(&Value::Number(900.0)), "not saved: its own");
        assert_eq!(p.get("gone"), None, "a saved key the light no longer has stays out");
        assert!(matches!(now.params(ThingId(6), &mine), Cow::Borrowed(_)), "a light the scene doesn't hold");
        let half = ScenesNow { mix: 0.5, ..now };
        assert_eq!(half.params(ThingId(5), &mine).get("strength"), Some(&Value::Number(2.5)));
        assert_eq!(half.words(), "The lights' own → Hot · 50%");
    }

    #[test]
    fn orders_go_round_bounce_and_never_repeat_at_random() {
        let in_order: Vec<usize> = (0..5).map(|k| order_at(SceneOrder::InOrder, k, 3, 1)).collect();
        assert_eq!(in_order, [1, 2, 3, 1, 2]);
        let bounce: Vec<usize> = (0..6).map(|k| order_at(SceneOrder::BackAndForth, k, 3, 1)).collect();
        assert_eq!(bounce, [1, 2, 3, 2, 1, 2]);
        let random: Vec<usize> = (0..40).map(|k| order_at(SceneOrder::Random, k, 4, 7)).collect();
        assert_eq!(random[0], 1);
        assert!(random.windows(2).all(|w| w[0] != w[1]), "{random:?}");
        assert!(random.iter().all(|&r| (1..=4).contains(&r)));
    }

    #[test]
    fn scene_edits_keep_show_scene_naming_the_same_scenes() {
        let kit = crate::kit();
        let reg = &kit.registry;
        let mut scene = crate::new_scene(reg);
        crate::things::light::add_light(&mut scene, reg, crate::things::light::LightType::Point, ThingId::ROOT)
            .unwrap();
        for _ in 0..3 {
            add_scene(&mut scene, reg, 0.0).unwrap();
        }
        assert_eq!(
            scenes_of(&scene).iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["Scene 1", "Scene 2", "Scene 3"]
        );
        let a = Address::on(ThingId::ROOT, SlotId::Lighting, SHOW_SCENE);
        for (t, n) in [(0.0, 1), (1.0, 2), (2.0, 3)] {
            scene.set_key(reg, &a, t, Value::Int(n)).unwrap();
        }
        assert!(scene.tracks[&a].keys.iter().all(|k| k.ease == Ease::Hold), "Show scene's keys jump");
        let shown = |s: &Scene| s.tracks[&a].keys.iter().map(|k: &Key| as_int(&k.value)).collect::<Vec<_>>();
        move_scene(&mut scene, reg, 2, 0).unwrap();
        assert_eq!(scenes_of(&scene)[0].name, "Scene 3");
        assert_eq!(shown(&scene), [2, 3, 1]);
        remove_scene(&mut scene, reg, 1).unwrap();
        assert_eq!(scenes_of(&scene).iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Scene 3", "Scene 2"]);
        assert_eq!(shown(&scene), [0, 2, 1]);
        duplicate_scene(&mut scene, reg, 0).unwrap();
        assert_eq!(scenes_of(&scene)[1].name, "Scene 3 copy");
        assert_eq!(shown(&scene), [0, 3, 1]);
    }

    #[test]
    fn a_scene_saves_the_lights_and_puts_them_back() {
        let kit = crate::kit();
        let reg = &kit.registry;
        let mut scene = crate::new_scene(reg);
        let light =
            crate::things::light::add_light(&mut scene, reg, crate::things::light::LightType::Spot, ThingId::ROOT)
                .unwrap();
        let strength = Address::own(light, "strength");
        scene.set_at(reg, &strength, Value::Number(3.0)).unwrap();
        add_scene(&mut scene, reg, 0.0).unwrap();
        let saved = &scenes_of(&scene)[0];
        assert_eq!(saved.of(light).and_then(|p| p.get("strength")), Some(&Value::Number(3.0)));
        assert_eq!(saved.of(light).and_then(|p| p.get("kind")), None, "what the light is isn't saved");
        scene.set_at(reg, &strength, Value::Number(0.5)).unwrap();
        put_on_lights(&mut scene, reg, 0, 0.0).unwrap();
        assert_eq!(scene.value_at(reg, &strength), Some(Value::Number(3.0)));
        // A keyed setting gets a key at the moment instead.
        scene.set_key(reg, &strength, 0.0, Value::Number(1.0)).unwrap();
        put_on_lights(&mut scene, reg, 0, 2.0).unwrap();
        assert_eq!(scene.tracks[&strength].keys.len(), 2);
        assert_eq!(scene.value_at_time(reg, &strength, 2.0), Some(Value::Number(3.0)));
    }
}
