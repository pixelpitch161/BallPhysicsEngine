//! One setting's row: its label, its widget for each value type, resetting, Follow bands and snapping.

use super::*;

/// One grid row for `def`, in `unit`: the label, wrapped to the grid's label width (help on
/// hover, in the accent colour when `mark` says the setting has keys or a pulse; a double-click
/// or Alt+click puts the default back), and the widget for its type, ending by `right`.
///
/// A right-click on the label or the widget opens the row's menu ([`row_menu`]) and never
/// changes the value, though sliders and number boxes otherwise move under any mouse button.
/// A colour or gradient keeps its own clicks, so only its label has the menu.
///
/// What was asked of its varying across the balls is added to `varies`: the menu's Vary across
/// balls or Stop varying, and, while it varies, its two-ended slider (or a To row under it for a
/// pair, colour or gradient) and the "Varies by" line.
#[allow(clippy::too_many_arguments)]
pub(super) fn setting(
    ui: &mut Ui,
    def: &SettingDef,
    unit: &str,
    value: &mut Value,
    mark: Option<KeyMark>,
    cx: &GridCx,
    right: f32,
    varies: &mut Vec<crate::vary::VaryEdit>,
    mut grad_row: Option<GradientRow>,
    trailing: Option<&mut dyn FnMut(&mut Ui)>,
) -> RowAction {
    use crate::vary::VaryEdit;
    // Where a gradient's bar and its tabs keep their memory (the selected stop, the open tab):
    // under the grid and the setting, so it holds however the rows around it change.
    let bar = ui.scope_id().with(("gradient bar", def.key));
    let offered = cx.vary.and_then(|v| v.use_of(def).map(|u| (v, u)));
    let now = offered.and_then(|(v, _)| v.now(def.key));
    let mut to = now.as_ref().and_then(|n| n.to.clone());
    let two_ended = matches!((&def.ty, def.range.or(def.span)), (ValueType::Number | ValueType::Int, Some(_)));
    let ink = label_colour(mark, cx.track);
    let differs = *value != def.default;
    let name = label_cell(ui, cx.label_width, |ui| {
        // The cell's full height, so the label is centred in it rather than hanging from its middle.
        let room = ui.available_size();
        ui.allocate_ui_with_layout(room, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            // A ring before the label when this thing changed the setting from its default.
            let (dot, _) = ui.allocate_exact_size(Vec2::new(6.0, 10.0), Sense::hover());
            if differs {
                style::changed_dot(ui.painter(), dot.center());
            }
            label(ui, def.label, def.help, ink)
        })
        .inner
    });
    let before = value.clone();
    let mut changed = false;
    let mut to_response: Option<Response> = None;
    let response = widget_cell(ui, right, |ui| {
        let mut draw = |ui: &mut Ui| {
            style::wells(ui, |ui| match (two_ended, to.as_mut()) {
                (true, Some(end)) => {
                    let range = def.range.or(def.span).unwrap_or((0.0, 1.0));
                    let feel = setting_feel(def).with_snap(setting_snapper(ui, def, unit));
                    let (a, b) = crate::vary::two_ended(ui, def, (value, end), range, feel, suffix(unit), name.id);
                    to_response = Some(b);
                    a
                }
                _ => {
                    let offers = offers_of(cx, def);
                    let offers = offers.as_ref().map(|o| o as &Offers);
                    widget(ui, def, unit, value, name.id, bar, cx.choices, grad_row.is_some(), offers)
                }
            })
        };
        let r = match trailing {
            Some(trailing) => {
                // The widget in all but the room kept for the trailing control.
                let room = (ui.available_width() - TRAILING_W).max(40.0);
                let layout = egui::Layout::left_to_right(egui::Align::Center);
                let r = ui
                    .allocate_ui_with_layout(Vec2::new(room, 0.0), layout, |ui| {
                        ui.set_max_width(room);
                        draw(ui)
                    })
                    .inner;
                trailing(ui);
                r
            }
            None => draw(ui),
        };
        changed |= r.changed();
        r
    });
    let own_clicks = matches!(def.ty, ValueType::Color | ValueType::Gradient);
    if changed && !own_clicks && right_button_busy(ui) {
        *value = before.clone();
        changed = false;
    }
    // Only the label resets (double-click or Alt+click), and the menu's Reset to default: a
    // double-click on a number box starts typing into it, and on a gradient bar adds a stop.
    let fallback = def.default.clone();
    if resettable(&name, value, &fallback, true) {
        changed = true;
    }
    let follow = cx.follow.map(|f| (f, def.key));
    if let Some(now) = follow.and_then(|(f, key)| (f.now)(key)) {
        follow_band(ui, def, value, &response, &now);
    }
    // Vary across balls (or Stop varying) under the menu's other items.
    let vary_pick = std::cell::Cell::new(None::<VaryEdit>);
    let mut vary_items = |ui: &mut Ui| {
        if offered.is_none() {
            return;
        }
        if now.is_some() || !cx.vary.is_some_and(|v| v.offered) {
            if now.is_some() && ui.button("Stop varying").on_hover_text("Every ball takes this value again").clicked() {
                vary_pick.set(Some(VaryEdit::Stop));
                ui.close();
            }
        } else if ui
            .button("Vary across balls")
            .on_hover_text("Give each ball its own value, between this one and another")
            .clicked()
        {
            vary_pick.set(Some(VaryEdit::Start));
            ui.close();
        }
    };
    let with = grad_row.as_ref().map(|g| g.companions);
    let mut picks =
        vec![row_menu_following(&name, def, unit, &before, mark, cx.choices, Some(&mut vary_items), follow, with)];
    if !own_clicks {
        picks.push(row_menu_following(
            &response,
            def,
            unit,
            &before,
            mark,
            cx.choices,
            Some(&mut vary_items),
            follow,
            with,
        ));
    }
    varies.extend(vary_pick.take());
    ui.end_row();
    // A gradient's tabs, in rows under it.
    if let Some(row) = grad_row.as_mut()
        && (row.tabs)(ui, bar, value)
    {
        changed = true;
    }
    if let (Some((vcx, use_)), Some(v), Some(end)) = (offered, now.as_ref(), to.as_mut()) {
        let to_mark = vcx.to_mark(def);
        let end_before = end.clone();
        // A pair, colour or gradient sets its To in a row of its own.
        if !two_ended {
            let to_name = label_cell(ui, cx.label_width, |ui| {
                let room = ui.available_size();
                ui.allocate_ui_with_layout(room, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add_space(9.0);
                    let words = format!("{} to", def.label);
                    let r = ui.add(egui::Label::new(RichText::new("to").color(label_colour(Some(to_mark), cx.track))));
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, words.clone()));
                    r.on_hover_text("The other end: what the last ball (or the far end of the spread) gets")
                })
                .inner
            });
            let r = widget_cell(ui, right, |ui| {
                style::wells(ui, |ui| widget(ui, def, unit, end, to_name.id, bar.with("to"), cx.choices, false, None))
            });
            if r.changed() {
                varies.push(VaryEdit::To(end.clone()));
            }
            to_response = Some(to_name);
            ui.end_row();
        } else if to_response.as_ref().is_some_and(|r| r.changed()) && !right_button_busy(ui) {
            varies.push(VaryEdit::To(end.clone()));
        }
        if let Some(r) = &to_response {
            match row_menu(r, def, unit, &end_before, Some(to_mark), cx.choices, None) {
                MenuPick::Paste(pasted) => varies.push(VaryEdit::To(pasted)),
                MenuPick::CreateKey => varies.push(VaryEdit::KeyTo),
                MenuPick::RemoveKeys => varies.push(VaryEdit::ClearTo),
                MenuPick::Nothing | MenuPick::Follow(_) => {}
            }
        }
        label_cell(ui, cx.label_width, |_| {});
        widget_cell(ui, right, |ui| {
            varies.extend(crate::vary::varies_line(ui, def, v, use_, vcx.rounds(), vcx.lit(def), vcx.healthy()));
        });
        ui.end_row();
    }
    let mut action = if changed { RowAction::Changed } else { RowAction::Nothing };
    for pick in picks {
        match pick {
            MenuPick::Nothing => {}
            MenuPick::Paste(pasted) => {
                *value = pasted;
                action = RowAction::Changed;
                // A gradient copied with its settings brings the ones this row has too.
                if let Some(row) = grad_row.as_mut()
                    && let Some(copy) = copied(ui.ctx())
                {
                    row.pasted.extend(companions_pasted(def, &copy.extras));
                }
            }
            MenuPick::CreateKey => action = RowAction::CreateKey,
            MenuPick::RemoveKeys => action = RowAction::RemoveKeys,
            MenuPick::Follow(source) => action = RowAction::Follow(source),
        }
    }
    action
}

/// The companions of `extras` (each by its name, as "Copy gradient with its settings" copied
/// them) that gradient setting `def` has, each made to fit; the others are left out.
pub(super) fn companions_pasted(def: &SettingDef, extras: &[(&'static str, Value)]) -> Vec<(&'static str, Value)> {
    extras
        .iter()
        .filter_map(|(name, v)| {
            let c = bpe_core::gradient_extras::companions_for(def).find(|c| c.name == *name)?;
            let v = pasted(&c.def, v)?;
            Some((c.name, v))
        })
        .collect()
}

/// The band under a following setting's slider: from its value to where the sound takes it at
/// full level, in the colour of what it follows, named "Follows <what>" for the screen reader.
pub(super) fn follow_band(
    ui: &mut Ui,
    def: &SettingDef,
    value: &Value,
    response: &Response,
    now: &crate::follow::Following,
) {
    let rail = ui.ctx().data(|d| d.get_temp::<egui::Rect>(response.id.with("rail"))).unwrap_or(response.rect);
    let x = match value {
        Value::Number(x) => *x,
        Value::Int(x) => *x as f64,
        _ => 0.0,
    };
    let (lo, hi) = def.span.or(def.range).unwrap_or((x.min(x + now.amount), x.max(x + now.amount)));
    let at = |v: f64| {
        let t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
        rail.left() + rail.width() * t as f32
    };
    let (a, b) = (at(x), at(x + now.amount));
    let band =
        egui::Rect::from_x_y_ranges(a.min(b)..=a.max(b).max(a.min(b) + 2.0), rail.bottom() - 3.0..=rail.bottom());
    let r = ui.interact(band, response.id.with("follow band"), Sense::hover());
    let words = format!("Follows {}", now.words);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, words.clone()));
    r.on_hover_text(&words);
    ui.painter().rect_filled(band, 1.5, now.colour);
}

/// Whether the response was double-clicked or (with `alt_too`) Alt+clicked, which puts a setting
/// back.
pub(super) fn reset_clicked(response: &Response, alt_too: bool) -> bool {
    let primary = egui::PointerButton::Primary;
    // egui counts a double-click that comes soon after a click elsewhere as a triple-click.
    let (alt, clicked, many_clicks) = response.ctx.input(|i| {
        let many = i.pointer.button_double_clicked(primary) || i.pointer.button_triple_clicked(primary);
        (i.modifiers.alt, i.pointer.button_clicked(primary), many)
    });
    // Sliders only sense drags, so a click on one shows up as a drag that stops where it started.
    let clicked = response.clicked() || (response.drag_stopped() && clicked);
    (clicked && alt && alt_too) || (clicked && many_clicks && !response.ctx.text_edit_focused())
}

/// Puts `value` back to `fallback` on a double-click or (with `alt_too`) Alt+click. True when it
/// changed.
pub(super) fn resettable(response: &Response, value: &mut Value, fallback: &Value, alt_too: bool) -> bool {
    if reset_clicked(response, alt_too) && value != fallback {
        *value = fallback.clone();
        true
    } else {
        false
    }
}

/// The span slider for a setting's number: over its usual span, clamped only by its hard range.
pub(super) fn spanned(
    ui: &mut Ui,
    def: &SettingDef,
    x: &mut f64,
    span: (f64, f64),
    feel: NumberFeel,
    unit: String,
    name: egui::Id,
) -> Response {
    let integer = def.ty == ValueType::Int;
    span_slider(ui, x, span, def.range, feel, integer, unit, Named::By(name))
}

/// How `def`, shown in `unit`, snaps to the grid while dragged right now: never the video's own
/// size (a size of the picture, not a distance in it), nor a slider that couldn't stop at four
/// steps across its span (a 0 to 10 px width at a 60 px grid would only take 0 and 7.5).
pub(super) fn setting_snapper(ui: &Ui, def: &SettingDef, unit: &str) -> Option<crate::setting_snap::Snapper> {
    if def.group == "Video" {
        return None;
    }
    let snap = crate::setting_snap::Snapper::for_unit(ui, unit, def.ty == ValueType::Vec2)?;
    match def.span.or(def.range) {
        Some((a, b)) if !snap.fits((a, b)) => None,
        _ => Some(snap),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn widget(
    ui: &mut Ui,
    def: &SettingDef,
    unit: &str,
    value: &mut Value,
    name: egui::Id,
    bar: egui::Id,
    choices: &TargetChoices,
    big: bool,
    offers: Option<&Offers>,
) -> Response {
    use crate::setting_snap::after_drag;
    // Distances and angles land on the grid while it snaps (and Alt isn't held).
    let snap = setting_snapper(ui, def, unit);
    let velocity = unit == VELOCITY;
    let unit = suffix(unit);
    let feel = setting_feel(def).with_snap(snap);
    match (&def.ty, value) {
        (ValueType::Vec2, Value::Vec2(v)) if velocity => speed_and_angle(ui, v, feel.decimals, name),
        (ValueType::Number, Value::Number(x)) => match (def.span, def.range) {
            (Some(span), _) => spanned(ui, def, x, span, feel, unit, name),
            (None, Some(range)) => {
                let before = *x;
                let r = number_slider(ui, x, range, feel, unit).labelled_by(name);
                after_drag(ui, &r, before, x, snap, false, Some(range));
                r
            }
            (None, None) => {
                let before = *x;
                let r = ui
                    .add(DragValue::new(x).speed(feel.drag_speed).fixed_decimals(feel.decimals).suffix(unit))
                    .labelled_by(name);
                after_drag(ui, &r, before, x, snap, false, None);
                r
            }
        },
        (ValueType::Int, Value::Int(x)) => match (def.span, def.range) {
            (Some(span), _) => {
                let mut v = *x as f64;
                let r = spanned(ui, def, &mut v, span, feel, unit, name);
                *x = v.round() as i64;
                r
            }
            (None, Some(range)) => {
                let before = *x as f64;
                let r = number_slider(ui, x, range, feel, unit).labelled_by(name);
                let mut v = *x as f64;
                if after_drag(ui, &r, before, &mut v, snap, true, Some(range)) {
                    *x = v as i64;
                }
                r
            }
            (None, None) => {
                let before = *x as f64;
                let r = ui.add(DragValue::new(x).speed(feel.drag_speed).suffix(unit)).labelled_by(name);
                let mut v = *x as f64;
                if after_drag(ui, &r, before, &mut v, snap, true, None) {
                    *x = v as i64;
                }
                r
            }
        },
        (ValueType::Bool, Value::Bool(b)) => style::switch(ui, b).labelled_by(name),
        // In a narrow panel y goes under x.
        (ValueType::Vec2, Value::Vec2(v)) => {
            ui.horizontal_wrapped(|ui| {
                let (speed, decimals) = (feel.drag_speed, feel.decimals);
                let before = *v;
                let x =
                    DragValue::new(&mut v[0]).speed(speed).fixed_decimals(decimals).prefix("x ").suffix(unit.clone());
                let x = ui.add(x).labelled_by(name);
                let y = DragValue::new(&mut v[1]).speed(speed).fixed_decimals(decimals).prefix("y ").suffix(unit);
                let y = ui.add(y).labelled_by(name);
                // A place lands on grid points while the grid snaps.
                for (i, r) in [&x, &y].into_iter().enumerate() {
                    let mut c = v[i] as f64;
                    if after_drag(ui, r, before[i] as f64, &mut c, snap, false, None) {
                        v[i] = c as _;
                    }
                }
                x | y
            })
            .inner
        }
        (ValueType::Color, Value::Color(c)) => color_button(ui, c).labelled_by(name),
        (ValueType::Gradient, Value::Gradient(g)) => {
            // The bar and its handles carry their own names, after the setting's.
            gradient_bar_named(ui, bar, g, def.label, big)
        }
        (ValueType::Choice(names), Value::Choice(current)) => {
            let mut response = style::raised(ui, |ui| {
                egui::ComboBox::from_id_salt(def.key).selected_text(pretty(current)).show_ui(ui, |ui| {
                    let mut picked = false;
                    // Only the choices that make sense where the thing is (the one picked always shows above).
                    for choice in names.iter().filter(|c| offers.is_none_or(|o| o(c))) {
                        picked |= ui.selectable_value(current, (*choice).to_owned(), pretty(choice)).changed();
                    }
                    picked
                })
            });
            if response.inner.unwrap_or(false) {
                response.response.mark_changed();
            }
            response.response.labelled_by(name)
        }
        (ValueType::Text, Value::Text(text)) if def.unit == "picture" => {
            crate::picture_picker::picture_row(ui, text, &choices.pictures, def.label, name)
        }
        (ValueType::Text, Value::Text(text)) if def.unit == "file" => {
            // Leave room for the browse button, or the row grows past the panel.
            let width = (ui.available_width() - 40.0).max(80.0);
            let mut response = ui.add(egui::TextEdit::singleline(text).desired_width(width)).labelled_by(name);
            let browse = ui.add(egui::Button::new("…").small()).on_hover_text("Choose a file");
            browse.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Browse {}", def.label)));
            if browse.clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                *text = path.to_string_lossy().into_owned();
                response.mark_changed();
            }
            response
        }
        // A Text's words: any number of lines.
        (ValueType::Text, Value::Text(text)) if def.unit == "lines" => {
            ui.vertical(|ui| crate::words_box::words_box(ui, text, choices, name)).inner
        }
        (ValueType::Text, Value::Text(text)) if def.unit == "font" => crate::font_picker::font_row(
            ui,
            text,
            &crate::font_picker::FontCx { scene_fonts: &choices.fonts, unreadable: &choices.unreadable_fonts },
            name,
        ),
        (ValueType::Text, Value::Text(text)) => {
            ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY)).labelled_by(name)
        }
        (ValueType::Target, Value::Target(t)) => target_picker(ui, t, choices).labelled_by(name),
        (ValueType::Who, Value::Who(w)) => ui.horizontal(|ui| who_picker(ui, w, choices)).inner.labelled_by(name),
        // Strokes are painted in the ball view; here they are counted and can be cleared.
        (ValueType::Strokes, Value::Strokes(strokes)) => {
            let words = strokes_words(strokes);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(words).color(style::TEXT))).labelled_by(name);
                let clear = ui.add_enabled(!strokes.list.is_empty(), egui::Button::new("Clear").small());
                let label = format!("Clear {}", def.label);
                clear.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !strokes.list.is_empty(), &label));
                let mut clear = clear.on_hover_text("Takes away every stroke. Paint them in the ball view above.");
                if clear.clicked() {
                    strokes.list.clear();
                    clear.mark_changed();
                }
                clear
            })
            .inner
        }
        // A shape's outline is moulded in the video; the row says what it is and can start it afresh.
        (ValueType::Path, Value::Path(path)) => {
            let words = path_words(path);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(words).color(style::TEXT))).labelled_by(name);
                let moulded = !path.is_empty();
                let reset = ui.add_enabled(moulded, egui::Button::new("Reset").small());
                let label = format!("Reset {}", def.label);
                reset.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, moulded, &label));
                let mut reset = reset
                    .on_hover_text("Puts the even polygon back")
                    .on_disabled_hover_text("Drag corners and sides in the video to mould it");
                if reset.clicked() {
                    *path = bpe_core::ShapePath::default();
                    reset.mark_changed();
                }
                reset
            })
            .inner
        }
        // Points are placed by dragging them in a view; the row only says how many there are.
        (ValueType::Points, Value::Points(ps)) => {
            let words = if ps.len() == 1 { "1 point".to_owned() } else { format!("{} points", ps.len()) };
            ui.label(RichText::new(words).color(style::MUTED)).labelled_by(name)
        }
        // Lighting scenes are listed and edited under the Lighting card's settings.
        (ValueType::LightScenes, Value::LightScenes(list)) => {
            ui.label(RichText::new(scenes_words(list.len())).color(style::MUTED)).labelled_by(name)
        }
        _ => ui.label(RichText::new("This setting holds a value of the wrong kind").color(style::TEXT)),
    }
}

/// The unit of a velocity: a pair in it shows as a speed and an angle.
pub(super) const VELOCITY: &str = "px/s";

/// A velocity's speed (px/s) and angle (degrees, 0 to the right, 90 down).
pub(super) fn speed_angle(v: [f32; 2]) -> (f64, f64) {
    let (x, y) = (f64::from(v[0]), f64::from(v[1]));
    (x.hypot(y), y.atan2(x).to_degrees())
}

/// A velocity as a speed and an angle (W12), as the Emitter and Particles set theirs. It is still
/// saved as x/y, written only when one of the two is changed, so an untouched pair keeps its exact
/// numbers. The angle of a still pair is remembered while it is still.
pub(super) fn speed_and_angle(ui: &mut Ui, v: &mut [f32; 2], decimals: usize, name: egui::Id) -> Response {
    let memory = name.with("still angle");
    let (len, turned) = speed_angle(*v);
    let mut speed = len;
    let mut angle = if len > 0.0 { turned } else { ui.data(|d| d.get_temp::<f64>(memory)).unwrap_or(0.0) };
    let response = ui
        .horizontal_wrapped(|ui| {
            let s = DragValue::new(&mut speed).range(0.0..=1e9).speed(2.0).fixed_decimals(decimals).suffix(" px/s");
            let s = ui.add(s).on_hover_text("How fast").labelled_by(name);
            let a = DragValue::new(&mut angle).speed(0.5).fixed_decimals(0).prefix("at ").suffix("°");
            let a = ui.add(a).on_hover_text("Which way: 0° is right, 90° down, -90° up").labelled_by(name);
            s | a
        })
        .inner;
    if response.changed() && speed.is_finite() && angle.is_finite() {
        let angle = (angle + 180.0).rem_euclid(360.0) - 180.0;
        let (sin, cos) = angle.to_radians().sin_cos();
        *v = [(speed * cos) as f32, (speed * sin) as f32];
        ui.data_mut(|d| d.insert_temp(memory, angle));
    }
    response
}
