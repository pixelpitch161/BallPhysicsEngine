//! The Rules view in the bottom panel (rules part 2, plan `docs/superpowers/plans/rules/part2.md`):
//! a lane per rule, its header (on switch, name, how often it fired, a problem mark) and its blocks
//! laid out from their links, with lines between them. Blocks are added from the block menu,
//! linked by dragging a block's out-dot onto another, and picked to show their settings in the
//! inspector ([`inspect`]).

pub mod face;
pub mod fired;
pub mod inspect;
pub mod layout;
pub mod menu;
pub mod setting_menu;

#[cfg(test)]
mod tests;

use crate::app::{Studio, clock};
use crate::style;
use crate::widgets::TargetChoices;
use bpe_core::rules::{CopiedBlock, CopiedRule};
use bpe_core::{BlockId, BlockSection, EditError, Registry, Rule, RuleId, Scene};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use face::{BLOCK_H, BLOCK_W, Looks};
use fired::Fired;
use std::collections::BTreeMap;

/// The header column of a lane.
pub const HEADER_W: f32 = 150.0;
/// Between two columns of blocks: room for the lines and the "+".
const COLUMN_GAP: f32 = 44.0;
const ROW_GAP: f32 = 10.0;
const PAD: f32 = 8.0;
/// The strip under a lane's blocks that holds "+ When".
const ADD_ROW: f32 = 24.0;
/// Between the header and the first column.
const BLOCKS_INSET: f32 = 14.0;
/// A right-click this close to a line opens its menu.
const LINE_REACH: f32 = 6.0;
const DOT_R: f32 = 4.0;

/// What the Rules view remembers between frames.
#[derive(Default)]
pub struct RulesPanel {
    /// The block whose settings the inspector shows (while no thing is selected).
    pub picked: Option<(RuleId, BlockId)>,
    /// The rule being renamed in place, and the name typed so far.
    renaming: Option<(RuleId, String)>,
    /// The rename field takes the keyboard on its next frame.
    focus_rename: bool,
    /// A link being dragged out of a block's out-dot.
    link_drag: Option<(RuleId, BlockId)>,
    /// A rule being dragged by its header to another place in the list.
    rule_drag: Option<RuleId>,
    /// The pass the view was last drawn on: a drag left over from before it was hidden is dropped.
    drawn_on: u64,
    clipboard_rule: Option<CopiedRule>,
    clipboard_block: Option<CopiedBlock>,
    /// The line a right-click landed near: (rule, from, to).
    link_menu: Option<(RuleId, BlockId, BlockId)>,
    /// When each block fired, from the playback's log.
    pub fired: Fired,
}

/// What the Rules view did this frame, for the studio.
#[derive(Default)]
pub struct Outcome {
    /// The edit is a step of its own in the undo history (adding, removing, linking…).
    pub separate: bool,
    /// Something to say in the status line.
    pub message: Option<String>,
}

/// What the running rules say about each rule.
struct RunNotes {
    problems: BTreeMap<RuleId, String>,
    busy: BTreeMap<RuleId, f64>,
}

/// A rule's problems in words: the running rules' first, then the scene's own checks.
fn problem_words(scene: &Scene, reg: &Registry, rule: RuleId, runtime: Option<&String>) -> Vec<String> {
    let mut out: Vec<String> = runtime.into_iter().cloned().collect();
    for p in scene.rule_problems(reg, rule) {
        let w = p.words();
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// "Rule N", one past the rules there are, not taken yet.
fn new_rule_name(scene: &Scene) -> String {
    let mut n = scene.rules.list.len() + 1;
    loop {
        let name = format!("Rule {n}");
        if !scene.rules.list.iter().any(|r| r.name == name) {
            return name;
        }
        n += 1;
    }
}

/// A block's name as its controls say it: "When Hit", "Do Change motion".
pub fn block_name(reg: &Registry, kind: &str) -> String {
    match reg.block(kind) {
        Some(k) if k.name() == k.section().name() => k.name().to_owned(),
        Some(k) => format!("{} {}", k.section().name(), k.name()),
        None => format!("Unknown block {kind}"),
    }
}

fn link_error_words(e: &EditError) -> String {
    match e {
        EditError::WrongType => "A When block can't have anything leading into it.".into(),
        EditError::RuleLoop => "That link would go round in a circle.".into(),
        other => format!("Couldn't link those: {other}"),
    }
}

/// Where a block's lines start: the middle of its right edge.
fn out_point(rect: Rect) -> Pos2 {
    Pos2::new(rect.right(), rect.center().y)
}

/// Where its `k`th of `n` inputs comes in, spread over its left edge.
fn in_point(rect: Rect, k: usize, n: usize) -> Pos2 {
    Pos2::new(rect.left(), rect.top() + rect.height() * (k + 1) as f32 / (n + 1) as f32)
}

/// The curve of a line from `a` to `b`, as points.
fn curve(a: Pos2, b: Pos2) -> [Pos2; 4] {
    let dx = ((b.x - a.x).abs() / 2.0).max(20.0);
    [a, a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0), b]
}

/// How far `p` is from the curve `c`.
fn distance_to_curve(c: [Pos2; 4], p: Pos2) -> f32 {
    let at = |t: f32| {
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        Pos2::new(c.iter().zip(w).map(|(q, w)| q.x * w).sum(), c.iter().zip(w).map(|(q, w)| q.y * w).sum())
    };
    let mut best = f32::INFINITY;
    let mut last = at(0.0);
    for i in 1..=24 {
        let next = at(i as f32 / 24.0);
        let seg = next - last;
        let t = if seg.length_sq() > 0.0 { ((p - last).dot(seg) / seg.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min((last + seg * t).distance(p));
        last = next;
    }
    best
}

fn paint_curve(painter: &egui::Painter, c: [Pos2; 4], ink: Color32) {
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        c,
        false,
        Color32::TRANSPARENT,
        Stroke::new(1.2, ink),
    ));
}

/// The rect of the block at (`column`, `row`) of a lane whose blocks start at `origin`.
fn block_rect(origin: Pos2, column: usize, row: usize) -> Rect {
    Rect::from_min_size(
        origin + Vec2::new(column as f32 * (BLOCK_W + COLUMN_GAP), row as f32 * (BLOCK_H + ROW_GAP)),
        Vec2::new(BLOCK_W, BLOCK_H),
    )
}

/// A lane's height for `rows` rows of blocks.
fn lane_height(rows: usize) -> f32 {
    PAD * 2.0 + rows as f32 * (BLOCK_H + ROW_GAP) - ROW_GAP + ADD_ROW
}

/// Draws the Rules view and applies what is done in it to `studio.scene`.
pub fn show(ui: &mut Ui, studio: &mut Studio) -> Outcome {
    let mut out = Outcome::default();
    let mut panel = std::mem::take(&mut studio.panels.rules);
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let rate = studio.playback.world().steps_per_second;
    panel.fired.update(studio.revision, studio.playback.log().events(), rate);
    let pass = ui.ctx().cumulative_pass_nr();
    if pass > panel.drawn_on + 1 {
        panel.link_drag = None;
        panel.rule_drag = None;
    }
    panel.drawn_on = pass;
    let notes = {
        let state = studio.playback.world().rules();
        RunNotes {
            problems: studio
                .scene
                .rules
                .list
                .iter()
                .filter_map(|r| Some((r.id, state?.problem(r.id)?.to_owned())))
                .collect(),
            busy: studio
                .scene
                .rules
                .list
                .iter()
                .filter_map(|r| Some((r.id, state?.busy_at(r.id)? as f64 / rate.max(1) as f64)))
                .collect(),
        }
    };
    let time = studio.sim_time();
    // A block that is gone isn't picked.
    if let Some((r, b)) = panel.picked
        && studio.scene.rules.rule(r).and_then(|r| r.block(b)).is_none()
    {
        panel.picked = None;
    }

    // ---------- the top row ----------
    ui.horizontal(|ui| {
        if ui.button("+ Add rule").on_hover_text("A new rule at the end of the list").clicked() {
            let name = new_rule_name(&studio.scene);
            studio.scene.add_rule(&name);
            out.separate = true;
        }
        if let Some(copied) = &panel.clipboard_rule
            && ui.button("Paste rule").on_hover_text("The copied rule, at the end of the list").clicked()
        {
            studio.scene.paste_rule(copied);
            out.separate = true;
        }
        let hint =
            "Rules run top first. Click a block to see its settings; drag a block's dot onto another to link them.";
        ui.label(RichText::new(hint).small().color(style::MUTED));
    });
    ui.add_space(4.0);
    if studio.scene.rules.is_empty() {
        let words = "No rules yet. A rule listens for something (a hit, a time, a beat), checks what it needs to, \
                     then changes the simulation: breaks a shape, speeds a ball up, changes any setting.";
        ui.add(egui::Label::new(RichText::new(words).color(style::MUTED)).wrap());
        studio.panels.rules = panel;
        return out;
    }

    let choices = TargetChoices::from_scene(&studio.scene, reg);
    let mut lane_rects: Vec<(RuleId, Rect)> = Vec::new();
    let mut block_rects: Vec<(RuleId, BlockId, Rect)> = Vec::new();
    let rules: Vec<Rule> = studio.scene.rules.list.clone();
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        for rule in &rules {
            lane(ui, studio, &mut panel, &mut out, rule, &choices, &notes, time, &mut lane_rects, &mut block_rects);
            ui.add_space(4.0);
        }
        drops(ui, studio, &mut panel, &mut out, &lane_rects, &block_rects);
    });
    studio.panels.rules = panel;
    out
}

/// Ends a link or rule drag when the mouse lets go: a link onto the block under the pointer, a rule
/// to the place between lanes under it.
fn drops(
    ui: &mut Ui,
    studio: &mut Studio,
    panel: &mut RulesPanel,
    out: &mut Outcome,
    lanes: &[(RuleId, Rect)],
    blocks: &[(RuleId, BlockId, Rect)],
) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let down = ui.input(|i| i.pointer.any_down());
    if let Some((rule, from)) = panel.link_drag {
        if let Some(p) = pointer {
            let start = blocks.iter().find(|(r, b, _)| *r == rule && *b == from).map(|(_, _, rect)| out_point(*rect));
            if let Some(a) = start {
                paint_curve(ui.painter(), curve(a, p), style::TEXT);
            }
        }
        if !down {
            panel.link_drag = None;
            let target =
                pointer.and_then(|p| blocks.iter().find(|(r, b, rect)| *r == rule && *b != from && rect.contains(p)));
            if let Some(&(_, to, _)) = target {
                match studio.scene.link(reg, rule, from, to) {
                    Ok(()) => out.separate = true,
                    Err(e) => out.message = Some(link_error_words(&e)),
                }
            }
        }
    }
    if let Some(id) = panel.rule_drag {
        let others: Vec<&(RuleId, Rect)> = lanes.iter().filter(|(r, _)| *r != id).collect();
        let to = pointer.map(|p| others.iter().filter(|(_, rect)| rect.center().y < p.y).count());
        if let Some(to) = to {
            // A line where it would land.
            let y = match others.get(to) {
                Some((_, rect)) => rect.top() - 2.0,
                None => others.last().map_or(0.0, |(_, rect)| rect.bottom() + 2.0),
            };
            if let Some((_, any)) = lanes.first() {
                ui.painter().hline(any.x_range(), y, Stroke::new(2.0, style::TEXT));
            }
        }
        if !down {
            panel.rule_drag = None;
            let from = studio.scene.rules.list.iter().position(|r| r.id == id);
            if let (Some(to), Some(from)) = (to, from)
                && to != from
                && studio.scene.move_rule(id, to).is_ok()
            {
                out.separate = true;
            }
        }
    }
}

/// One rule's lane: its header and its blocks.
#[allow(clippy::too_many_arguments)]
fn lane(
    ui: &mut Ui,
    studio: &mut Studio,
    panel: &mut RulesPanel,
    out: &mut Outcome,
    rule: &Rule,
    choices: &TargetChoices,
    notes: &RunNotes,
    time: f64,
    lane_rects: &mut Vec<(RuleId, Rect)>,
    block_rects: &mut Vec<(RuleId, BlockId, Rect)>,
) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let lay = layout::lay_out(rule);
    let width = HEADER_W + BLOCKS_INSET + lay.columns.max(1) as f32 * (BLOCK_W + COLUMN_GAP) + 20.0;
    let height = lane_height(lay.rows);
    let (rect, background) = ui.allocate_exact_size(Vec2::new(width.max(ui.available_width()), height), Sense::click());
    lane_rects.push((rule.id, rect));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let origin = Pos2::new(rect.left() + HEADER_W + BLOCKS_INSET, rect.top() + PAD);

    // ---------- the lines, and the menu of the one right-clicked ----------
    let rect_of = |b: BlockId| lay.place(b).map(|p| block_rect(origin, p.column, p.row));
    let inputs_of = |b: BlockId| rule.block(b).map_or(0, |b| b.inputs.len());
    let lines: Vec<(BlockId, BlockId, [Pos2; 4])> = lay
        .links
        .iter()
        .filter_map(|l| {
            let (a, b) = (rect_of(l.from)?, rect_of(l.to)?);
            Some((l.from, l.to, curve(out_point(a), in_point(b, l.input, inputs_of(l.to)))))
        })
        .collect();
    for (_, _, c) in &lines {
        paint_curve(&painter, *c, style::MUTED);
    }
    if background.secondary_clicked() {
        panel.link_menu = background.interact_pointer_pos().and_then(|p| {
            lines
                .iter()
                .map(|(a, b, c)| (distance_to_curve(*c, p), *a, *b))
                .filter(|(d, _, _)| *d <= LINE_REACH)
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, a, b)| (rule.id, a, b))
        });
    }
    background.context_menu(|ui| match panel.link_menu.filter(|(r, _, _)| *r == rule.id) {
        Some((_, from, to)) => {
            if ui.button("Remove link").clicked() {
                if studio.scene.unlink(rule.id, from, to).is_ok() {
                    out.separate = true;
                }
                panel.link_menu = None;
                ui.close();
            }
        }
        None => {
            ui.menu_button("Add When", |ui| {
                if let Some(kind) = menu::block_menu(ui, reg, true)
                    && let Ok(new) = studio.scene.add_when(reg, rule.id, kind)
                {
                    panel.picked = Some((rule.id, new));
                    studio.selection.clear();
                    out.separate = true;
                }
            });
            if let Some(copied) = panel.clipboard_block.clone()
                && ui.button("Paste block").clicked()
            {
                if studio.scene.paste_block(reg, rule.id, &copied, None).is_ok() {
                    out.separate = true;
                }
                ui.close();
            }
        }
    });

    header(ui, studio, panel, out, rule, Rect::from_min_size(rect.min, Vec2::new(HEADER_W, height)), notes, time);

    // ---------- the blocks ----------
    for place in &lay.places {
        let Some(block) = rule.block(place.block) else { continue };
        let r = block_rect(origin, place.column, place.row);
        block_rects.push((rule.id, block.id, r));
        let name = block_name(reg, &block.kind);
        let id = egui::Id::unique(("rule block", rule.id, block.id));
        let response = ui.interact(r, id, Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
        let kind = reg.block(&block.kind);
        let section = kind.map_or(BlockSection::Do, |k| k.section());
        let fields = kind.map(|k| face::fields(&studio.scene, &kit, choices, block, k)).unwrap_or_default();
        let looks = Looks {
            picked: panel.picked == Some((rule.id, block.id)),
            fired: panel.fired.recent(rule.id, block.id, time),
            hovered: response.hovered(),
        };
        let title = kind.map_or(block.kind.as_str(), |k| k.name());
        face::paint(&painter, r, section, title, &fields, looks);
        let response = match kind {
            Some(k) => response.on_hover_text(k.help()),
            None => response.on_hover_text("This version of Ball Studio doesn't know this block"),
        };
        if response.clicked() {
            panel.picked = Some((rule.id, block.id));
            studio.selection.clear();
        }
        response.context_menu(|ui| block_menu_items(ui, studio, panel, out, rule, block.id));

        // The out-dot: drag it onto another block to link them.
        let dot = Rect::from_center_size(out_point(r), Vec2::splat(DOT_R * 3.0));
        let dot_response = ui.interact(dot, id.with("out"), Sense::drag());
        let words = format!("Link from {name}");
        dot_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, words.clone()));
        let dot_response = dot_response.on_hover_text("Drag onto another block to link them");
        if dot_response.drag_started() {
            panel.link_drag = Some((rule.id, block.id));
        }
        let ink = if dot_response.hovered() || panel.link_drag == Some((rule.id, block.id)) {
            style::TEXT_STRONG
        } else {
            style::MUTED
        };
        painter.circle_filled(out_point(r), DOT_R, ink);
        painter.circle_stroke(out_point(r), DOT_R, Stroke::new(1.0, style::GROUND));

        // "+": the block menu, for a block after this one.
        let plus = Rect::from_min_size(Pos2::new(r.right() + 8.0, r.bottom() - 18.0), Vec2::splat(18.0));
        let add = ui.place(plus, egui::Button::new("+").small());
        let words = format!("Add after {name}");
        add.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, words.clone()));
        let add = add.on_hover_text("Add a block after this one");
        egui::Popup::menu(&add).show(|ui| {
            if let Some(kind) = menu::block_menu(ui, reg, false) {
                let after = Some(block.id);
                let when = reg.block(kind).is_some_and(|k| k.section() == BlockSection::When);
                let added = if when {
                    studio.scene.add_when(reg, rule.id, kind)
                } else {
                    studio.scene.add_block(reg, rule.id, kind, after)
                };
                if let Ok(new) = added {
                    panel.picked = Some((rule.id, new));
                    studio.selection.clear();
                    out.separate = true;
                }
            }
        });
    }

    // "+ When": under the first column.
    let first_rows = lay.places.iter().filter(|p| p.column == 0).count();
    let y = if first_rows == 0 { origin.y } else { origin.y + first_rows as f32 * (BLOCK_H + ROW_GAP) - ROW_GAP + 4.0 };
    let at = Rect::from_min_size(Pos2::new(origin.x, y), Vec2::new(72.0, 18.0));
    let add_when = ui.place(at, egui::Button::new("+ When").small());
    let words = format!("Add When to {}", rule.name);
    add_when.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, words.clone()));
    let add_when = add_when.on_hover_text("Something more this rule listens for, stacked under the others");
    egui::Popup::menu(&add_when).show(|ui| {
        if let Some(kind) = menu::block_menu(ui, reg, true)
            && let Ok(new) = studio.scene.add_when(reg, rule.id, kind)
        {
            panel.picked = Some((rule.id, new));
            studio.selection.clear();
            out.separate = true;
        }
    });
}

/// A block's right-click menu: Duplicate, Copy block, Paste block after, Delete.
fn block_menu_items(
    ui: &mut Ui,
    studio: &mut Studio,
    panel: &mut RulesPanel,
    out: &mut Outcome,
    rule: &Rule,
    block: BlockId,
) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let inputs = rule.block(block).map(|b| b.inputs.clone()).unwrap_or_default();
    if ui.button("Duplicate").on_hover_text("A copy fed by the same blocks, under this one").clicked() {
        if let Some(copied) = studio.scene.copy_block(rule.id, block)
            && let Ok(new) = studio.scene.paste_block(reg, rule.id, &copied, inputs.first().copied())
        {
            // A Logic block's copy takes every input.
            for &i in inputs.iter().skip(1) {
                let _ = studio.scene.link(reg, rule.id, i, new);
            }
            panel.picked = Some((rule.id, new));
            studio.selection.clear();
            out.separate = true;
        }
        ui.close();
    }
    if ui.button("Copy block").clicked() {
        panel.clipboard_block = studio.scene.copy_block(rule.id, block);
        ui.close();
    }
    let paste = ui.add_enabled(panel.clipboard_block.is_some(), egui::Button::new("Paste block after"));
    if paste.on_disabled_hover_text("Copy a block first").clicked() {
        if let Some(copied) = panel.clipboard_block.clone()
            && let Ok(new) = studio.scene.paste_block(reg, rule.id, &copied, Some(block))
        {
            panel.picked = Some((rule.id, new));
            studio.selection.clear();
            out.separate = true;
        }
        ui.close();
    }
    if ui.button("Delete").on_hover_text("Its links close up where they can").clicked() {
        if studio.scene.remove_block(reg, rule.id, block).is_ok() {
            out.separate = true;
        }
        if panel.picked == Some((rule.id, block)) {
            panel.picked = None;
        }
        ui.close();
    }
}

/// Starts renaming `rule`, keeping a name being typed for another rule first.
fn start_renaming(studio: &mut Studio, panel: &mut RulesPanel, rule: &Rule) {
    if let Some((other, text)) = panel.renaming.take()
        && other != rule.id
        && !text.trim().is_empty()
    {
        let _ = studio.scene.rename_rule(other, text.trim());
    }
    panel.renaming = Some((rule.id, rule.name.clone()));
}

/// A lane's header: the on switch, the name (renamed in place), how often it fired, its problem
/// mark and a busy note. Right-click for its menu; drag it to move the rule.
#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut Ui,
    studio: &mut Studio,
    panel: &mut RulesPanel,
    out: &mut Outcome,
    rule: &Rule,
    rect: Rect,
    notes: &RunNotes,
    time: f64,
) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let id = egui::Id::unique(("rule header", rule.id));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let words = format!("Rule {}", rule.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, words.clone()));
    let painter = ui.painter_at(rect);
    let fill = if response.hovered() || panel.rule_drag == Some(rule.id) { style::HOVER } else { style::RAISED };
    painter.rect_filled(rect, style::RADIUS, fill);
    if response.drag_started() {
        panel.rule_drag = Some(rule.id);
    }

    // The on switch.
    let switch_at = Rect::from_min_size(rect.min + Vec2::new(8.0, 8.0), Vec2::new(30.0, 18.0));
    let mut on = rule.enabled;
    let switch = ui.place(switch_at, |ui: &mut Ui| style::switch(ui, &mut on));
    let on_words = format!("{} on", rule.name);
    switch.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, on_words.clone()));
    if switch.on_hover_text("Off, the rule does nothing").changed()
        && studio.scene.set_rule_enabled(rule.id, on).is_ok()
    {
        out.separate = true;
    }

    // The name, or the field renaming it.
    let name_at = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top() + 30.0),
        Pos2::new(rect.right() - 22.0, rect.top() + 48.0),
    );
    match panel.renaming.as_mut().filter(|(r, _)| *r == rule.id) {
        Some((_, text)) => {
            let field = ui.place(name_at, egui::TextEdit::singleline(text).desired_width(name_at.width()));
            let label = format!("Name of {}", rule.name);
            field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label.clone()));
            if std::mem::take(&mut panel.focus_rename) {
                field.request_focus();
            }
            if field.lost_focus() {
                // Escape leaves the name as it was.
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    let name = text.trim().to_owned();
                    if !name.is_empty() && name != rule.name {
                        let _ = studio.scene.rename_rule(rule.id, &name);
                    }
                }
                panel.renaming = None;
            } else if !field.has_focus() {
                // Back after the view was switched away: carry on typing.
                field.request_focus();
            }
        }
        None => {
            let mut job =
                egui::text::LayoutJob::simple_singleline(rule.name.clone(), style::semibold(12.5), style::TEXT);
            job.wrap = egui::text::TextWrapping::truncate_at_width(name_at.width());
            let galley = painter.layout_job(job);
            painter.galley(name_at.left_top(), galley, style::TEXT);
        }
    }
    if response.double_clicked() {
        start_renaming(studio, panel, rule);
        panel.focus_rename = true;
    }

    // Fired, busy and the problem mark.
    let count = panel.fired.count(rule, time);
    let fired = match count {
        0 => "never fired".to_owned(),
        1 => "fired once".to_owned(),
        n => format!("fired {n} times"),
    };
    painter.text(rect.left_top() + Vec2::new(8.0, 52.0), egui::Align2::LEFT_TOP, fired, style::mono(9.5), style::MUTED);
    if let Some(at) = notes.busy.get(&rule.id) {
        let words = format!("too busy at {}", clock(*at));
        painter.text(
            rect.left_top() + Vec2::new(8.0, 66.0),
            egui::Align2::LEFT_TOP,
            words,
            style::mono(9.5),
            style::MUTED,
        );
    }
    let problems = problem_words(&studio.scene, reg, rule.id, notes.problems.get(&rule.id));
    if !problems.is_empty() {
        let mark = Rect::from_center_size(Pos2::new(rect.right() - 12.0, rect.top() + 17.0), Vec2::splat(12.0));
        painter.circle_filled(mark.center(), 5.0, style::PROBLEM);
        let r = ui.interact(mark, id.with("problem"), Sense::hover());
        let words = format!("Problem in {}", rule.name);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, words.clone()));
        let text = format!("This rule is skipped:\n{}", problems.join("\n"));
        r.on_hover_text(RichText::new(text).color(style::PROBLEM));
    }

    let response = response.on_hover_text("Double-click to rename; drag to move the rule; right-click for more");
    response.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            start_renaming(studio, panel, rule);
            panel.focus_rename = true;
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            if studio.scene.duplicate_rule(rule.id).is_ok() {
                out.separate = true;
            }
            ui.close();
        }
        if ui.button("Copy rule").clicked() {
            panel.clipboard_rule = studio.scene.copy_rule(rule.id);
            ui.close();
        }
        let paste = ui.add_enabled(panel.clipboard_rule.is_some(), egui::Button::new("Paste rule"));
        if paste.on_disabled_hover_text("Copy a rule first").clicked() {
            if let Some(copied) = panel.clipboard_rule.clone() {
                studio.scene.paste_rule(&copied);
                out.separate = true;
            }
            ui.close();
        }
        if ui.button("Delete").clicked() {
            if studio.scene.remove_rule(rule.id).is_ok() {
                out.separate = true;
            }
            if panel.picked.is_some_and(|(r, _)| r == rule.id) {
                panel.picked = None;
            }
            ui.close();
        }
    });
}
