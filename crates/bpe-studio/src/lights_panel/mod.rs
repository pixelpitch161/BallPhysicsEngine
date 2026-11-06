//! The Lights tab (lighting spec, layout B): every source of light in the scene in one place.
//! At the top the scene's Depth and Quality and buttons that add a light; then a strip per Light
//! thing (under the folder it sits in), per Gives off light sub-layer, and Ambient last. A strip
//! has a Strength fader (the real setting at the playhead), M (turns the light off, saved) and S
//! (solo: the editor's alone, never saved or exported).

mod strip;

use std::collections::BTreeSet;

use bpe_core::{Address, LayerId, Registry, Resolved, Scene, Settings, SlotId, ThingId, Value, ValueType, layered_key};
use bpe_kit::modules::lighting::LIGHTING;
use bpe_kit::things::light::{LIGHT, LightSettings, LightType};
use eframe::egui::{self, RichText, Ui};

use crate::app::Studio;
use crate::style;

/// The layer kinds that give off light (lighting part 1b's Gives off light sub-layer on ball and
/// shape layers). A layer of one of these kinds that the registry knows gets a strip.
pub const GIVES_OFF_LIGHT: [&str; 2] = ["sub.light", "shape.sub.light"];

/// One source of light, as the Lights tab lists and solos it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LightSource {
    /// A Light thing.
    Light(ThingId),
    /// A Gives off light layer on a thing's card.
    Layer(ThingId, SlotId, LayerId),
    /// The scene's ambient light (its Lighting card).
    Ambient,
}

/// A source as a strip shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub source: LightSource,
    /// "Torch", "Drifters › Body", "Ambient".
    pub name: String,
    /// The small words after the name: "spot · on Drifters", "gives off".
    pub what: String,
    /// The folder (or rig) it is listed under.
    pub group: Option<String>,
    /// The thing whose track colour tags the strip (none for Ambient).
    pub thing: Option<ThingId>,
    /// Its colour as sRGB bytes, for the dot.
    pub colour: Option<[u8; 3]>,
    /// On: not muted.
    pub on: bool,
    /// The setting its fader moves.
    pub strength: Option<Address>,
}

/// Every source of light in the scene, in tree order, Ambient last (when the scene is lit).
pub fn sources(scene: &Scene, reg: &Registry) -> Vec<Source> {
    let mut out = Vec::new();
    for &id in &scene.order {
        let Some(thing) = scene.things.get(&id) else { continue };
        if thing.kind == LIGHT {
            let s = LightSettings::from_params(&thing.params);
            let parent = thing.parent.and_then(|p| scene.things.get(&p)).filter(|p| p.id != ThingId::ROOT);
            let kind = kind_word(s.kind);
            let rides = parent.filter(|p| bpe_kit::things::light::RIDES.contains(&p.kind.as_str()));
            let what = match rides {
                Some(p) => format!("{kind} · on {}", p.name),
                None => kind.to_owned(),
            };
            let group = parent.filter(|p| rides.is_none() && p.kind != "scene").map(|p| p.name.clone());
            out.push(Source {
                source: LightSource::Light(id),
                name: thing.name.clone(),
                what,
                group,
                thing: Some(id),
                colour: Some(s.display_colour()),
                on: thing.enabled,
                strength: Some(Address::own(id, "strength")),
            });
            continue;
        }
        for (&slot, card) in &thing.slots {
            for layer in card.layers.iter().filter(|l| GIVES_OFF_LIGHT.contains(&l.kind.as_str())) {
                if reg.layer(&layer.kind).is_none() {
                    continue;
                }
                let parent = layer.parent.and_then(|p| card.layer(p));
                let parent_name = parent.map(|p| layer_name(reg, &p.name, &p.kind)).unwrap_or_default();
                let name = if parent_name.is_empty() {
                    format!("{} › {}", thing.name, layer_name(reg, &layer.name, &layer.kind))
                } else {
                    format!("{} › {parent_name}", thing.name)
                };
                let key = layered_key(layer.id, "strength");
                let address = Address::on(id, slot, &key);
                let colour = match card.params.get(&layered_key(layer.id, "colour")) {
                    Some(Value::Color(c)) => Some([c.r, c.g, c.b]),
                    _ => None,
                };
                out.push(Source {
                    source: LightSource::Layer(id, slot, layer.id),
                    name,
                    what: "gives off".into(),
                    group: None,
                    thing: Some(id),
                    colour,
                    on: layer.enabled && thing.enabled,
                    strength: scene.def_at(reg, &address).map(|_| address),
                });
            }
        }
    }
    if let Some(card) = scene.card(ThingId::ROOT, SlotId::Lighting).filter(|c| c.kind == LIGHTING) {
        let on = !matches!(card.params.get("ambient_on"), Some(Value::Bool(false)));
        let colour = match card.params.get("ambient_colour") {
            Some(Value::Color(c)) => [c.r, c.g, c.b],
            _ => [0xff, 0xff, 0xff],
        };
        out.push(Source {
            source: LightSource::Ambient,
            name: "Ambient".into(),
            what: String::new(),
            group: None,
            thing: None,
            colour: Some(colour),
            on,
            strength: Some(Address::on(ThingId::ROOT, SlotId::Lighting, "ambient")),
        });
    }
    out
}

/// A layer's name: its own, else its kind's.
fn layer_name(reg: &Registry, name: &str, kind: &str) -> String {
    if !name.is_empty() {
        return name.to_owned();
    }
    reg.layer(kind).map(|k| k.name().to_owned()).unwrap_or_else(|| kind.to_owned())
}

/// The lamp's kind in a word, as the strip shows it.
fn kind_word(kind: LightType) -> &'static str {
    match kind {
        LightType::Point => "point",
        LightType::Spot => "spot",
        LightType::Sun => "sun",
        LightType::Area => "area",
        LightType::Strip => "strip",
    }
}

/// The resolved scene the preview draws while some strips are soloed: the Light things and Gives
/// off light layers not soloed are left out, and ambient is off unless it is soloed. `None` when
/// nothing that is soloed is still in the scene (the picture as it is).
pub fn soloed(scene: &Scene, resolved: &Resolved, solo: &BTreeSet<LightSource>) -> Option<Resolved> {
    let live = |s: &LightSource| match s {
        LightSource::Light(id) => scene.things.get(id).is_some_and(|t| t.kind == LIGHT),
        LightSource::Layer(id, slot, layer) => scene.card(*id, *slot).is_some_and(|c| c.layer(*layer).is_some()),
        LightSource::Ambient => scene.card(ThingId::ROOT, SlotId::Lighting).is_some(),
    };
    if !solo.iter().any(live) {
        return None;
    }
    let mut out = resolved.clone();
    out.things.retain(|id, r| r.kind != LIGHT || solo.contains(&LightSource::Light(*id)));
    for (id, r) in out.things.iter_mut() {
        for (slot, card) in r.slots.iter_mut() {
            card.layers.retain(|l| {
                !GIVES_OFF_LIGHT.contains(&l.kind.as_str()) || solo.contains(&LightSource::Layer(*id, *slot, l.id))
            });
        }
    }
    if !solo.contains(&LightSource::Ambient)
        && let Some(card) = out.things.get_mut(&ThingId::ROOT).and_then(|r| r.slots.get_mut(&SlotId::Lighting))
    {
        // Not a keyframable setting, so the picture's keys can't turn it back on.
        card.params.set("ambient_on", Value::Bool(false));
    }
    Some(out)
}

/// The names of what is soloed, for the preview's "Solo: …" pill.
pub fn solo_names(scene: &Scene, reg: &Registry, solo: &BTreeSet<LightSource>) -> Vec<String> {
    sources(scene, reg).into_iter().filter(|s| solo.contains(&s.source)).map(|s| s.name).collect()
}

/// Where the Lights tab's "+ light" buttons and the tree's Add thing ▸ Light put a new light:
/// inside a selected folder or a thing it can ride, else beside the selection.
pub fn light_parent(scene: &Scene, primary: Option<ThingId>) -> ThingId {
    primary
        .and_then(|s| scene.things.get(&s))
        .and_then(|t| {
            let inside = t.kind == "folder" || bpe_kit::things::light::RIDES.contains(&t.kind.as_str());
            if inside { Some(t.id) } else { t.parent }
        })
        .unwrap_or(ThingId::ROOT)
}

/// Draws the Lights tab.
pub fn show(ui: &mut Ui, studio: &mut Studio) {
    let kit = studio.kit.clone();
    let reg = &kit.registry;
    let lit = studio.scene.card(ThingId::ROOT, SlotId::Lighting).is_some_and(|c| c.kind == LIGHTING);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if lit {
            card_chips(ui, studio, reg, "depth", "Depth");
            card_chips(ui, studio, reg, "quality", "Quality");
        } else {
            ui.label(
                RichText::new("The picture isn't lit. Add Lighting, or a light, to light it.").color(style::MUTED),
            );
            let add = ui.add(style::primary_button("Add Lighting")).on_hover_text(
                "Gives the scene its Lighting card: ambient light, shadows and quality. Then add lights.",
            );
            if add.clicked() && studio.scene.add_slot(reg, ThingId::ROOT, SlotId::Lighting, LIGHTING).is_ok() {
                studio.panels.inspector.own_step = true;
                studio.status = "Added the Lighting card.".into();
            }
        }
        ui.add_space(6.0);
        add_buttons(ui, studio, reg);
        ui.add_space(8.0);
        let list = sources(&studio.scene, reg);
        // Solo only what is still there.
        studio.light_solo.retain(|s| list.iter().any(|x| x.source == *s));
        if list.is_empty() {
            ui.label(RichText::new("No lights yet.").color(style::MUTED));
        }
        let mut group: Option<&str> = None;
        for source in &list {
            if source.group.as_deref() != group {
                group = source.group.as_deref();
                if let Some(name) = group {
                    ui.add_space(4.0);
                    ui.label(RichText::new(name.to_uppercase()).small().color(style::MUTED));
                }
            }
            strip::show(ui, studio, reg, source);
            ui.add_space(4.0);
        }
    });
}

/// The Lighting card's choice setting `key` as a row of chips.
fn card_chips(ui: &mut Ui, studio: &mut Studio, reg: &Registry, key: &str, label: &str) {
    let a = Address::on(ThingId::ROOT, SlotId::Lighting, key);
    let Some(def) = studio.scene.def_at(reg, &a) else { return };
    let ValueType::Choice(names) = &def.ty else { return };
    let now = studio.scene.value_at(reg, &a);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let name = ui.add_sized([62.0, 22.0], egui::Label::new(RichText::new(label).color(style::MUTED)));
        let name = name.on_hover_text(def.help);
        for choice in names.iter() {
            let on = now == Some(Value::Choice((*choice).to_owned()));
            let chip =
                ui.add(style::toggle_button(&crate::widgets::pretty(choice), on).min_size(egui::vec2(0.0, 22.0)));
            let chip = chip.labelled_by(name.id);
            if chip.clicked() && !on && studio.scene.set_at(reg, &a, Value::Choice((*choice).to_owned())).is_ok() {
                studio.panels.inspector.own_step = true;
            }
        }
    });
}

/// "+ Point", "+ Spot"…: adds a light where the tree would and selects it.
fn add_buttons(ui: &mut Ui, studio: &mut Studio, reg: &Registry) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for lamp in LightType::ALL {
            let word = match lamp {
                LightType::Point => "+ Point",
                LightType::Spot => "+ Spot",
                LightType::Sun => "+ Sun",
                LightType::Area => "+ Area",
                LightType::Strip => "+ Strip",
            };
            let b = ui.add(egui::Button::new(word).min_size(egui::vec2(0.0, 22.0)));
            let b = b.on_hover_text(format!("Add a {}", lamp.menu_name().to_lowercase()));
            if b.clicked() {
                let parent = light_parent(&studio.scene, studio.selection.primary);
                if let Ok(id) = bpe_kit::things::light::add_light(&mut studio.scene, reg, lamp, parent) {
                    studio.selection.set(id);
                    studio.panels.inspector.own_step = true;
                    studio.status = format!("Added a {}.", lamp.menu_name().to_lowercase());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests;
