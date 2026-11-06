//! The Emitter card (layout C): chips Emit | Catch | Marker over the emitter's own settings. Emit
//! has the mini view (drag to aim, spread the fan and set the speed), the rounds strip and what it
//! sits on; Catch and Marker are plain rows.

pub mod mini_view;
pub mod strip;

use bpe_core::{Address, SettingDef, Settings, ThingId, Value};
use bpe_kit::things::emitter::aim::AimCheck;
use bpe_kit::things::emitter::{self, Aim, Catches, EmitterSettings, FireWhen, SitsFocus, foci};
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
        .filter(|d| headings.contains(&d.group) && !matches!(d.key, "sits_on" | "sits_focus" | "aim_target"))
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
            own_grid(ui, inspector, cx, ("emitter emit", id), &tab_defs(cx, &[emitter::FIRE, emitter::ROUNDS]), edits);
            aim_rows(ui, inspector, cx, &s, edits);
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

/// The Aim rows: how it aims, at what (a menu, between Aim and the rest), when the balls arrive and
/// what happens to those that can't, then what may spoil it.
fn aim_rows(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, s: &EmitterSettings, edits: &mut Vec<Edit>) {
    let id = cx.id;
    let defs = tab_defs(cx, &[emitter::AIM]);
    let (first, rest): (Vec<SettingDef>, Vec<SettingDef>) = defs.into_iter().partition(|d| d.key == "aim");
    own_grid(ui, inspector, cx, ("emitter aim", id), &first, edits);
    if s.aim == Aim::Off || super::is_folded(inspector, (id, None), emitter::AIM) {
        return;
    }
    target_row(ui, cx, s, edits);
    // The rest of the heading's rows, under it without a heading of their own.
    let rest: Vec<SettingDef> = rest.into_iter().map(|d| SettingDef { group: "", ..d }).collect();
    own_grid(ui, inspector, cx, ("emitter aim rest", id), &rest, edits);
    for line in aim_lines(inspector.aim_checks.get(&id)) {
        ui.label(RichText::new(line).small().color(style::MUTED));
    }
}

/// What the card says under the Aim rows, from the playback's check of the aim.
pub fn aim_lines(check: Option<&AimCheck>) -> Vec<String> {
    let Some(c) = check else { return Vec::new() };
    if c.no_target {
        return vec!["Pick what to aim at.".to_owned()];
    }
    let mut out = Vec::new();
    if c.cant_reach > 0 {
        let what = match (c.cant_reach == 1, c.skip) {
            (true, true) => "it is skipped",
            (true, false) => "it goes anyway",
            (false, true) => "they are skipped",
            (false, false) => "they go anyway",
        };
        let when = if c.untimed { "" } else { " in time" };
        out.push(format!("{} of {} balls can't reach it{when}: {what}.", c.cant_reach, c.of));
    }
    if c.bends_keep {
        out.push(
            "Gravity, wind or drag bend the paths: with Keep mine few balls can meet. Try Adjust each ball.".to_owned(),
        );
    }
    if c.in_the_way {
        out.push("Something in the way: balls may miss.".to_owned());
    }
    if c.walls_move {
        out.push("The walls move: fewer balls find a way through.".to_owned());
    }
    if c.target_moves {
        out.push(
            "The target moves on its own: balls aim where it is heading and miss if it turns or bounces.".to_owned(),
        );
    }
    out
}

/// What the Target menu can aim at: the other focus of the shape it sits on (when it sits on
/// one), then every other thing but the scene and the camera, in tree order.
pub fn targets(scene: &bpe_core::Scene, me: ThingId) -> Vec<(i32, String)> {
    scene
        .order
        .iter()
        .filter(|id| **id != ThingId::ROOT && **id != me)
        .filter_map(|id| scene.things.get(id).map(|t| (*id, t)))
        .filter(|(_, t)| t.kind != "camera")
        .map(|(id, t)| (id.0 as i32, t.name.clone()))
        .collect()
}

/// The Target row: a menu of what the balls can be aimed at.
fn target_row(ui: &mut Ui, cx: &OwnCx, s: &EmitterSettings, edits: &mut Vec<Edit>) {
    let list = targets(cx.scene, cx.id);
    let auto = if s.sits_on > 0 { "The other focus" } else { "Nothing" };
    // A target that has gone aims at nothing.
    let gone = if s.aim_target > 0 { "Nothing" } else { auto };
    let shown = list.iter().find(|(id, _)| *id == s.aim_target).map_or(gone, |(_, name)| name.as_str()).to_owned();
    settings_like_grid(("aim target", cx.id)).show(ui, |ui| {
        let label = grid_label(ui, cx.label_width, "Target")
            .on_hover_text("What the balls arrive at together: another emitter or the middle of any thing");
        ui.menu_button(shown, |ui| {
            ui.set_min_width(180.0);
            let hover = if s.sits_on > 0 {
                "The focus across from the one it sits on"
            } else {
                "Sit it on a shape's focus to aim at the other one"
            };
            if ui.selectable_label(s.aim_target <= 0, auto).on_hover_text(hover).clicked() {
                edits.push(Edit::SetOwn("aim_target".into(), Value::Int(0)));
                ui.close();
            }
            for (id, name) in &list {
                if ui.selectable_label(s.aim_target == *id, name).clicked() {
                    edits.push(Edit::SetOwn("aim_target".into(), Value::Int(*id as i64)));
                    ui.close();
                }
            }
        })
        .response
        .labelled_by(label.id);
        ui.end_row();
    });
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
        .map(|(id, t)| Seat { id, name: t.name.clone(), two_foci: foci::two_foci(t) })
        .collect()
}

/// The words under Sits on, when it sits on `seat` (a circle's middle or an ellipse's focus)
/// with no aiming: where the round meets again, and what would spoil it (gravity pulling the
/// balls; on an ellipse, balls bouncing by their edge).
pub fn sits_on_hint(s: &EmitterSettings, seat: Option<&Seat>, gravity: bool, by_edge: bool) -> Option<String> {
    let seat = seat?;
    if s.sits_on <= 0 || s.aim != emitter::Aim::Off {
        return None;
    }
    let (mut hint, by_edge) = if seat.two_foci {
        ("Balls from here meet at the other focus".to_owned(), by_edge)
    } else {
        // From a circle's middle every ball comes straight back, whatever touches the wall.
        ("Balls from here come back to the middle together".to_owned(), false)
    };
    match (gravity, by_edge) {
        (true, true) => hint.push_str(" if gravity is off and balls touch walls with their middle"),
        (true, false) => hint.push_str(" if gravity is off"),
        (false, true) => hint.push_str(" if balls touch walls with their middle"),
        (false, false) => {}
    }
    hint.push('.');
    Some(hint)
}

/// Whether the emitter's balls touch walls with their edge (not their middle).
fn by_edge(cx: &OwnCx) -> bool {
    let at = Address::on(cx.id, bpe_core::SlotId::Bounce, "touch");
    !matches!(cx.scene.value_at_time(&cx.kit.registry, &at, cx.time), Some(Value::Choice(c)) if c == "middle")
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
                ui.label(RichText::new("No circles or ellipses in the scene").small().color(style::MUTED));
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
    if let Some(hint) = sits_on_hint(s, current, falls(cx), by_edge(cx)) {
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
    fn the_speed_varies_across_the_balls_from_its_menu() {
        let mut h = harness();
        h.get_all_by_role_and_label(Role::Label, "Speed").next().unwrap().click_secondary();
        h.run();
        h.get_by_label("Vary across balls").click();
        h.run();
        assert!(own(&h, "speed~to").is_some(), "the To end is stored");
        assert!(h.query_all_by_label("Speed varies by").next().is_some(), "the Varies by line shows");
        // Read as each ball is fired, it can't follow the balls' speed or hits.
        let (scene, id) = (&h.state().scene, h.state().emitter);
        let a = bpe_core::Address { thing: id, slot: None, key: "speed".into() };
        assert_eq!(scene.vary_use(&h.state().kit.registry, &a), Some(bpe_core::VaryUse::AtLaunch));
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
        assert!(h.query_by_label("Balls from here come back to the middle together.").is_some());
    }

    #[test]
    fn the_sits_on_hint_says_what_would_spoil_meeting_at_the_other_focus() {
        let s = EmitterSettings { sits_on: 3, ..EmitterSettings::default() };
        let ellipse = Seat { id: ThingId(3), name: "Oval".into(), two_foci: true };
        let circle = Seat { two_foci: false, ..ellipse.clone() };
        assert_eq!(
            sits_on_hint(&s, Some(&ellipse), false, false).as_deref(),
            Some("Balls from here meet at the other focus.")
        );
        assert_eq!(
            sits_on_hint(&s, Some(&ellipse), true, true).as_deref(),
            Some("Balls from here meet at the other focus if gravity is off and balls touch walls with their middle.")
        );
        assert_eq!(
            sits_on_hint(&s, Some(&ellipse), false, true).as_deref(),
            Some("Balls from here meet at the other focus if balls touch walls with their middle.")
        );
        assert_eq!(
            sits_on_hint(&s, Some(&circle), false, true).as_deref(),
            Some("Balls from here come back to the middle together."),
            "edge touching doesn't spoil a circle"
        );
        assert_eq!(sits_on_hint(&s, None, false, false), None);
    }

    #[test]
    fn aiming_shows_its_rows_and_the_target_menu_picks_another_emitter() {
        let mut h = harness();
        assert_eq!(h.query_all_by_label("Arrive after").count(), 0, "Aim off: no aim rows");
        assert!(h.query_all_by_label("Speed").next().is_some());
        let other = {
            let s = h.state_mut();
            let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, emitter::KIND, ThingId::ROOT).unwrap();
            s.scene.rename_thing(other, "Catcher");
            let me = s.emitter;
            s.scene.things.get_mut(&me).unwrap().params.set("aim", Value::Choice("straight_in".into()));
            s.resolved = resolve(&s.scene, &s.kit.registry);
            other
        };
        h.run();
        assert!(h.query_all_by_label("Arrive after").next().is_some());
        assert!(h.query_all_by_label("Can't reach").next().is_some());
        assert_eq!(h.query_all_by_label("Speed").count(), 0, "Straight in works the speed out");
        h.get_by_label("Target").click();
        h.run();
        h.get_by_label("Catcher").click();
        h.run();
        assert_eq!(own(&h, "aim_target"), Some(Value::Int(other.0 as i64)));
        // What the playback's check says shows under the rows.
        let id = h.state().emitter;
        let check = AimCheck { in_the_way: true, cant_reach: 3, of: 10, skip: true, ..AimCheck::default() };
        h.state_mut().inspector.aim_checks.insert(id, check);
        h.run();
        assert!(h.query_by_label("3 of 10 balls can't reach it in time: they are skipped.").is_some());
        assert!(h.query_by_label("Something in the way: balls may miss.").is_some());
    }

    #[test]
    fn through_bounces_shows_its_rows_and_hides_gap() {
        let mut h = harness();
        assert_eq!(h.query_all_by_label("Bounces at least").count(), 0, "Aim off: no bounce row");
        assert!(h.query_all_by_label("Gap").next().is_some());
        {
            let s = h.state_mut();
            let me = s.emitter;
            s.scene.things.get_mut(&me).unwrap().params.set("aim", Value::Choice("through_bounces".into()));
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        for row in ["Bounces at least", "Bounces at most", "Arrive", "Leave", "Ball directions", "Turn at most"] {
            assert!(h.query_all_by_label(row).next().is_some(), "{row}");
        }
        assert!(h.query_all_by_label("Speed").next().is_some(), "Speed is the fastest a ball goes");
        assert_eq!(h.query_all_by_label("Gap").count(), 0, "the round's balls leave together");
        assert_eq!(h.query_all_by_label("Arrive after").count(), 0, "as soon as all can: no time to arrive by");
        let id = h.state().emitter;
        let check = AimCheck {
            walls_move: true,
            bends_keep: true,
            untimed: true,
            cant_reach: 2,
            of: 8,
            skip: true,
            ..AimCheck::default()
        };
        h.state_mut().inspector.aim_checks.insert(id, check);
        h.run();
        assert!(h.query_by_label("The walls move: fewer balls find a way through.").is_some());
        assert!(h.query_by_label("2 of 8 balls can't reach it: they are skipped.").is_some());
        assert!(
            h.query_by_label(
                "Gravity, wind or drag bend the paths: with Keep mine few balls can meet. Try Adjust each ball."
            )
            .is_some()
        );
        // Keep mine: no turning; After: a time to arrive by.
        {
            let s = h.state_mut();
            let me = s.emitter;
            let params = &mut s.scene.things.get_mut(&me).unwrap().params;
            params.set("aim_way", Value::Choice("keep_mine".into()));
            params.set("aim_arrive", Value::Choice("after".into()));
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        assert_eq!(h.query_all_by_label("Turn at most").count(), 0);
        assert!(h.query_all_by_label("Arrive after").next().is_some());
        // Leaving when needed: the balls keep their own directions.
        {
            let s = h.state_mut();
            let me = s.emitter;
            s.scene.things.get_mut(&me).unwrap().params.set("aim_leave", Value::Choice("when_needed".into()));
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        assert_eq!(h.query_all_by_label("Ball directions").count(), 0);
    }

    #[test]
    fn aim_lines_say_what_to_fix() {
        assert!(aim_lines(None).is_empty());
        let none = AimCheck { no_target: true, ..AimCheck::default() };
        assert_eq!(aim_lines(Some(&none)), ["Pick what to aim at."]);
        let one = AimCheck { cant_reach: 1, of: 4, ..AimCheck::default() };
        assert_eq!(aim_lines(Some(&one)), ["1 of 4 balls can't reach it in time: it goes anyway."]);
        assert!(aim_lines(Some(&AimCheck { of: 4, ..AimCheck::default() })).is_empty());
    }

    #[test]
    fn catch_hints_say_what_the_rows_cannot() {
        let pass = EmitterSettings { catches: Catches::PassThrough, ..EmitterSettings::default() };
        assert_eq!(catch_hint(&pass), Some("Balls fly on but still count as arriving."));
        let remove = EmitterSettings { catches: Catches::Remove, ..EmitterSettings::default() };
        assert_eq!(catch_hint(&remove), None);
    }
}
