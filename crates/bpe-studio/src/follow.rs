//! Follow sound in the settings menus: right-click a setting → Follow sound → a shape → one of
//! its modulators or its Level, or a bus. The setting then moves by a quarter of its span with
//! that part of the sound (a pulse whose source is the sound), shows the range as a band in the
//! shape's colour, and joins the modulator's list of what it moves.

use crate::style;
use bpe_core::{Address, PulseSource, Scene, SettingDef, Settings, SlotId, SoundSource, ThingId, Value, ValueType};
use bpe_kit::sound::{ADSR, BUSES, MOD_KINDS, MixerSettings, VOICE, VoiceSettings};
use eframe::egui::Color32;

/// A share of a setting's span a new Follow sound moves it by.
const SHARE: f64 = 0.25;

/// What a Follow sound menu offers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FollowMenu {
    pub shapes: Vec<FollowShape>,
    /// The buses that are on: number and name.
    pub buses: Vec<(u8, String)>,
}

/// One sounding shape and the parts of its sound a setting can follow.
#[derive(Clone, Debug, PartialEq)]
pub struct FollowShape {
    pub id: ThingId,
    pub name: String,
    pub parts: Vec<(SoundSource, String)>,
}

/// What a setting following sound shows: its band and what it follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Following {
    pub source: PulseSource,
    pub colour: Color32,
    /// How far it moves at full level.
    pub amount: f64,
    /// What it follows, in words ("Shape · ADSR 1").
    pub words: String,
}

/// What the settings grids need for Follow sound: the menu, and what each setting (by key)
/// follows now.
pub struct FollowCx<'a> {
    pub menu: &'a FollowMenu,
    pub now: &'a dyn Fn(&str) -> Option<Following>,
}

/// Every shape whose sound is on, with its modulators that are on and its Level, and every bus
/// that is on (none while no shape makes sound).
pub fn menu(scene: &Scene) -> FollowMenu {
    let mut shapes = Vec::new();
    for id in &scene.order {
        let Some(thing) = scene.things.get(id) else { continue };
        let Some(card) = thing.slots.get(&SlotId::Sound).filter(|c| c.kind == VOICE) else { continue };
        if !VoiceSettings::from_params(&card.params).enabled {
            continue;
        }
        let mut parts: Vec<(SoundSource, String)> = card
            .layers
            .iter()
            .filter(|l| l.enabled && (l.kind == ADSR || MOD_KINDS.contains(&l.kind.as_str())))
            .map(|l| (SoundSource::Modulator(l.id), l.name.clone()))
            .collect();
        parts.push((SoundSource::Level, "Level".to_owned()));
        shapes.push(FollowShape { id: *id, name: thing.name.clone(), parts });
    }
    let mixer = scene
        .things
        .get(&ThingId::ROOT)
        .and_then(|r| r.slots.get(&SlotId::Mixer))
        .map(|s| MixerSettings::from_params(&s.params))
        .unwrap_or_default();
    // A bus is only worth following when some shape plays into one.
    let buses = if shapes.is_empty() {
        Vec::new()
    } else {
        (1..=BUSES as u8).filter(|&n| mixer.bus(n).on).map(|n| (n, mixer.bus(n).name.clone())).collect()
    };
    FollowMenu { shapes, buses }
}

/// What `source` follows, in words: "Shape · ADSR 1", "Shape · Level" or "Bus 1".
pub fn words(scene: &Scene, source: &PulseSource) -> String {
    let PulseSource::Sound { shape, source } = source else { return "Hits".to_owned() };
    let thing = scene.things.get(shape);
    let name = thing.map_or("?", |t| t.name.as_str());
    match source {
        SoundSource::Modulator(layer) => {
            let card = thing.and_then(|t| t.slots.get(&SlotId::Sound));
            let part = card.and_then(|c| c.layer(*layer)).map_or("?", |l| l.name.as_str());
            format!("{name} · {part}")
        }
        SoundSource::Level => format!("{name} · Level"),
        SoundSource::Bus(n) => bus_name(scene, *n),
    }
}

fn bus_name(scene: &Scene, n: u8) -> String {
    let mixer = scene.things.get(&ThingId::ROOT).and_then(|r| r.slots.get(&SlotId::Mixer));
    let name = mixer.map(|s| MixerSettings::from_params(&s.params).bus(n).name.clone()).unwrap_or_default();
    if name.trim().is_empty() { format!("Bus {n}") } else { name }
}

/// The colour a setting following `source` shows: the shape's (a bus's: the text colour).
pub fn colour(scene: &Scene, source: &PulseSource) -> Color32 {
    match source {
        PulseSource::Sound { shape, source } if !matches!(source, SoundSource::Bus(_)) => {
            style::colour_of(scene, *shape)
        }
        _ => style::TEXT_STRONG,
    }
}

/// How far a setting of `def` moves when it starts following sound: a quarter of its span (or
/// towards white, for a colour). `None` for a kind of setting that can't follow.
pub fn amount_for(def: &SettingDef) -> Option<Value> {
    let span = def.range.or(def.span).map(|(lo, hi)| (hi - lo) * SHARE);
    match def.ty {
        ValueType::Number => Some(Value::Number(span.unwrap_or(1.0))),
        ValueType::Int => Some(Value::Int(span.unwrap_or(1.0).round().max(1.0) as i64)),
        ValueType::Vec2 => {
            let s = span.unwrap_or(100.0) as f32;
            Some(Value::Vec2([s, s]))
        }
        ValueType::Color => Some(Value::Color(bpe_core::Color::rgb(255, 255, 255))),
        _ => None,
    }
}

/// Whether a setting of `def` can follow sound: it takes keys or pulses, and is a number, a
/// whole number, a pair or a colour.
pub fn can_follow(def: &SettingDef) -> bool {
    (def.flags.keyframe || def.flags.pulse) && amount_for(def).is_some()
}

/// The setting at `a` follows `source` (or stops following, with `None`). False when nothing
/// changed.
pub fn set(scene: &mut Scene, reg: &bpe_core::Registry, a: &Address, source: Option<PulseSource>) -> bool {
    match source {
        None => scene.remove_pulse(a),
        Some(source) => {
            let Some(amount) = scene.def_at(reg, a).and_then(amount_for) else { return false };
            if scene.pulses.get(a).is_some_and(|p| p.source == source) {
                return false;
            }
            scene.set_pulse(reg, a, source, amount, 0.25).is_ok()
        }
    }
}

/// What the setting at `a` follows, when it follows sound.
pub fn now(scene: &Scene, a: &Address) -> Option<Following> {
    let pulse = scene.pulses.get(a).filter(|p| matches!(p.source, PulseSource::Sound { .. }))?;
    let amount = match pulse.amount {
        Value::Number(x) => x,
        Value::Int(x) => x as f64,
        _ => 0.0,
    };
    Some(Following {
        source: pulse.source,
        colour: colour(scene, &pulse.source),
        amount,
        words: words(scene, &pulse.source),
    })
}

/// Every setting following modulator `layer` of `shape`: its address and "<thing> · <setting>".
pub fn followers(
    scene: &Scene,
    reg: &bpe_core::Registry,
    shape: ThingId,
    layer: bpe_core::LayerId,
) -> Vec<(Address, String)> {
    let source = PulseSource::Sound { shape, source: SoundSource::Modulator(layer) };
    scene
        .pulses
        .iter()
        .filter(|(_, p)| p.source == source)
        .map(|(a, _)| {
            let thing = scene.things.get(&a.thing).map_or("?", |t| t.name.as_str());
            let setting = scene.def_at(reg, a).map_or(a.key.as_str(), |d| d.label);
            (a.clone(), format!("{thing} · {setting}"))
        })
        .collect()
}
