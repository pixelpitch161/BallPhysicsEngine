//! The big gradient editor: a window opened from the Editor button beside a gradient's bar,
//! editing the same setting live. A wide bar, how the colours mix and Loop, the tools, a table
//! of the stops (colour, hex, place, the gap after each and its middle) with Add colour, and the
//! gradient on a ball and on a ring of balls. Closing it changes nothing: every edit was made as
//! it happened.
//!
//! The window belongs to its gradient row (`gradient_tabs::tab_rows` draws it), so it shows
//! while that row is in the inspector. Its controls are named after "{label} editor", and its
//! bar keeps the selected stop where the row's does, so both select the same stop.

use bpe_core::{Color, Gradient};
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Ui, Vec2};

use crate::gradient_bar::{bar_in, gaps, select_stop, selected_stop, shown};
use crate::gradient_tabs::{BLENDS, hex_box, loop_switch, mix_chips, name_it};
use crate::gradient_tools::{self, ToolsOutcome};
use crate::style;
use crate::widgets::color_button;

/// The window's width when it first opens.
const WIDTH: f32 = 600.0;
/// The ball preview's size across.
const BALL: f32 = 72.0;
/// The ring preview: its balls, how big each is, and how far out they sit.
const RING_BALLS: usize = 12;
const RING_BALL: f32 = 7.0;
const RING: f32 = 34.0;
/// Columns of the ball preview's mesh.
const SLICES: usize = 48;

fn open_id(bar: egui::Id) -> egui::Id {
    bar.with("big editor")
}

/// Whether the big editor of the gradient whose bar keeps its memory under `bar` is open.
pub fn is_open(ctx: &egui::Context, bar: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id(bar))).unwrap_or(false)
}

fn set_open(ctx: &egui::Context, bar: egui::Id, open: bool) {
    ctx.data_mut(|d| {
        if open {
            d.insert_temp(open_id(bar), true);
        } else {
            d.remove::<bool>(open_id(bar));
        }
    });
}

/// The Editor button beside a gradient's bar, named "{name} editor": opens the big editor, or
/// closes it when it is open (lit then).
pub fn open_button(ui: &mut Ui, bar: egui::Id, name: &str) {
    let open = is_open(ui.ctx(), bar);
    let r = ui.add(style::toggle_button("Editor", open).small());
    name_it(ui, &r, format!("{name} editor"));
    let help = if open { "Close the big gradient editor" } else { "Open the big gradient editor" };
    if r.clicked() {
        // It opens beside the button, over the preview rather than the inspector.
        ui.ctx().data_mut(|d| d.insert_temp(bar.with("big editor at"), r.rect.left_top()));
    }
    if r.on_hover_text(help).clicked() {
        set_open(ui.ctx(), bar, !open);
    }
}

/// The big editor of the gradient setting labelled `label`, whose bar keeps its memory under
/// `bar`, when it is open; edits go into `g`.
pub fn window(ctx: &egui::Context, label: &str, bar: egui::Id, g: &mut Gradient) -> ToolsOutcome {
    let mut open = is_open(ctx, bar);
    let mut out = ToolsOutcome::default();
    if !open {
        return out;
    }
    let name = format!("{label} editor");
    let at = ctx.data(|d| d.get_temp::<Pos2>(bar.with("big editor at"))).unwrap_or(Pos2::new(WIDTH + 40.0, 80.0));
    egui::Window::new(format!("{label}: gradient"))
        .pivot(egui::Align2::RIGHT_TOP)
        .default_pos(at - Vec2::new(12.0, 0.0))
        .id(bar.with("big editor window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(WIDTH)
        .show(ctx, |ui| {
            out.changed |= bar_in(ui, bar.with("big"), bar, g, &name, false).changed();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Mix").color(style::MUTED));
                out.changed |= mix_chips(ui, g, &name);
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Loop").color(style::MUTED));
                out.changed |= loop_switch(ui, g, &name);
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Tools").color(style::MUTED));
                out.merge(gradient_tools::tools(ui, g, bar, &name));
            });
            style::divider(ui);
            out.changed |= table(ui, g, bar, &name);
            let add = ui.button("Add colour");
            name_it(ui, &add, format!("{name} add colour"));
            if add.on_hover_text("Add a colour halfway along the selected colour's gap").clicked() {
                let i = selected_stop(ui.ctx(), bar, g);
                let k = gradient_tools::add_colour(g, i);
                select_stop(ui.ctx(), bar, k);
                out.changed = true;
            }
            style::divider(ui);
            ui.horizontal(|ui| {
                ball(ui, g, label);
                ui.add_space(16.0);
                ring(ui, g, label);
            });
        });
    set_open(ctx, bar, open);
    out
}

/// What a table row asked for that moves the stops about, done after the table is drawn.
enum Reorder {
    At(usize, f32),
    Remove(usize),
}

/// The table of stops: a row each, with its number (a click selects it), colour, hex, place,
/// the gap after it (blend and middle; empty where there is no gap) and Remove. True when the
/// gradient changed.
fn table(ui: &mut Ui, g: &mut Gradient, bar: egui::Id, name: &str) -> bool {
    let mut changed = false;
    let mut selected = selected_stop(ui.ctx(), bar, g);
    let with_gaps: Vec<usize> = gaps(g).iter().map(|(i, ..)| *i).collect();
    let mut reorder = None;
    let n = g.stops.len();
    egui::Grid::new(bar.with("big table")).num_columns(7).spacing([10.0, 6.0]).show(ui, |ui| {
        for heading in ["", "Colour", "Hex", "At", "Gap after", "Middle", ""] {
            ui.label(egui::RichText::new(heading).color(style::MUTED));
        }
        ui.end_row();
        for i in 0..n {
            let k = i + 1;
            let number = ui.selectable_label(i == selected, format!("{k}"));
            name_it(ui, &number, format!("{name} row {k}"));
            let mut touched = number.on_hover_text("Select this colour").clicked();
            let mut c = g.stops[i].color;
            let r = color_button(ui, &mut c);
            name_it(ui, &r, format!("{name} colour {k}"));
            touched |= r.clicked();
            if r.changed() {
                g.stops[i].color = c;
                changed = true;
            }
            if hex_box(ui, format!("{name} hex {k}"), bar.with("big"), i, &mut g.stops[i].color) {
                changed = true;
                touched = true;
            }
            let mut at = g.stops[i].at * 100.0;
            let r = ui.add(egui::DragValue::new(&mut at).range(0.0..=100.0).speed(0.25).fixed_decimals(1).suffix("%"));
            name_it(ui, &r, format!("{name} at {k}"));
            let dragging = r.dragged();
            if r.on_hover_text("Where along the gradient this colour sits").changed() {
                let at = (at / 100.0).clamp(0.0, 1.0);
                if dragging {
                    // Mid-drag the box belongs to this row, so the stop stays between its
                    // neighbours; typing a place moves it past them.
                    let lo = if i > 0 { g.stops[i - 1].at } else { 0.0 };
                    let hi = g.stops.get(i + 1).map_or(1.0, |s| s.at);
                    g.stops[i].at = at.clamp(lo, hi);
                    changed = true;
                    touched = true;
                } else {
                    reorder = Some(Reorder::At(i, at));
                }
            }
            if with_gaps.contains(&i) {
                let blend = g.stops[i].blend;
                let words = BLENDS.iter().find(|(b, _)| *b == blend).map_or("Smooth", |(_, w)| w);
                let menu = ui.menu_button(words, |ui| {
                    for (b, w) in BLENDS {
                        if ui.selectable_label(b == blend, w).clicked() {
                            if b != blend {
                                g.stops[i].blend = b;
                                changed = true;
                            }
                            touched = true;
                            ui.close();
                        }
                    }
                });
                name_it(ui, &menu.response, format!("{name} gap {k}"));
                menu.response.on_hover_text("How the gap after this colour blends to the next");
                let mut m = g.stops[i].middle * 100.0;
                let r =
                    ui.add(egui::DragValue::new(&mut m).range(1.0..=99.0).speed(0.25).fixed_decimals(0).suffix("%"));
                name_it(ui, &r, format!("{name} gap middle {k}"));
                if r.on_hover_text("Where the halfway colour of the gap sits (or its hard edge, in Steps)").changed() {
                    g.stops[i].middle = (m / 100.0).clamp(0.01, 0.99);
                    changed = true;
                    touched = true;
                }
            } else {
                ui.label("");
                ui.label("");
            }
            let remove = ui.add_enabled(n > 1, egui::Button::new("Remove").small());
            name_it(ui, &remove, format!("{name} remove {k}"));
            if remove.on_hover_text("Take this colour off").clicked() {
                reorder = Some(Reorder::Remove(i));
            }
            if touched {
                selected = i;
            }
            ui.end_row();
        }
    });
    match reorder {
        Some(Reorder::At(i, at)) => {
            let mut stop = g.stops.remove(i);
            stop.at = at;
            let k = g.stops.partition_point(|s| s.at < at);
            g.stops.insert(k, stop);
            selected = k;
            changed = true;
        }
        Some(Reorder::Remove(i)) if g.stops.len() > 1 => {
            g.stops.remove(i);
            if selected > i || selected >= g.stops.len() {
                selected = selected.saturating_sub(1);
            }
            changed = true;
        }
        _ => {}
    }
    select_stop(ui.ctx(), bar, selected);
    changed
}

/// The colour of `g` at `t`, as the editor shows it.
fn colour_at(g: &Gradient, t: f32) -> egui::Color32 {
    shown(Color::from_linear(g.sample(t)))
}

/// A preview, named for the screen reader.
fn preview(ui: &mut Ui, size: Vec2, name: String, help: &str) -> Rect {
    let (rect, r) = ui.allocate_exact_size(size, Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name.clone()));
    r.on_hover_text(help);
    rect
}

/// The gradient straight across a ball, left to right.
fn ball(ui: &mut Ui, g: &Gradient, label: &str) {
    let rect = preview(ui, Vec2::splat(BALL), format!("{label} on a ball"), "The colours across one ball");
    let (c, r) = (rect.center(), BALL * 0.5);
    let mut mesh = egui::Mesh::default();
    for k in 0..=SLICES {
        let t = k as f32 / SLICES as f32;
        let x = -r + 2.0 * r * t;
        let h = (r * r - x * x).max(0.0).sqrt();
        let colour = colour_at(g, t);
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(c.x + x, c.y - h), colour);
        mesh.colored_vertex(Pos2::new(c.x + x, c.y + h), colour);
        if k > 0 {
            mesh.add_triangle(base - 2, base - 1, base);
            mesh.add_triangle(base - 1, base, base + 1);
        }
    }
    ui.painter().add(mesh);
    ui.painter().circle_stroke(c, r, Stroke::new(1.0, style::LINE));
}

/// The gradient on a ring of balls coloured by their order, as the balls' Order spreads it: the
/// last ball one step short of the end when the gradient loops, so the ring closes.
fn ring(ui: &mut Ui, g: &Gradient, label: &str) {
    let size = Vec2::splat(2.0 * (RING + RING_BALL + 1.0));
    let rect = preview(ui, size, format!("{label} on a ring"), "The colours on a ring of balls, by their order");
    let c = rect.center();
    for i in 0..RING_BALLS {
        let t = if g.looped { i as f32 / RING_BALLS as f32 } else { i as f32 / (RING_BALLS - 1) as f32 };
        // From the top, clockwise.
        let angle = std::f32::consts::TAU * i as f32 / RING_BALLS as f32 - std::f32::consts::FRAC_PI_2;
        let at = c + RING * Vec2::angled(angle);
        ui.painter().circle(at, RING_BALL, colour_at(g, t), Stroke::new(1.0, style::LINE));
    }
}

#[cfg(test)]
mod tests {
    use crate::gradient_tabs::tests::{BLUE, GREEN, RED, click, harness, three};
    use bpe_core::{Color, GapBlend, Gradient, Mix};
    use eframe::egui;
    use egui_kittest::kittest::Queryable;

    fn open(h: &mut egui_kittest::Harness<'_, crate::gradient_tabs::tests::State>) {
        click(h, "Colours editor");
        assert!(h.query_by_label("Colours: gradient").is_some(), "the window is open");
    }

    /// Clears a text box and types `text` into it.
    fn type_into(h: &mut egui_kittest::Harness<'_, crate::gradient_tabs::tests::State>, label: &str, text: &str) {
        h.get_by_label(label).click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.get_by_label(label).type_text(text);
        h.run();
    }

    #[test]
    fn the_editor_button_opens_the_window_and_closing_changes_nothing() {
        let mut h = harness(three());
        assert!(h.query_by_label("Colours: gradient").is_none());
        open(&mut h);
        for label in [
            "Colours editor bar",
            "Colours editor mix Even",
            "Colours editor loop",
            "Colours editor flip",
            "Colours editor add colour",
            "Colours on a ball",
            "Colours on a ring",
        ] {
            assert!(h.query_by_label(label).is_some(), "{label} shows");
        }
        for k in 1..=3 {
            assert!(h.query_by_label(&format!("Colours editor row {k}")).is_some());
        }
        assert!(h.query_by_label("Colours editor row 4").is_none(), "a row per stop");
        // Closing it changes nothing.
        click(&mut h, "Close window");
        assert!(h.query_by_label("Colours: gradient").is_none());
        assert!(h.state().changes.is_empty(), "{:?}", h.state().changes);
        assert_eq!(h.state().gradient(), &three());
    }

    #[test]
    fn the_table_edits_the_right_stop() {
        let mut h = harness(three());
        open(&mut h);
        type_into(&mut h, "Colours editor hex 2", "#ffff00");
        assert_eq!(h.state().gradient().stops[1].color, Color::rgb(255, 255, 0));
        assert_eq!(h.state().gradient().stops[0].color, RED);
        // The row's Colours tab shows the stop the table touched.
        assert!(h.query_by_label("Stop 2").is_some());
        click(&mut h, "Colours editor gap 1");
        click(&mut h, "Steps");
        assert_eq!(h.state().gradient().stops[0].blend, GapBlend::Steps);
        assert_eq!(h.state().gradient().stops[1].blend, GapBlend::Smooth);
        assert!(h.query_by_label("Colours editor gap 3").is_none(), "no gap after the last stop");
        type_into(&mut h, "Colours editor gap middle 1", "20");
        h.key_press(egui::Key::Enter);
        h.run();
        assert!((h.state().gradient().stops[0].middle - 0.2).abs() < 1e-6, "{:?}", h.state().gradient());
        // Moving stop 1 past stop 2 re-sorts and keeps it selected.
        type_into(&mut h, "Colours editor at 1", "75");
        h.key_press(egui::Key::Enter);
        h.run();
        let g = h.state().gradient().clone();
        assert_eq!(g.stops.iter().map(|s| s.color).collect::<Vec<_>>(), vec![Color::rgb(255, 255, 0), RED, BLUE]);
        assert!(h.query_by_label("Stop 2").is_some(), "red is still the selected one: {g:?}");
        click(&mut h, "Colours editor remove 3");
        let colours: Vec<Color> = h.state().gradient().stops.iter().map(|s| s.color).collect();
        assert_eq!(colours, vec![Color::rgb(255, 255, 0), RED], "blue went");
    }

    #[test]
    fn dragging_a_place_keeps_the_stop_between_its_neighbours() {
        let mut h = harness(three());
        open(&mut h);
        let r = h.get_by_label("Colours editor at 1").rect();
        h.drag_at(r.center());
        h.run();
        for x in [40.0, 120.0, 400.0] {
            h.hover_at(r.center() + egui::vec2(x, 0.0));
            h.run();
        }
        h.drop_at(r.center() + egui::vec2(400.0, 0.0));
        h.run();
        let g = h.state().gradient().clone();
        assert_eq!(g.stops.iter().map(|s| s.color).collect::<Vec<_>>(), vec![RED, GREEN, BLUE], "{g:?}");
        assert!((g.stops[0].at - 0.5).abs() < 1e-6, "red stops at green: {g:?}");
        assert_eq!(g.stops[1].at, 0.5, "green never moved");
    }

    #[test]
    fn add_colour_mix_and_loop() {
        let mut h = harness(Gradient::even(&[RED, BLUE]));
        open(&mut h);
        click(&mut h, "Colours editor add colour");
        let g = h.state().gradient().clone();
        assert_eq!(g.stops.len(), 3);
        assert!((g.stops[1].at - 0.5).abs() < 1e-6);
        assert!(h.query_by_label("Stop 2").is_some(), "the new colour is selected");
        click(&mut h, "Colours editor mix Long way round");
        assert_eq!(h.state().gradient().mix, Mix::LongWayRound);
        click(&mut h, "Colours editor loop");
        assert!(h.state().gradient().looped);
        assert!(h.query_by_label("Colours editor gap 3").is_some(), "the gap back round");
    }

    #[test]
    fn selecting_in_the_window_selects_in_the_row() {
        let mut h = harness(Gradient::even(&[RED, GREEN, BLUE]));
        open(&mut h);
        click(&mut h, "Colours editor row 3");
        assert!(h.query_by_label("Stop 3").is_some());
    }

    #[test]
    fn a_one_colour_gradient_has_no_editor_button() {
        let h = harness(Gradient::solid(RED));
        assert!(h.query_by_label("Colours editor").is_none());
    }
}
