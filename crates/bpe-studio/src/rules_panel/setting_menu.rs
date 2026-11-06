//! Change setting's setting picker: thing ▸ card ▸ (layer ▸) setting, listing every setting a
//! rule can change (numbers, whole numbers, pairs, switches, colours, gradients and choices).

use bpe_core::{Address, Choice, Scene, SettingDef, SlotId, ThingId, ValueType, VaryUse, Who, layered_key};
use bpe_kit::Kit;
use bpe_kit::rules::change::CardChoice;
use eframe::egui::{self, RichText, Ui};

use crate::style;

/// One setting the menu offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Its key on the card (a layer's keeps its layer: "3/colour").
    pub key: String,
    pub label: &'static str,
    /// Its heading in the inspector, "" for none.
    pub group: &'static str,
    /// On a thing with balls: a change aimed at one ball changes the whole group.
    pub whole_group: bool,
}

/// One card's settings, its layers' under their names.
#[derive(Clone, Debug, PartialEq)]
pub struct CardItems {
    pub card: CardChoice,
    /// As the inspector calls the card: "Balls", "Look", "Bounce", "Effects".
    pub title: String,
    pub items: Vec<Item>,
    pub layers: Vec<(String, Vec<Item>)>,
}

/// The cards a change can be on, in the order the menu lists them.
const CARDS: [CardChoice; 6] = [
    CardChoice::Own,
    CardChoice::Look,
    CardChoice::Motion,
    CardChoice::Bounce,
    CardChoice::Shader,
    CardChoice::Lighting,
];

/// Whether a rule can change a setting of this type.
pub fn changeable(ty: &ValueType) -> bool {
    matches!(
        ty,
        ValueType::Number
            | ValueType::Int
            | ValueType::Vec2
            | ValueType::Bool
            | ValueType::Color
            | ValueType::Gradient
            | ValueType::Choice(_)
    )
}

/// The title of `card` on `thing`, as the inspector shows it.
pub fn card_title(scene: &Scene, kit: &Kit, thing: ThingId, card: CardChoice) -> String {
    let kind = scene.things.get(&thing).and_then(|t| kit.registry.thing(&t.kind));
    match (card.slot(), kind) {
        (None, Some(k)) => k.name().to_owned(),
        (Some(slot), Some(k)) => k.slot_label(slot).to_owned(),
        (Some(slot), None) => slot.name().to_owned(),
        (None, None) => "Own settings".to_owned(),
    }
}

/// Whether one ball of `thing` can have the setting at `a` its own way (else a change aimed at it
/// changes the whole thing): as Change setting itself decides.
fn per_ball(scene: &Scene, kit: &Kit, a: &Address) -> bool {
    let live = scene.vary_use(&kit.registry, a) == Some(VaryUse::Live);
    match a.slot {
        None | Some(SlotId::Look) => live,
        Some(SlotId::Motion | SlotId::Bounce) => {
            live && scene
                .card(a.thing, a.slot.expect("a card"))
                .and_then(|c| kit.sims.module(&c.kind))
                .is_some_and(|m| m.per_ball())
        }
        Some(_) => false,
    }
}

fn items_of(
    scene: &Scene,
    kit: &Kit,
    thing: ThingId,
    slot: Option<SlotId>,
    defs: &[SettingDef],
    key_of: &dyn Fn(&str) -> String,
) -> Vec<Item> {
    let balls = scene.things.get(&thing).is_some_and(|t| bpe_kit::things::balls::rows::has_balls(&t.kind));
    defs.iter()
        .filter(|d| changeable(&d.ty))
        .map(|d| {
            let key = key_of(d.key);
            let whole_group = balls && !per_ball(scene, kit, &Address { thing, slot, key: key.clone() });
            Item { key, label: d.label, group: d.group, whole_group }
        })
        .collect()
}

/// Every setting of `thing` a rule can change, card by card (cards it hasn't are left out).
pub fn cards_of(scene: &Scene, kit: &Kit, thing: ThingId) -> Vec<CardItems> {
    let reg = &kit.registry;
    let Some(t) = scene.things.get(&thing) else { return Vec::new() };
    let mut out = Vec::new();
    for card in CARDS {
        let (items, layers) = match card.slot() {
            None => {
                let Some(kind) = reg.thing(&t.kind) else { continue };
                (items_of(scene, kit, thing, None, kind.settings(), &|k| k.to_owned()), Vec::new())
            }
            Some(slot) => {
                let Some(c) = t.slots.get(&slot) else { continue };
                let own = reg.module(&c.kind).map(|m| m.settings()).unwrap_or_default();
                let items = items_of(scene, kit, thing, Some(slot), own, &|k| k.to_owned());
                let layers = c
                    .layers
                    .iter()
                    .filter_map(|l| {
                        let kind = reg.layer(&l.kind)?;
                        let get = |k: &str| c.params.get(&layered_key(l.id, k)).cloned();
                        let mut defs: Vec<SettingDef> = kind.settings().to_vec();
                        defs.extend(kind.dynamic_defs(&get).iter().cloned());
                        let items = items_of(scene, kit, thing, Some(slot), &defs, &|k| layered_key(l.id, k));
                        (!items.is_empty()).then(|| (l.name.clone(), items))
                    })
                    .collect();
                (items, layers)
            }
        };
        if items.is_empty() && layers.is_empty() {
            continue;
        }
        out.push(CardItems { card, title: card_title(scene, kit, thing, card), items, layers });
    }
    out
}

/// The thing a picked setting is read from: the one `who` names when it has the setting, else the
/// first in tree order that has it.
pub fn holder(scene: &Scene, kit: &Kit, who: &Who, card: CardChoice, key: &str) -> Option<ThingId> {
    let has = |thing: ThingId| {
        scene.def_at(&kit.registry, &Address { thing, slot: card.slot(), key: key.to_owned() }).is_some()
    };
    if key.is_empty() {
        return None;
    }
    who.names_thing().filter(|t| has(*t)).or_else(|| scene.order.iter().copied().find(|t| has(*t)))
}

/// The description of the picked setting, read from its [`holder`].
pub fn picked_def(
    scene: &Scene,
    kit: &Kit,
    who: &Who,
    card: CardChoice,
    key: &str,
) -> Option<(ThingId, &'static SettingDef)> {
    let thing = holder(scene, kit, who, card, key)?;
    let def = scene.def_at(&kit.registry, &Address { thing, slot: card.slot(), key: key.to_owned() })?;
    Some((thing, def))
}

/// The picked setting in words: "Bounce · Bounciness", "Look · Glow · Colour"; "No setting yet".
pub fn setting_words(scene: &Scene, kit: &Kit, who: &Who, card: CardChoice, key: &str) -> String {
    let Some((thing, def)) = picked_def(scene, kit, who, card, key) else {
        return if key.trim().is_empty() { "No setting yet".to_owned() } else { format!("\"{key}\" (not found)") };
    };
    let title = card_title(scene, kit, thing, card);
    let layer = card
        .slot()
        .zip(bpe_core::split_layered(key))
        .and_then(|(slot, (id, _))| scene.card(thing, slot).and_then(|c| c.layer(id)).map(|l| l.name.clone()));
    match layer {
        Some(layer) => format!("{title} · {layer} · {}", def.label),
        None => format!("{title} · {}", def.label),
    }
}

/// The items of one card or layer, under their group headings. Some((card, key)) when one is
/// picked.
fn item_buttons(
    ui: &mut Ui,
    card: CardChoice,
    items: &[Item],
    now: (CardChoice, &str),
) -> Option<(CardChoice, String)> {
    let mut picked = None;
    let mut heading = "";
    for item in items {
        if item.group != heading && !item.group.is_empty() {
            heading = item.group;
            ui.label(RichText::new(heading).small().color(style::FAINT));
        }
        let on = now.0 == card && now.1 == item.key;
        let text = if item.whole_group { format!("{} (whole group)", item.label) } else { item.label.to_owned() };
        let mut button = ui.selectable_label(on, text);
        if item.whole_group {
            button = button.on_hover_text("A change aimed at one ball changes every ball of the group");
        }
        if button.clicked() {
            picked = Some((card, item.key.clone()));
            ui.close();
        }
    }
    picked
}

/// The setting menu: a button naming the picked setting, opening thing ▸ card ▸ (layer ▸)
/// setting. Some((card, key)) when a setting is picked.
pub fn setting_menu(
    ui: &mut Ui,
    scene: &Scene,
    kit: &Kit,
    who: &Who,
    now: (CardChoice, &str),
) -> Option<(CardChoice, String)> {
    let mut picked = None;
    let words = setting_words(scene, kit, who, now.0, now.1);
    let menu = ui.menu_button(words, |ui| {
        ui.set_min_width(180.0);
        // The thing who names first, then every thing in tree order.
        let mut things: Vec<ThingId> = who.names_thing().into_iter().collect();
        things.extend(scene.order.iter().copied().filter(|t| Some(*t) != who.names_thing()));
        for thing in things {
            let cards = cards_of(scene, kit, thing);
            let Some(t) = scene.things.get(&thing).filter(|_| !cards.is_empty()) else { continue };
            ui.menu_button(&t.name, |ui| {
                for c in &cards {
                    ui.menu_button(&c.title, |ui| {
                        ui.set_min_width(170.0);
                        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                            if let Some(p) = item_buttons(ui, c.card, &c.items, now) {
                                picked = Some(p);
                            }
                            for (layer, items) in &c.layers {
                                ui.menu_button(layer, |ui| {
                                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                                        if let Some(p) = item_buttons(ui, c.card, items, now) {
                                            picked = Some(p);
                                        }
                                    });
                                });
                            }
                        });
                    });
                }
            });
        }
    });
    // Named by its own words (the setting picked), not by its row's label.
    menu.response.on_hover_text("The setting this block changes: thing, then card, then setting");
    picked
}

/// The value a card choice is saved as.
pub fn card_value(card: CardChoice) -> bpe_core::Value {
    bpe_core::Value::Choice(card.name().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kit() -> Kit {
        bpe_sound::kit()
    }

    fn find(cards: &[CardItems], card: CardChoice) -> &CardItems {
        cards.iter().find(|c| c.card == card).unwrap_or_else(|| panic!("{card:?} is listed"))
    }

    #[test]
    fn a_ball_lists_its_bounce_and_the_scene_its_gravity() {
        let kit = kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let ball = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").expect("a ball");
        let cards = cards_of(&scene, &kit, ball);
        let bounce = find(&cards, CardChoice::Bounce);
        let bounciness = bounce.items.iter().find(|i| i.key == "bounciness").expect("Bounciness is offered");
        assert_eq!(bounciness.label, "Bounciness");
        // The look's layers are listed under their names.
        let look = find(&cards, CardChoice::Look);
        assert!(!look.layers.is_empty(), "the Look's layers are listed");
        // Nothing that isn't changeable.
        let root = cards_of(&scene, &kit, ThingId::ROOT);
        let own = find(&root, CardChoice::Own);
        assert!(own.items.iter().any(|i| i.key == "gravity"), "the scene's Gravity");
        assert!(!own.items.iter().any(|i| i.whole_group), "the scene has no balls");
        assert!(changeable(&ValueType::Gradient) && !changeable(&ValueType::Text) && !changeable(&ValueType::Who));
    }

    #[test]
    fn the_picked_setting_is_read_from_the_named_thing_or_the_first_that_has_it() {
        let kit = kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let ball = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").unwrap();
        let (thing, def) = picked_def(&scene, &kit, &Who::That, CardChoice::Bounce, "bounciness").unwrap();
        assert_eq!(def.label, "Bounciness");
        assert!(scene.things[&thing].slots.contains_key(&SlotId::Bounce));
        let named = Who::Thing { thing: ball, ball: None };
        assert_eq!(holder(&scene, &kit, &named, CardChoice::Bounce, "bounciness"), Some(ball));
        assert_eq!(setting_words(&scene, &kit, &named, CardChoice::Bounce, "bounciness"), "Bounce · Bounciness");
        assert_eq!(setting_words(&scene, &kit, &named, CardChoice::Bounce, ""), "No setting yet");
        assert!(picked_def(&scene, &kit, &named, CardChoice::Bounce, "no such").is_none());
    }
}
