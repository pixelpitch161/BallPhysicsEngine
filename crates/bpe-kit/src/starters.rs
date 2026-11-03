//! What a new thing starts with: its own card for each slot its kind shows straight away, made for
//! its kind. Nothing is shared or passed down a folder.

use bpe_core::{EditError, Registry, Scene, SlotId, ThingId};

/// The card kind a new thing starts with in `slot`. `None` means the first card kind for the
/// slot that fits the thing.
fn starter_kind(slot: SlotId) -> Option<&'static str> {
    Some(match slot {
        SlotId::Look => "look.basic",
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
    let card = match starter_kind(slot).filter(|k| fits(k)) {
        Some(found) => found,
        None => reg.kinds_fitting(slot, kind).next().ok_or(EditError::NotAccepted)?.id(),
    };
    scene.add_slot(reg, id, slot, card)
}

/// Adds a thing of `kind` under `parent` with its own starting card for each slot its kind shows
/// straight away.
pub fn add_thing(scene: &mut Scene, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError> {
    let id = scene.add_thing(reg, kind, parent)?;
    let slots = reg.thing(kind).expect("add_thing checked the kind").default_slots();
    for &slot in slots {
        fill_slot(scene, reg, id, slot)?;
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
                &[(SlotId::Look, "look.basic"), (SlotId::Motion, "motion.basic"), (SlotId::Bounce, "bounce.basic")],
            ),
            ("arena", &[(SlotId::Look, "look.basic"), (SlotId::Bounce, "bounce.basic")]),
        ];
        for &(kind, slots) in table {
            let mut scene = Scene::empty(&reg);
            let a = add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
            assert_eq!(scene.things[&a].slots.len(), slots.len(), "{kind}");
            for &(slot, card) in slots {
                assert_eq!(scene.things[&a].slots[&slot], bpe_core::Slot::new(card), "{kind} {slot:?}");
            }
            let r = resolve(&scene, &reg);
            assert!(r.problems.is_empty(), "{kind}: {:?}", r.problems);
        }
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
        let glow = |t| Address::on(t, SlotId::Look, "glow");
        scene.set_at(&reg, &glow(a), Value::Number(1.0)).unwrap();
        scene.set_parent(a, ThingId::ROOT, bpe_core::Attach::Group).unwrap();
        assert_eq!(scene.value_at(&reg, &glow(a)), Some(Value::Number(1.0)), "moving keeps a thing's settings");
        assert_eq!(scene.value_at(&reg, &glow(b)), Some(Value::Number(0.0)), "and never touches another's");
    }

    #[test]
    fn effects_have_no_starter_so_the_first_fitting_kind_fills_the_slot() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        let a = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        fill_slot(&mut scene, &reg, a, SlotId::Shader).unwrap();
        let first = reg.kinds_fitting(SlotId::Shader, reg.thing("balls").unwrap()).next().unwrap().id();
        assert_eq!(scene.things[&a].slots[&SlotId::Shader].kind, first);
    }
}
