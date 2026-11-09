//! Follow sound in the settings menus: right-click a setting → Follow sound → a shape → one of
//! its modulators or its Level, or a bus. The setting then moves by a quarter of its span with
//! that part of the sound (a pulse whose source is the sound), shows the range as a band in the
//! shape's colour, and joins the modulator's list of what it moves.
//!
//! Follow light (lighting part 7) sits beside it: Follow light → a thing → how lit it is, its
//! brightest light, its shadow or In shadow, or a light's strength as it shines. Only in a scene
//! with a Lighting card, and never on a setting the lighting itself reads (sensing is one-way).
//!
//! Follow health (rules part 4) too: Follow health → a thing with a Health card. The setting
//! moves with how much health it has left (all of its balls', on average, for a group).

use crate::style;
use bpe_core::{
    Address, LightSource, PulseSource, Scene, SettingDef, Settings, SlotId, SoundSource, ThingId, Value, ValueType,
};
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
    /// What a setting can follow the light of (none without a Lighting card).
    pub lit: Vec<FollowLit>,
    /// The things with a Health card, by id and name, in tree order.
    pub health: Vec<(ThingId, String)>,
}

/// A thing whose light a setting can follow, and what of it.
#[derive(Clone, Debug, PartialEq)]
pub struct FollowLit {
    pub id: ThingId,
    pub name: String,
    pub parts: Vec<(LightSource, &'static str)>,
}

/// One sounding shape and the parts of its sound a setting can follow.
#[derive(Clone, Debug, PartialEq)]
pub struct FollowShape {
    pub id: ThingId,
    pub name: String,
    pub parts: Vec<(SoundSource, String)>,
    /// It has a Light modulator that is on: its sound hears the light, so a setting the lighting
    /// reads can't follow it (sensing is one-way).
    pub hears_light: bool,
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

/// What the settings grids need for Follow sound and Follow light: the menu, what each setting
/// (by key) follows now, and whether it may follow light.
pub struct FollowCx<'a> {
    pub menu: &'a FollowMenu,
    pub now: &'a dyn Fn(&str) -> Option<Following>,
    pub light: &'a dyn Fn(&str) -> bool,
}

/// The kinds whose light a setting can follow, and the parts each offers.
fn lit_parts(kind: &str) -> Option<Vec<(LightSource, &'static str)>> {
    let things = vec![
        (LightSource::On, "Light on it"),
        (LightSource::Brightest, "Brightest light on it"),
        (LightSource::Shadow, "Shadow on it"),
        (LightSource::InShadow, "In shadow (a switch)"),
    ];
    match kind {
        "balls" | "emitter" | "shape" | bpe_kit::things::text::KIND => Some(things),
        "light" => Some(vec![(LightSource::Strength, "Strength as it shines")]),
        _ => None,
    }
}

/// Every thing whose light a setting can follow, in tree order (none without a Lighting card).
pub fn lit_menu(scene: &Scene) -> Vec<FollowLit> {
    if !bpe_kit::sensing::has_lighting(scene) {
        return Vec::new();
    }
    scene
        .order
        .iter()
        .filter_map(|id| {
            let t = scene.things.get(id)?;
            Some(FollowLit { id: *id, name: t.name.clone(), parts: lit_parts(&t.kind)? })
        })
        .collect()
}

/// Every thing with a Health card, in tree order: what a setting can follow the health of.
pub fn health_menu(scene: &Scene) -> Vec<(ThingId, String)> {
    scene
        .order
        .iter()
        .filter_map(|id| scene.things.get(id))
        .filter(|t| t.slots.contains_key(&SlotId::Health))
        .map(|t| (t.id, t.name.clone()))
        .collect()
}

/// Whether the setting at `a` may follow light: the scene has a Lighting card and the lighting
/// doesn't read the setting.
pub fn can_light(scene: &Scene, reg: &bpe_core::Registry, a: &Address) -> bool {
    bpe_kit::sensing::can_follow_light(scene, reg, a)
}

/// Every shape, Text, Balls, Emitter or Particles thing whose sound is on, with its modulators that are on and its
/// Level, and every bus that is on (none while nothing makes sound).
pub fn menu(scene: &Scene) -> FollowMenu {
    use bpe_kit::things::{particles, shape, text};
    let mut shapes = Vec::new();
    for id in &scene.order {
        // Shapes, Texts, Balls and Emitters (W10), and Particles: a Particles thing whose sound a
        // setting follows steps with the physics, so its moments are heard (W12).
        let Some(thing) = scene.things.get(id).filter(|t| {
            [shape::KIND, text::KIND, particles::KIND].contains(&t.kind.as_str())
                || bpe_kit::sound::hears_own_balls(&t.kind)
        }) else {
            continue;
        };
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
        let hears_light = card.layers.iter().any(|l| l.enabled && l.kind == bpe_kit::sound::LIGHT_MOD);
        shapes.push(FollowShape { id: *id, name: thing.name.clone(), parts, hears_light });
    }
    let mixer = scene
        .things
        .get(&ThingId::ROOT)
        .and_then(|r| r.slots.get(&SlotId::Mixer))
        .map(|s| MixerSettings::from_params(&s.params))
        .unwrap_or_default();
    // A bus is only worth following when something plays into one.
    let buses = if shapes.is_empty() {
        Vec::new()
    } else {
        (1..=BUSES as u8).filter(|&n| mixer.bus(n).on).map(|n| (n, mixer.bus(n).name.clone())).collect()
    };
    FollowMenu { shapes, buses, lit: lit_menu(scene), health: health_menu(scene) }
}

/// What `source` follows, in words: "Shape · ADSR 1", "Shape · Level", "Bus 1" or
/// "Balls · In shadow".
pub fn words(scene: &Scene, source: &PulseSource) -> String {
    if let PulseSource::Health { thing } = source {
        let name = scene.things.get(thing).map_or("?", |t| t.name.as_str());
        return format!("{name} · Health");
    }
    if let PulseSource::Light { thing, source } = source {
        let name = scene.things.get(thing).map_or("?", |t| t.name.as_str());
        let part = scene
            .things
            .get(thing)
            .and_then(|t| lit_parts(&t.kind))
            .and_then(|p| p.into_iter().find(|(s, _)| s == source))
            .map_or(source.name(), |(_, w)| w);
        return format!("{name} · {part}");
    }
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

/// The colour a setting following `source` shows: the shape's or the thing's (a bus's: the text
/// colour).
pub fn colour(scene: &Scene, source: &PulseSource) -> Color32 {
    match source {
        PulseSource::Light { thing, .. } | PulseSource::Health { thing } => style::colour_of(scene, *thing),
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

/// What the setting at `a` follows, when it follows sound, light or health.
pub fn now(scene: &Scene, a: &Address) -> Option<Following> {
    let pulse = scene.pulses.get(a).filter(|p| {
        matches!(p.source, PulseSource::Sound { .. } | PulseSource::Light { .. } | PulseSource::Health { .. })
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::things::light::{LightType, add_light};

    #[test]
    fn follow_light_is_offered_with_a_lighting_card_and_never_on_what_the_lighting_reads() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        assert!(lit_menu(&scene).is_empty(), "no Lighting card: nothing to follow");
        let light = add_light(&mut scene, &kit.registry, LightType::Point, ThingId::ROOT).unwrap();
        let lit = lit_menu(&scene);
        let ball = scene.things.values().find(|t| t.kind == "balls").unwrap().id;
        assert!(lit.iter().any(|t| t.id == ball && t.parts.iter().any(|(s, _)| *s == LightSource::InShadow)));
        assert!(lit.iter().any(|t| t.id == light && t.parts == vec![(LightSource::Strength, "Strength as it shines")]));
        assert!(can_light(&scene, &kit.registry, &Address::own(ball, "radius")));
        assert!(!can_light(&scene, &kit.registry, &Address::own(light, "strength")), "a light can't follow light");
        let source = PulseSource::Light { thing: ball, source: LightSource::InShadow };
        let name = &scene.things[&ball].name;
        assert_eq!(words(&scene, &source), format!("{name} · In shadow (a switch)"));
        let a = Address::own(ball, "radius");
        assert!(set(&mut scene, &kit.registry, &a, Some(source)));
        assert_eq!(now(&scene, &a).map(|n| n.source), Some(source));
    }

    #[test]
    fn follow_light_offers_text_and_follow_sound_offers_particles() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        add_light(&mut scene, &kit.registry, LightType::Point, ThingId::ROOT).unwrap();
        let text = bpe_kit::add_thing(&mut scene, &kit.registry, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
        let lit = lit_menu(&scene);
        let parts = &lit.iter().find(|t| t.id == text).expect("a Text's light can be followed").parts;
        assert_eq!(parts.len(), 4, "{parts:?}");
        // A Particles thing whose sound is on, and the buses with it alone making sound.
        use bpe_kit::things::particles::looks::{Look, add_particles};
        let p = add_particles(&mut scene, &kit.registry, Look::SparkFountain, ThingId::ROOT).unwrap();
        assert!(menu(&scene).shapes.iter().all(|s| s.id != p), "no sound card yet");
        bpe_kit::fill_slot(&mut scene, &kit.registry, p, SlotId::Sound).unwrap();
        let card = scene.card_mut(p, SlotId::Sound).unwrap();
        card.params.set("enabled", Value::Bool(true));
        let m = menu(&scene);
        let shape = m.shapes.iter().find(|s| s.id == p).expect("its sound can be followed");
        assert!(shape.parts.iter().any(|(s, _)| *s == SoundSource::Level));
        let mixer = scene
            .things
            .get(&ThingId::ROOT)
            .and_then(|r| r.slots.get(&SlotId::Mixer))
            .map(|s| MixerSettings::from_params(&s.params))
            .unwrap_or_default();
        let on: Vec<u8> = (1..=BUSES as u8).filter(|&n| mixer.bus(n).on).collect();
        assert_eq!(m.buses.iter().map(|(n, _)| *n).collect::<Vec<_>>(), on);
    }

    #[test]
    fn follow_health_is_offered_for_things_with_a_health_card() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        assert!(menu(&scene).health.is_empty(), "no Health card: nothing to follow");
        let ball = scene.things.values().find(|t| t.kind == "balls").unwrap().id;
        bpe_kit::fill_slot(&mut scene, &kit.registry, ball, SlotId::Health).unwrap();
        let name = scene.things[&ball].name.clone();
        assert_eq!(menu(&scene).health, vec![(ball, name.clone())]);
        let source = PulseSource::Health { thing: ball };
        assert_eq!(words(&scene, &source), format!("{name} · Health"));
        let a = Address::on(ball, SlotId::Bounce, "bounciness");
        assert!(set(&mut scene, &kit.registry, &a, Some(source)));
        assert_eq!(now(&scene, &a).map(|n| n.source), Some(source));
        // Removing the thing stops what followed its health.
        scene.remove_thing(ball).unwrap();
        assert!(scene.pulses.values().all(|p| p.source != source));
    }

    #[test]
    fn a_balls_sound_can_be_followed() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        assert!(menu(&scene).shapes.is_empty() && menu(&scene).buses.is_empty(), "nothing sounds yet");
        let ball = scene.things.values().find(|t| t.kind == "balls").unwrap().id;
        let card = bpe_kit::sound::starter_voice(&kit.registry);
        scene.things.get_mut(&ball).unwrap().slots.insert(SlotId::Sound, card);
        bpe_kit::ensure_mixer(&mut scene, &kit.registry);
        let m = menu(&scene);
        let theirs = m.shapes.iter().find(|s| s.id == ball).expect("the balls' sound is offered (W10)");
        assert!(theirs.parts.iter().any(|(s, _)| *s == SoundSource::Level));
        assert!(!m.buses.is_empty(), "and the buses it plays into");
    }
}
