//! The gradient bar: a gradient's colours drawn across the row, with a handle per stop under
//! it. Click the bar to add a stop there; drag a handle to move its stop, or well below the bar
//! to take it off; click a handle to change its colour, right-click it to remove it. Beside the
//! bar, a menu picks how the colours blend from stop to stop, and a Palette menu swaps in a
//! whole palette, saves this one, rolls a random one or adds colours that go with the first.

use bpe_core::{Choice, Color, Gradient, Stop, Transition};
use bpe_kit::paint::harmony::{Harmony, add_matching};
use bpe_kit::paint::palettes::{Palette, built_in};
use bpe_kit::random;
use bpe_kit::rng::Rng;
use eframe::egui::{
    self, Color32, Popup, PopupCloseBehavior, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, color_picker::Alpha,
};

use crate::palettes;
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
/// A palette's swatch in the Palette menu.
const SWATCH: Vec2 = Vec2::new(48.0, 14.0);
/// The tallest the Palette menu's lists grow before they scroll.
const LISTS_HEIGHT: f32 = 320.0;

/// A gradient bar named "Colours". The inspector names each bar after its setting.
#[cfg_attr(not(test), allow(dead_code))]
pub fn gradient_bar(ui: &mut Ui, id: egui::Id, g: &mut Gradient) -> Response {
    gradient_bar_named(ui, id, g, "Colours")
}

/// The gradient bar, its controls named after `name` ("Colours bar", "Colours stop 1",
/// "Colours transition", "Colours palette"). A one-stop gradient shows as a single swatch and a
/// "+" that adds a second stop, white at the end. The response is marked changed when `g`
/// changed.
pub fn gradient_bar_named(ui: &mut Ui, id: egui::Id, g: &mut Gradient, name: &str) -> Response {
    if g.stops.len() <= 1 {
        return ui.horizontal(|ui| one_colour(ui, id, g, name)).inner;
    }
    let mut changed = false;
    let inner = ui.vertical(|ui| {
        let width = (ui.available_width() - MENU_WIDTH - ui.spacing().item_spacing.x).max(64.0);
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
        // The handles, with the Palette menu beside them, under the transition menu: the bar's row
        // has no room for both menus in a narrow inspector.
        let strip = ui
            .horizontal_top(|ui| {
                let (strip, _) = ui.allocate_exact_size(Vec2::new(width, HANDLE_STRIP), Sense::hover());
                if palette_menu(ui, id, g, name, true) {
                    changed = true;
                }
                strip
            })
            .inner;
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

/// One colour: its swatch, a "+" that adds a second stop, and the Palette menu.
fn one_colour(ui: &mut Ui, id: egui::Id, g: &mut Gradient, name: &str) -> Response {
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
    }
    // A palette replaces the whole gradient, so a single colour becomes all of its stops.
    if palette_menu(ui, id, g, name, false) {
        changed = true;
    }
    if changed {
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

/// What the Palette menu was asked to do.
enum PaletteAction {
    Apply(Gradient),
    Remove(String),
    Save(String),
}

/// The Palette menu: the built-in palettes, the saved ones, "Save as palette...", "Random
/// palette" and "Add matching colours". Saving needs a palette store in the context; without
/// one the menu shows the built-ins only. `small` fits the button to the handle strip. True when
/// `g` changed.
fn palette_menu(ui: &mut Ui, id: egui::Id, g: &mut Gradient, name: &str, small: bool) -> bool {
    let button = egui::Button::new("Palette");
    let button = ui.add(if small { button.small() } else { button });
    let label = format!("{name} palette");
    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
    let button = button.on_hover_text("Use a palette, save these colours, or add colours that go with them");
    let popup_id = id.with("palette menu");
    let naming_key = popup_id.with("naming");
    let error_key = popup_id.with("error");
    let focus_key = popup_id.with("focus name");
    let store = palettes::store(ui.ctx());
    let mut action = None;
    Popup::menu(&button).id(popup_id).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let saved = store.as_ref().map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).saved.clone());
        // The lists scroll, so however many palettes are saved the actions below stay in reach.
        egui::ScrollArea::vertical().id_salt(popup_id.with("lists")).max_height(LISTS_HEIGHT).show(ui, |ui| {
            for p in built_in() {
                if palette_item(ui, &p).clicked() {
                    action = Some(PaletteAction::Apply(p.colours));
                }
            }
            let Some(saved) = saved.filter(|s| !s.is_empty()) else { return };
            ui.separator();
            ui.weak("Saved");
            for p in saved {
                ui.horizontal(|ui| {
                    if palette_item(ui, &p).clicked() {
                        action = Some(PaletteAction::Apply(p.colours.clone()));
                    }
                    let more = ui.menu_button("More", |ui| {
                        if ui.button("Remove").on_hover_text("Take this palette off the list").clicked() {
                            action = Some(PaletteAction::Remove(p.name.clone()));
                            ui.close();
                        }
                    });
                    let label = format!("{} options", p.name);
                    more.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
                });
            }
        });
        if store.is_some() {
            ui.separator();
            let mut naming: Option<String> = ui.data(|d| d.get_temp(naming_key));
            let mut cancel = false;
            match &mut naming {
                None => {
                    if ui.button("Save as palette...").on_hover_text("Keep these colours to use again").clicked() {
                        naming = Some(String::new());
                        ui.data_mut(|d| d.insert_temp(focus_key, true));
                    }
                }
                Some(text) => {
                    ui.horizontal(|ui| {
                        let label = ui.label("Palette name");
                        let field = egui::TextEdit::singleline(text).hint_text("My palette").desired_width(120.0);
                        let field = ui.add(field).labelled_by(label.id);
                        if ui.data_mut(|d| d.remove_temp::<bool>(focus_key)).is_some() {
                            field.request_focus();
                        }
                        // Enter saves, as the Save button does.
                        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            action = Some(PaletteAction::Save(text.clone()));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            action = Some(PaletteAction::Save(text.clone()));
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                }
            }
            if cancel {
                naming = None;
            }
            ui.data_mut(|d| match naming {
                Some(text) => {
                    d.insert_temp(naming_key, text);
                }
                None => d.remove::<String>(naming_key),
            });
        } else {
            ui.separator();
        }
        if ui.button("Random palette").on_hover_text("New colours that go together").clicked() {
            let mut dice = Rng::new(random::fresh_seed(), 0);
            let colours = random::palette(&mut dice, g.stops.len().max(3));
            let mut random = Gradient::even(&colours);
            random.transition = g.transition;
            action = Some(PaletteAction::Apply(random));
        }
        ui.menu_button("Add matching colours", |ui| {
            for h in Harmony::ALL {
                if ui.button(h.label()).clicked() {
                    action = Some(PaletteAction::Apply(add_matching(g, h)));
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Replace the colours with the first one and colours that go with it");
        if let Some(error) = ui.data(|d| d.get_temp::<String>(error_key)) {
            ui.colored_label(theme::ERROR, error);
        }
    });
    if !Popup::is_id_open(ui.ctx(), popup_id) {
        ui.data_mut(|d| {
            d.remove::<String>(naming_key);
            d.remove::<String>(error_key);
        });
    }
    let mut changed = false;
    let mut close = false;
    match action {
        None => {}
        Some(PaletteAction::Apply(next)) => {
            changed = next != *g;
            *g = next;
            close = true;
        }
        Some(PaletteAction::Remove(palette)) => {
            if let Some(store) = &store {
                let done = store.lock().unwrap_or_else(|e| e.into_inner()).remove(&palette);
                if let Err(e) = done {
                    ui.data_mut(|d| d.insert_temp(error_key, format!("Could not remove: {e}")));
                }
            }
        }
        Some(PaletteAction::Save(palette)) => {
            if let Some(store) = &store {
                match store.lock().unwrap_or_else(|e| e.into_inner()).save_as(&palette, g) {
                    Ok(()) => {
                        ui.data_mut(|d| d.remove::<String>(naming_key));
                        close = true;
                    }
                    Err(e) => {
                        ui.data_mut(|d| d.insert_temp(error_key, format!("Could not save: {e}")));
                    }
                }
            }
        }
    }
    if close {
        Popup::close_id(ui.ctx(), popup_id);
        ui.data_mut(|d| d.remove::<String>(error_key));
    }
    changed
}

/// One palette in the menu: a small swatch of its colours and its name.
fn palette_item(ui: &mut Ui, p: &Palette) -> Response {
    ui.horizontal(|ui| {
        let (swatch, _) = ui.allocate_exact_size(SWATCH, Sense::hover());
        paint_bar(ui.painter(), swatch, &p.colours);
        ui.button(&p.name)
    })
    .inner
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
            if egui::color_picker::color_picker_color32(ui, &mut c, Alpha::OnlyBlend) {
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

#[cfg(test)]
mod tests {
    use super::gradient_bar;
    use crate::palettes::{self, PaletteStore};
    use bpe_core::{Color, Gradient, Transition};
    use bpe_kit::paint::harmony::{Harmony, add_matching};
    use bpe_kit::paint::palettes::built_in;
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};
    use std::path::PathBuf;

    const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
    const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

    /// A bar in a window the size of a narrow inspector, with a palette store when given one.
    fn harness(g: Gradient, store: Option<PaletteStore>) -> Harness<'static, Gradient> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 900.0))
            .allow_missing_glyphs()
            .with_context_setup(move |ctx| {
                crate::theme::apply(ctx);
                if let Some(store) = store {
                    palettes::install(ctx, store);
                }
            })
            .build_ui_state(
                |ui, g: &mut Gradient| {
                    let id = ui.make_persistent_id("bar");
                    gradient_bar(ui, id, g);
                },
                g,
            );
        h.run();
        h
    }

    fn temp_store(tag: &str) -> (PaletteStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ball-studio-palette-menu-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("palettes.ron");
        (PaletteStore::load_from(path.clone()), path)
    }

    fn ocean() -> Gradient {
        built_in().into_iter().find(|p| p.name == "Ocean").unwrap().colours
    }

    fn open_menu(h: &mut Harness<'_, Gradient>) {
        h.get_by_label("Colours palette").click();
        h.run();
    }

    fn red_blue_steps() -> Gradient {
        let mut g = Gradient::even(&[RED, BLUE]);
        g.transition = Transition::Steps;
        g
    }

    #[test]
    fn picking_a_palette_replaces_the_stops() {
        let mut h = harness(red_blue_steps(), None);
        let bar = h.get_by_label("Colours bar").rect();
        let menu = h.get_by_label("Colours palette").rect();
        assert!(menu.right() <= 420.0 && bar.width() >= 200.0, "fits a narrow inspector: {bar:?} {menu:?}");
        open_menu(&mut h);
        h.get_by_label("Ocean").click();
        h.run();
        assert_eq!(*h.state(), ocean(), "stops and transition both come from the palette");
        assert!(h.query_by_label("Sunset").is_none(), "the menu closes");
    }

    #[test]
    fn a_one_colour_bar_takes_a_whole_palette() {
        let mut h = harness(Gradient::solid(RED), None);
        open_menu(&mut h);
        h.get_by_label("Ocean").click();
        h.run();
        assert_eq!(h.state().stops.len(), ocean().stops.len());
        assert_eq!(*h.state(), ocean());
        assert!(h.query_by_label("Colours bar").is_some(), "it is a gradient now");
    }

    #[test]
    fn add_matching_triad_gives_three_stops() {
        let start = red_blue_steps();
        let mut h = harness(start.clone(), None);
        open_menu(&mut h);
        h.get_by_label("Add matching colours").click();
        h.run();
        for harmony in Harmony::ALL {
            assert!(h.query_by_label(harmony.label()).is_some(), "{harmony:?} listed");
        }
        h.get_by_label("Triad").click();
        h.run();
        assert_eq!(*h.state(), add_matching(&start, Harmony::Triad));
        assert_eq!(h.state().stops.len(), 3);
        assert_eq!(h.state().transition, Transition::Steps, "the transition is kept");
    }

    #[test]
    fn save_as_palette_adds_it_to_the_saved_list() {
        let (store, path) = temp_store("save");
        let start = red_blue_steps();
        let mut h = harness(start.clone(), Some(store));
        open_menu(&mut h);
        h.get_by_label("Save as palette...").click();
        h.run();
        h.get_by_label("Palette name").click();
        h.run();
        h.get_by_label("Palette name").type_text("Mine");
        h.run();
        h.get_by_label("Save").click();
        h.run();
        let store = palettes::store(&h.ctx).unwrap();
        let saved = store.lock().unwrap().saved.clone();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "Mine");
        assert_eq!(saved[0].colours, start);
        assert!(path.exists(), "written to disk");
        assert_eq!(*h.state(), start, "saving leaves the colours alone");
        // It shows in the menu under Saved, and picking it puts its colours back.
        h.state_mut().stops = Gradient::solid(BLUE).stops;
        h.run();
        if h.query_by_label("Saved").is_none() {
            open_menu(&mut h);
        }
        assert!(h.query_by_label("Saved").is_some());
        h.get_by_label("Mine").scroll_to_me();
        h.run();
        h.get_by_label("Mine").click();
        h.run();
        assert_eq!(*h.state(), start);
    }

    #[test]
    fn a_saved_palette_can_be_removed() {
        let (mut store, path) = temp_store("remove");
        store.save_as("Old", &red_blue_steps()).unwrap();
        let mut h = harness(Gradient::solid(BLUE), Some(store));
        open_menu(&mut h);
        h.get_by_label("Old options").scroll_to_me();
        h.run();
        h.get_by_label("Old options").click();
        h.run();
        h.get_by_label("Remove").click();
        h.run();
        let store = palettes::store(&h.ctx).unwrap();
        assert!(store.lock().unwrap().saved.is_empty());
        assert!(PaletteStore::load_from(path).saved.is_empty(), "removed on disk too");
        assert_eq!(*h.state(), Gradient::solid(BLUE), "removing leaves the colours alone");
    }

    #[test]
    fn a_failed_save_says_why() {
        // The store's folder is a file, so writing fails.
        let (_, path) = temp_store("fail");
        let blocker = path.with_file_name("not a folder");
        std::fs::write(&blocker, b"").unwrap();
        let store = PaletteStore::load_from(blocker.join("palettes.ron"));
        let mut h = harness(red_blue_steps(), Some(store));
        open_menu(&mut h);
        h.get_by_label("Save as palette...").click();
        h.run();
        h.get_by_label("Save").click();
        h.run();
        assert!(h.query_by_label_contains("Could not save").is_some(), "the menu says the save failed");
        let store = palettes::store(&h.ctx).unwrap();
        assert!(store.lock().unwrap().saved.is_empty(), "a failed save is not kept");
        assert!(h.query_by_label("Saved").is_none(), "nor listed");
    }

    #[test]
    fn the_name_field_takes_focus_and_enter_saves() {
        let (store, path) = temp_store("enter");
        let start = red_blue_steps();
        let mut h = harness(start.clone(), Some(store));
        open_menu(&mut h);
        h.get_by_label("Save as palette...").click();
        h.run();
        assert!(h.get_by_label("Palette name").is_focused(), "ready to type");
        h.get_by_label("Palette name").type_text("Quick");
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        let store = palettes::store(&h.ctx).unwrap();
        let saved = store.lock().unwrap().saved.clone();
        assert_eq!(saved.len(), 1, "{saved:?}");
        assert_eq!(saved[0].name, "Quick");
        assert_eq!(saved[0].colours, start);
        assert!(path.exists());
    }

    #[test]
    fn many_saved_palettes_keep_the_menu_actions_on_screen() {
        let (mut store, _) = temp_store("many");
        for i in 0..40 {
            store.save_as(&format!("Mine {i}"), &red_blue_steps()).unwrap();
        }
        let start = red_blue_steps();
        let mut h = harness(start.clone(), Some(store));
        open_menu(&mut h);
        for label in ["Save as palette...", "Random palette", "Add matching colours"] {
            let rect = h.get_by_label(label).rect();
            assert!(rect.top() >= 0.0 && rect.bottom() <= 900.0, "{label} is on screen: {rect:?}");
        }
        h.get_by_label("Add matching colours").click();
        h.run();
        h.get_by_label("Complementary").click();
        h.run();
        assert_eq!(*h.state(), add_matching(&start, Harmony::Complementary));
    }

    #[test]
    fn no_store_hides_save_as() {
        let mut h = harness(red_blue_steps(), None);
        open_menu(&mut h);
        assert!(h.query_by_label("Ocean").is_some(), "built-ins still show");
        assert!(h.query_by_label("Save as palette...").is_none());
        assert!(h.query_by_label("Saved").is_none());
    }

    #[test]
    fn random_palette_changes_the_colours() {
        let start = red_blue_steps();
        let mut h = harness(start.clone(), None);
        open_menu(&mut h);
        h.get_by_label("Random palette").click();
        h.run();
        let g = h.state().clone();
        assert_ne!(g.stops, start.stops);
        assert_eq!(g.stops.len(), 3, "at least three colours");
        assert_eq!(g.transition, Transition::Steps, "the transition is kept");
        assert!(g.stops.iter().all(|s| !s.color.is_additive()));
        assert_eq!(g.clone().sorted(), g);
    }
}
