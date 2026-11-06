//! Scenes from before the Effects stack: the scene's one Shader card of a built-in effect
//! (`shader.<file>`) becomes an Effects card holding that effect as its one layer, its Blend
//! becoming the layer's Mix. A `shader.*` card anywhere else is left out (things have Shader
//! layers in their Look instead).

use bpe_core::{Address, Layer, LayerId, Params, Scene, Slot, SlotId, ThingId, layered_key, split_layered};

use super::{EFFECTS, MIX, builtin_of, id_of};

/// The old one-effect card kind's prefix.
const OLD: &str = "shader.";

/// The Effects card an old one-effect card of `kind` with `params` becomes; `None` for a kind
/// that isn't an old effect card.
pub fn stack_of(kind: &str, params: &Params) -> Option<Slot> {
    let file = kind.strip_prefix(OLD)?;
    let builtin = builtin_of(&format!("picture.{file}"))?;
    let id = LayerId(0);
    let mut card = Slot::new(EFFECTS);
    card.layers.push(Layer {
        id,
        kind: id_of(builtin).into(),
        name: builtin.header().name.clone(),
        ..Layer::default()
    });
    for (k, v) in params.iter() {
        let key = if k == "blend" { MIX } else { k.as_str() };
        card.params.set(&layered_key(id, key), v.clone());
    }
    Some(card)
}

/// Whether `kind` is an old one-effect card.
pub fn is_old(kind: &str) -> bool {
    kind.starts_with(OLD)
}

/// Turns every old one-effect card in `scene` into the scene's Effects stack (or leaves it out,
/// off Scene settings), its keyframes and pulses going with its settings onto the layer (or
/// with it). Nothing happens to a scene without one.
pub fn single_effect_to_stack(scene: &mut Scene) {
    let mut moved = Vec::new();
    for (id, thing) in scene.things.iter_mut() {
        let Some(card) = thing.slots.get(&SlotId::Shader) else { continue };
        if !is_old(&card.kind) {
            continue;
        }
        match (*id == ThingId::ROOT).then(|| stack_of(&card.kind, &card.params)).flatten() {
            Some(stack) => {
                thing.slots.insert(SlotId::Shader, stack);
                moved.push((*id, true));
            }
            None => {
                thing.slots.remove(&SlotId::Shader);
                moved.push((*id, false));
            }
        }
    }
    for (id, kept) in moved {
        let on = |a: &Address| a.thing == id && a.slot == Some(SlotId::Shader) && split_layered(&a.key).is_none();
        let to = |a: &Address| {
            let key = if a.key == "blend" { MIX } else { a.key.as_str() };
            Address::on(id, SlotId::Shader, &layered_key(LayerId(0), key))
        };
        let tracks: Vec<Address> = scene.tracks.keys().filter(|a| on(a)).cloned().collect();
        for a in tracks {
            let track = scene.tracks.remove(&a).expect("listed");
            if kept {
                scene.tracks.insert(to(&a), track);
            }
        }
        let pulses: Vec<Address> = scene.pulses.keys().filter(|a| on(a)).cloned().collect();
        for a in pulses {
            let pulse = scene.pulses.remove(&a).expect("listed");
            if kept {
                scene.pulses.insert(to(&a), pulse);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;

    #[test]
    fn an_old_kaleidoscope_card_becomes_a_one_layer_stack() {
        let reg = crate::registry();
        let mut scene = crate::new_studio_scene(&reg);
        let mut old = Slot::new("shader.kaleidoscope");
        old.params.set("blend", Value::Number(0.5));
        old.params.set("slices", Value::Int(9));
        scene.things.get_mut(&ThingId::ROOT).unwrap().slots.insert(SlotId::Shader, old);
        let text = bpe_core::save(&scene);
        let loaded = bpe_core::load(&text, &reg).unwrap();
        let card = &loaded.things[&ThingId::ROOT].slots[&SlotId::Shader];
        assert_eq!(card.kind, EFFECTS);
        assert_eq!(card.layers.len(), 1);
        assert_eq!(
            (card.layers[0].kind.as_str(), card.layers[0].name.as_str()),
            ("picture.kaleidoscope", "Kaleidoscope")
        );
        let id = card.layers[0].id;
        assert_eq!(card.params.get(&layered_key(id, MIX)), Some(&Value::Number(0.5)));
        assert_eq!(card.params.get(&layered_key(id, "slices")), Some(&Value::Int(9)));
        // A stack opens as it is.
        let again = bpe_core::load(&bpe_core::save(&loaded), &reg).unwrap();
        assert_eq!(again.things[&ThingId::ROOT].slots[&SlotId::Shader], *card);
    }

    #[test]
    fn an_old_cards_keyframes_move_onto_its_layer() {
        let reg = crate::registry();
        let mut scene = crate::new_studio_scene(&reg);
        scene.tracks.clear();
        let mut old = Slot::new("shader.kaleidoscope");
        old.params.set("blend", Value::Number(0.5));
        scene.things.get_mut(&ThingId::ROOT).unwrap().slots.insert(SlotId::Shader, old);
        let mut track = bpe_core::Track::default();
        track.set_key(0.0, Value::Number(0.0));
        track.set_key(1.0, Value::Number(1.0));
        scene.tracks.insert(Address::on(ThingId::ROOT, SlotId::Shader, "blend"), track.clone());
        scene.tracks.insert(Address::on(ThingId::ROOT, SlotId::Shader, "spin"), track.clone());
        single_effect_to_stack(&mut scene);
        let on = |key: &str| Address::on(ThingId::ROOT, SlotId::Shader, &layered_key(LayerId(0), key));
        assert_eq!(scene.tracks.get(&on(MIX)), Some(&track), "Blend's keys become Mix's");
        assert_eq!(scene.tracks.get(&on("spin")), Some(&track));
        assert_eq!(scene.tracks.len(), 2, "none left behind on the old keys");
    }
}
