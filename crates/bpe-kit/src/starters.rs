//! What a new thing starts with: its own modules, made for its kind, unless a folder above it
//! already fills the slot, in which case it follows the folder. Moving things, or changing a
//! folder's modules, never changes what a following thing looks or sounds like: it gets its
//! own copy of what it used before.

use bpe_core::{
    Attach, EditError, InstrumentRef, InstrumentSource, Progression, ProgressionRef, Registry, Resolved, Scene,
    SettingValue, SlotId, SlotLink, ThingId, Value, resolve, resolve_slot,
};

/// Settings a starter changes from its module kind's defaults.
type Tweaks = &'static [(&'static str, Value)];

/// The module kind a new thing of `thing_kind` starts with in `slot`, and what it changes from
/// that kind's defaults. `None` means the first module kind for the slot that the thing takes.
fn starter_kind(thing_kind: &str, slot: SlotId) -> Option<(&'static str, Tweaks)> {
    const SOFT_LANDING: Tweaks = &[("bounciness", Value::Number(0.0))];
    Some(match (thing_kind, slot) {
        ("liquid", SlotId::Look) => ("look.liquid", &[]),
        ("liquid", SlotId::Motion) => ("motion.liquid", &[]),
        ("liquid", SlotId::Bounce) => ("bounce.basic", SOFT_LANDING),
        ("swarm", SlotId::Look) => ("look.swarm", &[]),
        ("swarm", SlotId::Motion) => ("motion.flock", &[]),
        (_, SlotId::Look) => ("look.classic", &[]),
        (_, SlotId::Motion) => ("motion.basic", &[]),
        (_, SlotId::Bounce) => ("bounce.basic", &[]),
        (_, SlotId::Sound) => ("sound.notes", &[]),
        _ => return None,
    })
}

/// The scene's library Pluck, added the first time something needs it.
pub(crate) fn shared_pluck(scene: &mut Scene) -> InstrumentRef {
    let pluck = bpe_core::Choice::name(bpe_mix::Instrument::Pluck);
    let found = scene
        .library
        .instruments
        .iter()
        .find(|(_, i)| matches!(&i.source, InstrumentSource::BuiltIn { which, .. } if which == pluck));
    if let Some((&id, _)) = found {
        return InstrumentRef(Some(id));
    }
    InstrumentRef(Some(scene.add_instrument(crate::built_in_instrument(bpe_mix::Instrument::Pluck))))
}

/// The scene's library starter progression, added the first time something needs it.
pub(crate) fn shared_progression(scene: &mut Scene) -> ProgressionRef {
    let name = Progression::starter().name;
    if let Some((&id, _)) = scene.library.progressions.iter().find(|(_, p)| p.name == name) {
        return ProgressionRef(Some(id));
    }
    ProgressionRef(Some(scene.add_progression(Progression::starter())))
}

/// Fills `slot` on `id`: follows a folder above that fills it, or else makes the thing its own
/// starter module for the slot.
pub fn fill_slot(scene: &mut Scene, reg: &Registry, id: ThingId, slot: SlotId) -> Result<(), EditError> {
    let thing = scene.things.get(&id).ok_or(EditError::Missing(id))?;
    let kind = reg.thing(&thing.kind).ok_or_else(|| EditError::UnknownKind(thing.kind.clone()))?;
    if !kind.slots().contains(&slot) {
        return Err(EditError::SlotNotSupported(slot));
    }
    let previous = scene.things[&id].slots.get(&slot).cloned();
    scene.set_slot_inherit(id, slot);
    if resolve_slot(scene, reg, id, slot, &mut Vec::new()).is_some() {
        return Ok(());
    }
    let (module_kind, tweaks) = match starter_kind(kind.id(), slot).filter(|(k, _)| kind.accepts(slot, k)) {
        Some(found) => found,
        None => match reg.modules_for(slot).find(|m| kind.accepts(slot, m.id())) {
            Some(m) => (m.id(), &[][..]),
            None => {
                match previous {
                    Some(link) => scene.things.get_mut(&id).expect("checked").slots.insert(slot, link),
                    None => scene.things.get_mut(&id).expect("checked").slots.remove(&slot),
                };
                return Err(EditError::NotAccepted);
            }
        },
    };
    let m = scene.add_starter(reg, id, module_kind)?;
    let mut params = tweaks.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<Vec<_>>();
    if module_kind == "sound.notes" {
        params.push(("instrument".into(), shared_pluck(scene).to_value()));
        params.push(("chords".into(), shared_progression(scene).to_value()));
    }
    let module = scene.library.modules.get_mut(&m).expect("just added");
    for (key, value) in params {
        module.params.set(&key, value);
    }
    Ok(())
}

/// Adds a thing of `kind` under `parent` with its starters: its own module for each slot its
/// kind shows straight away, or the folder's where a folder above fills that slot.
pub fn add_thing(scene: &mut Scene, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError> {
    let id = scene.add_thing(reg, kind, parent)?;
    let slots = reg.thing(kind).expect("add_thing checked the kind").default_slots();
    for &slot in slots {
        fill_slot(scene, reg, id, slot)?;
    }
    Ok(id)
}

/// After an edit, gives every thing that follows a folder its own copy of the module it used
/// in `before` wherever it would now see something else (another module's settings, or
/// nothing). Things are checked parents first, so a folder that gets its copy keeps its
/// followers' view the same.
pub fn keep_followers(scene: &mut Scene, reg: &Registry, before: &Resolved) {
    let following: Vec<(ThingId, SlotId)> = scene
        .order
        .iter()
        .flat_map(|id| {
            let thing = &scene.things[id];
            thing.slots.iter().filter(|(_, l)| matches!(l, SlotLink::Inherit)).map(move |(s, _)| (*id, *s))
        })
        .collect();
    for (id, slot) in following {
        let Some(was) = before.things.get(&id).and_then(|t| t.slots.get(&slot)) else { continue };
        if !scene.library.modules.contains_key(&was.module) {
            continue;
        }
        let now = resolve_slot(scene, reg, id, slot, &mut Vec::new());
        if now.is_some_and(|n| n.kind == was.kind && n.params == was.params) {
            continue;
        }
        let _ = scene.own_copy(reg, id, slot, was.module);
    }
}

/// Moves `id` (and what hangs under it) to `parent`, keeping what every follower uses.
pub fn move_thing(
    scene: &mut Scene,
    reg: &Registry,
    id: ThingId,
    parent: ThingId,
    attach: Attach,
) -> Result<(), EditError> {
    let before = resolve(scene, reg);
    scene.set_parent(id, parent, attach)?;
    keep_followers(scene, reg, &before);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{new_scene, registry};
    use bpe_core::{Color, Gradient, ModuleId, Value, resolve};

    fn linked(scene: &Scene, id: ThingId, slot: SlotId) -> ModuleId {
        match scene.things[&id].slots.get(&slot) {
            Some(SlotLink::Linked { module, .. }) => *module,
            other => panic!("{slot:?} on {id:?} is {other:?}"),
        }
    }

    fn effective(scene: &Scene, reg: &Registry, id: ThingId, slot: SlotId) -> Option<bpe_core::Params> {
        resolve_slot(scene, reg, id, slot, &mut Vec::new()).map(|r| r.params)
    }

    #[test]
    fn each_kind_starts_with_its_own_modules() {
        let reg = registry();
        let table: &[(&str, &[(SlotId, &str)])] = &[
            (
                "balls",
                &[
                    (SlotId::Look, "look.classic"),
                    (SlotId::Motion, "motion.basic"),
                    (SlotId::Bounce, "bounce.basic"),
                    (SlotId::Sound, "sound.notes"),
                ],
            ),
            (
                "soft",
                &[
                    (SlotId::Look, "look.classic"),
                    (SlotId::Motion, "motion.basic"),
                    (SlotId::Bounce, "bounce.basic"),
                    (SlotId::Sound, "sound.notes"),
                ],
            ),
            (
                "arena",
                &[(SlotId::Look, "look.classic"), (SlotId::Bounce, "bounce.basic"), (SlotId::Sound, "sound.notes")],
            ),
            (
                "obstacle",
                &[(SlotId::Look, "look.classic"), (SlotId::Bounce, "bounce.basic"), (SlotId::Sound, "sound.notes")],
            ),
            (
                "liquid",
                &[(SlotId::Look, "look.liquid"), (SlotId::Motion, "motion.liquid"), (SlotId::Bounce, "bounce.basic")],
            ),
            ("swarm", &[(SlotId::Look, "look.swarm"), (SlotId::Motion, "motion.flock")]),
            ("text", &[(SlotId::Look, "look.classic")]),
        ];
        for &(kind, slots) in table {
            let mut scene = Scene::empty(&reg);
            let a = add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
            let b = add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
            assert_eq!(scene.things[&a].slots.len(), slots.len(), "{kind}");
            for &(slot, module_kind) in slots {
                for id in [a, b] {
                    let m = &scene.library.modules[&linked(&scene, id, slot)];
                    let name = bpe_core::starter_name(&scene.things[&id].name, slot);
                    assert_eq!((m.kind.as_str(), m.owner, m.name.as_str()), (module_kind, Some(id), name.as_str()));
                }
                assert_ne!(linked(&scene, a, slot), linked(&scene, b, slot), "{kind} {slot:?}");
            }
            let r = resolve(&scene, &reg);
            assert!(r.problems.is_empty(), "{kind}: {:?}", r.problems);
        }
    }

    #[test]
    fn a_liquid_gets_its_own_surface_flow_and_soft_landing() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        let a = add_thing(&mut scene, &reg, "liquid", ThingId::ROOT).unwrap();
        let bounce = linked(&scene, a, SlotId::Bounce);
        assert_eq!(scene.library.modules[&bounce].params.get("bounciness"), Some(&Value::Number(0.0)));
    }

    #[test]
    fn emitters_fields_cameras_and_folders_start_empty() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        for kind in ["emitter", "field", "camera", "folder"] {
            let id = add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
            assert!(scene.things[&id].slots.is_empty(), "{kind}");
        }
        assert!(scene.library.modules.is_empty());
    }

    #[test]
    fn sounds_share_one_pluck_and_one_starter_progression() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        let a = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let b = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        assert_eq!(scene.library.instruments.len(), 1);
        assert_eq!(scene.library.progressions.len(), 1);
        let (&pluck, i) = scene.library.instruments.iter().next().unwrap();
        assert_eq!(i.name, "Pluck");
        let (&chords, p) = scene.library.progressions.iter().next().unwrap();
        assert_eq!(p.name, "Starter");
        for id in [a, b] {
            let params = &scene.library.modules[&linked(&scene, id, SlotId::Sound)].params;
            assert_eq!(params.get("instrument"), Some(&Value::Int(pluck.0 as i64)));
            assert_eq!(params.get("chords"), Some(&Value::Int(chords.0 as i64)));
        }
    }

    #[test]
    fn a_thing_added_in_a_filled_folder_follows_it() {
        let reg = registry();
        let mut scene = Scene::empty(&reg);
        let folder = add_thing(&mut scene, &reg, "folder", ThingId::ROOT).unwrap();
        fill_slot(&mut scene, &reg, folder, SlotId::Look).unwrap();
        let look = linked(&scene, folder, SlotId::Look);
        assert_eq!(scene.library.modules[&look].owner, Some(folder));
        let balls = add_thing(&mut scene, &reg, "balls", folder).unwrap();
        assert_eq!(scene.things[&balls].slots[&SlotId::Look], SlotLink::Inherit);
        linked(&scene, balls, SlotId::Motion);
        assert_eq!(resolve(&scene, &reg).things[&balls].slots[&SlotId::Look].module, look);
    }

    /// A folder with a look whose glow is 2, holding balls that follow it.
    fn folder_with_follower(reg: &Registry) -> (Scene, ThingId, ThingId, ModuleId) {
        let mut scene = Scene::empty(reg);
        let folder = add_thing(&mut scene, reg, "folder", ThingId::ROOT).unwrap();
        fill_slot(&mut scene, reg, folder, SlotId::Look).unwrap();
        let look = linked(&scene, folder, SlotId::Look);
        scene.library.modules.get_mut(&look).unwrap().params.set("glow", Value::Number(2.0));
        let balls = add_thing(&mut scene, reg, "balls", folder).unwrap();
        (scene, folder, balls, look)
    }

    #[test]
    fn moving_out_of_a_folder_keeps_the_module_through_a_copy() {
        let reg = registry();
        let (mut scene, _, balls, look) = folder_with_follower(&reg);
        let before = effective(&scene, &reg, balls, SlotId::Look);
        move_thing(&mut scene, &reg, balls, ThingId::ROOT, Attach::Group).unwrap();
        let own = linked(&scene, balls, SlotId::Look);
        assert_ne!(own, look);
        assert_eq!(scene.library.modules[&own].owner, Some(balls));
        assert_eq!(effective(&scene, &reg, balls, SlotId::Look), before);
    }

    #[test]
    fn moving_within_the_same_folder_changes_nothing() {
        let reg = registry();
        let (mut scene, folder, balls, _) = folder_with_follower(&reg);
        let modules = scene.library.modules.len();
        move_thing(&mut scene, &reg, balls, folder, Attach::Group).unwrap();
        assert_eq!(scene.things[&balls].slots[&SlotId::Look], SlotLink::Inherit);
        assert_eq!(scene.library.modules.len(), modules);
    }

    #[test]
    fn moving_a_folder_keeps_its_followers_looks() {
        let reg = registry();
        let (mut scene, folder, balls, _) = folder_with_follower(&reg);
        let other = add_thing(&mut scene, &reg, "folder", ThingId::ROOT).unwrap();
        fill_slot(&mut scene, &reg, other, SlotId::Look).unwrap();
        let theirs = linked(&scene, other, SlotId::Look);
        scene.library.modules.get_mut(&theirs).unwrap().params.set("glow", Value::Number(0.25));
        // The folder's own look comes with it, so its followers still see it.
        let before = effective(&scene, &reg, balls, SlotId::Look);
        move_thing(&mut scene, &reg, folder, other, Attach::Group).unwrap();
        assert_eq!(effective(&scene, &reg, balls, SlotId::Look), before);
        // A follower two folders down whose middle folder follows too: moving the outer folder
        // out from under the look leaves the follower its own copy.
        let inner = add_thing(&mut scene, &reg, "folder", folder).unwrap();
        let deep = add_thing(&mut scene, &reg, "balls", inner).unwrap();
        scene.set_slot_inherit(folder, SlotId::Look);
        let before = effective(&scene, &reg, deep, SlotId::Look);
        assert_eq!(before.as_ref().and_then(|p| p.get("glow")), Some(&Value::Number(0.25)));
        move_thing(&mut scene, &reg, folder, ThingId::ROOT, Attach::Group).unwrap();
        assert_eq!(effective(&scene, &reg, deep, SlotId::Look), before);
        assert_eq!(effective(&scene, &reg, balls, SlotId::Look), before);
    }

    #[test]
    fn turning_a_folder_slot_off_keeps_followers() {
        let reg = registry();
        let (mut scene, folder, balls, look) = folder_with_follower(&reg);
        let before_all = resolve(&scene, &reg);
        let before = effective(&scene, &reg, balls, SlotId::Look);
        scene.set_slot_off(folder, SlotId::Look);
        keep_followers(&mut scene, &reg, &before_all);
        let own = linked(&scene, balls, SlotId::Look);
        assert_ne!(own, look);
        assert_eq!(effective(&scene, &reg, balls, SlotId::Look), before);
    }

    #[test]
    fn a_new_scene_has_scene_settings_an_arena_and_a_pink_ball() {
        let reg = registry();
        let s = new_scene(&reg);
        assert_eq!(s.name, "New scene");
        let kinds: Vec<&str> = s.order.iter().map(|id| s.things[id].kind.as_str()).collect();
        assert_eq!(kinds, ["scene", "arena", "balls"]);
        assert!(s.things[&ThingId::ROOT].slots.is_empty());
        let (arena, ball) = (ThingId(1), ThingId(2));
        assert_eq!(s.things[&ball].name, "Ball");
        assert_eq!(s.things[&ball].transform.position, glam::Vec2::new(0.0, -200.0));
        let names = |id: ThingId| -> Vec<String> {
            s.things[&id].slots.keys().map(|slot| s.library.modules[&linked(&s, id, *slot)].name.clone()).collect()
        };
        assert_eq!(names(arena), ["Arena look", "Arena bounce", "Arena sound"]);
        assert_eq!(names(ball), ["Ball look", "Ball motion", "Ball bounce", "Ball sound"]);
        assert_eq!(s.library.modules.len(), 7);
        let res = resolve(&s, &reg);
        assert!(res.problems.is_empty(), "{:?}", res.problems);
        let pink = Gradient::solid(Color::rgb(0xff, 0x5c, 0xa8));
        assert_eq!(res.things[&ball].slots[&SlotId::Look].params.get("colours"), Some(&Value::Gradient(pink)));
    }
}
