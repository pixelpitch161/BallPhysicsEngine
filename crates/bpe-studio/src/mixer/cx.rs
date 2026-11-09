//! Drawing the Mixer's strips and effect editors: reading the mixer card at the playhead and
//! gathering edits, applied once everything is drawn ([`apply`]), a key at the playhead when a
//! setting has keys.

use crate::sound_tab::{name_it, number, write};
use crate::style::{self, Knob};
use crate::widgets::{self, KeyMark, TargetChoices, ValueMenu};
use bpe_core::{
    Address, Choice, CopiedLayer, LayerId, Scene, SettingDef, SlotId, ThingId, Value, ValueType, layered_key,
};
use bpe_kit::Kit;
use bpe_kit::sound::{BUSES, MIXER};
use eframe::egui::{self, RichText, Ui};

/// The editor open in the Mixer: a bus effect's, or the Master's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerPick {
    Layer(LayerId),
    Master,
}

/// A change the Mixer asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum MixEdit {
    /// A setting of the mixer card (a key at the playhead when it has keys).
    Set(Address, Value),
    /// A key at the playhead holding the setting's value as it stands.
    NewKey(Address),
    /// Every key of the setting goes; it keeps the value it has now.
    ClearKeys(Address),
    /// A new effect of `kind` at the end of bus `lane` (0..2); its editor opens.
    Add {
        kind: &'static str,
        lane: u8,
    },
    Remove(LayerId),
    /// An effect moved to place `index` among bus `lane`'s effects (another bus moves it there).
    Move {
        id: LayerId,
        lane: u8,
        index: usize,
    },
    Rename(LayerId, String),
    Duplicate(LayerId),
    Copy(LayerId),
    /// The copied effect at the end of bus `lane`.
    Paste(u8),
    /// Turns an effect on or off (bypass).
    Enable(LayerId, bool),
    Pick(Option<MixerPick>),
}

/// What drawing reads, and the edits it gathers.
pub struct MixCx<'a> {
    pub scene: &'a Scene,
    pub kit: &'a Kit,
    pub choices: &'a TargetChoices,
    /// The playhead: values show as they are there, and a keyed knob keys there.
    pub time: f64,
    /// The effect whose settings plain keys name (`None`: the card's own, such as `bus1_fader`).
    pub layer: Option<LayerId>,
    pub edits: Vec<MixEdit>,
}

/// Knobs in the strips are narrow, so four columns fit a Mixer as wide as the tree.
pub const STRIP_KNOB_W: f32 = 52.0;
/// Knobs in the editors.
pub const EDITOR_KNOB_W: f32 = 64.0;

impl MixCx<'_> {
    /// The mixer card's setting `key` (of [`MixCx::layer`] when one is set).
    pub fn at(&self, key: &str) -> Address {
        let key = match self.layer {
            Some(id) => layered_key(id, key),
            None => key.to_owned(),
        };
        Address::on(ThingId::ROOT, SlotId::Mixer, &key)
    }

    pub fn def(&self, key: &str) -> Option<&'static SettingDef> {
        self.scene.def_at(&self.kit.registry, &self.at(key))
    }

    /// The setting's value at the playhead (its default when it isn't set).
    pub fn get(&self, key: &str) -> Value {
        let a = self.at(key);
        self.scene
            .value_at_time(&self.kit.registry, &a, self.time)
            .or_else(|| self.def(key).map(|d| d.default.clone()))
            .unwrap_or(Value::Bool(false))
    }

    pub fn number(&self, key: &str) -> f64 {
        number(&self.get(key)).unwrap_or(0.0)
    }

    pub fn flag(&self, key: &str) -> bool {
        matches!(self.get(key), Value::Bool(true))
    }

    pub fn text(&self, key: &str) -> String {
        match self.get(key) {
            Value::Text(t) => t,
            _ => String::new(),
        }
    }

    /// The setting as a choice of `T` (its default when it holds another name).
    pub fn choice<T: Choice + Default>(&self, key: &str) -> T {
        match self.get(key) {
            Value::Choice(name) => T::from_name(&name).unwrap_or_default(),
            _ => T::default(),
        }
    }

    pub fn set(&mut self, key: &str, v: Value) {
        let a = self.at(key);
        self.edits.push(MixEdit::Set(a, v));
    }

    /// A knob on the number setting `key`, `width` wide, labelled `label`, its value read by
    /// `format`, with the usual right-click menu.
    pub fn knob_w(
        &mut self,
        ui: &mut Ui,
        key: &str,
        label: &str,
        format: &dyn Fn(f64) -> String,
        width: f32,
    ) -> egui::Response {
        let Some(def) = self.def(key) else {
            return ui.label(RichText::new(format!("{label}?")).color(style::MUTED));
        };
        let (lo, hi) = def.range.unwrap_or((0.0, 1.0));
        let default = number(&def.default).unwrap_or(lo);
        let now = self.get(key);
        let mut v = number(&now).unwrap_or(default);
        let bipolar = lo < 0.0 && hi > 0.0 && def.unit != "dB";
        let log = matches!(def.unit, "Hz" | "ms" | "s") && lo > 0.0 && hi / lo >= 20.0;
        let r = Knob::new(&mut v, lo..=hi, default, label)
            .format(format)
            .bipolar(bipolar)
            .log(log)
            .width(width)
            .step((def.ty == ValueType::Int).then_some(1.0))
            .show(ui);
        if r.changed {
            let value = match def.ty {
                ValueType::Int => Value::Int(v.round() as i64),
                _ => Value::Number(v),
            };
            self.set(key, value);
        }
        self.menu(&r.response, key, def, &now);
        r.response
    }

    /// An editor's knob.
    pub fn knob(&mut self, ui: &mut Ui, key: &str, label: &str, format: &dyn Fn(f64) -> String) -> egui::Response {
        self.knob_w(ui, key, label, format, EDITOR_KNOB_W)
    }

    /// A knob that reads in the setting's own unit.
    pub fn knob_auto(&mut self, ui: &mut Ui, key: &str, label: &str) -> egui::Response {
        let unit = self.def(key).map_or("", |d| d.unit);
        let format = move |v: f64| format_unit(v, unit);
        self.knob(ui, key, label, &format)
    }

    /// The value menu on any control for the setting `key`.
    pub fn menu(&mut self, response: &egui::Response, key: &str, def: &SettingDef, now: &Value) {
        let a = self.at(key);
        let mark = Some(KeyMark { can_key: def.flags.keyframe, keyed: self.scene.keyed(&a), pulsed: false });
        match widgets::value_menu(response, def, def.unit, now, mark, self.choices) {
            ValueMenu::Nothing => {}
            ValueMenu::Paste(v) => self.edits.push(MixEdit::Set(a, v)),
            ValueMenu::CreateKey => self.edits.push(MixEdit::NewKey(a)),
            ValueMenu::RemoveKeys => self.edits.push(MixEdit::ClearKeys(a)),
        }
    }

    /// One chip per choice of the setting `key`; the one it holds is lit.
    pub fn chips<T: Choice + Default + PartialEq + Copy>(&mut self, ui: &mut Ui, key: &str, choices: &[(T, &str)]) {
        let now: T = self.choice(key);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (choice, name) in choices {
                let on = now == *choice;
                let chip = ui.add(style::toggle_button(name, on).min_size(egui::vec2(0.0, 22.0)));
                if chip.clicked() && !on {
                    self.set(key, Value::Choice(choice.name().into()));
                }
            }
        });
    }

    /// A switch for the true/false setting `key`, named `name`.
    pub fn switch(&mut self, ui: &mut Ui, key: &str, name: &str) -> egui::Response {
        let mut on = self.flag(key);
        let r = style::switch(ui, &mut on);
        name_it(ui, &r, name);
        if r.changed() {
            self.set(key, Value::Bool(on));
        }
        r
    }

    /// The mixer card's effect layers of bus `lane` (0..2), in order.
    pub fn effects(&self, lane: u8) -> Vec<bpe_core::Layer> {
        self.scene
            .card(ThingId::ROOT, SlotId::Mixer)
            .map(|c| c.layers.iter().filter(|l| l.lane == Some(lane)).cloned().collect())
            .unwrap_or_default()
    }
}

/// `v` read in `unit`.
pub fn format_unit(v: f64, unit: &str) -> String {
    match unit {
        "dB" => crate::sound_tab::db(v),
        "Hz" if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
        "Hz" if v < 10.0 => format!("{v:.2} Hz"),
        "Hz" => format!("{v:.0} Hz"),
        "ms" if v < 10.0 => format!("{v:.1} ms"),
        "ms" => format!("{v:.0} ms"),
        "s" => format!("{v:.1} s"),
        "%" => format!("{v:.0} %"),
        "" => format!("{v:.1}"),
        u => format!("{v:.1} {u}"),
    }
}

/// What applying the Mixer's edits did.
#[derive(Default)]
pub struct Applied {
    /// The scene changed.
    pub changed: bool,
    /// A keyed setting was edited (the studio pauses).
    pub keyed: bool,
    /// The editor to open now (or none), when an edit picked something.
    pub pick: Option<Option<MixerPick>>,
}

/// The effect last copied in the Mixer.
pub type Copied = Option<CopiedLayer>;

/// Applies the Mixer's edits.
pub fn apply(scene: &mut Scene, kit: &Kit, time: f64, edits: Vec<MixEdit>, copied: &mut Copied) -> Applied {
    let reg = &kit.registry;
    let (root, slot) = (ThingId::ROOT, SlotId::Mixer);
    let mut out = Applied::default();
    if !edits.is_empty() && scene.card(root, slot).is_none_or(|c| c.kind != MIXER) {
        bpe_kit::ensure_mixer(scene, reg);
    }
    for edit in edits {
        out.changed |= match edit {
            MixEdit::Set(a, v) => match write::write(scene, reg, &a, v, time) {
                write::WriteOutcome::Set => true,
                write::WriteOutcome::Keyed => {
                    out.keyed = true;
                    true
                }
                write::WriteOutcome::Failed => false,
            },
            MixEdit::NewKey(a) => match scene.value_at_time(reg, &a, time) {
                Some(v) => scene.set_key(reg, &a, time, v).is_ok(),
                None => false,
            },
            MixEdit::ClearKeys(a) => crate::inspector::clear_keys(scene, reg, &a, time),
            MixEdit::Add { kind, lane } => match scene.add_layer_in(reg, root, slot, kind, Some(lane)) {
                Ok(id) => {
                    out.pick = Some(Some(MixerPick::Layer(id)));
                    true
                }
                Err(_) => false,
            },
            MixEdit::Remove(id) => scene.remove_layer(reg, root, slot, id).is_ok(),
            MixEdit::Move { id, lane, index } => move_effect(scene, id, lane, index),
            MixEdit::Rename(id, name) => {
                let name = name.trim();
                !name.is_empty() && scene.rename_layer(reg, root, slot, id, name).is_ok()
            }
            MixEdit::Duplicate(id) => match scene.duplicate_layer(reg, root, slot, id) {
                Ok(new) => {
                    out.pick = Some(Some(MixerPick::Layer(new)));
                    true
                }
                Err(_) => false,
            },
            MixEdit::Copy(id) => {
                *copied = scene.copy_layer(root, slot, id);
                false
            }
            MixEdit::Paste(lane) => match copied.clone() {
                Some(mut c) => {
                    c.lane = Some(lane);
                    scene.paste_layer(reg, root, slot, &c).is_ok()
                }
                None => false,
            },
            MixEdit::Enable(id, on) => scene.set_layer_enabled(reg, root, slot, id, on).is_ok(),
            MixEdit::Pick(pick) => {
                out.pick = Some(pick);
                false
            }
        };
    }
    out
}

/// Moves effect `id` to place `index` among bus `lane`'s effects (`lane` 0..2), keeping its
/// settings and keys. Layers stay sorted by bus.
pub fn move_effect(scene: &mut Scene, id: LayerId, lane: u8, index: usize) -> bool {
    if lane as usize >= BUSES {
        return false;
    }
    let Some(card) = scene.card_mut(ThingId::ROOT, SlotId::Mixer) else { return false };
    let Some(from) = card.layers.iter().position(|l| l.id == id) else { return false };
    let before = card.layers.clone();
    let mut layer = card.layers.remove(from);
    layer.lane = Some(lane);
    let in_lane: Vec<usize> = (0..card.layers.len()).filter(|&i| card.layers[i].lane == Some(lane)).collect();
    let at = match in_lane.get(index) {
        Some(&i) => i,
        None => match in_lane.last() {
            Some(&last) => last + 1,
            // An empty bus: after the buses before it.
            None => card.layers.iter().position(|l| l.lane.unwrap_or(0) > lane).unwrap_or(card.layers.len()),
        },
    };
    card.layers.insert(at, layer);
    card.layers != before
}
