//! Edits to the scene tree and the module library. Each edit either succeeds whole or changes
//! nothing.

use crate::{
    Address, Attach, Instrument, InstrumentId, InstrumentRef, KindId, Module, ModuleGraph, ModuleId, Owner, Params,
    Progression, ProgressionId, ProgressionRef, Registry, Scene, SettingValue, SlotId, SlotLink, Stash, Thing, ThingId,
    Value, ValueType,
};
use glam::Vec2;
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    UnknownKind(KindId),
    Missing(ThingId),
    MissingModule(ModuleId),
    /// Scene settings can't be removed or moved.
    Root,
    WouldLoop,
    SlotNotSupported(SlotId),
    /// The module or kind is made for a different slot.
    WrongSlot,
    NotLinked,
    /// These things still link the module.
    InUse(Vec<ThingId>),
    WrongType,
    UnknownSetting(String),
    MissingProgression(ProgressionId),
    /// These settings still point at the progression.
    ProgressionInUse(Vec<Address>),
    MissingInstrument(InstrumentId),
    /// These settings still point at the instrument.
    InstrumentInUse(Vec<Address>),
    /// A scene has only one of these.
    OnlyOne(KindId),
    /// The thing's kind doesn't take that kind of module in that slot.
    NotAccepted,
}

/// What a thing's own starter in `slot` is called: "Ball look", "Ball 2 sound".
pub fn starter_name(thing: &str, slot: SlotId) -> String {
    format!("{thing} {}", slot.name().to_lowercase())
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::UnknownKind(k) => write!(f, "This version of Ball Studio doesn't know \"{k}\""),
            EditError::Missing(_) => write!(f, "That thing isn't in the scene any more"),
            EditError::MissingModule(_) => write!(f, "That module isn't in the library any more"),
            EditError::Root => write!(f, "Scene settings can't be moved or removed"),
            EditError::WouldLoop => write!(f, "A thing can't ride on itself or on something inside it"),
            EditError::SlotNotSupported(s) => write!(f, "This kind of thing has no {} slot", s.name()),
            EditError::WrongSlot => write!(f, "That module is made for a different slot"),
            EditError::NotLinked => write!(f, "This slot doesn't use a module of its own"),
            EditError::InUse(users) if users.len() == 1 => write!(f, "One thing still uses this module"),
            EditError::InUse(users) => write!(f, "{} things still use this module", users.len()),
            EditError::WrongType => write!(f, "That kind of value doesn't fit this setting"),
            EditError::UnknownSetting(k) => write!(f, "There is no setting called \"{k}\" here"),
            EditError::MissingProgression(_) => write!(f, "That chord progression isn't in the library any more"),
            EditError::ProgressionInUse(users) if users.len() == 1 => {
                write!(f, "One setting still uses this chord progression")
            }
            EditError::ProgressionInUse(users) => {
                write!(f, "{} settings still use this chord progression", users.len())
            }
            EditError::MissingInstrument(_) => write!(f, "That instrument isn't in the library any more"),
            EditError::InstrumentInUse(users) if users.len() == 1 => write!(f, "One sound still uses this instrument"),
            EditError::InstrumentInUse(users) => write!(f, "{} sounds still use this instrument", users.len()),
            EditError::OnlyOne(k) => write!(f, "A scene has only one {k}"),
            EditError::NotAccepted => write!(f, "That kind of module can't go there"),
        }
    }
}

impl std::error::Error for EditError {}

impl Scene {
    /// Adds a thing of `kind` as the last child of `parent`.
    pub fn add_thing(&mut self, reg: &Registry, kind: &str, parent: ThingId) -> Result<ThingId, EditError> {
        let k = reg.thing(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        if k.single() && self.things.values().any(|t| t.kind == kind) {
            return Err(EditError::OnlyOne(kind.into()));
        }
        if !self.things.contains_key(&parent) {
            return Err(EditError::Missing(parent));
        }
        let id = ThingId(self.next_thing);
        self.next_thing += 1;
        let thing = Thing {
            id,
            name: self.free_name(k.name()),
            kind: kind.into(),
            parent: Some(parent),
            slots: k.default_slots().iter().map(|&s| (s, SlotLink::Inherit)).collect(),
            ..Thing::default()
        };
        self.things.insert(id, thing);
        let at = self.subtree(parent).end;
        self.order.insert(at, id);
        Ok(id)
    }

    /// `base`, or `base` with the next number after the highest one in use ("Balls 2").
    fn free_name(&self, base: &str) -> String {
        let prefix = format!("{base} ");
        let highest = self
            .things
            .values()
            .filter_map(|t| if t.name == base { Some(1) } else { t.name.strip_prefix(&prefix)?.parse::<u64>().ok() })
            .max();
        match highest {
            None => base.to_owned(),
            Some(n) => format!("{base} {}", n.saturating_add(1)),
        }
    }

    /// Where `id` and everything under it sit in `order`. Empty when `id` isn't there.
    fn subtree(&self, id: ThingId) -> Range<usize> {
        let Some(start) = self.order.iter().position(|&t| t == id) else { return 0..0 };
        let mut inside = BTreeSet::from([id]);
        let mut end = start + 1;
        while let Some(t) = self.order.get(end) {
            match self.things.get(t).and_then(|t| t.parent) {
                Some(p) if inside.contains(&p) => {
                    inside.insert(*t);
                    end += 1;
                }
                _ => break,
            }
        }
        start..end
    }

    fn check_movable(&self, id: ThingId) -> Result<(), EditError> {
        if id == ThingId::ROOT {
            return Err(EditError::Root);
        }
        if !self.things.contains_key(&id) {
            return Err(EditError::Missing(id));
        }
        Ok(())
    }

    /// Moves `id` and everything under it to the end of `parent`'s children.
    pub fn set_parent(&mut self, id: ThingId, parent: ThingId, attach: Attach) -> Result<(), EditError> {
        self.check_movable(id)?;
        if !self.things.contains_key(&parent) {
            return Err(EditError::Missing(parent));
        }
        if parent == id || self.ancestors(parent).contains(&id) {
            return Err(EditError::WouldLoop);
        }
        let block: Vec<ThingId> = self.order.drain(self.subtree(id)).collect();
        let thing = self.things.get_mut(&id).expect("checked above");
        thing.parent = Some(parent);
        thing.attach = attach;
        let at = self.subtree(parent).end;
        self.order.splice(at..at, block);
        Ok(())
    }

    /// Changes how `id` hangs off its parent without moving it among its siblings.
    pub fn set_attach(&mut self, id: ThingId, attach: Attach) -> Result<(), EditError> {
        self.check_movable(id)?;
        self.things.get_mut(&id).expect("checked above").attach = attach;
        Ok(())
    }

    /// Moves `id` and everything under it to just before `sibling`, which must share its parent.
    pub fn move_before(&mut self, id: ThingId, sibling: ThingId) -> Result<(), EditError> {
        self.check_movable(id)?;
        let parent = self.things[&id].parent;
        if self.things.get(&sibling).is_none_or(|s| s.parent != parent) || sibling == id {
            return Err(EditError::Missing(sibling));
        }
        let block: Vec<ThingId> = self.order.drain(self.subtree(id)).collect();
        let at = self.subtree(sibling).start;
        self.order.splice(at..at, block);
        Ok(())
    }

    /// Removes `id` and everything under it, returning every removed id in tree order.
    pub fn remove_thing(&mut self, id: ThingId) -> Result<Vec<ThingId>, EditError> {
        self.check_movable(id)?;
        let removed: Vec<ThingId> = self.order.drain(self.subtree(id)).collect();
        for t in &removed {
            self.things.remove(t);
        }
        // Their keys and pulses go with them (a saved file would otherwise keep them forever).
        let gone = |a: &Address| matches!(a.owner, Owner::Thing(t) if removed.contains(&t));
        self.tracks.retain(|a, _| !gone(a));
        self.pulses.retain(|a, _| !gone(a));
        self.locks.retain(|a| !gone(a));
        // Their starters go too, unless something else now uses them.
        let owned: Vec<ModuleId> = self
            .library
            .modules
            .values()
            .filter(|m| m.owner.is_some_and(|o| removed.contains(&o)))
            .map(|m| m.id)
            .collect();
        for m in owned {
            if self.remove_module(m).is_err()
                && let Some(module) = self.library.modules.get_mut(&m)
            {
                module.owner = None;
            }
        }
        Ok(removed)
    }

    /// Shows `slot` on the thing, inheriting for now. A slot already shown is left as it is.
    pub fn add_slot(&mut self, reg: &Registry, id: ThingId, slot: SlotId) -> Result<(), EditError> {
        let thing = self.things.get_mut(&id).ok_or(EditError::Missing(id))?;
        let kind = reg.thing(&thing.kind).ok_or_else(|| EditError::UnknownKind(thing.kind.clone()))?;
        if !kind.slots().contains(&slot) {
            return Err(EditError::SlotNotSupported(slot));
        }
        thing.slots.entry(slot).or_insert(SlotLink::Inherit);
        Ok(())
    }

    /// Turns module `m` into a module of `kind` (of the same slot) in place. Settings, keyframes,
    /// pulses, locks and per-thing changes the new kind also has stay; the rest are set aside in
    /// the module's stash under the old kind, and anything stashed earlier under `kind` comes
    /// back wherever the key is still free. Nothing changes if the edit is refused.
    pub fn set_module_kind(&mut self, reg: &Registry, m: ModuleId, kind: &str) -> Result<(), EditError> {
        let module = self.library.modules.get(&m).ok_or(EditError::MissingModule(m))?;
        let new = reg.module(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        if new.slot() != module.slot {
            return Err(EditError::WrongSlot);
        }
        if module.kind == kind {
            return Ok(());
        }
        let (slot, old_kind) = (module.slot, module.kind.clone());
        let users = self.users_of(m);
        let accepts = |key: &str, v: &Value| new.settings().iter().any(|d| d.key == key && d.ty.accepts(v));
        let has = |key: &str| new.settings().iter().any(|d| d.key == key);
        let mut out = Stash::default();

        // 1. The module's own settings.
        let module = self.library.modules.get_mut(&m).expect("checked above");
        let kept = reg.carry_over(&module.params, kind);
        for (k, v) in std::mem::take(&mut module.params).0 {
            if kept.get(&k).is_none() {
                out.params.set(&k, v);
            }
        }
        module.params = kept;

        // 3. Each linked thing's changes.
        for &t in &users {
            if let Some(SlotLink::Linked { overrides, .. }) =
                self.things.get_mut(&t).and_then(|t| t.slots.get_mut(&slot))
            {
                let kept = reg.carry_over(overrides, kind);
                let mut dropped = Params::default();
                for (k, v) in std::mem::take(overrides).0 {
                    if kept.get(&k).is_none() {
                        dropped.set(&k, v);
                    }
                }
                *overrides = kept;
                if !dropped.is_empty() {
                    out.overrides.insert(t, dropped);
                }
            }
        }

        // 2. Keyframes, pulses and locks on the module and on its linked things.
        let whose = |a: &Address| match a.owner {
            Owner::Module(id) if id == m && a.slot.is_none() => Some(None),
            Owner::Thing(t) if a.slot == Some(slot) && users.contains(&t) => Some(Some(t)),
            _ => None,
        };
        let gone_tracks: Vec<Address> = self
            .tracks
            .iter()
            .filter(|(a, t)| whose(a).is_some() && !t.keys.first().is_some_and(|k| accepts(&a.key, &k.value)))
            .map(|(a, _)| a.clone())
            .collect();
        for a in gone_tracks {
            let track = self.tracks.remove(&a).expect("listed above");
            match whose(&a).flatten() {
                None => out.tracks.insert(a.key, track),
                Some(t) => out.thing_tracks.insert((t, a.key), track),
            };
        }
        let gone_pulses: Vec<Address> = self
            .pulses
            .iter()
            .filter(|(a, p)| whose(a).is_some() && !accepts(&a.key, &p.amount))
            .map(|(a, _)| a.clone())
            .collect();
        for a in gone_pulses {
            let pulse = self.pulses.remove(&a).expect("listed above");
            match whose(&a).flatten() {
                None => out.pulses.insert(a.key, pulse),
                Some(t) => out.thing_pulses.insert((t, a.key), pulse),
            };
        }
        let gone_locks: Vec<Address> =
            self.locks.iter().filter(|a| whose(a).is_some() && !has(&a.key)).cloned().collect();
        for a in gone_locks {
            self.locks.remove(&a);
            match whose(&a).flatten() {
                None => out.locks.insert(a.key),
                Some(t) => out.thing_locks.insert((t, a.key)),
            };
        }

        // 4. Put back what an earlier stay as `kind` set aside, where nothing holds the key now.
        let back = self.library.modules.get_mut(&m).expect("checked above").stash.remove(kind).unwrap_or_default();
        let module = self.library.modules.get_mut(&m).expect("checked above");
        for (k, v) in back.params.0 {
            if module.params.get(&k).is_none() {
                module.params.set(&k, v);
            }
        }
        let at = |owner, key: String| Address { owner, slot: None, key };
        let thing_at = |t, key: String| Address { owner: Owner::Thing(t), slot: Some(slot), key };
        for (k, v) in back.tracks {
            self.tracks.entry(at(Owner::Module(m), k)).or_insert(v);
        }
        for (k, v) in back.pulses {
            self.pulses.entry(at(Owner::Module(m), k)).or_insert(v);
        }
        for k in back.locks {
            self.locks.insert(at(Owner::Module(m), k));
        }
        for (t, saved) in back.overrides {
            if let Some(SlotLink::Linked { overrides, .. }) =
                self.things.get_mut(&t).and_then(|t| t.slots.get_mut(&slot))
                && users.contains(&t)
            {
                for (k, v) in saved.0 {
                    if overrides.get(&k).is_none() {
                        overrides.set(&k, v);
                    }
                }
            }
        }
        for ((t, k), v) in back.thing_tracks.into_iter().filter(|((t, _), _)| users.contains(t)) {
            self.tracks.entry(thing_at(t, k)).or_insert(v);
        }
        for ((t, k), v) in back.thing_pulses.into_iter().filter(|((t, _), _)| users.contains(t)) {
            self.pulses.entry(thing_at(t, k)).or_insert(v);
        }
        for (t, k) in back.thing_locks.into_iter().filter(|(t, _)| users.contains(t)) {
            self.locks.insert(thing_at(t, k));
        }

        // 5. The kind and its graph. `channel` and `macros` stay as they are.
        let module = self.library.modules.get_mut(&m).expect("checked above");
        if out != Stash::default() {
            module.stash.insert(old_kind, out);
        }
        module.kind = kind.into();
        module.graph = match module.graph.take() {
            Some(g) if !g.custom => new.template().map(|graph| ModuleGraph { graph, template: kind.into(), ..g }),
            _ => None,
        };
        Ok(())
    }

    /// Adds an empty module of `kind` to the library.
    pub fn add_module(&mut self, reg: &Registry, kind: &str, name: &str) -> Result<ModuleId, EditError> {
        let k = reg.module(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?;
        let id = ModuleId(self.next_module);
        self.next_module += 1;
        let module = Module { id, name: name.into(), slot: k.slot(), kind: kind.into(), ..Module::default() };
        self.library.modules.insert(id, module);
        Ok(id)
    }

    /// Makes a module of `kind` for `thing` as its own starter, named after it, and links it
    /// into the kind's slot.
    pub fn add_starter(&mut self, reg: &Registry, thing: ThingId, kind: &str) -> Result<ModuleId, EditError> {
        let name = self.things.get(&thing).ok_or(EditError::Missing(thing))?.name.clone();
        let slot = reg.module(kind).ok_or_else(|| EditError::UnknownKind(kind.into()))?.slot();
        let m = self.add_module(reg, kind, &starter_name(&name, slot))?;
        self.library.modules.get_mut(&m).expect("just added").owner = Some(thing);
        if let Err(e) = self.link(reg, thing, slot, m) {
            self.library.modules.remove(&m);
            return Err(e);
        }
        Ok(m)
    }

    /// Adds a copy of module `from`, changed by `change`, with its keyframes, pulses and locks.
    /// The copy belongs to no thing unless `change` says so.
    fn copy_module(&mut self, from: ModuleId, change: impl FnOnce(&mut Module)) -> Result<ModuleId, EditError> {
        let source = self.library.modules.get(&from).ok_or(EditError::MissingModule(from))?;
        let id = ModuleId(self.next_module);
        let mut copy = Module { id, owner: None, ..source.clone() };
        change(&mut copy);
        self.next_module += 1;
        self.library.modules.insert(id, copy);
        let moved =
            |a: &Address| (a.owner == Owner::Module(from)).then(|| Address { owner: Owner::Module(id), ..a.clone() });
        let tracks: Vec<_> = self.tracks.iter().filter_map(|(a, t)| moved(a).map(|b| (b, t.clone()))).collect();
        let pulses: Vec<_> = self.pulses.iter().filter_map(|(a, p)| moved(a).map(|b| (b, p.clone()))).collect();
        let locks: Vec<_> = self.locks.iter().filter_map(moved).collect();
        self.tracks.extend(tracks);
        self.pulses.extend(pulses);
        self.locks.extend(locks);
        Ok(id)
    }

    /// Gives `id` its own copy of module `from` in `slot`, named after it as a starter, with
    /// the settings in `params` (what the thing saw, a folder's changes included) written in
    /// where they differ from the module's.
    pub fn own_copy(
        &mut self,
        reg: &Registry,
        id: ThingId,
        slot: SlotId,
        from: ModuleId,
        params: Params,
    ) -> Result<ModuleId, EditError> {
        let name = self.things.get(&id).ok_or(EditError::Missing(id))?.name.clone();
        let source = self.library.modules.get(&from).ok_or(EditError::MissingModule(from))?;
        if source.slot != slot {
            return Err(EditError::WrongSlot);
        }
        // Keep the module's own settings and add only what `params` changes from them, so the
        // copy still follows the kind's defaults everywhere else.
        let mut base = reg.module(&source.kind).map(|k| crate::resolve::defaults(k.settings())).unwrap_or_default();
        base.apply(&source.params);
        let changes: Vec<(String, Value)> =
            params.iter().filter(|(k, v)| base.get(k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect();
        let copy = self.copy_module(from, |m| {
            m.name = starter_name(&name, slot);
            m.owner = Some(id);
            for (k, v) in changes {
                m.params.set(&k, v);
            }
        })?;
        if let Err(e) = self.link(reg, id, slot, copy) {
            self.library.modules.remove(&copy);
            return Err(e);
        }
        Ok(copy)
    }

    /// Gives the thing its own copy of the module linked in `slot`, named after it as a
    /// starter. Its changes stay changes (unlike [`Scene::make_unique`]).
    pub fn own_linked(&mut self, id: ThingId, slot: SlotId) -> Result<ModuleId, EditError> {
        let (m, _) = self.linked(id, slot)?;
        let name = starter_name(&self.things[&id].name, slot);
        let copy = self.copy_module(m, |c| {
            c.name = name;
            c.owner = Some(id);
        })?;
        if let Some(SlotLink::Linked { module, .. }) = self.things.get_mut(&id).and_then(|t| t.slots.get_mut(&slot)) {
            *module = copy;
        }
        Ok(copy)
    }

    /// Gives the thing its own copy of whatever fills `slot` now (usually a folder's module).
    pub fn make_own(&mut self, reg: &Registry, id: ThingId, slot: SlotId) -> Result<ModuleId, EditError> {
        let from = crate::resolve_slot(self, reg, id, slot, &mut Vec::new()).ok_or(EditError::NotLinked)?;
        self.own_copy(reg, id, slot, from.module, from.params)
    }

    /// Sets a thing's name exactly as given (as it is typed) and renames the starters it owns
    /// that still carry its old name.
    pub fn set_thing_name(&mut self, id: ThingId, name: &str) {
        let Some(thing) = self.things.get_mut(&id) else { return };
        let old = std::mem::replace(&mut thing.name, name.to_owned());
        for m in self.library.modules.values_mut() {
            if m.owner == Some(id) && m.name == starter_name(&old, m.slot) {
                m.name = starter_name(name, m.slot);
            }
        }
    }

    /// Renames a thing and the starters it owns that still carry its old name. A blank or
    /// unchanged name changes nothing and returns false.
    pub fn rename_thing(&mut self, id: ThingId, name: &str) -> bool {
        let name = name.trim();
        let Some(thing) = self.things.get(&id) else { return false };
        if name.is_empty() || thing.name == name {
            return false;
        }
        self.set_thing_name(id, name);
        true
    }

    /// Links fresh copies, owned by `to`, of every module owned by `from` that `to` links, so a
    /// duplicated thing doesn't share its original's starters. Each slot keeps its overrides.
    pub fn copy_owned_starters(&mut self, from: ThingId, to: ThingId) {
        let Some(thing) = self.things.get(&to) else { return };
        let name = thing.name.clone();
        let owned: Vec<(SlotId, ModuleId)> = thing
            .slots
            .iter()
            .filter_map(|(slot, link)| match link {
                SlotLink::Linked { module, .. } => Some((*slot, *module)),
                _ => None,
            })
            .filter(|(_, m)| self.library.modules.get(m).is_some_and(|m| m.owner == Some(from)))
            .collect();
        for (slot, m) in owned {
            let Ok(copy) = self.copy_module(m, |c| {
                c.name = starter_name(&name, slot);
                c.owner = Some(to);
            }) else {
                continue;
            };
            if let Some(SlotLink::Linked { module, .. }) = self.things.get_mut(&to).and_then(|t| t.slots.get_mut(&slot))
            {
                *module = copy;
            }
        }
    }

    /// Fills the thing's `slot` with module `m`, with no changes of its own.
    pub fn link(&mut self, reg: &Registry, id: ThingId, slot: SlotId, m: ModuleId) -> Result<(), EditError> {
        let module = self.library.modules.get(&m).ok_or(EditError::MissingModule(m))?;
        if module.slot != slot {
            return Err(EditError::WrongSlot);
        }
        let thing = self.things.get_mut(&id).ok_or(EditError::Missing(id))?;
        let kind = reg.thing(&thing.kind).ok_or_else(|| EditError::UnknownKind(thing.kind.clone()))?;
        if !kind.slots().contains(&slot) {
            return Err(EditError::SlotNotSupported(slot));
        }
        if !kind.accepts(slot, &module.kind) {
            return Err(EditError::NotAccepted);
        }
        thing.slots.insert(slot, SlotLink::Linked { module: m, overrides: Params::default() });
        Ok(())
    }

    /// Turns the slot off, so neither this thing nor its parent fills it.
    pub fn set_slot_off(&mut self, id: ThingId, slot: SlotId) {
        if let Some(t) = self.things.get_mut(&id) {
            t.slots.insert(slot, SlotLink::Off);
        }
    }

    /// Lets the slot use whatever the parent uses.
    pub fn set_slot_inherit(&mut self, id: ThingId, slot: SlotId) {
        if let Some(t) = self.things.get_mut(&id) {
            t.slots.insert(slot, SlotLink::Inherit);
        }
    }

    /// The module linked in the slot and the thing's changes to it.
    fn linked(&self, id: ThingId, slot: SlotId) -> Result<(ModuleId, &Params), EditError> {
        let thing = self.things.get(&id).ok_or(EditError::Missing(id))?;
        match thing.slots.get(&slot) {
            Some(SlotLink::Linked { module, overrides }) => Ok((*module, overrides)),
            _ => Err(EditError::NotLinked),
        }
    }

    fn overrides_mut(&mut self, id: ThingId, slot: SlotId) -> &mut Params {
        match self.things.get_mut(&id).and_then(|t| t.slots.get_mut(&slot)) {
            Some(SlotLink::Linked { overrides, .. }) => overrides,
            _ => unreachable!("callers check the slot is linked first"),
        }
    }

    /// Gives the thing its own copy of the linked module, with its changes baked in.
    pub fn make_unique(&mut self, id: ThingId, slot: SlotId) -> Result<ModuleId, EditError> {
        let (m, overrides) = self.linked(id, slot)?;
        let overrides = overrides.clone();
        let thing = self.things[&id].name.clone();
        let copy_id = self.copy_module(m, |c| {
            c.name = format!("{} ({thing})", c.name);
            c.params = c.params.merged(&overrides);
        })?;
        self.things
            .get_mut(&id)
            .expect("checked above")
            .slots
            .insert(slot, SlotLink::Linked { module: copy_id, overrides: Params::default() });
        Ok(copy_id)
    }

    /// Writes the thing's changes into the shared module, so every thing using it gets them.
    pub fn push_to_module(&mut self, id: ThingId, slot: SlotId) -> Result<(), EditError> {
        let (m, overrides) = self.linked(id, slot)?;
        let overrides = overrides.clone();
        let module = self.library.modules.get_mut(&m).ok_or(EditError::MissingModule(m))?;
        module.params = module.params.merged(&overrides);
        *self.overrides_mut(id, slot) = Params::default();
        Ok(())
    }

    /// Drops the thing's change to one setting, so it follows the module again.
    pub fn reset_override(&mut self, id: ThingId, slot: SlotId, key: &str) -> Result<(), EditError> {
        self.linked(id, slot)?;
        self.overrides_mut(id, slot).remove(key);
        Ok(())
    }

    /// Gives the module a new name.
    pub fn rename_module(&mut self, m: ModuleId, name: &str) -> Result<(), EditError> {
        let module = self.library.modules.get_mut(&m).ok_or(EditError::MissingModule(m))?;
        module.name = name.to_owned();
        Ok(())
    }

    /// Adds a copy of the module, named "<name> copy", that nothing links to yet.
    pub fn duplicate_module(&mut self, m: ModuleId) -> Result<ModuleId, EditError> {
        self.copy_module(m, |c| c.name = format!("{} copy", c.name))
    }

    /// Puts a new folder where the first of `ids` was and moves them all into it, in their old
    /// order. The root and anything already inside another listed thing are left where they are.
    pub fn make_folder(&mut self, reg: &Registry, ids: &[ThingId]) -> Result<ThingId, EditError> {
        let listed: BTreeSet<ThingId> = ids.iter().copied().collect();
        let moving: Vec<ThingId> = self
            .order
            .iter()
            .copied()
            .filter(|id| {
                *id != ThingId::ROOT
                    && listed.contains(id)
                    && !self.ancestors(*id).iter().any(|a| listed.contains(a) && *a != ThingId::ROOT)
            })
            .collect();
        let Some(&first) = moving.first() else { return Err(EditError::Root) };
        let parent = self.things[&first].parent.unwrap_or(ThingId::ROOT);
        let folder = self.add_thing(reg, "folder", parent)?;
        self.move_before(folder, first)?;
        // Things that rode the old parent keep riding: the folder rides it in their place (on the
        // one ball they all rode, if they agreed on one) and they ride the folder as before.
        let riders: Vec<Attach> = moving
            .iter()
            .filter(|id| self.things[id].parent == Some(parent))
            .map(|id| self.things[id].attach)
            .filter(|a| matches!(a, Attach::Ride { .. }))
            .collect();
        if let Some(Attach::Ride { item, .. }) = riders.first().copied()
            && parent != ThingId::ROOT
        {
            let shared = riders.iter().all(|a| matches!(a, Attach::Ride { item: i, .. } if *i == item));
            let item = if shared { item } else { None };
            self.set_attach(folder, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item })?;
            for id in &moving {
                if let Attach::Ride { item: Some(_), .. } = self.things[id].attach
                    && shared
                {
                    let t = self.things.get_mut(id).expect("gathered above");
                    if let Attach::Ride { item, .. } = &mut t.attach {
                        *item = None;
                    }
                }
            }
        }
        for id in moving {
            let attach = match self.things[&id].attach {
                ride @ Attach::Ride { .. } if self.things[&id].parent == Some(parent) => ride,
                _ => Attach::Group,
            };
            self.set_parent(id, folder, attach)?;
        }
        Ok(folder)
    }

    /// Changes one setting of the module linked in `slot`, for this thing only.
    pub fn set_override(
        &mut self,
        reg: &Registry,
        id: ThingId,
        slot: SlotId,
        key: &str,
        v: Value,
    ) -> Result<(), EditError> {
        self.set_at(reg, &Address { owner: Owner::Thing(id), slot: Some(slot), key: key.to_owned() }, v)
    }

    /// Removes a module nothing links to any more.
    pub fn remove_module(&mut self, m: ModuleId) -> Result<(), EditError> {
        if !self.library.modules.contains_key(&m) {
            return Err(EditError::MissingModule(m));
        }
        let users = self.users_of(m);
        if !users.is_empty() {
            return Err(EditError::InUse(users));
        }
        self.library.modules.remove(&m);
        self.tracks.retain(|a, _| a.owner != Owner::Module(m));
        self.pulses.retain(|a, _| a.owner != Owner::Module(m));
        self.locks.retain(|a| a.owner != Owner::Module(m));
        Ok(())
    }

    /// Adds a chord progression to the library.
    pub fn add_progression(&mut self, p: Progression) -> ProgressionId {
        let id = ProgressionId(self.next_progression);
        self.next_progression += 1;
        self.library.progressions.insert(id, clamped(p));
        id
    }

    /// Replaces a progression, folding every chord's pitches into 0..=11 and its length into range.
    pub fn set_progression(&mut self, id: ProgressionId, p: Progression) -> Result<(), EditError> {
        let slot = self.library.progressions.get_mut(&id).ok_or(EditError::MissingProgression(id))?;
        *slot = clamped(p);
        Ok(())
    }

    /// Adds a copy of a progression, named "<name> copy".
    pub fn copy_progression(&mut self, id: ProgressionId) -> Result<ProgressionId, EditError> {
        let source = self.library.progressions.get(&id).ok_or(EditError::MissingProgression(id))?;
        let copy = Progression { name: format!("{} copy", source.name), ..source.clone() };
        Ok(self.add_progression(copy))
    }

    /// Every setting that points at progression `id`: things' own settings, their slot
    /// changes and library modules'.
    pub fn progression_users(&self, reg: &Registry, id: ProgressionId) -> Vec<Address> {
        self.settings_set_to(reg, ValueType::Progression, &ProgressionRef(Some(id)).to_value())
    }

    /// Every setting of type `ty` that is set to `wanted`.
    fn settings_set_to(&self, reg: &Registry, ty: ValueType, wanted: &Value) -> Vec<Address> {
        let mut out = Vec::new();
        let mut look = |owner: Owner, slot: Option<SlotId>, params: &Params, defs: &[crate::SettingDef]| {
            for d in defs.iter().filter(|d| d.ty == ty) {
                if params.get(d.key) == Some(wanted) {
                    out.push(Address { owner, slot, key: d.key.into() });
                }
            }
        };
        for (&tid, t) in &self.things {
            if let Some(kind) = reg.thing(&t.kind) {
                look(Owner::Thing(tid), None, &t.params, kind.settings());
            }
            for (&slot, link) in &t.slots {
                if let SlotLink::Linked { module, overrides } = link
                    && let Some(kind) = self.library.modules.get(module).and_then(|m| reg.module(&m.kind))
                {
                    look(Owner::Thing(tid), Some(slot), overrides, kind.settings());
                }
            }
        }
        for (&mid, m) in &self.library.modules {
            if let Some(kind) = reg.module(&m.kind) {
                look(Owner::Module(mid), None, &m.params, kind.settings());
            }
        }
        out
    }

    /// Removes a progression no setting points at any more.
    pub fn remove_progression(&mut self, reg: &Registry, id: ProgressionId) -> Result<(), EditError> {
        if !self.library.progressions.contains_key(&id) {
            return Err(EditError::MissingProgression(id));
        }
        let users = self.progression_users(reg, id);
        if !users.is_empty() {
            return Err(EditError::ProgressionInUse(users));
        }
        self.library.progressions.remove(&id);
        Ok(())
    }
}

impl Scene {
    /// Adds an instrument to the library.
    pub fn add_instrument(&mut self, i: Instrument) -> InstrumentId {
        let id = InstrumentId(self.next_instrument);
        self.next_instrument += 1;
        self.library.instruments.insert(id, i);
        id
    }

    /// Replaces an instrument.
    pub fn set_instrument(&mut self, id: InstrumentId, i: Instrument) -> Result<(), EditError> {
        let slot = self.library.instruments.get_mut(&id).ok_or(EditError::MissingInstrument(id))?;
        *slot = i;
        Ok(())
    }

    /// Adds a copy of an instrument, named "<name> copy".
    pub fn copy_instrument(&mut self, id: InstrumentId) -> Result<InstrumentId, EditError> {
        let source = self.library.instruments.get(&id).ok_or(EditError::MissingInstrument(id))?;
        let copy = Instrument { name: format!("{} copy", source.name), ..source.clone() };
        Ok(self.add_instrument(copy))
    }

    /// Every setting that points at instrument `id`.
    pub fn instrument_users(&self, reg: &Registry, id: InstrumentId) -> Vec<Address> {
        self.settings_set_to(reg, ValueType::Instrument, &InstrumentRef(Some(id)).to_value())
    }

    /// Removes an instrument no setting points at any more.
    pub fn remove_instrument(&mut self, reg: &Registry, id: InstrumentId) -> Result<(), EditError> {
        if !self.library.instruments.contains_key(&id) {
            return Err(EditError::MissingInstrument(id));
        }
        let users = self.instrument_users(reg, id);
        if !users.is_empty() {
            return Err(EditError::InstrumentInUse(users));
        }
        self.library.instruments.remove(&id);
        Ok(())
    }

    /// Sets a module's own value for one of its instrument's macros, or with `None` goes back to
    /// the instrument's.
    pub fn set_macro(&mut self, module: ModuleId, key: &str, value: Option<f32>) -> Result<(), EditError> {
        let m = self.library.modules.get_mut(&module).ok_or(EditError::MissingModule(module))?;
        match value {
            Some(v) if v.is_finite() => {
                m.macros.insert(key.into(), v);
            }
            Some(_) => return Err(EditError::WrongType),
            None => {
                m.macros.remove(key);
            }
        }
        Ok(())
    }
}

fn clamped(mut p: Progression) -> Progression {
    p.key.root %= 12;
    p.chords = p.chords.into_iter().map(|c| c.clamp()).collect();
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    #[test]
    fn progressions_get_fresh_ids_and_copies() {
        let mut s = Scene::default();
        assert_eq!(s.add_progression(Progression::starter()), ProgressionId(0));
        assert_eq!(s.add_progression(Progression::default()), ProgressionId(1));
        let copy = s.copy_progression(ProgressionId(0)).unwrap();
        assert_eq!(copy, ProgressionId(2));
        assert_eq!(s.library.progressions[&copy].name, "Starter copy");
        assert_eq!(s.copy_progression(ProgressionId(9)), Err(EditError::MissingProgression(ProgressionId(9))));
    }

    #[test]
    fn copying_an_instrument_names_it_copy() {
        let mut s = Scene::default();
        let a = s.add_instrument(Instrument::default());
        let copy = s.copy_instrument(a).unwrap();
        assert_eq!(copy, InstrumentId(1));
        assert_eq!(s.library.instruments[&copy].name, "Pluck copy");
        assert_eq!(s.copy_instrument(InstrumentId(7)), Err(EditError::MissingInstrument(InstrumentId(7))));
    }

    #[test]
    fn an_instrument_in_use_cannot_be_removed() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let i = s.add_instrument(Instrument::default());
        let m = s.add_module(&r, "sound.pluck", "Sound").unwrap();
        let a = Address { owner: Owner::Module(m), slot: None, key: "instrument".into() };
        s.set_at(&r, &a, InstrumentRef(Some(i)).to_value()).unwrap();
        assert_eq!(s.remove_instrument(&r, i), Err(EditError::InstrumentInUse(vec![a.clone()])));
        s.set_at(&r, &a, Value::Int(-1)).unwrap();
        assert_eq!(s.remove_instrument(&r, i), Ok(()));
        assert!(s.library.instruments.is_empty());
    }

    #[test]
    fn a_macro_override_is_set_and_cleared() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let m = s.add_module(&r, "sound.pluck", "Sound").unwrap();
        s.set_macro(m, "3.cutoff", Some(500.0)).unwrap();
        assert_eq!(s.library.modules[&m].macros["3.cutoff"], 500.0);
        assert_eq!(s.set_macro(m, "3.cutoff", Some(f32::NAN)), Err(EditError::WrongType));
        s.set_macro(m, "3.cutoff", None).unwrap();
        assert!(s.library.modules[&m].macros.is_empty());
    }

    #[test]
    fn a_progression_in_use_cannot_be_removed() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let p = s.add_progression(Progression::starter());
        let m = s.add_module(&r, "sound.pluck", "Sound").unwrap();
        let a = Address { owner: Owner::Module(m), slot: None, key: "chords".into() };
        s.set_at(&r, &a, ProgressionRef(Some(p)).to_value()).unwrap();
        assert_eq!(s.remove_progression(&r, p), Err(EditError::ProgressionInUse(vec![a.clone()])));
        s.set_at(&r, &a, Value::Int(-1)).unwrap();
        assert_eq!(s.remove_progression(&r, p), Ok(()));
        assert!(s.library.progressions.is_empty());
    }

    #[test]
    fn set_progression_clamps_chords() {
        let mut s = Scene::default();
        let id = s.add_progression(Progression::default());
        let chord = crate::Chord { root: 14, beats: 0, ..crate::Chord::default() };
        s.set_progression(id, Progression { chords: vec![chord], ..Progression::default() }).unwrap();
        let stored = s.library.progressions[&id].chords[0];
        assert_eq!((stored.root, stored.beats), (2, 1));
    }

    #[test]
    fn removing_a_thing_or_module_drops_its_locks() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "look.plain", "Look").unwrap();
        s.set_locked(Address { owner: Owner::Thing(a), slot: None, key: "radius".into() }, true);
        s.set_locked(Address { owner: Owner::Module(m), slot: None, key: "color".into() }, true);
        let root = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "gravity".into() };
        s.set_locked(root.clone(), true);
        assert_eq!(s.locks.len(), 3);
        s.remove_thing(a).unwrap();
        s.remove_module(m).unwrap();
        assert_eq!(s.locks.len(), 1);
        assert!(s.is_locked(&root));
    }

    #[test]
    fn add_thing_names_and_slots_from_the_kind() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!((s.things[&a].name.as_str(), s.things[&b].name.as_str()), ("Balls", "Balls 2"));
        assert_eq!(
            s.things[&a].slots.keys().copied().collect::<Vec<_>>(),
            [SlotId::Look, SlotId::Motion, SlotId::Bounce, SlotId::Sound]
        );
        assert!(matches!(s.add_thing(&r, "magnet", ThingId::ROOT), Err(EditError::UnknownKind(_))));
    }

    #[test]
    fn add_starter_names_owns_and_links() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let balls = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_starter(&r, balls, "look.plain").unwrap();
        let module = &s.library.modules[&m];
        assert_eq!((module.name.as_str(), module.owner, module.slot), ("Balls look", Some(balls), SlotId::Look));
        assert_eq!(s.users_of(m), [balls]);
        assert!(matches!(s.things[&balls].slots[&SlotId::Look], SlotLink::Linked { module, .. } if module == m));
        assert_eq!(starter_name("Ball 2", SlotId::Sound), "Ball 2 sound");
    }

    #[test]
    fn rename_thing_renames_only_matching_starters() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let balls = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let look = s.add_starter(&r, balls, "look.plain").unwrap();
        let sound = s.add_starter(&r, balls, "sound.pluck").unwrap();
        s.rename_module(sound, "Mine").unwrap();
        assert!(s.rename_thing(balls, "Red"));
        assert_eq!(s.things[&balls].name, "Red");
        assert_eq!(s.library.modules[&look].name, "Red look");
        assert_eq!(s.library.modules[&sound].name, "Mine");
        assert!(!s.rename_thing(balls, "   "));
        assert!(!s.rename_thing(balls, " Red "));
        assert!(!s.rename_thing(ThingId(99), "X"));
        assert_eq!(s.things[&balls].name, "Red");
    }

    #[test]
    fn removing_drops_unused_owned_starters_only() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let balls = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let look = s.add_starter(&r, balls, "look.plain").unwrap();
        let mine = s.add_module(&r, "sound.pluck", "Made in the Library").unwrap();
        s.link(&r, balls, SlotId::Sound, mine).unwrap();
        s.remove_thing(balls).unwrap();
        assert!(!s.library.modules.contains_key(&look));
        assert!(s.library.modules.contains_key(&mine));
    }

    #[test]
    fn removing_keeps_shared_starters_and_clears_owner() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let look = s.add_starter(&r, a, "look.plain").unwrap();
        s.link(&r, b, SlotId::Look, look).unwrap();
        s.remove_thing(a).unwrap();
        assert_eq!(s.library.modules[&look].owner, None);
        assert_eq!(s.users_of(look), [b]);
    }

    #[test]
    fn removing_a_folder_drops_its_childrens_starters() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let folder = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let balls = s.add_thing(&r, "balls", folder).unwrap();
        let mine = s.add_starter(&r, folder, "look.plain").unwrap();
        let theirs = s.add_starter(&r, balls, "sound.pluck").unwrap();
        s.remove_thing(folder).unwrap();
        assert!(!s.library.modules.contains_key(&mine) && !s.library.modules.contains_key(&theirs));
    }

    #[test]
    fn make_own_copies_the_followed_module() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let folder = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let balls = s.add_thing(&r, "balls", folder).unwrap();
        assert_eq!(s.make_own(&r, balls, SlotId::Look), Err(EditError::NotLinked));
        let f = s.add_starter(&r, folder, "look.plain").unwrap();
        s.library.modules.get_mut(&f).unwrap().params.set("glow", Value::Number(2.0));
        let own = s.make_own(&r, balls, SlotId::Look).unwrap();
        assert_ne!(own, f);
        let module = &s.library.modules[&own];
        assert_eq!((module.name.as_str(), module.owner), ("Balls look", Some(balls)));
        assert_eq!(module.params.get("glow"), Some(&Value::Number(2.0)));
        assert_eq!(s.library.modules[&f].name, "Folder look");
        assert_eq!(s.users_of(f), [folder]);
        assert_eq!(s.users_of(own), [balls]);
    }

    fn module_address(m: ModuleId, key: &str) -> crate::Address {
        crate::Address { owner: crate::Owner::Module(m), slot: None, key: key.into() }
    }

    /// Puts a key, a pulse and a lock on module `m`'s "glow".
    fn animate(s: &mut Scene, m: ModuleId) {
        let a = module_address(m, "glow");
        s.tracks.entry(a.clone()).or_default().set_key(1.0, Value::Number(4.0));
        s.pulses.insert(a.clone(), crate::Pulse::default());
        s.locks.insert(a);
    }

    fn is_animated(s: &Scene, m: ModuleId) -> bool {
        let a = module_address(m, "glow");
        s.tracks.contains_key(&a) && s.pulses.contains_key(&a) && s.locks.contains(&a)
    }

    #[test]
    fn make_own_keeps_the_folders_changes() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let folder = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let balls = s.add_thing(&r, "balls", folder).unwrap();
        let shared = s.add_module(&r, "look.plain", "Shared").unwrap();
        s.link(&r, folder, SlotId::Look, shared).unwrap();
        s.set_override(&r, folder, SlotId::Look, "glow", Value::Number(1.5)).unwrap();
        let own = s.make_own(&r, balls, SlotId::Look).unwrap();
        assert_eq!(s.library.modules[&own].params.get("glow"), Some(&Value::Number(1.5)));
    }

    #[test]
    fn copies_keep_the_modules_keyframes_pulses_and_locks() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let folder = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let balls = s.add_thing(&r, "balls", folder).unwrap();
        let f = s.add_starter(&r, folder, "look.plain").unwrap();
        animate(&mut s, f);
        let own = s.make_own(&r, balls, SlotId::Look).unwrap();
        assert!(is_animated(&s, own), "make_own");
        let dup = s.duplicate_module(f).unwrap();
        assert!(is_animated(&s, dup), "duplicate_module");
        let unique = s.make_unique(folder, SlotId::Look).unwrap();
        assert!(is_animated(&s, unique), "make_unique");
        let copy = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let mine = s.add_starter(&r, balls, "sound.pluck").unwrap();
        animate(&mut s, mine);
        let theirs = s.things[&balls].slots.clone();
        s.things.get_mut(&copy).unwrap().slots = theirs;
        s.copy_owned_starters(balls, copy);
        let Some(SlotLink::Linked { module, .. }) = s.things[&copy].slots.get(&SlotId::Sound) else { panic!() };
        assert_ne!(*module, mine);
        assert!(is_animated(&s, *module), "copy_owned_starters");
        assert!(is_animated(&s, f) && is_animated(&s, mine), "the originals keep theirs");
    }

    #[test]
    fn copies_made_by_hand_belong_to_no_thing() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let balls = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let look = s.add_starter(&r, balls, "look.plain").unwrap();
        let dup = s.duplicate_module(look).unwrap();
        assert_eq!(s.library.modules[&dup].owner, None);
        let unique = s.make_unique(balls, SlotId::Look).unwrap();
        assert_eq!(s.library.modules[&unique].owner, None);
    }

    #[test]
    fn setting_a_things_name_renames_its_starters_as_typed() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let balls = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let look = s.add_starter(&r, balls, "look.plain").unwrap();
        s.set_thing_name(balls, "Balls ");
        assert_eq!(s.things[&balls].name, "Balls ");
        assert_eq!(s.library.modules[&look].name, "Balls  look");
        s.set_thing_name(balls, "Red");
        assert_eq!(s.library.modules[&look].name, "Red look");
    }

    #[test]
    fn own_linked_gives_the_thing_a_copy_and_keeps_its_changes() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let shared = s.add_module(&r, "look.plain", "Shared").unwrap();
        s.library.modules.get_mut(&shared).unwrap().params.set("glow", Value::Number(1.0));
        animate(&mut s, shared);
        s.link(&r, a, SlotId::Look, shared).unwrap();
        s.link(&r, b, SlotId::Look, shared).unwrap();
        s.set_override(&r, b, SlotId::Look, "glow", Value::Number(0.5)).unwrap();
        let own = s.own_linked(b, SlotId::Look).unwrap();
        let module = &s.library.modules[&own];
        assert_eq!((module.name.as_str(), module.owner), ("Balls 2 look", Some(b)));
        assert_eq!(module.params.get("glow"), Some(&Value::Number(1.0)));
        assert!(is_animated(&s, own));
        let Some(SlotLink::Linked { module, overrides }) = s.things[&b].slots.get(&SlotId::Look) else { panic!() };
        assert_eq!((*module, overrides.get("glow")), (own, Some(&Value::Number(0.5))));
        assert_eq!(s.users_of(shared), [a]);
        assert_eq!(s.own_linked(b, SlotId::Sound), Err(EditError::NotLinked));
    }

    #[test]
    fn copy_owned_starters_gives_the_copy_its_own() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let look = s.add_starter(&r, a, "look.plain").unwrap();
        s.library.modules.get_mut(&look).unwrap().params.set("glow", Value::Number(3.0));
        let shared = s.add_module(&r, "sound.pluck", "Shared").unwrap();
        s.link(&r, a, SlotId::Sound, shared).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.things.get_mut(&b).unwrap().name = "Copy".into();
        s.things.get_mut(&b).unwrap().slots = s.things[&a].slots.clone();
        s.copy_owned_starters(a, b);
        let SlotLink::Linked { module: copy, .. } = s.things[&b].slots[&SlotId::Look] else { panic!("linked") };
        assert_ne!(copy, look);
        let module = &s.library.modules[&copy];
        assert_eq!((module.name.as_str(), module.owner), ("Copy look", Some(b)));
        assert_eq!(module.params.get("glow"), Some(&Value::Number(3.0)));
        assert_eq!(s.users_of(shared), [a, b], "modules it doesn't own stay shared");
    }

    #[test]
    fn link_refuses_a_module_kind_the_thing_does_not_accept() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let sealed = s.add_thing(&r, "sealed", ThingId::ROOT).unwrap();
        let pluck = s.add_module(&r, "sound.pluck", "Pluck").unwrap();
        assert_eq!(s.link(&r, sealed, SlotId::Sound, pluck), Err(EditError::NotAccepted));
        assert!(!s.things[&sealed].slots.contains_key(&SlotId::Sound));
        let balls = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!(s.link(&r, balls, SlotId::Sound, pluck), Ok(()));
    }

    #[test]
    fn add_thing_copes_with_odd_names_and_parents() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.things.get_mut(&a).unwrap().name = "Balls 4294967295".into();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!(s.things[&b].name, "Balls 4294967296");
        assert!(matches!(s.add_thing(&r, "balls", ThingId(999)), Err(EditError::Missing(ThingId(999)))));
    }

    #[test]
    fn slots_the_kind_lacks_are_refused() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert!(matches!(s.add_slot(&r, b, SlotId::Shader), Err(EditError::SlotNotSupported(SlotId::Shader))));
        assert!(s.add_slot(&r, b, SlotId::Trail).is_ok() && s.things[&b].slots[&SlotId::Trail] == SlotLink::Inherit);
        let m = s.add_module(&r, "look.plain", "Plain").unwrap();
        s.library.modules.get_mut(&m).unwrap().slot = SlotId::Shader;
        assert!(matches!(s.link(&r, b, SlotId::Shader, m), Err(EditError::SlotNotSupported(SlotId::Shader))));
        assert!(matches!(s.link(&r, b, SlotId::Look, ModuleId(9)), Err(EditError::MissingModule(ModuleId(9)))));
    }

    #[test]
    fn set_attach_keeps_the_place_among_siblings() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let ride = Attach::Ride { offset: glam::Vec2::new(1.0, 2.0), turn_with_parent: true, item: Some(3) };
        s.set_attach(a, ride).unwrap();
        assert_eq!((s.things[&a].attach, s.order.clone()), (ride, vec![ThingId::ROOT, a, b]));
        assert!(matches!(s.set_attach(ThingId::ROOT, ride), Err(EditError::Root)));
    }

    #[test]
    fn riding_on_a_descendant_is_refused() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        assert!(matches!(s.set_parent(f, b, Attach::Group), Err(EditError::WouldLoop)));
        assert!(matches!(s.set_parent(ThingId::ROOT, f, Attach::Group), Err(EditError::Root)));
    }

    #[test]
    fn set_parent_moves_the_whole_subtree() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let g = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.set_parent(f, g, Attach::Group).unwrap();
        assert_eq!(s.order, vec![ThingId::ROOT, g, f, b]);
    }

    #[test]
    fn move_before_reorders_siblings_with_their_children() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let g = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        s.move_before(g, f).unwrap();
        assert_eq!(s.order, vec![ThingId::ROOT, g, f, b]);
        assert!(matches!(s.move_before(b, g), Err(EditError::Missing(_))));
    }

    #[test]
    fn remove_thing_takes_the_subtree() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let f = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", f).unwrap();
        let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        assert_eq!(s.remove_thing(f).unwrap(), vec![f, b]);
        assert_eq!(s.order, vec![ThingId::ROOT, c]);
        assert!(matches!(s.remove_thing(ThingId::ROOT), Err(EditError::Root)));
    }

    #[test]
    fn ids_are_not_reused_after_reload() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.remove_thing(a).unwrap();
        let mut back = crate::save::load(&crate::save::save(&s), &r).unwrap();
        assert_ne!(back.add_thing(&r, "balls", ThingId::ROOT).unwrap(), a);
    }

    #[test]
    fn make_folder_gathers_the_selection_in_place() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let d = s.add_thing(&r, "balls", c).unwrap();
        let f = s.make_folder(&r, &[b, c, ThingId::ROOT, d]).unwrap();
        assert_eq!(s.things[&f].kind, "folder");
        assert_eq!(s.order, vec![ThingId::ROOT, a, f, b, c, d]);
        assert_eq!((s.things[&b].parent, s.things[&d].parent), (Some(f), Some(c)));
        assert!(matches!(s.make_folder(&r, &[ThingId::ROOT]), Err(EditError::Root)));
        assert!(matches!(s.make_folder(&r, &[]), Err(EditError::Root)));
    }

    #[test]
    fn make_folder_keeps_riders_riding_through_the_folder() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let arena = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let c = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let ride = |offset: f32, item| Attach::Ride { offset: Vec2::new(offset, 0.0), turn_with_parent: true, item };
        s.set_parent(b, arena, ride(10.0, Some(3))).unwrap();
        s.set_parent(c, arena, ride(20.0, Some(3))).unwrap();
        let f = s.make_folder(&r, &[b, c]).unwrap();
        // Both rode ball 3 of the arena: the folder rides that ball and they ride the folder.
        assert_eq!(s.things[&f].parent, Some(arena));
        assert_eq!(s.things[&f].attach, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: Some(3) });
        assert_eq!((s.things[&b].parent, s.things[&b].attach), (Some(f), ride(10.0, None)));
        assert_eq!((s.things[&c].parent, s.things[&c].attach), (Some(f), ride(20.0, None)));
        // Riders of different balls keep their own items; the folder rides the parent's place.
        let d = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let e = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.set_parent(d, arena, ride(1.0, Some(1))).unwrap();
        s.set_parent(e, arena, ride(2.0, Some(2))).unwrap();
        let g = s.make_folder(&r, &[d, e]).unwrap();
        assert_eq!(s.things[&g].attach, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: None });
        assert_eq!((s.things[&d].attach, s.things[&e].attach), (ride(1.0, Some(1)), ride(2.0, Some(2))));
        // Grouped things stay grouped, and a folder at the top level never rides.
        let h = s.make_folder(&r, &[arena]).unwrap();
        assert_eq!((s.things[&h].attach, s.things[&arena].attach), (Attach::Group, Attach::Group));
    }

    #[test]
    fn library_modules_can_be_renamed_and_duplicated() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let m = s.add_module(&r, "look.plain", "Neon").unwrap();
        s.library.modules.get_mut(&m).unwrap().params.set("glow", Value::Number(1.0));
        s.rename_module(m, "Neon glow").unwrap();
        assert_eq!(s.library.modules[&m].name, "Neon glow");
        let copy = s.duplicate_module(m).unwrap();
        assert_eq!(s.library.modules[&copy].name, "Neon glow copy");
        assert_eq!(s.library.modules[&copy].params, s.library.modules[&m].params);
        assert_eq!(s.library.modules[&copy].id, copy);
        assert!(s.users_of(copy).is_empty());
        assert!(matches!(s.rename_module(ModuleId(9), "x"), Err(EditError::MissingModule(_))));
        assert!(matches!(s.duplicate_module(ModuleId(9)), Err(EditError::MissingModule(_))));
    }

    #[test]
    fn set_override_writes_the_things_own_change() {
        let (r, mut s, a, _, m) = shared_look();
        s.set_override(&r, a, SlotId::Look, "glow", Value::Number(0.4)).unwrap();
        assert_eq!(s.linked(a, SlotId::Look).unwrap().1.get("glow"), Some(&Value::Number(0.4)));
        assert_eq!(s.library.modules[&m].params.get("glow"), None);
    }

    fn shared_look() -> (Registry, Scene, ThingId, ThingId, ModuleId) {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "look.plain", "Neon glow").unwrap();
        s.link(&r, a, SlotId::Look, m).unwrap();
        s.link(&r, b, SlotId::Look, m).unwrap();
        (r, s, a, b, m)
    }

    fn override_glow(s: &mut Scene, id: ThingId, v: f64) {
        if let Some(SlotLink::Linked { overrides, .. }) = s.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look) {
            overrides.set("glow", Value::Number(v));
        }
    }

    #[test]
    fn make_unique_copies_with_overrides_baked_in() {
        let (_, mut s, a, b, m) = shared_look();
        override_glow(&mut s, a, 0.7);
        let copy = s.make_unique(a, SlotId::Look).unwrap();
        assert_ne!(copy, m);
        assert_eq!(s.library.modules[&copy].name, "Neon glow (Balls)");
        assert_eq!(s.library.modules[&copy].params.get("glow"), Some(&Value::Number(0.7)));
        assert_eq!(s.users_of(m), vec![b]);
    }

    #[test]
    fn push_to_module_changes_every_user() {
        let (_, mut s, a, _, m) = shared_look();
        override_glow(&mut s, a, 0.7);
        s.push_to_module(a, SlotId::Look).unwrap();
        assert_eq!(s.library.modules[&m].params.get("glow"), Some(&Value::Number(0.7)));
    }

    #[test]
    fn modules_in_use_cannot_be_removed() {
        let (_, mut s, a, b, m) = shared_look();
        assert!(matches!(s.remove_module(m), Err(EditError::InUse(u)) if u == vec![a, b]));
        assert_eq!(EditError::InUse(vec![a]).to_string(), "One thing still uses this module");
        assert_eq!(EditError::InUse(vec![a, b]).to_string(), "2 things still use this module");
        s.set_slot_inherit(a, SlotId::Look);
        s.set_slot_off(b, SlotId::Look);
        assert_eq!(s.things[&a].slots[&SlotId::Look], SlotLink::Inherit);
        s.remove_module(m).unwrap();
        assert!(s.library.modules.is_empty());
        assert!(matches!(s.remove_module(m), Err(EditError::MissingModule(_))));
    }

    #[test]
    fn reset_override_drops_one_change() {
        let (_, mut s, a, _, _) = shared_look();
        override_glow(&mut s, a, 0.7);
        s.reset_override(a, SlotId::Look, "glow").unwrap();
        assert!(
            matches!(&s.things[&a].slots[&SlotId::Look], SlotLink::Linked { overrides, .. } if overrides.is_empty())
        );
        assert!(matches!(s.reset_override(a, SlotId::Sound, "glow"), Err(EditError::NotLinked)));
    }

    #[test]
    fn make_unique_copies_the_graph_too() {
        let (_, mut s, a, _, m) = shared_look();
        let graph = crate::ModuleGraph { template: "glow".into(), custom: true, ..Default::default() };
        s.library.modules.get_mut(&m).unwrap().graph = Some(graph.clone());
        let copy = s.make_unique(a, SlotId::Look).unwrap();
        assert_eq!(s.library.modules[&copy].graph, Some(graph));
        assert_eq!(s.library.modules[&copy].kind, "look.plain");
    }

    #[test]
    fn linking_into_the_wrong_slot_is_refused() {
        let (r, mut s, a, _, m) = shared_look();
        assert!(matches!(s.link(&r, a, SlotId::Sound, m), Err(EditError::WrongSlot)));
    }

    // Three look kinds that share `glow`: A also has `ring`; B has a template; C has neither.
    #[derive(crate::Settings)]
    struct LookASettings {
        #[setting(label = "Glow", range(0, 9))]
        glow: f32,
        #[setting(label = "Ring", range(0, 9))]
        ring: f32,
    }
    impl Default for LookASettings {
        fn default() -> Self {
            Self { glow: 0.0, ring: 0.0 }
        }
    }
    #[derive(crate::Settings)]
    struct LookBSettings {
        #[setting(label = "Glow", range(0, 9))]
        glow: f32,
    }
    impl Default for LookBSettings {
        fn default() -> Self {
            Self { glow: 0.0 }
        }
    }
    struct LookA;
    struct LookB;
    struct LookC;
    macro_rules! look_kind {
        ($t:ident, $id:literal, $s:ty $(, $tpl:expr)?) => {
            impl crate::ModuleKind for $t {
                fn id(&self) -> &'static str {
                    $id
                }
                fn name(&self) -> &'static str {
                    $id
                }
                fn help(&self) -> &'static str {
                    "A test look."
                }
                fn slot(&self) -> SlotId {
                    SlotId::Look
                }
                fn settings(&self) -> &'static [crate::SettingDef] {
                    <$s as crate::Settings>::defs()
                }
                $(fn template(&self) -> Option<bpe_graph::Graph> {
                    Some($tpl)
                })?
            }
        };
    }
    look_kind!(LookA, "look.a", LookASettings);
    look_kind!(LookB, "look.b", LookBSettings, bpe_graph::Graph::new("B template"));
    look_kind!(LookC, "look.c", LookBSettings);

    /// Two balls sharing a module of kind A with `glow` 1 and `ring` 2.
    fn switchable() -> (Registry, Scene, ThingId, ThingId, ModuleId) {
        let mut r = testkit::registry();
        r.add_module(LookA);
        r.add_module(LookB);
        r.add_module(LookC);
        let mut s = Scene::empty(&r);
        let t1 = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let t2 = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "look.a", "Shared").unwrap();
        s.link(&r, t1, SlotId::Look, m).unwrap();
        s.link(&r, t2, SlotId::Look, m).unwrap();
        let p = &mut s.library.modules.get_mut(&m).unwrap().params;
        p.set("glow", Value::Number(1.0));
        p.set("ring", Value::Number(2.0));
        (r, s, t1, t2, m)
    }

    fn params_of(pairs: &[(&str, f64)]) -> Params {
        let mut p = Params::default();
        for (k, v) in pairs {
            p.set(k, Value::Number(*v));
        }
        p
    }

    fn module_addr(m: ModuleId, key: &str) -> Address {
        Address { owner: Owner::Module(m), slot: None, key: key.into() }
    }

    fn thing_addr(t: ThingId, key: &str) -> Address {
        Address { owner: Owner::Thing(t), slot: Some(SlotId::Look), key: key.into() }
    }

    fn track_at(v: f64) -> crate::Track {
        crate::Track {
            keys: vec![crate::animation::Key { time: 1.0, value: Value::Number(v), ease: Default::default() }],
            range: None,
        }
    }

    #[test]
    fn set_module_kind_keeps_shared_keys_and_stashes_the_rest() {
        let (r, mut s, _, _, m) = switchable();
        s.set_module_kind(&r, m, "look.b").unwrap();
        let module = &s.library.modules[&m];
        assert_eq!(module.kind, "look.b");
        assert_eq!(module.params, params_of(&[("glow", 1.0)]));
        assert_eq!(module.stash["look.a"].params, params_of(&[("ring", 2.0)]));
    }

    #[test]
    fn switching_back_restores_the_stash_but_newer_shared_values_win() {
        let (r, mut s, _, _, m) = switchable();
        s.set_module_kind(&r, m, "look.b").unwrap();
        s.library.modules.get_mut(&m).unwrap().params.set("glow", Value::Number(5.0));
        s.set_module_kind(&r, m, "look.a").unwrap();
        let module = &s.library.modules[&m];
        assert_eq!(module.params, params_of(&[("glow", 5.0), ("ring", 2.0)]));
        assert!(!module.stash.contains_key("look.a"));
    }

    #[test]
    fn keyframes_pulses_and_locks_on_dropped_keys_are_stashed() {
        let (r, mut s, _, _, m) = switchable();
        let a = module_addr(m, "ring");
        let keep = module_addr(m, "glow");
        let pulse = crate::Pulse { decay: 0.5, ..Default::default() };
        s.tracks.insert(a.clone(), track_at(3.0));
        s.tracks.insert(keep.clone(), track_at(4.0));
        s.pulses.insert(a.clone(), pulse.clone());
        s.locks.insert(a.clone());
        s.set_module_kind(&r, m, "look.b").unwrap();
        assert_eq!(s.tracks.keys().collect::<Vec<_>>(), vec![&keep]);
        assert!(s.pulses.is_empty() && s.locks.is_empty());
        s.set_module_kind(&r, m, "look.a").unwrap();
        assert_eq!(s.tracks[&a], track_at(3.0));
        assert_eq!(s.tracks[&keep], track_at(4.0));
        assert_eq!(s.pulses[&a], pulse);
        assert!(s.locks.contains(&a));
    }

    #[test]
    fn switching_a_shared_module_stashes_each_things_overrides_and_their_keys() {
        let (r, mut s, t1, t2, m) = switchable();
        for (t, key, v) in [(t1, "ring", 7.0), (t1, "glow", 8.0), (t2, "ring", 9.0)] {
            if let Some(SlotLink::Linked { overrides, .. }) = s.things.get_mut(&t).unwrap().slots.get_mut(&SlotId::Look)
            {
                overrides.set(key, Value::Number(v));
            }
        }
        let ring = thing_addr(t1, "ring");
        let glow = thing_addr(t1, "glow");
        s.tracks.insert(ring.clone(), track_at(3.0));
        s.tracks.insert(glow.clone(), track_at(4.0));
        s.pulses.insert(ring.clone(), crate::Pulse::default());
        s.locks.insert(thing_addr(t2, "ring"));
        s.set_module_kind(&r, m, "look.b").unwrap();
        let ov = |s: &Scene, t: ThingId| match &s.things[&t].slots[&SlotId::Look] {
            SlotLink::Linked { overrides, .. } => overrides.clone(),
            _ => panic!("not linked"),
        };
        assert_eq!(ov(&s, t1), params_of(&[("glow", 8.0)]));
        assert_eq!(ov(&s, t2), Params::default());
        assert_eq!(s.tracks.keys().collect::<Vec<_>>(), vec![&glow]);
        assert!(s.pulses.is_empty() && s.locks.is_empty());
        s.set_module_kind(&r, m, "look.a").unwrap();
        assert_eq!(ov(&s, t1), params_of(&[("glow", 8.0), ("ring", 7.0)]));
        assert_eq!(ov(&s, t2), params_of(&[("ring", 9.0)]));
        assert_eq!(s.tracks[&ring], track_at(3.0));
        assert!(s.pulses.contains_key(&ring));
        assert!(s.locks.contains(&thing_addr(t2, "ring")));
        assert!(s.library.modules[&m].stash.is_empty());
    }

    #[test]
    fn a_stash_survives_a_switch_to_a_third_kind() {
        let (r, mut s, _, _, m) = switchable();
        s.set_module_kind(&r, m, "look.b").unwrap();
        s.set_module_kind(&r, m, "look.c").unwrap();
        s.set_module_kind(&r, m, "look.a").unwrap();
        assert_eq!(s.library.modules[&m].params, params_of(&[("glow", 1.0), ("ring", 2.0)]));
    }

    #[test]
    fn a_custom_graph_is_dropped_and_a_template_one_replaced() {
        let (r, mut s, _, _, m) = switchable();
        let graph = |custom| Some(crate::ModuleGraph { template: "look.a".into(), custom, ..Default::default() });
        s.library.modules.get_mut(&m).unwrap().graph = graph(true);
        s.set_module_kind(&r, m, "look.b").unwrap();
        assert_eq!(s.library.modules[&m].graph, None);
        s.set_module_kind(&r, m, "look.a").unwrap();
        s.library.modules.get_mut(&m).unwrap().graph = graph(false);
        s.set_module_kind(&r, m, "look.b").unwrap();
        let g = s.library.modules[&m].graph.clone().unwrap();
        assert_eq!((g.template.as_str(), g.custom), ("look.b", false));
        assert_eq!(g.graph, bpe_graph::Graph::new("B template"));
        s.set_module_kind(&r, m, "look.c").unwrap();
        assert_eq!(s.library.modules[&m].graph, None);
    }

    #[test]
    fn a_kind_of_another_slot_is_refused() {
        let (r, mut s, _, _, m) = switchable();
        let before = s.clone();
        assert!(matches!(s.set_module_kind(&r, m, "sound.pluck"), Err(EditError::WrongSlot)));
        assert!(matches!(s.set_module_kind(&r, m, "nope"), Err(EditError::UnknownKind(_))));
        assert!(matches!(s.set_module_kind(&r, ModuleId(99), "look.b"), Err(EditError::MissingModule(_))));
        assert_eq!(s, before);
    }

    #[test]
    fn a_stash_round_trips_through_ron() {
        let (r, mut s, t1, _, m) = switchable();
        s.tracks.insert(thing_addr(t1, "ring"), track_at(3.0));
        s.locks.insert(thing_addr(t1, "ring"));
        s.set_module_kind(&r, m, "look.b").unwrap();
        let back: Scene = ron::from_str(&ron::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
