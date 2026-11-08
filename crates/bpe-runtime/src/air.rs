//! The scene's air as the physics feels it (W13, "one wind"): balls whose Motion card follows the
//! air are carried by it. The air itself (a soft fluid the balls stir, shapes block and heat lifts)
//! is the kit's, kept in the world's states; the runtime reaches it through [`AirSim`], the way it
//! reaches the light through [`crate::LightMaker`].

use bpe_core::{Address, Scene, SlotId, Value};
use glam::Vec2;

use crate::{Sims, World};

/// The kit's air, as the physics uses it. Every world built from the same sims and scene wakes and
/// steps it the same, so the balls move the same in every playback.
pub trait AirSim: Send + Sync {
    /// Gives `world` an air that steps from now on: its Air card's, or still air without one.
    fn wake(&self, world: &mut World);
    /// Brings the world's air (when it has one) up to the world's step.
    fn catch_up(&self, world: &mut World);
    /// The air's velocity at each of `points`, px/s; `None` when the world has no air.
    fn velocities(&self, world: &World, points: &[Vec2]) -> Option<Vec<Vec2>>;
}

/// Whether some ball of `scene` follows the air: an enabled thing's Motion card whose module names
/// an [`crate::ModuleSim::air_key`] has it set above 0, varied from 0, keyed or following
/// something, or an enabled rule changes it. Read from the saved scene, so every world built from
/// it (and every part of it) agrees. The world then steps the air with the physics from the start.
pub fn air_felt(scene: &Scene, sims: &Sims) -> bool {
    let set = |v: Option<&Value>| match v {
        Some(Value::Number(x)) => *x != 0.0,
        Some(Value::Int(x)) => *x != 0,
        _ => false,
    };
    let ruled = |key: &str| {
        scene
            .rules
            .list
            .iter()
            .filter(|r| r.enabled)
            .flat_map(|r| &r.blocks)
            .filter_map(|b| sims.block(&b.kind).and_then(|s| s.changes(&b.params)))
            .any(|(slot, k)| slot == Some(SlotId::Motion) && k == key)
    };
    scene.things.values().filter(|t| t.enabled).any(|t| {
        let Some(card) = t.slots.get(&SlotId::Motion) else { return false };
        let Some(key) = sims.module(&card.kind).and_then(|m| m.air_key()) else { return false };
        let a = Address::on(t.id, SlotId::Motion, key);
        set(card.params.get(key))
            || set(card.params.get(&format!("{key}~to")))
            || scene.tracks.get(&a).is_some_and(|tr| !tr.keys.is_empty())
            || scene.pulses.contains_key(&a)
            || ruled(key)
    })
}

/// Whether every world built from `scene` reads its Air card: some ball feels the air
/// ([`air_felt`]), or a thing reads it as it is drawn ([`crate::ThingSim::reads_air`]). Then the
/// world senses the Air card's keys (and the sound or light they follow) and the physics
/// fingerprint counts the card and its keys, so an edit of them starts the playback again.
pub fn air_read(scene: &Scene, resolved: &bpe_core::Resolved, sims: &Sims) -> bool {
    air_felt(scene, sims) || resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| s.reads_air(r)))
}

impl World {
    /// Works out, once a step, the air each ball that follows it is in, and how hard it follows:
    /// the air brought to this step first. Nothing when no ball feels the air.
    pub(crate) fn work_out_air(&mut self, sims: &Sims) {
        self.air_at.clear();
        if !self.air_felt {
            return;
        }
        let Some(air) = sims.air.clone() else { return };
        air.catch_up(self);
        let mut out = std::collections::BTreeMap::new();
        for (id, prepared) in &self.modules {
            let Some(s) = prepared.get(&SlotId::Motion) else { continue };
            let Some(sim) = sims.module(&s.kind) else { continue };
            let Some(bodies) = self.bodies.get(id) else { continue };
            let grips: Vec<f32> = (0..bodies.len())
                .map(|row| {
                    let g = sim.air_grip(s.state_for(row));
                    if bodies.alive[row] && !bodies.held[row] && g.is_finite() && g > 0.0 { g } else { 0.0 }
                })
                .collect();
            if grips.iter().all(|g| *g == 0.0) {
                continue;
            }
            let Some(winds) = air.velocities(self, &bodies.position) else { continue };
            let each = winds
                .into_iter()
                .zip(grips)
                .map(|(w, g)| if w.is_finite() { (w, g) } else { (Vec2::ZERO, 0.0) })
                .collect();
            out.insert(*id, each);
        }
        self.air_at = out;
    }
}
