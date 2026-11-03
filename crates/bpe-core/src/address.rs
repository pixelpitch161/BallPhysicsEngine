//! Addresses name one setting anywhere in a scene, for keyframes and pulses.

use crate::{EditError, Registry, Scene, SettingDef, SlotId, ThingId, Value};
use serde::{Deserialize, Serialize};

/// One setting: a thing's own, or one on one of the thing's cards.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Address {
    pub thing: ThingId,
    /// The card the setting is on; `None` for the thing's own.
    pub slot: Option<SlotId>,
    pub key: String,
}

fn find(defs: &'static [SettingDef], key: &str) -> Option<&'static SettingDef> {
    defs.iter().find(|d| d.key == key)
}

impl Address {
    /// One of a thing's own settings.
    pub fn own(thing: ThingId, key: &str) -> Address {
        Address { thing, slot: None, key: key.into() }
    }

    /// A setting on one of a thing's cards.
    pub fn on(thing: ThingId, slot: SlotId, key: &str) -> Address {
        Address { thing, slot: Some(slot), key: key.into() }
    }
}

impl Scene {
    /// The description of the setting at `a`.
    pub fn def_at(&self, reg: &Registry, a: &Address) -> Option<&'static SettingDef> {
        let thing = self.things.get(&a.thing)?;
        let defs = match a.slot {
            None => reg.thing(&thing.kind)?.settings(),
            Some(slot) => reg.module(&thing.slots.get(&slot)?.kind)?.settings(),
        };
        find(defs, &a.key)
    }

    /// The value the setting at `a` has, its default when the thing hasn't changed it.
    pub fn value_at(&self, reg: &Registry, a: &Address) -> Option<Value> {
        let def = self.def_at(reg, a)?;
        let thing = &self.things[&a.thing];
        let params = match a.slot {
            None => &thing.params,
            Some(slot) => &thing.slots[&slot].params,
        };
        Some(params.get(&a.key).unwrap_or(&def.default).clone())
    }

    /// Sets the setting at `a`, pulled into range. A value equal to the default is removed
    /// instead of stored, so files only hold real changes.
    pub fn set_at(&mut self, reg: &Registry, a: &Address, v: Value) -> Result<(), EditError> {
        let thing = self.things.get(&a.thing).ok_or(EditError::Missing(a.thing))?;
        if let Some(slot) = a.slot
            && !thing.slots.contains_key(&slot)
        {
            return Err(EditError::NoSlot(slot));
        }
        let def = self.def_at(reg, a).ok_or_else(|| EditError::UnknownSetting(a.key.clone()))?;
        if !def.ty.accepts(&v) {
            return Err(EditError::WrongType);
        }
        let v = def.clamp(v);
        let thing = self.things.get_mut(&a.thing).expect("checked above");
        let params = match a.slot {
            None => &mut thing.params,
            Some(slot) => &mut thing.slots.get_mut(&slot).expect("checked above").params,
        };
        if def.default == v {
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
    fn set_at_writes_sparse_clamped_values() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.plain").unwrap();
        let glow = Address::on(b, SlotId::Look, "glow");
        s.set_at(&r, &glow, Value::Number(5.0)).unwrap();
        assert_eq!(s.value_at(&r, &glow), Some(Value::Number(2.0)));
        s.set_at(&r, &glow, Value::Number(0.0)).unwrap(); // the default
        assert!(s.things[&b].slots[&SlotId::Look].params.is_empty());
    }

    #[test]
    fn set_at_refuses_bad_writes() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let radius = Address::own(b, "radius");
        assert!(matches!(s.set_at(&r, &radius, Value::Bool(true)), Err(EditError::WrongType)));
        assert!(matches!(s.set_at(&r, &radius, Value::Number(f64::NAN)), Err(EditError::WrongType)));
        let nope = Address { key: "wobble".into(), ..radius.clone() };
        assert!(matches!(s.set_at(&r, &nope, Value::Number(1.0)), Err(EditError::UnknownSetting(_))));
        let look = Address::on(b, SlotId::Look, "glow");
        assert_eq!(s.set_at(&r, &look, Value::Number(1.0)), Err(EditError::NoSlot(SlotId::Look)));
        assert_eq!(
            s.set_at(&r, &Address::own(ThingId(9), "radius"), Value::Number(1.0)),
            Err(EditError::Missing(ThingId(9)))
        );
    }

    #[test]
    fn values_fall_back_to_defaults_and_a_missing_card_has_none() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let radius = Address::own(b, "radius");
        assert_eq!(s.value_at(&r, &radius), Some(Value::Number(6.0)));
        s.set_at(&r, &radius, Value::Number(9.0)).unwrap();
        assert_eq!(s.value_at(&r, &radius), Some(Value::Number(9.0)));
        let glow = Address::on(b, SlotId::Look, "glow");
        assert_eq!(s.def_at(&r, &glow), None);
        assert_eq!(s.value_at(&r, &glow), None);
        s.add_slot(&r, b, SlotId::Look, "look.plain").unwrap();
        assert_eq!(s.def_at(&r, &glow).map(|d| d.label), Some("Glow"));
        assert_eq!(s.value_at(&r, &glow), Some(Value::Number(0.0)));
    }
}
