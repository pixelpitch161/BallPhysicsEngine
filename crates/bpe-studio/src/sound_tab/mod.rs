//! A shape's Sound tab (layout B, "Wide plugin"): Enable sound, then Osc A and Osc B side by
//! side, the modulators and the picked one's editor, the notes, and Playing beside Output. The
//! inspector widens while it is open (see `Studio::inspector_panel`).
//!
//! Drawing reads the scene; every change is gathered as a [`TabEdit`] and applied once the tab
//! is drawn, through [`write::write`] (a key at the playhead when the setting has keys).

pub mod adsr_view;
pub mod chain_box;
pub mod keys;
pub mod mods;
pub mod notes;
pub mod osc;
pub mod output;
pub mod playing;
pub mod strip;
pub mod strip_views;
pub mod wave_view;
pub mod write;

use crate::inspector::Inspector;
use crate::style::{self, Knob, ModRing};
use crate::widgets::{self, KeyMark, TargetChoices, ValueMenu};
use bpe_core::{Address, Choice, ROUTES_KEY, Scene, SettingDef, SlotId, ThingId, Value, ValueType};
use bpe_kit::Kit;
use bpe_kit::sound::VOICE;
use bpe_sound::modmatrix::{Combine, combine_of, turns_in_ratios};
use eframe::egui::{self, Color32, RichText, Ui};

/// The Sound tab's own state, kept in the inspector.
#[derive(Clone, Debug, Default)]
pub struct SoundTab {
    /// The modulator each shape's editor shows.
    pub picked: std::collections::BTreeMap<ThingId, bpe_core::LayerId>,
    /// The layer strips that are open (any number at once).
    pub open_strips: std::collections::BTreeSet<(ThingId, bpe_core::LayerId)>,
    /// The strip being renamed in place, and its name so far.
    pub renaming: Option<(ThingId, bpe_core::LayerId, String)>,
    /// A chain layer has been copied (Paste layer is offered).
    pub can_paste_layer: bool,
    /// The Notes panel's state.
    pub notes: notes::NotesState,
    /// The modulator whose rings are lit (its tile hovered or its handle dragged).
    pub hot: Option<bpe_core::LayerId>,
}

/// A change the tab asks for, applied once it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum TabEdit {
    /// A setting of the sound card (a key at the playhead when it has keys).
    Set(Address, Value),
    /// A key at the playhead holding the setting's value as it stands.
    NewKey(Address),
    Enable,
    Disable,
    CopySound,
    PasteSound,
    /// Play a note on the shape now (a key clicked).
    Audition(u8),
    /// A chain layer of `kind` at the bottom of oscillator `lane`'s chain (it opens).
    AddLayer {
        lane: u8,
        kind: &'static str,
    },
    /// Turns a layer on or off.
    LayerOn(bpe_core::LayerId, bool),
    /// Moves a chain layer to this place in the card, within its lane.
    MoveLayer(bpe_core::LayerId, usize),
    RenameLayer(bpe_core::LayerId, String),
    DuplicateLayer(bpe_core::LayerId),
    CopyLayer(bpe_core::LayerId),
    /// The copied layer into oscillator `lane`'s chain, after `after` (else at the bottom).
    PasteLayer {
        lane: u8,
        after: Option<bpe_core::LayerId>,
    },
    DeleteLayer(bpe_core::LayerId),
    /// Another modulator of this kind at the end of its kind's tiles (it is picked).
    AddMod(&'static str),
    /// Turns a modulator on or off (a Distance never routed is turned on with its gain routes).
    ModOn(bpe_core::LayerId, bool),
    /// A copy of a modulator, named as a new one of its kind, without its routes.
    DuplicateMod(bpe_core::LayerId),
    /// A slot's notes onto the clipboard.
    CopyNotes(Vec<u8>),
    /// The setting stops following this shape's sound.
    Unfollow(Address),
    /// The edits made with this one are an undo step of their own (a preset, a key change, a
    /// row's edit), never merged with a knob turned just before.
    OwnStep,
}

/// What drawing the tab reads, and the edits it gathers.
pub struct TabCx<'a> {
    pub scene: &'a Scene,
    pub kit: &'a Kit,
    pub choices: &'a TargetChoices,
    pub id: ThingId,
    /// The playhead: values show as they are there, and a keyed knob keys there.
    pub time: f64,
    /// The shape's track colour (modulation rings, the keyboard's lit keys).
    pub colour: Color32,
    /// Notes copied from a slot (any shape's), for Paste notes.
    pub notes_clip: Option<Vec<u8>>,
    /// The modulator whose rings are lit.
    pub hot: Option<bpe_core::LayerId>,
    pub edits: Vec<TabEdit>,
}

impl TabCx<'_> {
    /// The sound card's setting `key`.
    pub fn at(&self, key: &str) -> Address {
        Address::on(self.id, SlotId::Sound, key)
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

    /// The setting as a choice of `T` (its default when it holds another name).
    pub fn choice<T: Choice + Default>(&self, key: &str) -> T {
        match self.get(key) {
            Value::Choice(name) => T::from_name(&name).unwrap_or_default(),
            _ => T::default(),
        }
    }

    pub fn set(&mut self, key: &str, v: Value) {
        let a = self.at(key);
        self.edits.push(TabEdit::Set(a, v));
    }

    /// The card's modulation routes, with the ones this frame already changed.
    pub fn routes(&self) -> Vec<bpe_core::ModRoute> {
        let a = self.at(ROUTES_KEY);
        let pending = self.edits.iter().rev().find_map(|e| match e {
            TabEdit::Set(at, Value::Routes(r)) if *at == a => Some(r.clone()),
            _ => None,
        });
        pending.unwrap_or_else(|| match self.get(ROUTES_KEY) {
            Value::Routes(r) => r,
            _ => Vec::new(),
        })
    }

    pub fn set_routes(&mut self, routes: Vec<bpe_core::ModRoute>) {
        self.set(ROUTES_KEY, Value::Routes(routes));
    }

    /// Whether modulator `layer` is an ADSR (its routes on a level knob shape the volume).
    pub fn is_envelope(&self, layer: bpe_core::LayerId) -> bool {
        self.scene
            .card(self.id, bpe_core::SlotId::Sound)
            .and_then(|c| c.layer(layer))
            .is_some_and(|l| l.kind == bpe_kit::sound::ADSR)
    }

    /// How route `r` moves its knob: an envelope scales a level, everything else adds.
    pub fn combine(&self, r: &bpe_core::ModRoute) -> Combine {
        combine_of(&r.target, self.is_envelope(r.source))
    }

    /// A knob on the number setting `key`, labelled `label`, its value read by `format`, with
    /// the usual right-click menu (Copy Value, Paste Value, Create Keyframe). A frequency knob
    /// turns evenly in ratios ([`turns_in_ratios`]), as the sound moves it.
    pub fn knob(&mut self, ui: &mut Ui, key: &str, label: &str, format: &dyn Fn(f64) -> String) -> egui::Response {
        let Some(def) = self.def(key) else {
            return ui.label(RichText::new(format!("{label}?")).color(style::MUTED));
        };
        let (lo, hi) = def.range.unwrap_or((0.0, 1.0));
        let default = number(&def.default).unwrap_or(lo);
        let now = self.get(key);
        let mut v = number(&now).unwrap_or(default);
        let bipolar = lo < 0.0 && hi > 0.0 && !is_db(def);
        let log = turns_in_ratios(def);
        // A ring for each route reaching the knob.
        let mut routes = self.routes();
        let reaching: Vec<usize> = (0..routes.len()).filter(|i| routes[*i].target == key).collect();
        let rings: Vec<ModRing> = reaching
            .iter()
            .map(|i| {
                let r = &routes[*i];
                mods::ring(r, v, (lo, hi), log, self.combine(r), self.hot == Some(r.source))
            })
            .collect();
        let r = Knob::new(&mut v, lo..=hi, default, label)
            .format(format)
            .bipolar(bipolar)
            .log(log)
            .width(KNOB_W)
            .step((def.ty == ValueType::Int).then_some(1.0))
            .rings(&rings)
            .colour(self.colour)
            .show(ui);
        if !rings.is_empty() {
            let words: Vec<String> = reaching
                .iter()
                .zip(&rings)
                .map(|(i, ring)| {
                    let name = mods::name_of(self, routes[*i].source);
                    if ring.lit { format!("{name} (lit)") } else { name }
                })
                .collect();
            let text = format!("Modulated by {}", words.join(", "));
            ui.ctx().accesskit_node_builder(r.response.id, |node| node.set_description(text));
        }
        if let Some(drag) = r.ring
            && let Some(&i) = reaching.get(drag.ring)
        {
            let mut route = routes[i].clone();
            let combine = self.combine(&route);
            route.amount = mods::amount_for(&route, v, (lo, hi), log, combine, drag.to);
            if drag.alt && combine == Combine::Add && !route.bipolar {
                route.bipolar = true;
            }
            if route != routes[i] {
                routes[i] = route;
                self.set_routes(routes);
            }
        } else if r.changed {
            let value = match def.ty {
                ValueType::Int => Value::Int(v.round() as i64),
                _ => Value::Number(v),
            };
            self.set(key, value);
        }
        mods::drag::on_knob(self, &r.response, key);
        self.knob_menu(&r.response, key, def, &now);
        r.response
    }

    /// A knob's value menu: Copy Value, Paste Value, Create Keyframe and Modulate by.
    fn knob_menu(&mut self, response: &egui::Response, key: &str, def: &SettingDef, now: &Value) {
        let a = self.at(key);
        let mark = Some(KeyMark { can_key: def.flags.keyframe, keyed: self.scene.keyed(&a), pulsed: false });
        let offers: Vec<(bpe_core::LayerId, String, bool)> = mods::list(self)
            .into_iter()
            .map(|m| {
                let ok = mods::drag::check(self, &mods::ModDrag { thing: self.id, layer: m.id }, key).is_ok();
                (m.id, m.name, ok)
            })
            .collect();
        let mut chosen = None;
        let mut extra = |ui: &mut Ui| {
            ui.separator();
            ui.menu_button("Modulate by", |ui| {
                for (id, name, ok) in &offers {
                    if ui.add_enabled(*ok, egui::Button::new(name)).clicked() {
                        chosen = Some(*id);
                        ui.close();
                    }
                }
            });
        };
        match widgets::value_menu_with(response, def, def.unit, now, mark, self.choices, Some(&mut extra)) {
            ValueMenu::Nothing => {}
            ValueMenu::Paste(v) => self.edits.push(TabEdit::Set(a, v)),
            ValueMenu::CreateKey => self.edits.push(TabEdit::NewKey(a)),
        }
        if let Some(layer) = chosen {
            mods::add_route(self, layer, key);
        }
    }

    /// The value menu on any control for the setting `key`.
    pub fn menu(&mut self, response: &egui::Response, key: &str, def: &SettingDef, now: &Value) {
        let a = self.at(key);
        let mark = Some(KeyMark { can_key: def.flags.keyframe, keyed: self.scene.keyed(&a), pulsed: false });
        match widgets::value_menu(response, def, def.unit, now, mark, self.choices) {
            ValueMenu::Nothing => {}
            ValueMenu::Paste(v) => self.edits.push(TabEdit::Set(a, v)),
            ValueMenu::CreateKey => self.edits.push(TabEdit::NewKey(a)),
        }
    }

    /// One chip per choice of the setting `key`; the one it holds is lit.
    pub fn chips<T: Choice + Default + PartialEq>(&mut self, ui: &mut Ui, key: &str, choices: &[(T, &str)]) {
        let now: T = self.choice(key);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (choice, name) in choices {
                let on = now == *choice;
                let chip = ui.add(style::toggle_button(name, on).min_size(egui::vec2(0.0, 22.0)));
                mods::drag::refuse(&chip);
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
        mods::drag::refuse(&r);
        if r.changed() {
            self.set(key, Value::Bool(on));
        }
        r
    }
}

/// Gives a control with no label of its own its name for screen readers and tests.
pub fn name_it(ui: &Ui, r: &egui::Response, name: &str) {
    let name = name.to_owned();
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(name));
}

/// Knobs on the Sound tab are narrower than the Mixer's, so six fit in an oscillator's row.
pub const KNOB_W: f32 = 54.0;

pub fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(x) => Some(*x),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn is_db(def: &SettingDef) -> bool {
    def.unit == "dB"
}

/// A level in dB, the bottom of the knob reading as silence.
pub fn db(v: f64) -> String {
    if v <= bpe_kit::sound::SILENT_DB as f64 + 1e-6 { "-inf dB".into() } else { format!("{v:+.1} dB") }
}

/// A panel of the tab: the panel grey with a soft line round it.
pub fn panel<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(style::PANEL)
        .stroke(egui::Stroke::new(1.0, style::LINE_SOFT))
        .corner_radius(style::RADIUS)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            add(ui)
        })
        .inner
}

/// Draws the Sound tab of shape `id` and applies its edits. True when the scene changed.
pub fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    scene: &mut Scene,
    kit: &Kit,
    choices: &TargetChoices,
    id: ThingId,
    time: f64,
) -> bool {
    let colour = style::colour_of(scene, id);
    let edits = {
        let scene: &Scene = scene;
        let notes_clip = inspector.clipboard.notes.clone();
        let hot = inspector.sound.hot;
        let mut cx = TabCx { scene, kit, choices, id, time, colour, notes_clip, hot, edits: Vec::new() };
        let card = scene.card(id, SlotId::Sound).filter(|c| c.kind == VOICE);
        let on = card.is_some() && cx.flag("enabled");
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                let mut ticked = on;
                let r = style::switch(ui, &mut ticked);
                let label = ui.label(RichText::new("Enable sound").font(style::semibold(13.0)));
                let r = r.labelled_by(label.id);
                let r = r.union(label);
                if ticked != on {
                    cx.edits.push(if ticked { TabEdit::Enable } else { TabEdit::Disable });
                }
                r.context_menu(|ui| {
                    if ui.add_enabled(card.is_some(), egui::Button::new("Copy sound")).clicked() {
                        cx.edits.push(TabEdit::CopySound);
                        ui.close();
                    }
                    let paste = ui
                        .add_enabled(inspector.clipboard.sound.is_some(), egui::Button::new("Paste sound"))
                        .on_disabled_hover_text("Copy a shape's sound first");
                    if paste.clicked() {
                        cx.edits.push(TabEdit::PasteSound);
                        ui.close();
                    }
                });
            });
            if card.is_none() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Turn it on and the shape plays a note each time a ball hits it.")
                            .small()
                            .color(style::MUTED),
                    );
                });
                return;
            }
            ui.add_space(6.0);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 0)).show(ui, |ui| {
                if !on {
                    ui.multiply_opacity(0.45);
                }
                body(ui, inspector, &mut cx);
            });
            ui.add_space(10.0);
        });
        cx.edits
    };
    apply(inspector, scene, kit, id, time, edits)
}

/// Everything under Enable sound.
fn body(ui: &mut Ui, inspector: &mut Inspector, cx: &mut TabCx) {
    inspector.sound.can_paste_layer = inspector.clipboard.chain_layer.is_some();
    ui.columns(2, |cols| {
        osc::show(&mut cols[0], cx, osc::Osc::A, &mut inspector.sound);
        osc::show(&mut cols[1], cx, osc::Osc::B, &mut inspector.sound);
    });
    ui.add_space(8.0);
    // The modulators' live readings (Now, the last hits) show only while playing: stopped, the
    // last readings would stay frozen and keep the editor redrawing.
    let meters = if inspector.playing { inspector.meters.clone() } else { None };
    panel(ui, |ui| mods::show(ui, cx, &mut inspector.sound, meters.as_deref()));
    ui.add_space(8.0);
    let playing = match (&inspector.meters, inspector.playing) {
        (Some(m), true) => m.shape_slot(cx.id.0),
        _ => None,
    };
    notes::show(ui, cx, &mut inspector.sound.notes, playing);
    ui.add_space(8.0);
    let level = inspector.meters.as_ref().map_or(f32::NEG_INFINITY, |m| m.take_shape(cx.id.0));
    if level > f32::NEG_INFINITY {
        // Keep the meter moving while the shape sounds.
        ui.ctx().request_repaint();
    }
    ui.columns(2, |cols| {
        playing::show(&mut cols[0], cx);
        output::show(&mut cols[1], cx, level);
    });
}

/// Applies the tab's edits. True when the scene changed.
fn apply(inspector: &mut Inspector, scene: &mut Scene, kit: &Kit, id: ThingId, time: f64, edits: Vec<TabEdit>) -> bool {
    let reg = &kit.registry;
    let mut changed = false;
    for edit in edits {
        changed |= match edit {
            TabEdit::Set(a, v) => match write::write(scene, reg, &a, v, time) {
                write::WriteOutcome::Set => true,
                write::WriteOutcome::Keyed => {
                    inspector.keyed_edit = true;
                    true
                }
                write::WriteOutcome::Failed => false,
            },
            TabEdit::Unfollow(a) => scene.remove_pulse(&a),
            TabEdit::NewKey(a) => match scene.value_at_time(reg, &a, time) {
                Some(v) => scene.set_key(reg, &a, time, v).is_ok(),
                None => false,
            },
            TabEdit::Enable => write::enable(scene, reg, id),
            TabEdit::Disable => write::disable(scene, reg, id),
            TabEdit::CopySound => {
                inspector.clipboard.sound = write::copy(scene, id);
                false
            }
            TabEdit::PasteSound => match inspector.clipboard.sound.clone() {
                Some(card) => write::paste(scene, reg, id, &card),
                None => false,
            },
            TabEdit::Audition(note) => {
                inspector.auditions.push((id, note));
                false
            }
            TabEdit::AddLayer { lane, kind } => match scene.add_layer_in(reg, id, SlotId::Sound, kind, Some(lane)) {
                Ok(layer) => {
                    inspector.sound.open_strips.insert((id, layer));
                    true
                }
                Err(_) => false,
            },
            TabEdit::LayerOn(layer, on) => scene.set_layer_enabled(reg, id, SlotId::Sound, layer, on).is_ok(),
            TabEdit::MoveLayer(layer, to) => scene.move_layer(reg, id, SlotId::Sound, layer, to).is_ok(),
            TabEdit::RenameLayer(layer, name) => scene.rename_layer(reg, id, SlotId::Sound, layer, &name).is_ok(),
            TabEdit::DuplicateLayer(layer) => scene.duplicate_layer(reg, id, SlotId::Sound, layer).is_ok(),
            TabEdit::CopyLayer(layer) => {
                if let Some(copied) = scene.copy_layer(id, SlotId::Sound, layer) {
                    inspector.clipboard.chain_layer = Some((id, copied));
                }
                false
            }
            TabEdit::PasteLayer { lane, after } => match inspector.clipboard.chain_layer.clone() {
                Some((from, copied)) => match chain_box::paste(scene, reg, id, from, &copied, lane, after) {
                    Some(layer) => {
                        inspector.sound.open_strips.insert((id, layer));
                        true
                    }
                    None => false,
                },
                None => false,
            },
            TabEdit::DeleteLayer(layer) => {
                inspector.sound.open_strips.remove(&(id, layer));
                // Modulations hearing it hear their own oscillator, as they sound, from now on.
                if let Some(card) = scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&SlotId::Sound)) {
                    bpe_kit::sound::chain::retarget_sources(card, &|l| (l != layer).then_some(l));
                    // In range of a Distance that goes: back to the first one that is on (a new
                    // layer taking its id later must not be picked up by surprise).
                    if matches!(card.params.get("play_zone"), Some(Value::Text(t)) if t.trim() == layer.0.to_string()) {
                        card.params.set("play_zone", Value::Text(String::new()));
                    }
                }
                scene.remove_layer(reg, id, SlotId::Sound, layer).is_ok()
            }
            TabEdit::AddMod(kind) => match bpe_kit::sound::add_modulator(scene, reg, id, kind) {
                Ok(layer) => {
                    inspector.sound.picked.insert(id, layer);
                    true
                }
                Err(_) => false,
            },
            TabEdit::ModOn(layer, on) => {
                let card = scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&SlotId::Sound));
                match card {
                    Some(card) if card.layer(layer).is_some() => {
                        let routed = matches!(card.params.get(ROUTES_KEY),
                            Some(Value::Routes(r)) if r.iter().any(|r| r.source == layer));
                        let distance = card.layer(layer).is_some_and(|l| l.kind == bpe_kit::sound::DISTANCE);
                        if on && distance && !routed {
                            bpe_kit::sound::turn_on_distance(card, layer);
                            true
                        } else {
                            scene.set_layer_enabled(reg, id, SlotId::Sound, layer, on).is_ok()
                        }
                    }
                    _ => false,
                }
            }
            TabEdit::DuplicateMod(layer) => match scene.duplicate_layer(reg, id, SlotId::Sound, layer) {
                Ok(copy) => {
                    let kind = scene.card(id, SlotId::Sound).and_then(|c| c.layer(copy)).map(|l| l.kind.clone());
                    if let (Some(kind), Some(card)) = (kind, scene.card(id, SlotId::Sound)) {
                        let name = bpe_kit::sound::numbered_name(card, &kind);
                        let _ = scene.rename_layer(reg, id, SlotId::Sound, copy, &name);
                    }
                    inspector.sound.picked.insert(id, copy);
                    true
                }
                Err(_) => false,
            },
            TabEdit::CopyNotes(notes) => {
                inspector.clipboard.notes = Some(notes);
                false
            }
            TabEdit::OwnStep => {
                inspector.own_step = true;
                false
            }
        };
    }
    changed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::app::Studio;
    use crate::inspector::InspectorTab;
    use eframe::egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

    /// The test scene's shape and ball.
    pub const SHAPE: ThingId = ThingId(1);
    pub const BALL: ThingId = ThingId(2);

    /// The studio's inspector beside an empty middle, in a wide window, each frame's edit
    /// recorded as the studio records it. The state keeps the inspector's width as drawn.
    pub fn studio_harness(selected: ThingId, sound: bool) -> Harness<'static, (Studio, f32)> {
        let mut studio = Studio::headless();
        studio.selection.set(selected);
        if sound {
            studio.panels.inspector.tab.insert(selected, InspectorTab::Sound);
        }
        let mut h = Harness::builder()
            .with_size(egui::vec2(1500.0, 1400.0))
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, (studio, width): &mut (Studio, f32)| {
                    let before = studio.scene.clone();
                    *width = studio.inspector_panel(ui);
                    egui::CentralPanel::default().show(ui, |_| {});
                    if std::mem::take(&mut studio.panels.inspector.keyed_edit) {
                        studio.set_playing(false);
                    }
                    studio.after_edit(before);
                },
                (studio, 0.0),
            );
        h.run();
        h
    }

    /// A studio harness on the shape's Sound tab with its sound turned on.
    pub fn sounding() -> Harness<'static, (Studio, f32)> {
        let mut h = studio_harness(SHAPE, true);
        click_switch(&mut h, "Enable sound");
        h
    }

    pub fn click_switch(h: &mut Harness<'_, (Studio, f32)>, name: &str) {
        h.get_by_role_and_label(Role::CheckBox, name).click();
        h.run();
    }

    fn card(h: &Harness<'_, (Studio, f32)>, id: ThingId) -> Option<bpe_core::Slot> {
        h.state().0.scene.card(id, SlotId::Sound).cloned()
    }

    #[test]
    fn a_shape_shows_settings_and_sound_buttons() {
        let mut h = studio_harness(SHAPE, false);
        assert!(h.query_by_label("Settings").is_some());
        h.get_by_label("Sound").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::CheckBox, "Enable sound").is_some());
        assert!(h.query_by_label("Shape settings").is_none(), "the shape's own cards are on the other tab");
        h.get_by_label("Settings").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::CheckBox, "Enable sound").is_none());
    }

    #[test]
    fn a_ball_shows_no_sound_button() {
        let h = studio_harness(BALL, false);
        assert!(h.query_by_label("Sound").is_none());
        assert!(h.query_by_label("Settings").is_none());
    }

    #[test]
    fn sound_widens_the_inspector_to_760_and_back() {
        let mut h = studio_harness(SHAPE, false);
        let narrow = h.state().1;
        assert!((narrow - 370.0).abs() <= 1.0, "{narrow}");
        h.get_by_label("Sound").click();
        h.run();
        let wide = h.state().1;
        assert!((wide - 760.0).abs() <= 1.0, "the Sound tab is {wide} wide");
        h.get_by_label("Settings").click();
        h.run();
        assert!((h.state().1 - narrow).abs() <= 1.0, "back to {narrow}: {}", h.state().1);
    }

    #[test]
    fn enable_sound_adds_the_starter_patch_and_undo_removes_it() {
        let mut h = studio_harness(SHAPE, true);
        assert!(card(&h, SHAPE).is_none());
        click_switch(&mut h, "Enable sound");
        let reg = bpe_kit::registry();
        assert_eq!(card(&h, SHAPE), Some(bpe_kit::sound::starter_voice(&reg)), "the starter patch");
        assert!(h.state_mut().0.undo());
        h.run();
        assert!(card(&h, SHAPE).is_none(), "one undo takes it away");
    }

    #[test]
    fn untick_and_tick_again_keeps_the_settings() {
        let mut h = sounding();
        let a = Address::on(SHAPE, SlotId::Sound, "a_gain");
        let reg = bpe_kit::registry();
        h.state_mut().0.scene.set_at(&reg, &a, Value::Number(-12.0)).unwrap();
        h.run();
        click_switch(&mut h, "Enable sound");
        let c = card(&h, SHAPE).expect("the card stays");
        assert_eq!(c.params.get("enabled"), Some(&Value::Bool(false)));
        click_switch(&mut h, "Enable sound");
        let c = card(&h, SHAPE).unwrap();
        assert_eq!(c.params.get("a_gain"), Some(&Value::Number(-12.0)), "the gain was kept");
        assert_ne!(c.params.get("enabled"), Some(&Value::Bool(false)));
    }

    #[test]
    fn copy_and_paste_sound_between_shapes() {
        let mut h = sounding();
        let reg = bpe_kit::registry();
        // A second shape to paste on.
        let other = {
            let studio = &mut h.state_mut().0;
            let id = bpe_kit::add_thing(&mut studio.scene, &reg, bpe_kit::things::shape::KIND, ThingId::ROOT).unwrap();
            studio.scene.set_at(&reg, &Address::on(SHAPE, SlotId::Sound, "a_cents"), Value::Number(7.0)).unwrap();
            id
        };
        h.run();
        let enable = h.get_by_role_and_label(Role::CheckBox, "Enable sound");
        enable.click_secondary();
        h.run();
        h.get_by_label("Copy sound").click();
        h.run();
        h.state_mut().0.selection.set(other);
        h.state_mut().0.panels.inspector.tab.insert(other, InspectorTab::Sound);
        h.run();
        h.get_by_role_and_label(Role::CheckBox, "Enable sound").click_secondary();
        h.run();
        h.get_by_label("Paste sound").click();
        h.run();
        let pasted = card(&h, other).expect("pasted");
        assert_eq!(pasted.params.get("a_cents"), Some(&Value::Number(7.0)));
        let ids: Vec<_> = pasted.layers.iter().map(|l| l.id).collect();
        match pasted.params.get(bpe_core::ROUTES_KEY) {
            Some(Value::Routes(routes)) => {
                assert!(!routes.is_empty());
                assert!(routes.iter().all(|r| ids.contains(&r.source)), "every route points at a pasted layer");
            }
            other => panic!("no routes: {other:?}"),
        }
    }

    #[test]
    fn shape_output_warns_when_its_bus_is_off() {
        let mut h = sounding();
        assert!(h.query_by_label_contains("is off").is_none(), "Bus 1 is on");
        // The Output panel sits below the chains and the notes.
        h.get_by_label("Bus 2").scroll_to_me();
        h.run();
        h.get_by_label("Bus 2").click();
        h.run();
        let card = card(&h, SHAPE).unwrap();
        assert_eq!(card.params.get("out_bus"), Some(&Value::Int(2)));
        assert!(h.query_by_label_contains("Bus 2 is off").is_some(), "it warns");
        // Its sends go to the other two buses.
        assert!(h.query_by_label("Send 1").is_some() && h.query_by_label("Send 3").is_some());
        assert!(h.query_by_label("Send 2").is_none());
        h.state_mut().0.scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap().params.set("bus2_on", Value::Bool(true));
        h.run();
        assert!(h.query_by_label_contains("is off").is_none(), "on, no warning");
    }
}
