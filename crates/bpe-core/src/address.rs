//! Addresses name one setting anywhere in a scene, for keyframes, dice, pulses and rules.

use crate::resolve::{defaults, resolve_slot};
use crate::{EditError, ModuleId, Registry, Scene, SettingDef, SlotId, SlotLink, ThingId, Value};
use serde::{Deserialize, Serialize};

/// What holds the setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Owner {
    Thing(ThingId),
    Module(ModuleId),
}

/// One setting: a thing's own, a thing's change to one of its slots, or a library module's.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Address {
    pub owner: Owner,
    /// For a thing, the slot whose module the setting belongs to; `None` for the thing's own.
    pub slot: Option<SlotId>,
    pub key: String,
}

fn find(defs: &'static [SettingDef], key: &str) -> Option<&'static SettingDef> {
    defs.iter().find(|d| d.key == key)
}

impl Scene {
    /// The module kind's settings for a thing's slot: its own link if it has one, else what it inherits.
    fn slot_defs(&self, reg: &Registry, id: ThingId, slot: SlotId) -> Option<&'static [SettingDef]> {
        let kind = match self.things.get(&id)?.slots.get(&slot) {
            Some(SlotLink::Linked { module, .. }) => self.library.modules.get(module)?.kind.clone(),
            _ => resolve_slot(self, reg, id, slot, &mut Vec::new())?.kind,
        };
        Some(reg.module(&kind)?.settings())
    }

    /// The description of the setting at `a`.
    pub fn def_at(&self, reg: &Registry, a: &Address) -> Option<&'static SettingDef> {
        let defs = match (a.owner, a.slot) {
            (Owner::Thing(id), None) => reg.thing(&self.things.get(&id)?.kind)?.settings(),
            (Owner::Thing(id), Some(slot)) => self.slot_defs(reg, id, slot)?,
            (Owner::Module(m), _) => reg.module(&self.library.modules.get(&m)?.kind)?.settings(),
        };
        find(defs, &a.key)
    }

    /// The value the setting at `a` has once everything above it is taken into account.
    pub fn value_at(&self, reg: &Registry, a: &Address) -> Option<Value> {
        let def = self.def_at(reg, a)?;
        match (a.owner, a.slot) {
            (Owner::Thing(id), None) => Some(self.things[&id].params.get(&a.key).unwrap_or(&def.default).clone()),
            (Owner::Thing(id), Some(slot)) => {
                resolve_slot(self, reg, id, slot, &mut Vec::new())?.params.get(&a.key).cloned()
            }
            (Owner::Module(m), _) => Some(self.library.modules[&m].params.get(&a.key).unwrap_or(&def.default).clone()),
        }
    }

    /// Sets the setting at `a`, pulled into range. A value equal to what it would get anyway is
    /// removed instead of stored, so files only hold real changes.
    pub fn set_at(&mut self, reg: &Registry, a: &Address, v: Value) -> Result<(), EditError> {
        match a.owner {
            Owner::Thing(id) if !self.things.contains_key(&id) => return Err(EditError::Missing(id)),
            Owner::Module(m) if !self.library.modules.contains_key(&m) => return Err(EditError::MissingModule(m)),
            _ => {}
        }
        let linked = match (a.owner, a.slot) {
            (Owner::Thing(id), Some(slot)) => match self.things[&id].slots.get(&slot) {
                Some(SlotLink::Linked { module, .. }) => Some(*module),
                _ => return Err(EditError::NotLinked),
            },
            _ => None,
        };
        let def = self.def_at(reg, a).ok_or_else(|| EditError::UnknownSetting(a.key.clone()))?;
        if !def.ty.accepts(&v) {
            return Err(EditError::WrongType);
        }
        let v = def.clamp(v);
        let (params, inherited) = match (a.owner, linked) {
            (Owner::Thing(id), Some(m)) => {
                let module = &self.library.modules.get(&m).ok_or(EditError::MissingModule(m))?.params;
                let defs = reg.module(&self.library.modules[&m].kind).map(|k| k.settings()).unwrap_or_default();
                let inherited = defaults(defs).merged(module).get(&a.key).cloned();
                let slot = a.slot.expect("linked means a slot");
                match self.things.get_mut(&id).and_then(|t| t.slots.get_mut(&slot)) {
                    Some(SlotLink::Linked { overrides, .. }) => (overrides, inherited),
                    _ => unreachable!("checked above"),
                }
            }
            (Owner::Thing(id), None) => {
                (&mut self.things.get_mut(&id).expect("checked").params, Some(def.default.clone()))
            }
            (Owner::Module(m), _) => {
                (&mut self.library.modules.get_mut(&m).expect("checked").params, Some(def.default.clone()))
            }
        };
        if inherited.as_ref() == Some(&v) {
            params.remove(&a.key);
        } else {
            params.set(&a.key, v);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    #[test]
    fn set_at_writes_sparse_clamped_overrides() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "look.plain", "Plain").unwrap();
        s.link(&r, b, SlotId::Look, m).unwrap();
        let glow = Address { owner: Owner::Thing(b), slot: Some(SlotId::Look), key: "glow".into() };
        s.set_at(&r, &glow, Value::Number(5.0)).unwrap();
        assert_eq!(s.value_at(&r, &glow), Some(Value::Number(2.0)));
        s.set_at(&r, &glow, Value::Number(0.0)).unwrap(); // equals the module's value
        assert!(
            matches!(&s.things[&b].slots[&SlotId::Look], SlotLink::Linked { overrides, .. } if overrides.is_empty())
        );
    }

    #[test]
    fn set_at_refuses_bad_writes() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let radius = Address { owner: Owner::Thing(b), slot: None, key: "radius".into() };
        assert!(matches!(s.set_at(&r, &radius, Value::Bool(true)), Err(EditError::WrongType)));
        let nope = Address { key: "wobble".into(), ..radius.clone() };
        assert!(matches!(s.set_at(&r, &nope, Value::Number(1.0)), Err(EditError::UnknownSetting(_))));
        let look = Address { owner: Owner::Thing(b), slot: Some(SlotId::Look), key: "glow".into() };
        assert!(matches!(s.set_at(&r, &look, Value::Number(1.0)), Err(EditError::NotLinked)));
    }

    #[test]
    fn thing_and_module_values_fall_back_to_defaults() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "look.plain", "Plain").unwrap();
        let radius = Address { owner: Owner::Thing(b), slot: None, key: "radius".into() };
        assert_eq!(s.value_at(&r, &radius), Some(Value::Number(6.0)));
        s.set_at(&r, &radius, Value::Number(9.0)).unwrap();
        assert_eq!(s.value_at(&r, &radius), Some(Value::Number(9.0)));
        let glow = Address { owner: Owner::Module(m), slot: None, key: "glow".into() };
        s.set_at(&r, &glow, Value::Number(1.0)).unwrap();
        assert_eq!(s.library.modules[&m].params.get("glow"), Some(&Value::Number(1.0)));
    }
}
