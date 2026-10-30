//! Instruments in the inspector: what a sound module plays through.

use bpe_core::{InstrumentId, InstrumentRef, ModuleId, Scene, SettingValue, SlotId, SlotLink, ThingId};
use bpe_mix::InstrumentChoice;

/// The instrument Scene defaults' sound plays through, if it points at one.
pub fn scene_instrument(scene: &Scene) -> Option<InstrumentId> {
    let root = scene.things.get(&ThingId::ROOT)?;
    let SlotLink::Linked { module, overrides } = root.slots.get(&SlotId::Sound)? else { return None };
    let m = scene.library.modules.get(module)?;
    let v = overrides.get("instrument").or_else(|| m.params.get("instrument"))?;
    InstrumentRef::from_value(v)?.0
}

/// What a module pointing at `id` plays: that instrument with the module's own macros, else
/// Scene defaults' one, else the built-in pluck.
pub fn choice(scene: &Scene, id: Option<InstrumentId>, module: Option<ModuleId>) -> InstrumentChoice {
    let lib = &scene.library;
    let found = id
        .and_then(|id| lib.instruments.get(&id))
        .or_else(|| scene_instrument(scene).and_then(|id| lib.instruments.get(&id)));
    let empty = Default::default();
    let macros = module.and_then(|m| lib.modules.get(&m)).map_or(&empty, |m| &m.macros);
    found.map_or_else(InstrumentChoice::pluck, |i| InstrumentChoice::from_library(i, macros))
}
