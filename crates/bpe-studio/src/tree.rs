//! The scene tree: every thing in order, with selection, rename, duplicate, delete, a filter and
//! a menu for adding things.

use crate::theme;
use bpe_core::{Problem, ProblemKind, Registry, Resolved, Scene, Thing, ThingId};
use bpe_kit::Kit;
use eframe::egui::{self, Color32, Key, Rect, RichText, Sense, Stroke, Ui, Vec2};

#[derive(Default)]
pub struct Tree {
    pub filter: String,
    /// The thing whose name is being edited in place, and the text so far.
    pub renaming: Option<ThingId>,
    pub rename_text: String,
    /// The rename field takes the keyboard on its first frame.
    focus_rename: bool,
}

#[derive(Default)]
pub struct TreeOutcome {
    pub edited: bool,
    pub removed: Option<ThingId>,
}

enum Action {
    Select(ThingId),
    StartRename(ThingId),
    Rename(ThingId, String),
    Toggle(ThingId),
    Duplicate(ThingId),
    Delete(ThingId),
    Add(&'static str),
}

/// Draws the tree and applies what was clicked. `selected` follows the clicks.
pub fn show(
    ui: &mut Ui,
    tree: &mut Tree,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    selected: &mut Option<ThingId>,
) -> TreeOutcome {
    let reg = &kit.registry;
    let mut action = None;
    // The add button keeps its place at the bottom; the rows scroll in what is left.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.menu_button("+ Add thing", |ui| {
            for kind in reg.things().filter(|k| k.id() != "scene") {
                if ui.button(kind.name()).on_hover_text(kind.help()).clicked() {
                    action = Some(Action::Add(kind.id()));
                    ui.close();
                }
            }
        });
        ui.add_space(4.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                let label = ui.label(RichText::new("Filter").small().color(theme::MUTED));
                let edit = egui::TextEdit::singleline(&mut tree.filter)
                    .desired_width(f32::INFINITY)
                    .hint_text("name, kind or tag");
                ui.add(edit).labelled_by(label.id);
            });
            let filter = tree.filter.trim().to_lowercase();
            ui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for &id in &scene.order {
                    let Some(thing) = scene.things.get(&id) else { continue };
                    if !filter.is_empty() && !matches(thing, &filter) {
                        continue;
                    }
                    let depth = scene.ancestors(id).len();
                    let problems: Vec<String> =
                        resolved.problems.iter().filter(|p| p.thing == id).map(problem_words).collect();
                    row(ui, tree, thing, depth, &problems, *selected == Some(id), &mut action);
                }
            });
        });
    });
    // Delete removes the selected thing, unless the keyboard is in a text field.
    if action.is_none()
        && !ui.ctx().text_edit_focused()
        && ui.input(|i| i.key_pressed(Key::Delete))
        && let Some(id) = *selected
    {
        action = Some(Action::Delete(id));
    }

    let mut out = TreeOutcome::default();
    match action {
        None => {}
        Some(Action::Select(id)) => *selected = Some(id),
        Some(Action::StartRename(id)) => {
            tree.renaming = Some(id);
            tree.rename_text = scene.things.get(&id).map(|t| t.name.clone()).unwrap_or_default();
            tree.focus_rename = true;
            *selected = Some(id);
        }
        Some(Action::Rename(id, name)) => {
            tree.renaming = None;
            let name = name.trim();
            if let Some(thing) = scene.things.get_mut(&id)
                && !name.is_empty()
                && thing.name != name
            {
                thing.name = name.to_owned();
                out.edited = true;
            }
        }
        Some(Action::Toggle(id)) => {
            if let Some(thing) = scene.things.get_mut(&id) {
                thing.enabled = !thing.enabled;
                out.edited = true;
            }
        }
        Some(Action::Duplicate(id)) => {
            if let Some(copy) = duplicate(scene, reg, id) {
                *selected = Some(copy);
                out.edited = true;
            }
        }
        Some(Action::Delete(id)) => {
            if let Ok(removed) = scene.remove_thing(id) {
                if selected.is_some_and(|s| removed.contains(&s)) {
                    *selected = None;
                }
                if tree.renaming.is_some_and(|r| removed.contains(&r)) {
                    tree.renaming = None;
                }
                out.removed = Some(id);
                out.edited = true;
            }
        }
        Some(Action::Add(kind)) => {
            // Beside the selected thing, or under Scene defaults.
            let parent = selected.and_then(|s| scene.things.get(&s)).and_then(|t| t.parent).unwrap_or(ThingId::ROOT);
            if let Ok(id) = scene.add_thing(reg, kind, parent) {
                *selected = Some(id);
                out.edited = true;
            }
        }
    }
    out
}

/// Whether the filter text appears in the thing's name, kind or tags.
fn matches(thing: &Thing, filter: &str) -> bool {
    thing.name.to_lowercase().contains(filter)
        || thing.kind.to_lowercase().contains(filter)
        || thing.tags.iter().any(|t| t.to_lowercase().contains(filter))
}

fn row(
    ui: &mut Ui,
    tree: &mut Tree,
    thing: &Thing,
    depth: usize,
    problems: &[String],
    selected: bool,
    action: &mut Option<Action>,
) {
    let id = thing.id;
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 16.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        let ink = if thing.enabled { theme::ACCENT_TEXT } else { theme::MUTED };
        paint_icon(ui.painter(), rect, &thing.kind, ink);

        if tree.renaming == Some(id) {
            let edit_id = ui.make_persistent_id(("rename", id));
            if std::mem::take(&mut tree.focus_rename) {
                // The whole name is selected, so typing replaces it.
                let mut state = egui::text_edit::TextEditState::default();
                let end = egui::text::CCursor::new(tree.rename_text.chars().count());
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
                state.store(ui.ctx(), edit_id);
                ui.memory_mut(|m| m.request_focus(edit_id));
            }
            let label = ui.label(RichText::new("Name").small().color(theme::MUTED));
            let edit = egui::TextEdit::singleline(&mut tree.rename_text).id(edit_id).desired_width(f32::INFINITY);
            let response = ui.add(edit).labelled_by(label.id);
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                tree.renaming = None;
            } else if response.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
                *action = Some(Action::Rename(id, tree.rename_text.clone()));
            }
        } else {
            let color = if thing.enabled { theme::TEXT } else { theme::MUTED };
            let response = ui.add(egui::Button::selectable(selected, RichText::new(&thing.name).color(color)));
            if response.double_clicked() {
                *action = Some(Action::StartRename(id));
            } else if response.clicked() {
                *action = Some(Action::Select(id));
            }
            response.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    *action = Some(Action::StartRename(id));
                    ui.close();
                }
                if id != ThingId::ROOT {
                    if ui.button("Duplicate").clicked() {
                        *action = Some(Action::Duplicate(id));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        *action = Some(Action::Delete(id));
                        ui.close();
                    }
                }
            });
        }
        if !thing.tags.is_empty() {
            let tags: Vec<&str> = thing.tags.iter().map(String::as_str).collect();
            ui.label(RichText::new(tags.join(" ")).small().color(theme::MUTED));
        }
        if !problems.is_empty() {
            let (dot, response) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.5, theme::ERROR);
            response.on_hover_text(problems.join("\n"));
        }
        // Hiding the root would hide the whole scene, so it has no eye.
        if id != ThingId::ROOT {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let help = if thing.enabled { "Shown. Click to hide it." } else { "Hidden. Click to show it." };
                let (rect, response) = ui.allocate_exact_size(Vec2::new(18.0, 16.0), Sense::click());
                let name = format!("{} {}", if thing.enabled { "Hide" } else { "Show" }, thing.name);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
                paint_eye(ui.painter(), rect, thing.enabled, ink);
                if response.on_hover_text(help).clicked() {
                    *action = Some(Action::Toggle(id));
                }
            });
        }
    });
}

/// An open eye for a shown thing, a struck-through one for a hidden thing.
fn paint_eye(painter: &egui::Painter, rect: Rect, open: bool, ink: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.3, ink);
    let (w, h) = (rect.width() * 0.42, rect.height() * 0.28);
    // The lids: two arcs meeting at the corners of the eye.
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    for i in 0..=12 {
        let t = i as f32 / 12.0;
        let x = c.x - w + 2.0 * w * t;
        let bulge = (std::f32::consts::PI * t).sin() * h;
        top.push(egui::pos2(x, c.y - bulge));
        bottom.push(egui::pos2(x, c.y + bulge));
    }
    painter.add(egui::Shape::line(top, stroke));
    painter.add(egui::Shape::line(bottom, stroke));
    if open {
        painter.circle_filled(c, h * 0.55, ink);
    } else {
        painter.line_segment([egui::pos2(c.x - w, c.y + h + 2.0), egui::pos2(c.x + w, c.y - h - 2.0)], stroke);
    }
}

/// A ring for an arena, a disc for balls, a square for the scene, a diamond for anything else.
fn paint_icon(painter: &egui::Painter, rect: Rect, kind: &str, ink: Color32) {
    let c = rect.center();
    let r = rect.width() * 0.32;
    match kind {
        "arena" => {
            painter.circle_stroke(c, r, Stroke::new(1.5, ink));
        }
        "balls" => {
            painter.circle_filled(c, r * 0.8, ink);
        }
        "scene" => {
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(r * 1.6)), 2.0, ink);
        }
        _ => {
            let points =
                vec![c + Vec2::new(0.0, -r), c + Vec2::new(r, 0.0), c + Vec2::new(0.0, r), c + Vec2::new(-r, 0.0)];
            painter.add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
        }
    }
}

/// A copy of `id` beside it, with the same settings, slots and transform. The scene's defaults
/// can't be copied.
pub fn duplicate(scene: &mut Scene, reg: &Registry, id: ThingId) -> Option<ThingId> {
    let source = scene.things.get(&id)?.clone();
    let parent = source.parent?;
    let copy = scene.add_thing(reg, &source.kind, parent).ok()?;
    let thing = scene.things.get_mut(&copy)?;
    thing.name = format!("{} copy", source.name);
    thing.tags = source.tags;
    thing.attach = source.attach;
    thing.transform = source.transform;
    thing.enabled = source.enabled;
    thing.params = source.params;
    thing.slots = source.slots;
    // Right after the original, not at the end of the parent's children.
    let _ = scene.move_before(copy, next_sibling(scene, id).unwrap_or(copy));
    Some(copy)
}

/// The sibling after `id` under the same parent, if any.
fn next_sibling(scene: &Scene, id: ThingId) -> Option<ThingId> {
    let parent = scene.things.get(&id)?.parent?;
    let siblings = scene.children(parent);
    let at = siblings.iter().position(|&s| s == id)?;
    siblings.get(at + 1).copied()
}

/// What a resolve problem means, in plain words.
pub fn problem_words(p: &Problem) -> String {
    let slot = p.slot.map(|s| s.name()).unwrap_or("A slot");
    match &p.kind {
        ProblemKind::UnknownKind(k) => format!("Its kind \"{k}\" isn't installed"),
        ProblemKind::MissingModule(m) => format!("{slot} links a module that is missing (id {})", m.0),
        ProblemKind::WrongSlot(m) => format!("{slot} links a module made for another slot (id {})", m.0),
        ProblemKind::UnknownModuleKind(k) => format!("{slot} links a module whose kind \"{k}\" isn't installed"),
        ProblemKind::MissingTarget(t) => format!("A setting points at a thing that isn't in the scene (id {})", t.0),
        ProblemKind::BrokenGraph(msg) => {
            format!("The {slot} graph doesn't compile, so the parent's module is used: {msg}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{ModuleId, Problem, ProblemKind, Resolved, SlotId, Value, resolve};
    use eframe::egui::{self, Key};
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        tree: Tree,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selected: Option<ThingId>,
    }

    fn harness() -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let state = State { tree: Tree::default(), scene, kit, resolved, selected: None };
        let mut h = Harness::builder()
            .with_size(egui::vec2(260.0, 600.0))
            // Real frame times, so two quick clicks count as a double-click.
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let out = show(ui, &mut s.tree, &mut s.scene, &s.kit, &s.resolved, &mut s.selected);
                    if out.edited {
                        s.resolved = resolve(&s.scene, &s.kit.registry);
                    }
                },
                state,
            );
        h.run();
        h
    }

    #[test]
    fn adding_a_thing_lists_and_selects_it() {
        let mut h = harness();
        h.get_by_label("+ Add thing").click();
        h.run();
        h.get_by_label("Balls").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.things.len(), 4);
        assert_eq!(s.selected, Some(ThingId(3)));
        // The new scene's ball is called "Ball", so the first added Balls keeps the kind's name.
        assert_eq!(s.scene.things[&ThingId(3)].name, "Balls");
        assert_eq!(s.scene.things[&ThingId(3)].parent, Some(ThingId::ROOT));
        assert!(h.query_by_label("Balls").is_some());
    }

    #[test]
    fn delete_removes_the_selected_thing_but_never_the_root() {
        let mut h = harness();
        h.state_mut().selected = Some(ThingId(2));
        h.get_by_label("Ball").click();
        h.run();
        h.key_press(Key::Delete);
        h.run();
        assert!(!h.state().scene.things.contains_key(&ThingId(2)));
        assert_eq!(h.state().selected, None);
        h.state_mut().selected = Some(ThingId::ROOT);
        h.key_press(Key::Delete);
        h.run();
        assert!(h.state().scene.things.contains_key(&ThingId::ROOT));
    }

    #[test]
    fn the_filter_hides_other_rows() {
        let mut h = harness();
        // Typing goes to the focused field, and a click is what focuses it.
        h.get_by_label("Filter").click();
        h.run();
        h.get_by_label("Filter").type_text("are");
        h.run();
        assert!(h.query_by_label("Arena").is_some());
        assert!(h.query_by_label("Ball").is_none());
    }

    #[test]
    fn double_clicking_a_name_renames_it_in_place() {
        let mut h = harness();
        h.get_by_label("Ball").click();
        h.step();
        h.get_by_label("Ball").click();
        h.run();
        assert_eq!(h.state().tree.renaming, Some(ThingId(2)));
        h.event(egui::Event::Text("Hero".into()));
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().tree.renaming, None);
        assert_eq!(h.state().scene.things[&ThingId(2)].name, "Hero");
        assert!(h.query_by_label("Hero").is_some());
    }

    #[test]
    fn a_duplicate_sits_beside_the_original_with_its_settings() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        scene.things.get_mut(&ThingId(2)).unwrap().params.set("radius", Value::Number(40.0));
        let copy = duplicate(&mut scene, &kit.registry, ThingId(2)).unwrap();
        assert_eq!(copy, ThingId(3));
        let (original, copied) = (&scene.things[&ThingId(2)], &scene.things[&ThingId(3)]);
        assert_eq!(copied.name, "Ball copy");
        assert_eq!(copied.parent, original.parent);
        assert_eq!(copied.params, original.params);
        assert_eq!(copied.slots, original.slots);
        assert_eq!(copied.transform, original.transform);
        assert_eq!(scene.order, vec![ThingId::ROOT, ThingId(1), ThingId(2), ThingId(3)]);
        assert_eq!(duplicate(&mut scene, &kit.registry, ThingId::ROOT), None);
    }

    #[test]
    fn problems_are_said_in_plain_words() {
        let p = Problem { thing: ThingId(2), slot: Some(SlotId::Look), kind: ProblemKind::MissingModule(ModuleId(99)) };
        assert_eq!(problem_words(&p), "Look links a module that is missing (id 99)");
        let p = Problem { thing: ThingId(2), slot: None, kind: ProblemKind::UnknownKind("blob".into()) };
        assert_eq!(problem_words(&p), "Its kind \"blob\" isn't installed");
    }

    #[test]
    fn the_root_has_no_eye_and_other_things_can_be_hidden() {
        let mut h = harness();
        assert!(h.query_by_label("Hide Scene defaults").is_none());
        h.get_by_label("Hide Ball").click();
        h.run();
        assert!(!h.state().scene.things[&ThingId(2)].enabled);
        h.get_by_label("Show Ball").click();
        h.run();
        assert!(h.state().scene.things[&ThingId(2)].enabled);
    }
}
