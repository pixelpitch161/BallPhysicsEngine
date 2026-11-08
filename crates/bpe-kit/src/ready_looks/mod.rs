//! Ready-made looks (improvement review E12, spec `docs/superpowers/specs/2026-11-08-ready-made-looks-design.md`):
//! finished Look stacks for Balls, Emitters and Shapes, picked from the Looks tab or the Add thing menu. A look is
//! only a recipe of existing layer kinds; picking it pastes the layers onto the thing ([`apply::apply`]), which owns
//! them from then on. Nothing here is saved or linked, and nothing is added to a scene unless a person picks a look.

pub mod apply;
mod balls;
mod shapes;
#[cfg(test)]
mod tests;

use std::sync::OnceLock;

use bpe_core::{Color, Gradient, Params, Pulse, Registry};

pub use apply::{Colours, How, add_with_look, apply, own_colours};

/// What a look is worn by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Wears {
    /// Balls and Emitters (their Look card is the balls' stack).
    Balls,
    Shape,
}

/// One ready-made look.
pub struct ReadyLook {
    /// Never shown and never saved: tests and the editor's picks name it.
    pub id: &'static str,
    pub name: &'static str,
    pub family: &'static str,
    /// One sentence on how it looks.
    pub help: &'static str,
    pub wears: Wears,
    /// Whether the thing's own colours can take the place of its main colour (Keep mine).
    pub takes_colour: bool,
    /// The look's own main colour.
    pub main: Color,
    /// Two colours that stand for it where no picture can be drawn (menus, tiles without a GPU).
    pub swatch: [Color; 2],
    /// The layers, given the main colours.
    build: fn(&Gradient) -> Recipe,
}

/// What a look is made of: its layers bottom first (the first is the Body), the card's own settings (a Look's
/// Surface) and the pulses its layers' settings follow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recipe {
    pub layers: Vec<RLayer>,
    pub card: Params,
    pub pulses: Vec<RPulse>,
}

/// One layer of a recipe: its kind, its values (only those that differ from the kind's defaults, by plain key)
/// and its sub-layers.
#[derive(Clone, Debug, PartialEq)]
pub struct RLayer {
    pub kind: &'static str,
    pub params: Params,
    pub subs: Vec<RLayer>,
}

/// A pulse on setting `key` of the recipe's layer number `layer` (top-level layers, counted from 0).
#[derive(Clone, Debug, PartialEq)]
pub struct RPulse {
    pub layer: usize,
    pub key: &'static str,
    pub pulse: Pulse,
}

impl RLayer {
    pub(crate) fn new(kind: &'static str, params: Params) -> Self {
        Self { kind, params, subs: Vec::new() }
    }

    /// The layer with sub-layer `sub` after its others.
    pub(crate) fn with(mut self, sub: RLayer) -> Self {
        self.subs.push(sub);
        self
    }

    /// The layer with `key` set to `v` too (a gradient's companion such as `colours~form`, or a Shader's own key).
    pub(crate) fn set(mut self, key: &str, v: bpe_core::Value) -> Self {
        self.params.set(key, v);
        self
    }
}

/// The families of looks worn by `w`, in order.
pub fn families(w: Wears) -> &'static [&'static str] {
    match w {
        Wears::Balls => &BALL_FAMILIES,
        Wears::Shape => &SHAPE_FAMILIES,
    }
}

pub const BALL_FAMILIES: [&str; 5] = ["Clean", "Shiny", "Neon", "Fire", "Trippy"];
pub const SHAPE_FAMILIES: [&str; 3] = ["Clean", "Glow", "Colour"];

/// The looks the Add thing menu offers for Balls and Emitters.
pub const BALL_FAVOURITES: [&str; 7] = ["candy", "chrome", "glass", "neon_ring", "comet", "lava", "rainbow"];
/// The looks the Add thing menu offers for Shapes.
pub const SHAPE_FAVOURITES: [&str; 7] = [
    "shape.solid",
    "shape.neon_tube",
    "shape.glass",
    "shape.glow_ring",
    "shape.sunset",
    "shape.rainbow_flow",
    "shape.hit_ripple",
];

/// Every look: the balls' then the shapes', each family in order.
pub fn all() -> &'static [ReadyLook] {
    static ALL: OnceLock<Vec<ReadyLook>> = OnceLock::new();
    ALL.get_or_init(|| balls::looks().into_iter().chain(shapes::looks()).collect())
}

/// The look called `id`.
pub fn find(id: &str) -> Option<&'static ReadyLook> {
    all().iter().find(|l| l.id == id)
}

/// What a thing of `kind` wears, when it can wear ready-made looks.
pub fn wears(kind: &str) -> Option<Wears> {
    if crate::things::balls::rows::has_balls(kind) {
        Some(Wears::Balls)
    } else if kind == crate::things::shape::KIND {
        Some(Wears::Shape)
    } else {
        None
    }
}

/// The looks a thing of `kind` can wear, in order.
pub fn for_kind(kind: &str) -> impl Iterator<Item = &'static ReadyLook> {
    let w = wears(kind);
    all().iter().filter(move |l| Some(l.wears) == w)
}

/// The favourite looks of things of `kind` for the Add thing menu.
pub fn favourites(kind: &str) -> Vec<&'static ReadyLook> {
    let ids: &[&str] = match wears(kind) {
        Some(Wears::Balls) => &BALL_FAVOURITES,
        Some(Wears::Shape) => &SHAPE_FAVOURITES,
        None => &[],
    };
    ids.iter().filter_map(|id| find(id)).collect()
}

impl ReadyLook {
    /// Its recipe with `colours` as its main colours (its own when `None`).
    pub fn recipe(&self, colours: Option<&Gradient>) -> Recipe {
        match colours {
            Some(c) if self.takes_colour => (self.build)(c),
            _ => (self.build)(&Gradient::solid(self.main)),
        }
    }

    /// The kinds of its layers, in order, by name: "Body, Shine, Outline, Flash on hit".
    pub fn comes_as(&self, reg: &Registry) -> String {
        let mut names: Vec<&str> = Vec::new();
        for l in self.recipe(None).layers {
            let name = reg.layer(l.kind).map_or(l.kind, |k| k.name());
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.join(", ")
    }

    /// Whether every word of `words` (lower case, split on spaces) is in its name or its description.
    pub fn matches(&self, words: &str) -> bool {
        let text = format!("{} {} {}", self.name, self.help, self.family).to_lowercase();
        words.split_whitespace().all(|w| text.contains(&w.to_lowercase()))
    }
}

/// The values of `s` that differ from its kind's defaults.
pub(crate) fn own<T: bpe_core::Settings>(s: &T) -> Params {
    s.to_params()
}
