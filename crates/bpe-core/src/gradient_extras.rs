//! Companion settings of a gradient: what lays its colours out, moves them or cuts them into
//! bands, kept beside the gradient's own key so each is an ordinary number that keyframes, hit
//! pulses, Follow sound and Follow light can drive.
//!
//! A gradient setting with key `k` (a layer's keeps its prefix: `"3/colours"`) may have
//! `"<k>~<name>"` beside it in the same params, in the style of the vary keys (`"<k>~to"`).
//! Every gradient setting gets them without its struct naming them: [`crate::Scene::def_at`] and
//! [`crate::layer_def`] answer their descriptions. A companion that isn't stored reads as its
//! default, and every default leaves the gradient exactly as it is.
//!
//! Each companion names the tab of the editor's gradient row it shows on. Gradients part 1 has
//! the Blend tab's Bands and Grain; part 2 adds the Shape tab's, part 3 the Move tab's.

use std::sync::LazyLock;

use crate::{Depth, Flags, Gradient, Params, Resolved, SettingDef, Value, ValueType};

/// What separates a gradient's key from a companion's name.
pub const SEP: char = '~';

/// The tab of the editor's gradient row a companion shows on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtraTab {
    Blend,
    Shape,
    Move,
}

impl ExtraTab {
    pub const ALL: [ExtraTab; 3] = [ExtraTab::Blend, ExtraTab::Shape, ExtraTab::Move];

    pub fn label(self) -> &'static str {
        match self {
            Self::Blend => "Blend",
            Self::Shape => "Shape",
            Self::Move => "Move",
        }
    }
}

/// One companion setting: its name after the `~`, its tab, and its description (whose `key` is
/// `"~<name>"`; the full key is the gradient's key with it on the end).
#[derive(Clone, Debug, PartialEq)]
pub struct Companion {
    pub name: &'static str,
    pub tab: ExtraTab,
    pub def: SettingDef,
}

/// The most bands a gradient can be cut into: past the GPU's 1024 samples a row more show
/// nothing new, and each band costs two stops.
pub const MOST_BANDS: f64 = 1024.0;

fn number(key: &'static str, label: &'static str, help: &'static str, span: (f64, f64)) -> SettingDef {
    SettingDef {
        key,
        label,
        help,
        ty: ValueType::Number,
        default: Value::Number(0.0),
        range: None,
        span: Some(span),
        unit: "",
        group: "",
        depth: Depth::More,
        flags: Flags { keyframe: true, pulse: true, hold: false },
        when: "",
    }
}

/// Every companion, in the order the editor lists them in their tabs.
pub static COMPANIONS: LazyLock<Vec<Companion>> = LazyLock::new(|| {
    let mut bands = number(
        "~bands",
        "Bands",
        "Cuts the colours into this many flat bands, each the colour at its middle. 0 is off.",
        (0.0, 24.0),
    );
    bands.range = Some((0.0, MOST_BANDS));
    let grain = number(
        "~grain",
        "Grain",
        "Fine still noise where the colours are painted, hiding stair-steps in slow dark gradients. 0 is off.",
        (0.0, 8.0),
    );
    vec![
        Companion { name: "bands", tab: ExtraTab::Blend, def: bands },
        Companion { name: "grain", tab: ExtraTab::Blend, def: grain },
    ]
});

/// The companion named `name`.
pub fn companion(name: &str) -> Option<&'static Companion> {
    COMPANIONS.iter().find(|c| c.name == name)
}

/// Companion `name` of the gradient at `base`: `"colours~bands"`.
pub fn key(base: &str, name: &str) -> String {
    format!("{base}{SEP}{name}")
}

/// The gradient key a companion key belongs to and which companion it is: `"3/colours~grain"` is
/// `("3/colours", grain)`. `None` for any other key.
pub fn split(key: &str) -> Option<(&str, &'static Companion)> {
    let (base, name) = key.rsplit_once(SEP)?;
    if base.is_empty() {
        return None;
    }
    Some((base, companion(name)?))
}

pub fn is_companion_key(key: &str) -> bool {
    split(key).is_some()
}

/// The description of companion `c` of the setting described by `base`, when that is a
/// gradient.
pub fn def_for(base: Option<&'static SettingDef>, c: &'static Companion) -> Option<&'static SettingDef> {
    base.filter(|d| d.ty == ValueType::Gradient).map(|_| &c.def)
}

/// The companions offered on gradient setting `base`, in order (none on anything else).
pub fn companions_for(base: &SettingDef) -> impl Iterator<Item = &'static Companion> + use<> {
    let gradient = base.ty == ValueType::Gradient;
    COMPANIONS.iter().filter(move |_| gradient)
}

/// A companion key's default, when it is one.
pub fn default_of(key: &str) -> Option<Value> {
    split(key).map(|(_, c)| c.def.default.clone())
}

/// A number a companion holds (0 for anything else).
fn number_of(v: &Value) -> f64 {
    match v {
        Value::Number(x) if x.is_finite() => *x,
        Value::Int(x) => *x as f64,
        _ => 0.0,
    }
}

/// Whether any companion is stored anywhere in `resolved`: the frame builder copies the
/// resolved scene to apply them only then.
pub fn any_in(resolved: &Resolved) -> bool {
    resolved.things.values().any(|t| {
        std::iter::once(&t.params)
            .chain(t.slots.values().map(|s| &s.params))
            .any(|p| p.iter().any(|(k, _)| is_companion_key(k)))
    })
}

/// Applies every gradient's companions to the gradient itself, once a frame before anything
/// draws, right after the animation (so keyed, pulsed and followed companions count): Bands cut
/// it into flat bands, then Grain is copied onto it for the GPU. A varied gradient's other end
/// (`~to`) gets the same, so each ball's mix of the two keeps the bands. Pure in its inputs:
/// nothing drawn earlier is read. `time` is for the moves of gradients part 3.
pub fn apply_moves(resolved: &mut Resolved, _time: f64) {
    for t in resolved.things.values_mut() {
        apply_in(&mut t.params);
        for s in t.slots.values_mut() {
            apply_in(&mut s.params);
        }
    }
}

/// [`apply_moves`] on one set of params.
fn apply_in(params: &mut Params) {
    // (gradient key, bands, grain) for every gradient with a companion stored.
    let mut found: Vec<(String, f64, f64)> = Vec::new();
    for (k, v) in params.iter() {
        let Some((base, c)) = split(k) else { continue };
        let at = match found.iter().position(|(b, ..)| b == base) {
            Some(i) => i,
            None => {
                found.push((base.to_owned(), 0.0, 0.0));
                found.len() - 1
            }
        };
        match c.name {
            "bands" => found[at].1 = number_of(v),
            "grain" => found[at].2 = number_of(v),
            _ => {}
        }
    }
    for (base, bands, grain) in found {
        let bands = bands.round().clamp(0.0, MOST_BANDS) as u32;
        for key in [base.clone(), crate::vary::to_key(&base)] {
            if let Some(Value::Gradient(g)) = params.get(&key) {
                let mut g: Gradient = if bands > 0 { g.banded(bands) } else { g.clone() };
                g.grain = grain as f32;
                params.set(&key, Value::Gradient(g));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Address, Color, Scene, SlotId, ThingId, testkit};

    #[test]
    fn keys_split_into_the_gradient_and_its_companion() {
        assert_eq!(key("colours", "bands"), "colours~bands");
        let (base, c) = split("3/colours~grain").unwrap();
        assert_eq!((base, c.name, c.tab), ("3/colours", "grain", ExtraTab::Blend));
        assert!(split("colours~to").is_none());
        assert!(split("~bands").is_none());
        assert!(split("colours").is_none());
        assert_eq!(default_of("colours~bands"), Some(Value::Number(0.0)));
    }

    #[test]
    fn only_gradients_have_companions() {
        let bands = companion("bands").unwrap();
        let gradient = SettingDef {
            key: "colours",
            ty: ValueType::Gradient,
            default: Value::Gradient(Gradient::default()),
            ..bands.def.clone()
        };
        let gradient: &'static SettingDef = Box::leak(Box::new(gradient));
        assert_eq!(def_for(Some(gradient), bands).map(|d| d.label), Some("Bands"));
        assert!(def_for(Some(&bands.def), bands).is_none());
        assert_eq!(companions_for(gradient).count(), COMPANIONS.len());
        assert_eq!(companions_for(&bands.def).count(), 0);
    }

    fn two_colours() -> Gradient {
        Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)])
    }

    #[test]
    fn the_pass_bands_gradients_and_carries_grain() {
        let mut p = Params::default();
        p.set("colours", Value::Gradient(two_colours()));
        p.set("colours~to", Value::Gradient(two_colours()));
        p.set("colours~bands", Value::Number(3.6));
        p.set("colours~grain", Value::Number(2.0));
        p.set("other", Value::Gradient(two_colours()));
        apply_in(&mut p);
        let Some(Value::Gradient(g)) = p.get("colours") else { panic!() };
        assert_eq!(g.stops.len(), 8, "3.6 rounds to 4 bands");
        assert_eq!(g.grain, 2.0);
        assert_eq!(p.get("colours~to"), p.get("colours"), "the other end gets the same");
        assert_eq!(p.get("other"), Some(&Value::Gradient(two_colours())), "a gradient without companions is left");
    }

    #[test]
    fn companions_are_settings_of_cards_and_layers() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        let layer = s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        let gradient = crate::layered_key(layer, "colours");
        let companion = Address::on(b, SlotId::Look, &key(&gradient, "bands"));
        assert_eq!(s.def_at(&r, &companion).map(|d| d.label), Some("Bands"));
        s.set_at(&r, &companion, Value::Number(5.0)).unwrap();
        assert_eq!(s.value_at(&r, &companion), Some(Value::Number(5.0)));
        let resolved = crate::resolve(&s, &r);
        assert!(any_in(&resolved));
        assert_eq!(resolved.things[&b].slots[&SlotId::Look].params.get(&companion.key), Some(&Value::Number(5.0)));
        let loaded = crate::load(&crate::save(&s), &r).unwrap();
        assert_eq!(loaded.things[&b].slots[&SlotId::Look].params.get(&companion.key), Some(&Value::Number(5.0)));
        assert_eq!(s.vary_use(&r, &companion), None, "companions don't vary across balls");
        // Keyed from its unstored default, it animates.
        s.set_at(&r, &companion, Value::Number(0.0)).unwrap();
        assert!(s.things[&b].slots[&SlotId::Look].params.get(&companion.key).is_none(), "the default isn't stored");
        s.set_key(&r, &companion, 0.0, Value::Number(0.0)).unwrap();
        s.set_key(&r, &companion, 2.0, Value::Number(4.0)).unwrap();
        let mut resolved = crate::resolve(&s, &r);
        crate::Animation::new(&s, &resolved).apply(&mut resolved, 2.0, &crate::Inputs::NONE);
        apply_moves(&mut resolved, 2.0);
        let Some(Value::Gradient(g)) = resolved.things[&b].slots[&SlotId::Look].params.get(&gradient) else { panic!() };
        assert_eq!(g.stops.len(), 8, "four bands at 2 s");
        // A number setting has none.
        let width = Address::on(b, SlotId::Look, &key(&crate::layered_key(layer, "width"), "bands"));
        assert!(s.def_at(&r, &width).is_none());
    }
}
