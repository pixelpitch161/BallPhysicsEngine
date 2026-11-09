//! The Placement card: where the thing sits, how it is turned, how it hangs off its parent,
//! its z-order and whether it sticks to the screen, and where balls are launched.

use super::*;

/// The Placement card, on every thing but the root, the camera, lights and rigs: the camera's and
/// a light's place is its own Position setting, which can be keyframed; a rig only groups lights.
pub(super) fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    edits: &mut Vec<Edit>,
) {
    let (scene, kit, id, thing, lw) = (cx.scene, cx.kit, cx.id, cx.thing, cx.label_width);
    if id != ThingId::ROOT
        && kind.id() != "camera"
        && kind.id() != bpe_kit::things::light::LIGHT
        && kind.id() != bpe_kit::things::rig::RIG
    {
        ui.add_space(6.0);
        card(ui, |ui| {
            let header = ui.horizontal(|ui| {
                let open = fold_arrow(ui, &mut inspector.collapsed, (id, None, "placement"), "Placement");
                ui.label(RichText::new("Placement").font(style::semibold(13.0)));
                open
            });
            card_header(ui, inspector, Card::Placement, &header.response);
            if header.inner {
                let mut placed = Vec::new();
                let z = kit.draws.thing(kind.id()).is_some();
                placement_rows(ui, scene, kind, thing, lw, z, &mut placed);
                // Number boxes move under any mouse button; a right-click changes nothing.
                if !crate::widgets::right_button_busy(ui) {
                    edits.extend(placed);
                }
                // Balls are launched from where they are placed.
                let launch: Vec<SettingDef> =
                    kind.settings().iter().filter(|d| PLACEMENT_KEYS.contains(&d.key)).cloned().collect();
                if !launch.is_empty() {
                    own_grid(ui, inspector, cx, ("placement own", id), &launch, edits);
                }
            }
        });
    }
}

/// A thing's own settings shown under Placement: where balls are launched.
pub(super) const PLACEMENT_KEYS: [&str; 1] = ["velocity"];

/// The Placement card's rows: where the thing sits, as a position of its own or an offset along
/// for the ride, how it is turned, how it hangs off its parent, and whether it sticks to the
/// screen.
pub(super) fn placement_rows(
    ui: &mut Ui,
    scene: &Scene,
    kind: &dyn bpe_core::ThingKind,
    thing: &bpe_core::Thing,
    lw: f32,
    z_order: bool,
    edits: &mut Vec<Edit>,
) {
    let right = ui.available_rect_before_wrap().right();
    settings_like_grid(("placement", thing.id)).show(ui, |ui| {
        position_rows(ui, scene, thing, lw, right, edits);
        if z_order {
            let label = grid_label(ui, lw, "Z-order").on_hover_text(
                "What draws on top: a thing with a higher z-order draws over one with a lower z-order. \
                 At the same z-order, shapes draw under balls",
            );
            let mut z = thing.z_order;
            let range = bpe_core::Thing::Z_ORDER_RANGE;
            if ui.add(DragValue::new(&mut z).speed(0.1).range(range)).labelled_by(label.id).changed() {
                edits.push(Edit::ZOrder(z));
            }
            ui.end_row();
        }
        if kind.can_stick_to_screen() {
            let label = grid_label(ui, lw, "Stick to screen")
                .on_hover_text("Stay put on the screen while the camera moves, like a title");
            let mut on = thing.sticks_to_screen(kind);
            if style::switch(ui, &mut on).labelled_by(label.id).changed() {
                edits.push(Edit::Screen((on != kind.sticks_to_screen()).then_some(on)));
            }
            ui.end_row();
        }
    });
}

/// A place's x and y boxes, dragged while the grid snaps, land on grid points.
pub(super) fn snap_place(ui: &Ui, (x, y): (&egui::Response, &egui::Response), before: glam::Vec2, at: &mut glam::Vec2) {
    let snap = Snapper::place(ui);
    for (r, was, now) in [(x, before.x, &mut at.x), (y, before.y, &mut at.y)] {
        let mut v = *now as f64;
        if crate::setting_snap::after_drag(ui, r, was as f64, &mut v, snap, false, None) {
            *now = v as f32;
        }
    }
}

/// Where the thing sits and how it hangs off its parent: Position (for a grouped thing),
/// Rotation, Attach, and a rider's Offset, Turns with it and Item.
pub(super) fn position_rows(
    ui: &mut Ui,
    scene: &Scene,
    thing: &bpe_core::Thing,
    lw: f32,
    right: f32,
    edits: &mut Vec<Edit>,
) {
    let mut t = thing.transform;
    if let Attach::Group = thing.attach {
        let position = grid_label(ui, lw, "Position")
            .on_hover_text("Where it sits in the world. At Zoom 1 the shot is 1080 across");
        // In a narrow panel y goes under x.
        let moved = widget_cell(ui, right, |ui| {
            ui.horizontal_wrapped(|ui| {
                let before = t.position;
                let x = ui.add(DragValue::new(&mut t.position.x).speed(1.0).prefix("x ").suffix(" px"));
                let y = ui.add(DragValue::new(&mut t.position.y).speed(1.0).prefix("y ").suffix(" px"));
                snap_place(ui, (&x, &y), before, &mut t.position);
                x.labelled_by(position.id).changed() || y.labelled_by(position.id).changed()
            })
            .inner
        });
        ui.end_row();
        if moved {
            edits.push(Edit::Transform(t));
        }
    }
    let rotation = grid_label(ui, lw, "Rotation").on_hover_text("How far it is turned, clockwise");
    let mut degrees = t.rotation.to_degrees();
    let before = degrees;
    let turn = ui.add(DragValue::new(&mut degrees).speed(1.0).suffix("°")).labelled_by(rotation.id);
    let mut d = degrees as f64;
    if crate::setting_snap::after_drag(ui, &turn, before as f64, &mut d, Snapper::angle(ui), false, None) {
        degrees = d as f32;
    }
    let turned = turn.changed();
    if turned {
        t.rotation = degrees.to_radians();
        edits.push(Edit::Transform(t));
    }
    ui.end_row();

    let parent = thing.parent.unwrap_or(ThingId::ROOT);
    let rides = thing.rides();
    grid_label(ui, lw, "Attach").on_hover_text("Grouped things keep their own place; a rider follows its parent");
    widget_cell(ui, right, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(!rides, "Grouped").clicked() && rides {
                edits.push(Edit::Attach(Attach::Group));
            }
            let ride = ui.add_enabled(parent != ThingId::ROOT, egui::Button::selectable(rides, "Rides on it"));
            let why = "Scene settings don't move, so there is nothing to ride";
            if ride.on_disabled_hover_text(why).clicked() && !rides {
                let ride = Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None };
                edits.push(Edit::Attach(ride));
            }
        });
    });
    ui.end_row();

    if let Attach::Ride { offset, turn_with_parent, item } = thing.attach {
        let mut o = offset;
        let label = grid_label(ui, lw, "Offset").on_hover_text("Where it sits from its parent, in video pixels");
        let moved = widget_cell(ui, right, |ui| {
            ui.horizontal_wrapped(|ui| {
                let before = o;
                let x = ui.add(DragValue::new(&mut o.x).speed(1.0).prefix("x ").suffix(" px"));
                let y = ui.add(DragValue::new(&mut o.y).speed(1.0).prefix("y ").suffix(" px"));
                snap_place(ui, (&x, &y), before, &mut o);
                x.labelled_by(label.id).changed() || y.labelled_by(label.id).changed()
            })
            .inner
        });
        ui.end_row();
        let label = grid_label(ui, lw, "Turns with it").on_hover_text("Swing around as the parent turns");
        let mut turns = turn_with_parent;
        let turned = style::switch(ui, &mut turns).labelled_by(label.id).changed();
        ui.end_row();
        let mut picked = item;
        let mut item_changed = false;
        if thing.parent.and_then(|p| scene.things.get(&p)).is_some_and(|p| p.kind == "balls") {
            // The same words as every other thing that rides balls: one ball, by its number from 1
            // (kept as the ball's number from 0).
            grid_label(ui, lw, "Follows").on_hover_text("The whole group, or one of its balls");
            widget_cell(ui, right, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let whole = ui.add(egui::Button::selectable(item.is_none(), "The whole group"));
                    if whole.clicked() && item.is_some() {
                        picked = None;
                        item_changed = true;
                    }
                    let one = ui.add(egui::Button::selectable(item.is_some(), "One ball"));
                    if one.clicked() && item.is_none() {
                        picked = Some(0);
                        item_changed = true;
                    }
                });
            });
            ui.end_row();
            if let Some(i) = item {
                let label = grid_label(ui, lw, "Ball it follows").on_hover_text("Which ball it rides, counted from 1");
                let mut n: u64 = i as u64 + 1;
                if ui.add(DragValue::new(&mut n).range(1..=10000)).labelled_by(label.id).changed() {
                    picked = Some(n.saturating_sub(1) as u32);
                    item_changed = true;
                }
                ui.end_row();
            }
        }
        if moved || turned || item_changed {
            edits.push(Edit::Attach(Attach::Ride { offset: o, turn_with_parent: turns, item: picked }));
        }
    }
}
