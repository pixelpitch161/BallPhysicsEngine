//! The node panel: slides up under the preview and shows one module's graph, the Nodes depth
//! of a card in the inspector, or one synth patch from the library.

use super::editor::{Editor, LiveView};
use crate::fx_list::FxPlace;
use crate::theme;
use bpe_core::{
    InstrumentId, InstrumentSource, Module, ModuleGraph, ModuleId, ModuleKind, Params, Resolved, Scene, SlotId,
    SlotLink, ThingId,
};
use bpe_graph::Graph;
use bpe_kit::graphs::{bind, compile_module, is_custom, module_graph, role_of, settings_from_graph};
use bpe_kit::{Kit, RulesState};
use bpe_runtime::{World, state};
use eframe::egui::{self, RichText, Ui};
use std::collections::BTreeMap;

/// How long a block glows after it fires, in seconds.
const GLOW: f32 = 0.4;

pub struct NodePanel {
    /// What the panel shows, when it is open.
    pub open: Option<PanelTarget>,
    pub(super) editor: Editor,
    shown: Option<Shown>,
    patch: Option<ShownPatch>,
}

/// What the node panel can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTarget {
    /// The graph of the module a thing's slot holds.
    Slot(ThingId, SlotId),
    /// A synth patch in the library.
    Patch(InstrumentId),
    /// An effects graph: an instrument's, a channel's, a bus's or the master's.
    Effects(FxPlace),
}

/// A patch or effects graph on the canvas: a working copy, and the stored graph it was copied
/// from, to notice an undo from outside the panel.
struct ShownPatch {
    target: PanelTarget,
    graph: Graph,
    stored: Graph,
    status: Result<(), String>,
    node_errors: BTreeMap<u32, String>,
}

impl ShownPatch {
    fn load(target: PanelTarget, graph: &Graph) -> Self {
        let (status, node_errors) = check_patch(target, graph);
        Self { target, graph: graph.clone(), stored: graph.clone(), status, node_errors }
    }
}

/// The blocks an effects graph may use: the effects, the shaping blocks, the maths and the
/// free-running modulators, with In and Out.
fn effects_catalog() -> &'static bpe_graph::SpecList {
    static CATALOG: std::sync::OnceLock<bpe_graph::SpecList> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let keep = |s: &bpe_graph::NodeSpec| match s.category.as_str() {
            "Effects" | "Shaping" | "Maths" => true,
            "Modulators" => matches!(s.kind.as_str(), "lfo" | "follower"),
            _ => matches!(s.kind.as_str(), "fx_in" | "fx_out"),
        };
        let specs = bpe_synth::catalog().specs.iter().filter(|s| keep(s)).cloned().collect();
        bpe_graph::SpecList { specs, cycles: false }
    })
}

/// Why a patch or effects graph doesn't compile, as its first problem, and its problems by
/// block. An empty effects graph passes the sound through, so it compiles.
fn check_patch(target: PanelTarget, graph: &Graph) -> (Result<(), String>, BTreeMap<u32, String>) {
    let compiled = match target {
        PanelTarget::Effects(_) if graph.nodes.is_empty() => Ok(()),
        PanelTarget::Effects(_) => bpe_synth::compile_effects(graph).map(drop),
        _ => bpe_synth::compile(graph).map(drop),
    };
    match compiled {
        Ok(_) => (Ok(()), BTreeMap::new()),
        Err(problems) => {
            let mut node_errors = BTreeMap::new();
            for p in &problems {
                if let Some(node) = p.node {
                    node_errors.entry(node).or_insert_with(|| p.message.clone());
                }
            }
            let first = problems.first().map_or_else(|| "Doesn't compile".to_owned(), |p| p.message.clone());
            (Err(first), node_errors)
        }
    }
}

/// A patch's problems on its blocks; patches don't fire, so nothing glows.
struct PatchErrors<'a>(&'a BTreeMap<u32, String>);

impl LiveView for PatchErrors<'_> {
    fn flash(&self, _node: u32) -> f32 {
        0.0
    }

    fn badge(&self, _node: u32) -> Option<String> {
        None
    }

    fn error(&self, node: u32) -> Option<String> {
        self.0.get(&node).cloned()
    }
}

/// The graph on the canvas: a working copy of the module's, with its settings bound to the
/// module's values, and what the module held when it was copied.
struct Shown {
    module: ModuleId,
    graph: Graph,
    /// The module's stored graph and settings the copy was made from, to notice an undo or a
    /// card edit from outside the panel.
    stored: Option<ModuleGraph>,
    params: Params,
    /// Why the graph doesn't compile, as one sentence, when it doesn't.
    status: Result<(), String>,
    /// The same problems, by the block they are on.
    node_errors: BTreeMap<u32, String>,
}

impl Default for NodePanel {
    fn default() -> Self {
        Self { open: None, editor: Editor::new("panel"), shown: None, patch: None }
    }
}

/// The module a thing's slot shows: the one it links, or the one it inherits.
fn shown_module(scene: &Scene, resolved: &Resolved, thing: ThingId, slot: SlotId) -> Option<ModuleId> {
    match scene.things.get(&thing)?.slots.get(&slot)? {
        SlotLink::Linked { module, .. } => Some(*module),
        SlotLink::Inherit => resolved.things.get(&thing)?.slots.get(&slot).map(|r| r.module),
        SlotLink::Off => None,
    }
}

/// The kind's defaults with the module's own values over them.
fn full_params(kind: &dyn ModuleKind, m: &Module) -> Params {
    let mut params = Params::default();
    for d in kind.settings() {
        params.set(d.key, d.default.clone());
    }
    params.apply(&m.params);
    params
}

/// Writes the graph's setting blocks back into the module's settings, where they differ.
pub fn sync_params(graph: &Graph, m: &mut Module, kind: &dyn ModuleKind) {
    if bind(graph, &full_params(kind, m)) != *graph {
        m.params.apply(&settings_from_graph(graph, kind.settings()));
    }
}

/// Throws a module's edited graph away. The values its setting blocks held stay as the
/// module's settings, so the card picks up where the graph left off.
pub fn revert_to_template(m: &mut Module, kind: &dyn ModuleKind) {
    if let Some(g) = &m.graph {
        let graph = g.graph.clone();
        sync_params(&graph, m, kind);
    }
    m.graph = None;
}

/// The module's name, with how many things share it when more than one does.
pub fn module_words(scene: &Scene, m: &Module) -> String {
    let users = scene.users_of(m.id).len();
    if users > 1 { format!("{} · shared ×{users}", m.name) } else { m.name.clone() }
}

fn check(slot: SlotId, graph: &Graph) -> (Result<(), String>, BTreeMap<u32, String>) {
    let status = compile_module(slot, graph).map(|_| ());
    let mut node_errors = BTreeMap::new();
    if let Some(role) = role_of(slot)
        && let Err(errors) = bpe_vm::compile(graph, role)
    {
        for e in errors {
            if let Some(node) = e.node {
                node_errors.entry(node).or_insert(e.message);
            }
        }
    }
    (status, node_errors)
}

impl Shown {
    fn load(module: ModuleId, slot: SlotId, m: &Module, kind: &dyn ModuleKind) -> Option<Self> {
        let params = full_params(kind, m);
        let graph = match &m.graph {
            Some(g) if g.custom || slot == SlotId::Rules => bind(&g.graph, &params),
            // An unedited copy only remembers where the blocks were: the kind's template is
            // what runs, so a template that changes in a later version reaches every module.
            Some(g) => {
                let mut fresh = module_graph(kind, m)?.graph;
                for n in &mut fresh.nodes {
                    if let Some(old) = g.graph.node(n.id) {
                        n.pos = old.pos;
                    }
                }
                fresh
            }
            None => module_graph(kind, m)?.graph,
        };
        let (status, node_errors) = check(slot, &graph);
        Some(Self { module, graph, stored: m.graph.clone(), params: m.params.clone(), status, node_errors })
    }

    /// Stores the working copy on the module after an edit, and says whether it is now its own.
    fn store(&mut self, slot: SlotId, m: &mut Module, kind: &dyn ModuleKind, simple_toggled: Option<(u32, String)>) {
        sync_params(&self.graph, m, kind);
        // Rules have no native path, so a rules graph is always its own.
        let custom = slot == SlotId::Rules
            || match kind.template() {
                Some(t) => is_custom(&self.graph, &bind(&t, &full_params(kind, m))),
                None => true,
            };
        let mut simple = match &m.graph {
            Some(g) => g.simple.clone(),
            None => module_graph(kind, m).map(|g| g.simple).unwrap_or_default(),
        };
        if let Some(setting) = simple_toggled
            && !simple.remove(&setting)
        {
            simple.insert(setting);
        }
        m.graph = Some(ModuleGraph { graph: self.graph.clone(), template: kind.id().to_owned(), custom, simple });
        self.stored = m.graph.clone();
        self.params = m.params.clone();
        let (status, node_errors) = check(slot, &self.graph);
        self.status = status;
        self.node_errors = node_errors;
    }
}

/// What the running scene shows on the blocks: a glow where a rule just fired, counters, and
/// the problems that stop the graph compiling.
#[derive(Default)]
pub struct Flashes {
    fired: BTreeMap<u32, f32>,
    now: f32,
    badges: BTreeMap<u32, String>,
    errors: BTreeMap<u32, String>,
}

impl Flashes {
    fn gather(
        world: Option<&World>,
        thing: ThingId,
        slot: SlotId,
        resolved: &Resolved,
        graph: &Graph,
        errors: &BTreeMap<u32, String>,
    ) -> Self {
        let mut out = Flashes { errors: errors.clone(), ..Default::default() };
        let Some(world) = world else { return out };
        out.now = world.time() as f32;
        // Rules run on the thing that links them, so a thing that inherits them watches the
        // owner's.
        let owner = match slot {
            SlotId::Rules => resolved.things.get(&thing).and_then(|t| t.slots.get(&slot)).map_or(thing, |s| s.from),
            _ => thing,
        };
        let Some((kind, st)) = world.module_state(owner, slot) else { return out };
        if kind != "rules.graph" {
            return out;
        }
        let Some(vm) = &state::<RulesState>(st).vm else { return out };
        for n in &graph.nodes {
            if let Some(t) = vm.fired_at(n.id) {
                out.fired.insert(n.id, t);
            }
            if matches!(n.kind.as_str(), "do.counter" | "counter")
                && let Some(id) = n.params.get("id").and_then(bpe_graph::Param::number)
            {
                let value = vm.counter(id as u32);
                let text = if value.fract() == 0.0 { format!("{value:.0}") } else { format!("{value:.1}") };
                out.badges.insert(n.id, text);
            }
        }
        out
    }
}

impl LiveView for Flashes {
    fn flash(&self, node: u32) -> f32 {
        self.fired.get(&node).map_or(0.0, |t| 1.0 - (self.now - t) / GLOW)
    }

    fn badge(&self, node: u32) -> Option<String> {
        self.badges.get(&node).cloned()
    }

    fn error(&self, node: u32) -> Option<String> {
        self.errors.get(&node).cloned()
    }
}

impl NodePanel {
    /// Opens the panel on a thing's slot. The view starts fitted to that graph.
    pub fn open_at(&mut self, thing: ThingId, slot: SlotId) {
        if self.open != Some(PanelTarget::Slot(thing, slot)) {
            self.editor = Editor::new((thing.0, slot as u8));
            self.shown = None;
            self.patch = None;
        }
        self.open = Some(PanelTarget::Slot(thing, slot));
    }

    /// Opens the panel on a synth patch in the library.
    pub fn open_patch(&mut self, id: InstrumentId) {
        if self.open != Some(PanelTarget::Patch(id)) {
            self.editor = Editor::new(("patch", id.0));
            self.shown = None;
            self.patch = None;
        }
        self.open = Some(PanelTarget::Patch(id));
    }

    /// Opens the panel on an effects graph.
    pub fn open_effects(&mut self, place: FxPlace) {
        if self.open != Some(PanelTarget::Effects(place)) {
            self.editor = Editor::new(("effects", place));
            self.shown = None;
            self.patch = None;
        }
        self.open = Some(PanelTarget::Effects(place));
    }

    /// The thing and slot shown, when the panel shows a slot's graph.
    pub fn slot_open(&self) -> Option<(ThingId, SlotId)> {
        match self.open {
            Some(PanelTarget::Slot(thing, slot)) => Some((thing, slot)),
            _ => None,
        }
    }

    pub fn close(&mut self) {
        self.open = None;
        self.shown = None;
        self.patch = None;
    }

    /// The graph on the canvas, when the panel is open.
    #[cfg(test)]
    pub fn graph(&self) -> Option<&Graph> {
        match self.open {
            Some(PanelTarget::Patch(_) | PanelTarget::Effects(_)) => self.patch.as_ref().map(|s| &s.graph),
            _ => self.shown.as_ref().map(|s| &s.graph),
        }
    }

    /// The patch or effects view: the graph edited with the synth's blocks, written back to the
    /// library on every change.
    fn show_patch(&mut self, ui: &mut Ui, scene: &mut Scene, kit: &Kit, target: PanelTarget) -> bool {
        let found = match target {
            PanelTarget::Patch(id) => scene.library.instruments.get(&id).and_then(|i| match &i.source {
                InstrumentSource::Patch { graph } => Some(graph),
                InstrumentSource::BuiltIn { .. } => None,
            }),
            PanelTarget::Effects(place) => place.graph(scene),
            PanelTarget::Slot(..) => None,
        };
        let Some(graph) = found else {
            self.close();
            return false;
        };
        if self.patch.as_ref().is_none_or(|p| p.target != target || &p.stored != graph) {
            self.editor.selection.clear();
            self.patch = Some(ShownPatch::load(target, graph));
        }
        let (title, used) = match target {
            PanelTarget::Patch(id) => {
                let users = scene.instrument_users(&kit.registry, id).len();
                let used = match users {
                    0 => "used by nothing".to_owned(),
                    1 => "used by 1".to_owned(),
                    n => format!("used by {n}"),
                };
                (format!("Instrument › {}", scene.library.instruments[&id].name), Some(used))
            }
            PanelTarget::Effects(place) => (format!("{} effects", place.label(scene)), None),
            PanelTarget::Slot(..) => unreachable!("slots are shown by show"),
        };
        let catalog: &dyn bpe_graph::Catalog = match target {
            PanelTarget::Effects(_) => effects_catalog(),
            _ => bpe_synth::catalog(),
        };
        let Self { editor, patch, .. } = self;
        let shown = patch.as_mut().expect("loaded above");
        let mut close = false;
        let mut changed = false;
        egui::Panel::bottom("nodes")
            .resizable(true)
            .default_size(300.0)
            .min_size(160.0)
            .max_size(800.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title.as_str()).strong());
                    if let Some(used) = &used {
                        ui.label(RichText::new(used.as_str()).color(theme::MUTED))
                            .on_hover_text("Editing this patch changes every sound that plays it.");
                    }
                    match &shown.status {
                        Ok(()) => _ = ui.label(RichText::new("Compiles").color(theme::MUTED)),
                        Err(message) => {
                            ui.label(RichText::new(message.as_str()).color(theme::ERROR)).on_hover_text(
                                "Until this is fixed, the patch plays nothing (effects pass the sound through).",
                            );
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").on_hover_text("Hide the node panel").clicked() {
                            close = true;
                        }
                        if ui.button("Fit").on_hover_text("Bring the whole graph into view").clicked() {
                            editor.refit();
                        }
                        if ui
                            .button("Add block…")
                            .on_hover_text("Search for a block to add (or double-click the canvas)")
                            .clicked()
                        {
                            editor.open_palette();
                        }
                    });
                });
                let live = PatchErrors(&shown.node_errors);
                let outcome = editor.show(ui, &mut shown.graph, catalog, Some(&live), scene);
                changed = outcome.changed;
            });
        if changed {
            changed = match target {
                PanelTarget::Patch(id) => {
                    let mut i = scene.library.instruments[&id].clone();
                    i.source = InstrumentSource::Patch { graph: shown.graph.clone() };
                    scene.set_instrument(id, i).is_ok()
                }
                PanelTarget::Effects(place) => place.set(scene, shown.graph.clone()),
                PanelTarget::Slot(..) => false,
            };
            shown.stored = shown.graph.clone();
            (shown.status, shown.node_errors) = check_patch(target, &shown.graph);
        }
        if close {
            self.close();
        }
        changed
    }

    /// Draws the panel along the bottom of `ui` when it is open. True when the scene changed.
    /// `world` is the running scene, for the glow on blocks that fire.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        scene: &mut Scene,
        kit: &Kit,
        resolved: &Resolved,
        world: Option<&World>,
    ) -> bool {
        let (thing, slot) = match self.open {
            None => return false,
            Some(target @ (PanelTarget::Patch(_) | PanelTarget::Effects(_))) => {
                return self.show_patch(ui, scene, kit, target);
            }
            Some(PanelTarget::Slot(thing, slot)) => (thing, slot),
        };
        let reg = &kit.registry;
        // The panel closes on its own when there is nothing left to show: the thing went, the
        // slot was switched off, or its module has no graph to edit.
        let Some(module) = shown_module(scene, resolved, thing, slot) else {
            self.close();
            return false;
        };
        let Some(role) = role_of(slot) else {
            self.close();
            return false;
        };
        let Some(kind) = scene.library.modules.get(&module).and_then(|m| reg.module(&m.kind)) else {
            self.close();
            return false;
        };
        {
            let m = &scene.library.modules[&module];
            let stale = self.shown.as_ref().is_none_or(|s| s.module != module || s.stored != m.graph);
            if stale {
                self.editor.selection.clear();
                match Shown::load(module, slot, m, kind) {
                    Some(shown) => self.shown = Some(shown),
                    None => {
                        self.close();
                        return false;
                    }
                }
            } else if let Some(s) = &mut self.shown
                && s.params != m.params
            {
                // A card edited a setting: the graph's blocks follow it.
                s.graph = bind(&s.graph, &full_params(kind, m));
                s.params = m.params.clone();
            }
        }
        let thing_name = scene.things.get(&thing).map_or("?", |t| t.name.as_str()).to_owned();
        let module_label = module_words(scene, &scene.library.modules[&module]);
        let custom = scene.library.modules[&module].graph.as_ref().is_some_and(|g| g.custom);
        let can_revert = custom && slot != SlotId::Rules && kind.template().is_some();
        let Self { editor, shown, .. } = self;
        let shown = shown.as_mut().expect("loaded above");

        let mut close = false;
        let mut revert = false;
        let mut changed = false;
        let mut simple_toggled = None;
        egui::Panel::bottom("nodes")
            .resizable(true)
            .default_size(300.0)
            .min_size(160.0)
            .max_size(800.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{thing_name} › {}", slot.name())).strong());
                    ui.label(RichText::new(module_label).color(theme::MUTED)).on_hover_text(
                        "The module this graph belongs to. Editing it changes every thing that shares it.",
                    );
                    match &shown.status {
                        Ok(()) => _ = ui.label(RichText::new("Compiles").color(theme::MUTED)),
                        Err(message) => {
                            ui.label(RichText::new(message.as_str()).color(theme::ERROR)).on_hover_text(
                                "Until this is fixed, the thing uses the module its parent would give it.",
                            );
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").on_hover_text("Hide the node panel").clicked() {
                            close = true;
                        }
                        if can_revert
                            && ui
                                .button("Revert to template")
                                .on_hover_text("Throw the edits away and go back to the card's own settings")
                                .clicked()
                        {
                            revert = true;
                        }
                        if ui.button("Fit").on_hover_text("Bring the whole graph into view").clicked() {
                            editor.refit();
                        }
                        if ui
                            .button("Add block…")
                            .on_hover_text("Search for a block to add (or double-click the canvas)")
                            .clicked()
                        {
                            editor.open_palette();
                        }
                    });
                });
                let live = Flashes::gather(world, thing, slot, resolved, &shown.graph, &shown.node_errors);
                let outcome = editor.show(ui, &mut shown.graph, bpe_vm::catalog(role), Some(&live), scene);
                if outcome.changed || outcome.simple_toggled.is_some() {
                    changed = true;
                    simple_toggled = outcome.simple_toggled;
                }
            });

        if changed && let Some(m) = scene.library.modules.get_mut(&module) {
            shown.store(slot, m, kind, simple_toggled);
        }
        if revert && let Some(m) = scene.library.modules.get_mut(&module) {
            revert_to_template(m, kind);
            changed = true;
        }
        if close {
            self.close();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector::{self, Inspector};
    use crate::selection::Selection;
    use bpe_core::{ModuleId, Resolved, Scene, SettingValue, SlotId, ThingId};
    use bpe_graph::Catalog;
    use bpe_kit::graphs::{compile_module, module_graph};
    use bpe_kit::{Kit, resolve_scene};
    use bpe_vm::{Role, catalog};
    use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    struct State {
        inspector: Inspector,
        panel: NodePanel,
        scene: Scene,
        kit: Kit,
        resolved: Resolved,
        selection: Selection,
    }

    /// The inspector on the right and the node panel along the bottom, wired up as the studio
    /// wires them: the card's Nodes switch asks, the panel opens, the switch shows it open.
    fn harness(scene: Scene, selected: Option<ThingId>) -> Harness<'static, State> {
        let kit = bpe_kit::kit();
        let resolved = resolve_scene(&scene, &kit.registry);
        let mut selection = Selection::default();
        if let Some(id) = selected {
            selection.set(id);
        }
        let state =
            State { inspector: Inspector::default(), panel: NodePanel::default(), scene, kit, resolved, selection };
        let mut h = Harness::builder()
            .with_size(vec2(1200.0, 1000.0))
            .with_step_dt(1.0 / 60.0)
            // Scene defaults' long card takes a few seconds of frames for its grids to settle.
            .with_max_steps(240)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let mut changed = s.panel.show(ui, &mut s.scene, &s.kit, &s.resolved, None);
                    s.inspector.nodes_open = s.panel.slot_open();
                    egui::Panel::right("inspector").default_size(420.0).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        changed |=
                            inspector::show(ui, &mut s.inspector, &mut s.scene, &s.kit, &s.resolved, &s.selection, 0.0);
                    });
                    if let Some((thing, slot)) = s.inspector.nodes_request.take() {
                        s.panel.open_at(thing, slot);
                    }
                    if let Some(place) = s.inspector.fx_request.take() {
                        s.panel.open_effects(place);
                    }
                    if let Some(id) = s.inspector.patch_request.take() {
                        s.panel.open_patch(id);
                    }
                    if changed {
                        s.resolved = resolve_scene(&s.scene, &s.kit.registry);
                    }
                },
                state,
            );
        h.run();
        h
    }

    fn new_scene() -> Scene {
        bpe_kit::new_scene(&bpe_kit::registry())
    }

    /// The ball's cards are its kind, then Look, Motion, Bounce and Sound; Look's switch is off.
    fn open_ball_bounce(h: &mut Harness<'_, State>) {
        h.get_all_by_label("Nodes").nth(2).expect("the Bounce card's switch").click();
        h.run();
    }

    fn click(h: &mut Harness<'_, State>, pos: Pos2) {
        h.hover_at(pos);
        h.step();
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        h.run();
    }

    fn key(h: &mut Harness<'_, State>, key: Key) {
        let modifiers = Modifiers::NONE;
        h.event(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        h.step();
        h.event(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
        h.run();
    }

    /// Where a block's title bar of the open graph is on screen.
    fn title(h: &Harness<'_, State>, node: u32) -> Pos2 {
        let s = h.state();
        s.panel.editor.node_screen_pos(s.panel.graph().expect("a graph is open"), catalog(Role::Bounce), node)
    }

    /// The root's modules are added look, motion, bounce, sound; the ball inherits the bounce.
    const DEFAULT_BOUNCE: ModuleId = ModuleId(2);

    #[test]
    fn the_nodes_switch_opens_the_panel_with_the_path_in_its_title() {
        let mut h = harness(new_scene(), Some(ThingId(2)));
        assert!(h.query_by_label_contains("›").is_none(), "closed until asked");
        open_ball_bounce(&mut h);
        assert_eq!(h.state().panel.open, Some(PanelTarget::Slot(ThingId(2), SlotId::Bounce)));
        h.get_by_label("Ball › Bounce");
        h.get_by_label("Default bounce");
        h.get_by_label("Compiles");
        // The template's blocks are on the canvas (their titles are painted, not widgets).
        let kinds: Vec<&str> = h.state().panel.graph().unwrap().nodes.iter().map(|n| n.kind.as_str()).collect();
        assert!(
            kinds.contains(&"bounce.in") && kinds.contains(&"bounce.out") && kinds.contains(&"reflect"),
            "{kinds:?}"
        );
        // Opening is not an edit: the module keeps running natively and the scene is unchanged.
        assert!(h.state().scene.library.modules[&DEFAULT_BOUNCE].graph.is_none());
    }

    #[test]
    fn editing_the_graph_marks_the_module_custom_and_revert_goes_back() {
        let mut h = harness(new_scene(), Some(ThingId(2)));
        open_ball_bounce(&mut h);
        let reflect = h.state().panel.graph().unwrap().nodes.iter().find(|n| n.kind == "reflect").unwrap().id;
        let at = title(&h, reflect);
        click(&mut h, at);
        key(&mut h, Key::Delete);
        {
            let s = h.state();
            let g = s.scene.library.modules[&DEFAULT_BOUNCE].graph.as_ref().expect("the edit is stored");
            assert!(g.custom);
            assert!(g.graph.node(reflect).is_none());
        }
        h.get_by_label("Edited as nodes");
        // Both the card and the panel offer it; the card's comes first.
        h.get_all_by_label("Revert to template").next().expect("the card offers to go back").click();
        h.run();
        let s = h.state();
        assert!(s.scene.library.modules[&DEFAULT_BOUNCE].graph.is_none());
        assert_eq!(s.panel.open, Some(PanelTarget::Slot(ThingId(2), SlotId::Bounce)), "the panel stays open");
        assert!(s.panel.graph().unwrap().nodes.iter().any(|n| n.kind == "reflect"), "on the fresh template");
    }

    #[test]
    fn a_broken_graph_shows_a_red_badge() {
        let kit = bpe_kit::kit();
        let mut scene = new_scene();
        // The ball gets its own bounce whose graph has two result blocks.
        let own = scene.add_module(&kit.registry, "bounce.basic", "Ball bounce").unwrap();
        scene.link(&kit.registry, ThingId(2), SlotId::Bounce, own).unwrap();
        let kind = kit.registry.module("bounce.basic").unwrap();
        let mut g = module_graph(kind, &scene.library.modules[&own]).unwrap();
        g.graph.add(catalog(Role::Bounce).spec("bounce.out").unwrap(), [900.0, 500.0]);
        g.custom = true;
        let message = compile_module(SlotId::Bounce, &g.graph).unwrap_err();
        scene.library.modules.get_mut(&own).unwrap().graph = Some(g);
        let mut h = harness(scene, Some(ThingId(2)));
        // The parent's bounce runs instead, and the card and the tree say so.
        assert_eq!(h.state().resolved.things[&ThingId(2)].slots[&SlotId::Bounce].module, DEFAULT_BOUNCE);
        h.get_by_label_contains("The Bounce graph doesn't compile, so the parent's module is used");
        let problem = h.state().resolved.problems.iter().find(|p| p.thing == ThingId(2)).cloned().unwrap();
        assert!(crate::tree::problem_words(&problem).contains(&message));
        // The panel shows the error where the status would say "Compiles".
        open_ball_bounce(&mut h);
        assert!(h.query_by_label("Compiles").is_none());
        h.get_by_label(&message);
    }

    /// A scene whose Scene defaults sound plays the blank patch.
    fn patch_scene() -> (Scene, bpe_core::InstrumentId) {
        let mut scene = new_scene();
        let id = scene.add_instrument(bpe_core::Instrument {
            fx: Default::default(),
            name: "Test patch".into(),
            source: InstrumentSource::Patch { graph: bpe_synth::blank_patch() },
            voices: Default::default(),
        });
        let bpe_core::SlotLink::Linked { module, .. } = scene.things[&ThingId::ROOT].slots[&SlotId::Sound] else {
            panic!("Scene defaults links a sound")
        };
        scene
            .library
            .modules
            .get_mut(&module)
            .unwrap()
            .params
            .set("instrument", bpe_core::InstrumentRef(Some(id)).to_value());
        (scene, id)
    }

    fn open_patch(h: &mut Harness<'_, State>) {
        h.get_by_label("Edit patch").scroll_to_me();
        h.run();
        h.get_by_label("Edit patch").click();
        h.run();
    }

    fn stored_patch(h: &Harness<'_, State>, id: bpe_core::InstrumentId) -> Graph {
        match &h.state().scene.library.instruments[&id].source {
            InstrumentSource::Patch { graph } => graph.clone(),
            InstrumentSource::BuiltIn { .. } => panic!("a patch"),
        }
    }

    fn block(h: &Harness<'_, State>, kind: &str) -> u32 {
        h.state().panel.graph().unwrap().nodes.iter().find(|n| n.kind == kind).unwrap().id
    }

    fn drag(h: &mut Harness<'_, State>, from: Pos2, to: Pos2) {
        h.hover_at(from);
        h.step();
        h.event(Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        for i in 1..=8 {
            h.hover_at(from + (to - from) * (i as f32 / 8.0));
            h.step();
        }
        h.event(Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.run();
    }

    #[test]
    fn panel_opens_effects() {
        let mut scene = new_scene();
        scene.library.master_fx = bpe_synth::chain_graph(&[("reverb".into(), Default::default(), true)]);
        let mut h = harness(scene, Some(ThingId::ROOT));
        h.state_mut().panel.open_effects(FxPlace::Master);
        h.run();
        assert_eq!(h.state().panel.open, Some(PanelTarget::Effects(FxPlace::Master)));
        assert_eq!(h.get_all_by_label("Master effects").count(), 2, "the panel header and the inspector card");
        h.get_by_label("Compiles");
        let kinds: Vec<&str> = h.state().panel.graph().unwrap().nodes.iter().map(|n| n.kind.as_str()).collect();
        assert!(kinds.contains(&"reverb"), "{kinds:?}");
        // An edit on the canvas is written back to the master's effects.
        let reverb = block(&h, "reverb");
        let at =
            h.state().panel.editor.node_screen_pos(h.state().panel.graph().unwrap(), super::effects_catalog(), reverb);
        click(&mut h, at);
        key(&mut h, Key::Delete);
        assert!(h.state().scene.library.master_fx.nodes.iter().all(|n| n.kind != "reverb"));
        assert!(super::effects_catalog().spec("note").is_none(), "no notes in effects");
    }

    #[test]
    fn edit_patch_opens_the_patch_in_the_panel() {
        let (scene, id) = patch_scene();
        let mut h = harness(scene, Some(ThingId::ROOT));
        open_patch(&mut h);
        assert_eq!(h.state().panel.open, Some(PanelTarget::Patch(id)));
        assert_eq!(h.state().inspector.nodes_open, None, "no card's Nodes switch is lit");
        h.get_by_label("Instrument › Test patch");
        h.get_by_label("used by 1");
        h.get_by_label("Compiles");
        assert_eq!(h.state().panel.graph().unwrap().nodes.len(), 5);
    }

    #[test]
    fn wiring_blocks_in_the_panel_changes_the_instrument() {
        let (scene, id) = patch_scene();
        let mut h = harness(scene, Some(ThingId::ROOT));
        open_patch(&mut h);
        let (env, filter) = (block(&h, "adsr"), block(&h, "filter"));
        let pin = |h: &Harness<'_, State>, node: u32, pin: u8, output: bool| {
            let s = h.state();
            let graph = s.panel.graph().unwrap();
            s.panel.editor.pin_screen_pos(
                graph,
                bpe_synth::catalog(),
                super::super::editor::PinRef { node, pin, output },
            )
        };
        let (from, to) = (pin(&h, env, 0, true), pin(&h, filter, 1, false));
        drag(&mut h, from, to);
        let g = stored_patch(&h, id);
        assert!(g.links.iter().any(|l| l.from == (env, 0) && l.to == (filter, 1)), "{:?}", g.links);
        // An undo from outside puts the old patch back, and the panel follows it.
        let mut i = h.state().scene.library.instruments[&id].clone();
        i.source = InstrumentSource::Patch { graph: bpe_synth::blank_patch() };
        h.state_mut().scene.set_instrument(id, i).unwrap();
        h.run();
        assert_eq!(h.state().panel.graph().unwrap().links.len(), 4);
    }

    #[test]
    fn removing_voice_out_shows_the_error() {
        let (scene, id) = patch_scene();
        let mut h = harness(scene, Some(ThingId::ROOT));
        open_patch(&mut h);
        let out = block(&h, "voice_out");
        let at = h.state().panel.editor.node_screen_pos(h.state().panel.graph().unwrap(), bpe_synth::catalog(), out);
        click(&mut h, at);
        key(&mut h, Key::Delete);
        assert!(stored_patch(&h, id).node(out).is_none());
        assert!(h.query_by_label("Compiles").is_none());
        h.get_by_label("Add a Voice out block");
    }

    #[test]
    fn look_has_no_nodes_yet() {
        let mut h = harness(new_scene(), Some(ThingId(2)));
        let look = h.get_all_by_label("Nodes").next().expect("the Look card's switch");
        assert!(look.accesskit_node().is_disabled());
        look.click();
        h.run();
        assert_eq!(h.state().panel.open, None);
        assert!(h.state().inspector.nodes_request.is_none());
    }

    /// Rules linked on the scene run on the scene; a ball that inherits them still sees its
    /// blocks glow.
    #[test]
    fn flashes_follow_the_rules_owner() {
        let kit = bpe_kit::kit();
        let mut scene = new_scene();
        let m = scene.add_module(&kit.registry, "rules.graph", "Grow").unwrap();
        let mut graph = kit.registry.module("rules.graph").unwrap().template().unwrap();
        let grow = bpe_kit::recipes().into_iter().find(|r| r.name == "Grow on hit").unwrap();
        let simple = (grow.add)(&mut graph).into_iter().collect();
        scene.library.modules.get_mut(&m).unwrap().graph =
            Some(bpe_core::ModuleGraph { graph: graph.clone(), template: "rules.graph".into(), custom: true, simple });
        scene.link(&kit.registry, ThingId::ROOT, SlotId::Rules, m).unwrap();
        let resolved = resolve_scene(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        assert_eq!(resolved.things[&ThingId(2)].slots[&SlotId::Rules].from, ThingId::ROOT);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        world.advance_to(5.0, &kit.sims, &mut bpe_runtime::Log::default());
        let live = Flashes::gather(Some(&world), ThingId(2), SlotId::Rules, &resolved, &graph, &BTreeMap::new());
        let when = graph.nodes.iter().find(|n| n.kind == "when.hit").unwrap().id;
        assert!(live.fired.contains_key(&when), "the ball's hits fired the scene's rule");
    }

    /// A stored, unedited graph is only a layout: the kind's template is what runs and what
    /// is shown, so a template that changes in a later version reaches every module.
    #[test]
    fn a_viewed_template_is_rebuilt_from_the_kind_and_keeps_its_layout() {
        let kit = bpe_kit::kit();
        let scene = new_scene();
        let m = &scene.library.modules[&DEFAULT_BOUNCE];
        let kind = kit.registry.module(&m.kind).unwrap();
        let fresh = module_graph(kind, m).unwrap();
        let first = fresh.graph.nodes[0].id;
        let last = fresh.graph.nodes.last().unwrap().id;
        let mut stale = fresh.clone();
        stale.graph.node_mut(first).unwrap().pos = [-999.0, 42.0];
        stale.graph.remove(&std::collections::BTreeSet::from([last]));
        let mut module = m.clone();
        module.graph = Some(stale);
        let shown = Shown::load(DEFAULT_BOUNCE, SlotId::Bounce, &module, kind).unwrap();
        assert_eq!(shown.graph.nodes.len(), fresh.graph.nodes.len(), "the kind's template wins over a stale copy");
        assert!(shown.graph.node(last).is_some());
        assert_eq!(shown.graph.node(first).unwrap().pos, [-999.0, 42.0], "the layout is kept");
        assert!(shown.status.is_ok());
    }
}
