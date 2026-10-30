//! The gradient bar: a gradient's colours drawn across the row, with a handle per stop under
//! it. Click the bar to add a stop there; drag a handle to move its stop, or well below the bar
//! to take it off; click a handle to change its colour, right-click it to remove it. Beside the
//! bar, a menu picks how the colours blend from stop to stop.

use bpe_core::{Choice, Color, Gradient, Stop, Transition};
use eframe::egui::{
    self, Color32, Popup, PopupCloseBehavior, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, color_picker::Alpha,
};

use crate::theme;
use crate::widgets::{color_button, picker_color, pretty, scene_color};

/// How many samples the bar is drawn from.
const SAMPLES: usize = 256;
/// The bar's height.
const BAR_HEIGHT: f32 = 18.0;
/// The strip of handles under the bar.
const HANDLE_STRIP: f32 = 16.0;
/// A handle dragged further than this below the bar comes off when let go.
const REMOVE_DROP: f32 = 30.0;
/// Room kept beside the bar for the transition menu.
const MENU_WIDTH: f32 = 72.0;

/// A gradient bar named "Colours". The inspector names each bar after its setting.
#[cfg_attr(not(test), allow(dead_code))]
pub fn gradient_bar(ui: &mut Ui, id: egui::Id, g: &mut Gradient) -> Response {
    gradient_bar_named(ui, id, g, "Colours")
}

/// The gradient bar, its controls named after `name` ("Colours bar", "Colours stop 1",
/// "Colours transition"). A one-stop gradient shows as a single swatch and a "+" that adds a
/// second stop, white at the end. The response is marked changed when `g` changed.
pub fn gradient_bar_named(ui: &mut Ui, id: egui::Id, g: &mut Gradient, name: &str) -> Response {
    if g.stops.len() <= 1 {
        return ui.horizontal(|ui| one_colour(ui, g, name)).inner;
    }
    let mut changed = false;
    let inner = ui.vertical(|ui| {
        let width = (ui.available_width() - MENU_WIDTH - ui.spacing().item_spacing.x).max(120.0);
        let (bar, mut response) = ui
            .horizontal(|ui| {
                let (bar, response) = ui.allocate_exact_size(Vec2::new(width, BAR_HEIGHT), Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name} bar")));
                paint_bar(ui.painter(), bar, g);
                let response = response.on_hover_text("Click to add a colour here");
                if response.clicked()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    let at = along(bar, p.x);
                    let color = Color::from_linear(g.sample(at));
                    let i = g.stops.partition_point(|s| s.at <= at);
                    g.stops.insert(i, Stop { at, color });
                    changed = true;
                }
                if transition_menu(ui, &mut g.transition, name) {
                    changed = true;
                }
                (bar, response)
            })
            .inner;
        let (strip, _) = ui.allocate_exact_size(Vec2::new(width, HANDLE_STRIP), Sense::hover());
        response |= handles(ui, id, g, bar, strip, name, &mut changed);
        // Air between the handles and the next row.
        ui.add_space(2.0);
        response
    });
    let mut response = inner.inner;
    if changed {
        response.mark_changed();
    }
    response
}

/// One colour: its swatch, and a "+" that adds a second stop.
fn one_colour(ui: &mut Ui, g: &mut Gradient, name: &str) -> Response {
    let mut color = g.first();
    let mut response = color_button(ui, &mut color);
    let label = format!("{name} colour");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ColorWell, true, label.clone()));
    let mut changed = response.changed();
    let add = ui.button("+").on_hover_text("Add a colour, to blend from one to the other");
    add.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Add a colour"));
    let mut stops = vec![Stop { at: 0.0, color }];
    if add.clicked() {
        stops.push(Stop { at: 1.0, color: Color::WHITE });
        changed = true;
    }
    if changed {
        g.stops = stops;
        response.mark_changed();
    }
    response | add
}

/// Where along the bar `x` falls, 0 to 1.
fn along(bar: Rect, x: f32) -> f32 {
    ((x - bar.left()) / bar.width()).clamp(0.0, 1.0)
}

/// The gradient, sampled across the bar, over a checkerboard so see-through colours show.
fn paint_bar(painter: &egui::Painter, bar: Rect, g: &Gradient) {
    // The checkerboard stays a pixel inside the colours, so its edge never peeks out where the
    // two round to pixels differently.
    let board = bar.shrink(1.0);
    let cell = BAR_HEIGHT / 2.0;
    let columns = (bar.width() / cell).ceil() as usize;
    for col in 0..columns {
        for row in 0..2 {
            let dark = (col + row) % 2 == 0;
            let min = Pos2::new(bar.left() + col as f32 * cell, bar.top() + row as f32 * cell);
            let cell_rect = Rect::from_min_size(min, Vec2::splat(cell)).intersect(board);
            painter.rect_filled(cell_rect, 0.0, if dark { Color32::from_gray(90) } else { Color32::from_gray(150) });
        }
    }
    let baked = g.bake(SAMPLES);
    let step = bar.width() / SAMPLES as f32;
    let mut mesh = egui::Mesh::default();
    for (i, c) in baked.iter().enumerate() {
        let color = shown(Color::from_linear(*c));
        let x = bar.left() + (i as f32 + 0.5) * step;
        let x = if i == 0 {
            bar.left()
        } else if i == SAMPLES - 1 {
            bar.right()
        } else {
            x
        };
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(x, bar.top()), color);
        mesh.colored_vertex(Pos2::new(x, bar.bottom()), color);
        if i > 0 {
            mesh.add_triangle(base - 2, base - 1, base);
            mesh.add_triangle(base - 1, base, base + 1);
        }
    }
    painter.add(mesh);
    painter.rect_stroke(bar, 2.0, Stroke::new(1.0, theme::MUTED), egui::StrokeKind::Outside);
}

/// A colour as the bar shows it: see-through over the checkerboard, an additive one solid.
fn shown(c: Color) -> Color32 {
    if c.is_additive() { Color32::from_rgb(c.r, c.g, c.b) } else { Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a) }
}

/// The menu picking how the colours blend. True when a new one was picked.
fn transition_menu(ui: &mut Ui, transition: &mut Transition, name: &str) -> bool {
    let mut picked = None;
    let menu = ui.menu_button(pretty(transition.name()), |ui| {
        for t in Transition::NAMES {
            if ui.selectable_label(t == &transition.name(), pretty(t)).clicked() {
                picked = Transition::from_name(t);
                ui.close();
            }
        }
    });
    let label = format!("{name} transition");
    menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
    menu.response.on_hover_text("How the colours blend from one stop to the next");
    match picked {
        Some(t) if t != *transition => {
            *transition = t;
            true
        }
        _ => false,
    }
}

/// The handle of stop `i`'s id.
fn handle_id(id: egui::Id, i: usize) -> egui::Id {
    id.with(("stop", i))
}

/// The handles under the bar, one per stop, and what they do. The stop being dragged is kept in
/// the bar's memory, so it stays the dragged one when the list re-sorts under it.
fn handles(
    ui: &mut Ui,
    id: egui::Id,
    g: &mut Gradient,
    bar: Rect,
    strip: Rect,
    name: &str,
    changed: &mut bool,
) -> Response {
    let drag_key = id.with("dragging");
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(drag_key));
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let far = |p: Option<Pos2>| p.is_some_and(|p| p.y > bar.bottom() + REMOVE_DROP);
    let mut remove = None;
    let mut response: Option<Response> = None;
    for i in 0..g.stops.len() {
        let hid = handle_id(id, i);
        let x = bar.left() + g.stops[i].at * bar.width();
        let rect = Rect::from_center_size(Pos2::new(x, strip.center().y), Vec2::new(12.0, HANDLE_STRIP));
        let r = ui.interact(rect, hid, Sense::click_and_drag());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name} stop {}", i + 1)));
        let leaving = dragging == Some(i) && far(pointer);
        paint_handle(ui.painter(), rect, g.stops[i].color, r.hovered() || r.has_focus(), leaving);
        if r.drag_started() {
            dragging = Some(i);
        }
        // A click opens the colour picker; its popup has its own id, apart from the menu's.
        let mut c = picker_color(g.stops[i].color);
        Popup::menu(&r).id(hid.with("picker")).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.spacing_mut().slider_width = 275.0;
            if egui::color_picker::color_picker_color32(ui, &mut c, Alpha::BlendOrAdditive) {
                g.stops[i].color = scene_color(c);
                *changed = true;
            }
        });
        Popup::context_menu(&r).id(hid.with("remove")).show(|ui| {
            if ui.button("Remove").on_hover_text("Take this colour off").clicked() {
                remove = Some(i);
                ui.close();
            }
        });
        let r = r.on_hover_text("Click to change the colour, drag to move it, drag down or right-click to remove");
        response = Some(match response {
            Some(all) => all | r,
            None => r,
        });
    }
    if let Some(j) = dragging {
        let down = ui.input(|i| i.pointer.primary_down());
        if j >= g.stops.len() {
            dragging = None;
        } else if down {
            if let Some(p) = pointer {
                let at = along(bar, p.x);
                let mut stop = g.stops.remove(j);
                if stop.at != at {
                    *changed = true;
                }
                stop.at = at;
                let k = g.stops.partition_point(|s| s.at < at);
                g.stops.insert(k, stop);
                dragging = Some(k);
                ui.memory_mut(|m| m.request_focus(handle_id(id, k)));
                if k != j {
                    ui.ctx().request_repaint();
                }
            }
        } else {
            if far(pointer) {
                remove = Some(j);
            }
            dragging = None;
        }
    }
    if let Some(i) = remove
        && g.stops.len() > 1
    {
        g.stops.remove(i);
        *changed = true;
    }
    ui.data_mut(|d| match dragging {
        Some(j) => {
            d.insert_temp(drag_key, j);
        }
        None => d.remove::<usize>(drag_key),
    });
    response.expect("a bar has at least two stops")
}

/// A stop's handle: a point up at the bar over a swatch of its colour. Lit when hovered or
/// focused, faded when it is about to come off.
fn paint_handle(painter: &egui::Painter, rect: Rect, color: Color, lit: bool, leaving: bool) {
    let edge = if lit { theme::ACCENT_TEXT } else { Color32::from_gray(220) };
    let edge = if leaving { edge.gamma_multiply(0.35) } else { edge };
    let fill = if leaving { shown(color).gamma_multiply(0.35) } else { shown(color) };
    let tip = Pos2::new(rect.center().x, rect.top());
    let shoulder = rect.top() + 5.0;
    let point = vec![tip, Pos2::new(rect.right(), shoulder), Pos2::new(rect.left(), shoulder)];
    painter.add(egui::Shape::convex_polygon(point, edge, Stroke::NONE));
    let swatch = Rect::from_min_max(Pos2::new(rect.left(), shoulder), rect.max);
    painter.rect_filled(swatch, 2.0, fill);
    painter.rect_stroke(swatch, 2.0, Stroke::new(1.5, edge), egui::StrokeKind::Inside);
}
