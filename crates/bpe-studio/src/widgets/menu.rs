//! A setting's right-click menu: Copy Value, Paste Value, Create Keyframe, Follow and Vary.

use super::*;

/// Whether the right mouse button is down or was just let go: a slider or number box under it is
/// opening the row's menu, not being dragged.
pub fn right_button_busy(ui: &Ui) -> bool {
    ui.input(|i| {
        let right = PointerButton::Secondary;
        i.pointer.button_down(right) || i.pointer.button_released(right) || i.pointer.button_pressed(right)
    })
}

/// A value copied from a setting's menu: the value and how it reads in the menu's Paste Value.
#[derive(Clone, Debug, PartialEq)]
pub struct Copied {
    pub value: Value,
    pub words: String,
    /// A gradient copied with its settings: each companion's name and value. Empty for every
    /// other copy.
    pub extras: Vec<(&'static str, Value)>,
}

/// Where the copied value lives: the editor's own clipboard, shared by every card and thing,
/// until the editor closes.
pub(super) fn clipboard_id() -> egui::Id {
    egui::Id::unique("setting value clipboard")
}

/// The value last copied from a setting's menu, if any.
pub fn copied(ctx: &egui::Context) -> Option<Copied> {
    ctx.data(|d| d.get_temp::<Copied>(clipboard_id()))
}

/// Puts `value` on the editor's clipboard.
pub fn copy_value(ctx: &egui::Context, value: Value, words: String) {
    copy_with(ctx, value, words, Vec::new());
}

/// Puts a gradient and its companions (`extras`, each by its name) on the editor's clipboard.
pub fn copy_with(ctx: &egui::Context, value: Value, words: String, extras: Vec<(&'static str, Value)>) {
    ctx.data_mut(|d| d.insert_temp(clipboard_id(), Copied { value, words, extras }));
}

/// What a row's menu asked for.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum MenuPick {
    Nothing,
    /// The copied value, made to fit the setting, or its default (Reset to default).
    Paste(Value),
    CreateKey,
    RemoveKeys,
    /// Follow sound ▸ …, or Stop following sound (`None`).
    Follow(Option<bpe_core::PulseSource>),
}

/// The right-click menu of one setting's row: Copy Value, Paste Value (naming what is copied) and
/// Create Keyframe. Paste is greyed out when nothing is copied or the copied value doesn't fit
/// this setting; Create Keyframe when the setting can't have keys.
pub(super) fn row_menu(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
    extra: Option<&mut dyn FnMut(&mut Ui)>,
) -> MenuPick {
    row_menu_following(response, def, unit, value, mark, choices, extra, None, None)
}

/// [`row_menu`] with Follow sound under Create Keyframe, when `follow` is given, and "Copy
/// gradient with its settings" under Copy Value when `with` gives a gradient's companions.
#[allow(clippy::too_many_arguments)]
pub(super) fn row_menu_following(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
    mut extra: Option<&mut dyn FnMut(&mut Ui)>,
    follow: Option<(&crate::follow::FollowCx, &str)>,
    with: Option<&dyn Fn() -> Vec<(&'static str, Value)>>,
) -> MenuPick {
    let mut pick = MenuPick::Nothing;
    let open = if response.container_secondary_clicked() {
        Some(egui::SetOpenCommand::Bool(true))
    } else if response.container_clicked() {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    // An id of its own: a menu button's own popup (a target or a choice) already uses the
    // response's usual popup id.
    let popup = egui::Popup::menu(response).id(response.id.with("setting menu")).open_memory(open).at_pointer_fixed();
    popup.show(|ui| {
        ui.set_min_width(160.0);
        if ui.button("Copy Value").on_hover_text("Copy this setting's value").clicked() {
            copy_value(ui.ctx(), value.clone(), value_words(value, unit, choices));
            ui.close();
        }
        if let Some(companions) = with {
            let help = "Copy the colours with how they blend, lie and move, to paste onto another gradient";
            if ui.button("Copy gradient with its settings").on_hover_text(help).clicked() {
                let words = format!("{} with its settings", value_words(value, unit, choices));
                copy_with(ui.ctx(), value.clone(), words, companions());
                ui.close();
            }
        }
        let clip = copied(ui.ctx());
        let text = match &clip {
            Some(c) => format!("Paste Value ({})", c.words),
            None => "Paste Value".to_owned(),
        };
        let fits = clip.as_ref().and_then(|c| pasted(def, &c.value));
        let why = if clip.is_none() { "Copy a value first" } else { "The copied value doesn't fit this setting" };
        let paste = ui.add_enabled(fits.is_some(), egui::Button::new(text)).on_disabled_hover_text(why);
        if paste.clicked()
            && let Some(v) = fits
        {
            pick = MenuPick::Paste(v);
            ui.close();
        }
        let default = &def.default;
        let reset = format!("Reset to default ({})", value_words(default, unit, choices));
        let reset = ui
            .add_enabled(value != default, egui::Button::new(reset))
            .on_hover_text("Put this setting back as it starts (also: double-click its name)")
            .on_disabled_hover_text("It is at its default already");
        if reset.clicked() {
            pick = MenuPick::Paste(default.clone());
            ui.close();
        }
        let can_key = mark.is_some_and(|m| m.can_key);
        let key = ui
            .add_enabled(can_key, egui::Button::new("Create Keyframe"))
            .on_hover_text("A keyframe at the playhead holding the value as it is")
            .on_disabled_hover_text("This setting can't have keyframes");
        if key.clicked() {
            pick = MenuPick::CreateKey;
            ui.close();
        }
        let keyed = mark.is_some_and(|m| m.keyed);
        let clear = ui
            .add_enabled(keyed, egui::Button::new("Remove all keyframes"))
            .on_hover_text("Every key of this setting goes; it keeps the value it has at the playhead")
            .on_disabled_hover_text("This setting has no keyframes");
        if clear.clicked() {
            pick = MenuPick::RemoveKeys;
            ui.close();
        }
        if let Some((f, key)) = follow {
            follow_items(ui, def, f, (f.now)(key), (f.light)(key), &mut pick);
        }
        if let Some(extra) = extra.as_mut() {
            extra(ui);
        }
    });
    pick
}

/// Follow sound ▸ (a shape ▸ its modulators and Level; the buses), and Stop following sound
/// when the setting follows already; then Follow light ▸ (a thing ▸ its light) with a Lighting
/// card, offered where `light_ok` (the lighting doesn't read the setting); then Follow health ▸
/// (a thing with a Health card).
pub(super) fn follow_items(
    ui: &mut Ui,
    def: &SettingDef,
    f: &crate::follow::FollowCx,
    now: Option<crate::follow::Following>,
    light_ok: bool,
    pick: &mut MenuPick,
) {
    use bpe_core::{PulseSource, SoundSource};
    let can = crate::follow::can_follow(def);
    // A setting the lighting reads can't follow a sound that hears the light (one-way).
    let lighting_reads = !f.menu.lit.is_empty() && !light_ok;
    let shapes: Vec<_> = f.menu.shapes.iter().filter(|s| !(lighting_reads && s.hears_light)).collect();
    let buses: &[(u8, String)] =
        if lighting_reads && f.menu.shapes.iter().any(|s| s.hears_light) { &[] } else { &f.menu.buses };
    let any = !shapes.is_empty() || !buses.is_empty();
    let why = if !can {
        "This setting can't follow sound"
    } else if !f.menu.shapes.is_empty() {
        "The lighting reads this setting, and the sound hears the light"
    } else {
        "No shape makes sound yet"
    };
    ui.add_enabled_ui(can && any, |ui| {
        let r = ui.menu_button("Follow sound", |ui| {
            for shape in &shapes {
                ui.menu_button(&shape.name, |ui| {
                    for (part, name) in &shape.parts {
                        let source = PulseSource::Sound { shape: shape.id, source: *part };
                        let on = now.as_ref().is_some_and(|n| n.source == source);
                        if ui.add(egui::Button::selectable(on, name)).clicked() {
                            *pick = MenuPick::Follow(Some(source));
                            ui.close();
                        }
                    }
                });
            }
            for (n, name) in buses {
                let shape = f.menu.shapes.first().map_or(ThingId::ROOT, |s| s.id);
                let source = PulseSource::Sound { shape, source: SoundSource::Bus(*n) };
                let on = now.as_ref().is_some_and(
                    |x| matches!(x.source, PulseSource::Sound { source: SoundSource::Bus(m), .. } if m == *n),
                );
                let label = if name.trim().is_empty() { format!("Bus {n}") } else { name.clone() };
                if ui.add(egui::Button::selectable(on, label)).clicked() {
                    *pick = MenuPick::Follow(Some(source));
                    ui.close();
                }
            }
        });
        r.response.on_hover_text("Move this setting with a shape's sound").on_disabled_hover_text(why);
    });
    let following_light = now.as_ref().is_some_and(|n| matches!(n.source, PulseSource::Light { .. }));
    let following_health = now.as_ref().is_some_and(|n| matches!(n.source, PulseSource::Health { .. }));
    if now.is_some() && !following_light && !following_health && ui.button("Stop following sound").clicked() {
        *pick = MenuPick::Follow(None);
        ui.close();
    }
    if !f.menu.lit.is_empty() {
        let may = can && light_ok;
        let why = if !can { "This setting can't follow light" } else { "The lighting reads this setting" };
        ui.add_enabled_ui(may, |ui| {
            let r = ui.menu_button("Follow light", |ui| {
                for thing in &f.menu.lit {
                    ui.menu_button(&thing.name, |ui| {
                        for (part, name) in &thing.parts {
                            let source = PulseSource::Light { thing: thing.id, source: *part };
                            let on = now.as_ref().is_some_and(|n| n.source == source);
                            if ui.add(egui::Button::selectable(on, *name)).clicked() {
                                *pick = MenuPick::Follow(Some(source));
                                ui.close();
                            }
                        }
                    });
                }
            });
            r.response.on_hover_text("Move this setting with how lit a thing is").on_disabled_hover_text(why);
        });
    }
    if following_light && ui.button("Stop following light").clicked() {
        *pick = MenuPick::Follow(None);
        ui.close();
    }
    if !f.menu.health.is_empty() {
        ui.add_enabled_ui(can, |ui| {
            let r = ui.menu_button("Follow health", |ui| {
                for (thing, name) in &f.menu.health {
                    let source = PulseSource::Health { thing: *thing };
                    let on = now.as_ref().is_some_and(|n| n.source == source);
                    if ui.add(egui::Button::selectable(on, name)).clicked() {
                        *pick = MenuPick::Follow(Some(source));
                        ui.close();
                    }
                }
            });
            r.response
                .on_hover_text("Move this setting with how much health a thing has left")
                .on_disabled_hover_text("This setting can't follow health");
        });
    }
    if following_health && ui.button("Stop following health").clicked() {
        *pick = MenuPick::Follow(None);
        ui.close();
    }
}

/// What the value menu on a control outside a settings grid (a knob) asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ValueMenu {
    Nothing,
    /// A pasted value, or the default (Reset to default).
    Paste(Value),
    CreateKey,
    RemoveKeys,
}

/// The same right-click menu as a settings row's (Copy Value, Paste Value, Reset to default,
/// Create Keyframe, Remove all keyframes), on any control.
pub(crate) fn value_menu(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
) -> ValueMenu {
    value_menu_with(response, def, unit, value, mark, choices, None)
}

/// [`value_menu`] with more items under Create Keyframe, drawn by `extra` (Modulate by).
pub(crate) fn value_menu_with(
    response: &Response,
    def: &SettingDef,
    unit: &str,
    value: &Value,
    mark: Option<KeyMark>,
    choices: &TargetChoices,
    extra: Option<&mut dyn FnMut(&mut Ui)>,
) -> ValueMenu {
    match row_menu(response, def, unit, value, mark, choices, extra) {
        MenuPick::Nothing | MenuPick::Follow(_) => ValueMenu::Nothing,
        MenuPick::Paste(v) => ValueMenu::Paste(v),
        MenuPick::CreateKey => ValueMenu::CreateKey,
        MenuPick::RemoveKeys => ValueMenu::RemoveKeys,
    }
}

/// `copied` as a value of `def`, kept inside its range, or `None` when it is another kind of
/// value or a choice this setting doesn't have.
pub(super) fn pasted(def: &SettingDef, copied: &Value) -> Option<Value> {
    let value = match (&def.ty, copied) {
        // A whole number pastes into a number and back, rounded.
        (ValueType::Number, Value::Int(i)) => Value::Number(*i as f64),
        (ValueType::Int, Value::Number(x)) if x.is_finite() => Value::Int(x.round() as i64),
        // A colour pastes into colours as one, and a one-colour gradient into a colour.
        (ValueType::Gradient, Value::Color(c)) => Value::Gradient(bpe_core::Gradient::solid(*c)),
        (ValueType::Color, Value::Gradient(g)) if g.stops.len() == 1 => Value::Color(g.stops[0].color),
        _ => copied.clone(),
    };
    def.ty.accepts(&value).then(|| def.clamp(value))
}
