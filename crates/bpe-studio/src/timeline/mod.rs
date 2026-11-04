//! The timeline panel: the transport, the ruler, the Hits lane, and a group per thing with keys or
//! pulses. The selected thing's group is open, a lane per setting drawn as a clip in its track
//! colour, with keys as diamonds that can be moved, eased and deleted; the others are one bar.

pub mod ease;
pub mod lanes;

use crate::app::{Studio, clock};
use crate::style;
use crate::transport;
use bpe_core::{Address, Ease, PulseSource, ThingId, Value};
use bpe_runtime::EventKind;
use eframe::egui::{self, Align, Color32, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder};
use lanes::{Group, Lane, groups_for};
use std::collections::BTreeSet;

/// The width of the column naming each lane.
pub const LABEL_WIDTH: f32 = style::LANE_NAME_W;
pub const LANE_HEIGHT: f32 = style::LANE_H;
const GROUP_HEIGHT: f32 = style::GROUP_H;
/// Keys dragged this close to a hit or a whole second snap to it.
const SNAP_PX: f32 = 6.0;
const KEY_SIZE: f32 = 4.5;

/// What the timeline remembers between frames.
#[derive(Default)]
pub struct Timeline {
    /// The key under the selection: its setting and its index on the track.
    pub selected: Option<(Address, usize)>,
    dragging: Option<(Address, usize)>,
    /// The curve editor is open for the selected key.
    curve_open: bool,
}

/// Where time sits across the lanes.
struct Ruler {
    track: Rect,
    duration: f64,
}

impl Ruler {
    fn x_of(&self, video_time: f64) -> f32 {
        self.track.left() + (video_time / self.duration).clamp(0.0, 1.0) as f32 * self.track.width()
    }
    fn time_of(&self, x: f32) -> f64 {
        ((x - self.track.left()) / self.track.width().max(1.0)).clamp(0.0, 1.0) as f64 * self.duration
    }
}

pub fn show(ui: &mut Ui, studio: &mut Studio) {
    transport::controls(ui, studio);
    let track = transport::scrub_bar(ui, studio, LABEL_WIDTH + 8.0);
    let ruler = Ruler { track, duration: studio.duration().max(0.001) };
    let groups = groups_for(&studio.scene, &studio.kit);
    // Only the selected thing's group is open; with nothing selected, the scene settings'.
    let open = studio.selection.primary.unwrap_or(ThingId::ROOT);
    let mut timeline = std::mem::take(&mut studio.panels.timeline);
    // A selection that points at a key that is gone is no selection.
    if let Some((a, i)) = &timeline.selected
        && studio.scene.tracks.get(a).is_none_or(|t| *i >= t.keys.len())
    {
        timeline.selected = None;
        timeline.curve_open = false;
    }
    // A key in a folded group is out of sight, so it is not selected (Delete would remove it unseen).
    if timeline.selected.as_ref().is_some_and(|(a, _)| a.thing != open) {
        timeline.selected = None;
        timeline.dragging = None;
        timeline.curve_open = false;
    }
    let mut picked = None;
    let mut recoloured = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        hits_lane(ui, studio, &ruler);
        if groups.is_empty() {
            ui.add_space(6.0);
            let hint = "Right-click a setting and pick Create keyframe. Its lane shows up here under its thing.";
            ui.label(RichText::new(hint).small().color(style::MUTED));
        }
        let mut index = 0;
        for group in &groups {
            let colour = studio.scene.things.get(&group.thing).and_then(|t| t.colour).map(style::track_color);
            let is_open = group.thing == open;
            let (clicked, colour_now) = group_header(ui, studio, group, colour, is_open, &ruler);
            if clicked {
                picked = Some(group.thing);
            }
            if let Some(c) = colour_now {
                recoloured = Some((group.thing, c));
            }
            if !is_open {
                index += group.lanes.len();
                continue;
            }
            for lane in &group.lanes {
                if lane.pulse {
                    pulse_lane(ui, studio, lane, colour, &ruler);
                } else {
                    track_lane(ui, studio, &mut timeline, lane, index, colour, &ruler);
                }
                index += 1;
            }
        }
    });
    if let Some(id) = picked {
        studio.selection.set(id);
    }
    if let Some((id, colour)) = recoloured
        && let Some(thing) = studio.scene.things.get_mut(&id)
    {
        thing.colour = Some(colour);
    }
    // Delete removes the selected key, and is used up so the tree keeps its thing.
    if let Some((a, i)) = timeline.selected.clone()
        && !ui.ctx().text_edit_focused()
        && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete))
    {
        studio.scene.remove_key(&a, i);
        timeline.selected = None;
        timeline.curve_open = false;
    }
    curve_window(ui, studio, &mut timeline);
    studio.panels.timeline = timeline;
}

/// The Hits lane: a tick per hit at its time, in the colour of the thing that hit.
fn hits_lane(ui: &mut Ui, studio: &Studio, ruler: &Ruler) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), Sense::hover());
    let name = Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, LANE_HEIGHT));
    name_cell(ui, name, egui::Label::new(RichText::new("Hits").small().color(style::MUTED)).selectable(false));
    let strip = strip_of(rect, ruler);
    let painter = ui.painter_at(rect);
    // One tick per pixel column and hitter is enough, however many hits land there.
    let rate = studio.playback.world().steps_per_second;
    let mut ticks: BTreeSet<(i32, ThingId)> = BTreeSet::new();
    for e in studio.playback.log().events().iter().filter(|e| e.kind == EventKind::Hit) {
        ticks.insert((ruler.x_of(e.time(rate)).round() as i32, e.a.thing));
    }
    for (x, thing) in ticks {
        let x = x as f32;
        let tick = Rect::from_min_max(Pos2::new(x - 1.0, strip.top() + 2.0), Pos2::new(x + 1.0, strip.bottom() - 2.0));
        painter.rect_filled(tick, 0.0, hit_colour(&studio.scene, thing));
    }
    playhead(&painter, studio, ruler, rect);
}

/// The colour of a hit's tick: the track colour of the thing that hit, or plain text for a
/// thing without one.
pub fn hit_colour(scene: &bpe_core::Scene, thing: ThingId) -> Color32 {
    style::colour_of(scene, thing)
}

/// A group's header: its swatch and name, and across the track a thin line in its colour when
/// open, or when folded a bar with every key of the thing on it. A click selects the thing,
/// which opens its group. True when clicked.
fn group_header(
    ui: &mut Ui,
    studio: &Studio,
    group: &Group,
    colour: Option<Color32>,
    open: bool,
    ruler: &Ruler,
) -> (bool, Option<bpe_core::TrackColour>) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), GROUP_HEIGHT), Sense::click());
    let name = format!("{} lanes", group.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    let painter = ui.painter_at(rect);
    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, style::LINE_SOFT));
    let swatch = Rect::from_center_size(Pos2::new(rect.left() + 5.0, rect.center().y), egui::vec2(10.0, 10.0));
    match colour {
        Some(c) => painter.rect_filled(swatch, 0.0, c),
        None => painter.rect_filled(swatch.shrink(1.0), 2.0, style::MUTED),
    };
    let ink = if response.hovered() { style::TEXT_STRONG } else { style::TEXT };
    let mut job = egui::text::LayoutJob::simple_singleline(group.name.clone(), style::semibold(12.0), ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width(LABEL_WIDTH - 22.0);
    let text = painter.layout_job(job);
    painter.galley(Pos2::new(swatch.right() + 7.0, rect.center().y - text.size().y / 2.0), text, ink);
    let strip = strip_of(rect, ruler);
    let edge = colour.unwrap_or(style::MUTED);
    if open {
        let line = Rect::from_center_size(Pos2::new(strip.center().x, rect.center().y), egui::vec2(strip.width(), 3.0));
        painter.rect_filled(line, 0.0, edge.gamma_multiply(0.55));
    } else {
        let bar = strip.shrink2(egui::vec2(0.0, 2.0));
        let (fill, edge) = style::clip_colours(colour);
        painter.rect_filled(bar, 0.0, fill);
        painter.hline(bar.x_range(), bar.top() + 0.5, Stroke::new(1.0, edge));
        for lane in group.lanes.iter().filter(|l| !l.pulse) {
            for key in studio.scene.tracks.get(&lane.address).map(|t| t.keys.as_slice()).unwrap_or_default() {
                style::paint_diamond(&painter, Pos2::new(ruler.x_of(key.time), bar.center().y), 3.5, false);
            }
        }
    }
    playhead(&painter, studio, ruler, rect);
    // The colour square opens a colour picker for the thing's colour.
    let mut recoloured = None;
    if colour.is_some() {
        let current = studio.scene.things.get(&group.thing).and_then(|t| t.colour);
        let square = ui.interact(swatch.expand(3.0), response.id.with("colour"), Sense::click());
        let words = format!("Colour of {}", group.name);
        square.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, words.clone()));
        let square = square.on_hover_text("Click to change this thing's colour");
        egui::Popup::menu(&square).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            recoloured = style::track_picker(ui, current);
        });
    }
    let hint = if open { "This thing's lanes" } else { "Click to open this thing's lanes" };
    (response.on_hover_text(hint).clicked(), recoloured)
}

/// A lane's name, at the left of its column and centred up and down.
fn name_cell(ui: &mut Ui, rect: Rect, label: egui::Label) -> egui::Response {
    let mut cell = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    cell.add(label)
}

/// Where a row's track is: under the ruler, inside the row.
fn strip_of(rect: Rect, ruler: &Ruler) -> Rect {
    Rect::from_min_max(
        Pos2::new(ruler.track.left(), rect.top() + 2.0),
        Pos2::new(ruler.track.right(), rect.bottom() - 2.0),
    )
}

/// The playhead's line across one row.
fn playhead(painter: &egui::Painter, studio: &Studio, ruler: &Ruler, rect: Rect) {
    let x = ruler.x_of(studio.time);
    painter.vline(x, rect.y_range(), Stroke::new(1.0, style::TEXT_STRONG));
}

/// The lane's name and its strip drawn as a clip in the thing's colour, with the playhead across.
fn lane_frame(
    ui: &mut Ui,
    studio: &Studio,
    lane: &Lane,
    colour: Option<Color32>,
    ruler: &Ruler,
) -> (Rect, Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), Sense::click());
    let label_rect =
        Rect::from_min_max(rect.min + egui::vec2(17.0, 0.0), Pos2::new(rect.left() + LABEL_WIDTH, rect.bottom()));
    let label = egui::Label::new(RichText::new(&lane.short).small().color(style::MUTED)).truncate().selectable(false);
    let shown = name_cell(ui, label_rect, label);
    let full = lane.label.clone();
    shown.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, full.clone()));
    let strip = strip_of(rect, ruler);
    let painter = ui.painter_at(rect);
    let (fill, edge) = style::clip_colours(colour);
    painter.rect_filled(strip, 0.0, fill);
    painter.hline(strip.x_range(), strip.top() + 0.5, Stroke::new(1.0, edge));
    playhead(&painter, studio, ruler, rect);
    (rect, strip, response)
}

/// A keyed setting: a diamond per key. Click selects, drag moves, double-click on empty space
/// adds a key, right-click eases or deletes.
fn track_lane(
    ui: &mut Ui,
    studio: &mut Studio,
    timeline: &mut Timeline,
    lane: &Lane,
    index: usize,
    colour: Option<Color32>,
    ruler: &Ruler,
) {
    let (rect, strip, response) = lane_frame(ui, studio, lane, colour, ruler);
    let a = &lane.address;
    let keys: Vec<(f64, Ease)> =
        studio.scene.tracks.get(a).map(|t| t.keys.iter().map(|k| (k.time, k.ease)).collect()).unwrap_or_default();
    let mut ease_pick: Option<(usize, Ease, bool)> = None;
    let mut delete: Option<usize> = None;
    for (i, (time, ease)) in keys.iter().enumerate() {
        let video = *time;
        let center = Pos2::new(ruler.x_of(video), rect.center().y);
        let key_rect = Rect::from_center_size(center, egui::vec2(14.0, 14.0));
        let key = ui.interact(key_rect, ui.scope_id().with(("key", index, i)), Sense::click_and_drag());
        let name = format!("Key at {} of {}", clock(video), lane.label);
        key.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
        let selected = timeline.selected.as_ref() == Some(&(a.clone(), i));
        let size = if key.hovered() { KEY_SIZE + 1.0 } else { KEY_SIZE };
        style::paint_diamond(ui.painter(), center, size, selected);
        if key.clicked() || key.drag_started() {
            timeline.selected = Some((a.clone(), i));
        }
        if key.drag_started() {
            timeline.dragging = Some((a.clone(), i));
            studio.set_playing(false);
        }
        key.context_menu(|ui| {
            timeline.selected = Some((a.clone(), i));
            ui.label(RichText::new("Eases into the next key").small().color(style::MUTED));
            if let Some(picked) = ease::ease_menu(ui, ease) {
                ease_pick = Some((i, picked, matches!(picked, Ease::Curve { .. })));
            }
            ui.separator();
            if ui.button("Delete key").clicked() {
                delete = Some(i);
                ui.close();
            }
        });
        key.on_hover_text(format!("{} · {}\nDrag to move · right-click for the ease", clock(video), ease.name()));
    }
    if let Some((i, picked, own)) = ease_pick {
        studio.scene.set_ease(a, i, picked);
        if own {
            timeline.curve_open = true;
        }
    }
    if let Some(i) = delete {
        studio.scene.remove_key(a, i);
        timeline.selected = None;
    }
    // A drag in progress on this lane follows the pointer, snapping to hits and seconds.
    if let Some((drag_a, i)) = timeline.dragging.clone()
        && drag_a == *a
    {
        let (down, pos) = ui.input(|input| (input.pointer.primary_down(), input.pointer.latest_pos()));
        if down {
            if let Some(pos) = pos {
                let video = snap(studio, ruler, pos.x);
                if let Some(new_index) = studio.scene.move_key(a, i, video) {
                    timeline.dragging = Some((a.clone(), new_index));
                    timeline.selected = Some((a.clone(), new_index));
                }
            }
        } else {
            timeline.dragging = None;
        }
    }
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && pos.x >= strip.left()
    {
        let sim = snap(studio, ruler, pos.x);
        let reg = &studio.kit.registry;
        if let Some(value) = studio.scene.value_at_time(reg, a, sim)
            && let Ok(i) = studio.scene.set_key(reg, a, sim, value)
        {
            timeline.selected = Some((a.clone(), i));
        }
    } else if response.clicked() {
        timeline.selected = None;
    }
    response.on_hover_text("Double-click to add a key here");
}

/// The time at `x`, pulled onto a hit the run has reached or a whole second when one is within
/// a few pixels.
fn snap(studio: &Studio, ruler: &Ruler, x: f32) -> f64 {
    let time = ruler.time_of(x);
    let seconds = (0..=ruler.duration.ceil() as u32).map(|s| s as f64);
    let steps_per_second = studio.playback.world().steps_per_second;
    let hits =
        studio.playback.log().events().iter().filter(|e| e.kind == EventKind::Hit).map(|e| e.time(steps_per_second));
    seconds
        .chain(hits)
        .filter(|&t| (ruler.x_of(t) - x).abs() <= SNAP_PX)
        .min_by(|p, q| (ruler.x_of(*p) - x).abs().total_cmp(&(ruler.x_of(*q) - x).abs()))
        .unwrap_or(time)
}

/// A pulsing setting: its source, how much it moves and how fast the pulse dies away.
fn pulse_lane(ui: &mut Ui, studio: &mut Studio, lane: &Lane, colour: Option<Color32>, ruler: &Ruler) {
    let (_rect, strip, _response) = lane_frame(ui, studio, lane, colour, ruler);
    let a = &lane.address;
    let Some(pulse) = studio.scene.pulses.get_mut(a) else { return };
    let mut remove = false;
    let mut child = ui.new_child(
        UiBuilder::new().max_rect(strip.shrink2(egui::vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 6.0;
    child.label(RichText::new("Pulse with").small().color(style::MUTED));
    egui::ComboBox::from_id_salt(("pulse-source", &lane.label)).selected_text(pulse.source.name()).width(80.0).show_ui(
        &mut child,
        |ui| {
            for source in PulseSource::NAMED {
                if source == PulseSource::Hits && lane.physics {
                    continue;
                }
                ui.selectable_value(&mut pulse.source, source, source.name());
            }
        },
    );
    child.label(RichText::new("Amount").small().color(style::MUTED));
    match &mut pulse.amount {
        Value::Number(x) => _ = child.add(egui::DragValue::new(x).speed(0.01)),
        Value::Int(x) => _ = child.add(egui::DragValue::new(x)),
        Value::Vec2(v) => {
            child.add(egui::DragValue::new(&mut v[0]).speed(1.0).prefix("x "));
            child.add(egui::DragValue::new(&mut v[1]).speed(1.0).prefix("y "));
        }
        Value::Color(_) => _ = child.label(RichText::new("towards white").small()),
        _ => {}
    }
    child.label(RichText::new("Decay").small().color(style::MUTED));
    child.add(egui::DragValue::new(&mut pulse.decay).speed(0.01).range(0.01..=5.0).suffix(" s"));
    if child.add(egui::Button::new("×").small()).on_hover_text("Stop pulsing").clicked() {
        remove = true;
    }
    if remove {
        studio.scene.remove_pulse(a);
    }
}

/// The curve editor for the selected key, when it has a curve of its own.
fn curve_window(ui: &mut Ui, studio: &mut Studio, timeline: &mut Timeline) {
    if !timeline.curve_open {
        return;
    }
    let Some((a, i)) = timeline.selected.clone() else {
        timeline.curve_open = false;
        return;
    };
    let Some(mut ease) = studio.scene.tracks.get(&a).and_then(|t| t.keys.get(i)).map(|k| k.ease) else {
        timeline.curve_open = false;
        return;
    };
    if !matches!(ease, Ease::Curve { .. }) {
        timeline.curve_open = false;
        return;
    }
    let mut open = true;
    egui::Window::new("Own curve").collapsible(false).resizable(false).open(&mut open).show(ui.ctx(), |ui| {
        ui.label(RichText::new("The curve from this key to the next, through two control points.").small());
        if ease::curve_editor(ui, &mut ease) {
            studio.scene.set_ease(&a, i, ease);
        }
        if ui.button("Done").clicked() {
            timeline.curve_open = false;
        }
    });
    if !open {
        timeline.curve_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::ThingId;
    use eframe::egui::PointerButton;
    use egui_kittest::{Harness, kittest::Queryable};

    fn size_address() -> Address {
        Address::own(ThingId(2), "radius")
    }

    /// A studio with the ball selected, a key on its size at 2 s and one on gravity at 1 s.
    fn harness() -> Harness<'static, Studio> {
        let mut studio = Studio::headless();
        let reg = &studio.kit.registry;
        let gravity = Address { thing: ThingId::ROOT, slot: None, key: "gravity".into() };
        studio.scene.set_key(reg, &size_address(), 2.0, Value::Number(60.0)).unwrap();
        studio.scene.set_key(reg, &gravity, 1.0, Value::Vec2([0.0, 100.0])).unwrap();
        studio.selection.set(ThingId(2));
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 300.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(|ui, s: &mut Studio| show(ui, s), studio);
        h.run();
        h
    }

    fn press_drag_release(h: &mut Harness<'_, Studio>, from: Pos2, to: Pos2) {
        h.hover_at(from);
        h.event(egui::Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.event(egui::Event::PointerMoved(from + egui::vec2(3.0, 0.0)));
        h.step();
        h.event(egui::Event::PointerMoved(to));
        h.step();
        h.event(egui::Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    #[test]
    fn only_the_selected_group_is_open() {
        let h = harness();
        assert!(h.query_by_label("Ball lanes").is_some(), "the ball's group header");
        assert!(h.query_by_label("Scene settings lanes").is_some(), "the scene settings' group header");
        assert!(h.query_by_label("Ball · Size").is_some(), "the ball's lane is open");
        assert!(h.query_by_label("Key at 0:02.00 of Ball · Size").is_some());
        assert!(h.query_by_label("Scene settings · Gravity").is_none(), "folded: no lanes");
        assert!(h.query_by_label("Key at 0:01.00 of Scene settings · Gravity").is_none());
    }

    #[test]
    fn clicking_a_group_header_selects_its_thing() {
        let mut h = harness();
        h.get_by_label("Scene settings lanes").click();
        h.run();
        assert_eq!(h.state().selection.primary, Some(ThingId::ROOT));
        assert!(h.query_by_label("Key at 0:01.00 of Scene settings · Gravity").is_some(), "now open");
        assert!(h.query_by_label("Ball · Size").is_none(), "the ball's group folded");
    }

    #[test]
    fn a_key_in_a_folded_group_is_not_selected() {
        let mut h = harness();
        h.get_by_label("Key at 0:02.00 of Ball · Size").click();
        h.run();
        assert!(h.state().panels.timeline.selected.is_some());
        h.state_mut().selection.set(ThingId::ROOT);
        h.run();
        assert_eq!(h.state().panels.timeline.selected, None);
        h.key_press(Key::Delete);
        h.run();
        assert!(h.state().scene.tracks.contains_key(&size_address()), "Delete leaves a hidden key alone");
    }

    #[test]
    fn a_hit_tick_takes_its_hitters_colour() {
        let h = harness();
        let scene = &h.state().scene;
        let ball = scene.things[&ThingId(2)].colour.expect("a ball has a colour");
        assert_eq!(hit_colour(scene, ThingId(2)), style::track_color(ball));
        assert_eq!(hit_colour(scene, ThingId::ROOT), style::TEXT, "no colour: plain text");
        assert_eq!(hit_colour(scene, ThingId(99)), style::TEXT, "gone: plain text");
    }

    #[test]
    fn dragging_a_key_moves_its_time() {
        let mut h = harness();
        let rect = h.get_by_label("Key at 0:02.00 of Ball · Size").rect();
        press_drag_release(&mut h, rect.center(), rect.center() + egui::vec2(100.0, 0.0));
        let keys = &h.state().scene.tracks[&size_address()].keys;
        assert_eq!(keys.len(), 1);
        assert!(keys[0].time > 3.0 && keys[0].time < 7.0, "{}", keys[0].time);
        assert_eq!(h.state().panels.timeline.selected, Some((size_address(), 0)));
        assert!(h.state().panels.timeline.dragging.is_none());
    }

    #[test]
    fn delete_removes_the_selected_key_not_the_thing() {
        let mut h = harness();
        h.get_by_label("Key at 0:02.00 of Ball · Size").click();
        h.run();
        assert_eq!(h.state().panels.timeline.selected, Some((size_address(), 0)));
        h.key_press(Key::Delete);
        h.run();
        assert!(!h.state().scene.tracks.contains_key(&size_address()));
        assert!(h.state().scene.things.contains_key(&ThingId(2)));
        assert_eq!(h.state().panels.timeline.selected, None);
        // The gravity key is still there, in the scene settings' folded group.
        assert!(h.query_by_label("Scene settings lanes").is_some());
        assert_eq!(h.state().scene.tracks.len(), 1);
    }

    #[test]
    fn the_context_menu_sets_an_ease() {
        let mut h = harness();
        h.get_by_label("Key at 0:02.00 of Ball · Size").click_secondary();
        h.run();
        h.get_by_label("Ease out").click();
        h.run();
        assert_eq!(h.state().scene.tracks[&size_address()].keys[0].ease, Ease::EaseOut);
        h.get_by_label("Key at 0:02.00 of Ball · Size").click_secondary();
        h.run();
        h.get_by_label("Own curve…").click();
        h.run();
        assert_eq!(h.state().scene.tracks[&size_address()].keys[0].ease, ease::OWN_CURVE);
        assert!(h.state().panels.timeline.curve_open);
        assert!(h.query_by_label("Done").is_some(), "the curve editor opened");
        h.get_by_label("Done").click();
        h.run();
        assert!(!h.state().panels.timeline.curve_open);
    }

    #[test]
    fn a_double_click_on_empty_lane_space_adds_a_key() {
        let mut h = harness();
        let key = h.get_by_label("Key at 0:02.00 of Ball · Size").rect();
        // Far to the right of the key, on the same lane.
        let pos = key.center() + egui::vec2(300.0, 0.0);
        h.hover_at(pos);
        for _ in 0..2 {
            h.event(egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            });
            h.step();
            h.event(egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
        h.run();
        let keys = &h.state().scene.tracks[&size_address()].keys;
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert!(keys[1].time > 2.0);
        assert_eq!(keys[1].value, Value::Number(60.0), "the new key holds the value at that time");
    }
}
