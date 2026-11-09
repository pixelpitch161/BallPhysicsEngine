//! Reading the scene's Grade card at the playhead, and the changes Grade mode asks for
//! ([`GradeEdit`]), applied once everything is drawn ([`apply`]). The scene gets its Grade card
//! with the first change (`ensure_grade`); until then the column reads the starting card, whose
//! layer ids the new card has too.

use crate::sound_tab::write::{WriteOutcome, write};
use bpe_core::{Address, LayerId, PulseSource, Scene, SettingDef, Slot, SlotId, ThingId, Value, layered_key};
use bpe_kit::Kit;
use bpe_kit::modules::grade;

/// The Grade card's setting `key` (a layered key for a panel's setting).
pub fn at(key: &str) -> Address {
    Address::on(ThingId::ROOT, SlotId::Grade, key)
}

/// The scene's Grade card, or the card it would start with.
pub fn card_or_start<'a>(scene: &'a Scene, start: &'a Slot) -> &'a Slot {
    scene.card(ThingId::ROOT, SlotId::Grade).filter(|c| c.kind == grade::GRADE).unwrap_or(start)
}

/// What drawing reads.
pub struct GradeCx<'a> {
    pub scene: &'a Scene,
    pub kit: &'a Kit,
    /// The card as it is (or as it would start).
    pub card: &'a Slot,
    /// The playhead: values show as they are there, and a keyed setting keys there.
    pub time: f64,
}

impl GradeCx<'_> {
    /// The description of layer `layer`'s setting `key` (the card's own with `None`).
    pub fn def(&self, layer: Option<LayerId>, key: &str) -> Option<&'static SettingDef> {
        let reg = &self.kit.registry;
        let defs = match layer {
            None => reg.module(grade::GRADE)?.settings(),
            Some(id) => reg.layer(&self.card.layer(id)?.kind)?.settings(),
        };
        defs.iter().find(|d| d.key == key)
    }

    /// Layer `layer`'s setting `key` at the playhead (its default when it isn't set).
    pub fn get(&self, layer: Option<LayerId>, key: &str) -> Value {
        let full = full_key(layer, key);
        self.scene
            .value_at_time(&self.kit.registry, &at(&full), self.time)
            .or_else(|| self.def(layer, key).map(|d| d.default.clone()))
            .unwrap_or(Value::Bool(false))
    }

    /// Whether the grade is on (the card's `on`).
    pub fn on(&self) -> bool {
        !matches!(self.get(None, "on"), Value::Bool(false))
    }

    /// Whether panel `layer` holds any value, key or pulse of its own (not all at its defaults).
    /// The Tone Curve's Parametric / Point choice is only which curve shows, so it doesn't count.
    pub fn changed(&self, layer: LayerId) -> bool {
        let prefix = format!("{layer}/");
        let set = self.card.params.0.iter().any(|(k, v)| {
            k.strip_prefix(&prefix)
                .is_some_and(|key| key != VIEW_KEY && self.def(Some(layer), key).is_none_or(|d| d.default != *v))
        });
        let animated = self
            .scene
            .tracks
            .keys()
            .chain(self.scene.pulses.keys())
            .any(|a| a.thing == ThingId::ROOT && a.slot == Some(SlotId::Grade) && a.key.starts_with(&prefix));
        set || animated
    }
}

/// The Tone Curve's setting saying which curve shows: kept by Reset panel, not counted as a change.
pub const VIEW_KEY: &str = "mode";

/// `"<layer>/<key>"`, or `key` for the card's own.
pub fn full_key(layer: Option<LayerId>, key: &str) -> String {
    match layer {
        Some(id) => layered_key(id, key),
        None => key.to_owned(),
    }
}

/// A change Grade mode asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum GradeEdit {
    /// A setting of the Grade card (a key at the playhead when it has keys).
    Set(String, Value),
    /// A key at the playhead holding the setting's value as it stands.
    NewKey(String),
    /// Every key of the setting goes; it keeps the value it has now.
    ClearKeys(String),
    /// The setting follows sound, light or health, or stops with `None`.
    Follow(String, Option<PulseSource>),
    /// A new panel of `kind` at the end of mask `.0`'s panels (the Whole picture's too).
    AddPanel(LayerId, &'static str),
    RemovePanel(LayerId),
    /// A panel to place `to` among its mask's panels (0 works first).
    MovePanel(LayerId, usize),
    /// Every setting of a panel (or part) back to its default: its values, keys and pulses go.
    ResetPanel(LayerId),
    /// Mask `.0`'s panels back in Lightroom's order.
    ResetOrder(LayerId),
    /// A panel, part or mask switched on or off (a mask's eye).
    Enable(LayerId, bool),
    /// A Point Colour panel's pick in slot `.1` (0..8) given up: every value, key and pulse of it goes.
    RemovePick(LayerId, usize),
    /// A new mask after the others, with one part of this kind and a Basic panel.
    AddMask(&'static str),
    /// A mask with its parts and panels, gone.
    RemoveMask(LayerId),
    /// A mask to place `to` among the masks after the Whole picture (0 works first).
    MoveMask(LayerId, usize),
    /// A copy of a mask (its parts, panels, keys and pulses) just after it.
    DuplicateMask(LayerId),
    /// A mask (or part or panel) named anew; blank gives back its kind's name.
    Rename(LayerId, String),
    /// A new part of `.1` after mask `.0`'s parts, joined by `.2`.
    AddPart(LayerId, &'static str, grade::Combine),
    RemovePart(LayerId),
    /// A part to place `to` among its mask's parts.
    MovePart(LayerId, usize),
    /// A Colour Range's pick in slot `.1` (0..5) given up.
    RemoveRangePick(LayerId, usize),
}

/// What applying the edits did.
#[derive(Default, Debug, PartialEq)]
pub struct Applied {
    pub changed: bool,
    /// A setting with keys was keyed at the playhead (the editor pauses).
    pub keyed: bool,
    /// The mask a new mask or copy made, to select.
    pub added: Option<LayerId>,
}

/// Applies `edits` to the scene, giving it its Grade card first.
pub fn apply(scene: &mut Scene, kit: &Kit, time: f64, edits: Vec<GradeEdit>) -> Applied {
    let reg = &kit.registry;
    let (root, slot) = (ThingId::ROOT, SlotId::Grade);
    let mut out = Applied::default();
    if edits.is_empty() {
        return out;
    }
    grade::ensure_grade(scene, reg);
    for edit in edits {
        out.changed |= match edit {
            GradeEdit::Set(key, v) => match write(scene, reg, &at(&key), v, time) {
                WriteOutcome::Set => true,
                WriteOutcome::Keyed => {
                    out.keyed = true;
                    true
                }
                WriteOutcome::Failed => false,
            },
            GradeEdit::NewKey(key) => match scene.value_at_time(reg, &at(&key), time) {
                Some(v) => scene.set_key(reg, &at(&key), time, v).is_ok(),
                None => false,
            },
            GradeEdit::ClearKeys(key) => crate::inspector::clear_keys(scene, reg, &at(&key), time),
            GradeEdit::Follow(key, source) => crate::follow::set(scene, reg, &at(&key), source),
            GradeEdit::AddPanel(mask, kind) => grade::add_panel(scene, reg, mask, kind).is_ok(),
            GradeEdit::RemovePanel(id) => {
                let allowed = scene.card(root, slot).is_some_and(|c| grade::can_remove_panel(c, id));
                allowed && scene.remove_layer(reg, root, slot, id).is_ok()
            }
            GradeEdit::MovePanel(id, to) | GradeEdit::MovePart(id, to) => {
                let before = scene.card(root, slot).map(|c| c.layers.clone());
                scene.move_sub_layer_in(reg, root, slot, id, to).is_ok()
                    && scene.card(root, slot).map(|c| &c.layers) != before.as_ref()
            }
            GradeEdit::ResetPanel(id) => reset_panel(scene, id),
            GradeEdit::RemovePick(id, slot) => clear(scene, &format!("{id}/{}", grade::pick_prefix(slot)), None),
            GradeEdit::RemoveRangePick(id, slot) => {
                let used = clear(scene, &format!("{id}/{}", grade::masks::range_key(slot, "used")), None);
                clear(scene, &format!("{id}/{}", grade::masks::range_key(slot, "colour")), None) || used
            }
            GradeEdit::ResetOrder(mask) => grade::reset_order(scene, reg, mask),
            GradeEdit::Enable(id, on) => scene.set_layer_enabled(reg, root, slot, id, on).is_ok(),
            GradeEdit::AddMask(kind) => match grade::add_mask(scene, reg, kind) {
                Ok(id) => {
                    out.added = Some(id);
                    true
                }
                Err(_) => false,
            },
            GradeEdit::RemoveMask(id) => {
                let ours = scene.card(root, slot).and_then(|c| c.layer(id)).is_some_and(|l| l.kind == grade::MASK);
                ours && scene.remove_layer(reg, root, slot, id).is_ok()
            }
            GradeEdit::RemovePart(id) => {
                let kind = scene.card(root, slot).and_then(|c| c.layer(id)).map(|l| l.kind.clone());
                let ours = kind.is_some_and(|k| grade::PART_KINDS.contains(&k.as_str()));
                ours && scene.remove_layer(reg, root, slot, id).is_ok()
            }
            GradeEdit::MoveMask(id, to) => {
                let before = scene.card(root, slot).map(|c| c.layers.clone());
                scene.move_layer(reg, root, slot, id, to + 1).is_ok()
                    && scene.card(root, slot).map(|c| &c.layers) != before.as_ref()
            }
            GradeEdit::DuplicateMask(id) => match scene.duplicate_layer(reg, root, slot, id) {
                Ok(copy) => {
                    out.added = Some(copy);
                    true
                }
                Err(_) => false,
            },
            GradeEdit::Rename(id, name) => {
                let before = scene.card(root, slot).and_then(|c| c.layer(id)).map(|l| l.name.clone());
                scene.rename_layer(reg, root, slot, id, &name).is_ok()
                    && scene.card(root, slot).and_then(|c| c.layer(id)).map(|l| l.name.clone()) != before
            }
            GradeEdit::AddPart(mask, kind, combine) => grade::add_part(scene, reg, mask, kind, combine).is_ok(),
        };
    }
    out
}

/// Takes every value, key and pulse of panel `id` off the Grade card (a Tone Curve keeps which
/// curve shows). True when any was there.
fn reset_panel(scene: &mut Scene, id: LayerId) -> bool {
    let prefix = format!("{id}/");
    let view = format!("{prefix}{VIEW_KEY}");
    clear(scene, &prefix, Some(&view))
}

/// Takes every value, key and pulse whose key starts with `prefix` off the Grade card, but the
/// value of `keep`. True when any was there.
fn clear(scene: &mut Scene, prefix: &str, keep: Option<&str>) -> bool {
    let mine = |a: &Address| a.thing == ThingId::ROOT && a.slot == Some(SlotId::Grade) && a.key.starts_with(prefix);
    let (tracks, pulses) = (scene.tracks.len(), scene.pulses.len());
    scene.tracks.retain(|a, _| !mine(a));
    scene.pulses.retain(|a, _| !mine(a));
    let mut changed = scene.tracks.len() != tracks || scene.pulses.len() != pulses;
    if let Some(card) = scene.card_mut(ThingId::ROOT, SlotId::Grade) {
        let count = card.params.0.len();
        card.params.0.retain(|k, _| !k.starts_with(prefix) || Some(k.as_str()) == keep);
        changed |= card.params.0.len() != count;
    }
    changed
}
