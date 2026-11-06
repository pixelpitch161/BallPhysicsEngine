//! What a new thing starts with: its own card for each slot its kind shows straight away, made for
//! its kind. Nothing is shared or passed down a folder.

use bpe_core::{EditError, Registry, Scene, SlotId, ThingId};

/// The card kind a new thing of `thing` starts with in `slot`. `None` means the first card kind
/// for the slot that fits the thing.
fn starter_kind(thing: &str, slot: SlotId) -> Option<&'static str> {
    Some(match slot {
        SlotId::Look if crate::things::balls::rows::has_balls(thing) => crate::modules::look::layers::CARD,
        SlotId::Look if thing == crate::things::shape::KIND => crate::modules::look::shape::CARD,
        SlotId::Look if thing == crate::things::particles::KIND => crate::things::particles::look::CARD,
        SlotId::Motion if thing == crate::things::particles::KIND => crate::things::particles::forces::CARD,
        SlotId::Bounce if thing == crate::things::particles::KIND => crate::things::particles::meets::CARD,
        SlotId::Motion => "motion.basic",
        SlotId::Bounce => "bounce.basic",
        _ => return None,
    })
}

/// Gives `id` its starting card in `slot`, every setting at its default. A card already there is
/// replaced.
pub fn fill_slot(scene: &mut Scene, reg: &Registry, id: ThingId, slot: SlotId) -> Result<(), EditError> {
    let thing = scene.things.get(&id).ok_or(EditError::Missing(id))?;
    let kind = reg.thing(&thing.kind).ok_or_else(|| EditError::UnknownKind(thing.kind.clone()))?;
    let fits = |k: &str| reg.module(k).is_some_and(|m| m.slot() == slot && m.fits(kind.id()));
    let card = match starter_kind(kind.id(), slot).filter(|k| fits(k)) {
        Some(found) => found,
        None => reg.kinds_fitting(slot, kind).next().ok_or(EditError::NotAccepted)?.id(),
    };
    scene.add_slot(reg, id, slot, card)?;
    // A layered card starts with its base layer alone: a ball is one plain white circle.
    if let Some(base) = reg.layers_for(card).find(|k| k.base()) {
        scene.add_layer(reg, id, slot, base.id())?;
    }
    Ok(())
}

/// Adds a thing of `kind` under `parent` with its own starting card for each slot its kind shows
/// straight away.
pub fn add_thing(scene: &mut Scene, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError> {
    let id = scene.add_thing(reg, kind, parent)?;
    let slots = reg.thing(kind).expect("add_thing checked the kind").default_slots();
    for &slot in slots {
        fill_slot(scene, reg, id, slot)?;
    }
    // The first light (or rig) lights the scene: it gets its Lighting card, so the light shows.
    let lit = scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Lighting));
    if (kind == crate::things::light::LIGHT || kind == crate::things::rig::RIG) && !lit {
        scene.add_slot(reg, ThingId::ROOT, SlotId::Lighting, crate::modules::lighting::LIGHTING)?;
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;
    use bpe_core::{Address, Value, resolve};

    #[test]
    fn each_kind_starts_with_its_own_cards() {
        let reg = registry();
        let table: &[(&str, &[(SlotId, &str)])] = &[
            (
                "balls",
                &[(SlotId::Look, "look.layers"), (SlotId::Motion, "motion.basic"), (SlotId::Bounce, "bounce.basic")],
            ),
            ("shape", &[(SlotId::Look, "look.shape"), (SlotId::Bounce, "bounce.basic")]),
        ];
        for &(kind, slots) in table {
            let mut scene = Scene::empty(&reg);
            let a = add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
            assert_eq!(scene.things[&a].slots.len(), slots.len(), "{kind}");
            for &(slot, card) in slots {
                let got = &scene.things[&a].slots[&slot];
                assert_eq!((got.kind.as_str(), got.params.is_empty()), (card, true), "{kind} {slot:?}");
            }
            let r = resolve(&scene, &reg);
            assert!(r.problems.is_empty(), "{kind}: {:?}", r.problems);
        }
    }

    #[test]
    fn a_new_ball_is_one_plain_white_circle() {
        use crate::modules::look::layers::{BODY, BodyLayer};
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        let a = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let card = &scene.things[&a].slots[&SlotId::Look];
        let kinds: Vec<&str> = card.layers.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, [BODY], "a Body and nothing else: no shine, outline, glow, flash or reaction");
        assert_eq!(card.layers[0].name, "Body");
        let look = resolve(&scene, &reg).things[&a].slots[&SlotId::Look].clone();
        let body = BodyLayer::from_params(&look.layer_params(card.layers[0].id));
        use bpe_core::Settings;
        assert_eq!(body, BodyLayer::default());
        assert_eq!(body.ball_shape, crate::BallShape::Circle);
        assert_eq!(body.fill.colours, bpe_core::Gradient::solid(bpe_core::Color::WHITE));
    }

    #[test]
    fn a_new_shape_is_a_plain_white_hollow_circle_with_one_body_layer() {
        use crate::{ShapeFill, ShapeSettings, ShapeStart};
        use bpe_core::Settings;
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        let a = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
        let card = &scene.things[&a].slots[&SlotId::Look];
        let kinds: Vec<&str> = card.layers.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, [crate::modules::look::shape::BODY]);
        assert_eq!(card.layers[0].name, "Body");
        let resolved = resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let s = ShapeSettings::from_params(&resolved.things[&a].params);
        assert_eq!(
            (s.start, s.fill, s.size, s.thickness, s.spin, s.shrink),
            (ShapeStart::Circle, ShapeFill::Hollow, 400.0, 8.0, 0.0, 0.0)
        );
        assert!(scene.things[&a].colour.is_some());
    }

    #[test]
    fn cameras_and_folders_start_empty() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        for kind in ["camera", "folder"] {
            let id = add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
            assert!(scene.things[&id].slots.is_empty(), "{kind}");
        }
    }

    #[test]
    fn a_thing_in_a_folder_keeps_its_own_cards() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        let folder = add_thing(&mut scene, &reg, "folder", ThingId::ROOT).unwrap();
        let a = add_thing(&mut scene, &reg, "balls", folder).unwrap();
        let b = add_thing(&mut scene, &reg, "balls", folder).unwrap();
        let roundness = |t| Address::on(t, SlotId::Look, "0/roundness");
        scene.set_at(&reg, &roundness(a), Value::Number(1.0)).unwrap();
        scene.set_parent(a, ThingId::ROOT, bpe_core::Attach::Group).unwrap();
        assert_eq!(scene.value_at(&reg, &roundness(a)), Some(Value::Number(1.0)), "moving keeps a thing's settings");
        assert_eq!(
            scene.value_at(&reg, &roundness(b)),
            Some(Value::Number(0.15f32 as f64)),
            "and never touches another's"
        );
    }

    #[test]
    fn effects_have_no_starter_so_the_first_fitting_kind_fills_the_slot() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        fill_slot(&mut scene, &reg, ThingId::ROOT, SlotId::Shader).unwrap();
        let first = reg.kinds_fitting(SlotId::Shader, reg.thing("scene").unwrap()).next().unwrap().id();
        assert_eq!(scene.things[&ThingId::ROOT].slots[&SlotId::Shader].kind, first);
    }
}
