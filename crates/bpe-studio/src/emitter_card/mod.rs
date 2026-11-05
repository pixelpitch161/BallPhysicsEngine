//! The Emitter card (layout C): chips Emit | Catch | Marker over the emitter's own settings. Emit
//! has the mini view (drag to aim, spread the fan and set the speed), the rounds strip and what it
//! sits on; Catch and Marker are plain rows.

pub mod mini_view;
pub mod strip;

use bpe_core::{Address, SettingDef, Settings, ThingId, Value};
use bpe_kit::things::emitter::{self, Catches, EmitterSettings, FireWhen, SitsFocus, foci};
use eframe::egui::{RichText, Ui};

use super::{Edit, Inspector, OwnCx, own_grid};
use crate::style;
use crate::widgets::{grid_label, settings_like_grid};

/// Which of the Emitter card's tabs shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EmitterTab {
    #[default]
    Emit,
    Catch,
    Marker,
}

/// The emitter's settings as they are at the playhead.
pub fn settings_now(cx: &OwnCx) -> EmitterSettings {
    let reg = &cx.kit.registry;
    let mut p = bpe_core::Params::default();
    for d in EmitterSettings::defs() {
        if let Some(v) = cx.scene.value_at_time(reg, &Address::own(cx.id, d.key), cx.time) {
            p.set(d.key, v);
        }
    }
    EmitterSettings::from_params(&p)
}

/// The settings shown on the tab, from the kind's: those under its headings, but what the card
/// shows its own way (Sits on, Focus) and the Size (by the Look's Ball / Group switch).
fn tab_defs(cx: &OwnCx, headings: &[&str]) -> Vec<SettingDef> {
    let Some(kind) = cx.kit.registry.thing(emitter::KIND) else { return Vec::new() };
    kind.settings()
        .iter()
        .filter(|d| headings.contains(&d.group) && !matches!(d.key, "sits_on" | "sits_focus"))
        .cloned()
        .collect()
}

/// The Emitter card's insides: the chips and the tab they pick.
pub fn show(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, edits: &mut Vec<Edit>) {
    let id = cx.id;
    let mut tab = inspector.emitter_tab.get(&id).copied().unwrap_or_default();
    ui.horizontal(|ui| {
        crate::look_editor::ball_view::segmented(
            ui,
            &mut tab,
            &[
                (EmitterTab::Emit, "Emit", "What it fires, which way, how fast and when"),
                (EmitterTab::Catch, "Catch", "What happens to balls that reach it"),
                (EmitterTab::Marker, "Marker", "How it shows in the video: its mark, flashes and ripples"),
            ],
        );
    });
    inspector.emitter_tab.insert(id, tab);
    ui.add_space(6.0);
    let s = settings_now(cx);
    match tab {
        EmitterTab::Emit => {
            if mini_view::show(ui, inspector, cx, &s, edits) {
                inspector.own_step = true;
            }
            ui.add_space(4.0);
            let duration = bpe_kit::SceneSettings::from_params(&cx.scene.things[&ThingId::ROOT].params).duration;
            strip::show(ui, inspector.rounds.get(&id).map(Vec::as_slice).unwrap_or_default(), duration, cx.time);
            ui.add_space(6.0);
            sits_on_row(ui, cx, &s, edits);
            if !super::layered_look(cx.thing) {
                // Without a layered Look there is no Ball / Group switch to show the Size by.
                own_grid(ui, inspector, cx, ("emitter size", id), &tab_defs(cx, &[emitter::SIZE]), edits);
            }
            own_grid(ui, inspector, cx, ("emitter emit", id), &tab_defs(cx, &emitter::EMIT_HEADINGS), edits);
        }
        EmitterTab::Catch => {
            own_grid(ui, inspector, cx, ("emitter catch", id), &tab_defs(cx, &emitter::CATCH_HEADINGS), edits);
            if let Some(hint) = catch_hint(&s) {
                ui.label(RichText::new(hint).small().color(style::MUTED));
            }
        }
        EmitterTab::Marker => {
            own_grid(ui, inspector, cx, ("emitter marker", id), &tab_defs(cx, &emitter::MARKER_HEADINGS), edits);
        }
    }
}

/// A line under the Catch rows saying what the choice does that the rows can't.
pub fn catch_hint(s: &EmitterSettings) -> Option<&'static str> {
    match s.catches {
        Catches::Nothing => Some("Balls fly through it untouched, and don't count as arriving."),
        Catches::PassThrough => Some("Balls fly on but still count as arriving."),
        Catches::SendBackOut if s.fire_when == FireWhen::WhenAWaveLands => {
            Some("Balls sent back out still count as a wave landing.")
        }
        _ => None,
    }
}

/// A shape an emitter can sit on: its id, its name, and whether its two foci are apart (an
/// ellipse's) or both its middle (a circle's).
#[derive(Clone, Debug, PartialEq)]
pub struct Seat {
    pub id: ThingId,
    pub name: String,
    pub two_foci: bool,
}

/// The shapes an emitter can sit on, in tree order.
pub fn seats(scene: &bpe_core::Scene) -> Vec<Seat> {
    scene
        .order
        .iter()
        .filter_map(|id| scene.things.get(id).map(|t| (*id, t)))
        .filter(|(_, t)| foci::can_sit_on(t))
        .map(|(id, t)| Seat { id, name: t.name.clone(), two_foci: false })
        .collect()
}

/// The words under Sits on, when it sits on a focus.
pub fn sits_on_hint(s: &EmitterSettings, gravity: bool) -> Option<String> {
    if s.sits_on <= 0 || s.aim != emitter::Aim::Off {
        return None;
    }
    let mut hint = "Balls from here meet again at the other focus".to_owned();
    if gravity {
        hint.push_str(" if gravity is off");
    }
    hint.push('.');
    Some(hint)
}

/// Whether the emitter's balls fall: the scene has gravity and its Motion card lets it pull.
fn falls(cx: &OwnCx) -> bool {
    let reg = &cx.kit.registry;
    let scene_g = bpe_kit::SceneSettings::from_params(&cx.scene.things[&ThingId::ROOT].params).gravity;
    let scale =
        match cx.scene.value_at_time(reg, &Address::on(cx.id, bpe_core::SlotId::Motion, "gravity_scale"), cx.time) {
            Some(Value::Number(v)) => v,
            _ => 0.0,
        };
    scene_g.length() > 0.0 && scale != 0.0
}

/// The Sits on row: Free, or a focus of one of the scene's shapes that have them.
fn sits_on_row(ui: &mut Ui, cx: &OwnCx, s: &EmitterSettings, edits: &mut Vec<Edit>) {
    let seats = seats(cx.scene);
    let current = seats.iter().find(|seat| seat.id.0 as i32 == s.sits_on);
    let place = |seat: &Seat, focus: SitsFocus| match (seat.two_foci, focus) {
        (false, _) => format!("{} · middle", seat.name),
        (true, SitsFocus::Focus1) => format!("{} · focus 1", seat.name),
        (true, SitsFocus::Focus2) => format!("{} · focus 2", seat.name),
    };
    let shown = current.map_or_else(|| "Free".to_owned(), |seat| place(seat, s.sits_focus));
    settings_like_grid(("sits on", cx.id)).show(ui, |ui| {
        let label = grid_label(ui, cx.label_width, "Sits on")
            .on_hover_text("Keep it on a shape's focus as the shape moves, or let it stand where it is placed");
        ui.menu_button(shown, |ui| {
            ui.set_min_width(180.0);
            if ui.selectable_label(current.is_none(), "Free").on_hover_text("Where it is placed").clicked() {
                edits.push(Edit::SetOwn("sits_on".into(), Value::Int(0)));
                ui.close();
            }
            if seats.is_empty() {
                ui.label(RichText::new("No circles in the scene").small().color(style::MUTED));
            }
            for seat in &seats {
                let foci: &[(SitsFocus, &str)] = if seat.two_foci {
                    &[(SitsFocus::Focus1, "focus1"), (SitsFocus::Focus2, "focus2")]
                } else {
                    &[(SitsFocus::Focus1, "focus1")]
                };
                for &(focus, key) in foci {
                    let on = s.sits_on == seat.id.0 as i32 && (s.sits_focus == focus || !seat.two_foci);
                    if ui.selectable_label(on, place(seat, focus)).clicked() {
                        edits.push(Edit::SetOwn("sits_on".into(), Value::Int(seat.id.0 as i64)));
                        edits.push(Edit::SetOwn("sits_focus".into(), Value::Choice(key.into())));
                        ui.close();
                    }
                }
            }
        })
        .response
        .labelled_by(label.id);
        ui.end_row();
    });
    if let Some(hint) = sits_on_hint(s, falls(cx)) {
        ui.label(RichText::new(hint).small().color(style::MUTED));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;
    use bpe_core::{Resolved, Scene, resolve};
    use bpe_kit::Kit;
    use bpe_kit::things::emitter::sim::RoundMark;
    use eframe::egui;
    use eframe::egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
        shape: ThingId,
        emitter: ThingId,
    }

    /// The blank project with a circle shape and a new emitter, selected.
    fn harness() -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let shape = bpe_kit::add_thing(&mut scene, &kit.registry, "shape", ThingId::ROOT).unwrap();
        scene.rename_thing(shape, "Ring");
        let emitter = bpe_kit::add_thing(&mut scene, &kit.registry, emitter::KIND, ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut selection = Selection::default();
        selection.set(emitter);
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection, shape, emitter };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 3000.0))
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

    fn own(h: &Harness<'_, State>, key: &str) -> Option<Value> {
        let s = h.state();
        s.scene.things[&s.emitter].params.get(key).cloned()
    }

    #[test]
    fn the_chips_switch_between_emit_catch_and_marker() {
        let mut h = harness();
        assert!(h.query_by_label("Emitter view").is_some(), "Emit shows the mini view");
        assert!(h.query_by_label("Catches").is_none());
        h.get_by_role_and_label(Role::Button, "Catch").click();
        h.run();
        assert!(h.query_by_label("Emitter view").is_none());
        assert!(h.query_all_by_label("Catches").next().is_some());
        h.get_by_role_and_label(Role::Button, "Marker").click();
        h.run();
        assert!(h.query_all_by_label("Glow").next().is_some());
        assert!(h.query_by_label("Catches").is_none());
    }

    #[test]
    fn dragging_the_square_in_the_mini_view_aims() {
        let mut h = harness();
        let view = h.get_by_label("Emitter view").rect();
        let s = EmitterSettings::default();
        let map = mini_view::map_for(view, &s);
        let from = map.to_view(mini_view::aim_handle(&s));
        // Drag it round to the right of the middle: 0°.
        let to = map.to_view(glam::Vec2::new(mini_view::ray_length(&s), 0.0));
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
            h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 4.0)));
            h.step();
        }
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
        assert_eq!(own(&h, "aim_angle"), Some(Value::Number(0.0)));
    }

    #[test]
    fn the_strip_counts_the_rounds_it_is_given() {
        let mut h = harness();
        let id = h.state().emitter;
        let marks = vec![
            RoundMark { time: 0.0, round: 1, size: 10, fired: true },
            RoundMark { time: 4.0, round: 2, size: 20, fired: false },
        ];
        h.state_mut().inspector.rounds.insert(id, marks);
        h.run();
        assert!(h.query_by_label("Rounds: 1 fired so far, 1 more to come").is_some());
    }

    #[test]
    fn sits_on_lists_the_circles_and_picks_one() {
        let mut h = harness();
        h.get_by_label("Sits on").click();
        h.run();
        h.get_by_label("Ring · middle").click();
        h.run();
        let shape = h.state().shape;
        assert_eq!(own(&h, "sits_on"), Some(Value::Int(shape.0 as i64)));
        assert!(h.query_by_label("Balls from here meet again at the other focus.").is_some());
    }

    #[test]
    fn catch_hints_say_what_the_rows_cannot() {
        let pass = EmitterSettings { catches: Catches::PassThrough, ..EmitterSettings::default() };
        assert_eq!(catch_hint(&pass), Some("Balls fly on but still count as arriving."));
        let remove = EmitterSettings { catches: Catches::Remove, ..EmitterSettings::default() };
        assert_eq!(catch_hint(&remove), None);
    }
}
