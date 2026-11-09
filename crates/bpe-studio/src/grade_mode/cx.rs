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
    pub fn changed(&self, layer: LayerId) -> bool {
        let prefix = format!("{layer}/");
        let set = self.card.params.0.iter().any(|(k, v)| {
            k.strip_prefix(&prefix).is_some_and(|key| self.def(Some(layer), key).is_none_or(|d| d.default != *v))
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
    /// A new panel of `kind` at the end of the Whole picture's panels.
    AddPanel(&'static str),
    RemovePanel(LayerId),
    /// A panel to place `to` among the Whole picture's panels (0 works first).
    MovePanel(LayerId, usize),
    /// Every setting of a panel back to its default: its values, keys and pulses go.
    ResetPanel(LayerId),
    /// The panels back in Lightroom's order.
    ResetOrder,
    /// A panel switched on or off.
    EnablePanel(LayerId, bool),
}

/// What applying the edits did.
#[derive(Default, Debug, PartialEq)]
pub struct Applied {
    pub changed: bool,
    /// A setting with keys was keyed at the playhead (the editor pauses).
    pub keyed: bool,
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
    let whole = scene.card(root, slot).and_then(grade::whole_of);
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
            GradeEdit::AddPanel(kind) => whole.is_some_and(|w| grade::add_panel(scene, reg, w, kind).is_ok()),
            GradeEdit::RemovePanel(id) => {
                let allowed = scene.card(root, slot).is_some_and(|c| grade::can_remove_panel(c, id));
                allowed && scene.remove_layer(reg, root, slot, id).is_ok()
            }
            GradeEdit::MovePanel(id, to) => {
                let before = scene.card(root, slot).map(|c| c.layers.clone());
                scene.move_sub_layer_in(reg, root, slot, id, to).is_ok()
                    && scene.card(root, slot).map(|c| &c.layers) != before.as_ref()
            }
            GradeEdit::ResetPanel(id) => reset_panel(scene, id),
            GradeEdit::ResetOrder => whole.is_some_and(|w| grade::reset_order(scene, reg, w)),
            GradeEdit::EnablePanel(id, on) => scene.set_layer_enabled(reg, root, slot, id, on).is_ok(),
        };
    }
    out
}

/// Takes every value, key and pulse of panel `id` off the Grade card. True when any was there.
fn reset_panel(scene: &mut Scene, id: LayerId) -> bool {
    let prefix = format!("{id}/");
    let mine = |a: &Address| a.thing == ThingId::ROOT && a.slot == Some(SlotId::Grade) && a.key.starts_with(&prefix);
    let (tracks, pulses) = (scene.tracks.len(), scene.pulses.len());
    scene.tracks.retain(|a, _| !mine(a));
    scene.pulses.retain(|a, _| !mine(a));
    let mut changed = scene.tracks.len() != tracks || scene.pulses.len() != pulses;
    if let Some(card) = scene.card_mut(ThingId::ROOT, SlotId::Grade) {
        let count = card.params.0.len();
        card.params.0.retain(|k, _| !k.starts_with(&prefix));
        changed |= card.params.0.len() != count;
    }
    changed
}
