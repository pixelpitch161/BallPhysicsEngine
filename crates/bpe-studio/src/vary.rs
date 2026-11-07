//! Varying a setting across the balls of a group (see `bpe_core::vary`): what the settings grids
//! read about it ([`VaryCx`]), what they ask for ([`VaryEdit`], turned into inspector edits by
//! [`edit`]), the two-ended slider of a varied number and the "Varies by" line under a varied row.

use bpe_core::vary::{self, Vary, VaryBy, VaryEase, VaryPart, VaryUse};
use bpe_core::{Address, Choice, Registry, Scene, SettingDef, SlotId, ThingId, Value};
use eframe::egui::{self, Response, RichText, Sense, Ui, Vec2};

use crate::inspector::Edit;
use crate::style;
use crate::widgets::{KeyMark, NumberFeel};

/// What a settings grid of one thing's settings reads about varying them across its balls.
pub struct VaryCx<'a> {
    pub scene: &'a Scene,
    pub reg: &'a Registry,
    /// The playhead, for To's keys.
    pub time: f64,
    /// Where the grid's setting `key` lives.
    pub at: &'a dyn Fn(&str) -> Address,
    /// Whether the thing has balls to vary across (see [`offered`]); a setting that varies
    /// already still shows it, and can stop, when it hasn't.
    pub offered: bool,
}

impl<'a> VaryCx<'a> {
    /// Varying for the settings of thing `id` (whose keys live at `at`). Only a thing with balls
    /// to vary across (a Balls group, or an Emitter) is offered it; any other shows the settings
    /// that vary already, so they can stop.
    pub fn of(
        scene: &'a Scene,
        reg: &'a Registry,
        time: f64,
        id: ThingId,
        at: &'a dyn Fn(&str) -> Address,
    ) -> Option<VaryCx<'a>> {
        Some(VaryCx { scene, reg, time, at, offered: offered(scene, id) })
    }

    /// Whether setting `def` may spread by Light or Shadow: a card's or a layer's setting (a
    /// thing's own settings are worked out by its sim, which has no light probe) in a scene with
    /// a Lighting card, which the lighting itself doesn't read (sensing is one-way).
    pub fn lit(&self, def: &SettingDef) -> bool {
        let a = (self.at)(def.key);
        a.slot.is_some() && bpe_kit::sensing::can_follow_light(self.scene, self.reg, &a)
    }

    /// Whether the thing has a Health card, so its settings can spread by Health.
    pub fn healthy(&self) -> bool {
        let thing = (self.at)("").thing;
        self.scene.things.get(&thing).is_some_and(|t| t.slots.contains_key(&SlotId::Health))
    }

    /// How setting `def` can vary across the balls; `None` when it can't (or the thing has no
    /// balls to vary it across, and it doesn't vary yet).
    pub fn use_of(&self, def: &SettingDef) -> Option<VaryUse> {
        let a = (self.at)(def.key);
        let use_ = self.scene.vary_use(self.reg, &a);
        if self.offered { use_ } else { self.scene.vary_at(&a).map(|_| use_.unwrap_or(VaryUse::Live)) }
    }

    /// How setting `key` varies, its To as it is at the playhead.
    pub fn now(&self, key: &str) -> Option<Vary> {
        let a = (self.at)(key);
        let mut v = self.scene.vary_at(&a)?;
        let to = Address { key: vary::to_key(&a.key), ..a };
        if let Some(value) = self.scene.value_at_time(self.reg, &to, self.time) {
            v.to = Some(value);
        }
        Some(v)
    }

    /// Whether the thing fires its balls in rounds (an Emitter), so they can spread by Round.
    pub fn rounds(&self) -> bool {
        let thing = (self.at)("").thing;
        self.scene.things.get(&thing).is_some_and(|t| t.kind == "emitter")
    }

    /// What setting `def`'s To end's keys look like.
    pub fn to_mark(&self, def: &SettingDef) -> KeyMark {
        let a = (self.at)(def.key);
        let to = Address { key: vary::to_key(&a.key), ..a };
        KeyMark {
            can_key: def.flags.keyframe,
            keyed: self.scene.keyed(&to),
            pulsed: self.scene.pulses.contains_key(&to),
        }
    }
}

/// Whether thing `id`'s settings can vary across its balls: it is a Balls group or an Emitter.
pub fn offered(scene: &Scene, id: ThingId) -> bool {
    let Some(thing) = scene.things.get(&id) else { return false };
    match thing.kind.as_str() {
        "balls" => matches!(thing.params.get("group"), Some(Value::Bool(true))),
        "emitter" => true,
        _ => false,
    }
}

/// What a varied row (or its menu) asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum VaryEdit {
    /// Vary across balls.
    Start,
    /// Stop varying.
    Stop,
    /// The To end set (dragged, typed or pasted).
    To(Value),
    /// The To end's key at the playhead, holding it as it stands.
    KeyTo,
    By(VaryBy),
    Ease(VaryEase),
}

/// The inspector edit for `v` on setting `key` (a card's in `slot`, or the thing's own).
pub(crate) fn edit(slot: Option<SlotId>, key: &str, v: VaryEdit) -> Edit {
    let part = |p: VaryPart| vary::part_key(key, p);
    let set = |k: String, value: Value| match slot {
        Some(slot) => Edit::SetSlot(slot, k, value),
        None => Edit::SetOwn(k, value),
    };
    match v {
        VaryEdit::Start => Edit::Vary(slot, key.to_owned(), true),
        VaryEdit::Stop => Edit::Vary(slot, key.to_owned(), false),
        VaryEdit::To(value) => set(part(VaryPart::To), value),
        VaryEdit::KeyTo => Edit::NewKey(slot, part(VaryPart::To)),
        VaryEdit::By(by) => set(part(VaryPart::By), Value::Choice(by.name().into())),
        VaryEdit::Ease(ease) => set(part(VaryPart::Ease), Value::Choice(ease.name().into())),
    }
}

/// `v` as a number, for the two-ended slider.
fn number(v: &Value) -> f64 {
    match v {
        Value::Number(x) => *x,
        Value::Int(x) => *x as f64,
        _ => 0.0,
    }
}

/// `x` put back into `like`'s kind of number.
fn like(like: &Value, x: f64) -> Value {
    match like {
        Value::Int(_) => Value::Int(x.round() as i64),
        _ => Value::Number(x),
    }
}

/// The two-ended slider of a varied number setting `def`, `range` its slider's span: one rail
/// with a handle for each end, the share between them filled, then a number box for each end.
/// The From box is named by the row's label `name`; the To box "<label> to". Returns the From
/// end's response, then the To end's (each marked changed when its end moved).
pub fn two_ended(
    ui: &mut Ui,
    def: &SettingDef,
    (from, to): (&mut Value, &mut Value),
    range: (f64, f64),
    feel: NumberFeel,
    unit: String,
    name: egui::Id,
) -> (Response, Response) {
    let (lo, hi) = range;
    let integer = matches!(from, Value::Int(_));
    let decimals = if integer { 0 } else { feel.decimals };
    let snap = |x: f64| {
        // On the grid while it snaps, else on the slider's steps.
        let x = match feel.snap {
            Some(s) => s.snap(x),
            None => {
                let step = if integer { 1.0 } else { feel.step };
                if step > 0.0 { (x / step).round() * step } else { x }
            }
        };
        let x = if integer { x.round() } else { x };
        x.clamp(lo.min(hi), hi.max(lo))
    };
    let (mut a, mut b) = (number(from), number(to));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Room for both number boxes at their widest.
        let widest = format!("{}{:.*}{unit}", if lo < 0.0 { "-" } else { "" }, decimals, lo.abs().max(hi.abs()));
        let font = egui::TextStyle::Button.resolve(ui.style());
        let box_w = ui.fonts_mut(|f| f.layout_no_wrap(widest, font, style::TEXT).size().x)
            + ui.spacing().button_padding.x * 2.0
            + 4.0;
        let rail_w = (ui.available_width() - 2.0 * (box_w + 4.0)).clamp(40.0, 320.0);
        let (rect, rail) = ui.allocate_exact_size(Vec2::new(rail_w, style::CONTROL_H - 6.0), Sense::click_and_drag());
        let label = format!("{} from and to", def.label);
        rail.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, label.clone()));
        let x_of = |v: f64| {
            let t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
            rect.left() + 6.0 + (rect.width() - 12.0) * t as f32
        };
        let v_of = |x: f32| lo + (hi - lo) * (((x - rect.left() - 6.0) / (rect.width() - 12.0)).clamp(0.0, 1.0) as f64);
        // Which end a drag holds, picked where it started: the nearer handle.
        let held = rail.id.with("end");
        let mut moved = (false, false);
        if let Some(p) = rail.interact_pointer_pos()
            && !crate::widgets::right_button_busy(ui)
        {
            let end = if rail.drag_started() || rail.clicked() || ui.ctx().data(|d| d.get_temp::<bool>(held)).is_none()
            {
                let to_end = (p.x - x_of(b)).abs() < (p.x - x_of(a)).abs();
                ui.ctx().data_mut(|d| d.insert_temp(held, to_end));
                to_end
            } else {
                ui.ctx().data(|d| d.get_temp::<bool>(held)).unwrap_or(false)
            };
            let v = snap(v_of(p.x));
            if end {
                moved.1 = v != b;
                b = v;
            } else {
                moved.0 = v != a;
                a = v;
            }
        }
        if rail.drag_stopped() {
            ui.ctx().data_mut(|d| d.remove::<bool>(held));
        }
        let painter = ui.painter();
        let mid = rect.center().y;
        let track = egui::Rect::from_x_y_ranges(rect.left() + 6.0..=rect.right() - 6.0, mid - 1.5..=mid + 1.5);
        painter.rect_filled(track, 1.5, style::LINE);
        let (xa, xb) = (x_of(a), x_of(b));
        let fill = egui::Rect::from_x_y_ranges(xa.min(xb)..=xa.max(xb), mid - 1.5..=mid + 1.5);
        painter.rect_filled(fill, 1.5, style::SLIDER_FILL);
        // From is a ring, To a filled knob, so which is which reads at a glance.
        painter.circle_stroke(egui::pos2(xa, mid), 5.0, egui::Stroke::new(2.0, style::TEXT_STRONG));
        painter.circle_filled(egui::pos2(xb, mid), 5.5, style::TEXT_STRONG);
        let boxed = |ui: &mut Ui, x: &mut f64| {
            let speed = if integer { feel.drag_speed.max(0.05) } else { feel.drag_speed };
            ui.add(egui::DragValue::new(x).speed(speed).fixed_decimals(decimals).suffix(unit.clone()).range(lo..=hi))
        };
        let (was_a, was_b) = (a, b);
        let mut from_box = boxed(ui, &mut a).labelled_by(name);
        let to_box = boxed(ui, &mut b);
        // A box dragged lands on the grid too; a typed number is kept as it is.
        let range = Some((lo, hi));
        crate::setting_snap::after_drag(ui, &from_box, was_a, &mut a, feel.snap, integer, range);
        crate::setting_snap::after_drag(ui, &to_box, was_b, &mut b, feel.snap, integer, range);
        let to_name = format!("{} to", def.label);
        to_box.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, to_name.clone()));
        let to_box = to_box.on_hover_text("The other end: what the last ball (or the far end of the spread) gets");
        if moved.0 {
            from_box.mark_changed();
        }
        let mut to_box = to_box;
        if moved.1 {
            to_box.mark_changed();
        }
        // While the grid snaps, what was dragged has landed already and a typed number stays exact.
        let keep = |x: f64| match feel.snap {
            Some(_) => if integer { x.round() } else { x }.clamp(lo.min(hi), hi.max(lo)),
            None => snap(x),
        };
        if from_box.changed() {
            *from = like(from, keep(a));
        }
        if to_box.changed() {
            *to = like(to, keep(b));
        }
        (from_box, to_box)
    })
    .inner
}

/// The line under a varied row: "Varies by" with the spread and easing menus. `offered` is how
/// the setting can vary (a setting read once at launch can't spread by speed or hits); Round is
/// only offered with `rounds` (balls fired in rounds), unless it is picked already; Light and
/// Shadow only with `lit` (see [`VaryCx::lit`]), Health only with `healthy` (a Health card).
pub fn varies_line(
    ui: &mut Ui,
    def: &SettingDef,
    v: &Vary,
    offered: VaryUse,
    rounds: bool,
    lit: bool,
    healthy: bool,
) -> Option<VaryEdit> {
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new("Varies by").small().color(style::MUTED));
        let by = ui.menu_button(by_words(v.by), |ui| {
            let lit = lit || v.by.reads_light();
            let healthy = healthy || v.by.reads_health();
            for b in VaryBy::offered(offered, lit, healthy)
                .filter(|b| rounds || *b != VaryBy::Round || v.by == VaryBy::Round)
            {
                if ui.add(egui::Button::selectable(b == v.by, by_words(b))).on_hover_text(by_help(b)).clicked() {
                    pick = Some(VaryEdit::By(b));
                    ui.close();
                }
            }
        });
        let by_name = format!("{} varies by", def.label);
        by.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, by_name.clone()));
        by.response.on_hover_text("What decides where between the two ends each ball's value is");
        let ease = ui.menu_button(ease_words(v.ease), |ui| {
            for e in VaryEase::NAMES.iter().filter_map(|n| VaryEase::from_name(n)) {
                if ui.add(egui::Button::selectable(e == v.ease, ease_words(e))).clicked() {
                    pick = Some(VaryEdit::Ease(e));
                    ui.close();
                }
            }
        });
        let ease_name = format!("{} easing", def.label);
        ease.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, ease_name.clone()));
        ease.response.on_hover_text("How the values are spread between the two ends");
    });
    pick
}

/// A spread's name, as the menus show it.
pub fn by_words(by: VaryBy) -> &'static str {
    match by {
        VaryBy::Number => "Number",
        VaryBy::Round => "Round",
        VaryBy::LaunchAngle => "Launch angle",
        VaryBy::Place => "Place",
        VaryBy::Random => "Random",
        VaryBy::Speed => "Speed",
        VaryBy::Hits => "Hits",
        VaryBy::Light => "Light",
        VaryBy::Shadow => "Shadow",
        VaryBy::Health => "Health",
    }
}

fn by_help(by: VaryBy) -> &'static str {
    match by {
        VaryBy::Number => "First ball to last",
        VaryBy::Round => "The round the Emitter fired each ball in, first to last",
        VaryBy::LaunchAngle => "The way each ball is launched, clockwise from the right",
        VaryBy::Place => "Across the group, left to right as it starts",
        VaryBy::Random => "A different pick for each ball, the same every time",
        VaryBy::Speed => "How fast each ball goes now: still at one end, 1500 px/s and up at the other",
        VaryBy::Hits => "How many times each ball has hit something: none to ten",
        VaryBy::Light => "How much light falls on each ball now: dark at one end, brightly lit at the other",
        VaryBy::Shadow => {
            "How much of the light each ball's shadows take now: in the open at one end, in shadow at the other"
        }
        VaryBy::Health => "How much health each ball has left: full at one end, none at the other",
    }
}

/// An easing's name, as the menu shows it.
pub fn ease_words(e: VaryEase) -> &'static str {
    match e {
        VaryEase::Even => "Even",
        VaryEase::EaseIn => "Ease in",
        VaryEase::EaseOut => "Ease out",
        VaryEase::EaseBoth => "Ease both",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_ask_becomes_the_inspector_edit_of_its_part() {
        let look = Some(SlotId::Look);
        let to = Value::Number(4.0);
        assert!(matches!(edit(look, "3/width", VaryEdit::To(to.clone())),
            Edit::SetSlot(SlotId::Look, k, v) if k == "3/width~to" && v == to));
        assert!(matches!(edit(None, "radius", VaryEdit::KeyTo), Edit::NewKey(None, k) if k == "radius~to"));
        assert!(matches!(edit(None, "radius", VaryEdit::By(VaryBy::Hits)),
            Edit::SetOwn(k, Value::Choice(c)) if k == "radius~by" && c == "hits"));
        assert!(matches!(edit(look, "3/width", VaryEdit::Ease(VaryEase::EaseBoth)),
            Edit::SetSlot(SlotId::Look, k, Value::Choice(c)) if k == "3/width~ease" && c == "ease_both"));
        assert!(matches!(edit(None, "radius", VaryEdit::Start), Edit::Vary(None, k, true) if k == "radius"));
        assert!(matches!(edit(None, "radius", VaryEdit::Stop), Edit::Vary(None, k, false) if k == "radius"));
    }

    #[test]
    fn only_groups_and_emitters_are_offered_varying() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
        let shape = bpe_kit::add_thing(&mut scene, &kit.registry, "shape", ThingId::ROOT).unwrap();
        assert!(!offered(&scene, ball));
        scene.things.get_mut(&ball).unwrap().params.set("group", Value::Bool(true));
        assert!(offered(&scene, ball));
        assert!(!offered(&scene, shape));
    }
}
