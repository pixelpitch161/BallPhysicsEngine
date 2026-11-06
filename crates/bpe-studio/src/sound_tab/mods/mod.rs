//! The modulators: the row of tiles (ADSR, Distance, LFO, Hit and Light, up to eight of each), the
//! picked one's editor with the knobs it moves, and dragging a tile's handle onto any knob to
//! modulate it. Knobs show each route reaching them as a ring in the shape's colour.

pub mod distance;
pub mod drag;
pub mod hit;
pub mod lfo;
pub mod lfo_view;
pub mod light;
pub mod routes;
pub mod row;

use super::{SoundTab, TabCx, TabEdit, adsr_view};
use crate::style::{self, ModRing};
use bpe_core::{LayerId, ModRoute, SlotId};
use bpe_kit::sound::{ADSR, DISTANCE, HIT, LFO, LIGHT_MOD, MOD_KINDS};
use bpe_sound::Meters;
use bpe_sound::modmatrix::{Combine, is_level_key, route_ends};
use eframe::egui::{RichText, Ui};

pub use drag::ModDrag;

/// One modulator of the card: its layer, kind, name and whether it is on.
#[derive(Clone, Debug, PartialEq)]
pub struct ModInfo {
    pub id: LayerId,
    pub kind: &'static str,
    pub name: String,
    pub on: bool,
}

/// The card's modulators in row order: the ADSRs, then the Distances, LFOs, Hits and Lights, each in
/// card order.
pub fn list(cx: &TabCx) -> Vec<ModInfo> {
    let Some(card) = cx.scene.card(cx.id, SlotId::Sound) else { return Vec::new() };
    MOD_KINDS
        .iter()
        .flat_map(|kind| {
            card.layers.iter().filter(move |l| l.kind == *kind).map(move |l| ModInfo {
                id: l.id,
                kind,
                name: l.name.clone(),
                on: l.enabled,
            })
        })
        .collect()
}

/// Modulator `layer`'s name (its id when it has gone).
pub fn name_of(cx: &TabCx, layer: LayerId) -> String {
    cx.scene
        .card(cx.id, SlotId::Sound)
        .and_then(|c| c.layer(layer))
        .map_or_else(|| format!("Layer {}", layer.0), |l| l.name.clone())
}

/// The ring a route draws on its knob, which sits at `base` over `range` (turning evenly in
/// ratios when `log`), the route moving it the `combine` way: from the knob's own side of the
/// range to the end a drag moves.
pub fn ring(r: &ModRoute, base: f64, range: (f64, f64), log: bool, combine: Combine, lit: bool) -> ModRing {
    let (at0, at1) = route_ends(r, base, range, log, combine);
    // The end away from the knob: the modulator at 1 when adding (at 0 inverted), at 0 when
    // scaling (at 1 inverted).
    let far_is_one = match combine {
        Combine::Add => !r.invert,
        Combine::Scale => r.invert,
    };
    let (from, to) = if far_is_one { (at0, at1) } else { (at1, at0) };
    ModRing { from, to, lit }
}

/// Where `v` sits along a knob over `range`, 0 at the bottom and 1 at the top (evenly in ratios
/// when `log`), as the knob draws it.
fn turn(v: f64, range: (f64, f64), log: bool) -> f64 {
    let (lo, hi) = range;
    if hi <= lo {
        return 0.0;
    }
    let t = if log && lo > 0.0 { (v.max(lo) / lo).ln() / (hi / lo).ln() } else { (v - lo) / (hi - lo) };
    if t.is_finite() { t } else { 0.0 }
}

/// The amount that puts route `r`'s far ring end (see [`ring`]) at `to`, the knob sitting at
/// `base` over `range`: an adding route's amount is how far round the knob that is, as in
/// Serum. Keeps the amount's sign for scaling routes.
pub fn amount_for(r: &ModRoute, base: f64, range: (f64, f64), log: bool, combine: Combine, to: f64) -> f32 {
    let a = match combine {
        Combine::Add => turn(to, range, log) - turn(base, range, log),
        Combine::Scale => {
            let a = if to >= base || to <= range.0 {
                if to >= base { 0.0 } else { 1.0 }
            } else {
                1.0 - 10f64.powf((to - base) / 20.0)
            };
            a * if r.amount < 0.0 { -1.0 } else { 1.0 }
        }
    };
    (a.clamp(-1.0, 1.0) * 1000.0).round() as f32 / 1000.0
}

/// The Amount a new route from `layer` to the knob `key` starts at: half the knob's travel, or a
/// tenth (about 7 dB) when a modulator other than an envelope moves a level, so dropping an LFO
/// on a Gain is a tremolo, not a jump to the top.
pub fn starting_amount(cx: &TabCx, layer: LayerId, key: &str) -> f32 {
    if !cx.is_envelope(layer) && is_level_key(key) { 0.1 } else { 0.5 }
}

/// Adds a route from `layer` to the knob `key` at its [`starting_amount`] (turning the modulator
/// on when it is off), as an undo step of its own.
pub fn add_route(cx: &mut TabCx, layer: LayerId, key: &str) {
    let mut routes = cx.routes();
    let amount = starting_amount(cx, layer, key);
    routes.push(ModRoute { source: layer, target: key.into(), amount, bipolar: false, invert: false });
    cx.set_routes(routes);
    let off = cx.scene.card(cx.id, SlotId::Sound).and_then(|c| c.layer(layer)).is_some_and(|l| !l.enabled);
    if off {
        cx.edits.push(TabEdit::ModOn(layer, true));
    }
    cx.edits.push(TabEdit::OwnStep);
}

/// Draws the row and the picked modulator's editor under it.
pub fn show(ui: &mut Ui, cx: &mut TabCx, tab: &mut SoundTab, meters: Option<&Meters>) {
    let Some(picked) = row::show(ui, cx, tab) else { return };
    let Some(m) = list(cx).into_iter().find(|m| m.id == picked) else { return };
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let mut on = m.on;
        let r = style::switch(ui, &mut on);
        super::name_it(ui, &r, &format!("{} on", m.name));
        drag::refuse(&r);
        if r.changed() {
            cx.edits.push(TabEdit::ModOn(m.id, on));
            cx.edits.push(TabEdit::OwnStep);
        }
        ui.label(RichText::new(&m.name).font(style::semibold(13.0)));
        if !m.on {
            ui.label(RichText::new("Off").small().color(style::FAINT));
        }
    });
    ui.add_space(4.0);
    match m.kind {
        ADSR => adsr_view::show(ui, cx, m.id, &m.name),
        DISTANCE => distance::show(ui, cx, m.id, meters),
        LFO => lfo::show(ui, cx, tab, m.id, meters),
        HIT => hit::show(ui, cx, m.id, meters),
        LIGHT_MOD => light::show(ui, cx, m.id, meters),
        _ => {}
    }
    routes::show(ui, cx, m.id, &m.name);
    drag::ghost(ui, cx);
}

#[cfg(test)]
mod tests {
    use super::super::tests::{BALL, SHAPE, sounding};
    use super::*;
    use crate::app::Studio;
    use bpe_core::{Address, PulseSource, ROUTES_KEY, SoundSource, ThingId, Value, layered_key};
    use eframe::egui::{self, Modifiers, PointerButton, Pos2, accesskit::Role};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    type H = Harness<'static, (Studio, f32)>;

    fn r(target: &str, amount: f32, bipolar: bool, invert: bool) -> ModRoute {
        ModRoute { source: LayerId(4), target: target.into(), amount, bipolar, invert }
    }

    #[test]
    fn a_ring_runs_from_the_knob_to_the_end_a_drag_moves() {
        let range = (-100.0, 100.0);
        let add = |route: &ModRoute, base: f64| ring(route, base, range, false, Combine::Add, false);
        assert_eq!(add(&r("a_cents", 0.25, false, false), 0.0), ModRing { from: 0.0, to: 50.0, lit: false });
        assert_eq!(add(&r("a_cents", 0.25, false, true), 0.0).to, 50.0, "inverted keeps its far end");
        let gain = ring(&r("a_gain", 1.0, false, false), 0.0, (-60.0, 12.0), false, Combine::Scale, true);
        assert_eq!((gain.from, gain.to), (0.0, -60.0), "an envelope's far end on a gain is silence");
        let lfo = ring(&r("a_gain", 0.25, false, false), -30.0, (-60.0, 12.0), false, Combine::Add, false);
        assert_eq!((lfo.from, lfo.to), (-30.0, -12.0), "an LFO on a gain moves it like any knob");
    }

    #[test]
    fn dragging_the_end_sets_the_amount_that_puts_it_there() {
        let range = (-100.0, 100.0);
        for route in [r("a_cents", 0.25, false, false), r("a_cents", -0.4, true, false), r("a_cents", 0.3, false, true)]
        {
            let end = ring(&route, 10.0, range, false, Combine::Add, false).to;
            assert!(
                (amount_for(&route, 10.0, range, false, Combine::Add, end) - route.amount).abs() < 1e-3,
                "{route:?}"
            );
        }
        let hz = (20.0, 20_000.0);
        for route in [r("3/cutoff", 0.25, false, false), r("3/cutoff", -0.3, true, false)] {
            let end = ring(&route, 400.0, hz, true, Combine::Add, false).to;
            assert!((amount_for(&route, 400.0, hz, true, Combine::Add, end) - route.amount).abs() < 1e-3, "{route:?}");
        }
        let gain = r("a_gain", 0.5, false, false);
        let end = ring(&gain, 0.0, (-60.0, 12.0), false, Combine::Scale, false).to;
        assert!((amount_for(&gain, 0.0, (-60.0, 12.0), false, Combine::Scale, end) - 0.5).abs() < 1e-3);
        assert_eq!(
            amount_for(&gain, 0.0, (-60.0, 12.0), false, Combine::Scale, -60.0),
            1.0,
            "the bottom is silence: full amount"
        );
    }

    #[test]
    fn a_ring_keeps_its_size_as_the_knob_turns() {
        // On a frequency knob drawn in ratios the ring is the same arc wherever the knob sits.
        let hz = (20.0, 20_000.0);
        let route = r("3/cutoff", 0.2, false, false);
        for base in [100.0, 400.0, 1_000.0, 3_000.0] {
            let ring = ring(&route, base, hz, true, Combine::Add, false);
            let arc = turn(ring.to, hz, true) - turn(ring.from, hz, true);
            assert!((arc - 0.2).abs() < 1e-4, "at {base} Hz the arc is {arc}");
        }
    }

    fn card(h: &H) -> bpe_core::Slot {
        h.state().0.scene.card(SHAPE, SlotId::Sound).unwrap().clone()
    }

    fn layer(h: &H, name: &str) -> LayerId {
        card(h).layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no {name}")).id
    }

    fn routes(h: &H) -> Vec<ModRoute> {
        match card(h).params.get(ROUTES_KEY) {
            Some(Value::Routes(r)) => r.clone(),
            _ => Vec::new(),
        }
    }

    fn set_routes(h: &mut H, routes: Vec<ModRoute>) {
        let studio = &mut h.state_mut().0;
        studio.scene.card_mut(SHAPE, SlotId::Sound).unwrap().params.set(ROUTES_KEY, Value::Routes(routes));
        h.run();
    }

    fn centre(h: &H, role: Role, label: &str) -> Pos2 {
        h.query_all_by_role_and_label(role, label).next().unwrap_or_else(|| panic!("no {label}")).rect().center()
    }

    fn press(h: &mut H, at: Pos2) {
        h.hover_at(at);
        h.run();
        h.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }

    fn move_to(h: &mut H, from: Pos2, to: Pos2) {
        for k in 1..=6 {
            h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
            h.step();
        }
    }

    fn release(h: &mut H, at: Pos2) {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    fn drag(h: &mut H, from: Pos2, to: Pos2) {
        press(h, from);
        move_to(h, from, to);
        release(h, to);
    }

    /// Osc A's Cutoff: a filter on Osc A, its strip open.
    fn with_filter(h: &mut H) -> LayerId {
        let reg = bpe_kit::registry();
        let studio = &mut h.state_mut().0;
        let f = studio.scene.add_layer_in(&reg, SHAPE, SlotId::Sound, bpe_kit::sound::FILTER, Some(0)).unwrap();
        studio.panels.inspector.sound.open_strips.insert((SHAPE, f));
        h.run();
        f
    }

    #[test]
    fn the_row_shows_six_tiles_and_a_plus() {
        let h = sounding();
        for name in ["ADSR 1", "ADSR 2", "Distance 1", "Distance 2", "LFO 1", "LFO 2"] {
            assert!(h.query_by_role_and_label(Role::Button, name).is_some(), "{name}");
            assert!(h.query_by_label(&format!("Drag {name}")).is_some(), "{name}'s handle");
        }
        assert!(h.query_by_label("Add modulator").is_some());
        let on: Vec<_> = card(&h).layers.iter().filter(|l| l.enabled).map(|l| l.name.clone()).collect();
        assert_eq!(on, ["ADSR 1"]);
    }

    #[test]
    fn dropping_a_handle_on_cutoff_adds_a_route() {
        let mut h = sounding();
        let f = with_filter(&mut h);
        let lfo = layer(&h, "LFO 1");
        let from = centre(&h, Role::Unknown, "Drag LFO 1");
        let to = centre(&h, Role::Slider, "Cutoff");
        drag(&mut h, from, to);
        let cutoff = layered_key(f, "cutoff");
        let added: Vec<_> = routes(&h).into_iter().filter(|r| r.source == lfo).collect();
        assert_eq!(added.len(), 1, "{added:?}");
        assert_eq!((added[0].target.as_str(), added[0].amount), (cutoff.as_str(), 0.5));
        assert!(card(&h).layer(lfo).unwrap().enabled, "dropping turns the LFO on");
    }

    #[test]
    fn dropping_on_its_own_knob_is_refused() {
        let mut h = sounding();
        h.get_by_role_and_label(Role::Button, "LFO 1").click();
        h.run();
        let before = routes(&h);
        let from = centre(&h, Role::Unknown, "Drag LFO 1");
        let to = centre(&h, Role::Slider, "Rate");
        press(&mut h, from);
        move_to(&mut h, from, to);
        h.step();
        assert_eq!(drag::current_note(&h.ctx).as_deref(), Some(drag::CANT), "the hover note");
        release(&mut h, to);
        assert_eq!(routes(&h), before, "no route to its own rate");
        // Nor onto a switch.
        let to = centre(&h, Role::CheckBox, "LFO 1 on");
        press(&mut h, from);
        move_to(&mut h, from, to);
        assert_eq!(drag::current_note(&h.ctx).as_deref(), Some(drag::CANT), "the hover note on a switch");
        release(&mut h, to);
        assert_eq!(routes(&h), before);
    }

    #[test]
    fn modulate_by_menu_adds_a_route() {
        let mut h = sounding();
        let lfo = layer(&h, "LFO 2");
        h.query_all_by_role_and_label(Role::Slider, "Cents").next().unwrap().click_secondary();
        h.run();
        h.get_by_label("Modulate by").click();
        h.run();
        // The menu's LFO 2, not its tile.
        h.query_all_by_label("LFO 2").find(|n| n.rect().height() < 40.0).unwrap().click();
        h.run();
        assert!(
            routes(&h).iter().any(|r| r.source == lfo && r.target == "a_cents" && r.amount == 0.5),
            "{:?}",
            routes(&h)
        );
    }

    /// The screen point at turn `t` of the dial centred at `c`, `radius` out.
    fn on_dial(c: Pos2, t: f64, radius: f32) -> Pos2 {
        let a = (135.0 + 270.0 * t as f32).to_radians();
        c + egui::vec2(a.cos(), a.sin()) * radius
    }

    /// Osc A's Cents moved by LFO 1 at a quarter: its ring runs from 0 to a quarter of the range.
    fn ring_on_cents(h: &mut H) -> (LayerId, (f64, f64)) {
        let lfo = layer(h, "LFO 1");
        let reg = bpe_kit::registry();
        let def = h.state().0.scene.def_at(&reg, &Address::on(SHAPE, SlotId::Sound, "a_cents")).unwrap();
        let mut all = routes(h);
        all.push(ModRoute { source: lfo, target: "a_cents".into(), amount: 0.25, bipolar: false, invert: false });
        set_routes(h, all);
        (lfo, def.range.unwrap())
    }

    fn ring_drag(h: &mut H, alt: bool) -> ModRoute {
        let (lfo, (lo, hi)) = ring_on_cents(h);
        let c = centre(h, Role::Slider, "Cents");
        let turn = |v: f64| (v - lo) / (hi - lo);
        let end = on_dial(c, turn(0.25 * (hi - lo)), 18.5);
        let to = on_dial(c, turn(0.375 * (hi - lo)), 18.5);
        if alt {
            h.event(egui::Event::ModifiersChanged(Modifiers::ALT));
        }
        drag(h, end, to);
        let before_cents = card(h).params.get("a_cents").cloned();
        assert!(before_cents.is_none() || before_cents == Some(Value::Number(0.0)), "the knob itself didn't turn");
        routes(h).into_iter().find(|r| r.source == lfo && r.target == "a_cents").unwrap()
    }

    #[test]
    fn dragging_a_ring_end_sets_the_amount() {
        let mut h = sounding();
        let r = ring_drag(&mut h, false);
        assert!((r.amount - 0.375).abs() < 0.02, "{r:?}");
        assert!(!r.bipolar);
    }

    #[test]
    fn alt_drag_makes_it_bipolar() {
        let mut h = sounding();
        let r = ring_drag(&mut h, true);
        assert!(r.bipolar, "{r:?}");
        assert!((r.amount - 0.375).abs() < 0.02, "{r:?}");
    }

    #[test]
    fn plus_adds_a_hit_tile_and_stops_at_eight() {
        let mut h = sounding();
        for n in 1..=8 {
            h.get_by_label("Add modulator").click();
            h.run();
            h.get_by_label("Hit").click();
            h.run();
            assert!(h.query_by_role_and_label(Role::Button, &format!("Hit {n}")).is_some(), "Hit {n}");
        }
        h.get_by_label("Add modulator").click();
        h.run();
        assert!(h.get_by_label("Hit").accesskit_node().is_disabled(), "eight is the most");
        assert!(!h.get_by_label("LFO").accesskit_node().is_disabled());
    }

    #[test]
    fn hovering_a_tile_highlights_its_rings() {
        let mut h = sounding();
        let gain = |h: &H| {
            let node = h.query_all_by_role_and_label(Role::Slider, "Gain").next().unwrap();
            node.accesskit_node().description().unwrap_or_default().to_owned()
        };
        assert_eq!(gain(&h), "Modulated by ADSR 1");
        h.hover_at(centre(&h, Role::Button, "ADSR 1"));
        h.run();
        assert_eq!(gain(&h), "Modulated by ADSR 1 (lit)");
        h.hover_at(centre(&h, Role::Button, "LFO 2"));
        h.run();
        assert_eq!(gain(&h), "Modulated by ADSR 1", "another tile lights only its own");
    }

    #[test]
    fn turning_a_distance_on_routes_both_gains() {
        let mut h = sounding();
        h.get_by_role_and_label(Role::Button, "Distance 1").click();
        h.run();
        h.get_by_role_and_label(Role::CheckBox, "Distance 1 on").click();
        h.run();
        let d = layer(&h, "Distance 1");
        let to: Vec<_> = routes(&h).into_iter().filter(|r| r.source == d).map(|r| r.target).collect();
        assert_eq!(to, ["a_gain", "b_gain"]);
        assert_eq!(card(&h).params.get(&layered_key(d, "far")), Some(&Value::Number(200.0)));
    }

    #[test]
    fn deleting_a_modulator_and_undo_bring_back_its_routes_and_follows() {
        let mut h = sounding();
        let lfo = layer(&h, "LFO 1");
        let mut all = routes(&h);
        all.push(ModRoute { source: lfo, target: "a_cents".into(), amount: 0.5, bipolar: false, invert: false });
        set_routes(&mut h, all);
        let a = Address { thing: BALL, slot: None, key: "radius".into() };
        let follows = PulseSource::Sound { shape: SHAPE, source: SoundSource::Modulator(lfo) };
        h.state_mut().0.scene.pulses.insert(a.clone(), bpe_core::Pulse { source: follows, ..Default::default() });
        h.run();
        let before = h.state().0.scene.clone();
        h.get_by_role_and_label(Role::Button, "LFO 1").click_secondary();
        h.run();
        h.get_by_label("Delete").click();
        h.run();
        let scene = &h.state().0.scene;
        assert!(scene.card(SHAPE, SlotId::Sound).unwrap().layer(lfo).is_none());
        assert!(routes(&h).iter().all(|r| r.source != lfo));
        assert!(!scene.pulses.contains_key(&a), "the follow went too");
        assert!(h.state_mut().0.undo());
        h.run();
        assert_eq!(h.state().0.scene, before, "one undo brings it all back");
        let _ = ThingId::ROOT;
    }
    #[test]
    fn a_setting_following_a_modulator_shows_in_its_list() {
        let mut h = sounding();
        let lfo = layer(&h, "LFO 1");
        let a = Address { thing: BALL, slot: None, key: "radius".into() };
        let follows = PulseSource::Sound { shape: SHAPE, source: SoundSource::Modulator(lfo) };
        h.state_mut().0.scene.pulses.insert(a.clone(), bpe_core::Pulse { source: follows, ..Default::default() });
        h.get_by_role_and_label(Role::Button, "LFO 1").click();
        h.run();
        let ball = h.state().0.scene.things[&BALL].name.clone();
        let words = format!("{ball} · Size");
        assert!(h.query_by_label(&words).is_some(), "the follower is listed by its thing's name");
        h.get_by_label(&format!("Stop {words} following")).click();
        h.run();
        assert!(!h.state().0.scene.pulses.contains_key(&a));
    }

    /// LFO 1's drawn shape, picked and on screen.
    fn lfo_shape(h: &mut H) -> (LayerId, egui::Rect) {
        let lfo = layer(h, "LFO 1");
        h.get_by_role_and_label(Role::Button, "LFO 1").click();
        h.run();
        let node = h.get_by_label("Drawn shape");
        node.scroll_to_me();
        h.run();
        (lfo, h.get_by_label("Drawn shape").rect())
    }

    fn curve_of(h: &H, lfo: LayerId) -> Vec<bpe_core::CurvePoint> {
        match card(h).params.get(&layered_key(lfo, "curve")) {
            Some(Value::Curve(c)) => c.clone(),
            _ => bpe_kit::sound::LfoPreset::Sine.points(),
        }
    }

    fn click_at(h: &mut H, at: Pos2) {
        press(h, at);
        release(h, at);
    }

    /// Two quick clicks, a few frames after anything clicked before.
    fn double_click_at(h: &mut H, at: Pos2) {
        h.hover_at(at);
        for _ in 0..30 {
            h.step();
        }
        // This harness steps a quarter second a frame, too slow for egui's 0.3 s double-click.
        h.ctx.options_mut(|o| o.input_options.max_double_click_delay = 2.0);
        for pressed in [true, false, true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        h.run();
    }

    #[test]
    fn double_clicking_the_lfo_shape_adds_a_point_on_the_grid() {
        let mut h = sounding();
        let (lfo, rect) = lfo_shape(&mut h);
        // Start from a flat line, so the grid spot is free.
        h.get_by_role_and_label(Role::Button, "Clear").click();
        h.run();
        assert_eq!(curve_of(&h, lfo), bpe_kit::sound::lfo_curve::flat());
        let plot = lfo_view::Plot::new(rect);
        let before = curve_of(&h, lfo).len();
        // Near (0.27, 0.06), which snaps to the 8 x 4 grid at (0.25, 0).
        let at = plot.pos(0.27, 0.06);
        double_click_at(&mut h, at);
        let after = curve_of(&h, lfo);
        assert_eq!(after.len(), before + 1, "{after:?}");
        assert!(after.iter().any(|p| p.x == 0.25 && p.y == 0.0), "{after:?}");
        // Double-clicking it again takes it away.
        let at = plot.pos(0.25, 0.0);
        double_click_at(&mut h, at);
        assert_eq!(curve_of(&h, lfo).len(), before);
    }

    #[test]
    fn a_preset_replaces_the_shape_and_painting_puts_in_a_step() {
        let mut h = sounding();
        let (lfo, _) = lfo_shape(&mut h);
        h.get_by_label("LFO preset").click();
        h.run();
        // The menu's row (an oscillator's wave chip is called Square too).
        h.query_all_by_label("Square").last().unwrap().click();
        h.run();
        assert_eq!(curve_of(&h, lfo), bpe_kit::sound::LfoPreset::Square.points());
        h.get_by_role_and_label(Role::Button, "Step").click();
        h.run();
        let rect = h.get_by_label("Drawn shape").rect();
        let plot = lfo_view::Plot::new(rect);
        // The seventh of eight cells, painted three quarters up.
        click_at(&mut h, plot.pos(0.8, 0.74));
        let c = bpe_kit::sound::curve_numbers(&curve_of(&h, lfo));
        assert_eq!(bpe_dsp::lfo::curve_at(&c, 0.8), 0.75, "{c:?}");
        assert_eq!(bpe_dsp::lfo::curve_at(&c, 0.6), 0.0, "the rest of the square is left alone");
    }
}
