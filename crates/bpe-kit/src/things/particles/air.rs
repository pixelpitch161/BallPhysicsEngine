//! A Particles thing and the scene's air (particles part 2): the Air card is added by itself the
//! first time a Particles thing follows the air, so the owner can find and set it. The air itself
//! is [`crate::air`]; the thing wakes it and brings it along in `sim.rs` and `step.rs`.

use bpe_core::{EditError, Registry, Scene, SlotId, ThingId};

use crate::modules::air::AIR;

/// Gives the scene its Air card when it has none.
pub fn ensure_air_card(scene: &mut Scene, reg: &Registry) -> Result<(), EditError> {
    let has = scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Air));
    if !has {
        scene.add_slot(reg, ThingId::ROOT, SlotId::Air, AIR)?;
    }
    Ok(())
}

/// Whether adding a layer of `kind` to a thing should give the scene its Air card: a Follow the
/// air force.
pub fn wants_air_card(kind: &str) -> bool {
    kind == super::forces::AIR
}
