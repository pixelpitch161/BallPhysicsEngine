//! The picked envelope's editor: the envelope drawn as it plays, its corners dragged to set the
//! times and the sustain, a segment dragged up or down to bend its curve, the gate as a dashed
//! line; its knobs, the Gate chips and the list of knobs it moves.

use super::TabCx;
use crate::style;
use bpe_core::{LayerId, Value, layered_key};
use bpe_dsp::envelope::bend;
use bpe_kit::sound::{GateMode, GateUnit};
use eframe::egui::{self, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2};

/// The view's size.
pub const VIEW: Vec2 = Vec2::new(240.0, 90.0);
/// How far (px) from a corner a press grabs it.
const GRAB: f32 = 9.0;
/// Curve change per pixel of a segment drag.
const CURVE_PER_PX: f32 = 1.0;

/// The envelope's settings, in ms and per cent (curves -100..100).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Env {
    pub delay: f32,
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub curves: [f32; 3],
    /// How long the gate is open (ms), or `None` until the next hit.
    pub gate: Option<f32>,
}

impl Env {
    /// Envelope `layer`'s settings as `cx` shows them.
    pub fn read(cx: &TabCx, layer: LayerId) -> Env {
        let n = |k: &str| cx.number(&layered_key(layer, k)) as f32;
        let mode: GateMode = cx.choice(&layered_key(layer, "gate_mode"));
        let unit: GateUnit = cx.choice(&layered_key(layer, "gate_unit"));
        let gate = match (mode, unit) {
            (GateMode::UntilNextHit, _) => None,
            (GateMode::Fixed, GateUnit::Ms) => Some(n("gate_ms")),
            (GateMode::Fixed, GateUnit::Beats) => Some(n("gate_beats") * beat_ms(cx)),
        };
        Env {
            delay: n("delay"),
            attack: n("attack"),
            hold: n("hold"),
            decay: n("decay"),
            sustain: n("sustain"),
            release: n("release"),
            curves: [n("attack_curve"), n("decay_curve"), n("release_curve")],
            gate,
        }
    }
}

/// One beat at the scene's tempo, in ms.
fn beat_ms(cx: &TabCx) -> f32 {
    let root = cx.scene.things.get(&bpe_core::ThingId::ROOT);
    let settings =
        root.map(|r| <bpe_kit::SceneSettings as bpe_core::Settings>::from_params(&r.params)).unwrap_or_default();
    (bpe_sound::TempoMap::from_settings(&settings).beat_seconds() * 1000.0) as f32
}

/// Where the envelope's parts sit in a view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub rect: Rect,
    /// Pixels per ms.
    pub scale: f32,
    /// The start of the attack, its top, the end of the decay, where the release starts and
    /// where it ends.
    pub start: Pos2,
    pub peak: Pos2,
    pub held: Pos2,
    pub decayed: Pos2,
    pub released_from: Pos2,
    pub end: Pos2,
    /// Where the gate closes, if it does.
    pub gate_x: Option<f32>,
}

/// How long the sustain is drawn when the gate is open until the next hit.
const OPEN_SUSTAIN_MS: f32 = 200.0;

/// Lays `env` out in `rect`: time across (the whole envelope fits), level up.
pub fn layout(env: &Env, rect: Rect) -> Layout {
    let rect = rect.shrink2(Vec2::new(6.0, 8.0));
    let before = env.delay + env.attack + env.hold + env.decay;
    let sustain_ms = match env.gate {
        Some(g) => (g - before).max(0.0).max(0.12 * (before + env.release).max(1.0)),
        None => OPEN_SUSTAIN_MS.max(0.12 * (before + env.release)),
    };
    let total = (before + sustain_ms + env.release).max(1.0);
    let scale = rect.width() / total;
    let x = |ms: f32| rect.left() + ms * scale;
    let y = |level: f32| rect.bottom() - level.clamp(0.0, 1.0) * rect.height();
    let sus = env.sustain / 100.0;
    let start = Pos2::new(x(env.delay), y(0.0));
    let peak = Pos2::new(x(env.delay + env.attack), y(1.0));
    let held = Pos2::new(x(env.delay + env.attack + env.hold), y(1.0));
    let decayed = Pos2::new(x(before), y(sus));
    let released_from = Pos2::new(x(before + sustain_ms), y(sus));
    let end = Pos2::new(x(before + sustain_ms + env.release), y(0.0));
    let gate_x = env.gate.map(x);
    Layout { rect, scale, start, peak, held, decayed, released_from, end, gate_x }
}

/// The envelope's line through a layout.
fn line(env: &Env, l: &Layout) -> Vec<Pos2> {
    let mut pts = vec![Pos2::new(l.rect.left(), l.start.y), l.start];
    let steps = 24;
    let c = env.curves.map(|c| c / 100.0);
    let sus = env.sustain / 100.0;
    let y = |level: f32| l.rect.bottom() - level.clamp(0.0, 1.0) * l.rect.height();
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        pts.push(Pos2::new(l.start.x + (l.peak.x - l.start.x) * t, y(bend(t, c[0]))));
    }
    pts.push(l.held);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        pts.push(Pos2::new(l.held.x + (l.decayed.x - l.held.x) * t, y(sus + (1.0 - sus) * (1.0 - bend(t, c[1])))));
    }
    pts.push(l.released_from);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        pts.push(Pos2::new(l.released_from.x + (l.end.x - l.released_from.x) * t, y(sus * (1.0 - bend(t, c[2])))));
    }
    pts
}

/// What a press in the view took hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    /// The attack's top: across sets Attack.
    Peak,
    /// The decay's end: across sets Decay, up and down Sustain.
    Decayed,
    /// The release's end: across sets Release.
    End,
    /// A segment's curve (0 attack, 1 decay, 2 release), with where the drag started.
    Curve(usize),
}

fn grab_at(l: &Layout, p: Pos2) -> Option<Grab> {
    let corners = [(Grab::Peak, l.peak), (Grab::Decayed, l.decayed), (Grab::End, l.end)];
    if let Some((g, _)) =
        corners.iter().filter(|(_, c)| c.distance(p) <= GRAB).min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)))
    {
        return Some(*g);
    }
    if p.x >= l.start.x && p.x < l.peak.x {
        Some(Grab::Curve(0))
    } else if p.x >= l.held.x && p.x < l.decayed.x {
        Some(Grab::Curve(1))
    } else if p.x >= l.released_from.x && p.x <= l.end.x {
        Some(Grab::Curve(2))
    } else {
        None
    }
}

/// Draws envelope `layer`'s editor.
pub fn show(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, name: &str) {
    let key = |k: &str| layered_key(layer, k);
    ui.horizontal_top(|ui| {
        view(ui, cx, layer, name);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (k, label) in [
                    ("delay", "Delay"),
                    ("attack", "Attack"),
                    ("hold", "Hold"),
                    ("decay", "Decay"),
                    ("sustain", "Sustain"),
                    ("release", "Release"),
                    ("velocity", "Velocity"),
                ] {
                    let format: &dyn Fn(f64) -> String =
                        if matches!(k, "sustain" | "velocity") { &percent } else { &ms };
                    cx.knob(ui, &key(k), label, format);
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Gate").small().color(style::MUTED));
                cx.chips(
                    ui,
                    &key("gate_mode"),
                    &[(GateMode::Fixed, "Fixed"), (GateMode::UntilNextHit, "Until next hit")],
                );
                if cx.choice::<GateMode>(&key("gate_mode")) == GateMode::Fixed {
                    ui.add_space(6.0);
                    cx.chips(ui, &key("gate_unit"), &[(GateUnit::Ms, "ms"), (GateUnit::Beats, "Beats")]);
                }
            });
            if cx.choice::<GateMode>(&key("gate_mode")) == GateMode::Fixed {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if cx.choice::<GateUnit>(&key("gate_unit")) == GateUnit::Beats {
                        cx.knob(ui, &key("gate_beats"), "Gate beats", &beats);
                    } else {
                        cx.knob(ui, &key("gate_ms"), "Gate length", &ms);
                    }
                });
            }
        });
    });
}

fn ms(v: f64) -> String {
    if v >= 1000.0 { format!("{:.2} s", v / 1000.0) } else { format!("{v:.0} ms") }
}

fn percent(v: f64) -> String {
    format!("{v:.0} %")
}

fn beats(v: f64) -> String {
    format!("{v:.2} beats")
}

/// The envelope view, dragged to change it.
fn view(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, name: &str) {
    let env = Env::read(cx, layer);
    let (rect, response) = ui.allocate_exact_size(VIEW, Sense::click_and_drag());
    super::name_it(ui, &response, &format!("{name} envelope"));
    let l = layout(&env, rect);
    let grab_id = response.id.with("grab");
    // Which ways the pointer has gone since the press (it may come back to where it started).
    let axes_id = response.id.with("axes");
    // What was pressed, not where the pointer is once the drag gets going.
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let grab = grab_at(&l, p);
        ui.data_mut(|d| {
            d.insert_temp(grab_id, (grab, env, l.scale));
            d.insert_temp(axes_id, (false, false));
        });
    }
    if response.dragged()
        && let Some((Some(grab), start, scale)) = ui.data(|d| d.get_temp::<(Option<Grab>, Env, f32)>(grab_id))
        && let (Some(now), Some(origin)) = (response.interact_pointer_pos(), ui.input(|i| i.pointer.press_origin()))
    {
        // From where the drag started, at the scale it started with, so the view rescaling under
        // the pointer doesn't run away with it.
        let d = now - origin;
        let (mx, my) = ui.data_mut(|data| {
            let a = data.get_temp_mut_or_default::<(bool, bool)>(axes_id);
            a.0 |= d.x != 0.0;
            a.1 |= d.y != 0.0;
            *a
        });
        // A setting is written only along a way the pointer has moved, and only when its value
        // changes (a sideways drag of the corner leaves Sustain, and its keys, alone).
        let set = |cx: &mut TabCx, k: &str, v: f32, moved: bool| {
            if !moved {
                return;
            }
            let key = layered_key(layer, k);
            let def = cx.def(&key);
            let (lo, hi) = def.and_then(|d| d.range).unwrap_or((0.0, 10_000.0));
            let v = (v as f64).clamp(lo, hi).round();
            if (cx.number(&key) - v).abs() > 1e-9 {
                cx.set(&key, Value::Number(v));
            }
        };
        let h = l.rect.height();
        match grab {
            Grab::Peak => set(cx, "attack", start.attack + d.x / scale, mx),
            Grab::Decayed => {
                set(cx, "decay", start.decay + d.x / scale, mx);
                set(cx, "sustain", start.sustain - d.y / h * 100.0, my);
            }
            Grab::End => set(cx, "release", start.release + d.x / scale, mx),
            Grab::Curve(i) => {
                let k = ["attack_curve", "decay_curve", "release_curve"][i];
                // Up bows the segment up: the attack rises sooner, the decay and release fall later.
                let sign = if i == 0 { 1.0 } else { -1.0 };
                set(cx, k, start.curves[i] - sign * d.y * CURVE_PER_PX, my);
            }
        }
    }
    if response.drag_stopped() {
        ui.data_mut(|d| {
            d.remove::<(Option<Grab>, Env, f32)>(grab_id);
            d.remove::<(bool, bool)>(axes_id);
        });
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, style::RADIUS, style::GROUND);
        painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, style::LINE_SOFT), egui::StrokeKind::Inside);
        let pts = line(&env, &l);
        let mut fill = pts.clone();
        fill.push(Pos2::new(l.end.x, l.rect.bottom()));
        fill.push(Pos2::new(l.rect.left(), l.rect.bottom()));
        painter.add(Shape::convex_polygon(fill, style::FAINT_MARK, Stroke::NONE));
        painter.add(Shape::line(pts, Stroke::new(1.6, style::TEXT)));
        if let Some(gx) = l.gate_x {
            let dash = Shape::dashed_line(
                &[Pos2::new(gx, rect.top() + 4.0), Pos2::new(gx, rect.bottom() - 4.0)],
                Stroke::new(1.0, style::MUTED),
                3.0,
                3.0,
            );
            painter.extend(dash);
        }
        for c in [l.peak, l.decayed, l.end] {
            painter.circle(c, 3.5, style::PANEL, Stroke::new(1.5, style::TEXT_STRONG));
        }
    }
    response.on_hover_text("Drag a corner to set its time (and the sustain), or a segment up or down to bend it");
}

#[cfg(test)]
mod tests {
    use super::super::tests::{SHAPE, sounding};
    use super::*;
    use crate::app::Studio;
    use crate::sound_tab::number;
    use bpe_core::{Address, SlotId};
    use eframe::egui::{Modifiers, PointerButton, accesskit::Role};
    use egui_kittest::{Harness, kittest::Queryable};

    fn value(h: &Harness<'_, (Studio, f32)>, key: &str) -> f64 {
        let a = Address::on(SHAPE, SlotId::Sound, &layered_key(LayerId(0), key));
        let reg = bpe_kit::registry();
        number(&h.state().0.scene.value_at_time(&reg, &a, 0.0).unwrap()).unwrap()
    }

    fn drag(h: &mut Harness<'_, (Studio, f32)>, from: Pos2, by: Vec2) {
        h.hover_at(from);
        h.event(egui::Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        for i in 1..=6 {
            h.event(egui::Event::PointerMoved(from + by * (i as f32 / 6.0)));
            h.step();
        }
        h.event(egui::Event::PointerButton {
            pos: from + by,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    /// Envelope ADSR 1's layout as the view draws it.
    fn laid_out(h: &Harness<'_, (Studio, f32)>) -> Layout {
        let rect = h.get_by_label("ADSR 1 envelope").rect();
        let s = &h.state().0;
        let choices = crate::widgets::TargetChoices::default();
        let cx = TabCx {
            scene: &s.scene,
            kit: &s.kit,
            choices: &choices,
            id: SHAPE,
            time: 0.0,
            colour: style::TEXT,
            notes_clip: None,
            hot: None,
            edits: Vec::new(),
        };
        layout(&Env::read(&cx, LayerId(0)), rect)
    }

    #[test]
    fn dragging_the_attack_corner_sets_attack() {
        let mut h = sounding();
        let before = value(&h, "attack");
        let l = laid_out(&h);
        drag(&mut h, l.peak, Vec2::new(30.0, 0.0));
        let after = value(&h, "attack");
        assert!(after > before + 10.0, "Attack went from {before} to {after}");
    }

    #[test]
    fn dragging_a_segment_bends_its_curve() {
        let mut h = sounding();
        let before = value(&h, "decay_curve");
        let l = laid_out(&h);
        let mid = Pos2::new((l.held.x + l.decayed.x) / 2.0, (l.held.y + l.decayed.y) / 2.0);
        drag(&mut h, mid, Vec2::new(0.0, -20.0));
        let after = value(&h, "decay_curve");
        assert!(after < before - 5.0, "dragging the decay up bows it up: {before} to {after}");
    }

    #[test]
    fn gate_beats_switch_shows_beats() {
        let mut h = sounding();
        assert!(h.query_by_role_and_label(Role::Slider, "Gate length").is_some());
        // The envelope's Beats (the Notes panel's Slots change has one too, further down).
        let gate = h.get_by_role_and_label(Role::Slider, "Gate length").rect();
        let beats = h.query_all_by_label("Beats").min_by_key(|n| (n.rect().center().y - gate.center().y).abs() as i64);
        beats.unwrap().click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Slider, "Gate beats").is_some());
        assert!(h.query_by_role_and_label(Role::Slider, "Gate length").is_none());
        h.get_by_label("Until next hit").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Slider, "Gate beats").is_none());
    }

    #[test]
    fn the_modulates_list_shows_osc_a_gain() {
        let h = sounding();
        assert!(h.query_by_label("ADSR 1 modulates").is_some());
        assert!(h.query_by_label("Osc A · Gain").is_some());
        assert!(h.query_by_label("-inf dB → +0.0 dB").is_some());
    }
}
