//! The inspector: the selected thing's header, how it hangs in the tree, its kind's settings and
//! one card per slot it has. Every card's settings are the thing's own.

use crate::selection::Selection;
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
    /// Each shape's tab: its settings or its sound.
    pub tab: BTreeMap<ThingId, InspectorTab>,
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
}

/// The Emitter card.
#[path = "emitter_card/mod.rs"]
pub(crate) mod emitter_card;

/// What the inspector shows of a shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorTab {
    #[default]
    Settings,
    Sound,
}

impl Inspector {
    /// Whether `primary` is a shape showing its Sound tab (the inspector is wide then).
    pub fn sound_open(&self, scene: &Scene, primary: Option<ThingId>) -> bool {
        primary.is_some_and(|id| {
            scene.things.get(&id).is_some_and(|t| t.kind == SHAPE)
                && self.tab.get(&id).copied().unwrap_or_default() == InspectorTab::Sound
        })
    }
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

/// The slots of [`PHYSICS`] the thing has, in that order.
fn physics_slots(thing: &bpe_core::Thing) -> Vec<SlotId> {
    PHYSICS.into_iter().filter(|s| thing.slots.contains_key(s)).collect()
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

/// A change the panel asks for, applied once the drawing is done.
pub(crate) enum Edit {
    Name(String),
    /// The thing's track colour; only the primary thing's.
    Colour(TrackColour),
    Tags(BTreeSet<String>),
    Enabled(bool),
    /// Drawn on the screen or in the world; `None` is the kind's own choice.
    Screen(Option<bool>),
    Transform(Transform),
    /// What draws over what: higher over lower.
    ZOrder(i32),
    Attach(Attach),
    /// One of the kind's own settings, for every selected thing of the kind.
    SetOwn(String, Value),
    /// One of the primary thing's own settings, for it alone: what was dragged in its Group view.
    SetOwnHere(String, Value),
    /// The card in a slot becomes another kind, for every selected thing whose card there is of
    /// the same kind.
    SetKind(SlotId, &'static str),
    /// One of a card's settings, for every selected thing whose card in the slot is of the same
    /// kind.
    /// A layer's setting (a layered key, "<layer>/<key>") reaches only the selected things whose
    /// card has a layer of that id and the same kind.
    SetSlot(SlotId, String, Value),
    /// A key at the playhead holding the setting's value as it stands (a slot's setting, or the
    /// thing's own with `None`), for every selected thing that has the setting.
    NewKey(Option<SlotId>, String),
    /// The setting (a slot's, or the thing's own with `None`) follows a part of a shape's sound,
    /// or stops following with `None`, for every selected thing that has the setting.
    Follow(Option<SlotId>, String, Option<bpe_core::PulseSource>),
    /// A card for a slot the thing doesn't have yet, with its kind's starting settings, for
    /// every selected thing of the kind without one.
    AddSlot(SlotId),
    /// Takes the card off every selected thing whose card in the slot is of the same kind.
    RemoveSlot(SlotId),
    // The layers of a layered card. These change the primary thing's card only, and select the
    // layer they add.
    AddLayer(SlotId, &'static str),
    RemoveLayer(SlotId, LayerId),
    /// A sub-layer of the kind named on a layer, last of its sub-layers.
    AddSubLayer(SlotId, LayerId, &'static str),
    /// A sub-layer to the place given among its layer's sub-layers, 0 the first.
    MoveSubLayer(SlotId, LayerId, usize),
    /// A copied sub-layer onto a layer, last of its sub-layers; then, with `Some(at)`, moved to
    /// that place among them.
    PasteSubLayer(SlotId, LayerId, CopiedLayer, Option<usize>),
    /// To the index given, counted from the bottom of the card's layers.
    MoveLayer(SlotId, LayerId, usize),
    DuplicateLayer(SlotId, LayerId),
    RenameLayer(SlotId, LayerId, String),
    LayerEnabled(SlotId, LayerId, bool),
    PasteLayer(SlotId, CopiedLayer),
    PasteLook(SlotId, CopiedLook),
    /// The balls of the group that wear a drawn layer, by number; none is every ball.
    LayerOnly(SlotId, LayerId, Vec<u32>),
    /// A new drawn layer that only these balls of the group wear.
    AddLayerFor(SlotId, &'static str, Vec<u32>),
}

/// The things an edit to `id`'s setting reaches: every selected thing of `id`'s kind for its own
/// settings (`slot` is `None`), or with a card of the same kind in `slot`.
fn reached(scene: &Scene, selected: &[ThingId], id: ThingId, slot: Option<SlotId>) -> Vec<ThingId> {
    let what = |t: &ThingId| -> Option<&str> {
        let thing = scene.things.get(t)?;
        match slot {
            None => Some(thing.kind.as_str()),
            Some(slot) => thing.slots.get(&slot).map(|c| c.kind.as_str()),
        }
    };
    let Some(mine) = what(&id) else { return Vec::new() };
    selected.iter().copied().filter(|t| what(t) == Some(mine)).collect()
}

/// The things an edit to `id`'s setting `key` in `slot` reaches: as [`reached`], and for a
/// layer's setting only those whose card has a layer of the same id and kind.
fn reached_key(scene: &Scene, selected: &[ThingId], id: ThingId, slot: Option<SlotId>, key: &str) -> Vec<ThingId> {
    let mut out = reached(scene, selected, id, slot);
    if let (Some(slot), Some((layer, _))) = (slot, bpe_core::split_layered(key)) {
        let kind_of = |t: &ThingId| Some(scene.card(*t, slot)?.layer(layer)?.kind.clone());
        let mine = kind_of(&id);
        out.retain(|t| mine.is_some() && kind_of(t) == mine);
    }
    out
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

    // A shape's settings or its sound.
    if thing.kind == SHAPE {
        let mut tab = inspector.tab.get(&id).copied().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add_space(CARD_MARGIN as f32);
            crate::look_editor::ball_view::segmented(
                ui,
                &mut tab,
                &[
                    (InspectorTab::Settings, "Settings", "The shape's outline, look and how it moves"),
                    (InspectorTab::Sound, "Sound", "The sound the shape makes when balls hit it"),
                ],
            );
        });
        ui.add_space(4.0);
        inspector.tab.insert(id, tab);
        if tab == InspectorTab::Sound {
            return crate::sound_tab::show(ui, inspector, scene, kit, &choices, id, time);
        }
    }

    let marked = jump_bar(ui, inspector, id, kind, &thing);
    inspector.card_tops.clear();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if selected.len() > 1 {
            ui.label(RichText::new(format!("Editing {} things", selected.len())).color(style::MUTED).small())
                .on_hover_text("Settings changed here apply to every selected thing that has them.");
        }
        // ---------- header ----------
        // Row 1: the name, the kind and the eye; row 2: the tags.
        // Indented like a card's insides, so its columns line up with the cards' settings.
        let indent = egui::Frame::new().inner_margin(egui::Margin::symmetric(CARD_MARGIN, 0));
        indent.show(ui, |ui| {
            settings_like_grid(("header", id)).show(ui, |ui| {
                let name_label = grid_label(ui, lw, "Name");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Hiding the root would hide the whole scene; its card already says what it is.
                    if id != ThingId::ROOT {
                        if eye_toggle(ui, thing.enabled) {
                            edits.push(Edit::Enabled(!thing.enabled));
                        }
                        ui.label(RichText::new(kind.name()).small().color(style::MUTED)).on_hover_text(kind.help());
                        // The track colour, which opens a colour picker.
                        let swatch = style::swatch_button(ui, thing.colour, "Track colour")
                            .on_hover_text("This thing's colour in the tree, the preview and the timeline");
                        egui::Popup::menu(&swatch).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
                            |ui| {
                                if let Some(colour) = style::track_picker(ui, thing.colour) {
                                    edits.push(Edit::Colour(colour));
                                }
                            },
                        );
                    }
                    let mut name = thing.name.clone();
                    let edit = egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY);
                    if ui.add(edit).labelled_by(name_label.id).changed() {
                        edits.push(Edit::Name(name));
                    }
                });
                ui.end_row();
                let tags_label = grid_label(ui, lw, "Tags").on_hover_text(
                    "Words that bounce targets and the camera can pick this out by, separated by spaces",
                );
                // What is typed is kept while the field has the keyboard, and set when it lets go.
                let mut tags = match &inspector.tags_draft {
                    Some((t, draft)) if *t == id => draft.clone(),
                    _ => thing.tags.iter().cloned().collect::<Vec<_>>().join(" "),
                };
                let tag_edit = ui
                    .add(egui::TextEdit::singleline(&mut tags).desired_width(f32::INFINITY).hint_text("none"))
                    .labelled_by(tags_label.id);
                if tag_edit.lost_focus() {
                    inspector.tags_draft = None;
                    let set: BTreeSet<String> = tags.split_whitespace().map(str::to_owned).collect();
                    if set != thing.tags {
                        edits.push(Edit::Tags(set));
                    }
                } else if tag_edit.has_focus() {
                    inspector.tags_draft = Some((id, tags));
                }
                ui.end_row();
            })
        });
        ui.add_space(6.0);

        // ---------- the kind's own settings ----------
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
        if id == ThingId::ROOT {
            // The root's settings are one card per group.
            for (i, name) in bpe_kit::ROOT_CARDS.into_iter().enumerate() {
                if i > 0 {
                    ui.add_space(6.0);
                }
                root_card(ui, inspector, &cx, name, &mut edits);
            }
        } else if kind.id() == EMITTER {
            card(ui, |ui| {
                let title = kind.name().to_owned();
                let header = ui.horizontal_wrapped(|ui| {
                    let open = fold_arrow(ui, &mut inspector.collapsed, Card::Thing.fold_key(id), &title);
                    ui.label(RichText::new(&title).font(style::semibold(13.0)));
                    folded_summary(ui, open, thing.params.iter().count());
                    open
                });
                card_header(ui, inspector, Card::Thing, &header.response);
                if header.inner {
                    emitter_card::show(ui, inspector, &cx, &mut edits);
                }
            });
        } else if kind.id() == BALLS && layered_look(&thing) {
            // A Balls thing's own settings show elsewhere: Ball size by the Look card's Ball /
            // Group switch, Launch speed under Placement, the group's in Group mode.
        } else {
            card(ui, |ui| {
                // The kind's name alone: "Balls", "Folder". A shape's outline is under its Look's
                // Body, so its own card holds the rest.
                let title = if kind.id() == SHAPE { "Shape settings".to_owned() } else { kind.name().to_owned() };
                let header = ui.horizontal_wrapped(|ui| {
                    let open = fold_arrow(ui, &mut inspector.collapsed, Card::Thing.fold_key(id), &title);
                    ui.label(RichText::new(&title).font(style::semibold(13.0)));
                    folded_summary(ui, open, thing.params.iter().count());
                    open
                });
                card_header(ui, inspector, Card::Thing, &header.response);
                if header.inner {
                    if kind.id() == BALLS {
                        // With no layered Look card, the Ball / Group switch (and Ball size) and
                        // Group mode show here; the other settings are under Placement and in
                        // Group mode.
                        look_mode_switch(ui, inspector, &cx, &mut edits);
                        if group_mode(inspector, id) {
                            let look = crate::look_editor::LookCx {
                                scene,
                                kit,
                                choices: &choices,
                                id,
                                slot: SlotId::Look,
                                time,
                                label_width: lw,
                            };
                            ui.add_space(6.0);
                            crate::look_editor::group::show(ui, inspector, &look, &mut edits);
                        }
                    } else if kind.id() == SHAPE {
                        // The outline's settings show under the Look's Body.
                        let defs: Vec<SettingDef> = kind
                            .settings()
                            .iter()
                            .filter(|d| !bpe_kit::BODY_HEADINGS.contains(&d.group))
                            .cloned()
                            .collect();
                        own_grid(ui, inspector, &cx, ("kind", id), &defs, &mut edits);
                    } else {
                        own_grid(ui, inspector, &cx, ("kind", id), kind.settings(), &mut edits);
                    }
                }
            });
        }

        // ---------- placement ----------
        // The camera's place is its own Position setting, which can be keyframed.
        if id != ThingId::ROOT && kind.id() != "camera" {
            ui.add_space(6.0);
            card(ui, |ui| {
                let header = ui.horizontal(|ui| {
                    let open = fold_arrow(ui, &mut inspector.collapsed, (id, None, "placement"), "Placement");
                    ui.label(RichText::new("Placement").font(style::semibold(13.0)));
                    open
                });
                card_header(ui, inspector, Card::Placement, &header.response);
                if header.inner {
                    let mut placed = Vec::new();
                    let z = kit.draws.thing(kind.id()).is_some();
                    placement_rows(ui, scene, kind, &thing, lw, z, &mut placed);
                    // Number boxes move under any mouse button; a right-click changes nothing.
                    if !crate::widgets::right_button_busy(ui) {
                        edits.extend(placed);
                    }
                    // Balls are launched from where they are placed.
                    let launch: Vec<SettingDef> =
                        kind.settings().iter().filter(|d| PLACEMENT_KEYS.contains(&d.key)).cloned().collect();
                    if !launch.is_empty() {
                        own_grid(ui, inspector, &cx, ("placement own", id), &launch, &mut edits);
                    }
                }
            });
        }

        // ---------- one card per slot ----------
        // Motion and Bounce show as one Physics card, where the first of them would be.
        let physics = physics_slots(&thing);
        for slot in SlotId::ALL.into_iter().filter(|s| s.in_card_list()) {
            if PHYSICS.contains(&slot) {
                if physics.first() == Some(&slot) {
                    ui.add_space(6.0);
                    physics_card(ui, inspector, &cx, kind, resolved, &physics, &mut edits);
                }
                continue;
            }
            let Some(card_now) = thing.slots.get(&slot) else { continue };
            ui.add_space(6.0);
            card(ui, |ui| {
                let title = kind.slot_label(slot);
                let header = ui.horizontal_wrapped(|ui| {
                    let open = fold_arrow(ui, &mut inspector.collapsed, (id, Some(slot), ""), title);
                    ui.label(RichText::new(title).font(style::semibold(13.0)));
                    folded_summary(ui, open, card_now.params.iter().count());
                    // The Type menu: what kind of card fills the slot, when more than one fits.
                    let fitting: Vec<&dyn ModuleKind> = reg.kinds_fitting(slot, kind).collect();
                    if let Some(current) = reg.module(&card_now.kind)
                        && fitting.len() > 1
                    {
                        ui.add_space(4.0);
                        let label = ui.label(RichText::new("Type").small().color(style::MUTED));
                        ui.menu_button(current.name(), |ui| {
                            ui.set_min_width(180.0);
                            for k in &fitting {
                                if ui
                                    .selectable_label(k.id() == current.id(), k.name())
                                    .on_hover_text(k.help())
                                    .clicked()
                                {
                                    edits.push(Edit::SetKind(slot, k.id()));
                                    ui.close();
                                }
                            }
                        })
                        .response
                        .labelled_by(label.id)
                        .on_hover_text("What kind of settings fill this card");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let remove = egui::Button::new(RichText::new("Remove").small()).frame(false);
                        let remove = ui.add(remove).on_hover_text(format!("Take {title} off this thing"));
                        remove.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Remove {title}"))
                        });
                        if remove.clicked() {
                            edits.push(Edit::RemoveSlot(slot));
                        }
                    });
                    open
                });
                card_header(ui, inspector, Card::Slot(slot), &header.response);
                let problems: Vec<String> = resolved
                    .problems
                    .iter()
                    .filter(|p| p.thing == id && p.slot == Some(slot))
                    .map(crate::tree::problem_words)
                    .collect();
                if !problems.is_empty() {
                    ui.label(RichText::new(problems.join("\n")).small().color(style::TEXT));
                }
                if !header.inner {
                    return;
                }
                if card_now.kind == bpe_kit::modules::look::shape::CARD {
                    ui.add_space(4.0);
                    let look =
                        crate::look_editor::LookCx { scene, kit, choices: &choices, id, slot, time, label_width: lw };
                    crate::look_editor::show(ui, inspector, &look, &mut edits);
                    return;
                }
                if card_now.kind == bpe_kit::modules::look::layers::CARD {
                    ui.add_space(4.0);
                    if has_ball_look(kind.id()) {
                        look_mode_switch(ui, inspector, &cx, &mut edits);
                        ui.add_space(4.0);
                    }
                    let look =
                        crate::look_editor::LookCx { scene, kit, choices: &choices, id, slot, time, label_width: lw };
                    if has_ball_look(kind.id()) && group_mode(inspector, id) {
                        crate::look_editor::group::show(ui, inspector, &look, &mut edits);
                    } else {
                        crate::look_editor::show(ui, inspector, &look, &mut edits);
                    }
                    return;
                }
                slot_grid(ui, inspector, &cx, slot, &card_now.kind, &mut edits);
            });
        }

        // ---------- + Add slot ----------
        ui.add_space(8.0);
        let missing: Vec<SlotId> = kind
            .slots()
            .iter()
            .copied()
            .filter(|s| !thing.slots.contains_key(s) && reg.kinds_fitting(*s, kind).next().is_some())
            .collect();
        // Motion and Bounce are one entry, Physics, at the first of them. It adds every Physics
        // card the kind uses, so each selected thing gets whichever it lacks.
        let mut entries: Vec<(&str, Vec<SlotId>, String)> = Vec::new();
        for &slot in &missing {
            if !PHYSICS.contains(&slot) {
                let label = kind.slot_label(slot);
                entries.push((label, vec![slot], format!("Adds {label} to this thing")));
            } else if !entries.iter().any(|(label, ..)| *label == "Physics") {
                let all: Vec<SlotId> = PHYSICS
                    .into_iter()
                    .filter(|s| kind.slots().contains(s) && reg.kinds_fitting(*s, kind).next().is_some())
                    .collect();
                let lacks: Vec<&str> =
                    missing.iter().filter(|s| PHYSICS.contains(s)).map(|s| kind.slot_label(*s)).collect();
                let help = format!("Adds {} to this thing's Physics", lacks.join(" and "));
                entries.push(("Physics", all, help));
            }
        }
        ui.add_enabled_ui(!missing.is_empty(), |ui| {
            ui.menu_button("+ Add slot", |ui| {
                for (label, slots, help) in &entries {
                    if ui.button(*label).on_hover_text(help).clicked() {
                        edits.extend(slots.iter().map(|s| Edit::AddSlot(*s)));
                        ui.close();
                    }
                }
            })
            .response
            .on_disabled_hover_text("Every slot this kind can use is already shown");
        });
    });
    // A jump to a card that wasn't drawn (it went away) is dropped.
    inspector.jump_to = None;
    // The bar marked the card at the top as of the last frame; once scrolling has moved
    // another card there, draw again so the bar catches up.
    if marked_card(inspector, id) != marked {
        ui.ctx().request_repaint();
    }

    let mut changed = false;
    for edit in edits {
        changed |= match edit {
            Edit::Name(name) => {
                scene.set_thing_name(id, &name);
                true
            }
            Edit::Colour(colour) => set_thing(scene, id, |t| t.colour = Some(colour)),
            Edit::Tags(tags) => set_thing(scene, id, |t| t.tags = tags),
            Edit::Enabled(enabled) => set_thing(scene, id, |t| t.enabled = enabled),
            Edit::Screen(screen) => set_thing(scene, id, |t| t.screen = screen),
            Edit::Transform(t) => set_thing(scene, id, |thing| thing.transform = t),
            Edit::ZOrder(z) => set_thing(scene, id, |thing| thing.z_order = z),
            Edit::Attach(attach) => crate::tree::reattach(scene, id, attach).is_ok(),
            Edit::SetOwn(key, value) => {
                let mut any = false;
                for t in reached(scene, &selected, id, None) {
                    let a = Address::own(t, &key);
                    let set = set_or_key(scene, reg, &a, value.clone(), time, &mut inspector.keyed_edit);
                    any |= set;
                    if set {
                        any |= shape_afresh(scene, t, &key);
                    }
                }
                any
            }
            Edit::SetOwnHere(key, value) => {
                set_or_key(scene, reg, &Address::own(id, &key), value, time, &mut inspector.keyed_edit)
            }
            Edit::SetKind(slot, kind) => {
                let mut any = false;
                for t in reached(scene, &selected, id, Some(slot)) {
                    any |= scene.set_slot_kind(reg, t, slot, kind).is_ok();
                }
                any
            }
            Edit::SetSlot(slot, key, value) => {
                let mut any = false;
                for t in reached_key(scene, &selected, id, Some(slot), &key) {
                    let a = Address::on(t, slot, &key);
                    any |= set_or_key(scene, reg, &a, value.clone(), time, &mut inspector.keyed_edit);
                }
                any
            }
            Edit::NewKey(slot, key) => {
                let mut any = false;
                for t in reached_key(scene, &selected, id, slot, &key) {
                    let a = Address { thing: t, slot, key: key.clone() };
                    any |= new_key(scene, reg, &a, time);
                }
                any
            }
            Edit::Follow(slot, key, source) => {
                let mut any = false;
                for t in reached_key(scene, &selected, id, slot, &key) {
                    let a = Address { thing: t, slot, key: key.clone() };
                    any |= crate::follow::set(scene, reg, &a, source);
                }
                any
            }
            Edit::AddSlot(slot) => {
                let mut any = false;
                for t in reached(scene, &selected, id, None) {
                    if !scene.things[&t].slots.contains_key(&slot) {
                        any |= bpe_kit::fill_slot(scene, reg, t, slot).is_ok();
                    }
                }
                any
            }
            Edit::RemoveSlot(slot) => {
                let mut any = false;
                for t in reached(scene, &selected, id, Some(slot)) {
                    any |= scene.remove_slot(t, slot);
                }
                any
            }
            edit => apply_layer_edit(scene, reg, inspector, id, edit),
        };
    }
    changed
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

/// After a shape's own setting `key` was set: a new Sides or Start from starts the outline afresh,
/// and an emptied outline is taken away rather than kept. In the same edit, so one undo brings
/// the moulded outline back. True when it took one away.
fn shape_afresh(scene: &mut Scene, id: ThingId, key: &str) -> bool {
    let Some(thing) = scene.things.get_mut(&id).filter(|t| t.kind == SHAPE) else { return false };
    let emptied = matches!(thing.params.get("path"), Some(Value::Path(p)) if p.is_empty());
    if matches!(key, "sides" | "start") || (key == "path" && emptied) {
        thing.params.remove("path").is_some()
    } else {
        false
    }
}

/// The Ball / Group switch of a Balls thing's look, with its Ball size under it (shown in both
/// modes).
fn look_mode_switch(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, edits: &mut Vec<Edit>) {
    use crate::look_editor::LookMode;
    let (id, lw) = (cx.id, cx.label_width);
    let mut mode = inspector.look_mode.get(&id).copied().unwrap_or_default();
    settings_like_grid(("look mode", id)).show(ui, |ui| {
        let label = grid_label(ui, lw, "Edit")
            .on_hover_text("One ball's look, or the whole group: where the balls start and their colours");
        ui.horizontal(|ui| {
            crate::look_editor::ball_view::segmented(
                ui,
                &mut mode,
                &[
                    (LookMode::Ball, "Ball", "One ball's look, layer by layer"),
                    (LookMode::Group, "Group", "The whole group: where the balls start and the colours across them"),
                ],
            );
        })
        .response
        .labelled_by(label.id);
        ui.end_row();
    });
    inspector.look_mode.insert(id, mode);
    if let Some(kind) = cx.kit.registry.thing(&cx.thing.kind) {
        let size: Vec<SettingDef> = kind
            .settings()
            .iter()
            .filter(|d| d.key == BALL_SIZE)
            .map(|d| SettingDef { label: "Ball size", ..d.clone() })
            .collect();
        own_grid(ui, inspector, cx, ("ball size", id), &size, edits);
    }
}

/// Whether `thing`'s Look card is a layered one (the Ball Look editor).
fn layered_look(thing: &bpe_core::Thing) -> bool {
    thing.slots.get(&SlotId::Look).is_some_and(|c| c.kind == bpe_kit::modules::look::layers::CARD)
}

/// A Balls thing's size setting, shown beside its Ball / Group switch.
const BALL_SIZE: &str = "radius";
/// A thing's own settings shown under Placement: where balls are launched.
const PLACEMENT_KEYS: [&str; 1] = ["velocity"];

/// Whether thing `id`'s look shows across its group.
fn group_mode(inspector: &Inspector, id: ThingId) -> bool {
    inspector.look_mode.get(&id) == Some(&crate::look_editor::LookMode::Group)
}

/// Applies a change to the layers of `id`'s card, selecting the layer it adds. True when the
/// scene changed.
fn apply_layer_edit(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    inspector: &mut Inspector,
    id: ThingId,
    edit: Edit,
) -> bool {
    let added = match edit {
        Edit::AddLayer(slot, kind) => scene.add_layer(reg, id, slot, kind).map(Some),
        Edit::RemoveLayer(slot, layer) => scene.remove_layer(reg, id, slot, layer).map(|_| None),
        Edit::AddSubLayer(slot, parent, kind) => scene.add_sub_layer(reg, id, slot, parent, kind).map(Some),
        Edit::MoveSubLayer(slot, layer, to) => scene.move_sub_layer(id, slot, layer, to).map(|_| None),
        Edit::PasteSubLayer(slot, parent, copied, at) => {
            scene.paste_sub_layer(reg, id, slot, parent, &copied).and_then(|sub| {
                if let Some(at) = at {
                    scene.move_sub_layer(id, slot, sub, at)?;
                }
                Ok(Some(sub))
            })
        }
        Edit::MoveLayer(slot, layer, to) => scene.move_layer(reg, id, slot, layer, to).map(|_| None),
        Edit::DuplicateLayer(slot, layer) => scene.duplicate_layer(reg, id, slot, layer).map(Some),
        Edit::RenameLayer(slot, layer, name) => scene.rename_layer(reg, id, slot, layer, &name).map(|_| None),
        Edit::LayerEnabled(slot, layer, on) => scene.set_layer_enabled(reg, id, slot, layer, on).map(|_| None),
        Edit::PasteLayer(slot, copied) => scene.paste_layer(reg, id, slot, &copied).map(Some),
        Edit::PasteLook(slot, copied) => {
            // The old layers are gone, so the body is selected.
            inspector.selected_layer.remove(&id);
            scene.paste_look(reg, id, slot, &copied).map(|_| None)
        }
        Edit::LayerOnly(slot, layer, only) => scene.set_layer_only(reg, id, slot, layer, only).map(|_| None),
        Edit::AddLayerFor(slot, kind, only) => scene.add_layer(reg, id, slot, kind).and_then(|layer| {
            scene.set_layer_only(reg, id, slot, layer, only)?;
            Ok(Some(layer))
        }),
        _ => unreachable!("only layer edits come here"),
    };
    match added {
        Ok(Some(layer)) => {
            // A group layer is picked in Group mode, any other in the ball's lists.
            let card = scene.things.get(&id).and_then(|t| t.slots.values().find(|c| c.layer(layer).is_some()));
            let kind = card.and_then(|c| c.layer(layer)).and_then(|l| reg.layer(&l.kind));
            if kind.is_some_and(|k| k.section() == bpe_core::LayerSection::Group) {
                inspector.selected_group_layer.insert(id, layer);
            } else {
                inspector.selected_layer.insert(id, layer);
            }
            true
        }
        Ok(None) => true,
        Err(_) => false,
    }
}

/// Writes `value` to `a`: as a key at `time` when the setting has keys (and notes so in
/// `keyed`), else as its plain value.
pub(crate) fn set_or_key(
    scene: &mut Scene,
    reg: &bpe_core::Registry,
    a: &Address,
    value: Value,
    time: f64,
    keyed: &mut bool,
) -> bool {
    if scene.keyed(a) {
        *keyed = true;
        scene.set_key(reg, a, time, value).is_ok()
    } else {
        scene.set_at(reg, a, value).is_ok()
    }
}

/// The Placement card's rows: where the thing sits, as a position of its own or an offset along
/// for the ride, how it is turned, how it hangs off its parent, and whether it sticks to the
/// screen.
fn placement_rows(
    ui: &mut Ui,
    scene: &Scene,
    kind: &dyn bpe_core::ThingKind,
    thing: &bpe_core::Thing,
    lw: f32,
    z_order: bool,
    edits: &mut Vec<Edit>,
) {
    let right = ui.available_rect_before_wrap().right();
    settings_like_grid(("placement", thing.id)).show(ui, |ui| {
        position_rows(ui, scene, thing, lw, right, edits);
        if z_order {
            let label = grid_label(ui, lw, "Z-order").on_hover_text(
                "What draws on top: a thing with a higher z-order draws over one with a lower z-order. \
                 At the same z-order, shapes draw under balls",
            );
            let mut z = thing.z_order;
            let range = bpe_core::Thing::Z_ORDER_RANGE;
            if ui.add(DragValue::new(&mut z).speed(0.1).range(range)).labelled_by(label.id).changed() {
                edits.push(Edit::ZOrder(z));
            }
            ui.end_row();
        }
        if kind.can_stick_to_screen() {
            let label = grid_label(ui, lw, "Stick to screen")
                .on_hover_text("Stay put on the screen while the camera moves, like a title");
            let mut on = thing.sticks_to_screen(kind);
            if style::switch(ui, &mut on).labelled_by(label.id).changed() {
                edits.push(Edit::Screen((on != kind.sticks_to_screen()).then_some(on)));
            }
            ui.end_row();
        }
    });
}

/// Where the thing sits and how it hangs off its parent: Position (for a grouped thing),
/// Rotation, Attach, and a rider's Offset, Turns with it and Item.
fn position_rows(ui: &mut Ui, scene: &Scene, thing: &bpe_core::Thing, lw: f32, right: f32, edits: &mut Vec<Edit>) {
    let mut t = thing.transform;
    if let Attach::Group = thing.attach {
        let position = grid_label(ui, lw, "Position")
            .on_hover_text("Where it sits in the world. At Zoom 1 the shot is 1080 across");
        // In a narrow panel y goes under x.
        let moved = widget_cell(ui, right, |ui| {
            ui.horizontal_wrapped(|ui| {
                let x = ui.add(DragValue::new(&mut t.position.x).speed(1.0).prefix("x ").suffix(" px"));
                let y = ui.add(DragValue::new(&mut t.position.y).speed(1.0).prefix("y ").suffix(" px"));
                x.labelled_by(position.id).changed() || y.labelled_by(position.id).changed()
            })
            .inner
        });
        ui.end_row();
        if moved {
            edits.push(Edit::Transform(t));
        }
    }
    let rotation = grid_label(ui, lw, "Rotation").on_hover_text("How far it is turned, clockwise");
    let mut degrees = t.rotation.to_degrees();
    let turned = ui.add(DragValue::new(&mut degrees).speed(1.0).suffix("°")).labelled_by(rotation.id).changed();
    if turned {
        t.rotation = degrees.to_radians();
        edits.push(Edit::Transform(t));
    }
    ui.end_row();

    let parent = thing.parent.unwrap_or(ThingId::ROOT);
    let rides = thing.rides();
    grid_label(ui, lw, "Attach").on_hover_text("Grouped things keep their own place; a rider follows its parent");
    widget_cell(ui, right, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(!rides, "Grouped").clicked() && rides {
                edits.push(Edit::Attach(Attach::Group));
            }
            let ride = ui.add_enabled(parent != ThingId::ROOT, egui::Button::selectable(rides, "Rides on it"));
            let why = "Scene settings don't move, so there is nothing to ride";
            if ride.on_disabled_hover_text(why).clicked() && !rides {
                let ride = Attach::Ride { offset: glam::Vec2::ZERO, turn_with_parent: true, item: None };
                edits.push(Edit::Attach(ride));
            }
        });
    });
    ui.end_row();

    if let Attach::Ride { offset, turn_with_parent, item } = thing.attach {
        let mut o = offset;
        let label = grid_label(ui, lw, "Offset").on_hover_text("Where it sits from its parent, in video pixels");
        let moved = widget_cell(ui, right, |ui| {
            ui.horizontal_wrapped(|ui| {
                let x = ui.add(DragValue::new(&mut o.x).speed(1.0).prefix("x ").suffix(" px"));
                let y = ui.add(DragValue::new(&mut o.y).speed(1.0).prefix("y ").suffix(" px"));
                x.labelled_by(label.id).changed() || y.labelled_by(label.id).changed()
            })
            .inner
        });
        ui.end_row();
        let label = grid_label(ui, lw, "Turns with it").on_hover_text("Swing around as the parent turns");
        let mut turns = turn_with_parent;
        let turned = style::switch(ui, &mut turns).labelled_by(label.id).changed();
        ui.end_row();
        let mut picked = item;
        let mut item_changed = false;
        if thing.parent.and_then(|p| scene.things.get(&p)).is_some_and(|p| p.kind == "balls") {
            let label =
                grid_label(ui, lw, "Item").on_hover_text("Ride one ball of the parent: its number, or -1 for all");
            let mut n: i64 = item.map_or(-1, |i| i as i64);
            if ui.add(DragValue::new(&mut n).range(-1..=9999)).labelled_by(label.id).changed() {
                picked = (n >= 0).then_some(n as u32);
                item_changed = true;
            }
            ui.end_row();
        }
        if moved || turned || item_changed {
            edits.push(Edit::Attach(Attach::Ride { offset: o, turn_with_parent: turns, item: picked }));
        }
    }
}

/// The header's eye: open while the thing is shown. True when it was clicked.
fn eye_toggle(ui: &mut Ui, shown: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, shown, "Shown"));
    let ink = if response.hovered() { style::TEXT } else { style::MUTED };
    crate::tree::paint_eye(ui.painter(), rect, shown, ink);
    let help = if shown {
        "Shown. A hidden thing is left out of the video and the physics."
    } else {
        "Hidden: left out of the video and the physics. Click to show it."
    };
    response.on_hover_text(help).clicked()
}

/// What a setting's keys look like, for its label and its menu.
fn mark_for(scene: &Scene, def: &SettingDef, a: &Address) -> KeyMark {
    KeyMark { can_key: def.flags.keyframe, keyed: scene.keyed(a), pulsed: scene.pulses.contains_key(a) }
}

/// A key at `time` holding the setting's value there, replacing a key already at that time.
fn new_key(scene: &mut Scene, reg: &bpe_core::Registry, a: &Address, time: f64) -> bool {
    let Some(value) = scene.value_at_time(reg, a, time) else { return false };
    scene.set_key(reg, a, time, value).is_ok()
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

/// Motion and Bounce as one card, Physics: its header folds, counts and removes both, and each
/// card's settings grid shows inside it, Motion's first. The scene still keeps the two cards.
fn physics_card(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    kind: &dyn bpe_core::ThingKind,
    resolved: &Resolved,
    slots: &[SlotId],
    edits: &mut Vec<Edit>,
) {
    let (reg, id) = (&cx.kit.registry, cx.id);
    card(ui, |ui| {
        let header = ui.horizontal_wrapped(|ui| {
            let open = fold_arrow(ui, &mut inspector.collapsed, Card::Physics.fold_key(id), "Physics");
            ui.label(RichText::new("Physics").font(style::semibold(13.0)));
            let changed = slots.iter().filter_map(|s| cx.thing.slots.get(s)).map(|c| c.params.iter().count()).sum();
            folded_summary(ui, open, changed);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let remove = egui::Button::new(RichText::new("Remove").small()).frame(false);
                let remove = ui.add(remove).on_hover_text("Take Physics off this thing");
                remove.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Remove Physics"));
                if remove.clicked() {
                    edits.extend(slots.iter().map(|s| Edit::RemoveSlot(*s)));
                }
            });
            open
        });
        card_header(ui, inspector, Card::Physics, &header.response);
        let problems: Vec<String> = resolved
            .problems
            .iter()
            .filter(|p| p.thing == id && p.slot.is_some_and(|s| slots.contains(&s)))
            .map(crate::tree::problem_words)
            .collect();
        if !problems.is_empty() {
            ui.label(RichText::new(problems.join("\n")).small().color(style::TEXT));
        }
        if !header.inner {
            return;
        }
        for &slot in slots {
            let Some(card_now) = cx.thing.slots.get(&slot) else { continue };
            // Each slot has one kind today; a second would need a Type menu here.
            debug_assert!(reg.kinds_fitting(slot, kind).count() <= 1, "{slot:?} needs a Type menu in Physics");
            slot_grid(ui, inspector, cx, slot, &card_now.kind, edits);
        }
    });
}

/// A plain card's settings: the card in `slot`, of kind `card_kind`, as a grid with folding
/// groups. Its edits are pushed to `edits`.
fn slot_grid(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, slot: SlotId, card_kind: &str, edits: &mut Vec<Edit>) {
    let (scene, reg, id, time) = (cx.scene, &cx.kit.registry, cx.id, cx.time);
    let Some(ckind) = reg.module(card_kind) else { return };
    let defs = ckind.settings();
    debug_assert!(
        unknown_groups(defs).is_empty(),
        "{} has groups that are not sections: {:?}",
        card_kind,
        unknown_groups(defs)
    );
    ui.add_space(4.0);
    let at = |key: &str| Address::on(id, slot, key);
    let get = |key: &str| scene.value_at_time(reg, &at(key), time).expect("a card's settings all resolve");
    let marks = |key: &str| {
        let def = defs.iter().find(|d| d.key == key)?;
        Some(mark_for(scene, def, &at(key)))
    };
    let applies = |d: &SettingDef| d.applies(Some(&cx.thing.kind), &|k| scene.value_at_time(reg, &at(k), time));
    let shut = folded_groups(inspector, (id, Some(slot)), defs);
    let folded = |group: &str| shut.contains(group);
    let follow_menu = crate::follow::menu(scene);
    let follow_now = |key: &str| crate::follow::now(scene, &at(key));
    let follow = crate::follow::FollowCx { menu: &follow_menu, now: &follow_now };
    let grid = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: &applies,
        folded: Some(&folded),
        label_width: cx.label_width,
        track: style::colour_of(scene, id),
        follow: Some(&follow),
    };
    let out = settings_grid(ui, ("slot", id, slot), defs, get, &grid);
    fold_groups(inspector, (id, Some(slot)), &out.folds);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetSlot(slot, key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(Some(slot), key.into())));
    edits.extend(out.follows.into_iter().map(|(key, source)| Edit::Follow(Some(slot), key.into(), source)));
}

/// A grid of some of a thing's own settings, with folding groups. Its edits are pushed to
/// `edits`.
fn own_grid(
    ui: &mut Ui,
    inspector: &mut Inspector,
    cx: &OwnCx,
    grid: impl std::hash::Hash + std::fmt::Debug,
    defs: &[SettingDef],
    edits: &mut Vec<Edit>,
) {
    let (scene, reg, id, time) = (cx.scene, &cx.kit.registry, cx.id, cx.time);
    let own = |key: &str| Address { thing: id, slot: None, key: key.into() };
    let get = |key: &str| scene.value_at_time(reg, &own(key), time).expect("the kind's own settings all resolve");
    let marks = |key: &str| {
        let def = defs.iter().find(|d| d.key == key)?;
        Some(mark_for(scene, def, &own(key)))
    };
    let folded = |group: &str| is_folded(inspector, (id, None), group);
    let follow_menu = crate::follow::menu(scene);
    let follow_now = |key: &str| crate::follow::now(scene, &own(key));
    let follow = crate::follow::FollowCx { menu: &follow_menu, now: &follow_now };
    let grid_cx = GridCx {
        choices: cx.choices,
        marks: &marks,
        applies: cx.applies,
        folded: Some(&folded),
        label_width: cx.label_width,
        track: style::colour_of(scene, id),
        follow: Some(&follow),
    };
    let out = settings_grid(ui, grid, defs, get, &grid_cx);
    fold_groups(inspector, (id, None), &out.folds);
    edits.extend(out.changes.into_iter().map(|(key, value)| Edit::SetOwn(key.into(), value)));
    edits.extend(out.new_keys.into_iter().map(|key| Edit::NewKey(None, key.into())));
    edits.extend(out.follows.into_iter().map(|(key, source)| Edit::Follow(None, key.into(), source)));
}

/// One of the root's cards: the root's own settings whose group is `name`, under a title that
/// folds them away.
fn root_card(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, name: &'static str, edits: &mut Vec<Edit>) {
    let Some(kind) = cx.kit.registry.thing(&cx.thing.kind) else { return };
    // Blanked, so no heading inside repeats the card's title.
    let defs: Vec<SettingDef> =
        kind.settings().iter().filter(|d| d.group == name).map(|d| SettingDef { group: "", ..d.clone() }).collect();
    let card_kind = Card::Section(name);
    card(ui, |ui| {
        let header = ui.horizontal_wrapped(|ui| {
            let open = fold_arrow(ui, &mut inspector.collapsed, card_kind.fold_key(cx.id), name);
            ui.label(RichText::new(name).font(style::semibold(13.0)));
            open
        });
        card_header(ui, inspector, card_kind, &header.response);
        if header.inner {
            own_grid(ui, inspector, cx, ("kind", cx.id, name), &defs, edits);
        }
    });
}

fn set_thing(scene: &mut Scene, id: ThingId, f: impl FnOnce(&mut bpe_core::Thing)) -> bool {
    match scene.things.get_mut(&id) {
        Some(t) => {
            f(t);
            true
        }
        None => false,
    }
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
        let entry = if PHYSICS.contains(&s) { (Card::Physics, "Physics") } else { (Card::Slot(s), kind.slot_label(s)) };
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

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, Resolved, Settings, SlotId, Value, resolve};
    use bpe_kit::resolve_scene;
    use eframe::egui::{self, Modifiers, PointerButton, accesskit::Role};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use glam::Vec2;

    struct State {
        inspector: Inspector,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
        time: f64,
    }

    fn harness(selected: Option<ThingId>) -> Harness<'static, State> {
        harness_of(bpe_kit::new_scene(&bpe_kit::registry()), selected)
    }

    fn harness_of(scene: Scene, selected: Option<ThingId>) -> Harness<'static, State> {
        harness_sized(scene, selected, 4000.0)
    }

    fn harness_sized(scene: Scene, selected: Option<ThingId>, height: f32) -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let resolved = resolve_scene(&scene, &kit.registry);
        let mut selection = Selection::default();
        if let Some(id) = selected {
            selection.set(id);
        }
        let state = State { inspector: Inspector::default(), scene, kit, resolved, selection, time: 0.0 };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, height))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    if show(ui, &mut s.inspector, &mut s.scene, &s.kit, &s.resolved, &s.selection, s.time) {
                        s.resolved = resolve_scene(&s.scene, &s.kit.registry);
                    }
                },
                state,
            );
        h.run();
        h
    }

    /// The first slider a setting's name labels (a ball's Size shows on its Balls card and again
    /// on its Look's Body).
    fn slider_rect(h: &Harness<'_, State>, label: &str) -> egui::Rect {
        h.get_all_by_role_and_label(Role::Slider, label).next().unwrap().rect()
    }

    /// Where the test ball's Glow layer keeps its Strength.
    const BALL_GLOW: &str = "2/strength";

    /// Picks the layer named `name` in the Look editor's lists.
    fn pick_layer(h: &mut Harness<'_, State>, name: &str) {
        h.get_by_role_and_label(Role::Button, name).click();
        h.run();
    }

    /// Presses on the slider and lets go at `fraction` of its width.
    fn drag_slider(h: &mut Harness<'_, State>, rect: egui::Rect, fraction: f32) {
        let start = rect.center();
        let end = egui::pos2(rect.left() + rect.width() * fraction, rect.center().y);
        h.hover_at(start);
        h.event(egui::Event::PointerButton {
            pos: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.event(egui::Event::PointerMoved(end));
        h.step();
        h.event(egui::Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    /// The settings of the ball's Body layer (the first of its Look).
    fn ball_look(s: &State) -> bpe_core::Params {
        let card = &s.scene.things[&ThingId(2)].slots[&SlotId::Look];
        bpe_core::Params(
            card.params.iter().filter_map(|(k, v)| Some((k.strip_prefix("0/")?.to_owned(), v.clone()))).collect(),
        )
    }

    /// Right-clicks the first setting `label` names, opening its menu.
    fn menu_of(h: &mut Harness<'_, State>, label: &str) {
        h.get_all_by_role_and_label(Role::Label, label).next().unwrap().click_secondary();
        h.run();
    }

    #[test]
    fn the_header_colour_button_sets_the_track_colour() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Track colour").click();
        h.run();
        h.get_by_label("Colour hex").click();
        h.run();
        h.get_by_label("Colour hex").type_text("#12abef");
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].colour, Some(TrackColour([0x12, 0xab, 0xef])));
    }

    #[test]
    fn a_balls_colours_are_one_list() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_by_label("Colour").is_none(), "no separate Colour row");
        h.get_by_label("Add a colour").click();
        h.run();
        let p = ball_look(h.state());
        assert!(matches!(p.get("colours"), Some(Value::Gradient(g)) if g.stops.len() == 2), "{p:?}");
    }

    #[test]
    fn a_palette_from_the_menu_sets_the_colours() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Colours palette").click();
        h.run();
        h.get_by_label("Ocean").click();
        h.run();
        let ocean = bpe_kit::paint::palettes::built_in().into_iter().find(|p| p.name == "Ocean").unwrap().colours;
        let p = ball_look(h.state());
        assert_eq!(p.get("colours"), Some(&Value::Gradient(ocean)), "{p:?}");
    }

    #[test]
    fn keyed_gradient_edit_writes_a_key() {
        let mut h = harness(Some(ThingId(2)));
        let colours = Address::on(ThingId(2), SlotId::Look, "0/colours");
        let red_blue = bpe_core::Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        h.state_mut()
            .scene
            .set_key(&bpe_kit::kit().registry, &colours, 0.0, Value::Gradient(red_blue.clone()))
            .unwrap();
        h.state_mut().time = 1.0;
        h.run();
        h.get_by_label("Colours bar").click();
        h.run();
        let s = h.state();
        let keys = &s.scene.tracks[&colours].keys;
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert_eq!(keys[0].value, Value::Gradient(red_blue), "the first key is left alone");
        assert_eq!(keys[1].time, 1.0);
        let Value::Gradient(g) = &keys[1].value else { panic!("{:?}", keys[1].value) };
        assert_eq!(g.stops.len(), 3, "the key holds the whole new gradient: {g:?}");
        assert!((g.stops[1].at - 0.5).abs() < 0.01, "{g:?}");
        assert!(s.inspector.keyed_edit);
    }

    #[test]
    fn a_paint_folds_under_its_heading() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_by_label("Add a colour").is_some());
        h.get_by_label("Hide Colours").click();
        h.run();
        assert!(h.query_by_label("Add a colour").is_none(), "the paint's rows fold away");
        assert!(h.query_by_role_and_label(Role::Label, "Shape").is_some(), "rows outside the group stay");
        h.get_by_label("Show Colours").click();
        h.run();
        assert!(h.query_by_label("Add a colour").is_some());
    }

    #[test]
    fn no_dice_padlocks_diamonds_or_module_buttons_are_left() {
        let mut h = harness(Some(ThingId(2)));
        for gone in ["Randomize these settings", "Keyframe Size", "Keyframe Glow", "Make own", "Change here"] {
            assert!(h.query_by_label(gone).is_none(), "{gone}");
        }
        menu_of(&mut h, "Size");
        let items: Vec<String> = h
            .get_all_by_role(Role::Button)
            .filter_map(|n| n.accesskit_node().label())
            .filter(|l| ["Copy Value", "Paste Value", "Create Keyframe", "Lock when randomizing"].contains(&l.as_str()))
            .collect();
        assert_eq!(items, ["Copy Value", "Paste Value", "Create Keyframe"]);
    }

    #[test]
    fn every_setting_that_applies_shows_and_a_card_folds_away() {
        let mut h = harness(Some(ThingId(2)));
        // A single ball: its own Launch speed shows, the block's rows and columns don't.
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some());
        assert!(h.query_by_role_and_label(Role::Label, "Columns").is_none(), "a single ball has no columns");
        assert!(h.query_by_role_and_label(Role::Label, "Friction").is_some(), "settings once under More show too");
        h.get_by_label("Hide Placement").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_none());
        assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_none());
        assert!(h.state().inspector.collapsed.contains(&(ThingId(2), None, "placement")));
        h.get_by_label("Show Placement").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some());
    }

    #[test]
    fn a_shape_shows_only_what_does_something_for_it() {
        let h = harness(Some(ThingId(1)));
        // The shape's bounce only matters when it takes over; ball-only settings never show.
        assert!(h.query_by_role_and_label(Role::Label, "Takes over").is_some());
        for ball_only in ["Bounciness", "Grow per hit", "Hits other balls", "Pops after", "Flash"] {
            assert!(h.query_by_role_and_label(Role::Label, ball_only).is_none(), "{ball_only} shows on the shape");
        }
        // A circle has a size but no sides or corners; its outline shows once, under the Body.
        assert_eq!(h.query_all_by_role_and_label(Role::Label, "Size").count(), 1);
        assert!(h.query_by_role_and_label(Role::Label, "Sides").is_none());
        assert!(h.query_by_role_and_label(Role::Label, "Shape settings").is_some());
    }

    #[test]
    fn editing_a_slider_writes_through_set_at() {
        let mut h = harness(Some(ThingId(2)));
        let rect = slider_rect(&h, "Size");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        let v = s.scene.things[&ThingId(2)].params.get("radius").cloned();
        assert!(matches!(v, Some(Value::Number(r)) if r > 100.0), "{v:?}");
    }

    #[test]
    fn right_clicking_a_slider_opens_the_menu_and_changes_nothing() {
        let mut h = harness(Some(ThingId(2)));
        let before = h.state().scene.clone();
        let rect = slider_rect(&h, "Size");
        let at = egui::pos2(rect.right() - 4.0, rect.center().y);
        h.hover_at(at);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
        h.run();
        assert_eq!(h.state().scene, before, "a right-click never changes a value");
        assert!(h.query_by_label("Create Keyframe").is_some(), "the menu is open");
    }

    /// The test scene with the starter sound on its shape (thing 1).
    fn sounding_scene() -> Scene {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        bpe_kit::ensure_mixer(&mut scene, &reg);
        let card = bpe_kit::sound::starter_voice(&reg);
        scene.things.get_mut(&ThingId(1)).unwrap().slots.insert(SlotId::Sound, card);
        scene
    }

    #[test]
    fn follow_sound_menu_adds_a_band() {
        use bpe_core::{LayerId, PulseSource, SoundSource};
        let scene = sounding_scene();
        let shape = scene.things[&ThingId(1)].name.clone();
        let mut h = harness_of(scene, Some(ThingId(2)));
        pick_layer(&mut h, "Glow");
        menu_of(&mut h, "Strength");
        h.get_by_label("Follow sound").click();
        h.run();
        h.get_by_label(&shape).click();
        h.run();
        h.get_by_label("ADSR 1").click();
        h.run();
        let glow = Address::on(ThingId(2), SlotId::Look, BALL_GLOW);
        let pulse = &h.state().scene.pulses[&glow];
        assert_eq!(pulse.source, PulseSource::Sound { shape: ThingId(1), source: SoundSource::Modulator(LayerId(0)) });
        assert_eq!(pulse.amount, Value::Number(0.5), "a quarter of Strength's 0..2");
        assert!(h.query_by_label(&format!("Follows {shape} · ADSR 1")).is_some(), "the band shows");
        // Following a bus, then stopping.
        menu_of(&mut h, "Strength");
        h.get_by_label("Follow sound").click();
        h.run();
        h.get_by_label("Bus 1").click();
        h.run();
        let source = h.state().scene.pulses[&glow].source;
        assert!(matches!(source, PulseSource::Sound { source: SoundSource::Bus(1), .. }), "{source:?}");
        menu_of(&mut h, "Strength");
        h.get_by_label("Stop following sound").click();
        h.run();
        assert!(!h.state().scene.pulses.contains_key(&glow));
        assert!(h.query_by_label(&format!("Follows {shape} · ADSR 1")).is_none());
    }

    #[test]
    fn without_a_sounding_shape_follow_sound_is_greyed_out() {
        let mut h = harness(Some(ThingId(2)));
        menu_of(&mut h, "Size");
        assert!(h.get_by_label("Follow sound").accesskit_node().is_disabled());
    }

    #[test]
    fn create_keyframe_puts_a_key_at_the_playhead() {
        let mut h = harness(Some(ThingId(2)));
        h.state_mut().time = 2.0;
        h.run();
        menu_of(&mut h, "Size");
        h.get_by_label("Create Keyframe").click();
        h.run();
        let size = Address::own(ThingId(2), "radius");
        let keys = &h.state().scene.tracks[&size].keys;
        assert_eq!(keys.len(), 1);
        assert_eq!((keys[0].time, &keys[0].value), (2.0, &Value::Number(24.0)));
        // Again at the same moment, the key there is replaced, not doubled.
        menu_of(&mut h, "Size");
        h.get_by_label("Create Keyframe").click();
        h.run();
        assert_eq!(h.state().scene.tracks[&size].keys.len(), 1);
        // A layer's setting keys the same way.
        pick_layer(&mut h, "Glow");
        menu_of(&mut h, "Strength");
        h.get_by_label("Create Keyframe").click();
        h.run();
        assert!(h.state().scene.tracks.contains_key(&Address::on(ThingId(2), SlotId::Look, BALL_GLOW)));
        // Launch speed can't have keys, so its Create Keyframe is greyed out.
        menu_of(&mut h, "Launch speed");
        assert!(h.get_by_label("Create Keyframe").accesskit_node().is_disabled());
    }

    #[test]
    fn a_copied_value_pastes_into_another_things_setting() {
        let mut h = harness(Some(ThingId(2)));
        {
            let s = h.state_mut();
            let radius = Address::own(ThingId(2), "radius");
            s.scene.set_at(&s.kit.registry, &radius, Value::Number(75.0)).unwrap();
        }
        h.run();
        menu_of(&mut h, "Size");
        h.get_by_label("Copy Value").click();
        h.run();
        // The shape's Size sits under its Look's Body.
        h.state_mut().selection.set(ThingId(1));
        h.run();
        menu_of(&mut h, "Size");
        h.get_by_label("Paste Value (75 px)").click();
        h.run();
        let size = Address::own(ThingId(1), "size");
        assert_eq!(h.state().scene.value_at(&h.state().kit.registry, &size), Some(Value::Number(75.0)));
    }

    #[test]
    fn a_keyed_or_pulsing_setting_is_marked_for_its_label() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let size = Address::own(ThingId(2), "radius");
        let def = bpe_kit::BallsSettings::defs().iter().find(|d| d.key == "radius").unwrap();
        assert_eq!(mark_for(&scene, def, &size), KeyMark { can_key: true, keyed: false, pulsed: false });
        scene.set_key(&reg, &size, 0.0, Value::Number(40.0)).unwrap();
        assert!(mark_for(&scene, def, &size).keyed);
        let speed = bpe_kit::BallsSettings::defs().iter().find(|d| d.key == "velocity").unwrap();
        assert!(!mark_for(&scene, speed, &Address::own(ThingId(2), "velocity")).can_key);
    }

    #[test]
    fn editing_a_keyed_slider_writes_a_key_not_the_base_value() {
        let mut h = harness(Some(ThingId(2)));
        let size = Address::own(ThingId(2), "radius");
        h.state_mut().scene.set_key(&bpe_kit::kit().registry, &size, 0.0, Value::Number(40.0)).unwrap();
        h.state_mut().time = 3.0;
        h.run();
        let rect = slider_rect(&h, "Size");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        assert_eq!(s.scene.things[&ThingId(2)].params.get("radius"), None, "the plain value is left alone");
        let keys = &s.scene.tracks[&size].keys;
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert_eq!(keys[1].time, 3.0);
        assert!(matches!(keys[1].value, Value::Number(r) if r > 100.0), "{:?}", keys[1].value);
        assert!(s.inspector.keyed_edit, "the studio pauses on this, so the drag keys one moment");
    }

    #[test]
    fn a_card_can_be_removed_and_added_back() {
        let mut h = harness(Some(ThingId(2)));
        let glow = Address::on(ThingId(2), SlotId::Look, BALL_GLOW);
        h.state_mut().scene.set_key(&bpe_kit::kit().registry, &glow, 1.0, Value::Number(0.5)).unwrap();
        h.run();
        h.get_by_label("Remove Look").click();
        h.run();
        assert!(!h.state().scene.things[&ThingId(2)].slots.contains_key(&SlotId::Look));
        assert!(!h.state().scene.tracks.contains_key(&glow), "the card's keys go with it");
        h.get_by_label("+ Add slot").click();
        h.run();
        h.get_by_label("Look").click();
        h.run();
        let card = &h.state().scene.things[&ThingId(2)].slots[&SlotId::Look];
        assert_eq!((card.kind.as_str(), card.layers.len()), ("look.layers", 1), "a fresh look: its Body alone");
    }

    #[test]
    fn remove_and_add_reach_every_selected_thing() {
        let mut h = harness(Some(ThingId(2)));
        let other = {
            let s = h.state_mut();
            let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
            s.selection.extra.insert(other);
            s.resolved = resolve(&s.scene, &s.kit.registry);
            other
        };
        h.run();
        h.get_by_label("Remove Look").click();
        h.run();
        let has_look =
            |h: &Harness<'_, State>, t: ThingId| h.state().scene.things[&t].slots.contains_key(&SlotId::Look);
        assert!(!has_look(&h, ThingId(2)) && !has_look(&h, other));
        h.get_by_label("+ Add slot").click();
        h.run();
        h.get_by_label("Look").click();
        h.run();
        assert!(has_look(&h, ThingId(2)) && has_look(&h, other));
    }

    #[test]
    fn right_dragging_a_position_box_moves_nothing() {
        let mut h = harness(Some(ThingId(2)));
        let before = h.state().scene.clone();
        let rect = h.get_all_by_role_and_label(Role::SpinButton, "Position").next().unwrap().rect();
        let (from, to) = (rect.center(), rect.center() + egui::vec2(80.0, 0.0));
        h.hover_at(from);
        h.event(egui::Event::PointerButton {
            pos: from,
            button: PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.event(egui::Event::PointerMoved(to));
        h.step();
        h.event(egui::Event::PointerButton {
            pos: to,
            button: PointerButton::Secondary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
        assert_eq!(h.state().scene, before);
    }

    #[test]
    fn add_slot_lists_only_slots_the_kind_lacks_and_adds_a_starter() {
        let mut h = harness(Some(ThingId(2)));
        // A new ball has every card a ball can have: trails and particles are layers of its Look.
        assert!(h.get_by_label("+ Add slot").accesskit_node().is_disabled());
        h.state_mut().scene.things.get_mut(&ThingId(2)).unwrap().slots.remove(&SlotId::Motion);
        h.run();
        h.get_by_label("+ Add slot").click();
        h.run();
        assert!(h.query_all_by_label("Trail").next().is_none(), "trails are layers of the Look");
        assert!(h.query_all_by_label("Particles").next().is_none(), "particles are layers of the Look");
        assert!(h.query_all_by_label("Shader").next().is_none(), "a ball's shaders are layers of its Look");
        assert!(h.query_all_by_label("Motion").next().is_none(), "Motion is added as part of Physics");
        // The menu's Physics, not the card's header or the bar's button.
        h.get_all_by_label("Physics").last().expect("the Physics entry").click();
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Motion].kind, "motion.basic");
    }

    #[test]
    fn a_new_balls_card_edits_straight_away() {
        let mut h = harness(Some(ThingId(2)));
        let before = h.state().scene.things[&ThingId(2)].slots[&SlotId::Motion].params.clone();
        let rect = slider_rect(&h, "Air drag");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        assert_ne!(s.scene.things[&ThingId(2)].slots[&SlotId::Motion].params, before, "the ball's own motion changed");
    }

    #[test]
    fn typing_a_new_name_renames_the_thing() {
        let mut h = harness(Some(ThingId(2)));
        let field = h.get_by_label("Name");
        field.click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("Hero".into()));
        h.run();
        assert_eq!(h.state().scene.things[&ThingId(2)].name, "Hero");
    }

    #[test]
    fn a_mixed_selection_only_edits_things_that_have_the_setting() {
        let mut h = harness(Some(ThingId(2)));
        {
            let s = h.state_mut();
            let c = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
            s.selection.extra.extend([ThingId(1), c]);
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        assert!(h.query_by_label("Editing 3 things").is_some());
        let rect = slider_rect(&h, "Size");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        let big = |id| matches!(s.scene.things[&id].params.get("radius"), Some(Value::Number(r)) if *r > 100.0);
        assert!(big(ThingId(2)) && big(ThingId(3)));
        assert!(s.scene.things[&ThingId(1)].params.get("radius").is_none(), "the shape's Size is another setting");
    }

    #[test]
    fn a_card_edit_reaches_every_selected_thing_with_that_card() {
        let mut h = harness(Some(ThingId(1)));
        let takes_over = |t| Address::on(t, SlotId::Bounce, "takes_over");
        let (picked, other) = {
            let s = h.state_mut();
            let picked = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "shape", ThingId::ROOT).unwrap();
            let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "shape", ThingId::ROOT).unwrap();
            for t in [ThingId(1), picked, other] {
                s.scene.set_at(&s.kit.registry, &takes_over(t), Value::Bool(true)).unwrap();
            }
            s.selection.extra.insert(picked);
            s.resolved = resolve(&s.scene, &s.kit.registry);
            (picked, other)
        };
        h.run();
        let rect = slider_rect(&h, "Friction");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        let friction = |t| s.scene.value_at(&s.kit.registry, &Address::on(t, SlotId::Bounce, "friction"));
        assert!(matches!(friction(ThingId(1)), Some(Value::Number(f)) if f > 0.8), "{:?}", friction(ThingId(1)));
        assert_eq!(friction(picked), friction(ThingId(1)), "the other selected shape's bounce changed too");
        assert_ne!(friction(other), friction(ThingId(1)), "a shape that isn't selected is left alone");
    }

    #[test]
    fn a_layer_edit_reaches_only_selected_balls_with_the_same_layer() {
        let mut h = harness(Some(ThingId(2)));
        let (twin, plain) = {
            let s = h.state_mut();
            // A copy has the same layers; a new ball has its Body alone.
            let twin = crate::tree::duplicate(&mut s.scene, &s.kit.registry, ThingId(2)).unwrap();
            let plain = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
            s.selection.extra.extend([twin, plain]);
            s.resolved = resolve(&s.scene, &s.kit.registry);
            (twin, plain)
        };
        let plain_look = h.state().scene.card(plain, SlotId::Look).unwrap().clone();
        h.run();
        pick_layer(&mut h, "Glow");
        let rect = slider_rect(&h, "Strength");
        drag_slider(&mut h, rect, 0.9);
        let s = h.state();
        let glow = |t| s.scene.value_at(&s.kit.registry, &Address::on(t, SlotId::Look, BALL_GLOW));
        assert!(matches!(glow(ThingId(2)), Some(Value::Number(g)) if g > 1.0));
        assert_eq!(glow(twin), glow(ThingId(2)), "the copy's Glow changed too");
        assert_eq!(s.scene.card(plain, SlotId::Look), Some(&plain_look), "a ball with no Glow is left alone");
    }

    #[test]
    fn the_attach_rows_ride_and_regroup() {
        // The tree moves things (Move into, or a drag); the Placement card hangs them off it.
        let mut scene = bpe_kit::new_scene(&bpe_kit::registry());
        scene.set_parent(ThingId(2), ThingId(1), Attach::Group).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        assert_eq!(h.state().scene.things[&ThingId(2)].parent, Some(ThingId(1)));
        h.get_by_label("Rides on it").click();
        h.run();
        let s = h.state();
        assert!(
            matches!(s.scene.things[&ThingId(2)].attach, Attach::Ride { offset, turn_with_parent: true, .. } if offset == Vec2::new(0.0, -200.0)),
            "{:?}",
            s.scene.things[&ThingId(2)].attach
        );
        assert!(h.query_all_by_label("Offset").next().is_some() && h.query_all_by_label("Position").next().is_none());
        assert!(h.query_all_by_label("Item").next().is_none(), "a shape has no balls to ride");
        h.get_by_label("Grouped").click();
        h.run();
        let s = h.state();
        assert_eq!(s.scene.things[&ThingId(2)].attach, Attach::Group);
        assert_eq!(s.scene.things[&ThingId(2)].transform.position, Vec2::new(0.0, -200.0));
    }

    #[test]
    fn a_target_setting_has_a_picker() {
        let mut h = harness(Some(ThingId(2)));
        // The bounce card's "Which balls" target shows once the balls hit each other.
        {
            let s = h.state_mut();
            let hits = Address::on(ThingId(2), SlotId::Bounce, "hit_balls");
            s.scene.set_at(&s.kit.registry, &hits, Value::Bool(true)).unwrap();
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        // Other balls starts folded, marked as holding the change.
        h.get_by_label("Other balls (changed)").click();
        h.run();
        // The picker is a button named by its row, reading "Anything" until something is picked.
        h.get_by_role_and_label(Role::Button, "Which balls").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Kind").click();
        h.run();
        // The kind, not the ball's own Shape setting.
        h.get_all_by_role_and_label(Role::Button, "Shape").last().unwrap().click();
        h.run();
        let s = h.state();
        let bounce = &s.scene.things[&ThingId(2)].slots[&SlotId::Bounce];
        assert_eq!(bounce.params.get("balls_to_hit"), Some(&Value::Target(bpe_core::Target::Kind("shape".into()))));
    }

    #[test]
    fn scene_settings_fits_the_panel() {
        // Long labels and units once pushed the number boxes past the edge, so the panel grew.
        let h = harness(Some(ThingId::ROOT));
        for n in h.get_all_by_role(Role::SpinButton) {
            assert!(n.rect().right() <= 420.0, "{:?} reaches {}", n.accesskit_node().label(), n.rect().right());
        }
    }

    #[test]
    fn target_settings_offer_the_scene_things_and_tags() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        scene.things.get_mut(&ThingId(2)).unwrap().tags.insert("bouncers".into());
        let special = Address::on(ThingId(2), SlotId::Bounce, "special");
        scene.set_at(&reg, &special, Value::Bool(true)).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        h.get_by_label("Show Against some things").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "Those things").click();
        h.run();
        // The Target menu's Thing, not the jump bar's.
        h.get_all_by_label("Thing").last().expect("the Thing submenu").hover();
        h.run();
        // The header's Ball / Group switch is named Ball too.
        assert_eq!(h.query_all_by_label("Ball").count(), 2, "the ball is offered by name");
        h.get_by_label("Tag").hover();
        h.run();
        h.get_by_label("bouncers").click();
        h.run();
        let target = h.state().scene.things[&ThingId(2)].slots[&SlotId::Bounce].params.get("special_target").cloned();
        assert_eq!(target, Some(Value::Target(bpe_core::Target::Tag("bouncers".into()))));
    }

    #[test]
    fn variation_and_blending_start_folded_and_open_when_clicked() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_all_by_label("Variation").next().is_some(), "the Variation heading");
        assert!(h.query_by_label("Vary hue").is_none(), "Variation starts folded");
        h.get_all_by_label("Variation").next().unwrap().click();
        h.run();
        assert!(h.query_all_by_label("Vary hue").next().is_some(), "a click opens it");
        // It stays open after looking at another thing and coming back.
        h.state_mut().selection.set(ThingId(1));
        h.run();
        h.state_mut().selection.set(ThingId(2));
        h.run();
        assert!(h.query_all_by_label("Vary hue").next().is_some());
        // And a click on the heading folds it again.
        h.get_all_by_label("Variation").next().unwrap().click();
        h.run();
        assert!(h.query_by_label("Vary hue").is_none());
    }

    #[test]
    fn a_folded_section_with_changes_shows_the_dot() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.query_all_by_label("Variation").next().is_some());
        assert!(h.query_by_label("Variation (changed)").is_none(), "nothing changed yet");
        assert!(h.query_by_label("Changes inside").is_none(), "no dot with defaults");
        h.state_mut().scene.card_mut(ThingId(2), SlotId::Look).unwrap().params.set("0/vary_hue", Value::Number(20.0));
        h.run();
        assert!(h.query_by_label("Variation (changed)").is_some(), "the heading says it holds changes");
        assert!(h.query_by_label("Changes inside").is_some(), "the dot is drawn");
        // Open, the rows show for themselves, so the dot goes.
        h.get_by_label("Variation (changed)").click();
        h.run();
        assert!(h.query_by_label("Changes inside").is_none());
    }

    #[test]
    fn a_folded_section_with_a_keyframe_shows_the_dot() {
        let mut h = harness(Some(ThingId(2)));
        let wobble = Address::on(ThingId(2), SlotId::Look, "0/wobble");
        let reg = bpe_kit::registry();
        let default = h.state().scene.value_at(&reg, &wobble).unwrap();
        h.state_mut().scene.set_key(&reg, &wobble, 0.0, default).unwrap();
        h.run();
        assert!(h.query_by_label("Variation (changed)").is_some(), "a key at the default still moves the video");
        assert!(h.query_by_label("Changes inside").is_some());
    }

    #[test]
    fn a_folded_section_with_only_a_pulse_shows_the_dot() {
        // The ball's Bounce card has a Bounce section; start it at its defaults so it starts clean.
        // The other cards go.
        let mut h = harness(Some(ThingId(2)));
        let s = h.state_mut();
        s.scene.card_mut(ThingId(2), SlotId::Bounce).unwrap().params = bpe_core::Params::default();
        s.scene.remove_slot(ThingId(2), SlotId::Motion);
        s.resolved = resolve(&s.scene, &s.kit.registry);
        h.run();
        h.get_by_label("Hide Bounce").click();
        h.run();
        assert!(h.query_by_label("Bounce (changed)").is_none(), "Bounce holds no changes yet");
        let def = bpe_kit::modules::bounce::BasicBounce::defs().iter().find(|d| d.key == "bounciness").unwrap();
        assert!(def.flags.pulse && def.group == "Bounce");
        let address = Address::on(ThingId(2), SlotId::Bounce, def.key);
        let reg = bpe_kit::registry();
        h.state_mut().scene.set_pulse(&reg, &address, bpe_core::PulseSource::Hits, Value::Number(0.25), 0.25).unwrap();
        h.run();
        assert!(h.query_by_label("Bounce (changed)").is_some(), "a pulse moves the video");
        assert!(h.query_by_label("Changes inside").is_some());
    }

    #[test]
    fn the_header_eye_hides_and_shows_the_thing() {
        let mut h = harness(Some(ThingId(2)));
        assert!(h.state().scene.things[&ThingId(2)].enabled);
        h.get_by_label("Shown").click();
        h.run();
        assert!(!h.state().scene.things[&ThingId(2)].enabled, "the eye hides it");
        h.get_by_label("Shown").click();
        h.run();
        assert!(h.state().scene.things[&ThingId(2)].enabled, "and shows it again");
    }

    #[test]
    fn the_picture_card_shows_the_grade() {
        let reg = bpe_kit::registry();
        let mut h = harness_of(bpe_kit::scenes::graded_sunset(&reg), Some(ThingId::ROOT));
        // At the start the map amount is 0, yet the map still shows.
        h.run();
        let slider =
            |h: &Harness<'_, State>, label: &str| h.query_all_by_role_and_label(Role::Slider, label).count() > 0;
        assert!(h.query_by_role_and_label(Role::Label, "Picture").is_some(), "a Picture card");
        assert!(slider(&h, "Brightness"), "the Brightness setting");
        assert!(slider(&h, "Vignette"), "the Vignette setting");
        assert!(h.query_all_by_label("Gradient map").next().is_some(), "the Gradient map setting at t=0");
        // The card folds like the others.
        h.get_by_label("Hide Picture").click();
        h.run();
        assert!(!slider(&h, "Vignette"), "the grade's rows fold away");
        assert!(h.state().inspector.collapsed.contains(&(ThingId::ROOT, None, "Picture")));
        h.get_by_label("Show Picture").click();
        h.run();
        assert!(slider(&h, "Vignette"));
    }

    /// Opens the card's Type menu: the one Type button on the page.
    fn open_type_menu(h: &mut Harness<'_, State>) {
        h.get_by_role_and_label(Role::Button, "Type").click();
        h.run();
    }

    /// A new scene whose settings have an effect in their Shader slot (only the scene has one), with one of its number
    /// settings moved off its default, and that setting's address. The other effect kinds named
    /// come back too: those that lack the setting.
    fn scene_with_an_effect() -> (Scene, Address, Vec<&'static str>) {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        bpe_kit::fill_slot(&mut scene, &reg, ThingId::ROOT, SlotId::Shader).unwrap();
        let kind = reg.module(&scene.things[&ThingId::ROOT].slots[&SlotId::Shader].kind).unwrap();
        let def = kind
            .settings()
            .iter()
            .find(|d| d.key != "blend" && matches!(d.default, Value::Number(_)))
            .expect("an effect with a number setting of its own");
        let (Value::Number(n), Some((low, high))) = (&def.default, def.range) else { unreachable!() };
        let a = Address::on(ThingId::ROOT, SlotId::Shader, def.key);
        scene.set_at(&reg, &a, Value::Number(if *n == low { high } else { low })).unwrap();
        let others = reg
            .modules_for(SlotId::Shader)
            .filter(|m| m.id() != kind.id() && m.settings().iter().all(|d| d.key != def.key))
            .map(|m| m.name())
            .collect();
        (scene, a, others)
    }

    /// The name of the kind of `id`'s card in its Shader slot.
    fn effect_name(s: &State, id: ThingId) -> &'static str {
        s.kit.registry.module(&s.scene.things[&id].slots[&SlotId::Shader].kind).unwrap().name()
    }

    #[test]
    fn an_effects_card_has_a_type_menu_and_a_balls_look_has_none() {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        // Only the ball's Look, Motion and Bounce: each has one kind.
        bpe_kit::fill_slot(&mut scene, &reg, ThingId::ROOT, SlotId::Shader).unwrap();
        let mut h = harness_of(scene, Some(ThingId(2)));
        assert!(h.query_by_role_and_label(Role::Button, "Type").is_none(), "a ball's look has one kind");
        h.state_mut().selection.set(ThingId::ROOT);
        h.run();
        let root = reg.thing("scene").unwrap();
        let effects: Vec<&str> = reg.kinds_fitting(SlotId::Shader, root).map(|k| k.name()).collect();
        assert!(effects.len() > 1, "{effects:?}");
        let current = effect_name(h.state(), ThingId::ROOT);
        open_type_menu(&mut h);
        for name in &effects {
            assert!(h.query_all_by_label(name).next().is_some(), "{name} is offered");
        }
        let other = effects.iter().find(|n| **n != current).unwrap();
        h.get_all_by_label(other).last().unwrap().click();
        h.run();
        assert_eq!(effect_name(h.state(), ThingId::ROOT), *other);
    }

    #[test]
    fn picking_a_type_switches_the_card_and_back_restores_it() {
        let (scene, moved, others) = scene_with_an_effect();
        let reg = bpe_kit::registry();
        let value = scene.value_at(&reg, &moved);
        let mut h = harness_of(scene, Some(ThingId::ROOT));
        let first = effect_name(h.state(), ThingId::ROOT);
        let other = *others.first().expect("an effect without the moved setting");
        open_type_menu(&mut h);
        h.get_all_by_label(other).last().unwrap().click();
        h.run();
        {
            let s = h.state();
            assert_eq!(effect_name(s, ThingId::ROOT), other);
            assert!(s.scene.things[&ThingId::ROOT].slots[&SlotId::Shader].params.get(&moved.key).is_none());
        }
        open_type_menu(&mut h);
        h.get_all_by_label(first).last().unwrap().click();
        h.run();
        let s = h.state();
        assert_eq!(effect_name(s, ThingId::ROOT), first);
        assert_eq!(s.scene.value_at(&s.kit.registry, &moved), value, "the moved setting is back");
    }

    #[test]
    fn undo_after_a_type_switch_restores_the_card() {
        let (scene, _, others) = scene_with_an_effect();
        let before = scene.clone();
        let mut h = harness_of(scene, Some(ThingId::ROOT));
        let first = effect_name(h.state(), ThingId::ROOT);
        open_type_menu(&mut h);
        h.get_all_by_label(others[0]).last().unwrap().click();
        h.run();
        assert_eq!(effect_name(h.state(), ThingId::ROOT), others[0]);
        // The studio records any frame that changed the scene as an undo step.
        let mut history = crate::history::History::default();
        history.record(before.clone(), false);
        let s = h.state_mut();
        assert!(history.undo(&mut s.scene));
        assert_eq!(effect_name(s, ThingId::ROOT), first);
        assert_eq!(s.scene, before);
        assert!(history.redo(&mut s.scene));
        assert_eq!(effect_name(s, ThingId::ROOT), others[0]);
    }

    #[test]
    fn the_header_is_name_kind_and_shown_then_tags() {
        let h = harness(Some(ThingId(2)));
        for label in ["Name", "Shown", "Tags"] {
            assert!(h.query_all_by_label(label).next().is_some(), "{label} is in the header");
        }
        for gone in ["Parent", "Kind"] {
            assert!(h.query_by_role_and_label(Role::Label, gone).is_none(), "{gone} is no longer a row");
        }
        // The Placement card's heading sits above its Position row.
        let heading = h.get_by_role_and_label(Role::Label, "Placement").rect();
        let position = h.get_by_role_and_label(Role::Label, "Position").rect();
        assert!(heading.top() < position.top(), "Position is under Placement");
        for row in ["Rotation", "Attach", "Stick to screen"] {
            let r = h.get_by_role_and_label(Role::Label, row).rect();
            assert!(heading.top() < r.top(), "{row} is under Placement");
        }
        // A ball's own settings sit by its Look's Ball / Group switch and under Placement: no
        // Balls card.
        assert!(h.query_by_label("Hide Balls").is_none());
        assert!(h.query_by_label_contains("Balls settings").is_none());
        // Launch speed and Z-order are under Placement.
        for row in ["Launch speed", "Z-order"] {
            let r = h.get_by_role_and_label(Role::Label, row).rect();
            assert!(heading.top() < r.top(), "{row} is under Placement");
        }
    }

    #[test]
    fn the_camera_and_scene_settings_have_no_placement_card() {
        let root = harness(Some(ThingId::ROOT));
        assert!(root.query_by_role_and_label(Role::Label, "Placement").is_none());
        assert!(root.query_by_role_and_label(Role::Label, "Shown").is_none(), "the root has no eye");
        assert!(root.query_by_label("Hide Video").is_some(), "the root's first card keeps its title");
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let camera = bpe_kit::add_thing(&mut scene, &reg, "camera", ThingId::ROOT).unwrap();
        let h = harness_of(scene, Some(camera));
        assert!(h.query_by_role_and_label(Role::Label, "Placement").is_none());
        assert!(h.query_by_role_and_label(Role::Label, "Stick to screen").is_none());
        assert!(h.query_all_by_label("Shown").next().is_some(), "the camera can still be hidden");
    }

    #[test]
    fn the_placement_card_folds_away() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Hide Placement").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_none());
        assert!(h.state().inspector.collapsed.contains(&(ThingId(2), None, "placement")));
        h.get_by_label("Show Placement").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_some());
    }

    const BAR_LABELS: [&str; 13] = [
        "Thing",
        "Look",
        "Physics",
        "Motion",
        "Bounce",
        "Shader",
        "Effects",
        "Video",
        "World",
        "Background",
        "Picture",
        "Sound",
        "Mixer",
    ];

    /// The jump bar's buttons, in order, each with whether it is selected.
    fn jump_bar(h: &Harness<'_, State>) -> Vec<(String, bool)> {
        h.get_all_by_role(Role::Button)
            .filter_map(|n| {
                let node = n.accesskit_node();
                let label = node.label()?;
                let toggled = node.toggled()?;
                BAR_LABELS.contains(&label.as_str()).then(|| (label, toggled == egui::accesskit::Toggled::True))
            })
            .collect()
    }

    fn the_bar_button<'h>(h: &'h Harness<'_, State>, label: &'h str) -> egui_kittest::Node<'h> {
        h.get_all_by_role_and_label(Role::Button, label)
            .find(|n| n.accesskit_node().toggled().is_some())
            .expect("the bar's button")
    }

    #[test]
    fn slider_columns_line_up_across_cards() {
        let h = harness(Some(ThingId(2)));
        // The Look's Size, under its layer lists, and the Physics card's Gravity.
        let size = h.get_all_by_role_and_label(Role::Slider, "Size").next().unwrap().rect();
        let gravity = slider_rect(&h, "Gravity ×");
        assert!(
            (size.left() - gravity.left()).abs() <= 1.0,
            "Size starts at {}, Gravity at {}",
            size.left(),
            gravity.left()
        );
    }

    #[test]
    fn the_header_and_placement_line_up_with_the_settings() {
        let h = harness(Some(ThingId(2)));
        let label = |l: &str| h.get_all_by_role_and_label(Role::Label, l).next().unwrap().rect().left();
        let size = label("Size");
        for row in ["Name", "Tags", "Position", "Rotation"] {
            assert!((label(row) - size).abs() <= 1.0, "{row}'s label starts at {}, Size's at {size}", label(row));
        }
        let slider = slider_rect(&h, "Size").left();
        let name = h.get_by_role_and_label(Role::TextInput, "Name").rect().left();
        let position = h.get_all_by_role_and_label(Role::SpinButton, "Position").next().unwrap().rect().left();
        let rotation = h.get_by_role_and_label(Role::SpinButton, "Rotation").rect().left();
        for (row, x) in [("Name", name), ("Position", position), ("Rotation", rotation)] {
            assert!((x - slider).abs() <= 1.0, "{row}'s field starts at {x}, Size's slider at {slider}");
        }
    }

    #[test]
    fn the_jump_bar_lists_thing_then_the_slots_in_order() {
        let h = harness_of(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)));
        let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, ["Thing", "Look", "Physics"]);
        assert_eq!(jump_bar(&h)[0], ("Thing".to_owned(), true), "at the top, Thing is selected");
    }

    #[test]
    fn tags_can_be_typed_a_letter_at_a_time() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_role_and_label(Role::TextInput, "Tags").click();
        h.run();
        for letter in "red blue".chars() {
            h.get_by_role_and_label(Role::TextInput, "Tags").type_text(&letter.to_string());
            h.run();
        }
        h.key_press(egui::Key::Enter);
        h.run();
        let tags: Vec<&str> = h.state().scene.things[&ThingId(2)].tags.iter().map(String::as_str).collect();
        assert_eq!(tags, ["blue", "red"]);
    }

    #[test]
    fn a_balls_thing_button_jumps_to_its_placement() {
        let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
        the_bar_button(&h, "Physics").click();
        h.run();
        the_bar_button(&h, "Thing").click();
        h.run();
        let s = h.state();
        assert!(s.inspector.jump_to.is_none(), "the jump landed on a card that is drawn");
        let tops = &s.inspector.card_tops;
        let top = tops.iter().find(|(c, _)| *c == Card::Placement).expect("Placement's top").1;
        assert!(top.abs() <= 2.0, "Placement's header is at the top of the scroll area, at {top}");
        let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
        assert_eq!(selected, ["Thing"]);
    }

    #[test]
    fn the_jump_bar_unfolds_and_scrolls_to_physics() {
        let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
        h.state_mut().inspector.collapsed.insert(Card::Physics.fold_key(ThingId(2)));
        h.run();
        assert!(h.query_by_label("Show Physics").is_some(), "the Physics card starts folded");
        the_bar_button(&h, "Physics").click();
        h.run();
        let s = h.state();
        let open = !s.inspector.collapsed.contains(&Card::Physics.fold_key(ThingId(2)));
        assert!(open, "the Physics card is open");
        assert!(s.inspector.jump_to.is_none());
        let tops = &s.inspector.card_tops;
        let top = tops.iter().find(|(c, _)| *c == Card::Physics).expect("Physics' top").1;
        assert!(top.abs() <= 2.0, "Physics' header is at the top of the scroll area, at {top}");
        // The header's own rect sits just under the bar, where the scroll area starts.
        let header = h.get_by_label("Hide Physics").rect();
        let bar = the_bar_button(&h, "Thing").rect();
        assert!(header.top() > bar.bottom() && header.top() < bar.bottom() + 60.0, "{header:?} under {bar:?}");
        let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
        assert_eq!(selected, ["Physics"], "tops {:?} bar {:?}", h.state().inspector.card_tops, jump_bar(&h));
    }

    /// Scene settings with every slot it can have, so its cards run well past one screen.
    fn a_root_with_every_slot() -> Scene {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        bpe_kit::fill_slot(&mut scene, &reg, ThingId::ROOT, SlotId::Shader).unwrap();
        // The mixer is there too, but has a panel of its own, not a card here.
        bpe_kit::ensure_mixer(&mut scene, &reg);
        scene
    }

    /// The top of a root card's strong title.
    fn heading_top(h: &Harness<'_, State>, name: &str) -> f32 {
        h.get_by_role_and_label(Role::Label, name).rect().top()
    }

    #[test]
    fn scene_settings_shows_a_card_per_group() {
        let h = harness(Some(ThingId::ROOT));
        let mut last = f32::NEG_INFINITY;
        for name in bpe_kit::ROOT_CARDS {
            assert!(h.query_by_label(&format!("Hide {name}")).is_some(), "{name} has a fold arrow");
            let top = heading_top(&h, name);
            assert!(top > last, "{name} comes after the card before it");
            last = top;
        }
        let tops: Vec<Card> = h.state().inspector.card_tops.iter().map(|(c, _)| *c).collect();
        assert_eq!(tops, bpe_kit::ROOT_CARDS.map(Card::Section), "the cards in order, and nothing else");
        assert!(h.query_by_label("Hide Scene settings").is_none(), "no single Scene settings card");
    }

    #[test]
    fn the_root_jump_bar_lists_the_cards_then_the_slots() {
        let h = harness(Some(ThingId::ROOT));
        let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, bpe_kit::ROOT_CARDS);
        assert_eq!(jump_bar(&h)[0], ("Video".to_owned(), true), "at the top, Video is selected");
        assert!(h.query_by_role_and_label(Role::Button, "Thing").is_none(), "no Thing button on the root");
        assert!(h.query_by_role_and_label(Role::Button, "Scene settings").is_none());
        // Its slots follow, in the order their cards are drawn.
        let h = harness_of(a_root_with_every_slot(), Some(ThingId::ROOT));
        let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
        let mut want: Vec<&str> = bpe_kit::ROOT_CARDS.to_vec();
        want.push("Effects");
        assert_eq!(labels, want);
    }

    #[test]
    fn jumping_to_background_scrolls_it_to_the_top() {
        let mut h = harness_sized(a_root_with_every_slot(), Some(ThingId::ROOT), 600.0);
        the_bar_button(&h, "Background").click();
        h.run();
        let s = h.state();
        let tops = &s.inspector.card_tops;
        let top = tops.iter().find(|(c, _)| *c == Card::Section("Background")).expect("Background's top").1;
        assert!(top.abs() <= 1.0, "Background's header is at the top of the scroll area, at {top}");
        assert_eq!(selected_in_bar(&h), ["Background"]);
    }

    #[test]
    fn a_root_card_with_no_shown_rows_still_has_its_title() {
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve_scene(&scene, &kit.registry);
        let state =
            State { inspector: Inspector::default(), scene, kit, resolved, selection: Selection::default(), time: 0.0 };
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 800.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let choices = TargetChoices::from_scene(&s.scene, &s.kit.registry);
                    let thing = s.scene.things[&ThingId::ROOT].clone();
                    let cx = OwnCx {
                        scene: &s.scene,
                        kit: &s.kit,
                        resolved: &s.resolved,
                        choices: &choices,
                        id: ThingId::ROOT,
                        thing: &thing,
                        time: s.time,
                        label_width: label_width(420.0),
                        applies: &|_| false,
                    };
                    let mut edits = Vec::new();
                    root_card(ui, &mut s.inspector, &cx, "World", &mut edits);
                },
                state,
            );
        h.run();
        assert!(h.query_by_label("Hide World").is_some(), "the card keeps its fold arrow");
        assert!(h.query_by_role_and_label(Role::Label, "World").is_some(), "and its title");
        assert!(h.query_by_role_and_label(Role::Label, "Gravity").is_none(), "no rows");
        assert!(h.query_by_role_and_label(Role::Label, "Seed").is_none(), "no rows");
        assert!(h.query_all_by_role(Role::Slider).next().is_none(), "no grid row is drawn");
    }

    fn selected_in_bar(h: &Harness<'_, State>) -> Vec<String> {
        jump_bar(h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect()
    }

    fn wheel(h: &mut Harness<'_, State>, dy: f32) {
        h.hover_at(egui::pos2(200.0, 400.0));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }

    #[test]
    fn a_short_last_card_stays_marked_after_jumping_to_it() {
        let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
        // Physics with its Motion, Bounce and Growing folded is short enough not to reach the top.
        for (slot, group) in [(SlotId::Motion, "Motion"), (SlotId::Bounce, "Bounce"), (SlotId::Bounce, "Growing")] {
            h.state_mut().inspector.collapsed.insert((ThingId(2), Some(slot), group));
        }
        h.run();
        the_bar_button(&h, "Physics").click();
        h.run();
        let tops = &h.state().inspector.card_tops;
        let last = tops.iter().find(|(c, _)| *c == Card::Physics).expect("Physics' top").1;
        assert!(last > 1.0, "Physics is too short to reach the top, at {last}");
        assert_eq!(selected_in_bar(&h), ["Physics"], "the card jumped to stays marked");
        // A scroll hands the mark back to the card at the top.
        wheel(&mut h, 40.0);
        assert_ne!(selected_in_bar(&h), ["Physics"]);
        assert!(h.state().inspector.jumped.is_none());
    }

    #[test]
    fn the_jump_bar_marks_the_card_at_the_top_while_scrolling() {
        let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
        // Scroll down until Look's header has passed the top but Physics' hasn't.
        for _ in 0..200 {
            let tops = &h.state().inspector.card_tops;
            let top_of = |card| tops.iter().find(|(c, _)| *c == card).map(|(_, t)| *t);
            if top_of(Card::Slot(SlotId::Look)).is_some_and(|t| t < -1.0) {
                break;
            }
            h.hover_at(egui::pos2(200.0, 400.0));
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -40.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            });
            h.run();
        }
        let tops = &h.state().inspector.card_tops;
        let look = tops.iter().find(|(c, _)| *c == Card::Slot(SlotId::Look)).unwrap().1;
        let physics = tops.iter().find(|(c, _)| *c == Card::Physics).unwrap().1;
        assert!(look < -1.0 && physics > 1.0, "scrolled past Look's header but not Physics': {look}, {physics}");
        let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
        assert_eq!(selected, ["Look"]);
    }

    #[test]
    fn a_ball_shows_one_physics_card_with_motion_and_bounce_inside() {
        let h = harness(Some(ThingId(2)));
        let tops = &h.state().inspector.card_tops;
        assert_eq!(tops.iter().filter(|(c, _)| *c == Card::Physics).count(), 1, "one Physics card: {tops:?}");
        assert!(
            !tops.iter().any(|(c, _)| matches!(c, Card::Slot(SlotId::Motion) | Card::Slot(SlotId::Bounce))),
            "no Motion or Bounce card of their own: {tops:?}"
        );
        assert!(h.query_by_label("Hide Physics").is_some(), "the Physics card's fold arrow");
        for row in ["Gravity ×", "Bounciness"] {
            assert!(h.query_all_by_role_and_label(Role::Slider, row).next().is_some(), "{row} shows");
        }
        for heading in ["Hide Motion", "Hide Bounce", "Hide Growing", "Show Other balls", "Show Against some things"] {
            assert!(h.query_by_label(heading).is_some(), "the {heading} heading");
        }
        // Motion's settings read first.
        let gravity = slider_rect(&h, "Gravity ×").top();
        let bounciness = slider_rect(&h, "Bounciness").top();
        assert!(gravity < bounciness, "Gravity × at {gravity}, Bounciness at {bounciness}");
    }

    #[test]
    fn removing_physics_takes_both_cards_off() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Remove Physics").click();
        h.run();
        let slots = &h.state().scene.things[&ThingId(2)].slots;
        assert!(!slots.contains_key(&SlotId::Motion) && !slots.contains_key(&SlotId::Bounce));
        assert!(h.query_by_label("Hide Physics").is_none(), "no Physics card left");
    }

    #[test]
    fn adding_physics_brings_both_back() {
        let mut h = harness(Some(ThingId(2)));
        h.get_by_label("Remove Physics").click();
        h.run();
        h.get_by_label("+ Add slot").click();
        h.run();
        assert_eq!(h.query_all_by_label("Physics").count(), 1, "one Physics entry in the menu");
        h.get_by_label("Physics").click();
        h.run();
        let slots = &h.state().scene.things[&ThingId(2)].slots;
        assert_eq!(slots[&SlotId::Motion].kind, "motion.basic");
        assert_eq!(slots[&SlotId::Bounce].kind, "bounce.basic");
    }

    #[test]
    fn a_shape_shows_bounce_settings_only() {
        let h = harness(Some(ThingId(1)));
        assert_eq!(h.state().scene.things[&ThingId(1)].kind, "shape", "the new scene's thing 1 is a shape");
        assert!(h.state().inspector.card_tops.iter().any(|(c, _)| *c == Card::Physics), "a Physics card");
        assert!(h.query_all_by_label("Takes over").next().is_some());
        assert!(h.query_all_by_role_and_label(Role::Slider, "Bounciness").next().is_none(), "only with Takes over");
        assert!(h.query_all_by_label("Gravity ×").next().is_none(), "a shape has no Motion card");
        assert!(h.query_by_label("Hide Motion").is_none());
        assert!(h.query_by_label("Hide Bounce").is_some(), "the shape's Bounce settings show");
    }

    #[test]
    fn the_jump_bar_has_one_physics_chip() {
        let h = harness(Some(ThingId(2)));
        let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels.iter().filter(|l| *l == "Physics").count(), 1, "{labels:?}");
        assert!(!labels.iter().any(|l| l == "Motion" || l == "Bounce"), "{labels:?}");
    }

    #[test]
    fn editing_inside_physics_edits_the_right_card() {
        let mut h = harness(Some(ThingId(2)));
        let card = |h: &Harness<'_, State>, slot| h.state().scene.things[&ThingId(2)].slots[&slot].params.clone();
        let (motion, bounce) = (card(&h, SlotId::Motion), card(&h, SlotId::Bounce));
        let rect = slider_rect(&h, "Air drag");
        drag_slider(&mut h, rect, 0.9);
        assert_ne!(card(&h, SlotId::Motion), motion, "Air drag is Motion's");
        assert_eq!(card(&h, SlotId::Bounce), bounce, "Bounce is untouched");
        let motion = card(&h, SlotId::Motion);
        let rect = slider_rect(&h, "Bounciness");
        drag_slider(&mut h, rect, 0.2);
        assert_ne!(card(&h, SlotId::Bounce), bounce, "Bounciness is Bounce's");
        assert_eq!(card(&h, SlotId::Motion), motion, "Motion is untouched");
    }

    #[test]
    fn adding_physics_fills_every_selected_ball() {
        let mut h = harness(Some(ThingId(2)));
        {
            let s = h.state_mut();
            let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
            s.scene.remove_slot(ThingId(2), SlotId::Bounce);
            s.scene.remove_slot(other, SlotId::Motion);
            s.scene.remove_slot(other, SlotId::Bounce);
            s.selection.extra.insert(other);
            s.resolved = resolve(&s.scene, &s.kit.registry);
        }
        h.run();
        h.get_by_label("+ Add slot").click();
        h.run();
        h.get_all_by_label("Physics").last().expect("the Physics entry").click();
        h.run();
        let s = h.state();
        for t in [ThingId(2), *s.selection.extra.last().expect("the other ball")] {
            let slots = &s.scene.things[&t].slots;
            assert!(slots.contains_key(&SlotId::Motion) && slots.contains_key(&SlotId::Bounce), "{t:?}");
        }
    }
}
