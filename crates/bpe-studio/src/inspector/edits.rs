//! The changes the inspector asks for ([`Edit`]), applied once the drawing is done, and how a
//! setting is written (as its value, or as a key at the playhead).

use super::*;

/// A change the panel asks for, applied once the drawing is done.
pub(crate) enum Edit {
    Name(String),
    /// The thing's track colour; only the primary thing's.
    Colour(TrackColour),
    Tags(BTreeSet<String>),
    Enabled(bool),
    /// Drawn on the screen or in the world; `None` is the kind's own choice.
    Screen(Option<bool>),
    Transform(Transform),
    /// What draws over what: higher over lower.
    ZOrder(i32),
    Attach(Attach),
    /// One of the kind's own settings, for every selected thing of the kind.
    SetOwn(String, Value),
    /// One of the primary thing's own settings, for it alone: what was dragged in its Group view.
    SetOwnHere(String, Value),
    /// The card in a slot becomes another kind, for every selected thing whose card there is of
    /// the same kind.
    SetKind(SlotId, &'static str),
    /// One of a card's settings, for every selected thing whose card in the slot is of the same
    /// kind.
    /// A layer's setting (a layered key, "<layer>/<key>") reaches only the selected things whose
    /// card has a layer of that id and the same kind.
    SetSlot(SlotId, String, Value),
    /// A key at the playhead holding the setting's value as it stands (a slot's setting, or the
    /// thing's own with `None`), for every selected thing that has the setting.
    NewKey(Option<SlotId>, String),
    /// Every key of the setting goes (it keeps the value it has at the playhead), for every
    /// selected thing that has the setting.
    ClearKeys(Option<SlotId>, String),
    /// The setting (a slot's, or the thing's own with `None`) follows a part of a shape's sound,
    /// or stops following with `None`, for every selected thing that has the setting.
    Follow(Option<SlotId>, String, Option<bpe_core::PulseSource>),
    /// The setting (a slot's, or the thing's own with `None`) starts varying across the balls
    /// (true) or stops (false), for every selected thing that has the setting.
    Vary(Option<SlotId>, String, bool),
    /// A card for a slot the thing doesn't have yet, with its kind's starting settings, for
    /// every selected thing of the kind without one.
    AddSlot(SlotId),
    /// Takes the card off every selected thing whose card in the slot is of the same kind.
    RemoveSlot(SlotId),
    /// A change to the scene's lighting scenes (the root's Lighting card).
    LightScene(crate::light_scenes_panel::SceneEdit),
    // The layers of a layered card. These change the primary thing's card only, and select the
    // layer they add.
    AddLayer(SlotId, &'static str),
    RemoveLayer(SlotId, LayerId),
    /// A sub-layer of the kind named on a layer, last of its sub-layers.
    AddSubLayer(SlotId, LayerId, &'static str),
    /// A sub-layer to the place given among its layer's sub-layers, 0 the first.
    MoveSubLayer(SlotId, LayerId, usize),
    /// A copied sub-layer onto a layer, last of its sub-layers; then, with `Some(at)`, moved to
    /// that place among them.
    PasteSubLayer(SlotId, LayerId, CopiedLayer, Option<usize>),
    /// To the index given, counted from the bottom of the card's layers.
    MoveLayer(SlotId, LayerId, usize),
    DuplicateLayer(SlotId, LayerId),
    RenameLayer(SlotId, LayerId, String),
    LayerEnabled(SlotId, LayerId, bool),
    PasteLayer(SlotId, CopiedLayer),
    PasteLook(SlotId, CopiedLook),
    /// The balls of the group that wear a drawn layer, by number; none is every ball.
    LayerOnly(SlotId, LayerId, Vec<u32>),
    /// A new drawn layer that only these balls of the group wear.
    AddLayerFor(SlotId, &'static str, Vec<u32>),
    /// A font file the scene keeps, under the name given (or the next free one like it), which
    /// every selected Text is then set in.
    LoadFont(String, std::sync::Arc<[u8]>),
    /// The scene forgets its font of this name.
    RemoveFont(String),
    /// A PNG the scene keeps, under the name given (or the next free one like it), which every
    /// selected thing's Picture then shows.
    LoadPicture(String, std::sync::Arc<[u8]>),
}

/// Whether `a` is a part of a vary (its To, spread or easing) on a setting that doesn't vary
/// there: an edit to one thing's varied setting leaves the other selected things' alone.
pub(super) fn no_vary_here(scene: &Scene, a: &Address) -> bool {
    bpe_core::vary::split(&a.key)
        .is_some_and(|(base, _)| scene.vary_at(&Address { key: base.to_owned(), ..a.clone() }).is_none())
}

/// The things an edit to `id`'s setting reaches: every selected thing of `id`'s kind for its own
/// settings (`slot` is `None`), or with a card of the same kind in `slot`.
pub(super) fn reached(scene: &Scene, selected: &[ThingId], id: ThingId, slot: Option<SlotId>) -> Vec<ThingId> {
    let what = |t: &ThingId| -> Option<&str> {
        let thing = scene.things.get(t)?;
        match slot {
            None => Some(thing.kind.as_str()),
            Some(slot) => thing.slots.get(&slot).map(|c| c.kind.as_str()),
        }
    };
    let Some(mine) = what(&id) else { return Vec::new() };
    selected.iter().copied().filter(|t| what(t) == Some(mine)).collect()
}

/// The things an edit to `id`'s setting `key` in `slot` reaches: as [`reached`], and for a
/// layer's setting only those whose card has a layer of the same id and kind.
pub(super) fn reached_key(
    scene: &Scene,
    selected: &[ThingId],
    id: ThingId,
    slot: Option<SlotId>,
    key: &str,
) -> Vec<ThingId> {
    let mut out = reached(scene, selected, id, slot);
    if let (Some(slot), Some((layer, _))) = (slot, bpe_core::split_layered(key)) {
        let kind_of = |t: &ThingId| Some(scene.card(*t, slot)?.layer(layer)?.kind.clone());
        let mine = kind_of(&id);
        out.retain(|t| mine.is_some() && kind_of(t) == mine);
    }
    out
}

/// Applies `edits` made on thing `id` to it and to the `selected` things they reach, values at
/// `time`. True when the scene changed.
pub(super) fn apply(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    inspector: &mut Inspector,
    (id, selected): (ThingId, &[ThingId]),
    time: f64,
    edits: Vec<Edit>,
) -> bool {
    let mut changed = false;
    for edit in edits {
        changed |= match edit {
            Edit::Name(name) => {
                scene.set_thing_name(id, &name);
                true
            }
            Edit::Colour(colour) => set_thing(scene, id, |t| t.colour = Some(colour)),
            Edit::Tags(tags) => set_thing(scene, id, |t| t.tags = tags),
            Edit::Enabled(enabled) => set_thing(scene, id, |t| t.enabled = enabled),
            Edit::Screen(screen) => set_thing(scene, id, |t| t.screen = screen),
            Edit::Transform(t) => set_thing(scene, id, |thing| thing.transform = t),
            Edit::ZOrder(z) => set_thing(scene, id, |thing| thing.z_order = z),
            Edit::Attach(attach) => crate::tree::reattach(scene, id, attach).is_ok(),
            Edit::SetOwn(key, value) => {
                let mut any = false;
                for t in reached(scene, selected, id, None) {
                    let a = Address::own(t, &key);
                    if no_vary_here(scene, &a) {
                        continue;
                    }
                    let set = set_or_key(scene, reg, &a, value.clone(), time, &mut inspector.keyed_edit);
                    any |= set;
                    if set {
                        any |= shape_afresh(scene, t, &key);
                    }
                }
                any
            }
            Edit::LoadFont(name, bytes) => {
                let name = scene.add_font(&name, bytes);
                for t in reached(scene, selected, id, None) {
                    set_or_key(
                        scene,
                        reg,
                        &Address::own(t, "font"),
                        Value::Text(name.clone()),
                        time,
                        &mut inspector.keyed_edit,
                    );
                }
                true
            }
            Edit::RemoveFont(name) => scene.remove_font(&name),
            Edit::LoadPicture(name, bytes) => {
                let name = scene.add_picture(&name, bytes);
                let key = bpe_core::PICTURE_KEY;
                for t in reached_key(scene, selected, id, None, key) {
                    let a = Address::own(t, key);
                    set_or_key(scene, reg, &a, Value::Text(name.clone()), time, &mut inspector.keyed_edit);
                }
                true
            }
            Edit::SetOwnHere(key, value) => {
                set_or_key(scene, reg, &Address::own(id, &key), value, time, &mut inspector.keyed_edit)
            }
            Edit::SetKind(slot, kind) => {
                let mut any = false;
                for t in reached(scene, selected, id, Some(slot)) {
                    any |= scene.set_slot_kind(reg, t, slot, kind).is_ok();
                }
                any
            }
            Edit::SetSlot(slot, key, value) => {
                let mut any = false;
                for t in reached_key(scene, selected, id, Some(slot), &key) {
                    let a = Address::on(t, slot, &key);
                    if no_vary_here(scene, &a) {
                        continue;
                    }
                    any |= set_or_key(scene, reg, &a, value.clone(), time, &mut inspector.keyed_edit);
                }
                any
            }
            Edit::NewKey(slot, key) => {
                let mut any = false;
                for t in reached_key(scene, selected, id, slot, &key) {
                    let a = Address { thing: t, slot, key: key.clone() };
                    if no_vary_here(scene, &a) {
                        continue;
                    }
                    any |= new_key(scene, reg, &a, time);
                }
                any
            }
            Edit::ClearKeys(slot, key) => {
                let mut any = false;
                for t in reached_key(scene, selected, id, slot, &key) {
                    let a = Address { thing: t, slot, key: key.clone() };
                    any |= clear_keys(scene, reg, &a, time);
                }
                any
            }
            Edit::Follow(slot, key, source) => {
                let mut any = false;
                for t in reached_key(scene, selected, id, slot, &key) {
                    let a = Address { thing: t, slot, key: key.clone() };
                    any |= crate::follow::set(scene, reg, &a, source);
                }
                any
            }
            Edit::Vary(slot, key, on) => {
                let mut any = false;
                for t in reached_key(scene, selected, id, slot, &key) {
                    let a = Address { thing: t, slot, key: key.clone() };
                    any |= if on {
                        // Only things with balls to vary across (a single ball, a shape, never).
                        crate::vary::offered(scene, t)
                            && scene.vary_at(&a).is_none()
                            && scene.start_varying(reg, &a).is_ok()
                    } else {
                        scene.stop_varying(&a)
                    };
                }
                any
            }
            Edit::AddSlot(slot) => {
                let mut any = false;
                for t in reached(scene, selected, id, None) {
                    if !scene.things[&t].slots.contains_key(&slot) {
                        any |= bpe_kit::fill_slot(scene, reg, t, slot).is_ok();
                    }
                }
                any
            }
            Edit::RemoveSlot(slot) => {
                let mut any = false;
                for t in reached(scene, selected, id, Some(slot)) {
                    any |= scene.remove_slot(t, slot);
                }
                any
            }
            Edit::LightScene(e) => {
                // A rename names its row by its place, which another edit could move.
                inspector.renaming_scene = None;
                crate::light_scenes_panel::apply(scene, reg, e, time)
            }
            edit => apply_layer_edit(scene, reg, inspector, id, edit),
        };
    }
    changed
}

/// After a shape's own setting `key` was set: a new Sides or Start from starts the outline afresh,
/// and an emptied outline is taken away rather than kept. In the same edit, so one undo brings
/// the moulded outline back. True when it took one away.
pub(super) fn shape_afresh(scene: &mut Scene, id: ThingId, key: &str) -> bool {
    let Some(thing) = scene.things.get_mut(&id).filter(|t| t.kind == SHAPE) else { return false };
    let emptied = matches!(thing.params.get("path"), Some(Value::Path(p)) if p.is_empty());
    if matches!(key, "sides" | "start") || (key == "path" && emptied) {
        thing.params.remove("path").is_some()
    } else {
        false
    }
}

/// Applies a change to the layers of `id`'s card, selecting the layer it adds. True when the
/// scene changed.
pub(super) fn apply_layer_edit(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    inspector: &mut Inspector,
    id: ThingId,
    edit: Edit,
) -> bool {
    // The card's own pick ([`crate::look_editor::pick_key`]).
    let key = match &edit {
        Edit::AddLayer(slot, ..)
        | Edit::AddSubLayer(slot, ..)
        | Edit::DuplicateLayer(slot, ..)
        | Edit::PasteLayer(slot, ..)
        | Edit::PasteSubLayer(slot, ..)
        | Edit::AddLayerFor(slot, ..) => crate::look_editor::pick_key(id, *slot),
        _ => id,
    };
    let added = match edit {
        Edit::AddLayer(slot, kind) => scene.add_layer(reg, id, slot, kind).map(|layer| {
            // The first Follow the air force gives the scene its Air card, so it can be found.
            if bpe_kit::things::particles::air::wants_air_card(kind) {
                let _ = bpe_kit::things::particles::air::ensure_air_card(scene, reg);
            }
            Some(layer)
        }),
        Edit::RemoveLayer(slot, layer) => scene.remove_layer(reg, id, slot, layer).map(|_| None),
        Edit::AddSubLayer(slot, parent, kind) => scene.add_sub_layer(reg, id, slot, parent, kind).map(Some),
        Edit::MoveSubLayer(slot, layer, to) => scene.move_sub_layer(id, slot, layer, to).map(|_| None),
        Edit::PasteSubLayer(slot, parent, copied, at) => {
            scene.paste_sub_layer(reg, id, slot, parent, &copied).and_then(|sub| {
                if let Some(at) = at {
                    scene.move_sub_layer(id, slot, sub, at)?;
                }
                Ok(Some(sub))
            })
        }
        Edit::MoveLayer(slot, layer, to) => scene.move_layer(reg, id, slot, layer, to).map(|_| None),
        Edit::DuplicateLayer(slot, layer) => scene.duplicate_layer(reg, id, slot, layer).map(Some),
        Edit::RenameLayer(slot, layer, name) => scene.rename_layer(reg, id, slot, layer, &name).map(|_| None),
        Edit::LayerEnabled(slot, layer, on) => scene.set_layer_enabled(reg, id, slot, layer, on).map(|_| None),
        Edit::PasteLayer(slot, copied) => scene.paste_layer(reg, id, slot, &copied).map(Some),
        Edit::PasteLook(slot, copied) => {
            // The old layers are gone, so the body is selected.
            inspector.selected_layer.remove(&id);
            scene.paste_look(reg, id, slot, &copied).map(|_| None)
        }
        Edit::LayerOnly(slot, layer, only) => scene.set_layer_only(reg, id, slot, layer, only).map(|_| None),
        Edit::AddLayerFor(slot, kind, only) => scene.add_layer(reg, id, slot, kind).and_then(|layer| {
            scene.set_layer_only(reg, id, slot, layer, only)?;
            Ok(Some(layer))
        }),
        _ => unreachable!("only layer edits come here"),
    };
    match added {
        Ok(Some(layer)) => {
            // A group layer is picked in Group mode, any other in the ball's lists.
            let card = scene.things.get(&id).and_then(|t| t.slots.values().find(|c| c.layer(layer).is_some()));
            let kind = card.and_then(|c| c.layer(layer)).and_then(|l| reg.layer(&l.kind));
            if kind.is_some_and(|k| k.section() == bpe_core::LayerSection::Group) {
                inspector.selected_group_layer.insert(id, layer);
            } else {
                inspector.selected_layer.insert(key, layer);
            }
            true
        }
        Ok(None) => true,
        Err(_) => false,
    }
}

/// Writes `value` to `a`: as a key at `time` when the setting has keys (and notes so in
/// `keyed`), else as its plain value.
pub(crate) fn set_or_key(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    a: &Address,
    value: Value,
    time: f64,
    keyed: &mut bool,
) -> bool {
    if scene.keyed(a) {
        *keyed = true;
        scene.set_key(reg, a, time, value).is_ok()
    } else {
        scene.set_at(reg, a, value).is_ok()
    }
}

/// What a setting's keys look like, for its label and its menu.
pub(super) fn mark_for(scene: &Scene, def: &SettingDef, a: &Address) -> KeyMark {
    KeyMark { can_key: def.flags.keyframe, keyed: scene.keyed(a), pulsed: scene.pulses.contains_key(a) }
}

/// A key at `time` holding the setting's value there, replacing a key already at that time.
/// Removes every key of the setting at `a`, leaving the value it has at `time`.
pub(crate) fn clear_keys(scene: &mut Scene, reg: &bpe_core::Registry, a: &Address, time: f64) -> bool {
    scene.clear_keys(reg, a, time)
}

pub(super) fn new_key(scene: &mut Scene, reg: &bpe_core::Registry, a: &Address, time: f64) -> bool {
    let Some(value) = scene.value_at_time(reg, a, time) else { return false };
    scene.set_key(reg, a, time, value).is_ok()
}

pub(super) fn set_thing(scene: &mut Scene, id: ThingId, f: impl FnOnce(&mut bpe_core::Thing)) -> bool {
    match scene.things.get_mut(&id) {
        Some(t) => {
            f(t);
            true
        }
        None => false,
    }
}
