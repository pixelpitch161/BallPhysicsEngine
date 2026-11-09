//! The inspector: the selected thing's header, how it hangs in the tree, its kind's settings and
//! one card per slot it has. Every card's settings are the thing's own.

use crate::selection::Selection;
use crate::setting_snap::Snapper;
use crate::style;
use crate::widgets::{
    GridCx, KeyMark, TargetChoices, fold_toggle, grid_label, label_width, settings_grid, settings_like_grid,
    widget_cell,
};
use bpe_core::{
    Address, Attach, CopiedLayer, CopiedLook, FOLDED_SECTIONS, LayerId, ModuleKind, Resolved, Scene, SettingDef,
    SlotId, ThingId, TrackColour, Transform, Value, section_rank,
};
use bpe_kit::Kit;
use eframe::egui::{self, DragValue, RichText, Ui};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Inspector {
    /// What is folded shut, by thing: its kind's settings card (`None`) and each slot's card
    /// (the group `""`), the root's cards by name, and groups of settings inside them by their
    /// heading.
    pub collapsed: BTreeSet<(ThingId, Option<SlotId>, &'static str)>,
    /// The quiet sections ([`FOLDED_SECTIONS`](bpe_core::FOLDED_SECTIONS)) opened, which
    /// otherwise stay folded.
    pub opened: BTreeSet<(ThingId, Option<SlotId>, &'static str)>,
    /// A keyed setting was edited this frame, so the key landed at the playhead; the studio
    /// pauses so a drag doesn't leave a key on every frame.
    pub keyed_edit: bool,
    /// This frame's edit starts an undo step of its own, though the last edit was a moment ago:
    /// a brush stroke, or the start of a handle drag.
    pub own_step: bool,
    /// The card the jump bar was clicked on, scrolled to when its header is next drawn.
    pub jump_to: Option<Card>,
    /// Each card header's top in the last frame, relative to the scroll area's top.
    pub card_tops: Vec<(Card, f32)>,
    /// The card last jumped to from the bar. The bar marks it until the next scroll, even when
    /// it is too short to reach the top.
    pub jumped: Option<(ThingId, Card)>,
    /// The layer of each thing's layered Look shown in the Look editor; its Body when none is,
    /// or when the one stored has gone.
    pub selected_layer: BTreeMap<ThingId, LayerId>,
    /// The layers of the Look editor whose sub-layers are folded away, by thing (editor state,
    /// never saved).
    pub folded_pieces: BTreeSet<(ThingId, LayerId)>,
    /// The layer or whole look last copied in the Look editor, kept while the selection changes.
    pub clipboard: crate::look_editor::Clipboard,
    /// The layer being renamed in place, and the name typed so far.
    pub renaming_layer: Option<(ThingId, LayerId, String)>,
    /// The rename field takes the keyboard on its next frame.
    pub focus_layer_rename: bool,
    /// The lighting scene being renamed in place (its place in the list) and the name typed so far.
    pub renaming_scene: Option<(usize, String)>,
    /// The Look editor's view of the ball.
    pub ball_view: crate::look_editor::BallView,
    /// Whether each Balls thing's look shows on one ball or across its group.
    pub look_mode: BTreeMap<ThingId, crate::look_editor::LookMode>,
    /// The group layer of each thing's Look shown in Group mode; its first when none is.
    pub selected_group_layer: BTreeMap<ThingId, LayerId>,
    /// The view of the whole group, in Group mode.
    pub group_view: crate::look_editor::GroupView,
    /// The window's GPU, for the ball view; none in tests.
    pub render: Option<eframe::egui_wgpu::RenderState>,
    /// Each thing's tab: its settings, its ready-made looks or its sound.
    pub tab: BTreeMap<ThingId, InspectorTab>,
    /// The Looks tab's own state.
    pub looks: crate::looks_page::LooksPage,
    /// The Sound tab's own state.
    pub sound: crate::sound_tab::SoundTab,
    /// Keys clicked on a Sound tab's keyboard this frame, for the studio to play: (shape, note).
    pub auditions: Vec<(ThingId, u8)>,
    /// The sound's levels, for the Sound tab's meter; none without speakers.
    pub meters: Option<std::sync::Arc<bpe_sound::Meters>>,
    /// Whether playback is running (the Sound tab outlines the slot playing).
    pub playing: bool,
    /// The Tags field's text while it is being typed in, by thing; set on the thing when the
    /// field lets go of the keyboard.
    pub tags_draft: Option<(ThingId, String)>,
    /// Each emitter's Emitter card tab.
    pub emitter_tab: BTreeMap<ThingId, emitter_card::EmitterTab>,
    /// What is being dragged on an emitter's mini view, and on which emitter.
    pub emitter_drag: Option<(ThingId, emitter_card::mini_view::MiniDrag)>,
    /// Each emitter's rounds as the playback has them, for the rounds strip; the studio fills it
    /// every frame for the selected emitter.
    pub rounds: BTreeMap<ThingId, Vec<bpe_kit::things::emitter::sim::RoundMark>>,
    /// What each aiming emitter's aim should warn about, as the playback has it; the studio fills
    /// it every frame for the selected emitter, like `rounds`.
    pub aim_checks: BTreeMap<ThingId, bpe_kit::things::emitter::aim::AimCheck>,
    /// "Preview final frame" was pressed on the Lighting card (the app draws it).
    pub final_frame_wanted: bool,
    /// What is typed in the Find a setting box; while it holds something, the settings it finds
    /// show in place of the cards.
    pub find: String,
}

mod add_card;
mod edits;
/// The Emitter card.
pub(crate) mod emitter_card;
/// The Find a setting box.
mod find;
mod header;
mod own;
mod placement;
mod slots;
#[cfg(test)]
mod tests;

use edits::*;
pub(crate) use edits::{Edit, clear_keys, set_or_key};
use own::*;
use slots::*;

/// What the inspector shows of a thing with tabs (a shape, a Balls group, an emitter, a Particles thing, a Text).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorTab {
    #[default]
    Settings,
    /// Ready-made looks, on Balls, Emitters and Shapes.
    Looks,
    Sound,
}

impl Inspector {
    /// Whether `primary` shows its Sound tab (a shape, a Text, a Particles, Balls or Emitter thing) or its Looks
    /// tab (Balls, Emitters, Shapes): the inspector is wide then.
    pub fn wide_open(&self, scene: &Scene, primary: Option<ThingId>) -> bool {
        primary.is_some_and(|id| {
            scene.things.get(&id).is_some_and(|t| {
                let tab = self.tab.get(&id).copied().unwrap_or_default();
                tab == InspectorTab::Sound && sounds(&t.kind)
                    || tab == InspectorTab::Looks && bpe_kit::ready_looks::wears(&t.kind).is_some()
            })
        })
    }
}

/// The tabs of things of `kind`, with their names and what they show; none for a thing without tabs.
fn tabs(kind: &str) -> Vec<(InspectorTab, &'static str, &'static str)> {
    let looks = bpe_kit::ready_looks::wears(kind).is_some();
    let mut out = Vec::new();
    let settings = match kind {
        SHAPE => "The shape's outline, look and how it moves",
        bpe_kit::things::text::KIND => "The words, their look and what balls do at them",
        bpe_kit::things::particles::KIND => "Where the particles come from, how they move and look",
        bpe_kit::things::emitter::KIND => "What it fires, how its balls look and move, what it catches",
        _ => "The balls' size, look and how they move",
    };
    if looks || sounds(kind) {
        out.push((InspectorTab::Settings, "Settings", settings));
    }
    if looks {
        out.push((InspectorTab::Looks, "Looks", "Ready-made looks to start from"));
    }
    if sounds(kind) {
        let sound = match kind {
            SHAPE => "The sound the shape makes when balls hit it",
            bpe_kit::things::text::KIND => "The sound the letters make when balls hit them",
            bpe_kit::things::emitter::KIND => "The sound its balls make when they are fired, hit things or are caught",
            bpe_kit::things::particles::KIND => "The sound the particles make when they hit things, land or burst",
            _ => "The sound its balls make when they hit things",
        };
        out.push((InspectorTab::Sound, "Sound", sound));
    }
    out
}

/// One of the inspector's cards, as the jump bar names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    /// The kind's own settings, on every thing but the root.
    Thing,
    Placement,
    /// One of the root's cards of its own settings, by name: one of [`bpe_kit::ROOT_CARDS`].
    Section(&'static str),
    Slot(SlotId),
    /// Motion and Bounce shown as one card ([`PHYSICS`]); the scene still keeps the two.
    Physics,
}

/// The slots whose cards show together as one Physics card, in the order their settings read.
const PHYSICS: [SlotId; 2] = [SlotId::Motion, SlotId::Bounce];

/// Whether `slot` of a thing of kind `kind` shows in the one Physics card: a Particles thing
/// shows its Motion (forces) and Meets things as cards of their own.
fn in_physics(kind: &str, slot: SlotId) -> bool {
    PHYSICS.contains(&slot) && kind != bpe_kit::things::particles::KIND
}

/// The slots of [`PHYSICS`] the thing has, in that order.
fn physics_slots(thing: &bpe_core::Thing) -> Vec<SlotId> {
    PHYSICS.into_iter().filter(|s| thing.slots.contains_key(s) && in_physics(&thing.kind, *s)).collect()
}

impl Card {
    /// The key that folds the card shut in [`Inspector::collapsed`].
    fn fold_key(self, id: ThingId) -> (ThingId, Option<SlotId>, &'static str) {
        match self {
            Card::Thing => (id, None, ""),
            Card::Placement => (id, None, "placement"),
            Card::Section(name) => (id, None, name),
            Card::Slot(slot) => (id, Some(slot), ""),
            Card::Physics => (id, None, "physics"),
        }
    }
}

/// Draws the inspector for the selection's primary thing and applies its edits, to every
/// selected thing where that makes sense. Values are shown as they are at `time` (the
/// playhead), and an edit to a keyed setting writes a key there. True when the scene changed.
pub fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    scene: &mut Scene,
    kit: &Kit,
    resolved: &Resolved,
    selection: &Selection,
    time: f64,
) -> bool {
    // Nothing inside may widen the panel: labels wrap at one width, the same in every card.
    let panel = ui.available_width();
    ui.set_max_width(panel);
    let lw = label_width(panel);
    let reg = &kit.registry;
    let Some((id, thing)) = selection.primary.and_then(|id| scene.things.get(&id).map(|t| (id, t.clone()))) else {
        ui.add_space(8.0);
        ui.label(RichText::new("Select something in the tree to see its settings.").color(style::MUTED));
        return false;
    };
    let Some(kind) = reg.thing(&thing.kind) else {
        ui.label(RichText::new(format!("Its kind \"{}\" isn't installed.", thing.kind)).color(style::TEXT));
        return false;
    };
    let selected = selection.all(scene);
    let choices = TargetChoices::from_scene(scene, reg);
    let mut edits = Vec::new();
    if thing.kind == bpe_kit::things::text::KIND {
        // The Font picker draws each name in its own font.
        style::ensure_text_fonts(ui.ctx(), scene);
    }

    // A thing's settings, its ready-made looks or its sound.
    let chips = tabs(&thing.kind);
    if !chips.is_empty() {
        let mut tab = inspector.tab.get(&id).copied().unwrap_or_default();
        if !chips.iter().any(|c| c.0 == tab) {
            tab = InspectorTab::Settings;
        }
        ui.horizontal(|ui| {
            ui.add_space(CARD_MARGIN as f32);
            crate::look_editor::ball_view::segmented(ui, &mut tab, &chips);
        });
        ui.add_space(4.0);
        inspector.tab.insert(id, tab);
        match tab {
            InspectorTab::Sound => return crate::sound_tab::show(ui, inspector, scene, kit, &choices, id, time),
            InspectorTab::Looks => return crate::looks_page::show(ui, inspector, scene, kit, &selected, id, time),
            InspectorTab::Settings => {}
        }
    }

    find::find_box(ui, inspector);
    let finding = inspector.find.trim().to_owned();
    let marked =
        if finding.is_empty() { jump_bar(ui, inspector, id, kind, &thing) } else { marked_card(inspector, id) };
    inspector.card_tops.clear();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if selected.len() > 1 {
            ui.label(RichText::new(format!("Editing {} things", selected.len())).color(style::MUTED).small())
                .on_hover_text("Settings changed here apply to every selected thing that has them.");
        }
        header::show(ui, inspector, (id, &thing), kind, lw, &mut edits);
        ui.add_space(6.0);

        let own = |key: &str| Address { thing: id, slot: None, key: key.into() };
        let applies = |d: &SettingDef| d.applies(Some(&thing.kind), &|k| scene.value_at_time(reg, &own(k), time));
        let cx = OwnCx {
            scene,
            kit,
            resolved,
            choices: &choices,
            id,
            thing: &thing,
            time,
            label_width: lw,
            applies: &applies,
        };
        if !finding.is_empty() {
            find::results(ui, inspector, &cx, kind, &finding, &mut edits);
            return;
        }
        own::kind_card(ui, inspector, &cx, kind, &mut edits);
        placement::show(ui, inspector, &cx, kind, &mut edits);
        slots::cards(ui, inspector, &cx, kind, &mut edits);
        add_card::menu(ui, &cx, kind, &mut edits);
    });
    // A jump to a card that wasn't drawn (it went away) is dropped.
    inspector.jump_to = None;
    // The bar marked the card at the top as of the last frame; once scrolling has moved
    // another card there, draw again so the bar catches up.
    if marked_card(inspector, id) != marked {
        ui.ctx().request_repaint();
    }
    take_requests(ui, &mut edits);
    edits::apply(scene, reg, inspector, (id, &selected), time, edits)
}

/// The files picked in the Picture and Font pickers: a PNG chosen for a Picture is kept in the
/// scene, and Load font and Remove in the Font picker change the scene.
fn take_requests(ui: &Ui, edits: &mut Vec<Edit>) {
    if let Some(picked) = crate::picture_picker::take_request(ui.ctx()) {
        if crate::picture_picker::is_png(&picked.bytes) {
            crate::picture_picker::set_note(ui.ctx(), None);
            edits.push(Edit::LoadPicture(picked.name, picked.bytes));
        } else {
            crate::picture_picker::set_note(ui.ctx(), Some(crate::picture_picker::NOT_A_PICTURE));
        }
    }

    match crate::font_picker::take_request(ui.ctx()) {
        Some(crate::font_picker::FontRequest::Load(bytes, stem)) => {
            if bpe_kit::text::fonts::is_font(&bytes) {
                let name = bpe_kit::text::fonts::family_name(&bytes).unwrap_or(stem);
                crate::font_picker::set_note(ui.ctx(), None);
                edits.push(Edit::LoadFont(name, bytes.into()));
            } else {
                crate::font_picker::set_note(ui.ctx(), Some(crate::font_picker::NOT_A_FONT));
            }
        }
        Some(crate::font_picker::FontRequest::Remove(name)) => edits.push(Edit::RemoveFont(name)),
        None => {}
    }
}

/// The kind of a group of balls.
const BALLS: &str = "balls";
/// The kind that fires balls of its own design.
const EMITTER: &str = bpe_kit::things::emitter::KIND;

/// Whether things of `kind` wear a ball Look with a Ball / Group switch.
pub(crate) fn has_ball_look(kind: &str) -> bool {
    bpe_kit::things::balls::rows::has_balls(kind)
}

/// The kind whose outline settings show under its Look's Body.
const SHAPE: &str = bpe_kit::things::shape::KIND;

/// Whether things of `kind` have a Sound tab: shapes, Text, Particles things (particles part 7),
/// and Balls and Emitters (W10).
pub(crate) fn sounds(kind: &str) -> bool {
    kind == SHAPE
        || kind == bpe_kit::things::particles::KIND
        || kind == bpe_kit::things::text::KIND
        || bpe_kit::sound::hears_own_balls(kind)
}

/// What drawing a thing's own settings reads.
pub(crate) struct OwnCx<'a> {
    scene: &'a Scene,
    kit: &'a Kit,
    resolved: &'a Resolved,
    choices: &'a TargetChoices,
    id: ThingId,
    thing: &'a bpe_core::Thing,
    time: f64,
    label_width: f32,
    /// Whether a setting does anything here; rows that don't are left out.
    applies: &'a dyn Fn(&SettingDef) -> bool,
}

/// The arrow at the left of a card's title that folds the card's settings away and back. True
/// while the card is open.
pub(crate) fn fold_arrow<K: Ord + Copy>(ui: &mut Ui, collapsed: &mut BTreeSet<K>, key: K, title: &str) -> bool {
    let open = !collapsed.contains(&key);
    if fold_toggle(ui, open, title) {
        if open {
            collapsed.insert(key);
        } else {
            collapsed.remove(&key);
        }
        return !open;
    }
    open
}

/// Whether a section stays folded until opened: one of [`FOLDED_SECTIONS`], or a paint's own
/// variation, which ranks with Variation.
fn quiet(group: &str) -> bool {
    section_rank(group).is_some_and(|rank| FOLDED_SECTIONS.iter().any(|f| section_rank(f) == Some(rank)))
}

/// Whether a group of a card's settings is folded shut: a quiet section unless opened, any
/// other group when folded.
fn is_folded(inspector: &Inspector, (id, slot): (ThingId, Option<SlotId>), group: &str) -> bool {
    let has = |set: &BTreeSet<(ThingId, Option<SlotId>, &'static str)>| {
        set.iter().any(|&(i, s, g)| i == id && s == slot && g == group)
    };
    if quiet(group) { !has(&inspector.opened) } else { has(&inspector.collapsed) }
}

/// The groups of a card's settings that are folded shut, as [`is_folded`] says.
pub(crate) fn folded_groups(
    inspector: &Inspector,
    card: (ThingId, Option<SlotId>),
    defs: &[SettingDef],
) -> BTreeSet<&'static str> {
    defs.iter().map(|d| d.group).filter(|g| !g.is_empty() && is_folded(inspector, card, g)).collect()
}

/// Folds shut or opens the groups of a card's settings, as their headings asked.
pub(crate) fn fold_groups(
    inspector: &mut Inspector,
    (id, slot): (ThingId, Option<SlotId>),
    folds: &[(&'static str, bool)],
) {
    for &(group, shut) in folds {
        let key = (id, slot, group);
        match (quiet(group), shut) {
            (true, true) => inspector.opened.remove(&key),
            (true, false) => inspector.opened.insert(key),
            (false, true) => inspector.collapsed.insert(key),
            (false, false) => inspector.collapsed.remove(&key),
        };
    }
}

/// The groups of a card's settings that are not sections, for the debug check that every group
/// is one. Every card kind should have none.
fn unknown_groups(defs: &[SettingDef]) -> Vec<&'static str> {
    defs.iter().map(|d| d.group).filter(|g| !g.is_empty() && section_rank(g).is_none()).collect()
}

/// The row of buttons above the inspector that jump to its cards: the thing's own card (the
/// root's six cards), then one per slot it has. The card last clicked is selected until the
/// next scroll, then the card whose header has scrolled to the top (or just past). A click
/// unfolds the card and scrolls it to the top when it's next drawn. Returns the selected card.
fn jump_bar(
    ui: &mut Ui,
    inspector: &mut Inspector,
    id: ThingId,
    kind: &dyn bpe_core::ThingKind,
    thing: &bpe_core::Thing,
) -> Card {
    let scrolled = ui.input(|i| i.smooth_scroll_delta.y != 0.0) && ui.rect_contains_pointer(ui.max_rect());
    if scrolled {
        inspector.jumped = None;
    }
    let at_top = marked_card(inspector, id);
    // Placement has no button: it sits just under the thing's own card. The root's own
    // settings are six cards, each with its own button.
    let own: Vec<(Card, &str)> = if id == ThingId::ROOT {
        bpe_kit::ROOT_CARDS.into_iter().map(|name| (Card::Section(name), name)).collect()
    } else if kind.id() == BALLS && layered_look(thing) {
        // A Balls thing with a layered Look has no card of its own: Thing goes to Placement.
        vec![(Card::Placement, "Thing")]
    } else {
        vec![(Card::Thing, "Thing")]
    };
    // Motion and Bounce share one button, Physics, at the first of them.
    let mut cards = own;
    for s in SlotId::ALL.into_iter().filter(|s| s.in_card_list() && thing.slots.contains_key(s)) {
        let entry =
            if in_physics(kind.id(), s) { (Card::Physics, "Physics") } else { (Card::Slot(s), kind.slot_label(s)) };
        if !cards.contains(&entry) {
            cards.push(entry);
        }
    }
    ui.horizontal_wrapped(|ui| {
        // Placement counts as the thing's own card.
        let own = |c: Card| if c == Card::Placement { Card::Thing } else { c };
        for (card, label) in cards {
            if ui.selectable_label(own(card) == own(at_top), label).clicked() {
                inspector.collapsed.remove(&card.fold_key(id));
                inspector.jump_to = Some(card);
                inspector.jumped = Some((id, card));
            }
        }
    });
    ui.add_space(4.0);
    at_top
}

/// The card the jump bar marks on `id`: the one last jumped to, else the one at the top.
fn marked_card(inspector: &Inspector, id: ThingId) -> Card {
    match inspector.jumped {
        Some((thing, card)) if thing == id => card,
        _ => card_at_top(&inspector.card_tops),
    }
}

/// The last card whose header is at or above the scroll area's top, or the first card drawn
/// when none is. Placement counts as part of the thing's own card.
fn card_at_top(tops: &[(Card, f32)]) -> Card {
    let first = tops.first().map_or(Card::Thing, |(c, _)| *c);
    match tops.iter().rfind(|(_, top)| *top <= 1.0).map_or(first, |(c, _)| *c) {
        Card::Placement => Card::Thing,
        card => card,
    }
}

/// Notes where a card's header was drawn, and scrolls it to the top when the jump bar asked to.
fn card_header(ui: &Ui, inspector: &mut Inspector, card: Card, header: &egui::Response) {
    inspector.card_tops.push((card, header.rect.top() - ui.clip_rect().top()));
    if inspector.jump_to == Some(card) {
        // egui leaves the item spacing above a rect scrolled to the top; shifting the rect down
        // by that much puts the header's top right at the scroll area's top.
        let gap = ui.spacing().item_spacing.y;
        ui.scroll_to_rect(header.rect.translate(egui::vec2(0.0, gap)), Some(egui::Align::TOP));
        inspector.jump_to = None;
    }
}

/// The space inside a card's edge.
const CARD_MARGIN: i8 = 10;

/// After a folded section's title, how many of its settings this thing changed.
fn folded_summary(ui: &mut Ui, open: bool, changed: usize) {
    if !open && changed > 0 {
        ui.label(RichText::new(format!("{changed} changed")).small().color(style::MUTED));
    }
}

/// A flat section of settings: no fill or corners, a soft line under it.
fn card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    let margin = egui::Margin { left: CARD_MARGIN, right: CARD_MARGIN, top: 0, bottom: 6 };
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        // As wide as the scroll area shows, less the margin: a card above that couldn't fit
        // stretches the scroll area's insides, and the cards under it mustn't follow.
        let shown = ui.clip_rect().right() - f32::from(CARD_MARGIN) - ui.max_rect().left();
        ui.set_width(ui.available_width().min(shown).max(0.0));
        add_contents(ui);
    });
    style::divider(ui);
}
