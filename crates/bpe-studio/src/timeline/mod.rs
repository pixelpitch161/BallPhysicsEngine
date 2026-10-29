//! The timeline panel: the transport, the scrub bar, and a lane for every keyed or pulsing
//! setting of the selection, with its keys as diamonds that can be moved, eased and deleted.

pub mod ease;
pub mod lanes;

use crate::app::{Studio, clock};
use crate::theme;
use crate::transport;
use crate::widgets::paint_diamond;
use bpe_core::{Address, Ease, PulseSource, Value};
use eframe::egui::{self, Align, Color32, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder};
use lanes::{Lane, lanes_for};

/// The width of the column naming each lane.
pub const LABEL_WIDTH: f32 = 230.0;
pub const LANE_HEIGHT: f32 = 22.0;
/// Keys dragged this close to a beat, a hit or a whole second snap to it.
const SNAP_PX: f32 = 6.0;
const KEY_SIZE: f32 = 6.0;

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
    ui.add_space(4.0);
    let track = transport::scrub_bar(ui, studio, LABEL_WIDTH + 8.0);
    let ruler = Ruler { track, duration: studio.duration().max(0.001) };
    let lanes = lanes_for(&studio.scene, &studio.kit, &studio.resolved, studio.selection.primary);
    let mut timeline = std::mem::take(&mut studio.panels.timeline);
    // A selection that points at a key that is gone is no selection.
    if let Some((a, i)) = &timeline.selected
        && studio.scene.tracks.get(a).is_none_or(|t| *i >= t.keys.len())
    {
        timeline.selected = None;
        timeline.curve_open = false;
    }
    if lanes.is_empty() {
        ui.add_space(6.0);
        let hint = "Click a setting's diamond in the inspector to add a keyframe at the playhead, or \
                    right-click it to pulse with the music. Keys appear here as lanes.";
        ui.label(RichText::new(hint).small().color(theme::MUTED));
    } else {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (index, lane) in lanes.iter().enumerate() {
                if lane.pulse {
                    pulse_lane(ui, studio, lane, &ruler);
                } else {
                    track_lane(ui, studio, &mut timeline, lane, index, &ruler);
                }
            }
        });
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

/// The lane's name and its strip, with the playhead drawn across it.
fn lane_frame(ui: &mut Ui, studio: &Studio, lane: &Lane, ruler: &Ruler) -> (Rect, Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), Sense::click());
    let label_rect = Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, LANE_HEIGHT));
    ui.put(label_rect, egui::Label::new(RichText::new(&lane.label).small()).truncate().selectable(false));
    let strip = Rect::from_min_max(
        Pos2::new(ruler.track.left(), rect.top() + 1.0),
        Pos2::new(ruler.track.right(), rect.bottom() - 1.0),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(strip, 3.0, theme::CARD);
    let x = ruler.x_of(studio.time);
    painter.line_segment([Pos2::new(x, strip.top()), Pos2::new(x, strip.bottom())], Stroke::new(1.0, theme::ACCENT));
    (rect, strip, response)
}

/// A keyed setting: a diamond per key. Click selects, drag moves, double-click on empty space
/// adds a key, right-click eases or deletes.
fn track_lane(ui: &mut Ui, studio: &mut Studio, timeline: &mut Timeline, lane: &Lane, index: usize, ruler: &Ruler) {
    let (rect, strip, response) = lane_frame(ui, studio, lane, ruler);
    let a = &lane.address;
    let keys: Vec<(f64, Ease)> =
        studio.scene.tracks.get(a).map(|t| t.keys.iter().map(|k| (k.time, k.ease)).collect()).unwrap_or_default();
    let warp = studio.warp.clone();
    let mut ease_pick: Option<(usize, Ease, bool)> = None;
    let mut delete: Option<usize> = None;
    for (i, (time, ease)) in keys.iter().enumerate() {
        let video = warp.video_time(*time);
        let center = Pos2::new(ruler.x_of(video), rect.center().y);
        let key_rect = Rect::from_center_size(center, egui::vec2(14.0, 14.0));
        let key = ui.interact(key_rect, ui.scope_id().with(("key", index, i)), Sense::click_and_drag());
        let name = format!("Key at {} of {}", clock(video), lane.label);
        key.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
        let selected = timeline.selected.as_ref() == Some(&(a.clone(), i));
        let color = if selected {
            Color32::WHITE
        } else if key.hovered() {
            theme::WARNING
        } else {
            theme::ACCENT_TEXT
        };
        paint_diamond(ui.painter(), center, KEY_SIZE, true, color);
        if key.clicked() || key.drag_started() {
            timeline.selected = Some((a.clone(), i));
        }
        if key.drag_started() {
            timeline.dragging = Some((a.clone(), i));
            studio.set_playing(false);
        }
        key.context_menu(|ui| {
            timeline.selected = Some((a.clone(), i));
            ui.label(RichText::new("Eases into the next key").small().color(theme::MUTED));
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
    // A drag in progress on this lane follows the pointer, snapping to beats, hits and seconds.
    if let Some((drag_a, i)) = timeline.dragging.clone()
        && drag_a == *a
    {
        let (down, pos) = ui.input(|input| (input.pointer.primary_down(), input.pointer.latest_pos()));
        if down {
            if let Some(pos) = pos {
                let video = snap(studio, ruler, pos.x);
                if let Some(new_index) = studio.scene.move_key(a, i, warp.sim_time(video)) {
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
        let sim = warp.sim_time(snap(studio, ruler, pos.x));
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

/// The video time at `x`, pulled onto a beat, a hit or a whole second when one is within
/// a few pixels.
fn snap(studio: &Studio, ruler: &Ruler, x: f32) -> f64 {
    let time = ruler.time_of(x);
    let seconds = (0..=ruler.duration.ceil() as u32).map(|s| s as f64);
    let beats = studio.panels.sound.beats.iter().map(|&b| b as f64);
    let hits = studio.panels.sound.hits.iter().map(|&h| h as f64);
    seconds
        .chain(beats)
        .chain(hits)
        .filter(|&t| (ruler.x_of(t) - x).abs() <= SNAP_PX)
        .min_by(|p, q| (ruler.x_of(*p) - x).abs().total_cmp(&(ruler.x_of(*q) - x).abs()))
        .unwrap_or(time)
}

/// A pulsing setting: its source, how much it moves and how fast the pulse dies away.
fn pulse_lane(ui: &mut Ui, studio: &mut Studio, lane: &Lane, ruler: &Ruler) {
    let (_rect, strip, _response) = lane_frame(ui, studio, lane, ruler);
    let a = &lane.address;
    let Some(pulse) = studio.scene.pulses.get_mut(a) else { return };
    let mut remove = false;
    let mut child = ui.new_child(
        UiBuilder::new().max_rect(strip.shrink2(egui::vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 6.0;
    child.label(RichText::new("Pulse with").small().color(theme::MUTED));
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
    child.label(RichText::new("Amount").small().color(theme::MUTED));
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
    child.label(RichText::new("Decay").small().color(theme::MUTED));
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
    use bpe_core::{Owner, ThingId};
    use eframe::egui::{PointerButton, accesskit::Role};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn size_address() -> Address {
        Address { owner: Owner::Thing(ThingId(2)), slot: None, key: "radius".into() }
    }

    /// A studio with the ball selected, a key on its size at 2 s and one on gravity at 1 s.
    fn harness() -> Harness<'static, Studio> {
        let mut studio = Studio::headless();
        let reg = &studio.kit.registry;
        let gravity = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "gravity".into() };
        studio.scene.set_key(reg, &size_address(), 2.0, Value::Number(60.0)).unwrap();
        studio.scene.set_key(reg, &gravity, 1.0, Value::Vec2([0.0, 100.0])).unwrap();
        studio.selection.set(ThingId(2));
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 300.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
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
    fn the_timeline_shows_lanes_for_the_selection_then_scene_defaults() {
        let h = harness();
        let labels: Vec<String> = h
            .get_all_by_role(Role::Label)
            .filter_map(|n| n.accesskit_node().label().or_else(|| n.accesskit_node().value()))
            .filter(|l| l.contains(" · ") && !l.starts_with("Drag"))
            .collect();
        assert_eq!(labels, ["Ball · Size", "Scene defaults · Gravity"]);
        assert!(h.query_by_label("Key at 0:02.00 of Ball · Size").is_some());
        assert!(h.query_by_label("Key at 0:01.00 of Scene defaults · Gravity").is_some());
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
        // The gravity lane is still there.
        assert!(h.query_by_label("Key at 0:01.00 of Scene defaults · Gravity").is_some());
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
