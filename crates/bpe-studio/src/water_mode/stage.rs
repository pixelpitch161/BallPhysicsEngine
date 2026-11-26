//! A Water scene's stage, in place of the ball preview (Water mode part 1): a strip with the
//! active tool's hint, the tool rail, and the course fitted to the room with the camera's shot
//! and a marker for each thing that is at a place. Since part 2b the water plays on it at draft
//! detail, drawn in the plain look (`draft`, `picture`).

use super::course_view::CourseView;
use super::draft::frame_of;
use super::picture::Rock;
use super::rail::{self, RAIL_W, Tool};
use crate::app::Studio;
use crate::style;
use bpe_core::{Address, Settings, SlotId, ThingId, Value};
use bpe_kit::modules::water::{SCREEN, WaterWorld};
use bpe_kit::things::water;
use eframe::egui::{self, Align2, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use glam::Vec2;

/// The strip's height.
const STRIP_H: f32 = 32.0;
/// A marker's radius, and how close a click must land to pick it.
const MARKER_R: f32 = 11.0;

/// What the Water stage keeps between frames.
#[derive(Default)]
pub struct WaterStage {
    pub tool: Tool,
    /// The thing being dragged, and where it is held: its place minus the pointer's world point.
    drag: Option<(ThingId, Vec2)>,
    /// Where the course was drawn last frame.
    pub view: Option<CourseView>,
    /// Goes up whenever the carve stroke being painted changes (or is let go or dropped), so the
    /// picture shows its rock.
    pub field_version: u64,
    /// The strip's height last frame (it grows to more rows when the stage is too narrow).
    strip_h: f32,
    /// The Carve tool's brush, and the stroke it is painting.
    pub brush: super::carve_tool::CarveBrush,
    pub painting: Option<super::carve_tool::Painting>,
}

/// What the stage says over a course with nothing carved.
pub const ALL_ROCK: &str = "The course is solid rock. Pick Carve and paint to dig it out.";
/// How far (frames) the draft may lag the playhead before the stage says it is working.
const BEHIND: u32 = 15;

/// The strip (`strip_h` high), the rail and the course's room inside `full`.
fn rooms(full: Rect, strip_h: f32) -> (Rect, Rect, Rect) {
    let strip = Rect::from_min_size(full.min, vec2(full.width(), strip_h.max(STRIP_H)));
    let rail = Rect::from_min_max(pos2(full.left(), strip.bottom()), pos2(full.left() + RAIL_W, full.bottom()));
    let room = Rect::from_min_max(pos2(rail.right(), strip.bottom()), full.max);
    (strip, rail, room)
}

/// The course's size in world pixels, from the scene's Water card.
pub fn course_size(studio: &Studio) -> Vec2 {
    let card = studio.resolved.things.get(&ThingId::ROOT).and_then(|t| t.slots.get(&SlotId::Water));
    card.map_or(SCREEN, |card| WaterWorld::from_params(&card.params).course_size())
}

/// The things with a marker, in the scene's order: the ones at a place.
fn placed(studio: &Studio) -> Vec<(ThingId, Vec2)> {
    let scene = &studio.scene;
    let things = scene.order.iter().filter_map(|id| scene.things.get(id).map(|t| (*id, t)));
    things.filter(|(_, t)| water::is_placed(&t.kind)).map(|(id, t)| (id, t.transform.position)).collect()
}

/// The marker under the screen point, the one drawn last (on top) first.
fn marker_at(view: &CourseView, things: &[(ThingId, Vec2)], at: Pos2) -> Option<(ThingId, Vec2)> {
    things.iter().rev().copied().find(|(_, p)| view.to_screen(*p).distance(at) <= MARKER_R + 1.0)
}

/// Draws the Water stage and applies what was clicked and dragged.
pub fn stage(ui: &mut Ui, studio: &mut Studio) {
    let (strip, rail, room) = rooms(ui.available_rect_before_wrap(), studio.water_stage.strip_h);
    let tool = studio.water_stage.tool;
    // The strip: the tool and what it does, on more rows when they don't fit on one.
    ui.painter().rect_filled(strip, 0.0, style::PANEL);
    ui.painter().hline(strip.x_range(), strip.bottom(), Stroke::new(1.0, style::LINE_SOFT));
    let inside = strip.shrink2(vec2(10.0, 0.0));
    let first = Rect::from_min_size(inside.min, vec2(inside.width(), STRIP_H));
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(first).layout(layout));
    bar.label(RichText::new(tool.name()).font(style::semibold(13.0)));
    if tool == Tool::Carve {
        super::carve_tool::strip(&mut bar, &mut studio.water_stage.brush);
    } else {
        bar.label(RichText::new(tool.hint()).color(style::MUTED));
    }
    // What a picked pour can't do (yet).
    let note = studio.selection.primary.and_then(|id| {
        let setup = studio.water_draft.setup.as_ref()?;
        setup.note_for(id).or_else(|| {
            let job = studio.water_draft.job.as_ref()?;
            (job.plays() == setup.print && job.in_rock(id)).then_some(water::draft::IN_ROCK)
        })
    });
    if let Some(note) = note {
        bar.label(RichText::new(note).color(style::TEXT));
    }
    let strip_h = (bar.min_rect().bottom() - strip.top() + 6.0).max(STRIP_H);
    if (strip_h - studio.water_stage.strip_h.max(STRIP_H)).abs() > 0.5 {
        studio.water_stage.strip_h = strip_h;
        ui.ctx().request_repaint();
    }

    rail::show(ui, rail, &mut studio.water_stage.tool);
    let tool = studio.water_stage.tool;

    let response = ui.allocate_rect(room, Sense::click_and_drag());
    let view = CourseView::fit(room, course_size(studio));
    studio.water_stage.view = Some(view);
    let step = studio.grid.step(ui.input(|i| i.modifiers.alt));
    let things = placed(studio);

    if let Some(kind) = tool.places() {
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
            && view.holds(at)
        {
            place(studio, kind, crate::grid::snap_point(view.to_world(at), step));
        }
    } else if tool == Tool::Carve {
        super::carve_tool::paint(ui, &response, studio, &view);
    } else if tool == Tool::Select {
        if response.drag_started()
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
            && let Some((id, place)) = marker_at(&view, &things, press)
        {
            studio.selection.set(id);
            studio.water_stage.drag = Some((id, place - view.to_world(press)));
        }
        if response.dragged()
            && let Some((id, held)) = studio.water_stage.drag
            && let Some(at) = response.interact_pointer_pos()
            && let Some(thing) = studio.scene.things.get_mut(&id)
        {
            let to = crate::grid::snap_point(view.to_world(at) + held, step);
            if thing.transform.position != to {
                thing.transform.position = to;
            }
        }
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            match marker_at(&view, &things, at) {
                Some((id, _)) => studio.selection.set(id),
                None => studio.selection.clear(),
            }
        }
    }
    if !response.dragged() {
        studio.water_stage.drag = None;
    }
    if tool != Tool::Carve {
        super::carve_tool::drop_stroke(studio);
    }
    let shown = water(ui, studio, &view, room);
    paint(ui, studio, &view, room, shown);
    if tool == Tool::Carve
        && let Some(at) = response.hover_pos()
        && view.holds(at)
    {
        super::carve_tool::outline(ui, studio, &view, at);
    }
    if water::carving_of(&studio.scene).list.is_empty() {
        // (A label that senses nothing, so the stage under it still takes the clicks.)
        let words = egui::Label::new(RichText::new(ALL_ROCK).color(style::MUTED)).selectable(false);
        let at = Rect::from_center_size(view.rect.center(), vec2(view.rect.width().max(40.0), 40.0));
        ui.put(at, words.wrap());
    }
}

/// Plays the draft to the playhead and draws its water over the course. Returns whether a
/// picture of it is on the stage.
fn water(ui: &Ui, studio: &mut Studio, view: &CourseView, room: Rect) -> bool {
    let Some(render) = studio.panels.inspector.render.clone() else { return false };
    studio.water_draft.start_on(&render);
    let Some(job) = studio.water_draft.job.as_ref() else { return false };
    let target = frame_of(studio.time);
    job.follow(target);
    let restarting = studio.water_draft.setup.as_ref().is_some_and(|s| s.print != job.plays());
    // (Until the new body comes, the old one keeps the look its liquids were numbered for.)
    let fresh = || bpe_kit::things::water::draft::plain_look(&studio.scene, &studio.resolved, studio.time);
    let look = match (restarting, studio.water_draft.picture.last_look()) {
        (true, Some(last)) => last.clone(),
        _ => fresh(),
    };
    let painter = ui.painter_at(room);
    let rock = Rock {
        version: studio.water_stage.field_version,
        painting: studio.water_stage.painting.as_ref().map(|p| p.mask()),
    };
    let shown = studio.water_draft.picture.show(&painter, &render, job, &look, view, rock);
    if job.busy() || studio.playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
    if let Some(reached) = job.reached()
        && (reached + BEHIND < target || restarting)
    {
        let behind = match restarting {
            true => "Starting the water over…".to_owned(),
            false => format!("Working out the water… {:.1} s", f64::from(reached) / 60.0),
        };
        let at = view.rect.center_top() + vec2(0.0, 14.0);
        let small = egui::TextStyle::Small.resolve(ui.style());
        let text = painter.layout_no_wrap(behind, small, style::TEXT);
        let pill = Rect::from_center_size(at, text.size() + vec2(16.0, 6.0));
        painter.rect_filled(pill, pill.height() / 2.0, style::PILL);
        painter.galley(pill.center() - text.size() / 2.0, text, style::TEXT);
    }
    shown
}

/// Adds a thing of `kind` at the world point, selects it, and goes back to Select.
fn place(studio: &mut Studio, kind: &'static str, at: Vec2) {
    match bpe_kit::add_thing(&mut studio.scene, &studio.kit.registry, kind, ThingId::ROOT) {
        Ok(id) => {
            if let Some(thing) = studio.scene.things.get_mut(&id) {
                thing.transform.position = at;
            }
            // The first pour brings a Liquid to pour (in the same undo step).
            let has_liquid = studio.scene.things.values().any(|t| t.kind == water::LIQUID);
            if kind == water::POUR
                && !has_liquid
                && let Ok(liquid) =
                    bpe_kit::add_thing(&mut studio.scene, &studio.kit.registry, water::LIQUID, ThingId::ROOT)
                && let Some(thing) = studio.scene.things.get_mut(&liquid)
            {
                thing.name = "Water".into();
            }
            studio.selection.set(id);
            studio.separate_edit = true;
        }
        Err(e) => studio.status = format!("Couldn't add it: {e}"),
    }
    studio.water_stage.tool = Tool::Select;
}

/// The course's outline with a line between its screens, the camera's shot, and the markers; the
/// flat ground under them where no water picture is (`shown` false).
fn paint(ui: &Ui, studio: &Studio, view: &CourseView, room: Rect, shown: bool) {
    let painter = ui.painter_at(room);
    if !shown {
        painter.rect_filled(view.rect, 0.0, style::GROUND);
    }
    let soft = Stroke::new(1.0, style::LINE_SOFT);
    let (wide, tall) = ((view.size.x / SCREEN.x).round() as u32, (view.size.y / SCREEN.y).round() as u32);
    for i in 1..wide {
        painter.vline(view.rect.left() + view.rect.width() * i as f32 / wide as f32, view.rect.y_range(), soft);
    }
    for i in 1..tall {
        painter.hline(view.rect.x_range(), view.rect.top() + view.rect.height() * i as f32 / tall as f32, soft);
    }
    painter.rect_stroke(view.rect, 0.0, Stroke::new(1.0, style::LINE), egui::StrokeKind::Outside);

    let small = egui::TextStyle::Small.resolve(ui.style());
    if let Some(shot) = camera_shot(studio, view) {
        painter.rect_stroke(shot, 0.0, Stroke::new(1.0, style::MUTED), egui::StrokeKind::Inside);
        let at = shot.left_top() + vec2(4.0, 3.0);
        painter.text(at, Align2::LEFT_TOP, "Camera", small.clone(), style::MUTED);
    }
    for (id, place) in placed(studio) {
        let Some(thing) = studio.scene.things.get(&id) else { continue };
        let at = view.to_screen(place);
        let ink = style::colour_of(&studio.scene, id);
        painter.circle_filled(at, MARKER_R, style::PANEL);
        painter.circle_stroke(at, MARKER_R, Stroke::new(1.0, ink));
        crate::tree::paint_icon(&painter, Rect::from_center_size(at, vec2(22.0, 22.0)), &thing.kind, ink);
        if studio.selection.contains(id) {
            painter.circle_stroke(at, MARKER_R + 3.0, Stroke::new(1.5, style::TEXT_STRONG));
        }
        let name = at + vec2(MARKER_R + 6.0, 0.0);
        painter.text(name, Align2::LEFT_CENTER, &thing.name, small.clone(), style::TEXT);
        if thing.kind == water::POUR {
            pour_marks(&painter, studio, view, id, place, ink);
        }
    }
}

/// A pour's aim and its stream's width (a tap), or the box it fills at the start.
fn pour_marks(
    painter: &egui::Painter,
    studio: &Studio,
    view: &CourseView,
    id: ThingId,
    place: Vec2,
    ink: egui::Color32,
) {
    let Some(card) = studio.resolved.things.get(&id).and_then(|t| t.slots.get(&SlotId::Main)) else { return };
    let pour = bpe_kit::modules::water::PourCard::from_params(&card.params);
    let line = Stroke::new(1.0, ink);
    let at = view.to_screen(place);
    if pour.kind == bpe_kit::modules::water::PourKind::FillAtTheStart {
        let size = pour.fill_size.max(Vec2::ZERO) * view.scale();
        painter.rect_stroke(Rect::from_center_size(at, vec2(size.x, size.y)), 0.0, line, egui::StrokeKind::Middle);
        return;
    }
    let (sin, cos) = pour.aim.to_radians().sin_cos();
    let way = vec2(cos, sin);
    let tip = at + way * (MARKER_R + 40.0);
    painter.line_segment([at + way * MARKER_R, tip], line);
    painter.line_segment([tip, tip - way * 7.0 + way.rot90() * 4.0], line);
    painter.line_segment([tip, tip - way * 7.0 - way.rot90() * 4.0], line);
    let half = ((pour.rate * pour.width).max(0.0) * view.scale() / 2.0).max(3.0);
    let across = way.rot90() * half;
    painter.line_segment([at + way * (MARKER_R + 3.0) - across, at + way * (MARKER_R + 3.0) + across], line);
}

/// The camera's 9:16 shot on screen, at its Position and Zoom at the playhead.
fn camera_shot(studio: &Studio, view: &CourseView) -> Option<Rect> {
    let camera = bpe_kit::camera_of(&studio.scene)?;
    let reg = &studio.kit.registry;
    let at = |key: &str| studio.scene.value_at_time(reg, &Address::own(camera, key), studio.time);
    let centre = match at("position") {
        Some(Value::Vec2([x, y])) => Vec2::new(x, y),
        _ => Vec2::ZERO,
    };
    let zoom = match at("zoom") {
        Some(Value::Number(z)) if z > 0.0 => z as f32,
        _ => 1.0,
    };
    let size = SCREEN / zoom * view.scale();
    Some(Rect::from_center_size(view.to_screen(centre), vec2(size.x, size.y)))
}

#[cfg(test)]
pub(crate) fn rooms_for_tests(full: Rect) -> (Rect, Rect, Rect) {
    rooms(full, STRIP_H)
}
