//! Which keyed and pulsing settings the timeline shows, grouped under their things, in what order.

use bpe_core::{Address, Scene, SlotId, ThingId};
use bpe_kit::Kit;
use std::collections::BTreeSet;

/// One row of the timeline: a setting's keys, or its pulse.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub address: Address,
    /// "Thing · Setting", or "Thing · Card · Setting" for a card's setting.
    pub label: String,
    /// The label without the thing's name, as the lane shows it under its group.
    pub short: String,
    /// A pulse lane rather than a track lane.
    pub pulse: bool,
    /// The setting moves the physics, so it can't pulse with hits.
    pub physics: bool,
}

/// One thing's lanes on the timeline, under a header with its name and track colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub thing: ThingId,
    pub name: String,
    pub lanes: Vec<Lane>,
}

/// A group for every thing with keys or pulses, in tree order (the scene settings first). Keys
/// on a thing that is gone are left out.
pub fn groups_for(scene: &Scene, kit: &Kit) -> Vec<Group> {
    scene
        .order
        .iter()
        .filter_map(|&id| {
            let thing = scene.things.get(&id)?;
            let lanes = lanes_for(scene, kit, id);
            (!lanes.is_empty()).then(|| Group { thing: id, name: thing.name.clone(), lanes })
        })
        .collect()
}

/// The lanes of one thing: its tracks then its pulses, each setting at most once per kind of
/// lane.
pub fn lanes_for(scene: &Scene, kit: &Kit, thing: ThingId) -> Vec<Lane> {
    let mut lanes = Vec::new();
    let mut seen: BTreeSet<(Address, bool)> = BTreeSet::new();
    let tracks: Vec<&Address> = scene.tracks.keys().collect();
    let pulses: Vec<&Address> = scene.pulses.keys().collect();
    for (pulse, addresses) in [(false, tracks), (true, pulses)] {
        for a in addresses.into_iter().filter(|a| a.thing == thing) {
            if seen.insert((a.clone(), pulse)) {
                let label = label_of(scene, kit, a);
                // The thing's name is on the group's header; the lane shows the rest.
                let short = label.split_once(" · ").map_or(label.clone(), |(_, rest)| rest.to_owned());
                lanes.push(Lane { address: a.clone(), label, short, pulse, physics: moves_physics(scene, kit, a) });
            }
        }
    }
    lanes
}

fn label_of(scene: &Scene, kit: &Kit, a: &Address) -> String {
    let found = scene.things.get(&a.thing);
    let thing = found.map(|t| t.name.as_str()).unwrap_or("?");
    let def = scene.def_at(&kit.registry, a);
    let setting = def.map(|d| d.label).unwrap_or(a.key.as_str());
    match a.slot {
        None => format!("{thing} · {setting}"),
        Some(slot) if let Some((layer, _)) = bpe_core::split_layered(&a.key) => {
            // A layer's setting goes by the layer's own name: "Ball · Outline · Width".
            let name = found.and_then(|t| t.slots.get(&slot)).and_then(|c| c.layer(layer)).map(|l| l.name.as_str());
            format!("{thing} · {} · {setting}", name.unwrap_or("?"))
        }
        // A sound setting goes by its part of the voice: "Shape · Osc A · Gain".
        Some(SlotId::Sound) if let Some(group) = def.map(|d| d.group).filter(|g| !g.is_empty()) => {
            format!("{thing} · {group} · {setting}")
        }
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
    fn groups_follow_the_tree_with_each_things_lanes() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let reg = &kit.registry;
        let size = Address::own(ThingId(2), "radius");
        let gravity = Address { thing: ThingId::ROOT, slot: None, key: "gravity".into() };
        // The test ball's Glow layer's Strength.
        let glow = Address::on(ThingId(2), SlotId::Look, "2/strength");
        let bloom = Address { thing: ThingId::ROOT, slot: None, key: "bloom".into() };
        scene.set_key(reg, &gravity, 1.0, Value::Vec2([0.0, 100.0])).unwrap();
        scene.set_key(reg, &size, 2.0, Value::Number(60.0)).unwrap();
        scene.set_key(reg, &glow, 0.0, Value::Number(0.5)).unwrap();
        scene.set_pulse(reg, &bloom, bpe_core::PulseSource::Hits, Value::Number(0.3), 0.25).unwrap();
        let groups = groups_for(&scene, &kit);
        let names: Vec<_> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Scene settings", "Ball"], "tree order, the arena has no keys");
        let labels = |g: &Group| g.lanes.iter().map(|l| (l.short.clone(), l.pulse)).collect::<Vec<_>>();
        assert_eq!(labels(&groups[0]), [("Gravity".to_owned(), false), ("Bloom".to_owned(), true)]);
        assert_eq!(labels(&groups[1]), [("Size".to_owned(), false), ("Glow · Strength".to_owned(), false)]);
        assert_eq!(groups[1].lanes[0].label, "Ball · Size");
        let ball = &groups[1].lanes;
        assert!(ball[0].physics && !ball[1].physics, "a ball's size moves the physics; glow doesn't");
        assert!(groups[0].lanes[0].physics && !groups[0].lanes[1].physics);
    }

    #[test]
    fn groups_skip_missing_things() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let gone = Address::own(ThingId(99), "radius");
        scene.tracks.insert(gone, bpe_core::Track::default());
        assert!(groups_for(&scene, &kit).is_empty());
    }

    #[test]
    fn a_sound_key_shows_in_the_shapes_timeline_group() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_scene(reg);
        let shape = bpe_kit::starters::add_thing(&mut scene, reg, bpe_kit::things::shape::KIND, ThingId::ROOT).unwrap();
        scene.add_slot(reg, shape, SlotId::Sound, bpe_kit::sound::VOICE).unwrap();
        scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, bpe_kit::sound::starter_voice(reg));
        let gain = Address::on(shape, SlotId::Sound, "a_gain");
        let attack = Address::on(shape, SlotId::Sound, &bpe_core::layered_key(bpe_core::LayerId(0), "attack"));
        scene.set_key(reg, &gain, 1.0, Value::Number(-6.0)).unwrap();
        scene.set_key(reg, &attack, 1.0, Value::Number(20.0)).unwrap();
        let groups = groups_for(&scene, &kit);
        let group = groups.iter().find(|g| g.thing == shape).expect("the shape has a group");
        let shorts: Vec<_> = group.lanes.iter().map(|l| l.short.as_str()).collect();
        assert_eq!(shorts, ["ADSR 1 · Attack", "Osc A · Gain"], "Osc A and Osc B both have a Gain");
        assert!(group.lanes.iter().all(|l| !l.physics), "sound never moves the physics");
    }
}
