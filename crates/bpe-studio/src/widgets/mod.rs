//! Setting widgets driven by the kit's setting descriptions: one grid row per setting.
//!
//! This file holds what the rows share (the grid's context, rows and outcomes, target choices).
//! `grid.rs` lays out the grid, `setting.rs` draws one setting's row and widget, `menu.rs` is its
//! right-click menu, `numbers.rs` the number sliders, `pickers.rs` the who, target and colour
//! pickers, and `words.rs` puts settings into words.

use bpe_core::{Color, Registry, Scene, SettingDef, Target, ThingId, Value, ValueType, section_rank};
use eframe::egui::{
    self, Color32, DragValue, PointerButton, Response, RichText, Sense, Slider, Ui, Vec2, color_picker::Alpha,
};

use crate::gradient_bar::gradient_bar_named;
use crate::style;

mod grid;
mod menu;
mod numbers;
mod pickers;
mod setting;
#[cfg(test)]
mod tests;
mod words;

pub use grid::*;
pub use menu::*;
pub use numbers::*;
pub(crate) use pickers::*;
use setting::*;
pub use words::*;

/// What a Target setting can point at: the scene's things, tags and kinds, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TargetChoices {
    /// In tree order.
    pub things: Vec<(ThingId, String)>,
    /// Every tag any thing has, sorted.
    pub tags: Vec<String>,
    /// Thing kinds as (id, name), in registry order, without the scene itself.
    pub kinds: Vec<(String, String)>,
    /// The things with balls (a Balls group or an emitter): a rule's who can name one ball of them.
    pub ball_things: Vec<ThingId>,
    /// The Texts set in the world (their letters a wall): a rule's who can name one letter of them.
    pub letter_things: Vec<ThingId>,
    /// A rule block's who picker offers the firing's own parties (That ball, The thing it hit,
    /// That ball's group): true for If and Do blocks, false for When blocks, where they would mean
    /// anything.
    pub relative: bool,
    /// The fonts the scene keeps, by name (a Text's Font picker lists them first).
    pub fonts: Vec<String>,
    /// Those of them that can't be read (damaged).
    pub unreadable_fonts: Vec<String>,
    /// Every counter the rules name, sorted (a Text's Insert value ▸ Counter lists them).
    pub counters: Vec<String>,
    /// The pictures the scene keeps, by name (a Particles thing's Picture row checks them).
    pub pictures: Vec<String>,
}

impl TargetChoices {
    pub fn from_scene(scene: &Scene, reg: &Registry) -> TargetChoices {
        let things = scene.order.iter().filter_map(|id| scene.things.get(id).map(|t| (*id, t.name.clone()))).collect();
        let tags: std::collections::BTreeSet<String> =
            scene.things.values().flat_map(|t| t.tags.iter().cloned()).collect();
        let kinds =
            reg.things().filter(|k| k.id() != "scene").map(|k| (k.id().to_owned(), k.name().to_owned())).collect();
        let ball_things = scene
            .order
            .iter()
            .filter(|id| scene.things.get(id).is_some_and(|t| bpe_kit::things::balls::rows::has_balls(&t.kind)))
            .copied()
            .collect();
        let letter_things = scene
            .order
            .iter()
            .filter(|id| {
                scene.things.get(id).is_some_and(|t| t.kind == bpe_kit::things::text::KIND && t.screen == Some(false))
            })
            .copied()
            .collect();
        let fonts = scene.fonts.keys().cloned().collect();
        let unreadable_fonts = scene
            .fonts
            .iter()
            .filter(|(name, font)| !bpe_kit::text::fonts::scene_font_reads(name, font))
            .map(|(name, _)| name.clone())
            .collect();
        let counters: std::collections::BTreeSet<String> = scene
            .rules
            .list
            .iter()
            .flat_map(|r| r.blocks.iter())
            .filter_map(|b| {
                // The block's own value, else its kind's default, where the kind shows a counter now.
                let defs = reg.block(&b.kind)?.settings();
                let get = |k: &str| {
                    b.params.get(k).cloned().or_else(|| defs.iter().find(|d| d.key == k).map(|d| d.default.clone()))
                };
                let def = defs.iter().find(|d| d.key == "counter")?;
                if !bpe_core::settings::applies(def.when, None, &get) {
                    return None;
                }
                match get("counter") {
                    Some(bpe_core::Value::Text(c)) if !c.trim().is_empty() => Some(c.trim().to_string()),
                    _ => None,
                }
            })
            .collect();
        TargetChoices {
            things,
            tags: tags.into_iter().collect(),
            kinds,
            ball_things,
            letter_things,
            relative: true,
            fonts,
            unreadable_fonts,
            counters: counters.into_iter().collect(),
            pictures: scene.pictures.keys().cloned().collect(),
        }
    }
}

/// What a settings grid needs to know beyond the values: what targets can point at and what each
/// setting's keys are (none for a setting that can't be keyed or pulsed).
pub struct GridCx<'a> {
    pub choices: &'a TargetChoices,
    pub marks: &'a dyn Fn(&str) -> Option<KeyMark>,
    /// Whether the setting does anything here; rows that don't are left out.
    pub applies: &'a dyn Fn(&SettingDef) -> bool,
    /// Which group headings are folded shut. With none, headings have no fold arrow.
    pub folded: Option<&'a dyn Fn(&str) -> bool>,
    /// How wide the label column is, from [`label_width`]. Labels wrap at it.
    pub label_width: f32,
    /// The thing's track colour: the label colour of a setting with keys or a pulse.
    pub track: Color32,
    /// Follow sound for these settings (none: the menu doesn't offer it).
    pub follow: Option<&'a crate::follow::FollowCx<'a>>,
    /// Varying these settings across the thing's balls (none: the menu doesn't offer it).
    pub vary: Option<&'a crate::vary::VaryCx<'a>>,
    /// Which choices of a setting its dropdown offers (none: every one), as the thing's kind says
    /// ([`bpe_core::ThingKind::offers`]).
    pub offers: Option<&'a OffersOf<'a>>,
    /// The colours a number setting's rail shows under it, left to right (the grade's colour
    /// sliders: the video owns colour); none for a plain rail.
    pub rails: Option<&'a RailsOf<'a>>,
}

/// The colours under a number setting's rail, by its key ([`GridCx::rails`]).
pub type RailsOf<'a> = dyn Fn(&str) -> Option<Vec<Color32>> + 'a;

/// Whether a dropdown offers a choice, by its saved name.
type Offers<'a> = dyn Fn(&str) -> bool + 'a;

/// Whether a setting's dropdown offers a choice, by its saved name.
pub type OffersOf<'a> = dyn Fn(&SettingDef, &str) -> bool + 'a;

/// The choices `def`'s dropdown offers in `cx`.
fn offers_of<'a>(cx: &'a GridCx, def: &'a SettingDef) -> Option<impl Fn(&str) -> bool + 'a> {
    cx.offers.map(|o| move |choice: &str| o(def, choice))
}

/// What the user did in a settings grid: values changed (pasted ones too), keyframes asked for
/// from a row's menu, and groups folded.
#[derive(Default)]
pub struct GridOutcome {
    pub changes: Vec<(&'static str, Value)>,
    /// The settings whose menu's Create Keyframe was picked: a key at the playhead holding the
    /// value as it stands.
    pub new_keys: Vec<&'static str>,
    /// The settings whose menu's Remove all keyframes was picked.
    pub cleared: Vec<&'static str>,
    /// Groups folded shut (true) or opened from their heading's arrow.
    pub folds: Vec<(&'static str, bool)>,
    /// Settings that start following sound (or stop, with `None`) from their menu.
    pub follows: Vec<(&'static str, Option<bpe_core::PulseSource>)>,
    /// What was asked of the settings that vary across the balls (or start or stop varying).
    pub varies: Vec<(&'static str, crate::vary::VaryEdit)>,
}

/// The description of the setting `key` among `defs`: a companion key (`"colours~bands"`) is
/// answered through its gradient's description, as the companion's own.
pub fn def_in<'a>(defs: &'a [SettingDef], key: &str) -> Option<&'a SettingDef> {
    match bpe_core::gradient_extras::split(key) {
        Some((base, c)) => {
            let base = defs.iter().find(|d| d.key == base)?;
            (base.ty == ValueType::Gradient).then(|| crate::gradient_tabs::companion_def(base, c))
        }
        None => defs.iter().find(|d| d.key == key),
    }
}

/// `electric_piano` to "Electric piano".
pub fn pretty(name: &str) -> String {
    let mut text = name.replace('_', " ");
    // "flat2 d" (a choice named Flat2D) reads "flat 2D".
    for dims in ["2", "3"] {
        let split = format!("{dims} d");
        if let Some(at) = text.find(&split)
            && text[at + split.len()..].chars().next().is_none_or(|c| c == ' ')
        {
            let space = if at > 0 && !text[..at].ends_with(' ') { " " } else { "" };
            text.replace_range(at..at + split.len(), &format!("{space}{dims}D"));
        }
    }
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// One row of a settings grid: a setting, or a heading over the group that follows.
pub enum Row<'a> {
    Group(&'static str),
    Setting(&'a SettingDef),
}

/// The rows a settings grid shows: every setting that `applies`, ungrouped first, then the
/// groups each under its heading: the sections in [`SECTION_ORDER`](bpe_core::SECTION_ORDER),
/// then any other groups (a thing kind's own) in the order they appear. A group with no row
/// shown has no heading.
pub fn rows_for<'a>(defs: &'a [SettingDef], applies: &dyn Fn(&SettingDef) -> bool) -> Vec<Row<'a>> {
    let shown: Vec<&SettingDef> = defs.iter().filter(|d| applies(d)).collect();
    let mut rows: Vec<Row<'a>> = shown.iter().filter(|d| d.group.is_empty()).map(|d| Row::Setting(d)).collect();
    let mut groups: Vec<&str> = Vec::new();
    for d in &shown {
        if !d.group.is_empty() && !groups.contains(&d.group) {
            groups.push(d.group);
        }
    }
    // A stable sort, so a paint's own variation stays beside the others, in appearance order.
    groups.sort_by_key(|g| section_rank(g).unwrap_or(usize::MAX));
    for group in groups {
        let members: Vec<Row<'a>> = shown.iter().filter(|d| d.group == group).map(|d| Row::Setting(d)).collect();
        if !members.is_empty() {
            rows.push(Row::Group(group));
            rows.extend(members);
        }
    }
    rows
}

/// The unit `def` shows. A gradient's "Colours end at" (a key ending in `span`) is a distance in
/// pixels when its paint follows Place or Start; otherwise the def's own unit.
pub fn unit_for(def: &SettingDef, value_of: &dyn Fn(&str) -> Option<Value>) -> &'static str {
    if let Some(prefix) = def.key.strip_suffix("span")
        && let Some(Value::Choice(follow)) = value_of(&format!("{prefix}follow"))
        && matches!(follow.as_str(), "place" | "start")
    {
        return "px";
    }
    def.unit
}

/// What a setting's keys look like, for its label and its right-click menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyMark {
    /// Keys are allowed on the setting.
    pub can_key: bool,
    /// The setting has keys.
    pub keyed: bool,
    /// The setting pulses with the hits.
    pub pulsed: bool,
}

/// How wide the room is that [`grid_row_with`] keeps at the end of a widget cell.
pub const TRAILING_W: f32 = 44.0;

/// What one grid row asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowAction {
    Nothing,
    /// `value` holds the new value: edited, put back to its default, or pasted.
    Changed,
    /// A key at the playhead, holding the value as it stands.
    CreateKey,
    /// Every key of the setting goes.
    RemoveKeys,
    /// Follow this part of the sound (or stop following, with `None`).
    Follow(Option<bpe_core::PulseSource>),
}

/// What names a control for the screen reader (and kittest): the label beside it, or words.
#[derive(Clone, Copy, Debug)]
pub enum Named<'a> {
    By(egui::Id),
    Words(&'a str),
}

impl Named<'_> {
    fn on(self, ui: &Ui, r: Response) -> Response {
        match self {
            Named::By(id) => r.labelled_by(id),
            Named::Words(words) => {
                let words = words.to_owned();
                ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(words));
                r
            }
        }
    }
}
