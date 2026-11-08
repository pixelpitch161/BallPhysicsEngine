//! The Looks tab's tile pictures: each look worn by the selected ball (a ball look) or by a sample solid ellipse
//! wearing the shape's own Look first (a shape look), drawn once by one shared renderer and copied into a picture of
//! the tile's own. A tile is drawn again only when what it shows changes (the thing's Look, its size, the colours),
//! and only a few a frame, so opening the tab never stalls the editor: tiles still being drawn show the look's two
//! colours.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use bpe_core::{Address, Scene, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer};
use bpe_kit::Kit;
use bpe_kit::ready_looks::{self, Colours, How, ReadyLook, Wears};
use bpe_runtime::World;
use eframe::{egui, egui_wgpu, wgpu};

use crate::look_editor::ball_view::{self, Ground, ViewMode};

/// A tile picture's size, in pixels.
pub const PIXELS: u32 = 168;
/// How many tiles are drawn in one frame at most.
pub const PER_FRAME: usize = 4;
/// When a tile's ball look is drawn: a second in, its trail (if any) behind it half a second of moving.
const AT: f64 = 1.0;
const MOVED: f64 = 0.5;

/// The tile pictures, one set for the editor.
#[derive(Default)]
pub struct Thumbs {
    renderer: Option<Renderer>,
    frame: Frame,
    tiles: HashMap<&'static str, Tile>,
    /// The editor frame being drawn, and how many tiles it has drawn so far.
    pass: u64,
    made: usize,
    /// What every tile of this frame shares (the thing and its Look), worked out once a frame.
    base: Option<(u64, ThingId, u64)>,
}

struct Tile {
    /// What the picture shows; 0 when it should be drawn again (its shaders were still building).
    key: u64,
    texture: wgpu::Texture,
    id: egui::TextureId,
}

impl Thumbs {
    /// The picture of `look` on thing `id` with `colours`: drawn now if it changed and this frame has room, else
    /// the last one drawn; `None` before the first one (or without a GPU).
    #[allow(clippy::too_many_arguments)]
    pub fn tile(
        &mut self,
        ui: &egui::Ui,
        render: Option<&egui_wgpu::RenderState>,
        scene: &Scene,
        kit: &Kit,
        id: ThingId,
        look: &'static ReadyLook,
        colours: Colours,
    ) -> Option<egui::TextureId> {
        let render = render?;
        let pass = ui.ctx().cumulative_pass_nr();
        if pass != self.pass {
            self.pass = pass;
            self.made = 0;
        }
        let base = match self.base {
            Some((p, t, base)) if p == pass && t == id => base,
            _ => {
                let base = base_key(scene, kit, id);
                self.base = Some((pass, id, base));
                base
            }
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (base, look.id, colours).hash(&mut h);
        let key = h.finish().max(1);
        let old = self.tiles.get(look.id).map(|t| (t.key, t.id));
        if let Some((k, texture)) = old
            && k == key
        {
            return Some(texture);
        }
        if self.made >= PER_FRAME {
            ui.ctx().request_repaint();
            return old.map(|(_, t)| t);
        }
        self.made += 1;
        if !tile_frame(scene, kit, id, look, colours, &mut self.frame) {
            return old.map(|(_, t)| t);
        }
        let device = &render.device;
        let renderer = self.renderer.get_or_insert_with(|| {
            let mut renderer = Renderer::new(device, PIXELS, PIXELS);
            renderer.share_programs(crate::preview::shared_programs());
            renderer
        });
        renderer.forget_effect_history();
        renderer.render(device, &render.queue, &self.frame);
        let waiting = crate::preview::redraw_when_built(renderer, ui.ctx());
        let tile = self.tiles.entry(look.id).or_insert_with(|| new_tile(render));
        let size = wgpu::Extent3d { width: PIXELS, height: PIXELS, depth_or_array_layers: 1 };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("look tile") });
        encoder.copy_texture_to_texture(renderer.output_texture().as_image_copy(), tile.texture.as_image_copy(), size);
        render.queue.submit([encoder.finish()]);
        tile.key = if waiting { 0 } else { key };
        Some(tile.id)
    }
}

/// A tile's own picture, registered with egui.
fn new_tile(render: &egui_wgpu::RenderState) -> Tile {
    let texture = render.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("look tile"),
        size: wgpu::Extent3d { width: PIXELS, height: PIXELS, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: bpe_gfx::OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let id = render.renderer.write().register_native_texture(&render.device, &view, wgpu::FilterMode::Linear);
    Tile { key: 0, texture, id }
}

/// What every tile of thing `id` depends on: its kind, its Look card (with the Look's keys and pulses) and, for
/// balls, its size.
fn base_key(scene: &Scene, kit: &Kit, id: ThingId) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(t) = scene.things.get(&id) {
        t.kind.hash(&mut h);
        format!("{:?}", t.slots.get(&SlotId::Look)).hash(&mut h);
        let look = |a: &&Address| a.thing == id && a.slot == Some(SlotId::Look);
        format!("{:?}", scene.tracks.keys().filter(look).collect::<Vec<_>>()).hash(&mut h);
        format!("{:?}", scene.pulses.iter().filter(|(a, _)| look(a)).collect::<Vec<_>>()).hash(&mut h);
        radius(scene, kit, id).to_bits().hash(&mut h);
    }
    h.finish()
}

/// The ball's radius at the start, for its tiles.
fn radius(scene: &Scene, kit: &Kit, id: ThingId) -> f32 {
    match scene.value_at_time(&kit.registry, &Address::own(id, "radius"), 0.0) {
        Some(Value::Number(r)) => r as f32,
        _ => 24.0,
    }
}

/// Builds the frame of `look` worn by thing `id` (a ball look) or a sample ellipse (a shape look) into `frame`.
/// False when it can't be worn.
pub fn tile_frame(
    scene: &Scene,
    kit: &Kit,
    id: ThingId,
    look: &ReadyLook,
    colours: Colours,
    frame: &mut Frame,
) -> bool {
    let reg = &kit.registry;
    match look.wears {
        Wears::Balls => {
            let Some(mut mini) = ball_view::mini_scene(scene, kit, id, Ground::Dark) else { return false };
            if ready_looks::apply(&mut mini, reg, id, look, colours, How::Replace).is_err() {
                return false;
            }
            let resolved = bpe_core::resolve(&mini, reg);
            let mut world = World::build(&mini, &resolved, &kit.sims);
            // A look with a trail shows it behind the ball, moving.
            let trail = look.recipe(None).layers.iter().any(|l| l.kind == bpe_kit::modules::look::layers::TRAIL);
            let (mode, clock) = if trail { (ViewMode::Moving, MOVED) } else { (ViewMode::Still, 0.0) };
            let r = radius(scene, kit, id);
            ball_view::build_view_frame_in(&mut world, &mini, &resolved, kit, id, 0, r, mode, AT, clock, 0, frame);
        }
        Wears::Shape => {
            let Some(mut mini) = super::shape_view::mini(scene, kit, id, true) else { return false };
            if ready_looks::apply(&mut mini, reg, id, look, colours, How::Replace).is_err() {
                return false;
            }
            let resolved = bpe_core::resolve(&mini, reg);
            let world = World::build(&mini, &resolved, &kit.sims);
            super::shape_view::build(&world, &mini, &resolved, kit, id, AT, None, frame);
        }
    }
    true
}
