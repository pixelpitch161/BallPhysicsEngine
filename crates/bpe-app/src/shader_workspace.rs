//! The Shaders workspace: node effects on a canvas. Each block shows a small live picture of
//! what it makes, problems sit on the block that has them, and the values marked "show on the
//! effect card" become the effect's sliders, also listed down the side here.

use crate::node_editor::{LiveView, NodeEditor};
use crate::theme;
use bpe_render::effects::{self as fx, EffectInfo, ThumbnailRequest};
use bpe_render::{EffectLibrary, FrameData, Renderer, wgpu};
use bpe_scene::{Effect, Scene};
use bpe_sim::Playback;
use eframe::egui::{self, RichText, Ui};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Block previews are drawn at most this often.
const PREVIEW_EVERY: Duration = Duration::from_millis(66);
/// Size of a block preview on the canvas, in canvas units (9:16, 64 tall).
const PREVIEW_SIZE: [f32; 2] = [36.0, 64.0];
/// Width of the list of sliders and problems beside the canvas.
const SIDE: f32 = 240.0;

pub struct State {
    pub editor: NodeEditor,
    /// The open node effect, by name.
    pub graph: Option<String>,
    previews: Option<Previews>,
    /// Blocks that were on screen last frame, for the previews.
    visible: Vec<u32>,
    /// The name being typed, until it is confirmed.
    rename: Option<String>,
    note: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            editor: NodeEditor::new("shaders"),
            graph: None,
            previews: None,
            visible: Vec::new(),
            rename: None,
            note: None,
        }
    }
}

impl State {
    /// Opens the node effect called `name`.
    pub fn open(&mut self, name: String) {
        if self.graph.as_ref() != Some(&name) {
            self.graph = Some(name);
            self.editor.refit();
            self.rename = None;
            self.note = None;
        }
    }
}

/// The blocks shader graphs can use.
pub fn catalog() -> &'static dyn bpe_graph::Catalog {
    fx::graph::Catalog::get()
}

/// Draws block previews with a small renderer of its own, and keeps one texture per block for
/// the canvas.
struct Previews {
    renderer: Renderer,
    frame_data: FrameData,
    /// The effect built for previews, and the graph it was built from.
    info: Option<(bpe_graph::Graph, Arc<EffectInfo>)>,
    /// One texture per preview slot, shown by the canvas.
    tiles: Vec<(wgpu::Texture, egui::TextureId)>,
    /// Which block each slot showed last time it was drawn.
    shown: HashMap<u32, usize>,
    last: Option<Instant>,
    /// What the last previews were drawn from, so unchanged frames aren't drawn again.
    drawn: Option<(Scene, f64, Vec<u32>)>,
}

struct Live {
    tiles: HashMap<u32, egui::TextureId>,
    problems: HashMap<u32, String>,
}

impl LiveView for Live {
    fn thumbnail(&self, node: u32) -> Option<(egui::TextureId, [f32; 2])> {
        self.tiles.get(&node).map(|&t| (t, PREVIEW_SIZE))
    }

    fn error(&self, node: u32) -> Option<String> {
        self.problems.get(&node).cloned()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    frame: &eframe::Frame,
    scene: &mut Scene,
    state: &mut State,
    library: &EffectLibrary,
    playback: &Playback,
    time: f64,
) {
    // Fall back to the first node effect when the open one is gone (undo, a new scene).
    if !state.graph.as_ref().is_some_and(|name| scene.shader_graphs.iter().any(|g| g.name == *name)) {
        state.graph = scene.shader_graphs.first().map(|g| g.name.clone());
        state.editor.refit();
    }
    top_bar(ui, scene, state, library);
    ui.add_space(4.0);
    let Some(index) = state.graph.as_ref().and_then(|name| scene.shader_graphs.iter().position(|g| g.name == *name))
    else {
        welcome(ui, scene, state, library);
        return;
    };
    library.sync_graphs(&scene.shader_graphs);
    let info = library.get(&fx::graph::effect_id(&scene.shader_graphs[index].name));

    egui::Panel::left("shader side")
        .resizable(false)
        .exact_size(SIDE)
        .frame(egui::Frame::new().inner_margin(egui::Margin { right: 10, ..Default::default() }))
        .show(ui, |ui| side(ui, scene, index, info.as_deref(), state));

    let problems: HashMap<u32, String> = info
        .as_ref()
        .and_then(|i| i.graph.as_ref())
        .map(|g| {
            let mut by_node = HashMap::new();
            for p in &g.problems {
                if let Some(node) = p.node {
                    by_node.entry(node).or_insert_with(|| p.message.clone());
                }
            }
            by_node
        })
        .unwrap_or_default();
    let tiles = match frame.wgpu_render_state() {
        Some(render_state) => previews(ui.ctx(), render_state, scene, index, state, library, playback, time),
        None => HashMap::new(),
    };
    let canvas = ui.available_rect_before_wrap();
    let live = Live { tiles, problems };
    let graph = &mut scene.shader_graphs[index];
    state.editor.show(ui, graph, catalog(), Some(&live));
    state.visible = graph
        .nodes
        .iter()
        .filter(|n| canvas.expand(40.0).contains(state.editor.node_screen_pos(graph, catalog(), n.id)))
        .map(|n| n.id)
        .collect();
}

fn graph_names(scene: &Scene) -> Vec<String> {
    scene.shader_graphs.iter().map(|g| g.name.clone()).collect()
}

fn top_bar(ui: &mut Ui, scene: &mut Scene, state: &mut State, library: &EffectLibrary) {
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("Node effect").color(theme::MUTED));
        let current = state.graph.clone().unwrap_or_else(|| "None yet".into());
        egui::ComboBox::from_id_salt("shader-graph")
            .selected_text(&current)
            .width(180.0)
            .show_ui(ui, |ui| {
                for name in graph_names(scene) {
                    if ui.selectable_label(state.graph.as_ref() == Some(&name), &name).clicked() {
                        state.open(name);
                    }
                }
            })
            .response
            .labelled_by(label.id);
        new_menu(ui, scene, state, library);
        let Some(index) = state.graph.as_ref().and_then(|n| scene.shader_graphs.iter().position(|g| g.name == *n))
        else {
            return;
        };
        let name = scene.shader_graphs[index].name.clone();
        let text = state.rename.get_or_insert_with(|| name.clone());
        let response = ui
            .add(egui::TextEdit::singleline(text).desired_width(140.0).hint_text("Name"))
            .on_hover_text("The effect's name. Press Enter to rename it.");
        if response.lost_focus() {
            let wanted = state.rename.take().unwrap_or_default().trim().to_string();
            if wanted != name {
                match rename(scene, &name, &wanted) {
                    Ok(()) => state.graph = Some(wanted),
                    Err(why) => state.note = Some(why),
                }
            }
        } else if !response.has_focus() {
            state.rename = None;
        }
        let used = scene.effects.iter().any(|e| e.shader == fx::graph::effect_id(&name));
        if !used
            && ui.button("Add to the video").on_hover_text("Put this effect at the end of the Effects tab").clicked()
        {
            scene.effects.push(Effect::new(fx::graph::effect_id(&name)));
        }
        if ui
            .button("Delete")
            .on_hover_text("Remove this node effect and the effect cards using it (Ctrl+Z brings it back)")
            .clicked()
        {
            delete(scene, index);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Add block").on_hover_text("Space").clicked() {
                state.editor.open_palette();
            }
        });
    });
    if let Some(note) = &state.note {
        ui.label(RichText::new(note.as_str()).small().color(theme::WARNING));
    }
}

/// "New node effect": empty, or from one of the node effects that come with the program.
fn new_menu(ui: &mut Ui, scene: &mut Scene, state: &mut State, library: &EffectLibrary) {
    ui.menu_button("New", |ui| {
        ui.set_min_width(240.0);
        if ui.button("Empty").on_hover_text("Just the picture, ready for your blocks").clicked() {
            state.open(add_graph(scene, library, blank_graph(), "My effect"));
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new("Start from").small().color(theme::MUTED));
        for template in fx::templates::graphs() {
            let stem = format!("My {}", template.name.to_lowercase());
            if ui.button(&template.name).clicked() {
                state.open(add_graph(scene, library, template, &stem));
                ui.close();
            }
        }
        ui.menu_button("A built-in effect", |ui| {
            for id in fx::templates::REBUILT {
                let title = library.get(id).map_or_else(|| id.to_string(), |i| i.title.clone());
                if ui.button(&title).clicked()
                    && let Some(graph) = fx::templates::rebuild(id)
                {
                    let stem = graph.name.clone();
                    state.open(add_graph(scene, library, graph, &stem));
                    ui.close();
                }
            }
        });
    });
}

/// The picture wired straight to the output: an effect that changes nothing yet.
pub fn blank_graph() -> bpe_graph::Graph {
    let mut b = fx::graph::Builder::new("");
    let picture = b.add("picture");
    let out = b.add("output");
    b.wire(picture, "out", out, "color");
    let mut graph = b.finish();
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == out) {
        node.pos = [260.0, 0.0];
    }
    graph
}

/// Adds `graph` to the scene under a free name starting with `stem`, with an effect card using
/// it. Returns the name.
pub fn add_graph(scene: &mut Scene, library: &EffectLibrary, mut graph: bpe_graph::Graph, stem: &str) -> String {
    graph.name = free_name(scene, library, stem);
    let name = graph.name.clone();
    scene.shader_graphs.push(graph);
    library.sync_graphs(&scene.shader_graphs);
    scene.effects.push(Effect::new(fx::graph::effect_id(&name)));
    name
}

/// A node effect name not used by the scene or by the ones that come with the program.
pub fn free_name(scene: &Scene, library: &EffectLibrary, stem: &str) -> String {
    let stem = if stem.trim().is_empty() { "My effect" } else { stem.trim() };
    (1..)
        .map(|n| if n == 1 { stem.to_string() } else { format!("{stem} {n}") })
        .find(|name| {
            !scene.shader_graphs.iter().any(|g| g.name == *name)
                && library.get(&fx::graph::effect_id(name)).is_none_or(|i| !i.is_builtin())
        })
        .unwrap()
}

/// Renames a node effect, and the effect cards using it.
fn rename(scene: &mut Scene, from: &str, to: &str) -> Result<(), String> {
    if to.is_empty() {
        return Err("A node effect needs a name.".into());
    }
    if scene.shader_graphs.iter().any(|g| g.name == to) || fx::templates::graphs().iter().any(|g| g.name == to) {
        return Err(format!("There is already an effect called {to}."));
    }
    let (old, new) = (fx::graph::effect_id(from), fx::graph::effect_id(to));
    for graph in scene.shader_graphs.iter_mut().filter(|g| g.name == from) {
        graph.name = to.to_string();
    }
    for effect in scene.effects.iter_mut().filter(|e| e.shader == old) {
        effect.shader = new.clone();
    }
    Ok(())
}

/// Removes node effect `index` and the effect cards using it, with their keyframes and pulses.
fn delete(scene: &mut Scene, index: usize) {
    let id = fx::graph::effect_id(&scene.shader_graphs.remove(index).name);
    while let Some(i) = scene.effects.iter().position(|e| e.shader == id) {
        scene.effects.remove(i);
        let i = i as u32;
        scene.remap_effect_tracks(|k| match k {
            k if k == i => None,
            k if k > i => Some(k - 1),
            k => Some(k),
        });
    }
}

fn welcome(ui: &mut Ui, scene: &mut Scene, state: &mut State, library: &EffectLibrary) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.25);
        ui.label(RichText::new("Build picture effects from blocks").size(18.0).strong());
        ui.add_space(4.0);
        ui.label(
            RichText::new("Connect blocks such as Ripple, Kaleidoscope or Bass to make an effect, no code needed.")
                .color(theme::MUTED),
        );
        ui.add_space(12.0);
        if ui.button(RichText::new("Start an empty node effect").strong()).clicked() {
            state.open(add_graph(scene, library, blank_graph(), "My effect"));
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Or start from").color(theme::MUTED));
            for template in fx::templates::graphs() {
                if ui.button(&template.name).clicked() {
                    let stem = format!("My {}", template.name.to_lowercase());
                    state.open(add_graph(scene, library, template, &stem));
                }
            }
        });
        let rebuildable: Vec<usize> = scene
            .effects
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                fx::templates::rebuild(&e.shader).is_some() && library.get(&e.shader).is_some_and(|i| i.is_builtin())
            })
            .map(|(i, _)| i)
            .collect();
        if !rebuildable.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Or open as blocks").color(theme::MUTED));
                for i in rebuildable {
                    let shader = scene.effects[i].shader.clone();
                    let title = library.get(&shader).map_or_else(|| shader.clone(), |info| info.title.clone());
                    if ui.button(title).on_hover_text("Swap this effect for its node version. It looks the same.").clicked()
                        && let Some(graph) = fx::templates::rebuild(&shader)
                    {
                        let mut graph = graph;
                        graph.name = free_name(scene, library, &graph.name);
                        let name = graph.name.clone();
                        scene.shader_graphs.push(graph);
                        scene.effects[i].shader = fx::graph::effect_id(&name);
                        state.open(name);
                    }
                }
            });
        }
    });
}

/// Beside the canvas: the effect's sliders, the selected block's help, and problems.
fn side(ui: &mut Ui, scene: &mut Scene, index: usize, info: Option<&EffectInfo>, state: &State) {
    let name = scene.shader_graphs[index].name.clone();
    let id = fx::graph::effect_id(&name);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(RichText::new("Effect sliders").strong());
        let Some(info) = info else { return };
        let card = scene.effects.iter().position(|e| e.shader == id);
        if info.params.is_empty() {
            ui.label(
                RichText::new("Right-click a value on a block and pick \"show\" to turn it into a slider here and on the effect card.")
                    .small()
                    .color(theme::MUTED),
            );
        } else if let Some(card) = card {
            let effect = &mut scene.effects[card];
            for param in &info.params {
                let stored = effect.params.get(&param.name).copied().filter(|v| v.is_finite());
                let mut value = stored.unwrap_or(param.default);
                ui.label(crate::inspector::pretty_name(&param.name)).on_hover_text(&param.help);
                let range = param.min.min(param.max)..=param.max.max(param.min);
                let changed = match param.kind {
                    fx::ParamKind::Toggle => {
                        let mut on = value > 0.5;
                        let changed = ui.checkbox(&mut on, "").changed();
                        value = if on { 1.0 } else { 0.0 };
                        changed
                    }
                    fx::ParamKind::Whole => ui.add(egui::Slider::new(&mut value, range).step_by(1.0)).changed(),
                    fx::ParamKind::Slider => ui.add(egui::Slider::new(&mut value, range)).changed(),
                };
                if changed {
                    effect.params.insert(param.name.clone(), value);
                }
            }
        } else {
            ui.label(RichText::new("Add this effect to the video to use its sliders.").small().color(theme::MUTED));
        }

        if let Some(selected) = state.editor.selection().iter().next().filter(|_| state.editor.selection().len() == 1)
            && let Some(node) = scene.shader_graphs[index].nodes.iter().find(|n| n.id == *selected)
            && let Some(block) = fx::graph::Catalog::get().block(&node.kind)
        {
            ui.add_space(12.0);
            ui.label(RichText::new(block.title).strong());
            ui.label(RichText::new(block.help).small().color(theme::MUTED));
        }

        if let Some(graph) = &info.graph
            && !graph.problems.is_empty()
        {
            ui.add_space(12.0);
            let broken = info.error().is_some();
            let (title, color) =
                if broken { ("Problems", theme::ERROR) } else { ("Blocks doing nothing", theme::WARNING) };
            ui.label(RichText::new(title).strong().color(color));
            if broken {
                ui.label(
                    RichText::new("The effect is skipped until these are fixed. The video keeps playing without it.")
                        .small()
                        .color(theme::MUTED),
                );
            }
            for problem in &graph.problems {
                ui.label(RichText::new(format!("• {}", problem.message)).small().color(color));
            }
        }
    });
}

/// Draws the visible blocks' previews when due, and returns the texture for each block.
fn previews(
    ctx: &egui::Context,
    render_state: &egui_wgpu::RenderState,
    scene: &Scene,
    index: usize,
    state: &mut State,
    library: &EffectLibrary,
    playback: &Playback,
    time: f64,
) -> HashMap<u32, egui::TextureId> {
    let device = &render_state.device;
    let graph = &scene.shader_graphs[index];
    let effect = scene.effects.iter().position(|e| e.shader == fx::graph::effect_id(&graph.name));
    let previews = state.previews.get_or_insert_with(|| {
        let (w, h) = fx::THUMB_SIZE;
        let mut renderer = Renderer::new(device, w, h);
        renderer.set_effect_library(library.clone());
        Previews {
            renderer,
            frame_data: FrameData::default(),
            info: None,
            tiles: Vec::new(),
            shown: HashMap::new(),
            last: None,
            drawn: None,
        }
    });
    // Previews only show where the effect is in the video.
    let Some(effect) = effect else {
        previews.renderer.set_effect_thumbnails(None);
        previews.shown.clear();
        return HashMap::new();
    };
    if previews.info.as_ref().is_none_or(|(g, _)| g != graph) {
        previews.info = Some((graph.clone(), Arc::new(EffectInfo::graph_thumbnails(graph))));
    }
    let info = previews.info.as_ref().unwrap().1.clone();
    let order = info.graph.as_ref().and_then(|g| g.generated.as_ref()).map(|g| g.thumbnails.clone());
    let Some(order) = order.filter(|_| info.error().is_none()) else {
        previews.shown.clear();
        return HashMap::new();
    };
    let wanted: Vec<u32> =
        state.visible.iter().copied().filter(|id| order.contains(id)).take(fx::MAX_THUMBS).collect();
    let due = previews.last.is_none_or(|t| t.elapsed() >= PREVIEW_EVERY);
    let key = (scene.clone(), time, wanted.clone());
    if due && previews.drawn.as_ref() != Some(&key) && !wanted.is_empty() {
        let views = wanted.iter().map(|id| order.iter().position(|o| o == id).unwrap() as u32 + 1).collect();
        previews.renderer.set_effect_thumbnails(Some(ThumbnailRequest { effect, info, views }));
        let pixel = previews.renderer.pixel_size(scene);
        previews.frame_data.fill(scene, playback, pixel);
        previews.renderer.render(device, &render_state.queue, scene, time, &previews.frame_data);
        if previews.renderer.effect_thumbnails_drawn() {
            copy_tiles(render_state, previews, wanted.len());
            previews.shown = wanted.iter().enumerate().map(|(slot, &id)| (id, slot)).collect();
        } else {
            previews.shown.clear();
        }
        previews.last = Some(Instant::now());
        previews.drawn = Some(key);
    } else if !due && previews.drawn.as_ref() != Some(&key) {
        // Come back when the previews are due.
        ctx.request_repaint_after(PREVIEW_EVERY);
    }
    previews.shown.iter().filter_map(|(&id, &slot)| Some((id, previews.tiles.get(slot)?.1))).collect()
}

/// Copies the first `count` atlas tiles into the per-block textures, making any that are missing.
fn copy_tiles(render_state: &egui_wgpu::RenderState, previews: &mut Previews, count: usize) {
    let device = &render_state.device;
    let (w, h) = fx::THUMB_SIZE;
    while previews.tiles.len() < count {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("block preview"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let id = render_state.renderer.write().register_native_texture(device, &view, wgpu::FilterMode::Linear);
        previews.tiles.push((texture, id));
    }
    let atlas = previews.renderer.effect_thumbnails(device).texture().clone();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("block previews") });
    for (i, (tile, _)) in previews.tiles.iter().take(count).enumerate() {
        let [x, y, w, h] = fx::thumbnail_tile(i);
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            tile.as_image_copy(),
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
    render_state.queue.submit([encoder.finish()]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_moves_the_effect_cards_along() {
        let library = EffectLibrary::new();
        let mut scene = Scene::default();
        let name = add_graph(&mut scene, &library, blank_graph(), "Glow");
        assert_eq!(name, "Glow");
        assert_eq!(scene.effects[0].shader, "graph:Glow");
        rename(&mut scene, "Glow", "Shine").unwrap();
        assert_eq!(scene.shader_graphs[0].name, "Shine");
        assert_eq!(scene.effects[0].shader, "graph:Shine");
        let second = add_graph(&mut scene, &library, blank_graph(), "Shine");
        assert_eq!(second, "Shine 2");
        assert!(rename(&mut scene, "Shine 2", "Shine").is_err());
        assert!(rename(&mut scene, "Shine 2", "").is_err());
    }

    #[test]
    fn names_never_hide_the_effects_that_come_with_the_program() {
        let library = EffectLibrary::new();
        let scene = Scene::default();
        let template = &fx::templates::graphs()[0].name;
        assert_ne!(free_name(&scene, &library, template), *template);
    }

    #[test]
    fn deleting_removes_its_cards_and_their_keyframes() {
        let library = EffectLibrary::new();
        let mut scene = Scene::default();
        scene.effects.push(Effect::new("film"));
        add_graph(&mut scene, &library, blank_graph(), "Glow");
        let mut track = bpe_scene::Track::new(bpe_scene::Property::EffectAmount(1));
        track.set_key(0.0, bpe_scene::Value::Number(0.5));
        scene.animation.push(track);
        delete(&mut scene, 0);
        assert!(scene.shader_graphs.is_empty());
        assert_eq!(scene.effects.len(), 1);
        assert!(scene.animation.is_empty());
    }

    struct Studio {
        inspector: crate::inspector::Inspector,
        scene: Scene,
    }

    fn effects_tab(scene: Scene) -> egui_kittest::Harness<'static, Studio> {
        let state = Studio {
            inspector: crate::inspector::Inspector { tab: crate::inspector::Tab::Effects, ..Default::default() },
            scene,
        };
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(460.0, 2400.0))
            .with_max_steps(240)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(|ui, s: &mut Studio| crate::inspector::show(ui, &mut s.inspector, &mut s.scene), state);
        harness.run();
        harness
    }

    #[test]
    fn new_node_effect_adds_a_card_and_opens_the_workspace() {
        use egui_kittest::kittest::Queryable;
        let mut harness = effects_tab(Scene::default());
        harness.get_by_label("New node effect").click();
        harness.run();
        harness.get_by_label("Start empty").click();
        harness.run();
        let state = harness.state();
        assert_eq!(state.scene.shader_graphs.len(), 1);
        assert_eq!(state.scene.effects[0].shader, fx::graph::effect_id(&state.scene.shader_graphs[0].name));
        assert_eq!(state.inspector.effects.open_graph.as_deref(), Some("My effect"));
    }

    #[test]
    fn edit_as_nodes_swaps_a_built_in_for_its_node_version() {
        use egui_kittest::kittest::Queryable;
        let scene = Scene { effects: vec![Effect::new("kaleidoscope").with("slices", 9.0)], ..Scene::default() };
        let mut harness = effects_tab(scene);
        harness.get_by_label("More options").click();
        harness.run();
        harness.get_by_label("Edit as nodes").click();
        harness.run();
        let scene = &harness.state().scene;
        assert_eq!(scene.shader_graphs.len(), 1);
        assert!(fx::graph::graph_name(&scene.effects[0].shader).is_some());
        // Its sliders carry over, since the node version has sliders of the same names.
        assert_eq!(scene.effects[0].params["slices"], 9.0);
        assert!(harness.query_by_label("nodes").is_some(), "the card says it is made of blocks");
    }

    #[test]
    fn every_template_and_rebuild_makes_a_working_effect() {
        for graph in fx::templates::graphs().into_iter().chain(fx::templates::REBUILT.iter().filter_map(|id| fx::templates::rebuild(id))) {
            let info = EffectInfo::from_graph(&graph, false);
            assert!(info.error().is_none(), "{}: {:?}", graph.name, info.error());
        }
        let blank = EffectInfo::from_graph(&blank_graph(), false);
        assert!(blank.error().is_none(), "{:?}", blank.error());
    }
}

/// Writes a scene with node effects and pulses to `$BPE_DEMO_SCENE`, for screenshots:
/// `BPE_DEMO_SCENE=demo.ron cargo test -p bpe-app write_demo_scene -- --ignored`.
#[cfg(test)]
#[test]
#[ignore]
fn write_demo_scene() {
    let Ok(path) = std::env::var("BPE_DEMO_SCENE") else { return };
    let library = EffectLibrary::new();
    let mut scene = Scene::presets()[0].scene.clone();
    let tunnel = fx::templates::graphs().into_iter().next().unwrap();
    add_graph(&mut scene, &library, tunnel, "My bass tunnel");
    add_graph(&mut scene, &library, fx::templates::rebuild("kaleidoscope").unwrap(), "Kaleidoscope (nodes)");
    for (property, band) in [
        (bpe_scene::Property::Bloom, bpe_scene::AudioBand::Bass),
        (bpe_scene::Property::ArenaColor, bpe_scene::AudioBand::Beat),
        (bpe_scene::Property::Gravity, bpe_scene::AudioBand::Treble),
    ] {
        let base = property.get(&scene);
        let mut m = bpe_scene::Modulator::new(property.clone(), band);
        if let Some(bpe_scene::Value::Number(v)) = base {
            m.amount = crate::setting_menu::pulse::pulse_amount(&property, v);
        }
        scene.modulators.push(m);
    }
    scene.save(std::path::Path::new(&path)).unwrap();
}

#[cfg(test)]
#[test]
fn looking_at_a_node_effect_does_not_change_it() {
    let rebuilt = fx::templates::REBUILT.iter().filter_map(|id| fx::templates::rebuild(id));
    for graph in fx::templates::graphs().into_iter().chain(rebuilt) {
        let before = graph.clone();
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
            |ui, g: &mut bpe_graph::Graph| {
                let mut editor = NodeEditor::new("t");
                editor.show(ui, g, catalog(), None);
            },
            graph,
        );
        harness.run_steps(3);
        let after = harness.state();
        for (a, b) in before.nodes.iter().zip(&after.nodes) {
            assert_eq!(a, b, "{}", before.name);
        }
        assert_eq!(before.links, after.links, "{}", before.name);
        assert_eq!(&before, after);
    }
}
