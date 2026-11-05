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
use bpe_sound::TempoMap;
use eframe::egui::{self, Align, Color32, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder};
use lanes::{Group, Lane, groups_for};
use std::collections::{BTreeMap, BTreeSet};

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
    /// Where each sounding shape's slots change, worked out again when the scene, the run so
    /// far or the clip's length changes.
    slot_marks: SlotMarks,
}

#[derive(Default)]
struct SlotMarks {
    made_for: Option<(u64, usize, u64)>,
    marks: BTreeMap<ThingId, Vec<(f64, usize)>>,
}

/// At most this many slot changes get a name of their own (for screen readers and tests).
const NAMED_MARKS: usize = 256;

/// Where time sits across the lanes.
struct Ruler {
    track: Rect,
    duration: f64,
    /// The bar and beat lines across the lanes, from [`beat_lines`].
    beats: Vec<BeatLine>,
}

/// A line of the bars-and-beats grid: where it is, whether it starts a bar, and the bar's number
/// when it is written on the ruler.
type BeatLine = (f32, bool, Option<u32>);

/// Lines closer than this are left out.
const BEAT_MIN_PX: f32 = 4.0;
/// Bar numbers closer than this are left out.
const BAR_NUMBER_PX: f32 = 28.0;

/// The bars-and-beats grid for `tempo` over the ruler: every beat, or only bars when beats would
/// sit closer than 4 px, then only every 2nd, 4th… bar. Bars are numbered from 1.
fn beat_lines(ruler: &Ruler, tempo: &TempoMap) -> Vec<BeatLine> {
    let beat = tempo.beat_seconds();
    let per_bar = tempo.top().max(1);
    if beat.is_nan() || beat <= 0.0 || ruler.duration <= 0.0 {
        return Vec::new();
    }
    let px = |seconds: f64| (seconds / ruler.duration) as f32 * ruler.track.width();
    let show_beats = px(beat) >= BEAT_MIN_PX;
    let bar_px = px(beat * per_bar as f64).max(1e-3);
    let mut bar_every = 1u32;
    while bar_px * (bar_every as f32) < BEAT_MIN_PX && bar_every < 1 << 20 {
        bar_every *= 2;
    }
    let mut number_every = bar_every;
    while bar_px * (number_every as f32) < BAR_NUMBER_PX && number_every < 1 << 20 {
        number_every *= 2;
    }
    let beats = (ruler.duration / beat + 1e-9).floor() as u64;
    let mut lines = Vec::new();
    for n in 0..=beats {
        let is_bar = n.is_multiple_of(per_bar as u64);
        let bar = (n / per_bar as u64) as u32;
        if is_bar && !bar.is_multiple_of(bar_every) || !is_bar && !show_beats {
            continue;
        }
        let number = (is_bar && bar.is_multiple_of(number_every)).then_some(bar + 1);
        lines.push((ruler.x_of(n as f64 * beat), is_bar, number));
    }
    lines
}

/// Draws the grid's lines across `strip`.
fn paint_beats(painter: &egui::Painter, ruler: &Ruler, strip: Rect) {
    for &(x, bar, _) in &ruler.beats {
        let ink = if bar { style::LINE } else { style::LINE_SOFT };
        painter.vline(x, strip.y_range(), Stroke::new(1.0, ink));
    }
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
    let mut ruler = Ruler { track, duration: studio.duration().max(0.001), beats: Vec::new() };
    ruler.beats = beat_lines(&ruler, &TempoMap::from_settings(&studio.settings()));
    // Bar numbers along the top of the ruler.
    let painter = ui.painter_at(track);
    for &(x, _, number) in &ruler.beats {
        if let Some(n) = number {
            painter.text(
                Pos2::new(x + 2.0, track.top()),
                egui::Align2::LEFT_TOP,
                n.to_string(),
                style::mono(9.0),
                style::FAINT,
            );
        }
    }
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
    let made_for = (studio.revision, studio.playback.log().events().len(), ruler.duration.to_bits());
    if timeline.slot_marks.made_for != Some(made_for) {
        let (log, world) = (studio.playback.log(), studio.playback.world());
        timeline.slot_marks.marks = groups
            .iter()
            .filter(|g| g.slots)
            .map(|g| {
                (g.thing, bpe_sound::voices::slot_change_times(&studio.scene, g.thing, log, world, ruler.duration))
            })
            .collect();
        // An emitter's rounds, numbered from 1 as slots are.
        for g in groups.iter().filter(|g| g.rounds) {
            let marks = bpe_kit::things::emitter::sim::round_marks(world, g.thing, ruler.duration)
                .into_iter()
                .map(|m| (m.time, m.round.saturating_sub(1) as usize))
                .collect();
            timeline.slot_marks.marks.insert(g.thing, marks);
        }
        timeline.slot_marks.made_for = Some(made_for);
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
            let marks = timeline.slot_marks.marks.get(&group.thing).map(Vec::as_slice).unwrap_or_default();
            let (clicked, colour_now) = group_header(ui, studio, group, colour, is_open, &ruler, marks);
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
    paint_beats(&painter, ruler, strip);
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
/// which opens its group. A sounding shape's slot changes are ticks along it, each with the
/// slot's number where there is room. True when clicked.
fn group_header(
    ui: &mut Ui,
    studio: &Studio,
    group: &Group,
    colour: Option<Color32>,
    open: bool,
    ruler: &Ruler,
    marks: &[(f64, usize)],
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
    let word = if group.rounds { "Round" } else { "Slot" };
    slot_ticks(ui, &painter, strip, ruler, group.thing, marks, word);
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

/// Ticks where a shape's slots change, numbered where they are far enough apart.
/// `word` names what a tick starts: "Slot" or "Round".
fn slot_ticks(
    ui: &Ui,
    painter: &egui::Painter,
    strip: Rect,
    ruler: &Ruler,
    thing: ThingId,
    marks: &[(f64, usize)],
    word: &str,
) {
    let mut last_number = f32::NEG_INFINITY;
    for (i, &(time, slot)) in marks.iter().enumerate() {
        let x = ruler.x_of(time);
        let tick = Rect::from_min_max(Pos2::new(x - 0.5, strip.top() + 1.0), Pos2::new(x + 0.5, strip.bottom() - 1.0));
        painter.rect_filled(tick, 0.0, style::TEXT_STRONG);
        if x - last_number >= 16.0 {
            painter.text(
                Pos2::new(x + 2.0, strip.top()),
                egui::Align2::LEFT_TOP,
                (slot + 1).to_string(),
                style::mono(9.0),
                style::TEXT,
            );
            last_number = x;
        }
        if i < NAMED_MARKS {
            let name = format!("{word} {} from {}", slot + 1, clock(time));
            let r = ui.interact(
                tick.expand2(egui::vec2(2.0, 0.0)),
                ui.scope_id().with(("slot mark", thing, i, x as i32)),
                Sense::hover(),
            );
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, name.clone()));
            r.on_hover_text(name);
        }
    }
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
    paint_beats(&painter, ruler, strip);
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
    // A setting that only holds (Jump to slot) has no eases; a jump's key says its slot.
    let hold = studio.scene.def_at(&studio.kit.registry, a).is_some_and(|d| d.flags.hold);
    let jump = a.slot == Some(bpe_core::SlotId::Sound) && a.key == "notes_jump";
    let slots: Vec<Option<String>> = studio
        .scene
        .tracks
        .get(a)
        .map(|t| {
            t.keys
                .iter()
                .map(|k| match k.value {
                    Value::Int(n) if jump => Some(format!("Slot {n}")),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    for (i, (time, ease)) in keys.iter().enumerate() {
        let video = *time;
        let center = Pos2::new(ruler.x_of(video), rect.center().y);
        let key_rect = Rect::from_center_size(center, egui::vec2(14.0, 14.0));
        let key = ui.interact(key_rect, ui.scope_id().with(("key", index, i)), Sense::click_and_drag());
        let slot = slots.get(i).cloned().flatten();
        let name = match &slot {
            Some(slot) => format!("Key at {} of {} · {slot}", clock(video), lane.label),
            None => format!("Key at {} of {}", clock(video), lane.label),
        };
        key.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
        let selected = timeline.selected.as_ref() == Some(&(a.clone(), i));
        let size = if key.hovered() { KEY_SIZE + 1.0 } else { KEY_SIZE };
        style::paint_diamond(ui.painter(), center, size, selected);
        if let Some(slot) = &slot {
            ui.painter().text(
                center + egui::vec2(KEY_SIZE + 3.0, 0.0),
                egui::Align2::LEFT_CENTER,
                slot,
                style::mono(9.0),
                style::TEXT,
            );
        }
        if key.clicked() || key.drag_started() {
            timeline.selected = Some((a.clone(), i));
        }
        if key.drag_started() {
            timeline.dragging = Some((a.clone(), i));
            studio.set_playing(false);
            // Each drag is its own undo step.
            studio.panels.inspector.own_step = true;
        }
        key.context_menu(|ui| {
            timeline.selected = Some((a.clone(), i));
            if hold {
                ui.label(RichText::new("Holds until the next key").small().color(style::MUTED));
            } else {
                ui.label(RichText::new("Eases into the next key").small().color(style::MUTED));
                if let Some(picked) = ease::ease_menu(ui, ease) {
                    ease_pick = Some((i, picked, matches!(picked, Ease::Curve { .. })));
                }
            }
            ui.separator();
            if ui.button("Delete key").clicked() {
                delete = Some(i);
                ui.close();
            }
        });
        let hover = match &slot {
            Some(slot) => format!("{} · jumps to {slot}\nDrag to move · right-click to delete", clock(video)),
            None => format!("{} · {}\nDrag to move · right-click for the ease", clock(video), ease.name()),
        };
        key.on_hover_text(hover);
    }
    if let Some((i, picked, own)) = ease_pick.filter(|_| !hold) {
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
            // Put down on another key, it takes that key's place.
            if let Some(new_index) = studio.scene.settle_key(a, i) {
                timeline.selected = Some((a.clone(), new_index));
            }
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
/// a few pixels, and onto a beat too while the grid snaps.
fn snap(studio: &Studio, ruler: &Ruler, x: f32) -> f64 {
    let time = ruler.time_of(x);
    let beat = TempoMap::from_settings(&studio.settings()).beat_seconds();
    let beats = (0..=(ruler.duration / beat).floor() as u64).map(move |n| n as f64 * beat).filter(|_| studio.grid.snap);
    let seconds = (0..=ruler.duration.ceil() as u32).map(|s| s as f64).chain(beats);
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
    // A setting following sound says what it follows; its source is picked from its menu.
    let follows = studio
        .scene
        .pulses
        .get(a)
        .filter(|p| matches!(p.source, PulseSource::Sound { .. }))
        .map(|p| crate::follow::words(&studio.scene, &p.source));
    let Some(pulse) = studio.scene.pulses.get_mut(a) else { return };
    let mut remove = false;
    let mut child = ui.new_child(
        UiBuilder::new().max_rect(strip.shrink2(egui::vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 6.0;
    if let Some(words) = &follows {
        child.label(RichText::new(format!("Follows {words}")).small().color(style::MUTED));
    } else {
        child.label(RichText::new("Pulse with").small().color(style::MUTED));
        egui::ComboBox::from_id_salt(("pulse-source", &lane.label))
            .selected_text(pulse.source.name())
            .width(80.0)
            .show_ui(&mut child, |ui| {
                for source in PulseSource::NAMED {
                    if source == PulseSource::Hits && lane.physics {
                        continue;
                    }
                    ui.selectable_value(&mut pulse.source, source, source.name());
                }
            });
    }
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
    if follows.is_none() {
        child.label(RichText::new("Decay").small().color(style::MUTED));
        child.add(egui::DragValue::new(&mut pulse.decay).speed(0.01).range(0.01..=5.0).suffix(" s"));
    }
    let stop = if follows.is_some() { "Stop following sound" } else { "Stop pulsing" };
    if child.add(egui::Button::new("×").small()).on_hover_text(stop).clicked() {
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
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn ruler(width: f32, duration: f64) -> Ruler {
        let track = Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 18.0));
        Ruler { track, duration, beats: Vec::new() }
    }

    #[test]
    fn bar_lines_follow_the_tempo() {
        let r = ruler(800.0, 8.0);
        let lines = beat_lines(&r, &TempoMap::new(120.0, 4, 4));
        let bars: Vec<f32> = lines.iter().filter(|l| l.1).map(|l| l.0).collect();
        assert_eq!(bars, [0.0, 200.0, 400.0, 600.0, 800.0], "a bar every 2 s, the last at the end");
        assert_eq!(lines.len(), 17, "every half-second beat");
        let numbers: Vec<u32> = lines.iter().filter_map(|l| l.2).collect();
        assert_eq!(numbers, [1, 2, 3, 4, 5]);
        // 6/8 at 90: a beat is an eighth (1/3 s), a bar 2 s.
        let lines = beat_lines(&r, &TempoMap::new(90.0, 6, 8));
        let bars: Vec<f32> = lines.iter().filter(|l| l.1).map(|l| l.0).collect();
        assert_eq!(bars, [0.0, 200.0, 400.0, 600.0, 800.0]);
    }

    #[test]
    fn dense_beats_are_thinned() {
        // 300 BPM 4/4 over a minute on 600 px: beats 2 px apart, bars 8 px.
        let r = ruler(600.0, 60.0);
        let lines = beat_lines(&r, &TempoMap::new(300.0, 4, 4));
        assert!(lines.iter().all(|l| l.1), "only bar lines");
        assert_eq!(lines.len(), 76, "every bar");
        // Over ten minutes the bars are 0.8 px apart: every 8th bar.
        let r = ruler(600.0, 600.0);
        let lines = beat_lines(&r, &TempoMap::new(300.0, 4, 4));
        let gaps: Vec<f32> = lines.windows(2).map(|w| w[1].0 - w[0].0).collect();
        assert!(gaps.iter().all(|g| *g >= BEAT_MIN_PX - 1e-3), "{gaps:?}");
        let numbers: Vec<u32> = lines.iter().filter_map(|l| l.2).take(3).collect();
        assert_eq!(numbers, [1, 65, 129], "numbers 28 px or more apart (every 64th bar here)");
    }

    #[test]
    fn the_speaker_button_mutes_without_editing_the_scene() {
        let mut h = harness();
        let before = h.state().scene.clone();
        assert!(h.state().listen.on);
        h.get_by_label("Speaker").click();
        h.run();
        assert!(!h.state().listen.on, "muted");
        assert_eq!(h.state().scene, before, "the scene is not touched");
        assert!(!h.state().history.can_undo(), "nothing to undo");
        assert!(h.query_by_label("Listening volume").is_some());
        h.get_by_label("Speaker").click();
        h.run();
        assert!(h.state().listen.on);
    }

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
    fn a_key_dragged_onto_another_replaces_it_and_is_its_own_undo_step() {
        let mut h = harness();
        {
            let s = h.state_mut();
            let kit = s.kit.clone();
            s.scene.set_key(&kit.registry, &size_address(), 4.0, Value::Number(90.0)).unwrap();
        }
        h.run();
        let from = h.get_by_label("Key at 0:02.00 of Ball · Size").rect().center();
        let to = h.get_by_label("Key at 0:04.00 of Ball · Size").rect().center();
        press_drag_release(&mut h, from, to);
        let keys = &h.state().scene.tracks[&size_address()].keys;
        assert_eq!(keys.len(), 1, "{keys:?}");
        assert_eq!((keys[0].time, &keys[0].value), (4.0, &Value::Number(60.0)), "the moved key wins");
        assert_eq!(h.state().panels.timeline.selected, Some((size_address(), 0)));
        assert!(h.state().panels.inspector.own_step, "the drag starts an undo step of its own");
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

    const SHAPE: ThingId = ThingId(1);

    /// A studio whose shape sounds with `slots` slots, the shape selected.
    fn sounding(slots: usize) -> Harness<'static, Studio> {
        let mut studio = Studio::headless();
        let reg = &studio.kit.registry;
        assert!(crate::sound_tab::write::enable(&mut studio.scene, reg, SHAPE));
        let notes = (0..slots).map(|i| bpe_core::NoteSlot { notes: vec![60 + i as u8], order_clicked: vec![] });
        let slots_at = Address::on(SHAPE, bpe_core::SlotId::Sound, "notes_slots");
        studio.scene.set_at(reg, &slots_at, Value::Slots(notes.collect())).unwrap();
        studio.selection.set(SHAPE);
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

    #[test]
    fn slot_change_ticks_on_the_timeline() {
        let h = sounding(3);
        let name = h.state().scene.things[&SHAPE].name.clone();
        assert!(h.query_by_label(&format!("{name} lanes")).is_some(), "a shape whose slots take turns has a group");
        // 120 BPM in 4/4 changes slot every bar, 2 s: slot 2 at 2 s, 3 at 4 s, 1 at 6 s.
        assert!(h.query_by_label("Slot 2 from 0:02.00").is_some());
        assert!(h.query_by_label("Slot 3 from 0:04.00").is_some());
        assert!(h.query_by_label("Slot 1 from 0:06.00").is_some());
        let one = sounding(1);
        assert!(one.query_by_label(&format!("{name} lanes")).is_none(), "one slot never changes");
    }

    #[test]
    fn two_shapes_slot_ticks_at_the_same_time_are_both_there() {
        let mut h = sounding(3);
        {
            let s = h.state_mut();
            let kit = s.kit.clone();
            crate::tree::duplicate(&mut s.scene, &kit.registry, SHAPE).expect("a copy");
            s.revision += 1;
        }
        h.run();
        assert_eq!(h.query_all_by_label("Slot 2 from 0:02.00").count(), 2, "one tick on each shape's group");
    }

    #[test]
    fn jump_keys_show_their_slot_and_hold() {
        let mut h = sounding(3);
        let jump = Address::on(SHAPE, bpe_core::SlotId::Sound, "notes_jump");
        {
            let s = h.state_mut();
            let kit = s.kit.clone();
            s.scene.set_key(&kit.registry, &jump, 1.0, Value::Int(3)).unwrap();
        }
        h.run();
        let key = h.query_all_by_label_contains("Key at 0:01.00 of").next().expect("the jump key");
        let key = key.accesskit_node().label().unwrap_or_default().to_string();
        assert!(key.ends_with("Slot 3"), "{key}");
        h.get_by_label(&key).click_secondary();
        h.run();
        assert!(h.query_by_label("Ease out").is_none(), "a jump has no ease");
        assert!(h.query_by_label("Holds until the next key").is_some());
    }
}
