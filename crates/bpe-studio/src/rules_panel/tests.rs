//! The Rules view and a block's inspector, driven as the editor draws them.

use super::*;
use crate::settings::BottomView;
use bpe_core::{Settings, ThingId, Value, Who};
use bpe_kit::rules::change::ChangeSettingSettings;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

/// The bottom panel (timeline or rules) under the inspector, each frame recorded as the editor
/// does.
fn harness(studio: Studio) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, studio: &mut Studio| {
                let before = studio.scene.clone();
                studio.recorded = false;
                egui::Panel::bottom("timeline").exact_size(430.0).show(ui, |ui| crate::timeline::show(ui, studio));
                studio.inspector_panel(ui);
                egui::CentralPanel::default().show(ui, |_| {});
                studio.record_frame(before);
            },
            studio,
        );
    h.run();
    h
}

fn rules_studio() -> Studio {
    let mut studio = Studio::headless();
    studio.bottom = BottomView::Rules;
    studio
}

fn rule_of(studio: &Studio, n: usize) -> &Rule {
    &studio.scene.rules.list[n]
}

/// Adds a rule of blocks one after another; their ids.
fn lane_of(studio: &mut Studio, name: &str, kinds: &[&str]) -> (RuleId, Vec<BlockId>) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let before = studio.scene.clone();
    let rule = studio.scene.add_rule(name);
    let mut ids: Vec<BlockId> = Vec::new();
    for kind in kinds {
        ids.push(studio.scene.add_block(reg, rule, kind, ids.last().copied()).unwrap());
    }
    studio.after_separate_edit(before);
    (rule, ids)
}

#[test]
fn the_chips_switch_the_bottom_view() {
    let mut h = harness(Studio::headless());
    assert_eq!(h.state().bottom, BottomView::Timeline);
    assert!(h.query_by_label("+ Add rule").is_none());
    h.get_by_label("Rules").click();
    h.run();
    assert_eq!(h.state().bottom, BottomView::Rules);
    assert!(h.query_by_label("+ Add rule").is_some());
    h.get_by_label("Timeline").click();
    h.run();
    assert_eq!(h.state().bottom, BottomView::Timeline);
}

#[test]
fn adding_a_rule_a_when_and_a_do_after_it() {
    let mut h = harness(rules_studio());
    h.get_by_label("+ Add rule").click();
    h.run();
    assert_eq!(h.state().scene.rules.list.len(), 1);
    assert_eq!(rule_of(h.state(), 0).name, "Rule 1");
    h.get_by_label("Add When to Rule 1").click();
    h.run();
    h.get_by_label("Hit").click();
    h.run();
    let rule = rule_of(h.state(), 0).clone();
    assert_eq!(rule.blocks.len(), 1);
    assert_eq!(rule.blocks[0].kind, "when.hit");
    // The new block is picked: its settings show in the inspector.
    assert_eq!(h.state().picked_block(), Some((rule.id, rule.blocks[0].id)));
    assert!(h.query_by_label("When · Hit").is_some());
    h.get_by_label("Add after When Hit").click();
    h.run();
    h.get_by_label("Do").click();
    h.run();
    h.get_by_label("Change motion").click();
    h.run();
    let rule = rule_of(h.state(), 0).clone();
    assert_eq!(rule.blocks.len(), 2);
    assert_eq!(rule.blocks[1].kind, "do.motion");
    assert_eq!(rule.blocks[1].inputs, vec![rule.blocks[0].id], "linked after the When");
    assert!(h.query_by_label("Do Change motion").is_some());
    // Each was its own undo step.
    assert!(h.state_mut().undo());
    assert_eq!(rule_of(h.state(), 0).blocks.len(), 1);
}

#[test]
fn dragging_an_out_dot_onto_a_logic_block_links_it() {
    let mut studio = rules_studio();
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let (rule, ids) = lane_of(&mut studio, "Join", &["when.hit", "logic"]);
    let time = studio.scene.add_when(reg, rule, "when.time").unwrap();
    let mut h = harness(studio);
    let from = h.get_by_label("Link from When Time").rect().center();
    let to = h.get_by_label("Logic").rect().center();
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run();
    }
    h.drop_at(to);
    h.run();
    let logic = h.state().scene.rules.rule(rule).unwrap().block(ids[1]).unwrap().clone();
    assert_eq!(logic.inputs, vec![ids[0], time], "the Logic block takes a second input");
    // Dragging onto a When is refused, with a word why.
    let from = h.get_by_label("Link from When Hit").rect().center();
    let to = h.get_by_label("When Time").rect().center();
    h.drag_at(from);
    h.run();
    h.hover_at(to);
    h.run();
    h.drop_at(to);
    h.run();
    assert!(h.state().status.contains("When block"), "{}", h.state().status);
}

#[test]
fn a_blocks_settings_edit_it_in_the_inspector() {
    let mut studio = rules_studio();
    let (rule, ids) = lane_of(&mut studio, "Speed up", &["when.hit", "do.motion"]);
    studio.panels.rules.picked = Some((rule, ids[1]));
    let mut h = harness(studio);
    assert!(h.query_by_label("Do · Change motion").is_some());
    // Amount from the firing: the hit's values are offered.
    h.get_by_label("Amount from the firing").click();
    h.run();
    let block = h.state().scene.rules.rule(rule).unwrap().block(ids[1]).unwrap().clone();
    assert_eq!(block.params.get("amount~from"), Some(&Value::Text("strength".into())));
    assert!(h.query_all_by_label("Times").next().is_some() && h.query_all_by_label("Plus").next().is_some());
    h.get_by_label("Amount from the firing").click();
    h.run();
    let block = h.state().scene.rules.rule(rule).unwrap().block(ids[1]).unwrap().clone();
    assert!(block.params.get("amount~from").is_none(), "off again");
    // The who picker names a thing.
    h.state_mut().panels.rules.picked = Some((rule, ids[0]));
    h.run();
    let who = h.query_all_by_label("Who").find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button);
    who.expect("the who picker").click();
    h.run();
    h.get_by_label("Thing").click();
    h.run();
    h.get_by_label("Ball").click();
    h.run();
    let block = h.state().scene.rules.rule(rule).unwrap().block(ids[0]).unwrap().clone();
    assert_eq!(block.params.get("who"), Some(&Value::Who(Who::Thing { thing: ThingId(2), ball: None })));
}

#[test]
fn change_setting_picks_a_setting_from_its_menu() {
    let mut studio = rules_studio();
    let (rule, ids) = lane_of(&mut studio, "Bouncier", &["when.hit", "do.change"]);
    studio.panels.rules.picked = Some((rule, ids[1]));
    let mut h = harness(studio);
    assert!(h.query_by_label("To").is_none(), "no To before a setting is picked");
    h.get_by_label("No setting yet").click();
    h.run();
    h.get_by_label("Ball").click();
    h.run();
    h.get_by_label("Bounce").click();
    h.run();
    h.get_by_label("Bounciness").click();
    h.run();
    let block = h.state().scene.rules.rule(rule).unwrap().block(ids[1]).unwrap().clone();
    let s = ChangeSettingSettings::from_params(&block.params);
    assert_eq!((s.card, s.setting.as_str()), (bpe_kit::rules::change::CardChoice::Bounce, "bounciness"));
    let to = h.query_all_by_label("To").next().expect("a To row");
    assert_eq!(to.accesskit_node().role(), egui::accesskit::Role::Slider, "a number");
    assert!(h.query_by_label("Bounce · Bounciness").is_some(), "the menu names it");
}

#[test]
fn a_rule_naming_a_deleted_thing_shows_its_problem() {
    let mut studio = rules_studio();
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let (rule, ids) = lane_of(&mut studio, "Gone", &["when.hit"]);
    studio
        .scene
        .set_block_param(reg, rule, ids[0], "who", Value::Who(Who::Thing { thing: ThingId(2), ball: None }))
        .unwrap();
    let h = harness(studio);
    assert!(h.query_by_label("Problem in Gone").is_none());
    let mut h = h;
    h.state_mut().scene.remove_thing(ThingId(2)).unwrap();
    h.run();
    assert!(h.query_by_label("Problem in Gone").is_some());
}

#[test]
fn right_click_deletes_a_rule_and_undo_brings_it_back() {
    let mut studio = rules_studio();
    lane_of(&mut studio, "Doomed", &["when.time"]);
    let mut h = harness(studio);
    h.get_by_label("Rule Doomed").click_secondary();
    h.run();
    h.get_by_label("Delete").click();
    h.run();
    assert!(h.state().scene.rules.is_empty());
    assert!(h.state_mut().undo());
    assert_eq!(rule_of(h.state(), 0).name, "Doomed");
}

#[test]
fn the_timeline_gets_a_rules_lane_with_the_firings() {
    let mut studio = Studio::headless();
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let (rule, ids) = lane_of(&mut studio, "Tick", &["when.time", "do.motion"]);
    let before = studio.scene.clone();
    studio.scene.set_block_param(reg, rule, ids[0], "at", Value::Number(0.25)).unwrap();
    studio.after_edit(before);
    let mut h = harness(studio);
    assert!(h.query_by_label("Rules lane, 0 firings").is_some(), "shown once the scene has a rule");
    let s = h.state_mut();
    s.time = 1.0;
    s.playback.seek(1.0);
    h.run();
    assert!(h.query_by_label("Rules lane, 1 firings").is_some());
    // In the Rules view, the rule says it fired and the Do block rings until 0.4 s.
    h.state_mut().bottom = BottomView::Rules;
    h.run();
    assert_eq!(h.state().panels.rules.fired.count(rule_of(h.state(), 0), 1.0), 1);
    assert!(h.state().panels.rules.fired.recent(rule, ids[1], 0.3));
    assert!(!h.state().panels.rules.fired.recent(rule, ids[1], 1.0));
}
