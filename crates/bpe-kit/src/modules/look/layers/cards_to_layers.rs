//! Opens files from before format 10, when trails and particles were cards of their own: a
//! thing's Trail card becomes a Streak trail layer and its Particles card an On hit particles
//! layer, at the end of its Look's "Trails and particles" list, with the same values, keyframes,
//! pulses and Follow sound.

use bpe_core::{Address, Layer, LayerId, Params, Scene, SlotId, ThingId, Value, layered_key};

use super::particles::{PARTICLES, SHAPE_PARTICLES};
use super::trail::TRAIL;

/// The old card kinds.
pub const OLD_TRAIL: &str = "trail.streak";
pub const OLD_PARTICLES: &str = "particles.burst";

/// Runs on files below format 10: every thing's Trail and Particles cards become layers of its
/// Look (a Look of another kind, or none, leaves them out), and the cards go.
pub fn cards_to_layers(scene: &mut Scene) {
    let ids: Vec<ThingId> = scene.things.keys().copied().collect();
    for id in ids {
        if let Some(card) = take_card(scene, id, SlotId::Trail, OLD_TRAIL) {
            add_layer(scene, id, SlotId::Trail, card, |look| (look == super::CARD).then_some(TRAIL), trail_value);
        }
        if let Some(card) = take_card(scene, id, SlotId::Particles, OLD_PARTICLES) {
            let kind = |look: &str| match look {
                super::CARD => Some(PARTICLES),
                crate::modules::look::shape::CARD => Some(SHAPE_PARTICLES),
                _ => None,
            };
            add_layer(scene, id, SlotId::Particles, card, kind, particles_value);
        }
        // Whatever else sat in the old slots (a card of an unknown kind) has nowhere to go.
        if let Some(thing) = scene.things.get_mut(&id) {
            thing.slots.remove(&SlotId::Trail);
            thing.slots.remove(&SlotId::Particles);
        }
        drop_animation(scene, id, SlotId::Trail);
        drop_animation(scene, id, SlotId::Particles);
    }
}

/// The card of `kind` in `slot`, taken out of the thing.
fn take_card(scene: &mut Scene, id: ThingId, slot: SlotId, kind: &str) -> Option<Params> {
    let thing = scene.things.get_mut(&id)?;
    if thing.slots.get(&slot)?.kind != kind {
        return None;
    }
    thing.slots.remove(&slot).map(|c| c.params)
}

/// A Trail card's setting as a trail layer's: Taper becomes Width at the tail, 1 - taper.
fn trail_value(key: &str, v: &Value) -> (String, Value) {
    match (key, v) {
        ("taper", Value::Number(t)) => ("tail_width".into(), Value::Number((1.0 - t).clamp(0.0, 1.0))),
        ("taper", _) => ("tail_width".into(), v.clone()),
        _ => (key.into(), v.clone()),
    }
}

/// A Particles card's setting as a particles layer's: Preset becomes Made of.
fn particles_value(key: &str, v: &Value) -> (String, Value) {
    match key {
        "preset" => ("made_of".into(), v.clone()),
        _ => (key.into(), v.clone()),
    }
}

/// Adds the layer the card `params` (from `slot`) becomes to the thing's Look, when `kind` says
/// what it is for that Look's card kind, and moves the card's keyframes and pulses onto it.
fn add_layer(
    scene: &mut Scene,
    id: ThingId,
    slot: SlotId,
    params: Params,
    kind: impl Fn(&str) -> Option<&'static str>,
    value: fn(&str, &Value) -> (String, Value),
) {
    let Some(look) = scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&SlotId::Look)) else { return };
    let Some(kind) = kind(&look.kind) else { return };
    let layer = look.next_layer_id();
    let name = free_name(&look.layers, if kind == TRAIL { "Trail" } else { "Particles" });
    // After the drawn layers and the trails and particles already there, before the reactions
    // and the group layers.
    let at = look
        .layers
        .iter()
        .position(|l| l.parent.is_none() && (l.kind.starts_with("react.") || l.kind.starts_with("group.")))
        .unwrap_or(look.layers.len());
    look.layers.insert(at, Layer { id: layer, kind: kind.into(), name, ..Layer::default() });
    for (k, v) in params.iter() {
        let (k, v) = value(k, v);
        look.params.set(&layered_key(layer, &k), v);
    }
    if kind == TRAIL {
        // The old streak faded out quickly at first.
        look.params.set(&layered_key(layer, "fade"), Value::Choice("out".into()));
    } else {
        // Old bursts drew over the balls.
        look.params.set(&layered_key(layer, "place"), Value::Choice("in_front".into()));
    }
    move_animation(scene, id, slot, layer, value);
}

/// `base`, or `base` with the lowest free number from 2.
fn free_name(layers: &[Layer], base: &str) -> String {
    let taken = |n: &str| layers.iter().any(|l| l.name == n);
    if !taken(base) {
        return base.to_owned();
    }
    (2u32..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).expect("a free number exists")
}

/// Moves the keyframes and pulses on the thing's `slot` card onto `layer` of its Look, each key
/// renamed (and a keyframe's values changed) as `value` says.
fn move_animation(
    scene: &mut Scene,
    id: ThingId,
    slot: SlotId,
    layer: LayerId,
    value: fn(&str, &Value) -> (String, Value),
) {
    let on = |a: &Address| a.thing == id && a.slot == Some(slot);
    let to = |a: &Address| {
        let (key, _) = value(&a.key, &Value::Bool(false));
        Address::on(id, SlotId::Look, &layered_key(layer, &key))
    };
    let tracks: Vec<Address> = scene.tracks.keys().filter(|a| on(a)).cloned().collect();
    for a in tracks {
        let mut track = scene.tracks.remove(&a).expect("listed");
        for k in &mut track.keys {
            k.value = value(&a.key, &k.value).1;
        }
        scene.tracks.insert(to(&a), track);
    }
    let pulses: Vec<Address> = scene.pulses.keys().filter(|a| on(a)).cloned().collect();
    for a in pulses {
        let pulse = scene.pulses.remove(&a).expect("listed");
        scene.pulses.insert(to(&a), pulse);
    }
}

/// Drops whatever keyframes and pulses are left on the thing's `slot`.
fn drop_animation(scene: &mut Scene, id: ThingId, slot: SlotId) {
    let on = |a: &Address| a.thing == id && a.slot == Some(slot);
    scene.tracks.retain(|a, _| !on(a));
    scene.pulses.retain(|a, _| !on(a));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::look::layers::{SUB_GLOW, trail::TrailLayer};
    use crate::registry;
    use bpe_core::{Settings, Slot};

    /// Layer `id`'s own values in `card`, by plain key.
    fn layer_params(card: &Slot, id: LayerId) -> Params {
        let mut p = Params::default();
        for (k, v) in card.params.iter() {
            if let Some((l, key)) = bpe_core::split_layered(k)
                && l == id
            {
                p.set(key, v.clone());
            }
        }
        p
    }

    /// A format 9 file of a ball with a Trail and a Particles card, keyframes on the trail's
    /// Length and Taper, and a shape with a Particles card.
    fn old_file() -> (String, ThingId, ThingId) {
        let reg = registry();
        let mut scene = crate::new_studio_scene(&reg);
        let ball = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let shape = crate::add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
        scene.add_layer(&reg, ball, SlotId::Look, crate::modules::look::layers::FLASH).unwrap();
        let mut trail = Slot::new(OLD_TRAIL);
        trail.params.set("seconds", Value::Number(0.8));
        trail.params.set("taper", Value::Number(0.25));
        trail.params.set("source", Value::Choice("own".into()));
        let mut burst = Slot::new(OLD_PARTICLES);
        burst.params.set("preset", Value::Choice("confetti".into()));
        burst.params.set("count", Value::Int(30));
        let t = scene.things.get_mut(&ball).unwrap();
        t.slots.insert(SlotId::Trail, trail);
        t.slots.insert(SlotId::Particles, burst.clone());
        scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Particles, burst);
        let key = |k: &str| Address::on(ball, SlotId::Trail, k);
        let track = |a: f64, b: f64| bpe_core::Track {
            keys: vec![
                bpe_core::Key { time: 0.0, value: Value::Number(a), ease: Default::default() },
                bpe_core::Key { time: 1.0, value: Value::Number(b), ease: Default::default() },
            ],
            range: Some((0.0, 2.0)),
        };
        scene.tracks.insert(key("seconds"), track(0.2, 1.0));
        scene.tracks.insert(key("taper"), track(1.0, 0.25));
        let text = bpe_core::save(&scene).replace("format: 10", "format: 9");
        (text, ball, shape)
    }

    #[test]
    fn old_trail_and_particle_cards_open_as_layers_with_their_keyframes() {
        let reg = registry();
        let (text, ball, shape) = old_file();
        let scene = bpe_core::load(&text, &reg).unwrap();
        let t = &scene.things[&ball];
        assert!(!t.slots.contains_key(&SlotId::Trail) && !t.slots.contains_key(&SlotId::Particles));
        let look = &t.slots[&SlotId::Look];
        let kinds: Vec<&str> = look.layers.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, [crate::modules::look::layers::BODY, TRAIL, PARTICLES, crate::modules::look::layers::FLASH]);
        let trail = look.layers[1].id;
        let p = layer_params(scene.card(ball, SlotId::Look).unwrap(), trail);
        let layer = TrailLayer::from_params(&p);
        assert_eq!(layer.seconds, 0.8);
        assert_eq!(layer.tail_width, 0.75);
        assert_eq!(layer.source, crate::modules::look::layers::trail::TrailSource::Own);
        assert_eq!(layer.fade, crate::modules::look::layers::trail::TrailFade::Out);
        let burst = look.layers[2].id;
        let p = layer_params(look, burst);
        assert_eq!(p.get("made_of"), Some(&Value::Choice("confetti".into())));
        assert_eq!(p.get("count"), Some(&Value::Int(30)));
        assert_eq!(p.get("place"), Some(&Value::Choice("in_front".into())));
        // Keyframes moved to the layer's settings, Taper's turned round.
        let on = |k: &str| Address::on(ball, SlotId::Look, &layered_key(trail, k));
        let seconds: Vec<Value> = scene.tracks[&on("seconds")].keys.iter().map(|k| k.value.clone()).collect();
        assert_eq!(seconds, [Value::Number(0.2), Value::Number(1.0)]);
        let tail: Vec<Value> = scene.tracks[&on("tail_width")].keys.iter().map(|k| k.value.clone()).collect();
        assert_eq!(tail, [Value::Number(0.0), Value::Number(0.75)]);
        assert!(scene.tracks.keys().all(|a| a.slot != Some(SlotId::Trail)));
        // The shape's burst is a shape Particles layer.
        let kinds: Vec<&str> =
            scene.things[&shape].slots[&SlotId::Look].layers.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, [crate::modules::look::shape::BODY, SHAPE_PARTICLES]);
        // The new layers are ordinary: they take sub-layers.
        let mut scene = scene;
        scene.add_sub_layer(&reg, ball, SlotId::Look, trail, SUB_GLOW).unwrap();
        assert!(bpe_core::resolve(&scene, &reg).problems.is_empty());
    }

    #[test]
    fn a_format_10_file_is_left_alone() {
        let reg = registry();
        let (text, ball, _) = old_file();
        let scene = bpe_core::load(&text.replace("format: 9", "format: 10"), &reg).unwrap();
        assert_eq!(scene.things[&ball].slots[&SlotId::Trail].kind, OLD_TRAIL, "kept for resolve to report");
    }
}
