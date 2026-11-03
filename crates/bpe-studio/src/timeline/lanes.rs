//! Which keyed and pulsing settings the timeline shows for a selection, and in what order.

use bpe_core::{Address, Scene, SlotId, ThingId};
use bpe_kit::Kit;
use std::collections::BTreeSet;

/// One row of the timeline: a setting's keys, or its pulse.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub address: Address,
    /// "Thing · Setting", or "Thing · Card · Setting" for a card's setting.
    pub label: String,
    /// A pulse lane rather than a track lane.
    pub pulse: bool,
    /// The setting moves the physics, so it can't pulse with hits.
    pub physics: bool,
}

/// The lanes for `selected`: its tracks then its pulses, then the scene settings' when
/// something else is selected, each setting at most once per kind of lane.
pub fn lanes_for(scene: &Scene, kit: &Kit, selected: Option<ThingId>) -> Vec<Lane> {
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
        let tracks: Vec<&Address> = scene.tracks.keys().collect();
        let pulses: Vec<&Address> = scene.pulses.keys().collect();
        for (pulse, addresses) in [(false, tracks), (true, pulses)] {
            for a in addresses.into_iter().filter(|a| a.thing == id) {
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
    let found = scene.things.get(&a.thing);
    let thing = found.map(|t| t.name.as_str()).unwrap_or("?");
    let setting = scene.def_at(&kit.registry, a).map(|d| d.label).unwrap_or(a.key.as_str());
    match a.slot {
        None => format!("{thing} · {setting}"),
        Some(slot) => {
            // The card's title as the inspector shows it ("Effects" for the Shader slot).
            let title = found.and_then(|t| kit.registry.thing(&t.kind)).map_or(slot.name(), |k| k.slot_label(slot));
            format!("{thing} · {title} · {setting}")
        }
    }
}

/// Whether the setting at `a` moves the physics: a Motion or Bounce setting, or one of a
/// thing's own that its sim says changes the run.
pub fn moves_physics(scene: &Scene, kit: &Kit, a: &Address) -> bool {
    match a.slot {
        None => scene
            .things
            .get(&a.thing)
            .and_then(|t| kit.sims.thing(&t.kind))
            .is_some_and(|sim| sim.affects_physics(&a.key)),
        Some(slot) => matches!(slot, SlotId::Motion | SlotId::Bounce),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;

    #[test]
    fn lanes_follow_the_selection_then_the_scene_settings() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let reg = &kit.registry;
        let size = Address::own(ThingId(2), "radius");
        let gravity = Address { thing: ThingId::ROOT, slot: None, key: "gravity".into() };
        let glow = Address::on(ThingId(2), SlotId::Look, "glow");
        let bloom = Address { thing: ThingId::ROOT, slot: None, key: "bloom".into() };
        scene.set_key(reg, &gravity, 1.0, Value::Vec2([0.0, 100.0])).unwrap();
        scene.set_key(reg, &size, 2.0, Value::Number(60.0)).unwrap();
        scene.set_key(reg, &glow, 0.0, Value::Number(0.5)).unwrap();
        scene.set_pulse(reg, &bloom, bpe_core::PulseSource::Hits, Value::Number(0.3), 0.25).unwrap();
        let labels =
            |selected| lanes_for(&scene, &kit, selected).into_iter().map(|l| (l.label, l.pulse)).collect::<Vec<_>>();
        // The ball's own key first, then its look card's, then the scene settings'.
        assert_eq!(
            labels(Some(ThingId(2))),
            [
                ("Ball · Size".to_owned(), false),
                ("Ball · Look · Glow".to_owned(), false),
                ("Scene settings · Gravity".to_owned(), false),
                ("Scene settings · Bloom".to_owned(), true),
            ]
        );
        assert_eq!(labels(None).len(), 2, "nothing selected shows the scene's lanes");
        assert_eq!(labels(Some(ThingId::ROOT)).len(), 2);
        let lanes = lanes_for(&scene, &kit, Some(ThingId(2)));
        assert!(
            lanes[0].physics && !lanes[1].physics && lanes[2].physics && !lanes[3].physics,
            "a ball's size and gravity move the physics; glow doesn't"
        );
    }
}
