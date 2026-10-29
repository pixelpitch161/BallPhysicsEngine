//! Which keyed and pulsing settings the timeline shows for a selection, and in what order.

use bpe_core::{Address, Owner, Resolved, Scene, SlotId, SlotLink, ThingId};
use bpe_kit::Kit;
use std::collections::BTreeSet;

/// One row of the timeline: a setting's keys, or its pulse.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub address: Address,
    /// "Thing or module · Setting".
    pub label: String,
    /// A pulse lane rather than a track lane.
    pub pulse: bool,
    /// The setting moves the physics, so it can't pulse with hits.
    pub physics: bool,
}

/// The owners whose settings belong to `id`: the thing itself (its own settings and its slot
/// overrides) and the modules its slots show, linked or inherited.
fn owners_of(scene: &Scene, resolved: &Resolved, id: ThingId) -> BTreeSet<Owner> {
    let mut owners = BTreeSet::from([Owner::Thing(id)]);
    if let Some(thing) = scene.things.get(&id) {
        for (slot, link) in &thing.slots {
            let module = match link {
                SlotLink::Linked { module, .. } => Some(*module),
                SlotLink::Inherit => resolved.things.get(&id).and_then(|r| r.slots.get(slot)).map(|s| s.module),
                SlotLink::Off => None,
            };
            if let Some(m) = module {
                owners.insert(Owner::Module(m));
            }
        }
    }
    owners
}

/// The lanes for `selected`: its tracks then its pulses, then the Scene defaults' when
/// something else is selected, each setting at most once per kind of lane.
pub fn lanes_for(scene: &Scene, kit: &Kit, resolved: &Resolved, selected: Option<ThingId>) -> Vec<Lane> {
    let mut lanes = Vec::new();
    let mut seen: BTreeSet<(Address, bool)> = BTreeSet::new();
    let mut ids = Vec::new();
    if let Some(id) = selected.filter(|id| scene.things.contains_key(id)) {
        ids.push(id);
    }
    if !ids.contains(&ThingId::ROOT) {
        ids.push(ThingId::ROOT);
    }
    for id in ids {
        let owners = owners_of(scene, resolved, id);
        let tracks: Vec<&Address> = scene.tracks.keys().collect();
        let pulses: Vec<&Address> = scene.pulses.keys().collect();
        for (pulse, addresses) in [(false, tracks), (true, pulses)] {
            for a in addresses.into_iter().filter(|a| owners.contains(&a.owner)) {
                if seen.insert((a.clone(), pulse)) {
                    lanes.push(Lane {
                        address: a.clone(),
                        label: label_of(scene, kit, a),
                        pulse,
                        physics: moves_physics(scene, kit, a),
                    });
                }
            }
        }
    }
    lanes
}

fn label_of(scene: &Scene, kit: &Kit, a: &Address) -> String {
    let owner = match a.owner {
        Owner::Thing(id) => scene.things.get(&id).map(|t| t.name.as_str()).unwrap_or("?"),
        Owner::Module(m) => scene.library.modules.get(&m).map(|m| m.name.as_str()).unwrap_or("?"),
    };
    let setting = scene.def_at(&kit.registry, a).map(|d| d.label).unwrap_or(a.key.as_str());
    format!("{owner} · {setting}")
}

/// Whether the setting at `a` moves the physics: a Motion or Bounce setting, or one of a
/// thing's own that its sim says changes the run.
pub fn moves_physics(scene: &Scene, kit: &Kit, a: &Address) -> bool {
    match (a.owner, a.slot) {
        (Owner::Thing(id), None) => {
            scene.things.get(&id).and_then(|t| kit.sims.thing(&t.kind)).is_some_and(|sim| sim.affects_physics(&a.key))
        }
        (Owner::Thing(_), Some(slot)) => matches!(slot, SlotId::Motion | SlotId::Bounce),
        (Owner::Module(m), _) => {
            scene.library.modules.get(&m).is_some_and(|m| matches!(m.slot, SlotId::Motion | SlotId::Bounce))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{ModuleId, Value, resolve};

    #[test]
    fn lanes_follow_the_selection_then_the_scene_defaults() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let reg = &kit.registry;
        let size = Address { owner: Owner::Thing(ThingId(2)), slot: None, key: "radius".into() };
        let gravity = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "gravity".into() };
        let ball_look = scene.library.modules.values().find(|m| m.name == "Ball look").unwrap().id;
        let glow = Address { owner: Owner::Module(ball_look), slot: None, key: "glow".into() };
        let plain_glow = Address { owner: Owner::Module(ModuleId(0)), slot: None, key: "glow".into() };
        let bloom = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "bloom".into() };
        scene.set_key(reg, &gravity, 1.0, Value::Vec2([0.0, 100.0])).unwrap();
        scene.set_key(reg, &size, 2.0, Value::Number(60.0)).unwrap();
        scene.set_key(reg, &glow, 0.0, Value::Number(0.5)).unwrap();
        scene.set_key(reg, &plain_glow, 0.0, Value::Number(0.7)).unwrap();
        scene.set_pulse(reg, &bloom, bpe_core::PulseSource::Beat, Value::Number(0.3), 0.25).unwrap();
        let resolved = resolve(&scene, reg);
        let labels = |selected| {
            lanes_for(&scene, &kit, &resolved, selected).into_iter().map(|l| (l.label, l.pulse)).collect::<Vec<_>>()
        };
        // The ball's own key first, then its look module's, then the scene's and the modules the
        // scene's slots show (the ball doesn't use the plain look, its own look stands in).
        assert_eq!(
            labels(Some(ThingId(2))),
            [
                ("Ball · Size".to_owned(), false),
                ("Ball look · Glow".to_owned(), false),
                ("Scene defaults · Gravity".to_owned(), false),
                ("Plain look · Glow".to_owned(), false),
                ("Scene defaults · Bloom".to_owned(), true),
            ]
        );
        assert_eq!(labels(None).len(), 3, "nothing selected shows the scene's lanes");
        assert_eq!(labels(Some(ThingId::ROOT)).len(), 3);
        let lanes = lanes_for(&scene, &kit, &resolved, Some(ThingId(2)));
        assert!(
            lanes[0].physics && !lanes[1].physics && lanes[2].physics && !lanes[3].physics,
            "a ball's size and gravity move the physics; glow doesn't"
        );
    }
}
