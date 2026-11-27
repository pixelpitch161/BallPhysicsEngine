//! A Water scene's stage, in place of the ball preview (Water mode part 1): a strip with the
//! active tool's hint, the tool rail, and the course fitted to the room with the camera's shot
//! and a marker for each thing that is at a place. Since part 2b the water plays on it at draft
//! detail, drawn in the plain look (`draft`, `picture`); since part 3 the Piece and Pen tools add
//! pieces and walls, and Select moves, turns, sizes and moulds them (`piece_tool`, `pen_tool`,
//! `solid_edit`).

use super::course_view::CourseView;
use super::draft::frame_of;
use super::final_job::WaterView;
use super::final_view::{self, FinalShown};
use super::picture::{Rock, Shown};
use super::rail::{self, RAIL_W, Tool};
use crate::app::Studio;
use crate::style;
use bpe_core::{Settings, SlotId, ThingId};
use bpe_kit::modules::water::{Keep, ReadyLook, SCREEN, WaterWorld};
use bpe_kit::things::water;
use bpe_water::SolidField;
use bpe_water::draw::look::{Look, Quality};
use eframe::egui::{self, Align2, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use glam::Vec2;

/// The strip's height.
const STRIP_H: f32 = 32.0;
/// The room the Use a look menu takes at the strip's right end.
const LOOK_MENU_W: f32 = 100.0;
/// A marker's radius, and how close a click must land to pick it.
const MARKER_R: f32 = 11.0;

/// What the Water stage keeps between frames.
#[derive(Default)]
pub struct WaterStage {
    pub tool: Tool,
    /// The thing being dragged, and where it is held: its place minus the pointer's world point.
    pub(super) drag: Option<(ThingId, Vec2)>,
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
    /// The Piece tool's next piece, and the Pen's next wall.
    pub piece: super::piece_tool::PieceBrush,
    pub pen: super::pen_tool::PenBrush,
    /// The wall being drawn with the Pen (world points).
    pub line: Option<Vec<Vec2>>,
    /// The handle Select is dragging, and whether a piece or wall is being moved or moulded (the
    /// picture then shows the course as it will be).
    pub grab: Option<super::solid_edit::Grab>,
    pub moulding: bool,
    /// The course as it will be while a stroke is painted or a solid moved or moulded (kept after a
    /// move until the draft plays it), and a count that goes up when it changes.
    preview: Option<Preview>,
    preview_version: u64,
    /// What Use a look leaves as it is.
    pub keep: Keep,
    /// What the stage shows: the whole course, or the camera's shot with its whole finish.
    pub seen: Seen,
    /// Grade mode's scopes and clipping picture of the shot.
    pub graded: super::graded::Graded,
}

/// What the Water stage shows (decision 44).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Seen {
    /// The whole course, with bloom and exposure but no Effects or Grade (they are made for the
    /// shot's shape).
    #[default]
    Course,
    /// The camera's shot at the playhead, with the whole finish, as the video shows it.
    Shot,
}

/// The course as it will be, and what it was made for.
struct Preview {
    /// The carve stroke's version and the scene's physics (`WaterSetup::print`).
    stroke: u64,
    print: u64,
    /// Whether a piece or wall was moved or moulded (not a stroke painted).
    moulded: bool,
    field: SolidField,
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
pub(super) fn marker_at(view: &CourseView, things: &[(ThingId, Vec2)], at: Pos2) -> Option<(ThingId, Vec2)> {
    things.iter().rev().copied().find(|(_, p)| view.to_screen(*p).distance(at) <= MARKER_R + 1.0)
}

/// Draws the Water stage and applies what was clicked and dragged (in Grade mode, the shot under
/// Grade mode's tools: `graded`).
pub fn stage(ui: &mut Ui, studio: &mut Studio) {
    if studio.mode == crate::grade_mode::EditorMode::Grade {
        super::graded::stage(ui, studio);
        return;
    }
    let (strip, rail, room) = rooms(ui.available_rect_before_wrap(), studio.water_stage.strip_h);
    let tool = studio.water_stage.tool;
    // The strip: the tool and what it does, on more rows when they don't fit on one.
    ui.painter().rect_filled(strip, 0.0, style::PANEL);
    ui.painter().hline(strip.x_range(), strip.bottom(), Stroke::new(1.0, style::LINE_SOFT));
    let inside = strip.shrink2(vec2(10.0, 0.0));
    // (Use a look sits at the first row's right end, so the tool's row never wraps for it.)
    let menu = Rect::from_min_max(
        pos2(inside.right() - LOOK_MENU_W, inside.top()),
        pos2(inside.right(), inside.top() + STRIP_H),
    );
    let first = Rect::from_min_size(inside.min, vec2(inside.width() - LOOK_MENU_W - 8.0, STRIP_H));
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(first).layout(layout));
    bar.label(RichText::new(tool.name()).font(style::semibold(13.0)));
    match tool {
        Tool::Carve => super::carve_tool::strip(&mut bar, &mut studio.water_stage.brush),
        Tool::Piece => super::piece_tool::strip(&mut bar, &mut studio.water_stage.piece),
        Tool::Pen => {
            super::pen_tool::strip(&mut bar, &mut studio.water_stage.pen);
            bar.label(RichText::new(tool.hint()).color(style::MUTED));
        }
        _ => {
            bar.label(RichText::new(tool.hint()).color(style::MUTED));
        }
    }
    // A picked ready-made piece can become a Custom piece of its outline.
    if matches!(tool, Tool::Select | Tool::Piece)
        && let Some(id) = studio.selection.primary
        && super::piece_tool::can_make_custom(studio, id)
        && bar
            .button("Make custom")
            .on_hover_text("Turn the piece into a Custom one of its outline, to move its points")
            .clicked()
    {
        super::piece_tool::make_custom(studio, id);
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
    let mut menu_ui =
        ui.new_child(egui::UiBuilder::new().max_rect(menu).layout(egui::Layout::right_to_left(egui::Align::Center)));
    look_menu(&mut menu_ui, studio);
    // (Each row is a strip's height: the first fills the strip, and the rest wrap below it. Never
    // while a button is held: picking a piece adds to the strip, and the course moving under a
    // drag would move the piece with it.)
    let strip_h = (bar.min_rect().bottom() - strip.top()).max(STRIP_H);
    let held = ui.input(|i| i.pointer.any_down());
    if (strip_h - studio.water_stage.strip_h.max(STRIP_H)).abs() > 0.5 && !held {
        studio.water_stage.strip_h = strip_h;
        ui.ctx().request_repaint();
    }

    rail::show(ui, rail, &mut studio.water_stage.tool);
    let tool = studio.water_stage.tool;

    let response = ui.allocate_rect(room, Sense::click_and_drag());
    let view = match studio.water_stage.seen {
        Seen::Course => CourseView::fit(room, course_size(studio)),
        Seen::Shot => {
            let (centre, shot) = bpe_cli::water::shot_at(&studio.scene, &studio.kit.registry, studio.time);
            CourseView::fit_shot(room, course_size(studio), centre, shot)
        }
    };
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
    } else if tool == Tool::Piece {
        super::piece_tool::place(ui, &response, studio, &view);
    } else if tool == Tool::Pen {
        super::pen_tool::draw(ui, &response, studio, &view);
    } else if tool == Tool::Select {
        super::solid_edit::select(ui, &response, studio, &view, &things);
    }
    if !response.dragged() {
        super::solid_edit::let_go(studio);
    }
    if tool != Tool::Carve {
        super::carve_tool::drop_stroke(studio);
    }
    if tool != Tool::Pen {
        super::pen_tool::drop_line(studio);
    }
    let shown = water(ui, studio, &view, room).is_some();
    paint(ui, studio, &view, room, shown);
    super::solid_edit::paint(ui, studio, &view, room, tool == Tool::Select);
    let hover = response.hover_pos().filter(|p| view.holds(*p));
    match tool {
        Tool::Carve => {
            if let Some(at) = hover {
                super::carve_tool::outline(ui, studio, &view, at);
            }
        }
        Tool::Piece => {
            if let Some(at) = hover {
                super::piece_tool::ghost(ui, studio, &view, at);
            }
        }
        Tool::Pen => super::pen_tool::show_line(ui, studio, &view, hover),
        _ => {}
    }
    if water::carving_of(&studio.scene).list.is_empty() {
        // (A label that senses nothing, so the stage under it still takes the clicks.)
        let words = egui::Label::new(RichText::new(ALL_ROCK).color(style::MUTED)).selectable(false);
        let seen = view.seen_rect();
        let at = Rect::from_center_size(seen.center(), vec2(seen.width().max(40.0), 40.0));
        ui.put(at, words.wrap());
    }
}

/// Plays the draft to the playhead and draws its water over the course, or in the Final view the
/// final's moment where it is made. Returns the picture of the water on the stage, if one is.
pub(super) fn water(ui: &Ui, studio: &mut Studio, view: &CourseView, room: Rect) -> Option<Shown> {
    let render = studio.panels.inspector.render.clone()?;
    studio.water_draft.start_on(&render);
    update_preview(studio);
    let painter = ui.painter_at(room);
    let look_here = look_now(studio, view);
    // The Final view: the final's moment, unless it isn't made (the draft shows then).
    let wf = &mut studio.water_final;
    let final_shown = match (&wf.found, wf.view) {
        (Some(found), WaterView::Final) => Some(final_view::shown_at(found, studio.time)),
        _ => None,
    };
    if let Some(shown) = final_shown
        && shown != FinalShown::NotMade
        && let Some(found) = &wf.found
    {
        let drawn = wf.picture.show(&painter, &render, found, &wf.finals, &look_here, view, studio.time);
        if let FinalShown::OutOfDate { restart } = shown {
            pill(ui, &painter, view.seen_rect().center_bottom() - vec2(0.0, 14.0), final_view::out_of_date(restart));
        }
        if drawn {
            return wf.picture.shown().cloned();
        }
    }
    let job = studio.water_draft.job.as_ref()?;
    let target = frame_of(studio.time);
    job.follow(target);
    let restarting = studio.water_draft.setup.as_ref().is_some_and(|s| s.print != job.plays());
    // (Until the new body comes, the old one keeps the look its liquids were numbered for, with
    // the finish as it is now, so Grade mode's changes show meanwhile.)
    let look = match (restarting, studio.water_draft.picture.last_look()) {
        (true, Some(last)) => Look { finish: look_here.finish.clone(), ..last.clone() },
        _ => look_here,
    };
    let rock = Rock {
        version: studio.water_stage.preview_version,
        preview: studio.water_stage.preview.as_ref().map(|p| &p.field),
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
        pill(ui, &painter, view.seen_rect().center_top() + vec2(0.0, 14.0), behind);
    }
    if final_shown == Some(FinalShown::NotMade) {
        pill(ui, &painter, view.seen_rect().center_bottom() - vec2(0.0, 14.0), final_view::NOT_MADE.to_owned());
    } else if let Some(note) = studio.water_draft.picture.notes().first() {
        pill(ui, &painter, view.seen_rect().center_bottom() - vec2(0.0, 14.0), note.clone());
    }
    if shown { studio.water_draft.picture.shown().cloned() } else { None }
}

/// The scene's look at the playhead, with Draft light while a drag holds the runs, so the stage
/// keeps up with it; the whole course has no Effects or Grade (they are made for the shot). In
/// Grade mode the grade is shown as Grade mode shows it (Before, Split, the overlay, a stroke being
/// painted), and with the snapshot the pointer is over put on.
pub(super) fn look_now(studio: &Studio, view: &CourseView) -> Look {
    let grading = studio.mode == crate::grade_mode::EditorMode::Grade && view.shot.is_some();
    let peeked = grading.then(|| crate::preview::peeked(studio)).flatten();
    let (scene, resolved) = match peeked.as_ref().and_then(|p| p.as_ref()) {
        Some(p) => (&p.scene, &p.resolved),
        None => (&studio.scene, &studio.resolved),
    };
    let mut look = water::look::look_of(scene, resolved, studio.time);
    drop(peeked);
    // (Not in Grade mode: a press there is on the grade, which doesn't move the water, and the
    // scopes, clipping and picks read the picture as the video shows it.)
    if studio.hold_runs && !grading {
        look.light.quality = Quality::Draft;
    }
    if view.shot.is_none() {
        look.finish = look.finish.without_shot_finish();
    } else if grading {
        studio.grade.show_on(&mut look.finish.grade);
    }
    look
}

/// A line of words in a pill centred at `at`.
fn pill(ui: &Ui, painter: &egui::Painter, at: Pos2, words: String) {
    let small = egui::TextStyle::Small.resolve(ui.style());
    let text = painter.layout_no_wrap(words, small, style::TEXT);
    let pill = Rect::from_center_size(at, text.size() + vec2(16.0, 6.0));
    painter.rect_filled(pill, pill.height() / 2.0, style::PILL);
    painter.galley(pill.center() - text.size() / 2.0, text, style::TEXT);
}

/// Makes the course as it will be while a carve stroke is painted (with the pieces and walls over
/// it) or a piece or wall is moved or moulded (the draft's restart waits for the drag to rest),
/// again only when the stroke or the scene's physics changes. A moved or moulded course is kept
/// once it is let go until the draft plays it, so the piece never jumps back meanwhile; a stroke's
/// is let go with it (the carving's own rule).
fn update_preview(studio: &mut Studio) {
    let (painting, moulding) = (studio.water_stage.painting.is_some(), studio.water_stage.moulding);
    if !painting && !moulding {
        let draft = &studio.water_draft;
        let keep = studio.water_stage.preview.as_ref().is_some_and(|p| {
            let played = draft.job.as_ref().is_none_or(|job| job.plays() == p.print);
            let coming = studio.runs_due.is_some() || draft.setup.as_ref().is_some_and(|s| s.print == p.print);
            p.moulded && !played && coming
        });
        if !keep && studio.water_stage.preview.take().is_some() {
            studio.water_stage.preview_version += 1;
        }
        return;
    }
    let Some(setup) = super::draft::WaterRun::setup_of(&studio.scene, &studio.resolved) else { return };
    let stroke = studio.water_stage.field_version;
    if studio.water_stage.preview.as_ref().is_some_and(|p| p.stroke == stroke && p.print == setup.print) {
        return;
    }
    let field = match &studio.water_stage.painting {
        Some(painting) => setup.field_with(painting.mask()),
        None => setup.field(),
    };
    let moulded = moulding && !painting;
    studio.water_stage.preview = Some(Preview { stroke, print: setup.print, moulded, field });
    studio.water_stage.preview_version += 1;
}

/// The Use a look menu (decision 40): the seven looks, then the two switches.
fn look_menu(bar: &mut Ui, studio: &mut Studio) {
    let mut picked = None;
    let keep = &mut studio.water_stage.keep;
    // (The switches leave it open; a look closes it.)
    let stay = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let menu = egui::containers::menu::MenuButton::new("Use a look…").config(stay).ui(bar, |ui| {
        for look in ReadyLook::ALL {
            if ui.button(look.title()).on_hover_text(look.words()).clicked() {
                picked = Some(look);
                ui.close();
            }
        }
        ui.separator();
        ui.checkbox(&mut keep.colours, "Keep my colours")
            .on_hover_text("Leave every colour and gradient, and how each liquid's colour is picked, as it is");
        ui.checkbox(&mut keep.lamps, "Keep my lamps").on_hover_text("Leave the lamps as they are, and add none");
    });
    menu.0.on_hover_text("Set the liquids, the rock, the back, the light and the lamps to a ready-made look");
    if let Some(look) = picked {
        use_look(studio, look);
    }
}

/// Sets the scene to `look`, keeping what the menu's switches say, as one undo step.
pub(crate) fn use_look(studio: &mut Studio, look: ReadyLook) {
    let kit = studio.kit.clone();
    let before: Vec<ThingId> = studio.scene.things.keys().copied().collect();
    bpe_kit::modules::water::apply_look(&mut studio.scene, &kit.registry, look, studio.water_stage.keep);
    // (Lamps it took away are no longer picked.)
    let gone: Vec<ThingId> = before.into_iter().filter(|id| !studio.scene.things.contains_key(id)).collect();
    studio.selection.remove(&gone);
    studio.separate_edit = true;
    studio.status = format!("Using the {} look.", look.title());
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
    // (In the Shot view only the shot: the course reaches past it.)
    let painter = ui.painter_at(if view.shot.is_some() { room.intersect(view.seen_rect()) } else { room });
    if !shown {
        painter.rect_filled(view.seen_rect(), 0.0, style::GROUND);
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
    // (The Shot view is the shot.)
    if view.shot.is_none()
        && let Some(shot) = camera_shot(studio, view)
    {
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
        if thing.kind == water::LAMP {
            lamp_marks(&painter, studio, view, id, place, ink);
        }
        if thing.kind == water::FORCE {
            force_marks(&painter, studio, view, id, place, ink);
        }
    }
}

/// A force's reach: a Magnet's and a Whirlpool's ring (a Magnet's spikes as ticks across it, a
/// Whirlpool's spin as an arrow head on it), a Wind's three arrows along its Aim. At the playhead,
/// with its keyframes.
fn force_marks(
    painter: &egui::Painter,
    studio: &Studio,
    view: &CourseView,
    id: ThingId,
    place: Vec2,
    ink: egui::Color32,
) {
    use bpe_kit::modules::water::ForceKind;
    let Some(force) = water::draft::force_card_at(&studio.scene, &studio.resolved, id, studio.time) else { return };
    let line = Stroke::new(1.0, ink);
    let at = view.to_screen(place);
    let way_at = |angle: f32| {
        let (sin, cos) = angle.sin_cos();
        vec2(cos, sin)
    };
    let head = |tip: Pos2, way: egui::Vec2| {
        painter.line_segment([tip, tip - way * 7.0 + way.rot90() * 4.0], line);
        painter.line_segment([tip, tip - way * 7.0 - way.rot90() * 4.0], line);
    };
    match force.kind {
        ForceKind::Magnet | ForceKind::Whirlpool => {
            let r = (force.reach.max(0.0) * view.scale()).max(MARKER_R + 6.0);
            painter.circle_stroke(at, r, Stroke::new(1.0, style::LINE_SOFT));
            if force.kind == ForceKind::Magnet && force.spikes > 0 {
                let n = force.spikes.min(64);
                let spread = force.spike_spread.clamp(0.0, 360.0).to_radians();
                let from = force.spike_aim.to_radians() - spread / 2.0;
                for k in 0..n {
                    let d = way_at(from + spread * (k as f32 + 0.5) / n as f32);
                    painter.line_segment([at + d * (r - 5.0), at + d * (r + 5.0)], line);
                }
            }
            if force.kind == ForceKind::Whirlpool && force.strength != 0.0 {
                // (Above 0 it turns clockwise on the screen: at the ring's top, to the right.)
                let tip = at + vec2(0.0, -r);
                head(tip, vec2(force.strength.signum(), 0.0));
            }
        }
        ForceKind::Wind => {
            let way = way_at(force.aim.to_radians()) * if force.strength < 0.0 { -1.0 } else { 1.0 };
            for i in [-1.0, 0.0, 1.0] {
                let from = at + way.rot90() * (22.0 * i) - way * 18.0 + way * (MARKER_R + 22.0);
                let tip = from + way * 36.0;
                painter.line_segment([from, tip], line);
                head(tip, way);
            }
        }
        ForceKind::Heater | ForceKind::Cooler => {}
    }
}

/// How far a Spot's cone is drawn from its marker, screen points.
const CONE_LONG: f32 = 120.0;

/// A lamp's light: a Spot's cone (its Spread), a Strip's line (its Length) with a tick toward its
/// aim, a Sun's arrows down from the course's top edge.
fn lamp_marks(
    painter: &egui::Painter,
    studio: &Studio,
    view: &CourseView,
    id: ThingId,
    place: Vec2,
    ink: egui::Color32,
) {
    use bpe_kit::modules::water::look::LampKind;
    // (At the playhead, with its keyframes, as the light is drawn.)
    let Some(lamp) = water::look::lamp_at(&studio.scene, &studio.resolved, id, studio.time) else { return };
    let line = Stroke::new(1.0, ink);
    let at = view.to_screen(place);
    let aim = lamp.aim.to_radians();
    let way_at = |angle: f32| {
        let (sin, cos) = angle.sin_cos();
        vec2(cos, sin)
    };
    let way = way_at(aim);
    match lamp.kind {
        LampKind::Spot => {
            let half = (lamp.spread.clamp(1.0, 360.0) / 2.0).to_radians();
            if half >= std::f32::consts::PI - 1e-3 {
                painter.circle_stroke(at, CONE_LONG, line);
                return;
            }
            for side in [-half, half] {
                let d = way_at(aim + side);
                painter.line_segment([at + d * MARKER_R, at + d * CONE_LONG], line);
            }
        }
        LampKind::Strip => {
            let across = way.rot90();
            let half = (lamp.length.max(0.0) * view.scale() / 2.0).max(MARKER_R);
            let thick = Stroke::new(2.0, ink);
            painter.line_segment([at - across * half, at - across * MARKER_R], thick);
            painter.line_segment([at + across * MARKER_R, at + across * half], thick);
            painter.line_segment([at + way * MARKER_R, at + way * (MARKER_R + 14.0)], line);
        }
        LampKind::Sun => {
            for i in [-1.0, 0.0, 1.0] {
                let from = pos2(at.x + 36.0 * i, view.rect.top());
                let tip = from + way * 40.0;
                painter.line_segment([from, tip], line);
                painter.line_segment([tip, tip - way * 7.0 + way.rot90() * 4.0], line);
                painter.line_segment([tip, tip - way * 7.0 - way.rot90() * 4.0], line);
            }
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
    bpe_kit::camera_of(&studio.scene)?;
    let (centre, shot) = bpe_cli::water::shot_at(&studio.scene, &studio.kit.registry, studio.time);
    let size = shot * view.scale();
    Some(Rect::from_center_size(view.to_screen(centre), vec2(size.x, size.y)))
}

#[cfg(test)]
pub(crate) fn rooms_for_tests(full: Rect) -> (Rect, Rect, Rect) {
    rooms(full, STRIP_H)
}
