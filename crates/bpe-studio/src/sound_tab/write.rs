//! The Sound tab's edits: a knob turned (a key at the playhead when the setting has keys), the
//! sound turned on and off, and a whole sound copied and pasted.

use bpe_core::{Address, LayerId, ROUTES_KEY, Registry, Scene, Slot, SlotId, ThingId, Value};
use bpe_kit::sound::{VOICE, starter_voice};

/// What a [`write`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The setting holds the new value.
    Set,
    /// The setting has keys: a key at the playhead holds it (the studio pauses, as for any keyed
    /// edit, so a drag doesn't key every frame).
    Keyed,
    /// Nothing changed (no such setting, or a value of the wrong kind).
    Failed,
}

/// Sets the setting at `a` to `v`, or keys it at `time` when it has keys.
pub fn write(scene: &mut Scene, reg: &Registry, a: &Address, v: Value, time: f64) -> WriteOutcome {
    if scene.keyed(a) {
        match scene.set_key(reg, a, time, v) {
            Ok(_) => WriteOutcome::Keyed,
            Err(_) => WriteOutcome::Failed,
        }
    } else {
        match scene.set_at(reg, a, v) {
            Ok(()) => WriteOutcome::Set,
            Err(_) => WriteOutcome::Failed,
        }
    }
}

/// The sound card's `enabled` switch.
fn enabled_at(id: ThingId) -> Address {
    Address::on(id, SlotId::Sound, "enabled")
}

/// Turns the shape's sound on: the starter patch the first time, else the card as it was left.
pub fn enable(scene: &mut Scene, reg: &Registry, id: ThingId) -> bool {
    if scene.card(id, SlotId::Sound).is_some_and(|c| c.kind == VOICE) {
        return scene.set_at(reg, &enabled_at(id), Value::Bool(true)).is_ok();
    }
    if scene.card(id, SlotId::Sound).is_some() {
        scene.remove_slot(id, SlotId::Sound);
    }
    if scene.add_slot(reg, id, SlotId::Sound, VOICE).is_err() {
        return false;
    }
    match scene.things.get_mut(&id) {
        Some(thing) => {
            thing.slots.insert(SlotId::Sound, starter_voice(reg));
            true
        }
        None => false,
    }
}

/// Turns the shape's sound off, keeping every setting.
pub fn disable(scene: &mut Scene, reg: &Registry, id: ThingId) -> bool {
    scene.card(id, SlotId::Sound).is_some() && scene.set_at(reg, &enabled_at(id), Value::Bool(false)).is_ok()
}

/// The shape's whole sound card, to paste on another shape.
pub fn copy(scene: &Scene, id: ThingId) -> Option<Slot> {
    scene.card(id, SlotId::Sound).filter(|c| c.kind == VOICE).cloned()
}

/// Puts a copied sound on `id` in place of its own (whose keys go with it). The layers get the
/// ids 0, 1, 2… in order and the routes and layered settings follow them, so the card is the
/// same sound whatever ids it was copied with.
pub fn paste(scene: &mut Scene, reg: &Registry, id: ThingId, copied: &Slot) -> bool {
    if copied.kind != VOICE || !scene.things.contains_key(&id) {
        return false;
    }
    let card = renumbered(copied);
    scene.remove_slot(id, SlotId::Sound);
    if scene.add_slot(reg, id, SlotId::Sound, VOICE).is_err() {
        return false;
    }
    match scene.things.get_mut(&id) {
        Some(thing) => {
            thing.slots.insert(SlotId::Sound, card);
            true
        }
        None => false,
    }
}

/// `card` with its layers numbered from 0 and everything that names a layer following.
fn renumbered(card: &Slot) -> Slot {
    let map: Vec<(LayerId, LayerId)> =
        card.layers.iter().enumerate().map(|(i, l)| (l.id, LayerId(i as u32))).collect();
    let new_id = |old: LayerId| map.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
    let mut out = card.clone();
    for layer in &mut out.layers {
        if let Some(n) = new_id(layer.id) {
            layer.id = n;
        }
        layer.parent = layer.parent.and_then(new_id);
    }
    let mut params = bpe_core::Params::default();
    for (key, value) in card.params.iter() {
        let value = match (key.as_str(), value) {
            (ROUTES_KEY, Value::Routes(routes)) => Value::Routes(
                routes
                    .iter()
                    .filter_map(|r| Some(bpe_core::ModRoute { source: new_id(r.source)?, ..r.clone() }))
                    .collect(),
            ),
            _ => value.clone(),
        };
        let key = match bpe_core::split_layered(key) {
            Some((layer, rest)) => match new_id(layer) {
                Some(n) => bpe_core::layered_key(n, rest),
                None => continue,
            },
            None => key.clone(),
        };
        params.set(&key, value);
    }
    out.params = params;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Layer, ModRoute, layered_key};

    #[test]
    fn renumbering_keeps_routes_and_layered_settings_on_their_layers() {
        let mut card = Slot::new(VOICE);
        let layer = |id: u32| Layer { id: LayerId(id), kind: bpe_kit::sound::ADSR.into(), ..Layer::default() };
        card.layers = vec![layer(7), layer(3)];
        card.params.set(&layered_key(LayerId(3), "attack"), Value::Number(40.0));
        card.params.set(
            ROUTES_KEY,
            Value::Routes(vec![ModRoute {
                source: LayerId(3),
                target: "a_gain".into(),
                amount: 0.5,
                bipolar: false,
                invert: false,
            }]),
        );
        let out = renumbered(&card);
        assert_eq!(out.layers.iter().map(|l| l.id.0).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(out.params.get(&layered_key(LayerId(1), "attack")), Some(&Value::Number(40.0)));
        match out.params.get(ROUTES_KEY) {
            Some(Value::Routes(r)) => assert_eq!(r[0].source, LayerId(1)),
            other => panic!("{other:?}"),
        }
    }
}
