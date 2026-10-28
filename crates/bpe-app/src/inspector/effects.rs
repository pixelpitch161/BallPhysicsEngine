//! The Effects tab: a stack of shader effects applied to the finished picture, top to bottom.

use super::{Roll, dice_button, grid, heading, hint, key, label, percent, resettable, slider};
use crate::theme;
use bpe_render::EffectLibrary;
use bpe_render::effects::{self as fx, EffectInfo, ParamKind};
use bpe_scene::{Effect, EffectTarget, Property, Scene};
use eframe::egui::{self, RichText, Ui};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often effect files are checked for changes.
const REFRESH: Duration = Duration::from_millis(500);

/// Effects tab state that is not part of the scene.
pub struct EffectsPanel {
    pub library: EffectLibrary,
    /// Errors the GPU reported for effects that passed the shader check, as (id, message).
    pub gpu_errors: Vec<(String, String)>,
    last_refresh: Instant,
    /// The folder of the open scene, whose `effects` folder is searched first.
    scene_dir: Option<Option<PathBuf>>,
    note: Option<(String, bool)>,
    /// A node effect to open in the Shaders workspace, for the app to pick up.
    pub open_graph: Option<String>,
}

impl Default for EffectsPanel {
    fn default() -> Self {
        Self {
            library: EffectLibrary::new(),
            gpu_errors: Vec::new(),
            last_refresh: Instant::now(),
            scene_dir: None,
            note: None,
            open_graph: None,
        }
    }
}

impl EffectsPanel {
    /// Reloads effect files that changed on disk, and keeps checking while it matters.
    /// `scene_dir` is the folder of the open scene file, if it has been saved.
    pub fn poll(&mut self, ctx: &egui::Context, scene: &Scene, scene_dir: Option<PathBuf>, tab_open: bool) {
        if self.scene_dir.as_ref() != Some(&scene_dir) {
            self.library.set_folders(fx::default_folders(scene_dir.as_deref()));
            self.scene_dir = Some(scene_dir);
            self.last_refresh = Instant::now();
            ctx.request_repaint();
        } else if self.last_refresh.elapsed() >= REFRESH {
            self.last_refresh = Instant::now();
            if self.library.refresh() {
                ctx.request_repaint();
            }
        }
        // Node effects live in the scene; the Effects tab reads them before the preview does.
        if self.library.sync_graphs(&scene.shader_graphs) {
            ctx.request_repaint();
        }
        let uses_files = scene.effects.iter().any(|e| self.library.get(&e.shader).is_none_or(|i| !i.is_builtin()));
        if tab_open || uses_files {
            ctx.request_repaint_after(REFRESH);
        }
    }

    /// The folder new effect files go in: `effects` next to the program.
    fn user_folder(&self) -> Option<PathBuf> {
        self.library.folders().last().cloned()
    }
}

/// Ready-made stacks of effects.
fn looks() -> Vec<(&'static str, &'static str, Vec<Effect>)> {
    vec![
        (
            "Neon mandala",
            "Kaleidoscope slices with spinning light trails",
            vec![
                Effect::new("kaleidoscope").with("slices", 10.0).with("spin", 0.02),
                Effect::new("echo").with("fade", 0.94).with("spin", -0.6),
                Effect::new("color_split").with("amount", 0.003),
            ],
        ),
        (
            "Bounce punch",
            "Every bounce sends a glowing ripple and a colour kick",
            vec![
                Effect::new("shockwave").with("strength", 1.0).with("glow", 0.8),
                Effect::new("color_split").with("amount", 0.002).with("kick", 4.0),
            ],
        ),
        (
            "Dream",
            "Soft flowing warp, colour trails and a slow colour drift",
            vec![
                Effect::new("liquid").with("amount", 0.03).with("speed", 0.2),
                Effect::new("echo").with("fade", 0.9).with("hue", 0.006),
                Effect::new("color_cycle").with("speed", 0.03),
            ],
        ),
        (
            "Light tunnel",
            "Ghosts fly towards you and turn, like a tunnel of light",
            vec![Effect::new("echo").with("fade", 0.95).with("zoom", 1.02).with("spin", 0.5).with("hue", 0.005)],
        ),
        (
            "Retro TV",
            "Chunky pixels, scanlines and a glitch on each bounce",
            vec![
                Effect::new("pixels").with("size", 6.0).with("gap", 0.25),
                Effect::new("film").with("scanlines", 0.6).with("vignette", 0.6).with("grain", 0.08),
                Effect::new("glitch").with("amount", 0.3),
            ],
        ),
    ]
}

pub use bpe_export::batch::roll_effect;

pub fn show(ui: &mut Ui, panel: &mut EffectsPanel, scene: &mut Scene, roll: &mut Option<Roll>) {
    let effects = &mut scene.effects;
    ui.label(
        RichText::new(
            "Effects change the whole picture, one after another from top to bottom, or with Applies to, only some balls, the walls or the obstacles, like a material. They are saved in the scene and show in the exported video.",
        )
        .color(theme::MUTED),
    );
    ui.add_space(6.0);
    let replaced = add_buttons(ui, panel, effects);
    ui.add_space(10.0);

    if effects.is_empty() {
        ui.label(RichText::new("No effects yet. Add one above, or start from a look.").color(theme::MUTED));
    }
    let mut action = None;
    let count = effects.len();
    let groups = group_names(&scene.spawners);
    for (index, effect) in effects.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            if let Some(a) = effect_card(ui, panel, effect, index, count, &groups) {
                action = Some((index, a));
            }
        });
        ui.add_space(8.0);
    }
    // Keyframes on an effect move with it.
    let swap = |a: usize, b: usize| {
        move |k: u32| {
            Some(if k == a as u32 {
                b as u32
            } else if k == b as u32 {
                a as u32
            } else {
                k
            })
        }
    };
    match action {
        Some((i, CardAction::Up)) => {
            effects.swap(i, i - 1);
            scene.remap_effect_tracks(swap(i, i - 1));
        }
        Some((i, CardAction::Down)) => {
            effects.swap(i, i + 1);
            scene.remap_effect_tracks(swap(i, i + 1));
        }
        Some((i, CardAction::Remove)) => {
            effects.remove(i);
            let i = i as u32;
            scene.remap_effect_tracks(|k| match k {
                k if k == i => None,
                k if k > i => Some(k - 1),
                k => Some(k),
            });
        }
        Some((i, CardAction::Duplicate)) => {
            let copy = effects[i].clone();
            effects.insert(i + 1, copy);
            let i = i as u32;
            scene.remap_effect_tracks(|k| Some(if k > i { k + 1 } else { k }));
            scene.copy_effect_tracks(i, i + 1);
        }
        Some((i, CardAction::EditCopy)) => edit_copy(panel, &mut effects[i]),
        Some((i, CardAction::EditNodes)) => {
            let shader = effects[i].shader.clone();
            if let Some(name) = edit_as_nodes(panel, scene, &shader) {
                for effect in scene.effects.iter_mut().filter(|e| e.shader == shader) {
                    effect.shader = fx::graph::effect_id(&name);
                }
                panel.open_graph = Some(name);
            }
        }
        Some((i, CardAction::SaveCode)) => {
            let shader = effects[i].shader.clone();
            save_as_code(panel, &shader);
        }
        Some((i, CardAction::Roll)) => *roll = Some(Roll::Effect(i)),
        None => {}
    }
    if replaced {
        scene.remap_effect_tracks(|_| None);
    }

    ui.add_space(6.0);
    heading(ui, "Your own effects");
    ui.label(
        RichText::new(
            "An effect is a small WGSL shader file. Files in the effects folder show up in Add effect, and the preview updates every time you save one.",
        )
        .color(theme::MUTED),
    );
    ui.horizontal_wrapped(|ui| {
        ui.menu_button("New node effect", |ui| {
            ui.set_min_width(260.0);
            ui.label(RichText::new("Build an effect by connecting blocks, no code needed").small().color(theme::MUTED));
            if ui.button("Start empty").on_hover_text("Just the picture, ready for your blocks").clicked() {
                new_node_effect(panel, scene, None);
                ui.close();
            }
            for template in fx::templates::graphs() {
                if ui.button(format!("Start from {}", template.name)).clicked() {
                    new_node_effect(panel, scene, Some(template));
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Make an effect from blocks in the Shaders workspace");
        if ui.button("New effect file").on_hover_text("Make a starter effect file, add it here and open it").clicked() {
            new_effect(panel, &mut scene.effects);
        }
        if ui.button("Open effects folder").clicked() {
            match panel.user_folder() {
                Some(folder) => {
                    let _ = std::fs::create_dir_all(&folder);
                    open_path(&folder);
                }
                None => panel.note = Some(("Couldn't find the program's folder.".into(), true)),
            }
        }
    });
    if let Some(folder) = panel.user_folder() {
        ui.label(RichText::new(folder.display().to_string()).small().color(theme::MUTED));
    }
    if let Some((note, is_error)) = &panel.note {
        ui.label(RichText::new(note.as_str()).color(if *is_error { theme::ERROR } else { theme::MUTED }));
    }
    let broken: Vec<_> = panel.library.list().into_iter().filter(|i| !i.is_builtin() && i.error().is_some()).collect();
    for info in broken {
        if !scene.effects.iter().any(|e| e.shader == info.id) {
            ui.label(RichText::new(format!("{} has an error:", file_name(&info))).color(theme::WARNING));
            error_text(ui, info.error().unwrap_or_default());
        }
    }
}

/// The add buttons. Returns whether the effects were replaced by a look.
fn add_buttons(ui: &mut Ui, panel: &mut EffectsPanel, effects: &mut Vec<Effect>) -> bool {
    let mut replaced = false;
    ui.horizontal(|ui| {
        ui.menu_button(RichText::new("+ Add effect").strong(), |ui| {
            ui.set_min_width(300.0);
            let list = panel.library.list();
            for info in &list {
                let mut text = RichText::new(&info.title);
                if !info.is_builtin() {
                    text = text.color(theme::ACCENT_TEXT);
                }
                let mut hover = info.about.clone();
                if let Some(error) = info.error() {
                    hover = format!("This file has an error and won't show until it is fixed:\n{error}");
                    text = text.color(theme::WARNING);
                }
                if ui.button(text).on_hover_text(hover).clicked() {
                    effects.push(Effect::new(info.id.clone()));
                    ui.close();
                }
            }
        });
        ui.menu_button("Start from a look", |ui| {
            ui.set_min_width(320.0);
            ui.label(RichText::new("Replaces the effects below. Ctrl+Z puts them back.").small().color(theme::MUTED));
            for (name, about, stack) in looks() {
                if ui.button(name).on_hover_text(about).clicked() {
                    *effects = stack;
                    replaced = true;
                    ui.close();
                }
            }
        });
    });
    replaced
}

enum CardAction {
    Up,
    Down,
    Remove,
    Duplicate,
    EditCopy,
    /// Open the node effect in the Shaders workspace, making a node version first if needed.
    EditNodes,
    /// Write a node effect out as a code effect file.
    SaveCode,
    Roll,
}

/// A name for each group of balls, as the Balls tab lists them.
pub(crate) fn group_names(spawners: &[bpe_scene::Spawner]) -> Vec<String> {
    spawners
        .iter()
        .enumerate()
        .map(|(i, spawner)| {
            let kind = match spawner {
                bpe_scene::Spawner::Grid { .. } => "block of balls",
                bpe_scene::Spawner::Single { .. } => "single ball",
                bpe_scene::Spawner::Stream { .. } => "stream of balls",
            };
            format!("Balls {}: {kind}", i + 1)
        })
        .collect()
}

/// The name of what an effect changes.
fn target_name(target: EffectTarget, groups: &[String]) -> String {
    match target {
        EffectTarget::Picture => "The whole picture".into(),
        EffectTarget::Balls => "Every ball".into(),
        EffectTarget::Group(n) => groups.get(n as usize).cloned().unwrap_or_else(|| format!("Balls {} (gone)", n + 1)),
        EffectTarget::Walls => "The arena's walls".into(),
        EffectTarget::Obstacles => "Pegs and obstacles".into(),
    }
}

/// Picks what an effect changes: the whole picture, or only some things in it.
fn target_row(ui: &mut Ui, effect: &mut Effect, groups: &[String]) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Applies to").color(theme::MUTED)).on_hover_text(
            "The whole picture, or only some things in it, like a material. Effects on things are drawn first, under flashes, particles and captions.",
        );
        let mut targets = vec![EffectTarget::Picture, EffectTarget::Balls];
        targets.extend((0..groups.len() as u32).map(EffectTarget::Group));
        targets.extend([EffectTarget::Walls, EffectTarget::Obstacles]);
        let combo = egui::ComboBox::from_id_salt("applies-to")
            .selected_text(target_name(effect.target, groups))
            .width(220.0)
            .show_ui(ui, |ui| {
                for target in targets {
                    ui.selectable_value(&mut effect.target, target, target_name(target, groups));
                }
            });
        let enabled = combo.response.enabled();
        combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, "Applies to"));
    });
}

fn effect_card(
    ui: &mut Ui,
    panel: &mut EffectsPanel,
    effect: &mut Effect,
    index: usize,
    count: usize,
    groups: &[String],
) -> Option<CardAction> {
    let info = panel.library.get(&effect.shader);
    let mut action = None;
    let frame = egui::Frame::new()
        .fill(theme::CARD)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(8.0)
        .inner_margin(10.0);
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(&mut effect.enabled, "").on_hover_text("Turn this effect on or off");
            let title = info.as_ref().map_or_else(|| effect.shader.clone(), |i| i.title.clone());
            let color = if effect.enabled { theme::TEXT } else { theme::MUTED };
            ui.label(RichText::new(title).strong().color(color));
            if let Some(info) = &info
                && info.is_graph()
            {
                ui.label(RichText::new("nodes").small().color(theme::ACCENT_TEXT))
                    .on_hover_text("Made from blocks. Edit as nodes to change it.");
            } else if let Some(info) = &info
                && let Some(path) = &info.path
            {
                ui.label(RichText::new("file").small().color(theme::ACCENT_TEXT))
                    .on_hover_text(format!("Loaded from {}", path.display()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let more = egui::containers::menu::MenuButton::from_button(icon_button());
                let (more_response, _) = more.ui(ui, |ui| {
                    if ui.button("Duplicate").clicked() {
                        action = Some(CardAction::Duplicate);
                        ui.close();
                    }
                    if let Some(info) = &info {
                        if ui.add_enabled(!effect.params.is_empty(), egui::Button::new("Reset all sliders")).clicked() {
                            effect.params.clear();
                            effect.amount = 1.0;
                            ui.close();
                        }
                        if info.is_graph() || (info.is_builtin() && fx::templates::rebuild(&info.id).is_some()) {
                            let help = if info.is_graph() {
                                "Open its blocks in the Shaders workspace"
                            } else {
                                "Swap this for a node version you can change block by block. It looks the same."
                            };
                            if ui.button("Edit as nodes").on_hover_text(help).clicked() {
                                action = Some(CardAction::EditNodes);
                                ui.close();
                            }
                        }
                        if info.is_graph() {
                            let ready = info.graph.as_ref().is_some_and(|g| g.generated.is_some());
                            let button = ui.add_enabled(ready, egui::Button::new("Save as code")).on_hover_text(
                                "Write this effect as a WGSL file in the effects folder, to edit as code",
                            );
                            if button.clicked() {
                                action = Some(CardAction::SaveCode);
                                ui.close();
                            }
                        } else {
                            let edit = if info.is_builtin() { "Edit a copy as code" } else { "Open the effect file" };
                            if ui.button(edit).clicked() {
                                if info.is_builtin() {
                                    action = Some(CardAction::EditCopy);
                                } else if let Some(path) = &info.path {
                                    open_path(path);
                                }
                                ui.close();
                            }
                        }
                    }
                    if ui.button("Remove").clicked() {
                        action = Some(CardAction::Remove);
                        ui.close();
                    }
                });
                paint_icon(ui, &more_response, Icon::More);
                more_response.on_hover_text("More options");
                if icon(ui, Icon::Down, index + 1 < count).on_hover_text("Move down").clicked() {
                    action = Some(CardAction::Down);
                }
                if icon(ui, Icon::Up, index > 0).on_hover_text("Move up").clicked() {
                    action = Some(CardAction::Up);
                }
                if info.as_ref().is_some_and(|i| !i.params.is_empty()) {
                    if dice_button(ui, "Roll random settings for this effect").clicked() {
                        action = Some(CardAction::Roll);
                    }
                    crate::dice_panel::lock_button(ui, crate::dice_panel::Part::Effect(index));
                }
            });
        });

        let Some(info) = info else {
            ui.label(
                RichText::new(format!(
                    "There is no effect called \"{}\". Put {}.wgsl in the effects folder, or remove this one.",
                    effect.shader, effect.shader
                ))
                .color(theme::WARNING),
            );
            return;
        };
        if !info.about.is_empty() {
            ui.label(RichText::new(&info.about).small().color(theme::MUTED));
        }
        if let Some(graph) = &info.graph {
            graph_problems(ui, graph, info.error().is_some());
        } else if let Some(error) = info.error() {
            ui.label(
                RichText::new("This effect has an error, so it is skipped until the file is fixed:")
                    .color(theme::ERROR),
            );
            error_text(ui, error);
        }
        if let Some((_, error)) = panel.gpu_errors.iter().find(|(id, _)| *id == info.id) {
            ui.label(RichText::new("The graphics card couldn't run this effect:").color(theme::ERROR));
            error_text(ui, error);
        }
        ui.add_space(4.0);
        ui.add_enabled_ui(effect.enabled, |ui| {
            target_row(ui, effect, groups);
            grid(ui, "params", |ui| {
                key(ui, Property::EffectAmount(index as u32));
                slider(
                    ui,
                    "Amount",
                    "How much of the effect shows. 0% is off, 100% is the full effect.",
                    &mut effect.amount,
                    1.0,
                    0.0..=1.0,
                    percent,
                );
                for param in &info.params {
                    param_row(ui, effect, index, param);
                }
            });
        });
    });
    action
}

fn param_row(ui: &mut Ui, effect: &mut Effect, index: usize, param: &fx::ParamInfo) {
    let stored = effect.params.get(&param.name).copied().filter(|v| v.is_finite());
    let mut value = stored.unwrap_or(param.default);
    let text = pretty_name(&param.name);
    key(ui, Property::EffectParam(index as u32, param.name.clone()));
    match param.kind {
        ParamKind::Toggle => {
            let name = label(ui, &text, &param.help);
            let mut on = value > 0.5;
            let response = hint(ui.checkbox(&mut on, ""), &param.help);
            if response.changed() {
                value = if on { 1.0 } else { 0.0 };
            }
            resettable(response.union(name), &mut value, param.default);
            ui.end_row();
        }
        ParamKind::Whole => {
            let range = param.min..=param.max;
            slider(ui, &text, &param.help, &mut value, param.default, range, |s| {
                s.step_by(1.0).custom_formatter(|v, _| format!("{v:.0}"))
            });
        }
        ParamKind::Slider => {
            let range = param.min..=param.max;
            let span = (param.max - param.min).abs();
            let decimals = if span <= 0.2 {
                3
            } else if span <= 5.0 {
                2
            } else {
                1
            };
            slider(ui, &text, &param.help, &mut value, param.default, range, move |s| {
                s.custom_formatter(move |v, _| format!("{v:.decimals$}"))
            });
        }
    }
    if Some(value) != stored && !(stored.is_none() && value == param.default) {
        effect.params.insert(param.name.clone(), value);
    }
}

#[derive(Clone, Copy)]
enum Icon {
    Up,
    Down,
    More,
}

/// An empty button sized for a drawn icon, so it looks the same whatever fonts are installed.
fn icon_button() -> egui::Button<'static> {
    egui::Button::new("").min_size(egui::vec2(26.0, 22.0))
}

fn icon(ui: &mut Ui, icon: Icon, enabled: bool) -> egui::Response {
    let response = ui.add_enabled(enabled, icon_button());
    paint_icon(ui, &response, icon);
    response
}

fn paint_icon(ui: &Ui, response: &egui::Response, icon: Icon) {
    let c = response.rect.center();
    let color = if response.enabled() { theme::TEXT } else { theme::MUTED.gamma_multiply(0.6) };
    let painter = ui.painter();
    let v = egui::vec2;
    match icon {
        Icon::Up | Icon::Down => {
            let s = if matches!(icon, Icon::Up) { -1.0 } else { 1.0 };
            let points = vec![c + v(-5.0, -2.5 * s), c + v(0.0, 2.5 * s), c + v(5.0, -2.5 * s)];
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.8, color)));
        }
        Icon::More => {
            for dx in [-5.0, 0.0, 5.0] {
                painter.circle_filled(c + v(dx, 0.0), 1.7, color);
            }
        }
    }
}

/// `left_right` to "Left right".
pub(crate) fn pretty_name(name: &str) -> String {
    let text = name.replace('_', " ");
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A node effect's problems: errors that stop it, or warnings about blocks that do nothing.
fn graph_problems(ui: &mut Ui, graph: &fx::GraphInfo, broken: bool) {
    if graph.problems.is_empty() {
        return;
    }
    let (text, color) = if broken {
        ("This node effect has a problem, so it is skipped until it is fixed:", theme::ERROR)
    } else {
        ("Some blocks aren't doing anything:", theme::WARNING)
    };
    ui.label(RichText::new(text).color(color));
    for problem in graph.problems.iter().take(4) {
        let block = problem
            .node
            .and_then(|id| graph.graph.nodes.iter().find(|n| n.id == id))
            .and_then(|n| fx::graph::Catalog::get().block(&n.kind))
            .map(|b| format!("{}: ", b.title))
            .unwrap_or_default();
        ui.label(RichText::new(format!("• {block}{}", problem.message)).small().color(color));
    }
    if graph.problems.len() > 4 {
        ui.label(RichText::new(format!("and {} more", graph.problems.len() - 4)).small().color(color));
    }
}

/// Adds a node effect to the scene, from `template` or empty, and opens it in the workspace.
fn new_node_effect(panel: &mut EffectsPanel, scene: &mut Scene, template: Option<fx::graph::Graph>) {
    let graph = template.unwrap_or_else(crate::shader_workspace::blank_graph);
    let stem =
        if graph.name.is_empty() { "My effect".to_string() } else { format!("My {}", graph.name.to_lowercase()) };
    let name = crate::shader_workspace::add_graph(scene, &panel.library, graph, &stem);
    panel.open_graph = Some(name);
}

/// The graph to open for `shader`: the scene's own node effect, or a copy of a node effect that
/// comes with the program, or the node version of a built-in code effect. Returns its name,
/// once it is in the scene.
fn edit_as_nodes(panel: &mut EffectsPanel, scene: &mut Scene, shader: &str) -> Option<String> {
    if let Some(name) = fx::graph::graph_name(shader)
        && scene.shader_graphs.iter().any(|g| g.name == name)
    {
        return Some(name.to_string());
    }
    let info = panel.library.get(shader)?;
    let mut graph = match &info.graph {
        Some(graph) => graph.graph.clone(),
        None => fx::templates::rebuild(shader)?,
    };
    let stem = if info.is_graph() { format!("{} (my copy)", graph.name) } else { graph.name.clone() };
    graph.name = crate::shader_workspace::free_name(scene, &panel.library, &stem);
    let name = graph.name.clone();
    scene.shader_graphs.push(graph);
    panel.library.sync_graphs(&scene.shader_graphs);
    Some(name)
}

/// Writes a node effect's generated code to a new file in the effects folder.
fn save_as_code(panel: &mut EffectsPanel, shader: &str) {
    let Some(info) = panel.library.get(shader) else { return };
    let Some(generated) = info.graph.as_ref().and_then(|g| g.generated.as_ref()) else { return };
    let stem: String = info
        .title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let stem = if stem.is_empty() { "node_effect".to_string() } else { stem };
    let source = generated.source.clone();
    write_effect_file(panel, &stem, &source);
}

fn error_text(ui: &mut Ui, error: &str) {
    ui.add(egui::Label::new(RichText::new(error).monospace().small().color(theme::ERROR)).wrap());
}

fn file_name(info: &EffectInfo) -> String {
    info.path.as_ref().and_then(|p| p.file_name()).map_or_else(|| info.id.clone(), |n| n.to_string_lossy().into())
}

/// A file name in `folder` that doesn't exist yet: `stem.wgsl`, `stem_2.wgsl`, ...
fn free_path(folder: &Path, stem: &str) -> PathBuf {
    (1..)
        .map(|n| if n == 1 { folder.join(format!("{stem}.wgsl")) } else { folder.join(format!("{stem}_{n}.wgsl")) })
        .find(|p| !p.exists())
        .unwrap()
}

fn write_effect_file(panel: &mut EffectsPanel, stem: &str, source: &str) -> Option<String> {
    let Some(folder) = panel.user_folder() else {
        panel.note = Some(("Couldn't find the program's folder.".into(), true));
        return None;
    };
    let path = free_path(&folder, stem);
    let result = std::fs::create_dir_all(&folder).and_then(|_| std::fs::write(&path, source));
    if let Err(error) = result {
        panel.note = Some((format!("Couldn't write {}: {error}", path.display()), true));
        return None;
    }
    panel.library.refresh();
    let id = path.file_stem().and_then(|s| s.to_str()).map(fx::effect_id)?;
    // A file in a folder earlier in the search order would hide this one.
    if panel.library.get(&id).and_then(|i| i.path.clone()).as_deref() != Some(path.as_path()) {
        panel.note = Some((format!("Made {}, but another effect called {id} is used instead.", path.display()), true));
        return None;
    }
    panel.note = Some((format!("Made {}. Edit and save it to see the changes.", path.display()), false));
    open_path(&path);
    Some(id)
}

fn new_effect(panel: &mut EffectsPanel, effects: &mut Vec<Effect>) {
    if let Some(id) = write_effect_file(panel, "my_effect", fx::TEMPLATE) {
        effects.push(Effect::new(id));
    }
}

/// Copies a built-in effect to a new file and switches this effect over to it, keeping its sliders.
fn edit_copy(panel: &mut EffectsPanel, effect: &mut Effect) {
    let Some(info) = panel.library.builtin(&effect.shader) else { return };
    let source = info.user_source.replacen(
        &format!("// name: {}", info.title),
        &format!("// name: {} (my copy)", info.title),
        1,
    );
    if let Some(id) = write_effect_file(panel, &format!("{}_copy", info.id), &source) {
        effect.shader = id;
    }
}

/// Opens a file or folder with whatever the system uses for it.
fn open_path(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_use_real_effects_and_params() {
        let library = EffectLibrary::new();
        for (name, _, stack) in looks() {
            for effect in stack {
                let info = library.get(&effect.shader).unwrap_or_else(|| panic!("{name}: no {}", effect.shader));
                for (param, value) in &effect.params {
                    let p = info.params.iter().find(|p| p.name == *param);
                    let p = p.unwrap_or_else(|| panic!("{name}: {} has no {param}", effect.shader));
                    assert!((p.min..=p.max).contains(value), "{name}: {param} = {value} out of range");
                }
            }
        }
    }

    #[test]
    fn names_read_nicely() {
        assert_eq!(pretty_name("left_right"), "Left right");
        assert_eq!(pretty_name("zoom"), "Zoom");
    }

    #[test]
    fn free_paths_skip_existing_files() {
        let dir = std::env::temp_dir().join(format!("bpe-free-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a"), dir.join("a.wgsl"));
        std::fs::write(dir.join("a.wgsl"), "").unwrap();
        assert_eq!(free_path(&dir, "a"), dir.join("a_2.wgsl"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
