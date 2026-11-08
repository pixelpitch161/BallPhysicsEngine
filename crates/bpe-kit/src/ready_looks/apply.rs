//! Putting a ready-made look on a thing: its recipe becomes a copied look pasted onto the thing's Look card
//! (Replace), or its layers pasted above the thing's own (On top). Either way the thing owns the layers afterwards.

use bpe_core::{
    Address, CopiedLayer, CopiedLook, EditError, Gradient, LayerId, LayerSection, Registry, Scene, SlotId, ThingId,
    Value, layered_key,
};

use super::{RLayer, ReadyLook, Recipe, Wears, wears};
use crate::modules::look::layers::{self as l, BodyLayer};
use crate::modules::look::shape::{self as s, PaintLayer};

/// Whose colours a look is put on with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Colours {
    /// The look's own.
    #[default]
    Look,
    /// The thing's own colours where the look's main colour goes (when the look takes colours).
    Mine,
}

/// How a look is put on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum How {
    /// In place of the thing's whole look (its group layers stay).
    Replace,
    /// Its layers above the thing's own; the thing keeps its Body and its card's own settings.
    OnTop,
}

/// The settings of a ball's Body that stay when a look is put on: its outline.
const BODY_KEEPS: [&str; 2] = ["ball_shape", "roundness"];

/// The thing's own colours, for Keep mine: a ball's Body Colours, or a shape's lowest Colour and gradient layer's
/// colours (`None` when it has none).
pub fn own_colours(scene: &Scene, thing: ThingId) -> Option<Gradient> {
    let kind = &scene.things.get(&thing)?.kind;
    let card = scene.card(thing, SlotId::Look)?;
    let (layer, default) = match wears(kind)? {
        Wears::Balls => (card.layers.iter().find(|x| x.kind == l::BODY)?, BodyLayer::default().fill.colours),
        Wears::Shape => {
            (card.layers.iter().find(|x| x.kind == s::PAINT && x.parent.is_none())?, PaintLayer::default().colours)
        }
    };
    match card.params.get(&layered_key(layer.id, "colours")) {
        Some(Value::Gradient(g)) => Some(g.clone()),
        _ => Some(default),
    }
}

/// `recipe` as a copied look: its layers numbered from 0, bottom first, each followed by its sub-layers.
pub fn copied(recipe: &Recipe) -> CopiedLook {
    let mut next = 0u32;
    let copy = |r: &RLayer, next: &mut u32| -> CopiedLayer {
        let mut one = |r: &RLayer| {
            let id = LayerId(*next);
            *next += 1;
            CopiedLayer {
                id,
                kind: r.kind.into(),
                name: String::new(),
                enabled: true,
                params: r.params.clone(),
                subs: Vec::new(),
                only: Vec::new(),
                lane: None,
                target: None,
            }
        };
        let mut top = one(r);
        top.subs = r.subs.iter().map(&mut one).collect();
        top
    };
    let layers = recipe.layers.iter().map(|r| copy(r, &mut next)).collect();
    CopiedLook { layers, card: recipe.card.clone() }
}

/// Whether `look` can be worn by `thing`, whose Look card it needs.
fn fits(scene: &Scene, thing: ThingId, look: &ReadyLook) -> bool {
    let Some(t) = scene.things.get(&thing) else { return false };
    let card = match look.wears {
        Wears::Balls => l::CARD,
        Wears::Shape => s::CARD,
    };
    wears(&t.kind) == Some(look.wears) && scene.card(thing, SlotId::Look).is_some_and(|c| c.kind == card)
}

/// Puts `look` on `thing` with `colours`, `how`. Returns how many layers it brought. Refused (and nothing
/// changes) when the thing can't wear it.
pub fn apply(
    scene: &mut Scene,
    reg: &Registry,
    thing: ThingId,
    look: &ReadyLook,
    colours: Colours,
    how: How,
) -> Result<usize, EditError> {
    if !fits(scene, thing, look) {
        return Err(EditError::NotAccepted);
    }
    let mine = match colours {
        Colours::Mine => own_colours(scene, thing),
        Colours::Look => None,
    };
    let recipe = look.recipe(mine.as_ref());
    let mut copy = copied(&recipe);
    let ids: Vec<LayerId> = match how {
        How::Replace => {
            // The ball keeps its outline: a square ball stays square.
            if look.wears == Wears::Balls
                && let Some(card) = scene.card(thing, SlotId::Look)
                && let Some(body) = card.layers.iter().find(|x| x.kind == l::BODY)
                && let Some(base) = copy.layers.iter_mut().find(|x| x.kind == l::BODY)
            {
                for key in BODY_KEEPS {
                    base.params.0.remove(key);
                    if let Some(v) = card.params.get(&layered_key(body.id, key)) {
                        base.params.set(key, v.clone());
                    }
                }
            }
            scene.paste_look(reg, thing, SlotId::Look, &copy)?;
            // The card's top-level layers, but the group's, are the recipe's, in order.
            let card = scene.card(thing, SlotId::Look).ok_or(EditError::NotAccepted)?;
            let group = |kind: &str| reg.layer(kind).is_some_and(|k| k.section() == LayerSection::Group);
            card.layers.iter().filter(|x| x.parent.is_none() && !group(&x.kind)).map(|x| x.id).collect()
        }
        How::OnTop => {
            let mut ids = vec![LayerId(u32::MAX)];
            for layer in copy.layers.iter().skip(1) {
                ids.push(scene.paste_layer(reg, thing, SlotId::Look, layer)?);
            }
            ids
        }
    };
    for p in &recipe.pulses {
        if let Some(&id) = ids.get(p.layer).filter(|id| id.0 != u32::MAX) {
            let address = Address { thing, slot: Some(SlotId::Look), key: layered_key(id, p.key) };
            scene.pulses.insert(address, p.pulse.clone());
        }
    }
    Ok(match how {
        How::Replace => recipe.layers.len(),
        How::OnTop => recipe.layers.len().saturating_sub(1),
    })
}

/// Adds a thing of `kind` under `parent` wearing `look` in its own colours (the Add thing menu's favourites).
pub fn add_with_look(
    scene: &mut Scene,
    reg: &Registry,
    kind: &str,
    parent: ThingId,
    look: &ReadyLook,
) -> Result<ThingId, EditError> {
    let id = crate::starters::add_thing(scene, reg, kind, parent)?;
    apply(scene, reg, id, look, Colours::Look, How::Replace)?;
    Ok(id)
}

/// The values of the Body a look would keep from a thing (tests).
#[cfg(test)]
pub(crate) fn body_values(scene: &Scene, thing: ThingId) -> Option<BodyLayer> {
    let card = scene.card(thing, SlotId::Look)?;
    let body = card.layers.iter().find(|x| x.kind == l::BODY)?;
    let prefix = format!("{}/", body.id);
    use bpe_core::Settings;
    let own = bpe_core::Params(
        card.params.iter().filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_owned(), v.clone()))).collect(),
    );
    Some(BodyLayer::from_params(&own))
}
