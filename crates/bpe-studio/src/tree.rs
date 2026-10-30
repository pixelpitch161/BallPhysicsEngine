//! The scene tree: every thing in order with its folders and riders, selection of one or many,
//! rename, duplicate, delete, collapsing, drag and drop, a filter and a menu for adding things.

use std::collections::BTreeSet;

use crate::selection::Selection;
use crate::theme;
use bpe_core::{Attach, EditError, ModuleId, Problem, ProblemKind, Registry, Resolved, Scene, Thing, ThingId};
use bpe_kit::Kit;
use eframe::egui::{self, Color32, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

#[derive(Default)]
pub struct Tree {
    pub filter: String,
    /// The thing whose name is being edited in place, and the text so far.
    pub renaming: Option<ThingId>,
    pub rename_text: String,
    /// The rename field takes the keyboard on its first frame.
    focus_rename: bool,
    /// Things whose children are folded away.
    pub collapsed: BTreeSet<ThingId>,
}

#[derive(Default)]
pub struct TreeOutcome {
    pub edited: bool,
    /// Something was deleted: the first thing that went.
    pub removed: Option<ThingId>,
    /// A word for the status line about something that was refused.
    pub message: Option<String>,
}

/// Where a dragged row is let go over another row.
#[derive(Clone, Debug, PartialEq)]
pub enum Drop {
    /// Just above the row, as its sibling.
    Before,
    /// Onto the row: into it, grouped or riding.
    Into(Attach),
    /// Just below the row, as its sibling.
    After,
}

enum Action {
    Select(ThingId, Modifiers),
    StartRename(ThingId),
    Rename(ThingId, String),
    Toggle(ThingId),
    Collapse(ThingId, bool),
    Duplicate(ThingId),
    Delete(Vec<ThingId>),
    /// Delete this row: with everything else selected when the row is part of the selection.
    DeleteRow(ThingId),
    Add(&'static str),
    /// A folder around this row, or around the whole selection when the row is part of it.
    MakeFolder(ThingId),
    Folder(Vec<ThingId>),
    SelectUsers(ModuleId),
    /// How the thing hangs off the parent it already has.
    Attach(ThingId, Attach),
    /// Ride on another thing, keeping the place on screen.
    AttachTo(ThingId, ThingId),
    Drop(ThingId, ThingId, Drop),
    /// A module dragged from the Library lands on a thing.
    Link(ThingId, ModuleId),
}

/// Draws the tree and applies what was clicked. `selection` follows the clicks.
pub fn show(
    ui: &mut Ui,
    tree: &mut Tree,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    selection: &mut Selection,
) -> TreeOutcome {
    let reg = &kit.registry;
    let mut action = None;
    // The rows drawn, top to bottom, for Shift-clicks.
    let mut visible: Vec<ThingId> = Vec::new();
    // The add button keeps its place at the bottom; the rows scroll in what is left.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.label(
            RichText::new("Drag rows to reorder · drop on a row to group · hold Alt to ride")
                .small()
                .color(theme::MUTED),
        );
        ui.menu_button("+ Add thing", |ui| {
            let present = |k: &str| scene.things.values().any(|t| t.kind == k);
            for kind in reg.things().filter(|k| k.id() != "scene" && !(k.single() && present(k.id()))) {
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
                let mut rows: Vec<(ThingId, Rect)> = Vec::new();
                for &id in &scene.order {
                    let Some(thing) = scene.things.get(&id) else { continue };
                    if !filter.is_empty() && !matches(thing, &filter) {
                        continue;
                    }
                    let ancestors = scene.ancestors(id);
                    if filter.is_empty() && ancestors.iter().any(|a| tree.collapsed.contains(a)) {
                        continue;
                    }
                    let problems: Vec<String> =
                        resolved.problems.iter().filter(|p| p.thing == id).map(problem_words).collect();
                    let cx = RowCx {
                        depth: ancestors.len(),
                        has_children: !scene.children(id).is_empty(),
                        parent_name: thing.parent.and_then(|p| scene.things.get(&p)).map(|p| p.name.as_str()),
                        problems: &problems,
                        selected: selection.contains(id),
                        single: reg.thing(&thing.kind).is_some_and(|k| k.single()),
                    };
                    let rect = row(ui, tree, scene, thing, &cx, &mut action);
                    rows.push((id, rect));
                    visible.push(id);
                }
                drop_target(ui, scene, &rows, &mut action);
            });
        });
    });
    // Delete removes everything selected, unless the keyboard is in a text field.
    if action.is_none()
        && !ui.ctx().text_edit_focused()
        && ui.input(|i| i.key_pressed(Key::Delete))
        && selection.primary.is_some()
    {
        action = Some(Action::Delete(selection.all(scene)));
    }

    // A row's own menu acts on the whole selection when the row is part of it.
    let with_selection = |id: ThingId| if selection.contains(id) { selection.all(scene) } else { vec![id] };
    let action = match action {
        Some(Action::DeleteRow(id)) => Some(Action::Delete(with_selection(id))),
        Some(Action::MakeFolder(id)) => Some(Action::Folder(with_selection(id))),
        other => other,
    };

    let mut out = TreeOutcome::default();
    match action {
        None => {}
        Some(Action::Select(id, modifiers)) => {
            if modifiers.command {
                selection.toggle(id);
            } else if modifiers.shift {
                selection.range(id, &visible);
            } else {
                selection.set(id);
            }
        }
        Some(Action::StartRename(id)) => {
            tree.renaming = Some(id);
            tree.rename_text = scene.things.get(&id).map(|t| t.name.clone()).unwrap_or_default();
            tree.focus_rename = true;
            selection.set(id);
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
        Some(Action::Collapse(id, collapsed)) => {
            if collapsed {
                tree.collapsed.insert(id);
            } else {
                tree.collapsed.remove(&id);
            }
        }
        Some(Action::Duplicate(id)) => {
            if let Some(copy) = duplicate(scene, reg, id) {
                selection.set(copy);
                out.edited = true;
            }
        }
        Some(Action::DeleteRow(_)) | Some(Action::MakeFolder(_)) => unreachable!("turned into Delete and Folder"),
        Some(Action::Delete(ids)) => {
            // Parents first; a child whose parent went is already gone. Nothing holding a
            // one-per-scene thing (the camera) goes.
            let ids: Vec<ThingId> = ids.into_iter().filter(|&id| !holds_single(scene, reg, id)).collect();
            for id in ids {
                if let Ok(removed) = scene.remove_thing(id) {
                    selection.remove(&removed);
                    if tree.renaming.is_some_and(|r| removed.contains(&r)) {
                        tree.renaming = None;
                    }
                    out.removed.get_or_insert(id);
                    out.edited = true;
                }
            }
        }
        Some(Action::Add(kind)) => {
            // Into a selected folder, beside any other selected thing, or under Scene defaults.
            let parent = selection
                .primary
                .and_then(|s| scene.things.get(&s))
                .and_then(|t| if t.kind == "folder" { Some(t.id) } else { t.parent })
                .unwrap_or(ThingId::ROOT);
            // The kit links the starter modules a kind needs, such as a liquid's surface look.
            if let Ok(id) = bpe_kit::add_thing(scene, reg, kind, parent) {
                selection.set(id);
                out.edited = true;
            }
        }
        Some(Action::Folder(ids)) => match scene.make_folder(reg, &ids) {
            Ok(folder) => {
                selection.set(folder);
                out.edited = true;
            }
            Err(e) => out.message = Some(refusal(&e)),
        },
        Some(Action::SelectUsers(m)) => {
            let users = scene.users_of(m);
            selection.clear();
            if let Some((&first, rest)) = users.split_first() {
                selection.set(first);
                selection.extra.extend(rest.iter().copied());
            }
        }
        Some(Action::Attach(id, attach)) => match reattach(scene, id, attach) {
            Ok(()) => out.edited = true,
            Err(e) => out.message = Some(refusal(&e)),
        },
        Some(Action::AttachTo(id, parent)) => {
            let ride = Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None };
            match apply_drop(scene, id, parent, Drop::Into(ride)) {
                Ok(()) => out.edited = true,
                Err(e) => out.message = Some(refusal(&e)),
            }
        }
        Some(Action::Drop(dragged, onto, drop)) => match apply_drop(scene, dragged, onto, drop) {
            Ok(()) => out.edited = true,
            Err(e) => out.message = Some(refusal(&e)),
        },
        Some(Action::Link(id, m)) => match link_dropped(scene, reg, id, m) {
            Ok(()) => out.edited = true,
            Err(e) => out.message = Some(refusal(&e)),
        },
    }
    out
}

/// Links a module dropped onto a thing into the slot the module is made for.
pub(crate) fn link_dropped(
    scene: &mut Scene,
    reg: &Registry,
    onto: ThingId,
    module: ModuleId,
) -> Result<(), EditError> {
    let slot = scene.library.modules.get(&module).ok_or(EditError::MissingModule(module))?.slot;
    scene.link(reg, onto, slot, module)
}

/// Where in a row a drop lands: the top quarter puts the thing before the row, the bottom
/// quarter after it, the middle inside it, riding when Alt is held.
pub fn drop_action(row: Rect, pointer: Pos2, alt: bool) -> Drop {
    let along = (pointer.y - row.top()) / row.height().max(1.0);
    if along < 0.25 {
        Drop::Before
    } else if along > 0.75 {
        Drop::After
    } else if alt {
        Drop::Into(Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None })
    } else {
        Drop::Into(Attach::Group)
    }
}

/// Moves `dragged` where it was dropped. Before and after another row keep how the thing hangs
/// off its parent; into a row groups or rides it there, and riding keeps the thing where it is
/// on screen by measuring its offset from the new parent.
pub fn apply_drop(scene: &mut Scene, dragged: ThingId, onto: ThingId, drop: Drop) -> Result<(), EditError> {
    if dragged == onto {
        return Ok(());
    }
    let world = world_position(scene, dragged).ok_or(EditError::Missing(dragged))?;
    let (parent, attach) = match drop {
        Drop::Before | Drop::After => {
            let parent = scene.things.get(&onto).ok_or(EditError::Missing(onto))?.parent.ok_or(EditError::Root)?;
            (parent, scene.things[&dragged].attach)
        }
        Drop::Into(attach) => (onto, attach),
    };
    let attach = keeping_place(scene, dragged, parent, attach, world);
    scene.set_parent(dragged, parent, attach)?;
    if attach == Attach::Group {
        // Grouped, the thing's own position is where it shows: the spot it had while riding.
        scene.things.get_mut(&dragged).expect("moved above").transform.position = world;
    }
    match drop {
        Drop::Before => scene.move_before(dragged, onto),
        Drop::After => match next_sibling(scene, onto) {
            Some(next) => scene.move_before(dragged, next),
            None => Ok(()),
        },
        Drop::Into(_) => Ok(()),
    }
}

/// Where the thing sits on screen and which way it faces before anything moves, in video
/// pixels and radians: its own transform, or its ride's place worked out from the parent's (and
/// the parent's own ride, all the way up), turning the offset with the parent when the ride says
/// so. Riding one ball of a Balls parent counts as riding the parent's place.
fn world_place(scene: &Scene, id: ThingId) -> Option<(glam::Vec2, f32)> {
    let thing = scene.things.get(&id)?;
    let own = (thing.transform.position, thing.transform.rotation);
    let Attach::Ride { offset, turn_with_parent, .. } = thing.attach else { return Some(own) };
    let Some((parent, angle)) = thing.parent.and_then(|p| world_place(scene, p)) else { return Some(own) };
    Some(if turn_with_parent {
        (parent + glam::Vec2::from_angle(angle).rotate(offset), angle + thing.transform.rotation)
    } else {
        (parent + offset, thing.transform.rotation)
    })
}

/// Where the thing sits on screen before it moves: its own position, or its ride's.
fn world_position(scene: &Scene, id: ThingId) -> Option<glam::Vec2> {
    world_place(scene, id).map(|(position, _)| position)
}

/// The attach to use under `parent` so that a rider stays put on screen: its offset is measured
/// from the parent's place, turned back by the parent's angle when the ride turns with it.
fn keeping_place(scene: &Scene, id: ThingId, parent: ThingId, attach: Attach, world: glam::Vec2) -> Attach {
    match attach {
        Attach::Ride { turn_with_parent, item, .. } if parent != ThingId::ROOT && parent != id => {
            let (anchor, angle) = world_place(scene, parent).unwrap_or_default();
            let offset = world - anchor;
            let offset = if turn_with_parent { glam::Vec2::from_angle(-angle).rotate(offset) } else { offset };
            Attach::Ride { offset, turn_with_parent, item }
        }
        Attach::Ride { .. } => Attach::Group,
        Attach::Group => Attach::Group,
    }
}

/// Switches a thing between riding on and being grouped under the parent it has, without it
/// jumping on screen. A thing that already rides takes the ride as given: an offset, turn or
/// item typed in the inspector is what the user wants, not where the thing happens to be.
pub(crate) fn reattach(scene: &mut Scene, id: ThingId, attach: Attach) -> Result<(), EditError> {
    let world = world_position(scene, id).ok_or(EditError::Missing(id))?;
    let parent = scene.things[&id].parent.ok_or(EditError::Root)?;
    let rides = matches!(scene.things[&id].attach, Attach::Ride { .. });
    let attach = match attach {
        Attach::Ride { .. } if rides && parent != ThingId::ROOT => attach,
        _ => keeping_place(scene, id, parent, attach, world),
    };
    scene.set_attach(id, attach)?;
    if attach == Attach::Group {
        scene.things.get_mut(&id).expect("checked above").transform.position = world;
    }
    Ok(())
}

/// A refused edit, in plain words.
fn refusal(e: &EditError) -> String {
    match e {
        EditError::WouldLoop => "A thing can't go inside itself or its own children.".to_owned(),
        EditError::Root => "Scene defaults stay at the top.".to_owned(),
        other => format!("{other}"),
    }
}

/// Paints where a dragged row or module would land and, when it is let go, says so.
fn drop_target(ui: &mut Ui, scene: &Scene, rows: &[(ThingId, Rect)], action: &mut Option<Action>) {
    let ctx = ui.ctx();
    let thing = egui::DragAndDrop::payload::<ThingId>(ctx);
    let module = egui::DragAndDrop::payload::<ModuleId>(ctx);
    if thing.is_none() && module.is_none() {
        return;
    }
    let Some(pointer) = ctx.pointer_latest_pos() else { return };
    let dragged_name = thing.as_deref().and_then(|t| scene.things.get(t)).map(|t| t.name.clone());
    let gap = ui.spacing().item_spacing.y * 0.5;
    let over = rows.iter().find(|(_, r)| r.expand2(Vec2::new(0.0, gap)).contains(pointer));
    let Some((id, rect)) = over.filter(|(id, _)| thing.as_deref() != Some(id)) else {
        // Nothing to land on here: just the name travelling with the pointer.
        if let Some(name) = dragged_name {
            bubble(ui, pointer, name);
        }
        return;
    };
    let (alt, released) = ctx.input(|i| (i.modifiers.alt, i.pointer.any_released()));
    let drop = match &thing {
        Some(_) => drop_action(*rect, pointer, alt),
        None => Drop::Into(Attach::Group),
    };
    let stroke = Stroke::new(2.0, theme::ACCENT);
    match &drop {
        Drop::Before => {
            ui.painter().hline(rect.x_range(), rect.top() - gap, stroke);
        }
        Drop::After => {
            ui.painter().hline(rect.x_range(), rect.bottom() + gap, stroke);
        }
        Drop::Into(_) => {
            ui.painter().rect_stroke(rect.expand(1.0), 4.0, stroke, egui::StrokeKind::Outside);
        }
    }
    let onto = scene.things.get(id).map(|t| t.name.as_str()).unwrap_or_default();
    let what = dragged_name.unwrap_or_else(|| "Module".to_owned());
    let words = match &drop {
        Drop::Before | Drop::After => format!("{what} beside {onto}"),
        Drop::Into(Attach::Ride { .. }) => format!("{what} rides on {onto}"),
        Drop::Into(Attach::Group) if thing.is_some() => format!("{what} into {onto}"),
        Drop::Into(Attach::Group) => format!("{what} on {onto}"),
    };
    bubble(ui, pointer, words);
    if released {
        *action = match (thing, module) {
            (Some(dragged), _) => Some(Action::Drop(*dragged, *id, drop)),
            (None, Some(m)) => Some(Action::Link(*id, *m)),
            (None, None) => None,
        };
    }
}

/// A word by the pointer while dragging: what is dragged, or what letting go would do.
fn bubble(ui: &Ui, pointer: Pos2, words: String) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(words, font, theme::TEXT);
    let at = pointer + Vec2::new(14.0, 12.0);
    let rect = Rect::from_min_size(at, galley.size() + Vec2::splat(8.0));
    let painter = ui.painter().clone().with_clip_rect(ui.ctx().content_rect());
    painter.rect(rect, 4.0, theme::RAISED, Stroke::new(1.0, theme::LINE), egui::StrokeKind::Inside);
    painter.galley(at + Vec2::splat(4.0), galley, theme::TEXT);
}

/// Whether the filter text appears in the thing's name, kind or tags.
fn matches(thing: &Thing, filter: &str) -> bool {
    thing.name.to_lowercase().contains(filter)
        || thing.kind.to_lowercase().contains(filter)
        || thing.tags.iter().any(|t| t.to_lowercase().contains(filter))
}

/// What a row needs to know about its thing's place in the tree.
struct RowCx<'a> {
    depth: usize,
    has_children: bool,
    parent_name: Option<&'a str>,
    problems: &'a [String],
    selected: bool,
    /// One per scene, such as the camera: it can't be deleted or duplicated.
    single: bool,
}

/// Draws one row and returns the rectangle it covers.
fn row(ui: &mut Ui, tree: &mut Tree, scene: &Scene, thing: &Thing, cx: &RowCx, action: &mut Option<Action>) -> Rect {
    let id = thing.id;
    let ink = if thing.enabled { theme::ACCENT_TEXT } else { theme::MUTED };
    let response = ui.horizontal(|ui| {
        ui.add_space(cx.depth as f32 * 16.0);
        // The fold toggle, or its empty place so icons line up.
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
        if cx.has_children {
            let collapsed = tree.collapsed.contains(&id);
            let name = format!("{} {}", if collapsed { "Expand" } else { "Collapse" }, thing.name);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
            paint_fold(ui.painter(), rect, collapsed, theme::MUTED);
            if response.clicked() {
                *action = Some(Action::Collapse(id, !collapsed));
            }
        }
        if thing.rides() {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(10.0, 14.0), Sense::hover());
            let words = format!("Rides on {}", cx.parent_name.unwrap_or("its parent"));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, words.clone()));
            paint_hook(ui.painter(), rect, theme::MUTED);
            response.on_hover_text(words);
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        paint_icon(ui.painter(), rect, &thing.kind, ink);

        if tree.renaming == Some(id) {
            rename_field(ui, tree, id, action);
        } else {
            name_button(ui, scene, thing, cx, action);
        }
        if !thing.tags.is_empty() {
            let tags: Vec<&str> = thing.tags.iter().map(String::as_str).collect();
            ui.label(RichText::new(tags.join(" ")).small().color(theme::MUTED));
        }
        if !cx.problems.is_empty() {
            let (dot, response) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.5, theme::ERROR);
            response.on_hover_text(cx.problems.join("\n"));
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
    response.response.rect
}

/// The in-place rename field, taking the keyboard on its first frame.
fn rename_field(ui: &mut Ui, tree: &mut Tree, id: ThingId, action: &mut Option<Action>) {
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
}

/// The thing's name: a selectable button that can be dragged to another row, with the row's
/// right-click menu.
fn name_button(ui: &mut Ui, scene: &Scene, thing: &Thing, cx: &RowCx, action: &mut Option<Action>) {
    let id = thing.id;
    let color = if thing.enabled { theme::TEXT } else { theme::MUTED };
    let mut button = egui::Button::selectable(cx.selected, RichText::new(&thing.name).color(color));
    if id != ThingId::ROOT {
        // A drag-only layer over the button would swallow its clicks, so the button drags too.
        button = button.sense(Sense::click_and_drag());
    }
    let response = ui.add(button);
    if id != ThingId::ROOT {
        response.dnd_set_drag_payload(id);
    }
    if response.double_clicked() {
        *action = Some(Action::StartRename(id));
    } else if response.clicked() {
        *action = Some(Action::Select(id, ui.input(|i| i.modifiers)));
    }
    response.context_menu(|ui| context_menu(ui, scene, thing, cx.single, action));
}

fn context_menu(ui: &mut Ui, scene: &Scene, thing: &Thing, single: bool, action: &mut Option<Action>) {
    let id = thing.id;
    if ui.button("Rename").clicked() {
        *action = Some(Action::StartRename(id));
        ui.close();
    }
    if id != ThingId::ROOT {
        if !single && ui.button("Duplicate").clicked() {
            *action = Some(Action::Duplicate(id));
            ui.close();
        }
        if !single && ui.button("Delete").clicked() {
            *action = Some(Action::DeleteRow(id));
            ui.close();
        }
        ui.separator();
        if ui.button("Make folder from selection").on_hover_text("A folder around every selected thing.").clicked() {
            *action = Some(Action::MakeFolder(id));
            ui.close();
        }
        let (rides, ride) = match &thing.attach {
            Attach::Ride { .. } => (true, "Group under parent"),
            Attach::Group => (false, "Ride on parent"),
        };
        let help = if rides {
            "Stop following the parent; keep its own place."
        } else {
            "Follow the parent as it moves and turns."
        };
        if ui.button(ride).on_hover_text(help).clicked() {
            let attach = if rides {
                Attach::Group
            } else {
                Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None }
            };
            *action = Some(Action::Attach(id, attach));
            ui.close();
        }
        ui.menu_button("Attach to", |ui| {
            ui.label(RichText::new("Ride on…").small().color(theme::MUTED));
            // Everything but the thing itself, what hangs under it, and Scene defaults.
            let under = scene.descendants(id);
            for other in scene.order.iter().filter(|o| **o != id && **o != ThingId::ROOT && !under.contains(o)) {
                let Some(name) = scene.things.get(other).map(|t| t.name.as_str()) else { continue };
                if ui.button(name).clicked() {
                    *action = Some(Action::AttachTo(id, *other));
                    ui.close();
                }
            }
        });
    }
    let linked: Vec<(String, ModuleId)> = thing
        .slots
        .iter()
        .filter_map(|(slot, link)| match link {
            bpe_core::SlotLink::Linked { module, .. } => {
                let name = scene.library.modules.get(module).map(|m| m.name.as_str()).unwrap_or("missing");
                Some((format!("{}: {name}", slot.name()), *module))
            }
            _ => None,
        })
        .collect();
    if !linked.is_empty() {
        ui.menu_button("Select things using the same module", |ui| {
            for (words, m) in linked {
                if ui.button(words).clicked() {
                    *action = Some(Action::SelectUsers(m));
                    ui.close();
                }
            }
        });
    }
}

/// A right-pointing triangle when folded, a down-pointing one when open.
fn paint_fold(painter: &egui::Painter, rect: Rect, collapsed: bool, ink: Color32) {
    let c = rect.center();
    let r = 3.5;
    let points = if collapsed {
        vec![c + Vec2::new(-r * 0.6, -r), c + Vec2::new(r * 0.9, 0.0), c + Vec2::new(-r * 0.6, r)]
    } else {
        vec![c + Vec2::new(-r, -r * 0.6), c + Vec2::new(r, -r * 0.6), c + Vec2::new(0.0, r * 0.9)]
    };
    painter.add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
}

/// A hook from above into the icon: this thing rides on the one over it.
fn paint_hook(painter: &egui::Painter, rect: Rect, ink: Color32) {
    let stroke = Stroke::new(1.3, ink);
    let x = rect.left() + 3.0;
    let y = rect.center().y + 1.0;
    painter.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, y)], stroke);
    painter.line_segment([egui::pos2(x, y), egui::pos2(rect.right(), y)], stroke);
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(rect.right(), y),
            egui::pos2(rect.right() - 3.0, y - 2.5),
            egui::pos2(rect.right() - 3.0, y + 2.5),
        ],
        ink,
        Stroke::NONE,
    ));
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

/// The shape drawn next to a thing of some kind in the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Icon {
    /// An arena.
    Ring,
    /// Balls.
    Disc,
    /// The scene's defaults.
    Square,
    /// An emitter: a nozzle with drops leaving it.
    Spout,
    /// A liquid.
    Drop,
    /// A swarm: a scatter of dots.
    Dots,
    /// A field: lines of wind.
    Wind,
    /// A folder: a tabbed box.
    Folder,
    /// An obstacle: a bar with a peg.
    Bar,
    /// A soft body: a squashed blob.
    Blob,
    /// Text: a letter.
    Letter,
    /// The camera: a 9:16 frame.
    Frame,
    /// Anything else.
    Diamond,
}

fn icon_shape(kind: &str) -> Icon {
    match kind {
        "arena" => Icon::Ring,
        "balls" => Icon::Disc,
        "scene" => Icon::Square,
        "emitter" => Icon::Spout,
        "liquid" => Icon::Drop,
        "swarm" => Icon::Dots,
        "field" => Icon::Wind,
        "camera" => Icon::Frame,
        "folder" => Icon::Folder,
        "obstacle" => Icon::Bar,
        "soft" => Icon::Blob,
        "text" => Icon::Letter,
        _ => Icon::Diamond,
    }
}

fn paint_icon(painter: &egui::Painter, rect: Rect, kind: &str, ink: Color32) {
    let c = rect.center();
    let r = rect.width() * 0.32;
    let stroke = Stroke::new(1.5, ink);
    match icon_shape(kind) {
        Icon::Ring => {
            painter.circle_stroke(c, r, stroke);
        }
        Icon::Disc => {
            painter.circle_filled(c, r * 0.8, ink);
        }
        Icon::Square => {
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(r * 1.6)), 2.0, ink);
        }
        Icon::Spout => {
            // A nozzle at the bottom and three drops rising from it.
            let base = c + Vec2::new(0.0, r);
            painter.add(egui::Shape::convex_polygon(
                vec![base + Vec2::new(-r * 0.8, 0.0), base + Vec2::new(r * 0.8, 0.0), base + Vec2::new(0.0, -r * 0.9)],
                ink,
                Stroke::NONE,
            ));
            for (dx, dy) in [(-0.55, -0.5), (0.0, -0.95), (0.55, -0.5)] {
                painter.circle_filled(c + Vec2::new(dx * r, dy * r), r * 0.22, ink);
            }
        }
        Icon::Drop => {
            // A droplet: a round bottom with a point at the top.
            let bottom = c + Vec2::new(0.0, r * 0.25);
            painter.circle_filled(bottom, r * 0.72, ink);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    bottom + Vec2::new(-r * 0.7, -r * 0.15),
                    c + Vec2::new(0.0, -r * 1.05),
                    bottom + Vec2::new(r * 0.7, -r * 0.15),
                ],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::Dots => {
            for (dx, dy) in [(-0.7, -0.5), (0.5, -0.8), (0.8, 0.4), (-0.3, 0.7), (0.0, -0.1)] {
                painter.circle_filled(c + Vec2::new(dx * r, dy * r), r * 0.26, ink);
            }
        }
        Icon::Wind => {
            // Three lines of wind, the middle one longest, each ending in a curl.
            for (dy, len) in [(-0.6, 0.7), (0.0, 1.0), (0.6, 0.55)] {
                let y = c.y + dy * r;
                let from = egui::pos2(c.x - r, y);
                let to = egui::pos2(c.x - r + 2.0 * r * len, y);
                painter.line_segment([from, to], stroke);
                painter.circle_stroke(to + Vec2::new(0.0, -r * 0.22), r * 0.22, stroke);
            }
        }
        Icon::Folder => {
            let body = Rect::from_center_size(c + Vec2::new(0.0, r * 0.2), Vec2::new(r * 2.0, r * 1.4));
            painter.rect_stroke(body, 1.5, Stroke::new(1.3, ink), egui::StrokeKind::Inside);
            let tab = Rect::from_min_size(body.left_top() - Vec2::new(0.0, r * 0.5), Vec2::new(r * 0.9, r * 0.5));
            painter.rect_filled(tab, 1.0, ink);
        }
        Icon::Bar => {
            let bar = Rect::from_center_size(c + Vec2::new(0.0, r * 0.4), Vec2::new(r * 2.0, r * 0.6));
            painter.rect_filled(bar, 1.5, ink);
            painter.circle_filled(c + Vec2::new(r * 0.3, -r * 0.5), r * 0.4, ink);
        }
        Icon::Blob => {
            // A circle flattened on the ground it rests on.
            let body = Rect::from_center_size(c + Vec2::new(0.0, r * 0.25), Vec2::new(r * 2.1, r * 1.5));
            painter.rect_stroke(body, r * 0.7, stroke, egui::StrokeKind::Inside);
            painter.circle_filled(c + Vec2::new(-r * 0.3, -r * 0.1), r * 0.18, ink);
        }
        Icon::Letter => {
            let top = c + Vec2::new(0.0, -r * 0.9);
            painter.line_segment([top - Vec2::new(r * 0.9, 0.0), top + Vec2::new(r * 0.9, 0.0)], Stroke::new(2.0, ink));
            painter.line_segment([top, c + Vec2::new(0.0, r * 0.95)], Stroke::new(2.0, ink));
        }
        Icon::Frame => {
            let rect = Rect::from_center_size(c, Vec2::new(r * 1.15, r * 2.0));
            painter.rect_stroke(rect, 1.5, Stroke::new(1.6, ink), egui::StrokeKind::Inside);
            painter.circle_filled(c, r * 0.28, ink);
        }
        Icon::Diamond => {
            let points =
                vec![c + Vec2::new(0.0, -r), c + Vec2::new(r, 0.0), c + Vec2::new(0.0, r), c + Vec2::new(-r, 0.0)];
            painter.add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
        }
    }
}

/// A copy of `id` beside it, with the same settings, slots and transform. The scene's defaults
/// can't be copied.
/// Whether `id` or anything under it is a one-per-scene thing, such as the camera.
fn holds_single(scene: &Scene, reg: &Registry, id: ThingId) -> bool {
    scene.things.values().any(|t| {
        reg.thing(&t.kind).is_some_and(|k| k.single())
            && std::iter::successors(Some(t.id), |c| scene.things.get(c).and_then(|t| t.parent)).any(|a| a == id)
    })
}

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
    use eframe::egui::{self, Key, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        tree: Tree,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
        messages: Vec<String>,
    }

    fn harness() -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let state = State {
            tree: Tree::default(),
            scene,
            kit,
            resolved,
            selection: Selection::default(),
            messages: Vec::new(),
        };
        let mut h = Harness::builder()
            .with_size(egui::vec2(260.0, 600.0))
            // Real frame times, so two quick clicks count as a double-click.
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let out = show(ui, &mut s.tree, &mut s.scene, &s.kit, &s.resolved, &mut s.selection);
                    if out.edited {
                        s.resolved = resolve(&s.scene, &s.kit.registry);
                    }
                    s.messages.extend(out.message);
                },
                state,
            );
        h.run();
        h
    }

    /// Clicks the row called `label` with `modifiers` held.
    fn click_with(h: &mut Harness<'_, State>, label: &str, modifiers: Modifiers) {
        h.get_by_label(label).click_modifiers(modifiers);
        h.run();
    }

    /// Presses at `from`, moves to `to` in a few steps and lets go, with `modifiers` held.
    fn drag(h: &mut Harness<'_, State>, from: Pos2, to: Pos2, modifiers: Modifiers) {
        h.hover_at(from);
        h.step();
        h.event(egui::Event::ModifiersChanged(modifiers));
        h.event(egui::Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers });
        h.step();
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            h.event(egui::Event::PointerMoved(from + (to - from) * t));
            h.step();
        }
        h.event(egui::Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers });
        h.step();
        h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
        h.run();
    }

    fn add_folder(h: &mut Harness<'_, State>) -> ThingId {
        let s = h.state_mut();
        let f = s.scene.add_thing(&s.kit.registry, "folder", ThingId::ROOT).unwrap();
        s.resolved = resolve(&s.scene, &s.kit.registry);
        h.run();
        f
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
        assert_eq!(s.selection.primary, Some(ThingId(3)));
        // The new scene's ball is called "Ball", so the first added Balls keeps the kind's name.
        assert_eq!(s.scene.things[&ThingId(3)].name, "Balls");
        assert_eq!(s.scene.things[&ThingId(3)].parent, Some(ThingId::ROOT));
        assert!(h.query_by_label("Balls").is_some());
    }

    #[test]
    fn adding_into_a_selected_folder_puts_the_thing_inside() {
        let mut h = harness();
        let f = add_folder(&mut h);
        h.state_mut().selection.set(f);
        h.get_by_label("+ Add thing").click();
        h.run();
        h.get_by_label("Obstacle").click();
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(4)].parent, Some(f));
    }

    #[test]
    fn delete_removes_the_selected_thing_but_never_the_root() {
        let mut h = harness();
        h.get_by_label("Ball").click();
        h.run();
        h.key_press(Key::Delete);
        h.run();
        assert!(!h.state().scene.things.contains_key(&ThingId(2)));
        assert_eq!(h.state().selection.primary, None);
        h.state_mut().selection.set(ThingId::ROOT);
        h.key_press(Key::Delete);
        h.run();
        assert!(h.state().scene.things.contains_key(&ThingId::ROOT));
    }

    #[test]
    fn ctrl_click_extends_the_selection_and_delete_removes_it_all() {
        let mut h = harness();
        h.get_by_label("Arena").click();
        h.run();
        click_with(&mut h, "Ball", Modifiers::COMMAND);
        assert_eq!(h.state().selection.all(&h.state().scene), vec![ThingId(1), ThingId(2)]);
        // Ctrl-clicking again takes it out.
        click_with(&mut h, "Ball", Modifiers::COMMAND);
        assert_eq!(h.state().selection.all(&h.state().scene), vec![ThingId(1)]);
        click_with(&mut h, "Ball", Modifiers::COMMAND);
        h.key_press(Key::Delete);
        h.run();
        assert_eq!(h.state().scene.things.len(), 1);
        assert_eq!(h.state().selection, Selection::default());
    }

    #[test]
    fn shift_click_selects_the_rows_between() {
        let mut h = harness();
        let f = add_folder(&mut h);
        h.get_by_label("Arena").click();
        h.run();
        click_with(&mut h, "Folder", Modifiers::SHIFT);
        assert_eq!(h.state().selection.all(&h.state().scene), vec![ThingId(1), ThingId(2), f]);
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

    #[test]
    fn adding_a_liquid_from_the_tree_links_liquid_surface() {
        let mut h = harness();
        h.get_by_label("+ Add thing").click();
        h.run();
        h.get_by_label("Liquid").click();
        h.run();
        let s = h.state();
        let id = s.selection.primary.expect("the new liquid is selected");
        assert_eq!(s.scene.things[&id].kind, "liquid");
        let bpe_core::SlotLink::Linked { module, .. } = s.scene.things[&id].slots[&SlotId::Look] else {
            panic!("the liquid starts with its own look")
        };
        assert_eq!(s.scene.library.modules[&module].name, "Liquid surface");
        assert!(h.query_by_label("Liquid").is_some());
    }

    #[test]
    fn icons_exist_for_every_kit_kind() {
        let reg = bpe_kit::registry();
        for kind in reg.things() {
            assert_ne!(icon_shape(kind.id()), Icon::Diamond, "{} has no icon of its own", kind.id());
        }
        assert_eq!(icon_shape("something-else"), Icon::Diamond);
    }

    #[test]
    fn drop_zones_split_a_row_in_three() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 20.0));
        assert_eq!(drop_action(rect, egui::pos2(10.0, 2.0), false), Drop::Before);
        assert_eq!(drop_action(rect, egui::pos2(10.0, 19.0), false), Drop::After);
        assert_eq!(drop_action(rect, egui::pos2(10.0, 10.0), false), Drop::Into(Attach::Group));
        assert!(matches!(
            drop_action(rect, egui::pos2(10.0, 10.0), true),
            Drop::Into(Attach::Ride { turn_with_parent: true, .. })
        ));
    }

    #[test]
    fn applying_drops_regroups_reorders_and_refuses_loops() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let f = scene.add_thing(&kit.registry, "folder", ThingId::ROOT).unwrap();
        apply_drop(&mut scene, ThingId(2), f, Drop::Into(Attach::Group)).unwrap();
        assert_eq!(scene.things[&ThingId(2)].parent, Some(f));
        apply_drop(&mut scene, ThingId(2), ThingId(1), Drop::Before).unwrap();
        assert_eq!(scene.order, vec![ThingId::ROOT, ThingId(2), ThingId(1), f]);
        apply_drop(&mut scene, f, ThingId(2), Drop::After).unwrap();
        assert_eq!(scene.order, vec![ThingId::ROOT, ThingId(2), f, ThingId(1)]);
        scene.things.get_mut(&ThingId(2)).unwrap().transform.position = glam::Vec2::new(50.0, 0.0);
        let ride = Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None };
        apply_drop(&mut scene, ThingId(2), ThingId(1), Drop::Into(ride)).unwrap();
        assert!(
            matches!(scene.things[&ThingId(2)].attach, Attach::Ride { offset, .. } if offset == glam::Vec2::new(50.0, 0.0))
        );
        // Moved beside the folder, the rider keeps riding its new parent from the same spot.
        apply_drop(&mut scene, ThingId(2), f, Drop::Before).unwrap();
        assert_eq!(scene.things[&ThingId(2)].parent, Some(ThingId::ROOT));
        assert_eq!(scene.things[&ThingId(2)].attach, Attach::Group);
        assert_eq!(scene.things[&ThingId(2)].transform.position, glam::Vec2::new(50.0, 0.0));
        // Back under the arena, nothing can make the arena hang under it.
        apply_drop(&mut scene, ThingId(2), ThingId(1), Drop::Into(Attach::Group)).unwrap();
        assert!(matches!(
            apply_drop(&mut scene, ThingId(1), ThingId(2), Drop::Into(Attach::Group)),
            Err(EditError::WouldLoop)
        ));
        assert!(matches!(apply_drop(&mut scene, f, ThingId::ROOT, Drop::Before), Err(EditError::Root)));
        // Dropping a thing on itself changes nothing.
        let before = scene.clone();
        apply_drop(&mut scene, f, f, Drop::Into(Attach::Group)).unwrap();
        assert_eq!(scene, before);
    }

    #[test]
    fn grouping_a_rider_back_under_its_parent_keeps_its_place() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        scene.things.get_mut(&ThingId(1)).unwrap().transform.position = glam::Vec2::new(100.0, 100.0);
        let ride = Attach::Ride { offset: glam::Vec2::new(10.0, 0.0), turn_with_parent: true, item: None };
        scene.set_parent(ThingId(2), ThingId(1), ride).unwrap();
        reattach(&mut scene, ThingId(2), Attach::Group).unwrap();
        assert_eq!(scene.things[&ThingId(2)].attach, Attach::Group);
        assert_eq!(scene.things[&ThingId(2)].transform.position, glam::Vec2::new(110.0, 100.0));
        reattach(&mut scene, ThingId(2), ride).unwrap();
        assert!(
            matches!(scene.things[&ThingId(2)].attach, Attach::Ride { offset, .. } if offset == glam::Vec2::new(10.0, 0.0))
        );
        // Already riding, a new offset (typed in the inspector) is taken as it is.
        let typed = Attach::Ride { offset: glam::Vec2::new(0.0, -100.0), turn_with_parent: false, item: None };
        reattach(&mut scene, ThingId(2), typed).unwrap();
        assert_eq!(scene.things[&ThingId(2)].attach, typed);
        assert!(matches!(reattach(&mut scene, ThingId::ROOT, Attach::Group), Err(EditError::Root)));
    }

    #[test]
    fn a_rider_dropped_at_the_top_level_keeps_its_place_and_turned_parents_are_measured_right() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let f = scene.add_thing(&kit.registry, "folder", ThingId::ROOT).unwrap();
        scene.things.get_mut(&ThingId(1)).unwrap().transform.position = glam::Vec2::new(100.0, 100.0);
        let ride = Attach::Ride { offset: glam::Vec2::new(0.0, -50.0), turn_with_parent: true, item: None };
        scene.set_parent(ThingId(2), ThingId(1), ride).unwrap();
        // Dropped beside the folder at the top level, the balls stop riding where they were.
        apply_drop(&mut scene, ThingId(2), f, Drop::Before).unwrap();
        assert_eq!(scene.things[&ThingId(2)].parent, Some(ThingId::ROOT));
        assert_eq!(scene.things[&ThingId(2)].attach, Attach::Group);
        assert_eq!(scene.things[&ThingId(2)].transform.position, glam::Vec2::new(100.0, 50.0));
        // The arena is turned a quarter: riding it from (100, 50) is an offset of (-50, 0) before
        // the turn, which the turn puts above the centre.
        scene.things.get_mut(&ThingId(1)).unwrap().transform.rotation = std::f32::consts::FRAC_PI_2;
        apply_drop(&mut scene, ThingId(2), ThingId(1), Drop::Into(ride)).unwrap();
        let Attach::Ride { offset, .. } = scene.things[&ThingId(2)].attach else { panic!() };
        assert!((offset - glam::Vec2::new(-50.0, 0.0)).length() < 1e-3, "{offset:?}");
        assert!((world_position(&scene, ThingId(2)).unwrap() - glam::Vec2::new(100.0, 50.0)).length() < 1e-3);
        // A folder riding the arena carries its own riders' places along.
        apply_drop(&mut scene, f, ThingId(1), Drop::Into(ride)).unwrap();
        apply_drop(&mut scene, ThingId(2), f, Drop::Into(ride)).unwrap();
        assert!((world_position(&scene, ThingId(2)).unwrap() - glam::Vec2::new(100.0, 50.0)).length() < 1e-3);
    }

    #[test]
    fn a_collapsed_folder_hides_its_children_and_a_rider_shows_a_hook() {
        let mut h = harness();
        {
            let s = h.state_mut();
            let f = s.scene.add_thing(&s.kit.registry, "folder", ThingId::ROOT).unwrap();
            s.scene.set_parent(ThingId(2), f, Attach::Group).unwrap();
            let ride = Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None };
            s.scene.set_parent(ThingId(1), ThingId(2), ride).unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        assert!(h.query_by_label("Ball").is_some());
        assert!(h.query_by_label("Rides on Ball").is_some());
        h.get_by_label("Collapse Folder").click();
        h.run();
        assert!(h.query_by_label("Ball").is_none());
        assert!(h.query_by_label("Arena").is_none());
        h.get_by_label("Expand Folder").click();
        h.run();
        assert!(h.query_by_label("Ball").is_some());
    }

    #[test]
    fn dragging_a_row_onto_a_folder_groups_it_there() {
        let mut h = harness();
        let f = add_folder(&mut h);
        let from = h.get_by_label("Ball").rect().center();
        let to = h.get_by_label("Folder").rect().center();
        drag(&mut h, from, to, Modifiers::NONE);
        assert_eq!(h.state().scene.things[&ThingId(2)].parent, Some(f));
        assert_eq!(h.state().scene.things[&ThingId(2)].attach, Attach::Group);
        // Dragging it back out, onto the top edge of the arena's row, puts it before the arena.
        let from = h.get_by_label("Ball").rect().center();
        let arena = h.get_by_label("Arena").rect();
        drag(&mut h, from, egui::pos2(arena.center().x, arena.top() + 1.0), Modifiers::NONE);
        assert_eq!(h.state().scene.order, vec![ThingId::ROOT, ThingId(2), ThingId(1), f]);
        // A thing can't be dropped into itself: the status line says so.
        let from = h.get_by_label("Scene defaults").rect().center();
        let to = h.get_by_label("Folder").rect().center();
        drag(&mut h, from, to, Modifiers::NONE);
        assert_eq!(h.state().scene.things[&ThingId::ROOT].parent, None);
        assert!(h.state().messages.is_empty(), "the root isn't draggable at all: {:?}", h.state().messages);
    }

    #[test]
    fn dragging_with_alt_makes_the_thing_ride() {
        let mut h = harness();
        let from = h.get_by_label("Ball").rect().center();
        let to = h.get_by_label("Arena").rect().center();
        drag(&mut h, from, to, Modifiers::ALT);
        let thing = &h.state().scene.things[&ThingId(2)];
        assert_eq!(thing.parent, Some(ThingId(1)));
        assert!(matches!(thing.attach, Attach::Ride { .. }), "{:?}", thing.attach);
    }

    #[test]
    fn dropping_a_module_on_a_thing_without_the_slot_is_refused() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let f = scene.add_thing(&kit.registry, "arena", ThingId::ROOT).unwrap();
        let motion = scene.library.modules.values().find(|m| m.name == "Default motion").unwrap().id;
        let err = link_dropped(&mut scene, &kit.registry, f, motion).unwrap_err();
        assert_eq!(err.to_string(), "This kind of thing has no Motion slot");
        let look = scene.library.modules.values().find(|m| m.name == "Ball look").unwrap().id;
        link_dropped(&mut scene, &kit.registry, f, look).unwrap();
        assert!(
            matches!(scene.things[&f].slots[&SlotId::Look], bpe_core::SlotLink::Linked { module, .. } if module == look)
        );
        assert!(matches!(link_dropped(&mut scene, &kit.registry, f, ModuleId(99)), Err(EditError::MissingModule(_))));
    }

    #[test]
    fn a_module_dropped_on_a_row_links_it() {
        let mut h = harness();
        let m = {
            let s = h.state_mut();
            let m = s.scene.add_module(&s.kit.registry, "look.classic", "Red look").unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
            m
        };
        h.run();
        let to = h.get_by_label("Arena").rect().center();
        // The Library (another panel) sets the payload; here the drag is started by hand.
        h.hover_at(to);
        egui::DragAndDrop::set_payload(&h.ctx, m);
        h.event(egui::Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        egui::DragAndDrop::set_payload(&h.ctx, m);
        h.event(egui::Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.run();
        let s = h.state();
        assert!(
            matches!(s.scene.things[&ThingId(1)].slots.get(&SlotId::Look), Some(bpe_core::SlotLink::Linked { module, .. }) if *module == m)
        );
    }

    #[test]
    fn camera_cannot_be_deleted_or_doubled() {
        let mut h = harness();
        let s = h.state_mut();
        let cam = bpe_kit::ensure_camera(&mut s.scene, &s.kit.registry);
        s.resolved = resolve(&s.scene, &s.kit.registry);
        h.run();
        h.get_by_label("Camera").click();
        h.run();
        h.key_press(Key::Delete);
        h.run();
        assert!(h.state().scene.things.contains_key(&cam));
        h.get_by_label("+ Add thing").click();
        h.run();
        assert!(h.query_by_label("Camera").is_none() || h.query_all_by_label("Camera").count() == 1);
        assert!(duplicate(&mut h.state_mut().scene, &bpe_kit::registry(), cam).is_none());
    }
}
