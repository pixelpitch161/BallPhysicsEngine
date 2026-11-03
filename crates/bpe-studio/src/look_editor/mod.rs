//! The Look editor: the Look card of a thing whose look is a stack of layers. A list of the
//! drawn layers (the top one first, the Body at the bottom), a list of the reactions, and under
//! them the settings of the one picked.

pub mod ball_view;
mod clipboard;
mod layer_list;

pub use ball_view::BallView;
pub use clipboard::Clipboard;

use bpe_core::{Address, LayerId, LayerSection, Params, Scene, SettingDef, SlotId, ThingId, Value, layered_key};
use bpe_kit::Kit;
use eframe::egui::{self, RichText, Ui};

use crate::inspector::{Edit, Inspector};
use crate::style;
use crate::widgets::{GridCx, KeyMark, TargetChoices, settings_grid};

/// The shader draws at most this many records per ball (MAX_LAYERS in bpe-gfx).
const DRAWN_LIMIT: usize = bpe_gfx::MAX_LAYERS as usize;

/// What the Look editor reads.
pub struct LookCx<'a> {
    pub scene: &'a Scene,
    pub kit: &'a Kit,
    pub choices: &'a TargetChoices,
    pub id: ThingId,
    pub slot: SlotId,
    pub time: f64,
    pub label_width: f32,
}

impl LookCx<'_> {
    fn reg(&self) -> &bpe_core::Registry {
        &self.kit.registry
    }

    fn card(&self) -> Option<&bpe_core::Slot> {
        self.scene.card(self.id, self.slot)
    }

    /// Where layer `layer`'s setting `key` is kept.
    fn at(&self, layer: LayerId, key: &str) -> Address {
        Address::on(self.id, self.slot, &layered_key(layer, key))
    }

    /// Layer `layer`'s settings as they are at the playhead, hidden or not.
    pub fn params_of(&self, layer: LayerId, defs: &[SettingDef]) -> Params {
        let mut p = Params::default();
        for d in defs {
            if let Some(v) = self.scene.value_at_time(self.reg(), &self.at(layer, d.key), self.time) {
                p.set(d.key, v);
            }
        }
        p
    }
}

/// The layer of `id`'s card shown below the lists: the one picked, or the Body.
pub fn selected(inspector: &Inspector, card: &bpe_core::Slot, id: ThingId) -> Option<LayerId> {
    let picked = inspector.selected_layer.get(&id).copied().filter(|l| card.layer(*l).is_some());
    picked.or_else(|| card.layers.first().map(|l| l.id))
}

/// Draws the Look editor's lists and the picked layer's settings, pushing what the user does to
/// `edits`.
pub fn show(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let Some(card) = cx.card() else { return };
    let reg = cx.reg();
    let Some(picked) = selected(inspector, card, cx.id) else {
        ui.label(RichText::new("This look has no layers.").color(style::MUTED));
        return;
    };
    // ---------- the ball view ----------
    {
        let Inspector { ball_view, render, .. } = inspector;
        ball_view::show(ui, ball_view, render.as_ref(), cx.scene, cx.kit, cx.id, cx.time);
    }
    ui.add_space(6.0);
    let section = |kind: &str| reg.layer(kind).map(|k| k.section());
    let drawn: Vec<&bpe_core::Layer> =
        card.layers.iter().filter(|l| section(&l.kind) != Some(LayerSection::Reaction)).collect();
    let reactions: Vec<&bpe_core::Layer> =
        card.layers.iter().filter(|l| section(&l.kind) == Some(LayerSection::Reaction)).collect();

    // ---------- Layers ----------
    list_header(ui, cx, card, "Layers", "+ Add layer", LayerSection::Drawn, edits);
    let mut rows = Vec::new();
    for layer in drawn.iter().rev() {
        let index = card.layer_index(layer.id).unwrap_or(0);
        let rect = layer_list::row(ui, inspector, cx, card, layer, index, picked, edits);
        rows.push((layer.id, rect));
    }
    layer_list::drop_target(ui, cx, card, &rows, edits);
    if drawn.iter().filter(|l| l.enabled).count() > DRAWN_LIMIT {
        ui.label(RichText::new(format!("Only the first {DRAWN_LIMIT} layers are drawn")).small().color(style::MUTED));
    }

    // ---------- Reactions ----------
    ui.add_space(6.0);
    list_header(ui, cx, card, "Reactions", "+ Add reaction", LayerSection::Reaction, edits);
    if reactions.is_empty() {
        ui.label(
            RichText::new("None yet: add one to change the ball as it moves or hits.").small().color(style::FAINT),
        );
    }
    for layer in &reactions {
        let index = card.layer_index(layer.id).unwrap_or(0);
        layer_list::row(ui, inspector, cx, card, layer, index, picked, edits);
    }

    // ---------- the picked one's settings ----------
    let Some(layer) = card.layer(picked) else { return };
    let Some(kind) = reg.layer(&layer.kind) else {
        ui.label(RichText::new(format!("Its kind \"{}\" isn't installed.", layer.kind)).color(style::TEXT));
        return;
    };
    ui.add_space(8.0);
    style::divider(ui);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(&layer.name).font(style::semibold(13.0)));
        if layer.name != kind.name() {
            ui.label(RichText::new(kind.name()).small().color(style::MUTED));
        }
    })
    .response
    .on_hover_text(kind.help());
    ui.add_space(2.0);
    if kind.base() {
        size_row(ui, cx, edits);
    }
    layer_settings(ui, inspector, cx, picked, kind.settings(), edits);
}

/// A list's title and its add menu: the kinds of `section` that fit the card, greyed when one
/// that can't repeat is already there.
fn list_header(
    ui: &mut Ui,
    cx: &LookCx,
    card: &bpe_core::Slot,
    title: &str,
    add: &str,
    section: LayerSection,
    edits: &mut Vec<Edit>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(style::medium(12.0)).color(style::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(RichText::new(add).small(), |ui| {
                ui.set_min_width(170.0);
                for kind in cx.reg().layers_for(&card.kind).filter(|k| k.section() == section && !k.base()) {
                    let there = card.layers.iter().any(|l| l.kind == kind.id());
                    let free = kind.repeats() || !there;
                    let item = ui.add_enabled(free, egui::Button::new(kind.name()));
                    let item = item.on_hover_text(kind.help()).on_disabled_hover_text("This look has one already");
                    if item.clicked() {
                        edits.push(Edit::AddLayer(cx.slot, kind.id()));
                        ui.close();
                    }
                }
            });
        });
    });
}

/// The Body's Size row: the thing's own Radius, which the physics uses.
fn size_row(ui: &mut Ui, cx: &LookCx, edits: &mut Vec<Edit>) {
    let Some(thing) = cx.scene.things.get(&cx.id) else { return };
    let Some(kind) = cx.reg().thing(&thing.kind) else { return };
    let Some(radius) = kind.settings().iter().find(|d| d.key == "radius") else { return };
    let def = SettingDef { label: "Size", group: "", ..radius.clone() };
    let own = |key: &str| Address::own(cx.id, key);
    let get = |key: &str| cx.scene.value_at_time(cx.reg(), &own(key), cx.time).unwrap_or(Value::Number(0.0));
    let marks = |key: &str| (key == def.key).then(|| mark_for(cx.scene, &def, &own(key)));
    let applies = |_: &SettingDef| true;
    let grid = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: &applies,
        folded: None,
        label_width: cx.label_width,
        track: style::colour_of(cx.scene, cx.id),
    };
    let out = settings_grid(ui, ("look size", cx.id), std::slice::from_ref(&def), get, &grid);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetOwn(key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(None, key.into())));
}

/// Layer `layer`'s settings grid. Its edits carry the layered keys.
fn layer_settings(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    layer: LayerId,
    defs: &'static [SettingDef],
    edits: &mut Vec<Edit>,
) {
    let (scene, reg, time) = (cx.scene, cx.reg(), cx.time);
    let get = |key: &str| scene.value_at_time(reg, &cx.at(layer, key), time).expect("a layer's settings all resolve");
    let marks = |key: &str| {
        let def = defs.iter().find(|d| d.key == key)?;
        Some(mark_for(scene, def, &cx.at(layer, key)))
    };
    let kind = scene.things.get(&cx.id).map(|t| t.kind.clone());
    let applies = |d: &SettingDef| d.applies(kind.as_deref(), &|k| scene.value_at_time(reg, &cx.at(layer, k), time));
    let shut = crate::inspector::folded_groups(inspector, (cx.id, Some(cx.slot)), defs);
    let folded = |group: &str| shut.contains(group);
    let grid = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: &applies,
        folded: Some(&folded),
        label_width: cx.label_width,
        track: style::colour_of(scene, cx.id),
    };
    let out = settings_grid(ui, ("layer", cx.id, cx.slot, layer.0), defs, get, &grid);
    crate::inspector::fold_groups(inspector, (cx.id, Some(cx.slot)), &out.folds);
    let slot = cx.slot;
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, layered_key(layer, key), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(Some(slot), layered_key(layer, key))));
}

fn mark_for(scene: &Scene, def: &SettingDef, a: &Address) -> KeyMark {
    KeyMark { can_key: def.flags.keyframe, keyed: scene.keyed(a), pulsed: scene.pulses.contains_key(a) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;
    use bpe_core::{Resolved, resolve};
    use bpe_kit::modules::look::layers as kinds;
    use eframe::egui::accesskit::Role;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
    }

    /// The blank project with an arena and two new balls, the first selected.
    fn harness() -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        bpe_kit::add_thing(&mut scene, &kit.registry, "arena", ThingId::ROOT).unwrap();
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut selection = Selection::default();
        selection.set(ball);
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 4000.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let changed = crate::inspector::show(
                        ui,
                        &mut s.inspector,
                        &mut s.scene,
                        &s.kit,
                        &s.resolved,
                        &s.selection,
                        0.0,
                    );
                    if changed {
                        s.resolved = resolve(&s.scene, &s.kit.registry);
                    }
                },
                state,
            );
        h.run();
        h
    }

    const ARENA: ThingId = ThingId(2);
    const BALL: ThingId = ThingId(3);
    const OTHER: ThingId = ThingId(4);

    fn card(h: &Harness<'_, State>, id: ThingId) -> bpe_core::Slot {
        h.state().scene.card(id, SlotId::Look).unwrap().clone()
    }

    fn kinds_of(h: &Harness<'_, State>, id: ThingId) -> Vec<String> {
        card(h, id).layers.iter().map(|l| l.kind.clone()).collect()
    }

    /// Picks `item` from the add menu `menu` ("+ Add layer" or "+ Add reaction").
    fn add(h: &mut Harness<'_, State>, menu: &str, item: &str) {
        h.get_by_label(menu).click();
        h.run();
        h.get_by_role_and_label(Role::Button, item).click();
        h.run();
    }

    /// Picks `item` from the right-click menu of the row named `row`.
    fn row_menu(h: &mut Harness<'_, State>, row: &str, item: &str) {
        h.get_by_role_and_label(Role::Button, row).click_secondary();
        h.run();
        h.get_by_label(item).click();
        h.run();
    }

    fn layer_named(h: &Harness<'_, State>, id: ThingId, name: &str) -> bpe_core::Layer {
        card(h, id).layers.iter().find(|l| l.name == name).cloned().unwrap_or_else(|| panic!("no layer {name}"))
    }

    #[test]
    fn a_new_ball_shows_its_body_alone_and_no_reactions() {
        let h = harness();
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY]);
        assert!(h.query_by_role_and_label(Role::Button, "Body").is_some(), "the Body row");
        assert!(h.query_by_label("Hide Body").is_none(), "the Body can't be hidden");
        assert!(h.query_by_label("None yet: add one to change the ball as it moves or hits.").is_some());
        // The Body's settings show under the lists, its Size first.
        assert_eq!(h.get_all_by_role_and_label(Role::Slider, "Size").count(), 2, "on the Balls card and the Body");
        assert!(h.query_by_role_and_label(Role::Label, "Shape").is_some());
        assert!(h.query_by_label("Add a colour").is_some());
    }

    #[test]
    fn the_body_s_size_is_the_ball_s_own_radius() {
        let mut h = harness();
        let body_size = h.get_all_by_role_and_label(Role::SpinButton, "Size").nth(1).unwrap();
        body_size.set_value_accesskit(40.0);
        h.run();
        assert_eq!(h.state().scene.things[&BALL].params.get("radius"), Some(&bpe_core::Value::Number(40.0)));
    }

    #[test]
    fn an_added_layer_is_picked_and_its_settings_edit_it() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Outline");
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY, kinds::OUTLINE]);
        let outline = layer_named(&h, BALL, "Outline");
        assert_eq!(h.state().inspector.selected_layer.get(&BALL), Some(&outline.id), "the new row is picked");
        let width = h.get_by_role_and_label(Role::SpinButton, "Width");
        width.set_value_accesskit(9.0);
        h.run();
        let key = layered_key(outline.id, "width");
        assert_eq!(card(&h, BALL).params.get(&key), Some(&bpe_core::Value::Number(9.0)));
        assert!(h.query_by_label("4 px · Inside").is_none() && h.query_by_label("9 px · Inside").is_some());
        // The other ball, not selected, is left alone.
        assert_eq!(kinds_of(&h, OTHER), [kinds::BODY]);
    }

    #[test]
    fn a_layer_renames_hides_and_deletes() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Outline");
        // Half a second on, so the menu's clicks don't make this a triple click.
        for _ in 0..30 {
            h.step();
        }
        h.get_by_role_and_label(Role::Button, "Outline").click();
        h.step();
        h.get_by_role_and_label(Role::Button, "Outline").click();
        h.run();
        assert!(h.state().inspector.renaming_layer.is_some(), "a double click renames");
        let field = h.get_all_by_role_and_label(Role::TextInput, "Name").last().unwrap();
        field.type_text("Rim");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        let rim = layer_named(&h, BALL, "Rim");
        assert_eq!(rim.kind, kinds::OUTLINE);
        h.get_by_label("Hide Rim").click();
        h.run();
        assert!(!layer_named(&h, BALL, "Rim").enabled);
        h.get_by_label("Show Rim").click();
        h.run();
        assert!(layer_named(&h, BALL, "Rim").enabled);
        row_menu(&mut h, "Rim", "Delete");
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY]);
        assert_eq!(
            h.state().inspector.selected_layer.get(&BALL).copied().and_then(|l| card(&h, BALL).layer(l).cloned()),
            None
        );
        assert!(h.query_by_role_and_label(Role::Label, "Shape").is_some(), "the Body is shown again");
    }

    #[test]
    fn layers_move_up_and_down_and_duplicate() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Outline");
        add(&mut h, "+ Add layer", "Glow");
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY, kinds::OUTLINE, kinds::GLOW]);
        row_menu(&mut h, "Outline", "Move up");
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY, kinds::GLOW, kinds::OUTLINE]);
        row_menu(&mut h, "Outline", "Move down");
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY, kinds::OUTLINE, kinds::GLOW]);
        row_menu(&mut h, "Outline", "Duplicate");
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY, kinds::OUTLINE, kinds::OUTLINE, kinds::GLOW]);
        let copy = layer_named(&h, BALL, "Outline copy");
        assert_eq!(h.state().inspector.selected_layer.get(&BALL), Some(&copy.id));
    }

    #[test]
    fn the_body_s_menu_only_copies_and_pastes() {
        let mut h = harness();
        h.get_by_role_and_label(Role::Button, "Body").click_secondary();
        h.run();
        for item in ["Duplicate", "Move up", "Move down", "Delete"] {
            assert!(h.get_by_label(item).accesskit_node().is_disabled(), "{item}");
        }
        assert!(!h.get_by_label("Copy layer").accesskit_node().is_disabled());
        assert!(!h.get_by_label("Copy whole look").accesskit_node().is_disabled());
        assert!(h.get_by_label("Paste layer").accesskit_node().is_disabled(), "nothing copied yet");
    }

    #[test]
    fn a_copied_layer_pastes_onto_another_ball() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Outline");
        let outline = layer_named(&h, BALL, "Outline");
        h.state_mut()
            .scene
            .card_mut(BALL, SlotId::Look)
            .unwrap()
            .params
            .set(&layered_key(outline.id, "width"), bpe_core::Value::Number(12.0));
        h.run();
        row_menu(&mut h, "Outline", "Copy layer");
        h.state_mut().selection.set(OTHER);
        h.run();
        row_menu(&mut h, "Body", "Paste layer");
        assert_eq!(kinds_of(&h, OTHER), [kinds::BODY, kinds::OUTLINE]);
        let pasted = layer_named(&h, OTHER, "Outline");
        assert_eq!(card(&h, OTHER).params.get(&layered_key(pasted.id, "width")), Some(&bpe_core::Value::Number(12.0)));
    }

    #[test]
    fn a_whole_look_pastes_onto_another_ball() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Glow");
        add(&mut h, "+ Add reaction", "Flash on hit");
        row_menu(&mut h, "Body", "Copy whole look");
        h.state_mut().selection.set(OTHER);
        h.run();
        row_menu(&mut h, "Body", "Paste whole look");
        assert_eq!(kinds_of(&h, OTHER), kinds_of(&h, BALL));
        assert!(h.query_by_role_and_label(Role::Button, "Flash on hit").is_some(), "the reaction's row");
    }

    #[test]
    fn a_layer_that_can_only_be_there_once_is_greyed_in_the_menu() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Blend");
        h.get_by_label("+ Add layer").click();
        h.run();
        // The Blend row and, greyed, the menu's Blend.
        let blends: Vec<bool> =
            h.get_all_by_role_and_label(Role::Button, "Blend").map(|n| n.accesskit_node().is_disabled()).collect();
        assert_eq!(blends.len(), 2);
        assert!(blends.contains(&true), "{blends:?}");
        assert!(!h.get_by_role_and_label(Role::Button, "Outline").accesskit_node().is_disabled(), "outlines repeat");
    }

    #[test]
    fn a_layer_s_keyframe_lane_is_named_after_the_layer() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Outline");
        h.get_all_by_role_and_label(Role::Label, "Width").next().unwrap().click_secondary();
        h.run();
        h.get_by_label("Create Keyframe").click();
        h.run();
        let s = h.state();
        let groups = crate::timeline::lanes::groups_for(&s.scene, &s.kit);
        let lanes: Vec<&str> = groups.iter().flat_map(|g| g.lanes.iter().map(|l| l.short.as_str())).collect();
        assert_eq!(lanes, ["Outline · Width"]);
    }

    #[test]
    fn undo_brings_back_each_step() {
        let mut h = harness();
        let mut history = crate::history::History::default();
        let start = h.state().scene.clone();
        history.record(start.clone(), true);
        add(&mut h, "+ Add layer", "Outline");
        let added = h.state().scene.clone();
        history.record(added.clone(), true);
        row_menu(&mut h, "Outline", "Delete");
        let mut scene = h.state().scene.clone();
        assert!(history.undo(&mut scene));
        assert_eq!(scene, added, "the delete is undone");
        assert!(history.undo(&mut scene));
        assert_eq!(scene, start, "and the add");
    }

    #[test]
    fn the_arena_keeps_its_plain_look_card() {
        let mut h = harness();
        h.state_mut().selection.set(ARENA);
        h.run();
        assert!(h.query_by_label("+ Add layer").is_none());
        assert!(h.query_by_role_and_label(Role::Label, "Glow").is_some(), "the old grid");
    }

    #[test]
    fn the_ball_view_buttons_pick_the_motion_and_the_ground() {
        let mut h = harness();
        assert_eq!(h.state().inspector.ball_view.mode, ball_view::ViewMode::Still);
        h.get_by_label("Moving").click();
        h.run();
        assert_eq!(h.state().inspector.ball_view.mode, ball_view::ViewMode::Moving);
        h.get_by_label("Hit").click();
        h.run();
        assert_eq!(h.state().inspector.ball_view.mode, ball_view::ViewMode::Hit);
        assert_eq!(h.state().inspector.ball_view.ground, ball_view::Ground::Dark);
        h.get_by_label("Scene").click();
        h.run();
        assert_eq!(h.state().inspector.ball_view.ground, ball_view::Ground::Scene);
    }

    #[test]
    fn dragging_a_layer_onto_another_moves_it() {
        let mut h = harness();
        add(&mut h, "+ Add layer", "Outline");
        add(&mut h, "+ Add layer", "Glow");
        // Top first: Glow, Outline, Body. Drag Glow below Outline.
        let glow = h.get_by_role_and_label(Role::Button, "Glow").rect();
        let outline = h.get_by_role_and_label(Role::Button, "Outline").rect();
        let to = egui::pos2(outline.center().x, outline.bottom() + 2.0);
        h.drag_at(glow.center());
        h.run();
        h.hover_at(to);
        h.run();
        h.drop_at(to);
        h.run();
        assert_eq!(kinds_of(&h, BALL), [kinds::BODY, kinds::GLOW, kinds::OUTLINE]);
    }
}
