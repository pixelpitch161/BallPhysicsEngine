//! The Look editor in Group mode, for a Balls thing: the Group view, then where the balls start
//! (the group's shape), the colour across the group (its group layers), and the folds for
//! Variety, Release, Launch and Odd ones out.

use bpe_core::{Address, Choice, Layer, LayerId, LayerSection, SettingDef, Settings, SlotId, ThingId, Value};
use bpe_kit::modules::look::layers::{self as kinds, Changes, GroupGradient, GroupMix, Spread};
use bpe_kit::things::balls::{GROUP, LAUNCH, RELEASE, VARIETY, shapes};
use bpe_kit::{BallsSettings, GroupShape};
use eframe::egui::{self, Rect, RichText, Sense, Stroke, Ui};
use glam::Vec2;

use super::group_view::{self, ViewCx};
use super::{LookCx, layer_list, mark_for};
use crate::inspector::{Edit, Inspector};
use crate::style;
use crate::widgets::{GridCx, fold_toggle, settings_grid};

/// The size of a shape chip.
const CHIP: egui::Vec2 = egui::vec2(60.0, 46.0);

/// Whether the inspector shows a Balls thing's look on one ball or across its group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LookMode {
    #[default]
    Ball,
    Group,
}

/// The group layer of `id`'s card shown under the group look list: the one picked, or the first.
pub fn selected_group_layer(
    inspector: &Inspector,
    card: &bpe_core::Slot,
    reg: &bpe_core::Registry,
    id: ThingId,
) -> Option<LayerId> {
    let group = |l: &&Layer| reg.layer(&l.kind).is_some_and(|k| k.section() == LayerSection::Group);
    let picked = inspector.selected_group_layer.get(&id).copied().filter(|l| card.layer(*l).is_some_and(|l| group(&l)));
    picked.or_else(|| card.layers.iter().find(group).map(|l| l.id))
}

/// Draws Group mode for the Balls thing `cx.id`, pushing what the user does to `edits`. `cx.slot`
/// is its Look card, which may be missing: then the colour across the group isn't offered.
pub fn show(ui: &mut Ui, inspector: &mut Inspector, cx: &LookCx, edits: &mut Vec<Edit>) {
    let reg = cx.reg();
    let card = cx.card().filter(|c| c.kind == kinds::CARD);
    let picked = card.and_then(|c| selected_group_layer(inspector, c, reg, cx.id));
    // The view shows where the balls start: everything as it is at the start.
    let start = LookCx { time: 0.0, ..*cx };
    let gradient = card
        .zip(picked)
        .and_then(|(c, l)| c.layer(l))
        .filter(|l| l.kind == kinds::GROUP_GRADIENT && l.enabled)
        .map(|l| (l.id, GroupGradient::from_params(&start.params_of(l.id, GroupGradient::defs()))));
    let view_cx = ViewCx { scene: cx.scene, kit: cx.kit, id: cx.id, time: 0.0, gradient };
    let s = view_cx.settings();
    // ---------- the Group view ----------
    {
        let Inspector { group_view, render, .. } = inspector;
        // Picked balls belong to the thing they were picked on.
        group_view.keep_picks_for(cx.id);
        group_view::show(ui, group_view, render.as_ref(), &view_cx, cx.slot, edits);
    }
    view_tools(ui, inspector, &s, edits);

    // ---------- Group ----------
    ui.add_space(6.0);
    heading(ui, "Group");
    let defs = |group: &str| -> Vec<SettingDef> {
        BallsSettings::defs()
            .iter()
            .filter(|d| d.group == group)
            .map(|d| SettingDef { group: "", ..d.clone() })
            .collect()
    };
    let group_defs = defs(GROUP);
    let (switch, rest): (Vec<SettingDef>, Vec<SettingDef>) = group_defs.into_iter().partition(|d| d.key == "group");
    own_rows(ui, cx, "group switch", &switch, edits);
    if !s.group {
        ui.label(RichText::new("One ball").color(style::MUTED))
            .on_hover_text("Turn Group on for many balls in a shape, released and launched together");
        return;
    }
    shape_chips(ui, &s, edits);
    let rest: Vec<SettingDef> = rest.into_iter().filter(|d| d.key != "shape" && d.key != "custom").collect();
    own_rows(ui, cx, "group settings", &rest, edits);
    shape_note(ui, &s);

    // ---------- Across the group ----------
    if let Some(card) = card {
        ui.add_space(8.0);
        colour_section(ui, inspector, cx, card, picked, edits);
    }

    // ---------- the folds ----------
    for heading in [VARIETY, RELEASE, LAUNCH] {
        ui.add_space(6.0);
        if fold(ui, inspector, cx.id, heading) {
            own_rows(ui, cx, heading, &defs(heading), edits);
            if heading == VARIETY {
                let shuffle = ui.button("Shuffle").on_hover_text("Pick each ball's variety again");
                if shuffle.clicked() {
                    edits.push(Edit::SetOwnHere("seed".into(), Value::Int(s.seed as i64 + 1)));
                }
            }
        }
    }
    ui.add_space(6.0);
    if fold(ui, inspector, cx.id, "Odd ones out") {
        odd_ones_out(ui, inspector, cx, card, edits);
    }
}

/// A heading for a part of Group mode.
fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(style::semibold(12.0)).color(style::MUTED));
}

/// A heading that folds what is under it, shut until opened. True while open.
fn fold(ui: &mut Ui, inspector: &mut Inspector, id: ThingId, title: &'static str) -> bool {
    let key = (id, Some(SlotId::Look), title);
    let open = inspector.opened.contains(&key);
    let mut toggled = false;
    ui.horizontal(|ui| {
        toggled |= fold_toggle(ui, open, title);
        let label = egui::Label::new(RichText::new(title).font(style::semibold(12.0)).color(style::MUTED))
            .selectable(false)
            .sense(Sense::click());
        toggled |= ui.add(label).clicked();
    });
    if toggled {
        if open {
            inspector.opened.remove(&key);
        } else {
            inspector.opened.insert(key);
        }
    }
    open != toggled
}

/// A grid of some of the thing's own settings; its edits set them.
fn own_rows(ui: &mut Ui, cx: &LookCx, salt: &str, defs: &[SettingDef], edits: &mut Vec<Edit>) {
    let (scene, reg, id, time) = (cx.scene, cx.reg(), cx.id, cx.time);
    let own = |key: &str| Address::own(id, key);
    let get = |key: &str| scene.value_at_time(reg, &own(key), time).expect("the kind's own settings all resolve");
    let marks = |key: &str| {
        let def = defs.iter().find(|d| d.key == key)?;
        Some(mark_for(scene, def, &own(key)))
    };
    let kind = scene.things.get(&id).map(|t| t.kind.clone());
    let applies = |d: &SettingDef| d.applies(kind.as_deref(), &|k| scene.value_at_time(reg, &own(k), time));
    let grid = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: &applies,
        folded: None,
        label_width: cx.label_width,
        track: style::colour_of(scene, id),
    };
    let out = settings_grid(ui, ("group mode", salt, id), defs, get, &grid);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetOwn(key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(None, key.into())));
}

/// The buttons under the view: + adds a ball to a custom shape.
fn view_tools(ui: &mut Ui, inspector: &mut Inspector, s: &BallsSettings, edits: &mut Vec<Edit>) {
    if !s.group {
        return;
    }
    ui.horizontal(|ui| {
        if s.shape == GroupShape::Custom {
            let empty = s.custom.is_empty();
            let add = ui
                .add(style::toggle_button("+ Add ball", empty))
                .on_hover_text("A ball one step right of the last one (or in the middle). Drag balls to place them; right-click one to take it away.");
            if add.clicked() {
                let next = match s.custom.last() {
                    Some(p) => [p[0] + s.step(), p[1]],
                    None => [0.0, 0.0],
                };
                let mut points = s.custom.clone();
                points.push(next);
                edits.push(Edit::SetOwnHere("custom".into(), Value::Points(points)));
            }
        }
        let view = &mut inspector.group_view;
        let pick = ui
            .add(style::toggle_button("Pick balls", view.picking))
            .on_hover_text("Click balls in the view to pick them, for layers of their own (Odd ones out)");
        if pick.clicked() {
            view.picking = !view.picking;
        }
        if !view.picked.is_empty() {
            ui.label(RichText::new(format!("{} picked", view.picked.len())).small().color(style::MUTED));
        }
    });
}

/// The shape chips, each with a small picture of its dots.
fn shape_chips(ui: &mut Ui, s: &BallsSettings, edits: &mut Vec<Edit>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for &name in GroupShape::NAMES {
            let Some(shape) = GroupShape::from_name(name) else { continue };
            let on = s.shape == shape;
            let (rect, response) = ui.allocate_exact_size(CHIP, Sense::click());
            let label = crate::widgets::pretty(name);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label.clone()));
            let fill = if on {
                style::SELECTED
            } else if response.hovered() {
                style::HOVER
            } else {
                style::RAISED
            };
            let painter = ui.painter();
            painter.rect_filled(rect, style::RADIUS, fill);
            if on {
                painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.5, style::TEXT), egui::StrokeKind::Inside);
            }
            let picture =
                Rect::from_min_max(rect.min + egui::vec2(6.0, 4.0), egui::pos2(rect.max.x - 6.0, rect.max.y - 16.0));
            paint_dots(painter, picture, shape, if on { style::TEXT_STRONG } else { style::MUTED });
            let ink = if on { style::TEXT_STRONG } else { style::TEXT };
            painter.text(
                egui::pos2(rect.center().x, rect.max.y - 8.0),
                egui::Align2::CENTER_CENTER,
                &label,
                egui::FontId::proportional(10.0),
                ink,
            );
            if response.on_hover_text(format!("Start the balls in a {}", label.to_lowercase())).clicked() && !on {
                if shape == GroupShape::Custom && s.custom.is_empty() {
                    // Custom starts from the places shown now, before the turn.
                    let now = shapes::offsets(&BallsSettings { turn: 0.0, ..s.clone() });
                    edits.push(Edit::SetOwnHere(
                        "custom".into(),
                        Value::Points(now.iter().map(|p| [p.x, p.y]).collect()),
                    ));
                }
                edits.push(Edit::SetOwn("shape".into(), Value::Choice(name.into())));
            }
        }
    });
}

/// A shape's dots, fitted into `rect`.
fn paint_dots(painter: &egui::Painter, rect: Rect, shape: GroupShape, ink: egui::Color32) {
    let sample = BallsSettings {
        group: true,
        shape,
        count: if shape == GroupShape::Heart { 14 } else { 12 },
        columns: 4,
        radius: 1.0,
        space: 1.0,
        ring_radius: 5.0,
        word: "A".into(),
        letter_height: 22.0,
        area: Vec2::new(14.0, 10.0),
        custom: vec![[-4.0, 2.0], [-1.0, -3.0], [2.0, 1.0], [5.0, -2.0], [3.0, 4.0]],
        ..BallsSettings::default()
    };
    let points = shapes::offsets(&sample);
    let (lo, hi) = shapes::bounds(&points);
    let size = (hi - lo).max_element().max(1e-3);
    let scale = (rect.width().min(rect.height()) / size).min(4.0);
    let mid = (lo + hi) * 0.5;
    let dot = (scale * 0.9).clamp(1.2, 2.6);
    for p in points {
        let q = (p - mid) * scale;
        painter.circle_filled(rect.center() + egui::vec2(q.x, q.y), dot, ink);
    }
}

/// A word on how many balls the shape holds, where that isn't How many.
fn shape_note(ui: &mut Ui, s: &BallsSettings) {
    let placed = shapes::offsets(s).len();
    let note = match s.shape {
        GroupShape::Word if s.word.trim().is_empty() || shapes::word(&s.word, s.letter_height, s.step()).is_empty() => {
            "No letters to place".to_owned()
        }
        GroupShape::Custom if s.custom.is_empty() => "No balls placed yet: + adds one".to_owned(),
        GroupShape::Word if placed >= shapes::MAX_BALLS => {
            format!("Only {} balls fit: make the letters smaller or the balls bigger", shapes::MAX_BALLS)
        }
        GroupShape::FilledRing if placed >= shapes::MAX_BALLS => {
            format!("Only {} balls fit: make the radius smaller or the balls bigger", shapes::MAX_BALLS)
        }
        GroupShape::Word | GroupShape::Custom | GroupShape::FilledRing => balls(placed),
        _ if placed < (s.count as usize).min(shapes::MAX_BALLS) => format!("Only {placed} fit"),
        _ => return,
    };
    ui.label(RichText::new(note).small().color(style::MUTED));
}

fn balls(n: usize) -> String {
    if n == 1 { "1 ball".into() } else { format!("{n} balls") }
}

/// Across the group: the group layers (gradients and shaders), top of the list applied last, then
/// the picked one's settings.
fn colour_section(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    card: &bpe_core::Slot,
    picked: Option<LayerId>,
    edits: &mut Vec<Edit>,
) {
    let reg = cx.reg();
    ui.horizontal(|ui| {
        heading(ui, "Across the group");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(RichText::new("+ Add").small(), |ui| {
                ui.set_min_width(170.0);
                for kind in reg.layers_for(&card.kind).filter(|k| k.section() == LayerSection::Group) {
                    let free = kind.repeats() || !card.layers.iter().any(|l| l.kind == kind.id());
                    let item = ui.add_enabled(free, egui::Button::new(kind.name()));
                    if item.on_hover_text(kind.help()).on_disabled_hover_text("This look has one already").clicked() {
                        edits.push(Edit::AddLayer(cx.slot, kind.id()));
                        ui.close();
                    }
                }
            });
        });
    });
    let group: Vec<&Layer> =
        card.layers.iter().filter(|l| reg.layer(&l.kind).is_some_and(|k| k.section() == LayerSection::Group)).collect();
    let Some(picked) = picked else {
        ui.label(
            RichText::new("None yet: add a Gradient or a Shader to spread a look across the group.")
                .small()
                .color(style::FAINT),
        );
        return;
    };
    for layer in group.iter().rev() {
        layer_list::row(ui, inspector, cx, card, layer, picked, edits);
    }
    let Some(layer) = card.layer(picked) else { return };
    let Some(kind) = reg.layer(&layer.kind) else { return };
    ui.add_space(4.0);
    let gradient = layer.kind == kinds::GROUP_GRADIENT;
    let g = gradient.then(|| GroupGradient::from_params(&cx.params_of(layer.id, GroupGradient::defs())));
    if let Some(g) = &g
        && g.spread == Spread::Place
    {
        let view = &mut inspector.group_view;
        let edit = ui
            .add(style::toggle_button("Edit in view", view.edit_gradient))
            .on_hover_text("Drag the line's end dots to turn and stretch the colours, its diamond to ease them; double-click the diamond for the next easing");
        if edit.clicked() {
            view.edit_gradient = !view.edit_gradient;
        }
    }
    // A shader's Effect and Code have rows of their own; Choose's layers are a checklist.
    let own_rows = super::layer_rows::show(ui, inspector, cx, card, layer, edits);
    let get = |key: &str| cx.scene.value_at_time(reg, &cx.at(layer.id, key), cx.time);
    let defs: Vec<SettingDef> = kind
        .settings()
        .iter()
        .chain(kind.dynamic_defs(&get))
        .filter(|d| d.key != "chosen" && !own_rows.contains(&d.key))
        .cloned()
        .collect();
    super::layer_settings(ui, inspector, cx, layer.id, &defs, edits);
    let chosen = match g {
        Some(g) => (g.mix != GroupMix::Off && g.changes == Changes::Choose).then(|| g.chosen_ids()),
        None if layer.kind == kinds::GROUP_SHADER => {
            let s = kinds::GroupShader::from_params(&cx.params_of(layer.id, kind.settings()));
            (s.changes == Changes::Choose).then(|| s.chosen_ids())
        }
        None => None,
    };
    if let Some(chosen) = chosen {
        choose_list(ui, cx, card, layer.id, &chosen, edits);
    }
}

/// The drawn layers to tick for Choose: those the group colour or shader goes into.
fn choose_list(ui: &mut Ui, cx: &LookCx, card: &bpe_core::Slot, layer: LayerId, chosen: &[u32], edits: &mut Vec<Edit>) {
    let reg = cx.reg();
    ui.label(RichText::new("Layers it changes").small().color(style::MUTED));
    // The drawn layers that have colours of their own (not Blend).
    let coloured = |l: &&Layer| reg.layer(&l.kind).is_some_and(|k| k.section() == LayerSection::Drawn && k.per_ball());
    for l in card.layers.iter().filter(coloured).rev() {
        let mut on = chosen.contains(&l.id.0);
        if ui.checkbox(&mut on, &l.name).changed() {
            let mut ids: Vec<u32> = chosen.iter().copied().filter(|id| *id != l.id.0).collect();
            if on {
                ids.push(l.id.0);
            }
            ids.sort_unstable();
            let text = ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            edits.push(Edit::SetSlot(cx.slot, bpe_core::layered_key(layer, "chosen"), Value::Text(text)));
        }
    }
}

/// Odd ones out: pick balls in the view, then give them a layer of their own.
fn odd_ones_out(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &LookCx,
    card: Option<&bpe_core::Slot>,
    edits: &mut Vec<Edit>,
) {
    let view = &mut inspector.group_view;
    ui.label(
        RichText::new("Pick balls in the view, then give them a layer only they wear.").small().color(style::MUTED),
    );
    ui.horizontal(|ui| {
        let pick = ui.add(style::toggle_button("Pick balls", view.picking));
        if pick.clicked() {
            view.picking = !view.picking;
        }
        let clear = ui.add_enabled(!view.picked.is_empty(), egui::Button::new("Clear picks"));
        if clear.clicked() {
            view.picked.clear();
        }
    });
    let picked: Vec<u32> = view.picked.iter().copied().collect();
    let Some(card) = card else { return };
    let reg = cx.reg();
    ui.add_enabled_ui(!picked.is_empty(), |ui| {
        ui.menu_button("Give picked balls a layer", |ui| {
            ui.set_min_width(170.0);
            let fits = |k: &&dyn bpe_core::LayerKind| k.section() == LayerSection::Drawn && !k.base() && k.per_ball();
            for kind in reg.layers_for(&card.kind).filter(fits) {
                let free = kind.repeats() || !card.layers.iter().any(|l| l.kind == kind.id());
                let item = ui.add_enabled(free, egui::Button::new(kind.name()));
                if item.on_hover_text(kind.help()).clicked() {
                    edits.push(Edit::AddLayerFor(cx.slot, kind.id(), picked.clone()));
                    ui.close();
                }
            }
        })
        .response
        .on_disabled_hover_text("Pick some balls in the view first");
    });
    if !picked.is_empty() {
        ui.label(RichText::new(format!("{} picked", picked.len())).small().color(style::MUTED));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;
    use bpe_core::{Resolved, Scene, resolve};
    use bpe_kit::Kit;
    use eframe::egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
        history: crate::history::History,
    }

    const BALL: ThingId = ThingId(2);

    /// The blank project with one new ball, selected, in Group mode.
    fn harness() -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        assert_eq!(ball, BALL);
        let resolved = resolve(&scene, &kit.registry);
        let mut selection = Selection::default();
        selection.set(ball);
        let history = crate::history::History::default();
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection, history };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 4000.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let before = s.scene.clone();
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
                    // As the app does: edits while a button is held are one undo step.
                    if s.scene != before {
                        s.history.record(before, false);
                    }
                    if !ui.input(|i| i.pointer.any_down()) {
                        s.history.end_merging();
                    }
                },
                state,
            );
        h.run();
        h.get_by_role_and_label(Role::Button, "Group").click();
        h.run();
        h
    }

    fn own(h: &Harness<'_, State>, key: &str) -> Option<Value> {
        h.state().scene.things[&BALL].params.get(key).cloned()
    }

    fn turn_group_on(h: &mut Harness<'_, State>) {
        h.state_mut().scene.things.get_mut(&BALL).unwrap().params.set("group", Value::Bool(true));
        h.run();
    }

    fn open(h: &mut Harness<'_, State>, fold: &str) {
        h.get_by_label(fold).click();
        h.run();
    }

    /// The Group view's rectangle.
    fn view(h: &Harness<'_, State>) -> Rect {
        h.get_by_label("Group view").rect()
    }

    /// Presses at `from`, drags to `to` over a few frames and lets go.
    fn drag(h: &mut Harness<'_, State>, from: egui::Pos2, to: egui::Pos2) {
        h.hover_at(from);
        h.run();
        h.event(egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        for k in 1..=4 {
            let p = from + (to - from) * (k as f32 / 4.0);
            h.event(egui::Event::PointerMoved(p));
            h.step();
        }
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }

    /// Where world point `w` shows in the view, as the view maps it now.
    fn screen(h: &Harness<'_, State>, w: Vec2) -> egui::Pos2 {
        let (centre, size) = h.state().inspector.group_view.fit().unwrap();
        group_view::ViewMap { rect: view(h), centre, size }.to_screen(w)
    }

    fn settings(h: &Harness<'_, State>) -> BallsSettings {
        BallsSettings::from_params(&h.state().scene.things[&BALL].params)
    }

    #[test]
    fn typing_how_many_sets_it() {
        let mut h = harness();
        turn_group_on(&mut h);
        h.get_by_role_and_label(Role::SpinButton, "How many").click();
        h.run();
        // The box starts with its text picked, so typing replaces it.
        for c in ["2", "5"] {
            h.event(egui::Event::Text(c.into()));
            h.run();
        }
        assert_eq!(own(&h, "count"), Some(Value::Int(25)), "typed numbers aren't snapped to tens");
    }

    #[test]
    fn the_ball_group_switch_heads_the_look_card_or_the_balls_card_without_one() {
        let mut h = harness();
        // Once, at the top of the Look card, under its title.
        assert_eq!(h.query_all_by_role_and_label(Role::Button, "Group").count(), 1);
        let switch = h.get_by_role_and_label(Role::Button, "Group").rect();
        let look = h.query_all_by_label("Look").next().unwrap().rect();
        assert!(switch.top() > look.bottom(), "{switch:?} under {look:?}");
        h.state_mut().scene.things.get_mut(&BALL).unwrap().slots.remove(&SlotId::Look);
        h.run();
        assert_eq!(h.query_all_by_role_and_label(Role::Button, "Group").count(), 1, "in the Balls card");
    }

    #[test]
    fn group_mode_with_the_group_off_says_one_ball() {
        let h = harness();
        assert_eq!(h.state().inspector.look_mode.get(&BALL), Some(&LookMode::Group));
        assert!(h.query_by_label("One ball").is_some());
        assert!(h.query_by_role_and_label(Role::Button, "Ring").is_none(), "no shapes while it's one ball");
        assert!(h.query_by_label("Across the group").is_none());
        assert!(h.query_by_label("+ Add layer").is_none(), "the ball's own lists are gone");
    }

    #[test]
    fn turning_the_group_on_shows_its_shapes_and_ring_shows_its_radius() {
        let mut h = harness();
        h.get_by_role_and_label(Role::CheckBox, "Group").click();
        h.run();
        assert_eq!(own(&h, "group"), Some(Value::Bool(true)));
        assert!(h.query_by_label("One ball").is_none());
        assert!(h.query_by_role_and_label(Role::Label, "Columns").is_some(), "a grid has columns");
        assert!(h.query_by_role_and_label(Role::Label, "Space between").is_some());
        h.get_by_role_and_label(Role::Button, "Ring").click();
        h.run();
        assert_eq!(own(&h, "shape"), Some(Value::Choice("ring".into())));
        assert!(h.query_by_role_and_label(Role::Label, "Radius").is_some());
        assert!(h.query_by_role_and_label(Role::Label, "Columns").is_none());
        assert!(h.query_by_role_and_label(Role::Label, "Space between").is_none());
    }

    #[test]
    fn the_ball_card_no_longer_lists_the_group_settings() {
        let mut h = harness();
        turn_group_on(&mut h);
        // How many shows once, in Group mode, not on the Balls card too.
        assert_eq!(h.get_all_by_role_and_label(Role::Label, "How many").count(), 1);
        h.get_by_role_and_label(Role::Button, "Ball").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "How many").is_none());
        assert!(h.query_by_label("+ Add layer").is_some(), "the ball's own lists are back");
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some(), "launch speed stays");
    }

    #[test]
    fn scatter_says_how_many_fit_and_word_how_many_balls() {
        let mut h = harness();
        turn_group_on(&mut h);
        let p = &mut h.state_mut().scene.things.get_mut(&BALL).unwrap().params;
        p.set("shape", Value::Choice("scatter".into()));
        p.set("count", Value::Int(400));
        p.set("area", Value::Vec2([200.0, 200.0]));
        h.run();
        assert!(h.query_all_by_label("Only").next().is_none());
        let fit = shapes::offsets(&settings(&h)).len();
        assert!(fit < 400);
        assert!(h.query_by_label(&format!("Only {fit} fit")).is_some());
        h.state_mut().scene.things.get_mut(&BALL).unwrap().params.set("shape", Value::Choice("word".into()));
        h.run();
        let n = shapes::offsets(&settings(&h)).len();
        assert!(h.query_by_label(&format!("{n} balls")).is_some());
        h.state_mut().scene.things.get_mut(&BALL).unwrap().params.set("word", Value::Text("  ".into()));
        h.run();
        assert!(h.query_by_label("No letters to place").is_some());
    }

    #[test]
    fn adding_a_gradient_shows_its_settings_and_undo_takes_it_away() {
        let mut h = harness();
        turn_group_on(&mut h);
        h.get_by_label("+ Add").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Gradient").click();
        h.run();
        let card = h.state().scene.card(BALL, SlotId::Look).unwrap().clone();
        let layer = card.layers.last().unwrap().clone();
        assert_eq!(layer.kind, kinds::GROUP_GRADIENT);
        assert_eq!(h.state().inspector.selected_group_layer.get(&BALL), Some(&layer.id));
        assert!(h.query_by_role_and_label(Role::Label, "Spread by").is_some());
        assert!(h.query_by_role_and_label(Role::Label, "With ball colours").is_some());
        // It isn't in the ball's own lists.
        h.get_by_role_and_label(Role::Button, "Ball").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Button, "Gradient").is_none());
        let mut scene = h.state().scene.clone();
        let history = &mut h.state_mut().history;
        assert!(history.undo(&mut scene));
        assert!(scene.card(BALL, SlotId::Look).unwrap().layers.iter().all(|l| l.kind != kinds::GROUP_GRADIENT));
    }

    #[test]
    fn a_shader_across_the_group_shows_its_effect_and_choose_lists_the_layers() {
        let mut h = harness();
        turn_group_on(&mut h);
        h.get_by_label("+ Add").click();
        h.run();
        // Shaders repeat, so the menu's Shader is there however many the look has.
        h.get_by_role_and_label(Role::Button, "Shader").click();
        h.run();
        let card = h.state().scene.card(BALL, SlotId::Look).unwrap().clone();
        let layer = card.layers.last().unwrap().clone();
        assert_eq!(layer.kind, kinds::GROUP_SHADER);
        assert_eq!(h.state().inspector.selected_group_layer.get(&BALL), Some(&layer.id));
        for label in ["Effect", "Speed", "With ball colours", "Colours it changes", "Where it shows"] {
            assert!(h.query_all_by_label(label).next().is_some(), "{label}");
        }
        assert!(h.query_by_label("Edit in view").is_none(), "the gradient tool is the gradient's");
        assert!(h.query_by_label("Layers it changes").is_none());
        let kit = bpe_kit::kit();
        let at = bpe_core::Address::on(BALL, SlotId::Look, &bpe_core::layered_key(layer.id, "changes"));
        h.state_mut().scene.set_at(&kit.registry, &at, Value::Choice("choose".into())).unwrap();
        h.run();
        assert!(h.query_by_label("Layers it changes").is_some());
        assert!(h.query_by_role_and_label(Role::Label, "Where it shows").is_none(), "only over the whole ball");
    }

    #[test]
    fn choose_ticks_the_layers_the_colour_goes_into() {
        let mut h = harness();
        turn_group_on(&mut h);
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let scene = &mut h.state_mut().scene;
        let outline = scene.add_layer(reg, BALL, SlotId::Look, kinds::OUTLINE).unwrap();
        let g = scene.add_layer(reg, BALL, SlotId::Look, kinds::GROUP_GRADIENT).unwrap();
        let look = scene.card_mut(BALL, SlotId::Look).unwrap();
        look.params.set(&bpe_core::layered_key(g, "changes"), Value::Choice("choose".into()));
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Layers").is_none(), "the id text isn't shown");
        h.get_by_role_and_label(Role::CheckBox, "Outline").click();
        h.run();
        let card = h.state().scene.card(BALL, SlotId::Look).unwrap();
        assert_eq!(card.params.get(&bpe_core::layered_key(g, "chosen")), Some(&Value::Text(outline.0.to_string())));
    }

    #[test]
    fn shuffle_picks_the_variety_again() {
        let mut h = harness();
        turn_group_on(&mut h);
        open(&mut h, "Variety");
        assert!(h.query_by_role_and_label(Role::Label, "Size spread").is_some());
        h.get_by_label("Shuffle").click();
        h.run();
        assert_eq!(own(&h, "seed"), Some(Value::Int(2)));
        assert_eq!(h.get_all_by_role_and_label(Role::Label, "Release").count(), 1, "the fold alone");
        open(&mut h, "Release");
        assert_eq!(h.get_all_by_role_and_label(Role::Label, "Release").count(), 2, "and its setting");
        open(&mut h, "Launch");
        assert!(h.query_by_role_and_label(Role::Label, "Fan angle").is_none(), "only for a fan");
    }

    #[test]
    fn choosing_custom_starts_from_the_shape_shown() {
        let mut h = harness();
        turn_group_on(&mut h);
        let p = &mut h.state_mut().scene.things.get_mut(&BALL).unwrap().params;
        p.set("count", Value::Int(4));
        p.set("columns", Value::Int(2));
        p.set("turn", Value::Number(90.0));
        h.run();
        let before = shapes::offsets(&settings(&h));
        h.get_by_role_and_label(Role::Button, "Custom").click();
        h.run();
        let s = settings(&h);
        assert_eq!(s.shape, GroupShape::Custom);
        assert_eq!(s.custom.len(), 4);
        let after = shapes::offsets(&s);
        for (a, b) in before.iter().zip(&after) {
            assert!(a.distance(*b) < 1e-3, "{a} {b}");
        }
        // + adds one a step right of the last.
        h.get_by_label("+ Add ball").click();
        h.run();
        let s2 = settings(&h);
        assert_eq!(s2.custom.len(), 5);
        assert_eq!(s2.custom[4], [s.custom[3][0] + s.step(), s.custom[3][1]]);
    }

    #[test]
    fn dragging_a_custom_ball_moves_it_in_one_undo_step_and_right_click_removes_it() {
        let mut h = harness();
        turn_group_on(&mut h);
        let p = &mut h.state_mut().scene.things.get_mut(&BALL).unwrap().params;
        p.set("shape", Value::Choice("custom".into()));
        p.set("custom", Value::Points(vec![[-100.0, 0.0], [100.0, 0.0]]));
        h.run();
        let before = h.state().scene.clone();
        let from = screen(&h, Vec2::new(100.0, 0.0));
        drag(&mut h, from, from + egui::vec2(0.0, 20.0));
        let s = settings(&h);
        assert_eq!(s.custom[0], [-100.0, 0.0]);
        assert!((s.custom[1][0] - 100.0).abs() < 1.0 && s.custom[1][1] > 10.0, "{:?}", s.custom);
        let mut scene = h.state().scene.clone();
        assert!(h.state_mut().history.undo(&mut scene));
        assert_eq!(scene, before, "the whole drag is one undo step");
        let at = screen(&h, Vec2::new(-100.0, 0.0));
        h.hover_at(at);
        h.run();
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
        assert_eq!(settings(&h).custom.len(), 1);
    }

    #[test]
    fn the_group_view_has_no_launch_arrow_to_drag() {
        let mut h = harness();
        turn_group_on(&mut h);
        let s = settings(&h);
        let places = shapes::offsets(&s);
        let (lo, hi) = shapes::bounds(&places);
        // Where the old arrow's tip was: dragging there changes nothing.
        let tip = screen(&h, (lo + hi) * 0.5 + s.velocity * 0.3);
        let before = h.state().scene.clone();
        drag(&mut h, tip, tip + egui::vec2(-30.0, 0.0));
        assert_eq!(h.state().scene, before);
    }

    #[test]
    fn the_gradient_tool_turns_stretches_and_eases_in_one_undo_step_each() {
        let mut h = harness();
        turn_group_on(&mut h);
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let g = h.state_mut().scene.add_layer(reg, BALL, SlotId::Look, kinds::GROUP_GRADIENT).unwrap();
        h.run();
        h.get_by_label("Edit in view").click();
        h.run();
        let gradient = || GroupGradient::default();
        let places = shapes::offsets(&settings(&h));
        let (a, b) = group_view::gradient_ends(&gradient(), &places);
        let before = h.state().scene.clone();
        // The end dot, dragged down: the line turns clockwise.
        let end = screen(&h, b);
        drag(&mut h, end, end + egui::vec2(0.0, 40.0));
        let look = |h: &Harness<'_, State>, key: &str| {
            h.state().scene.card(BALL, SlotId::Look).unwrap().params.get(&bpe_core::layered_key(g, key)).cloned()
        };
        match look(&h, "angle") {
            Some(Value::Number(angle)) => assert!(angle > 5.0, "{angle}"),
            other => panic!("{other:?}"),
        }
        let mut scene = h.state().scene.clone();
        assert!(h.state_mut().history.undo(&mut scene));
        assert_eq!(scene, before, "one undo step");
        // The diamond, dragged towards the end: Ease in.
        h.state_mut().scene = before.clone();
        h.run();
        let diamond = screen(&h, a.lerp(b, 0.5));
        let towards = screen(&h, a.lerp(b, 0.75));
        drag(&mut h, diamond, towards);
        assert_eq!(look(&h, "ease"), Some(Value::Choice("ease_in".into())));
        // Half way to the end puts the middle colour at three quarters: t^k = 0.5 at t = 0.75.
        match look(&h, "ease_strength") {
            Some(Value::Number(k)) => assert!((k - 2.4).abs() < 0.15, "{k}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn picks_stay_with_their_thing_and_blend_isn_t_offered() {
        let mut h = harness();
        turn_group_on(&mut h);
        h.state_mut().inspector.group_view.picked.insert(3);
        h.run();
        open(&mut h, "Odd ones out");
        h.get_by_label("Give picked balls a layer").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Button, "Outline").is_some());
        assert!(h.query_by_role_and_label(Role::Button, "Blend").is_none(), "Blend is every ball's");
        h.key_press(egui::Key::Escape);
        // Another ball thing, selected: the picks were the first one's.
        let kit = bpe_kit::kit();
        let other = bpe_kit::add_thing(&mut h.state_mut().scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        h.state_mut().selection.set(other);
        h.state_mut().inspector.look_mode.insert(other, LookMode::Group);
        h.run();
        assert!(h.state().inspector.group_view.picked.is_empty());
    }

    #[test]
    fn picked_balls_get_a_layer_of_their_own() {
        let mut h = harness();
        turn_group_on(&mut h);
        h.get_all_by_label("Pick balls").next().unwrap().click();
        h.run();
        let places = shapes::offsets(&settings(&h));
        for n in [0usize, 7] {
            let at = screen(&h, places[n]);
            h.hover_at(at);
            h.run();
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            });
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.run();
            // Far enough apart in time that the second isn't a double click.
            for _ in 0..30 {
                h.step();
            }
        }
        assert_eq!(h.state().inspector.group_view.picked.iter().copied().collect::<Vec<_>>(), [0, 7]);
        open(&mut h, "Odd ones out");
        h.get_by_label("Give picked balls a layer").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Outline").click();
        h.run();
        let card = h.state().scene.card(BALL, SlotId::Look).unwrap().clone();
        let outline = card.layers.iter().find(|l| l.kind == kinds::OUTLINE).unwrap();
        assert_eq!(outline.only, [0, 7]);
        // The ball's own list says so, and Apply to every ball clears it.
        h.get_by_role_and_label(Role::Button, "Ball").click();
        h.run();
        assert!(h.query_by_label_contains("only 2 balls").is_some());
        h.get_by_role_and_label(Role::Button, "Outline").click_secondary();
        h.run();
        h.get_by_label("Apply to every ball").click();
        h.run();
        let card = h.state().scene.card(BALL, SlotId::Look).unwrap();
        assert!(card.layers.iter().find(|l| l.kind == kinds::OUTLINE).unwrap().only.is_empty());
    }
}
