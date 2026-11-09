//! A Look card's Surface section: how the thing takes light (lighting's surfaces).

use super::*;

/// A Look card's Surface section (lighting part 2a): a chip per Surface, each a painted ball,
/// which sets the numbers under it in the same edit, then those numbers (folded until opened;
/// Bulge only on balls).
pub(super) fn surface_section(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    slot: SlotId,
    ball: bool,
    edits: &mut Vec<Edit>,
) {
    use bpe_core::Choice;
    use bpe_kit::modules::look::surface::{self, SurfaceKind, preset_values};
    const GROUP: &str = "Surface";
    let (scene, reg, id, time) = (cx.scene, &cx.kit.registry, cx.id, cx.time);
    let at = |key: &str| Address::on(id, slot, key);
    let get = |key: &str| scene.value_at_time(reg, &at(key), time).expect("a Look card's Surface settings resolve");
    let now = match get("surface") {
        Value::Choice(name) => SurfaceKind::from_name(&name).unwrap_or_default(),
        _ => SurfaceKind::default(),
    };
    let open = !is_folded(inspector, (id, Some(slot)), GROUP);
    ui.horizontal(|ui| {
        if fold_toggle(ui, open, GROUP) {
            fold_groups(inspector, (id, Some(slot)), &[(GROUP, open)]);
        }
        ui.label(RichText::new(GROUP).font(style::semibold(12.0)));
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (i, name) in SurfaceKind::NAMES.iter().enumerate() {
            let kind = SurfaceKind::from_name(name).expect("one of the names");
            if surface_chip(ui, i, &crate::widgets::pretty(name), kind == now).clicked() && kind != now {
                edits.push(Edit::SetSlot(slot, "surface".into(), Value::Choice((*name).into())));
                for (key, value) in preset_values(kind) {
                    edits.push(Edit::SetSlot(slot, key.into(), value));
                }
            }
        }
    });
    if !scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Lighting)) {
        ui.label(RichText::new("Surfaces show once the scene has lighting (add a light).").small().color(style::MUTED));
    }
    if !open {
        return;
    }
    let defs: Vec<SettingDef> = surface::defs()
        .iter()
        .filter(|d| d.key != "surface" && (ball || d.key != "bulge"))
        .map(|d| SettingDef { group: "", ..d.clone() })
        .collect();
    let marks = |key: &str| {
        let def = crate::widgets::def_in(&defs, key)?;
        Some(mark_for(scene, def, &at(key)))
    };
    let applies = |d: &SettingDef| d.applies(Some(&cx.thing.kind), &|k| scene.value_at_time(reg, &at(k), time));
    let follow_menu = crate::follow::menu(scene);
    let follow_now = |key: &str| crate::follow::now(scene, &at(key));
    let follow_light = |key: &str| crate::follow::can_light(scene, reg, &at(key));
    let follow = crate::follow::FollowCx { menu: &follow_menu, now: &follow_now, light: &follow_light };
    let grid = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: &applies,
        folded: None,
        label_width: cx.label_width,
        track: style::colour_of(scene, id),
        follow: Some(&follow),
        vary: None,
        offers: None,
        rails: None,
    };
    let out = settings_grid(ui, ("surface", id, slot), &defs, get, &grid);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(Some(slot), key.into())));
    edits.extend(out.cleared.into_iter().map(|key| Edit::ClearKeys(Some(slot), key.into())));
    edits.extend(out.follows.into_iter().map(|(key, source)| Edit::Follow(Some(slot), key.into(), source)));
    ui.add_space(4.0);
}

/// One Surface chip: Surface number `kind` painted as a little ball, then its name; outlined in
/// the text colour when picked.
pub(super) fn surface_chip(ui: &mut Ui, kind: usize, name: &str, on: bool) -> egui::Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = ui.painter().layout_no_wrap(name.to_owned(), font, if on { style::TEXT_STRONG } else { style::TEXT });
    let size = egui::vec2(26.0 + text.size().x + 8.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    // Named as a surface: a Lava surface beside a Lava effect.
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, format!("{name} surface")));
    let response = response.on_hover_text(format!("Make it {}", name.to_lowercase()));
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, style::RADIUS, style::HOVER);
    }
    let edge = if on { style::TEXT } else { style::LINE };
    painter.rect_stroke(rect, style::RADIUS, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    style::surface_swatch(painter, egui::pos2(rect.left() + 13.0, rect.center().y), 7.5, kind);
    painter.galley(egui::pos2(rect.left() + 26.0, rect.center().y - text.size().y / 2.0), text, style::TEXT);
    response
}
