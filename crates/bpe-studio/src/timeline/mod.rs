//! The timeline panel: the transport, the ruler, the Hits lane, and a group per thing with keys or
//! pulses. The selected thing's group is open, a lane per setting drawn as a clip in its track
//! colour, with keys as diamonds that can be moved, eased and deleted; the others are one bar.

pub mod ease;
pub mod lanes;

use crate::app::{Studio, clock};
use crate::style;
use crate::transport;
use bpe_core::{Address, Ease, PulseSource, ThingId, Track, Value};
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
    /// The selected keys: each by its setting and its index on the track. Only keys of the open
    /// group are ever selected.
    pub selected: BTreeSet<(Address, usize)>,
    /// When each selected key was as the timeline last drew: a key whose index now holds a key
    /// at another time (an undo, or a key added in front of it elsewhere) is no longer selected.
    picked_at: BTreeMap<(Address, usize), f64>,
    /// The selected keys being dragged.
    drag: Option<GroupDrag>,
    /// A box dragged over empty lane space to select keys: where it started, and whether it adds
    /// to the selection (Shift or Ctrl held).
    boxing: Option<(Pos2, bool)>,
    /// The curve editor is open for the selected key.
    curve_open: bool,
    /// "Paste keys at the playhead" was picked in a lane's menu.
    paste_asked: bool,
    /// Where each sounding shape's slots change, worked out again when the scene, the run so
    /// far or the clip's length changes.
    slot_marks: SlotMarks,
    /// The Hits lane's ticks, added to as hits arrive.
    hit_ticks: HitTicks,
}

/// Selected keys being dragged together (W5): their tracks as the drag began, which keys move,
/// and how far.
struct GroupDrag {
    /// When the grabbed key was as the drag began.
    grab: f64,
    before: BTreeMap<Address, Track>,
    picked: Vec<(Address, usize)>,
    /// How far they have moved so far, in seconds.
    delta: f64,
}

impl Timeline {
    /// The selected key, when exactly one is.
    pub fn single(&self) -> Option<(Address, usize)> {
        if self.selected.len() == 1 { self.selected.first().cloned() } else { None }
    }

    /// Selects this key alone.
    fn select_only(&mut self, key: (Address, usize)) {
        self.selected.clear();
        self.selected.insert(key);
    }
}

#[derive(Default)]
struct SlotMarks {
    made_for: Option<(u64, usize, u64)>,
    marks: BTreeMap<ThingId, Vec<(f64, usize)>>,
    /// When they were last worked out: while playing they are worked out again at most every
    /// [`SLOT_MARKS_EVERY`] as the run grows.
    at: Option<std::time::Instant>,
}

/// How often the slot marks follow a run that grows while playing.
const SLOT_MARKS_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

/// The Hits lane's ticks, one per pixel column and hitter: the log is read once, each hit as it
/// arrives, and read again from the start only when the run, the ruler or the clip's length
/// changes (or the run went back).
#[derive(Default)]
pub(crate) struct HitTicks {
    /// The revision, the strip's left and width, the clip's length and the steps a second.
    made_for: Option<(u64, u32, u32, u64, u32)>,
    /// How many of the log's events have been read.
    read: usize,
    pub(crate) ticks: BTreeSet<(i32, ThingId)>,
}

impl HitTicks {
    /// Brings the ticks up to `events`, read with `x_of` for a run at `revision` (`key` the rest
    /// of what they were made for).
    pub(crate) fn update(
        &mut self,
        revision: u64,
        key: (u32, u32, u64, u32),
        events: &[bpe_runtime::Event],
        x_of: impl Fn(f64) -> f32,
    ) {
        let made_for = (revision, key.0, key.1, key.2, key.3);
        if self.made_for != Some(made_for) || events.len() < self.read {
            self.made_for = Some(made_for);
            self.read = 0;
            self.ticks.clear();
        }
        for e in events[self.read..].iter().filter(|e| e.kind == EventKind::Hit) {
            self.ticks.insert((x_of(e.time(key.3)).round() as i32, e.a.thing));
        }
        self.read = events.len();
    }
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
/// sit closer than 4 px, then only every 2nd, 4th… bar. Bars are numbered from 1, the first at
/// the scene's first beat (nothing is drawn before it).
fn beat_lines(ruler: &Ruler, tempo: &TempoMap) -> Vec<BeatLine> {
    let beat = tempo.beat_seconds();
    let first = tempo.first();
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
    if first > ruler.duration {
        return Vec::new();
    }
    let beats = ((ruler.duration - first) / beat + 1e-9).floor() as u64;
    let mut lines = Vec::new();
    for n in 0..=beats {
        let is_bar = n.is_multiple_of(per_bar as u64);
        let bar = (n / per_bar as u64) as u32;
        if is_bar && !bar.is_multiple_of(bar_every) || !is_bar && !show_beats {
            continue;
        }
        let number = (is_bar && bar.is_multiple_of(number_every)).then_some(bar + 1);
        lines.push((ruler.x_of(first + n as f64 * beat), is_bar, number));
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
    if studio.bottom == crate::settings::BottomView::Rules {
        let outcome = crate::rules_panel::show(ui, studio);
        studio.separate_edit |= outcome.separate;
        if let Some(message) = outcome.message {
            studio.status = message;
        }
        return;
    }
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
    let grading = studio.mode == crate::grade_mode::EditorMode::Grade;
    let mut groups = groups_for(&studio.scene, &studio.kit);
    if grading {
        grade_lanes_only(&mut groups);
    }
    // Only the selected thing's group is open; with nothing selected, the scene settings'. In
    // Grade mode, the scene settings' grade lanes.
    let open = if grading { ThingId::ROOT } else { studio.selection.primary.unwrap_or(ThingId::ROOT) };
    let mut timeline = std::mem::take(&mut studio.panels.timeline);
    // A selected key that is gone is no longer selected, and a key in a folded group is out of
    // sight, so it isn't either (Delete would remove it unseen).
    let tracks = &studio.scene.tracks;
    let picked_at = std::mem::take(&mut timeline.picked_at);
    timeline.selected.retain(|(a, i)| {
        let Some(key) = tracks.get(a).and_then(|t| t.keys.get(*i)) else { return false };
        let same = picked_at.get(&(a.clone(), *i)).is_none_or(|t| t.to_bits() == key.time.to_bits());
        a.thing == open && same
    });
    if timeline.drag.as_ref().is_some_and(|d| d.picked.iter().any(|(a, _)| a.thing != open)) {
        timeline.drag = None;
    }
    if timeline.single().is_none() {
        timeline.curve_open = false;
    }
    let made_for = (studio.revision, studio.playback.log().events().len(), ruler.duration.to_bits());
    let marks_for = timeline.slot_marks.made_for;
    // A new run or length now; a run growing while playing at most every SLOT_MARKS_EVERY.
    let only_grew = marks_for.is_some_and(|(r, _, d)| (r, d) == (made_for.0, made_for.2));
    let followed_lately = timeline.slot_marks.at.is_some_and(|t| t.elapsed() < SLOT_MARKS_EVERY);
    if marks_for != Some(made_for) && !(only_grew && studio.playing && followed_lately) {
        timeline.slot_marks.at = Some(std::time::Instant::now());
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
    // Where each key of the open group was drawn, for the selecting box.
    let mut spots: Vec<(Address, usize, Pos2)> = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        hits_lane(ui, studio, &ruler, &mut timeline.hit_ticks);
        song_lane(ui, studio, &ruler);
        if !studio.scene.rules.is_empty() {
            rules_lane(ui, studio, &ruler);
        }
        if grading {
            ui.add_space(4.0);
            let hint = if groups.is_empty() {
                "Only grade keyframes show in Grade mode. Right-click a panel's setting and pick Create keyframe."
            } else {
                "Only grade keyframes show in Grade mode."
            };
            ui.label(RichText::new(hint).small().color(style::MUTED));
        } else if groups.is_empty() {
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
                    track_lane(ui, studio, &mut timeline, lane, index, colour, &ruler, &mut spots);
                }
                index += 1;
            }
        }
        select_box(ui, &mut timeline, &spots);
    });
    if let Some(id) = picked {
        studio.selection.set(id);
    }
    if let Some((id, colour)) = recoloured
        && let Some(thing) = studio.scene.things.get_mut(&id)
    {
        thing.colour = Some(colour);
    }
    drag_keys(ui, studio, &mut timeline, &ruler);
    // Delete removes the selected keys, and is used up so the tree keeps its thing; Escape lets
    // go of them.
    let typing = ui.ctx().text_edit_focused();
    if !timeline.selected.is_empty() && !typing && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Delete))
    {
        remove_keys(studio, &timeline.selected);
        timeline.selected.clear();
        timeline.curve_open = false;
    }
    if !timeline.selected.is_empty() && !typing && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
    {
        timeline.selected.clear();
    }
    curve_window(ui, studio, &mut timeline);
    let tracks = &studio.scene.tracks;
    timeline.picked_at = timeline
        .selected
        .iter()
        .filter_map(|(a, i)| Some(((a.clone(), *i), tracks.get(a)?.keys.get(*i)?.time)))
        .collect();
    studio.panels.timeline = timeline;
    if std::mem::take(&mut studio.panels.timeline.paste_asked) {
        paste_keys(studio);
    }
}

/// The Hits lane: a tick per hit at its time, in the colour of the thing that hit.
fn hits_lane(ui: &mut Ui, studio: &Studio, ruler: &Ruler, hit_ticks: &mut HitTicks) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), Sense::hover());
    let name = Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, LANE_HEIGHT));
    name_cell(ui, name, egui::Label::new(RichText::new("Hits").small().color(style::MUTED)).selectable(false));
    let strip = strip_of(rect, ruler);
    let painter = ui.painter_at(rect);
    paint_beats(&painter, ruler, strip);
    // One tick per pixel column and hitter is enough, however many hits land there.
    let rate = studio.playback.world().steps_per_second;
    let key = (ruler.track.left().to_bits(), ruler.track.width().to_bits(), ruler.duration.to_bits(), rate);
    hit_ticks.update(studio.revision, key, studio.playback.log().events(), |t| ruler.x_of(t));
    for &(x, thing) in &hit_ticks.ticks {
        let x = x as f32;
        let tick = Rect::from_min_max(Pos2::new(x - 1.0, strip.top() + 2.0), Pos2::new(x + 1.0, strip.bottom() - 2.0));
        painter.rect_filled(tick, 0.0, hit_colour(&studio.scene, thing));
    }
    playhead(&painter, studio, ruler, rect);
}

/// The Song lane, shown while the Mixer has a backing track: the song's waveform where it plays
/// (from Starts at), dimmed while it is off.
fn song_lane(ui: &mut Ui, studio: &Studio, ruler: &Ruler) {
    let name = crate::mixer::backing::backing_name(&studio.scene, &studio.kit);
    if name.is_empty() {
        return;
    }
    let mixer = |key: &str| {
        let a = Address::on(ThingId::ROOT, bpe_core::SlotId::Mixer, key);
        studio.scene.value_at_time(&studio.kit.registry, &a, 0.0)
    };
    let on = matches!(mixer("backing_on"), Some(Value::Bool(true)));
    let start = mixer("backing_at").as_ref().and_then(crate::mixer::number).unwrap_or(0.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), Sense::hover());
    let cell = Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, LANE_HEIGHT));
    name_cell(ui, cell, egui::Label::new(RichText::new("Song").small().color(style::MUTED)).selectable(false));
    let strip = strip_of(rect, ruler);
    let painter = ui.painter_at(rect);
    paint_beats(&painter, ruler, strip);
    let data = studio.scene.audio.get(&name).and_then(|a| bpe_sound::SampleBank::ready(&a.bytes)).and_then(Result::ok);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, format!("Song lane, {name}")));
    if let Some(data) = data {
        let peaks = data.peaks();
        let seconds = data.seconds();
        let colour = if on { style::TEXT.gamma_multiply(0.7) } else { style::FAINT };
        let mid = strip.center().y;
        let half = strip.height() * 0.45;
        let (from, to) = (ruler.x_of(start).max(strip.left()), ruler.x_of(start + seconds).min(strip.right()));
        let mut x = from.floor();
        while x < to {
            // The song's share under this pixel column.
            let (a, b) = (ruler.time_of(x) - start, ruler.time_of(x + 1.0) - start);
            let n = peaks.len();
            let i = ((a.max(0.0) / seconds * n as f64) as usize).min(n);
            let j = ((b.max(0.0) / seconds * n as f64).ceil() as usize).max(i + 1).min(n);
            let peak = peaks[i.min(j)..j].iter().fold(0.0f32, |m, v| m.max(*v));
            let h = (peak.min(1.0) * half).max(0.5);
            painter.line_segment(
                [Pos2::new(x + 0.5, mid - h), Pos2::new(x + 0.5, mid + h)],
                egui::Stroke::new(1.0, colour),
            );
            x += 1.0;
        }
    }
    response.on_hover_text(format!("{name}, from {}", clock(start)));
    playhead(&painter, studio, ruler, rect);
}

/// The Rules lane, shown while the scene has rules: a tick for every firing of a rule (its last
/// blocks acting), one per pixel column and rule. Hovering names the rule and the moment.
fn rules_lane(ui: &mut Ui, studio: &mut Studio, ruler: &Ruler) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), Sense::hover());
    let name = Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, LANE_HEIGHT));
    name_cell(ui, name, egui::Label::new(RichText::new("Rules").small().color(style::MUTED)).selectable(false));
    let strip = strip_of(rect, ruler);
    let painter = ui.painter_at(rect);
    paint_beats(&painter, ruler, strip);
    let rate = studio.playback.world().steps_per_second;
    let fired = &mut studio.panels.rules.fired;
    fired.update(studio.revision, studio.playback.log().events(), rate);
    let mut ticks: BTreeMap<(i32, usize), f64> = BTreeMap::new();
    for (i, rule) in studio.scene.rules.list.iter().enumerate() {
        // In time order, so a column's first firing is kept and the rest skipped cheaply.
        let mut last = None;
        for &t in fired.firings(rule).iter() {
            let x = ruler.x_of(t).round() as i32;
            if last != Some(x) && (strip.left() - 2.0..=strip.right() + 2.0).contains(&(x as f32)) {
                ticks.entry((x, i)).or_insert(t);
            }
            last = Some(x);
        }
    }
    for &(x, _) in ticks.keys() {
        let x = x as f32;
        let tick = Rect::from_min_max(Pos2::new(x - 1.0, strip.top() + 2.0), Pos2::new(x + 1.0, strip.bottom() - 2.0));
        painter.rect_filled(tick, 0.0, style::TEXT);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Image, true, format!("Rules lane, {} firings", ticks.len()))
    });
    if let Some(p) = response.hover_pos() {
        let near = ticks
            .iter()
            .map(|(&(x, i), &t)| ((x as f32 - p.x).abs(), i, t))
            .filter(|(d, _, _)| *d <= 4.0)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, i, t)) = near {
            let rule = &studio.scene.rules.list[i].name;
            response.on_hover_text(format!("{rule} at {}", clock(t)));
        }
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
    sense: Sense,
) -> (Rect, Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), sense);
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

/// What a key's right-click menu asked for.
enum KeyMenu {
    Ease(Ease),
    Copy,
    Repeat,
    Delete,
}

/// A keyed setting: a diamond per key. Click selects (Shift or Ctrl: adds or takes away), drag
/// moves the selected keys, a drag on empty space draws a selecting box, double-click on empty
/// space adds a key, right-click eases, copies, repeats or deletes. Each key's centre goes in
/// `spots`.
#[allow(clippy::too_many_arguments)]
fn track_lane(
    ui: &mut Ui,
    studio: &mut Studio,
    timeline: &mut Timeline,
    lane: &Lane,
    index: usize,
    colour: Option<Color32>,
    ruler: &Ruler,
    spots: &mut Vec<(Address, usize, Pos2)>,
) {
    // Dragged on empty space, a lane draws the selecting box.
    let (rect, strip, response) = lane_frame(ui, studio, lane, colour, ruler, Sense::click_and_drag());
    let a = &lane.address;
    let keys: Vec<(f64, Ease)> =
        studio.scene.tracks.get(a).map(|t| t.keys.iter().map(|k| (k.time, k.ease)).collect()).unwrap_or_default();
    let mut pick: Option<KeyMenu> = None;
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
    let adding = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    let repeat_words = repeat_words(studio, timeline);
    for (i, (time, ease)) in keys.iter().enumerate() {
        let video = *time;
        let center = Pos2::new(ruler.x_of(video), rect.center().y);
        spots.push((a.clone(), i, center));
        let key_rect = Rect::from_center_size(center, egui::vec2(14.0, 14.0));
        let key = ui.interact(key_rect, ui.scope_id().with(("key", index, i)), Sense::click_and_drag());
        let slot = slots.get(i).cloned().flatten();
        let name = match &slot {
            Some(slot) => format!("Key at {} of {} · {slot}", clock(video), lane.label),
            None => format!("Key at {} of {}", clock(video), lane.label),
        };
        let this = (a.clone(), i);
        let selected = timeline.selected.contains(&this);
        key.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, name.clone()));
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
        if key.clicked() {
            if !adding {
                timeline.select_only(this.clone());
            } else if !timeline.selected.remove(&this) {
                timeline.selected.insert(this.clone());
            }
        }
        if key.drag_started() {
            // A key that isn't selected yet is picked up alone (or joins with Shift or Ctrl).
            if !selected {
                if !adding {
                    timeline.selected.clear();
                }
                timeline.selected.insert(this.clone());
            }
            let picked: Vec<(Address, usize)> = timeline.selected.iter().cloned().collect();
            let before =
                picked.iter().filter_map(|(a, _)| Some((a.clone(), studio.scene.tracks.get(a)?.clone()))).collect();
            timeline.drag = Some(GroupDrag { grab: video, before, picked, delta: 0.0 });
            studio.set_playing(false);
            // Each drag is its own undo step.
            studio.panels.inspector.own_step = true;
        }
        key.context_menu(|ui| {
            if !timeline.selected.contains(&this) {
                timeline.select_only(this.clone());
            }
            let many = timeline.selected.len();
            if hold {
                ui.label(RichText::new("Holds until the next key").small().color(style::MUTED));
            } else {
                let words = if many > 1 { "Each eases into its next key" } else { "Eases into the next key" };
                ui.label(RichText::new(words).small().color(style::MUTED));
                if let Some(picked) = ease::ease_menu(ui, ease) {
                    pick = Some(KeyMenu::Ease(picked));
                }
            }
            ui.separator();
            let copy = if many > 1 { format!("Copy {many} keys") } else { "Copy key".to_owned() };
            let copy = ui.add(egui::Button::new(copy).shortcut_text("Ctrl+C"));
            if copy.on_hover_text("Paste them at the playhead with Ctrl+V").clicked() {
                pick = Some(KeyMenu::Copy);
                ui.close();
            }
            let help = "Copies of these keys every bar (or every few bars, for keys spread wider) to the end";
            if ui.button(&repeat_words).on_hover_text(help).clicked() {
                pick = Some(KeyMenu::Repeat);
                ui.close();
            }
            let delete = if many > 1 { format!("Delete {many} keys") } else { "Delete key".to_owned() };
            if ui.add(egui::Button::new(delete).shortcut_text("Del")).clicked() {
                pick = Some(KeyMenu::Delete);
                ui.close();
            }
        });
        let hover = match &slot {
            Some(slot) => format!("{} · jumps to {slot}\nDrag to move · right-click to delete", clock(video)),
            None => format!(
                "{} · {}\nDrag to move · Shift-click to pick several · right-click for the ease",
                clock(video),
                ease.name()
            ),
        };
        key.on_hover_text(hover);
    }
    match pick {
        None => {}
        Some(KeyMenu::Ease(picked)) => {
            let own = matches!(picked, Ease::Curve { .. });
            for (a, i) in timeline.selected.clone() {
                // Keys that only hold have no eases.
                if !studio.scene.def_at(&studio.kit.registry, &a).is_some_and(|d| d.flags.hold) {
                    studio.scene.set_ease(&a, i, picked);
                }
            }
            if own && timeline.single().is_some() {
                timeline.curve_open = true;
            }
        }
        Some(KeyMenu::Copy) => {
            copy_selected(studio, timeline);
        }
        Some(KeyMenu::Repeat) => {
            repeat_selected(studio, timeline);
        }
        Some(KeyMenu::Delete) => {
            remove_keys(studio, &timeline.selected);
            timeline.selected.clear();
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
            timeline.select_only((a.clone(), i));
        }
    } else if response.clicked() && !adding {
        timeline.selected.clear();
    }
    if response.drag_started()
        && let Some(from) = ui.input(|i| i.pointer.press_origin())
    {
        timeline.boxing = Some((from, adding));
    }
    // Right-click on empty space: paste copied keys at the playhead.
    let keys_copied = match &studio.clipboard {
        crate::things::Clipboard::Keys(clip) => Some(clip.keys.len()),
        _ => None,
    };
    let mut paste = false;
    response.context_menu(|ui| {
        let words = match keys_copied {
            Some(1) => "Paste key at the playhead".to_owned(),
            Some(n) => format!("Paste {n} keys at the playhead"),
            None => "Paste keys at the playhead".to_owned(),
        };
        let button = egui::Button::new(words).shortcut_text("Ctrl+V");
        if ui.add_enabled(keys_copied.is_some(), button).on_disabled_hover_text("Copy keys first").clicked() {
            paste = true;
            ui.close();
        }
    });
    timeline.paste_asked |= paste;
    response.on_hover_text("Double-click to add a key here · drag to select keys in a box");
}

/// The selecting box: drawn while it is dragged; let go, every key whose centre is inside it is
/// selected (with the keys already selected when it started with Shift or Ctrl).
fn select_box(ui: &Ui, timeline: &mut Timeline, spots: &[(Address, usize, Pos2)]) {
    let Some((from, adding)) = timeline.boxing else { return };
    let (down, at) = ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
    let area = Rect::from_two_pos(from, at.unwrap_or(from));
    if down {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("box")));
        painter.rect_filled(area, 0.0, style::TEXT.gamma_multiply(0.08));
        painter.rect_stroke(area, 0.0, Stroke::new(1.0, style::TEXT), egui::StrokeKind::Inside);
        return;
    }
    if !adding {
        timeline.selected.clear();
    }
    timeline.selected.extend(spots.iter().filter(|(.., p)| area.contains(*p)).map(|(a, i, _)| (a.clone(), *i)));
    timeline.boxing = None;
}

/// A drag of the selected keys in progress: they follow the pointer together (the grabbed one
/// snapping to hits, seconds and, with the grid, beats), none before the start; let go, each
/// takes the place of any key it was put down on.
fn drag_keys(ui: &Ui, studio: &mut Studio, timeline: &mut Timeline, ruler: &Ruler) {
    let Some(drag) = &mut timeline.drag else { return };
    let (down, pos) = ui.input(|input| (input.pointer.primary_down(), input.pointer.latest_pos()));
    if down && let Some(pos) = pos {
        let earliest = drag
            .picked
            .iter()
            .filter_map(|(a, i)| Some(drag.before.get(a)?.keys.get(*i)?.time))
            .fold(f64::INFINITY, f64::min);
        drag.delta = (snap(studio, ruler, pos.x) - drag.grab).max(-earliest.min(drag.grab));
    }
    let settle = !down;
    let now = studio.scene.move_keys(&drag.before, &drag.picked, drag.delta, settle);
    timeline.selected = now.into_iter().collect();
    if settle {
        timeline.drag = None;
    }
}

/// Removes the keys `keys` names (the last on each track first, so the others keep their
/// places).
fn remove_keys(studio: &mut Studio, keys: &BTreeSet<(Address, usize)>) {
    for (a, i) in keys.iter().rev() {
        studio.scene.remove_key(a, *i);
    }
}

/// The selected keys, each with its setting, as they are now, earliest first.
fn selected_keys(studio: &Studio, timeline: &Timeline) -> Vec<(Address, bpe_core::Key)> {
    let mut keys: Vec<(Address, bpe_core::Key)> = timeline
        .selected
        .iter()
        .filter_map(|(a, i)| Some((a.clone(), studio.scene.tracks.get(a)?.keys.get(*i)?.clone())))
        .collect();
    keys.sort_by(|x, y| x.1.time.total_cmp(&y.1.time));
    keys
}

/// Copies the selected keys to the clipboard, each by how long after the first it comes. False
/// with no keys selected.
fn copy_selected(studio: &mut Studio, timeline: &Timeline) -> bool {
    let keys = selected_keys(studio, timeline);
    let Some(first) = keys.first().map(|(_, k)| k.time) else { return false };
    let n = keys.len();
    let keys = keys.into_iter().map(|(a, k)| (a, k.time - first, k)).collect();
    studio.clipboard = crate::things::Clipboard::Keys(crate::things::KeyClip { keys });
    studio.status = if n == 1 { "Copied 1 key.".to_owned() } else { format!("Copied {n} keys.") };
    studio.copied_note = Some(format!("Ball Studio: {}", studio.status));
    true
}

/// Ctrl+C on the timeline: copies the selected keys. False with none selected (the things are
/// copied then).
pub fn copy_keys(studio: &mut Studio) -> bool {
    let timeline = std::mem::take(&mut studio.panels.timeline);
    let copied = copy_selected(studio, &timeline);
    studio.panels.timeline = timeline;
    copied
}

/// Ctrl+V with keys copied: each at the playhead plus how long after the first it came, with
/// its ease. Keys copied from one thing go onto the open thing's same settings when another
/// thing is open; settings it doesn't have are skipped. The pasted keys are selected, as an
/// undo step of their own. False when no key landed.
pub fn paste_keys(studio: &mut Studio) -> bool {
    let crate::things::Clipboard::Keys(clip) = &studio.clipboard else { return false };
    let clip = clip.clone();
    let open = studio.selection.primary.unwrap_or(ThingId::ROOT);
    let onto = |a: &Address| match clip.thing() {
        Some(t) if t != open => Address { thing: open, ..a.clone() },
        _ => a.clone(),
    };
    let reg = &studio.kit.registry;
    let mut landed: Vec<(Address, f64)> = Vec::new();
    for (a, offset, key) in &clip.keys {
        let a = onto(a);
        let time = studio.time + offset;
        if let Ok(i) = studio.scene.set_key(reg, &a, time, key.value.clone()) {
            studio.scene.set_ease(&a, i, key.ease);
            landed.push((a, time));
        }
    }
    let skipped = clip.keys.len() - landed.len();
    // Found again by time once all are in: a later key on the same track moves the others' places.
    let selected: BTreeSet<(Address, usize)> = landed
        .iter()
        .filter_map(|(a, t)| Some((a.clone(), studio.scene.tracks.get(a)?.key_near(*t, bpe_core::SAME_TIME)?)))
        .collect();
    let n = landed.len();
    let mut words = match n {
        0 => "No keys pasted".to_owned(),
        1 => format!("Pasted 1 key at {}", clock(studio.time)),
        n => format!("Pasted {n} keys at {}", clock(studio.time)),
    };
    if skipped > 0 {
        let name = studio.scene.things.get(&open).map(|t| t.name.clone()).unwrap_or_default();
        words.push_str(&format!(": {name} doesn't have the settings of {skipped} of them"));
    }
    words.push('.');
    studio.status = words;
    if n > 0 {
        studio.panels.timeline.selected = selected;
        studio.separate_edit = true;
    }
    n > 0
}

/// How often "Repeat" repeats the selected keys, in bars: every bar, or the fewest whole bars
/// longer than they spread over (keys a whole bar apart repeat every two).
fn repeat_bars(studio: &Studio, timeline: &Timeline) -> (f64, u32) {
    let bar = TempoMap::from_settings(&studio.settings()).bar_seconds();
    let keys = selected_keys(studio, timeline);
    let span = match (keys.first(), keys.last()) {
        (Some(a), Some(b)) => b.1.time - a.1.time,
        _ => 0.0,
    };
    // Keys spread over exactly a bar repeat every two, so the first key's copy never lands on
    // the last key.
    let bars = if bar > 0.0 && bar.is_finite() { ((span / bar + 1e-6).floor() + 1.0).min(1e6) as u32 } else { 1 };
    (bar, bars)
}

/// The Repeat item's words: "Repeat every bar", "Repeat every 2 bars".
fn repeat_words(studio: &Studio, timeline: &Timeline) -> String {
    match repeat_bars(studio, timeline).1 {
        1 => "Repeat every bar".to_owned(),
        n => format!("Repeat every {n} bars"),
    }
}

/// The most copies Repeat makes of each key.
const MOST_REPEATS: u32 = 4096;

/// Repeats the selected keys every bar (or every few bars, see [`repeat_bars`]) to the end of the
/// video, replacing keys at the same times, as an undo step of its own. Returns how many keys it
/// put down.
fn repeat_selected(studio: &mut Studio, timeline: &mut Timeline) -> usize {
    let (bar, bars) = repeat_bars(studio, timeline);
    let period = bar * f64::from(bars);
    let keys = selected_keys(studio, timeline);
    if !(period.is_finite() && period > 0.0) || keys.is_empty() {
        return 0;
    }
    let end = studio.duration() + 1e-9;
    let reg = &studio.kit.registry;
    let mut made = 0;
    for n in 1..=MOST_REPEATS {
        let shift = f64::from(n) * period;
        if keys[0].1.time + shift > end {
            break;
        }
        for (a, key) in &keys {
            let time = key.time + shift;
            if time <= end
                && let Ok(i) = studio.scene.set_key(reg, a, time, key.value.clone())
            {
                studio.scene.set_ease(a, i, key.ease);
                made += 1;
            }
        }
    }
    // The originals are found again where they are now.
    timeline.selected = keys
        .iter()
        .filter_map(|(a, k)| Some((a.clone(), studio.scene.tracks.get(a)?.key_near(k.time, bpe_core::SAME_TIME)?)))
        .collect();
    let every = if bars == 1 { "every bar".to_owned() } else { format!("every {bars} bars") };
    studio.status = format!("Repeated {} {every}: {made} keys.", if keys.len() == 1 { "1 key" } else { "the keys" });
    studio.separate_edit = true;
    made
}

/// The time at `x`, pulled onto a hit the run has reached or a whole second when one is within
/// a few pixels, and onto a beat too while the grid snaps.
fn snap(studio: &Studio, ruler: &Ruler, x: f32) -> f64 {
    let time = ruler.time_of(x);
    let tempo = TempoMap::from_settings(&studio.settings());
    let (beat, first) = (tempo.beat_seconds(), tempo.first());
    let count = ((ruler.duration - first).max(0.0) / beat).floor() as u64;
    let beats = (0..=count).map(move |n| first + n as f64 * beat).filter(|_| studio.grid.snap);
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
    let (_rect, strip, _response) = lane_frame(ui, studio, lane, colour, ruler, Sense::click());
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
    let Some((a, i)) = timeline.single() else {
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

/// Grade mode's timeline: the scene settings' grade lanes alone.
pub fn grade_lanes_only(groups: &mut Vec<lanes::Group>) {
    groups.retain(|g| g.thing == ThingId::ROOT);
    for g in groups.iter_mut() {
        g.lanes.retain(|l| l.address.slot == Some(bpe_core::SlotId::Grade));
        g.slots = false;
        g.rounds = false;
    }
    groups.retain(|g| !g.lanes.is_empty());
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
    fn hit_ticks_read_each_hit_once_and_start_over_for_another_run() {
        let hit = |step: u64, thing: u32| bpe_runtime::Event {
            step,
            kind: EventKind::Hit,
            a: bpe_runtime::Party { thing: ThingId(thing), item: 0 },
            b: None,
            point: glam::Vec2::ZERO,
            normal: glam::Vec2::Y,
            speed: 100.0,
            at: 0.0,
        };
        let events = vec![hit(240, 2), hit(480, 3), hit(720, 2)];
        let x_of = |t: f64| (t * 100.0) as f32;
        let key = (0, 0, 0, 240);
        let mut whole = HitTicks::default();
        whole.update(1, key, &events, x_of);
        let mut grown = HitTicks::default();
        for n in 1..=events.len() {
            grown.update(1, key, &events[..n], x_of);
            assert_eq!(grown.read, n, "only the new hits are read");
        }
        assert_eq!(grown.ticks, whole.ticks);
        assert_eq!(whole.ticks.len(), 3);
        // The run went back: read again from the start.
        grown.update(1, key, &events[..1], x_of);
        assert_eq!(grown.ticks.len(), 1);
        // Another run (an edit): the old ticks go.
        grown.update(2, key, &[], x_of);
        assert!(grown.ticks.is_empty());
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
    fn the_grid_starts_at_the_first_beat() {
        let r = ruler(800.0, 8.0);
        let lines = beat_lines(&r, &TempoMap::new(120.0, 4, 4).with_first(1.0));
        let bars: Vec<f32> = lines.iter().filter(|l| l.1).map(|l| l.0).collect();
        assert_eq!(bars, [100.0, 300.0, 500.0, 700.0], "bar 1 at 1 s, then every 2 s");
        assert!(lines.iter().all(|l| l.0 >= 100.0), "nothing before the first beat");
        let numbers: Vec<u32> = lines.iter().filter_map(|l| l.2).collect();
        assert_eq!(numbers, [1, 2, 3, 4]);
        assert!(beat_lines(&r, &TempoMap::new(120.0, 4, 4).with_first(9.0)).is_empty());
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
        // Tall enough for a key's whole menu.
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 600.0))
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
        assert!(!h.state().panels.timeline.selected.is_empty());
        h.state_mut().selection.set(ThingId::ROOT);
        h.run();
        assert!(h.state().panels.timeline.selected.is_empty());
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
        assert_eq!(h.state().panels.timeline.single(), Some((size_address(), 0)));
        assert!(h.state().panels.timeline.drag.is_none());
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
        assert_eq!(h.state().panels.timeline.single(), Some((size_address(), 0)));
        assert!(h.state().panels.inspector.own_step, "the drag starts an undo step of its own");
    }

    #[test]
    fn delete_removes_the_selected_key_not_the_thing() {
        let mut h = harness();
        h.get_by_label("Key at 0:02.00 of Ball · Size").click();
        h.run();
        assert_eq!(h.state().panels.timeline.single(), Some((size_address(), 0)));
        h.key_press(Key::Delete);
        h.run();
        assert!(!h.state().scene.tracks.contains_key(&size_address()));
        assert!(h.state().scene.things.contains_key(&ThingId(2)));
        assert!(h.state().panels.timeline.selected.is_empty());
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

    /// The harness with a second key on the ball's size, at 4 s.
    fn two_keys() -> Harness<'static, Studio> {
        let mut h = harness();
        {
            let s = h.state_mut();
            let kit = s.kit.clone();
            s.scene.set_key(&kit.registry, &size_address(), 4.0, Value::Number(90.0)).unwrap();
        }
        h.run();
        h
    }

    fn times(h: &Harness<'_, Studio>) -> Vec<f64> {
        h.state().scene.tracks[&size_address()].keys.iter().map(|k| k.time).collect()
    }

    #[test]
    fn shift_click_picks_several_keys_and_dragging_one_moves_them_all() {
        let mut h = two_keys();
        h.get_by_label("Key at 0:02.00 of Ball · Size").click();
        h.run();
        h.get_by_label("Key at 0:04.00 of Ball · Size").click_modifiers(Modifiers::SHIFT);
        h.run();
        assert_eq!(h.state().panels.timeline.selected.len(), 2);
        // Shift-clicking a selected key takes it out again, and back.
        h.get_by_label("Key at 0:04.00 of Ball · Size").click_modifiers(Modifiers::SHIFT);
        h.run();
        assert_eq!(h.state().panels.timeline.selected.len(), 1);
        h.get_by_label("Key at 0:04.00 of Ball · Size").click_modifiers(Modifiers::SHIFT);
        h.run();
        let from = h.get_by_label("Key at 0:02.00 of Ball · Size").rect().center();
        press_drag_release(&mut h, from, from + egui::vec2(60.0, 0.0));
        let t = times(&h);
        assert_eq!(t.len(), 2, "{t:?}");
        assert!(t[0] > 2.3, "{t:?}");
        assert!((t[1] - t[0] - 2.0).abs() < 1e-9, "they keep their distance: {t:?}");
        assert_eq!(h.state().panels.timeline.selected.len(), 2, "both stay selected");
        // A plain click picks one alone again.
        h.get_by_label(&format!("Key at {} of Ball · Size", clock(t[1]))).click();
        h.run();
        assert_eq!(h.state().panels.timeline.single(), Some((size_address(), 1)));
    }

    #[test]
    fn keys_dragged_left_stop_at_the_start() {
        let mut h = two_keys();
        h.state_mut().panels.timeline.selected = BTreeSet::from([(size_address(), 0), (size_address(), 1)]);
        h.run();
        let from = h.get_by_label("Key at 0:04.00 of Ball · Size").rect().center();
        press_drag_release(&mut h, from, from - egui::vec2(800.0, 0.0));
        assert_eq!(times(&h), [0.0, 2.0], "the earliest stops at 0 and the other keeps its distance");
    }

    #[test]
    fn a_box_dragged_over_empty_lane_space_selects_the_keys_in_it() {
        let mut h = two_keys();
        let a = h.get_by_label("Key at 0:02.00 of Ball · Size").rect().center();
        let b = h.get_by_label("Key at 0:04.00 of Ball · Size").rect().center();
        press_drag_release(&mut h, a - egui::vec2(20.0, 8.0), b + egui::vec2(20.0, 8.0));
        assert_eq!(h.state().panels.timeline.selected.len(), 2);
        assert_eq!(times(&h), [2.0, 4.0], "no key moved");
        // Escape lets go of them.
        h.key_press(Key::Escape);
        h.run();
        assert!(h.state().panels.timeline.selected.is_empty());
    }

    #[test]
    fn delete_removes_every_selected_key() {
        let mut h = two_keys();
        h.state_mut().panels.timeline.selected = BTreeSet::from([(size_address(), 0), (size_address(), 1)]);
        h.run();
        h.key_press(Key::Delete);
        h.run();
        assert!(!h.state().scene.tracks.contains_key(&size_address()));
    }

    #[test]
    fn copied_keys_paste_at_the_playhead_and_onto_another_thing() {
        let mut h = two_keys();
        h.state_mut().panels.timeline.selected = BTreeSet::from([(size_address(), 0), (size_address(), 1)]);
        h.run();
        assert!(copy_keys(h.state_mut()));
        h.state_mut().seek(10.0);
        assert!(paste_keys(h.state_mut()));
        h.run();
        assert_eq!(times(&h), [2.0, 4.0, 10.0, 12.0]);
        let s = h.state();
        assert_eq!(s.scene.tracks[&size_address()].keys[2].value, Value::Number(60.0));
        assert_eq!(s.panels.timeline.selected, BTreeSet::from([(size_address(), 2), (size_address(), 3)]));
        assert!(s.status.contains("Pasted 2 keys at 0:10.00"), "{}", s.status);
        assert!(s.separate_edit, "its own undo step");
        // Onto the shape, which has no Size: nothing lands, and the status line says so.
        let s = h.state_mut();
        s.separate_edit = false;
        s.selection.set(SHAPE);
        assert!(!paste_keys(s));
        assert!(s.status.contains("doesn't have the settings of 2 of them"), "{}", s.status);
        // A copy of the ball takes them onto its own Size.
        let kit = s.kit.clone();
        let copy = crate::tree::duplicate(&mut s.scene, &kit.registry, ThingId(2)).unwrap();
        s.selection.set(copy);
        s.seek(0.0);
        assert!(paste_keys(s));
        let mine: Vec<f64> = s.scene.tracks[&Address::own(copy, "radius")].keys.iter().map(|k| k.time).collect();
        assert_eq!(mine, [0.0, 2.0, 4.0, 10.0, 12.0], "the copy's own keys and the pasted ones (0 and 2)");
    }

    #[test]
    fn repeat_every_bar_fills_to_the_end() {
        let mut h = harness();
        h.get_by_label("Key at 0:02.00 of Ball · Size").click_secondary();
        h.run();
        // 120 BPM in 4/4: a bar is 2 s; the video is 20 s long.
        h.get_by_label("Repeat every bar").click();
        h.run();
        let t = times(&h);
        assert_eq!(t, [2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0], "{}", h.state().status);
        assert!(h.state().separate_edit);
    }

    #[test]
    fn keys_spread_over_more_than_a_bar_repeat_every_few_bars() {
        let mut h = two_keys();
        {
            let s = h.state_mut();
            let kit = s.kit.clone();
            s.scene.set_key(&kit.registry, &size_address(), 4.5, Value::Number(30.0)).unwrap();
            s.panels.timeline.selected = BTreeSet::from([(size_address(), 0), (size_address(), 2)]);
        }
        h.run();
        let t = h.state().panels.timeline.single();
        assert!(t.is_none());
        h.get_by_label("Key at 0:02.00 of Ball · Size").click_secondary();
        h.run();
        // 2 s to 4.5 s is more than a bar: every 2 bars (4 s).
        h.get_by_label("Repeat every 2 bars").click();
        h.run();
        assert_eq!(times(&h), [2.0, 4.0, 4.5, 6.0, 8.5, 10.0, 12.5, 14.0, 16.5, 18.0]);
    }

    #[test]
    fn keys_exactly_a_bar_apart_repeat_every_two_bars_keeping_the_pattern() {
        let mut h = two_keys();
        h.state_mut().panels.timeline.selected = BTreeSet::from([(size_address(), 0), (size_address(), 1)]);
        h.run();
        h.get_by_label("Key at 0:02.00 of Ball · Size").click_secondary();
        h.run();
        h.get_by_label("Repeat every 2 bars").click();
        h.run();
        let values: Vec<Value> = h.state().scene.tracks[&size_address()].keys.iter().map(|k| k.value.clone()).collect();
        assert_eq!(times(&h), [2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0]);
        assert_eq!(values[2], values[0], "the first key's copy");
        assert_eq!(values[3], values[1], "the second key's copy, not overwritten");
    }

    #[test]
    fn a_key_added_in_front_elsewhere_unselects_the_keys_it_moved() {
        let mut h = two_keys();
        h.state_mut().panels.timeline.selected = BTreeSet::from([(size_address(), 1)]);
        h.run();
        assert_eq!(h.state().panels.timeline.selected.len(), 1);
        {
            let s = h.state_mut();
            let kit = s.kit.clone();
            s.scene.set_key(&kit.registry, &size_address(), 0.5, Value::Number(10.0)).unwrap();
        }
        h.run();
        assert!(h.state().panels.timeline.selected.is_empty(), "index 1 now holds another key");
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
