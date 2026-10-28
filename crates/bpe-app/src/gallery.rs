//! The preset gallery: every built-in starting point with a small moving preview.

use crate::theme;
use bpe_render::{FrameData, Renderer};
use bpe_scene::{Preset, Scene};
use bpe_sim::Playback;
use eframe::egui::{self, Color32, RichText, Sense};
use std::time::{Duration, Instant};

/// Thumbnail size in screen points.
const THUMB: egui::Vec2 = egui::vec2(135.0, 240.0);
/// Each thumbnail loops over the first this many seconds of its scene.
const LOOP_SECONDS: f64 = 10.0;
/// Most time one preview spends catching its simulation up in an editor frame.
const THUMB_BUDGET: Duration = Duration::from_millis(3);
/// Most time all the previews together spend in an editor frame. Previews past it wait for the
/// next frame, starting where this one stopped.
const FRAME_BUDGET: Duration = Duration::from_millis(8);
/// Previews move at about 30 frames a second, however fast the screen refreshes.
const FRAME_EVERY: Duration = Duration::from_millis(33);

pub struct Gallery {
    pub open: bool,
    presets: Vec<Preset>,
    /// A preview for each preset, made the first time its card scrolls into view.
    thumbs: Vec<Option<Thumb>>,
    /// Which cards were on screen last frame. Only those are simulated and drawn.
    visible: Vec<bool>,
    /// The card to start from next frame, so a long list shares the budget fairly.
    next: usize,
    last_drawn: Option<Instant>,
    opened: Instant,
}

struct Thumb {
    renderer: Renderer,
    texture: egui::TextureId,
    playback: Playback,
    frame: FrameData,
}

impl Gallery {
    pub fn new(open: bool) -> Self {
        let presets = Scene::presets();
        let count = presets.len();
        Self {
            open,
            presets,
            thumbs: (0..count).map(|_| None).collect(),
            visible: vec![false; count],
            next: 0,
            last_drawn: None,
            opened: Instant::now(),
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.opened = Instant::now();
        self.last_drawn = None;
    }

    /// Shows the gallery if it is open. Returns the preset the user picked.
    pub fn show(&mut self, ctx: &egui::Context, frame: &eframe::Frame) -> Option<Scene> {
        if !self.open {
            return None;
        }
        self.render_thumbnails(ctx, frame);
        let mut picked = None;
        let modal = egui::Modal::new(egui::Id::unique("preset-gallery")).show(ctx, |ui| {
            ui.set_width(3.0 * (THUMB.x + 60.0));
            ui.heading("Start from a preset");
            ui.label(
                RichText::new("Pick a starting point. You can change everything about it afterwards.")
                    .color(theme::MUTED),
            );
            ui.add_space(10.0);
            // Leave room for the title and buttons; the presets scroll when they don't all fit.
            let max_height = (ctx.content_rect().height() - 220.0).max(THUMB.y + 80.0);
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
                egui::Grid::new("presets").spacing([14.0, 14.0]).show(ui, |ui| {
                    for (i, preset) in self.presets.iter().enumerate() {
                        let texture = self.thumbs[i].as_ref().map(|t| t.texture);
                        let response = card(ui, preset, texture);
                        self.visible[i] = ui.is_rect_visible(response.rect);
                        if response.clicked() {
                            picked = Some(preset.scene.clone());
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            });
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Keep my current scene").clicked() {
                    self.open = false;
                }
            });
        });
        if modal.should_close() || picked.is_some() {
            self.open = false;
        }
        if self.open {
            let wait = self.last_drawn.map_or(Duration::ZERO, |t| FRAME_EVERY.saturating_sub(t.elapsed()));
            ctx.request_repaint_after(wait);
        }
        picked
    }

    fn render_thumbnails(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else { return };
        if self.last_drawn.is_some_and(|t| t.elapsed() < FRAME_EVERY) {
            return;
        }
        self.last_drawn = Some(Instant::now());
        let device = &render_state.device;
        let pixels = ctx.pixels_per_point();
        let started = Instant::now();
        let count = self.presets.len();
        let time = self.opened.elapsed().as_secs_f64();
        let mut stopped_at = None;
        for i in (0..count).map(|k| (self.next + k) % count).filter(|&i| self.visible[i]) {
            if started.elapsed() >= FRAME_BUDGET {
                stopped_at = Some(i);
                break;
            }
            let preset = &self.presets[i];
            let thumb = self.thumbs[i].get_or_insert_with(|| {
                let scale = THUMB.x * pixels / preset.scene.video.width as f32;
                let (width, height) = bpe_export::output_size(&preset.scene, scale);
                let mut renderer = Renderer::new(device, width, height);
                // A preset's own node effects, in a library of its own.
                if !preset.scene.shader_graphs.is_empty() {
                    let library = bpe_render::EffectLibrary::new();
                    library.sync_graphs(&preset.scene.shader_graphs);
                    renderer.set_effect_library(library);
                }
                let texture = render_state.renderer.write().register_native_texture(
                    device,
                    renderer.output_view(),
                    bpe_render::wgpu::FilterMode::Linear,
                );
                Thumb {
                    renderer,
                    texture,
                    playback: Playback::with_inputs(&preset.scene, &bpe_audio::sim_inputs(&preset.scene, None)),
                    frame: FrameData::default(),
                }
            });
            let time = time % LOOP_SECONDS.min(preset.scene.video.duration as f64);
            // A heavy variation catches up over a few frames rather than holding up the others.
            let budget = THUMB_BUDGET.min(FRAME_BUDGET.saturating_sub(started.elapsed()));
            thumb.playback.seek_within(time, Some(budget));
            let time = thumb.playback.simulation().time();
            thumb.frame.fill(&preset.scene, &thumb.playback, thumb.renderer.pixel_size(&preset.scene));
            thumb.renderer.render(device, &render_state.queue, &preset.scene, time, &thumb.frame);
        }
        self.next = stopped_at.unwrap_or(0);
    }
}

fn card(ui: &mut egui::Ui, preset: &Preset, texture: Option<egui::TextureId>) -> egui::Response {
    let width = THUMB.x + 46.0;
    let response = egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(10.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(width);
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.vertical_centered(|ui| match texture {
                    Some(texture) => {
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(texture, THUMB)).corner_radius(6.0));
                    }
                    None => {
                        ui.allocate_exact_size(THUMB, Sense::hover());
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(preset.name).strong().color(theme::TEXT));
                ui.add(egui::Label::new(RichText::new(preset.description).small().color(theme::MUTED)).wrap());
            });
        })
        .response
        .interact(Sense::click());
    if response.hovered() {
        ui.painter().rect_stroke(response.rect, 10.0, egui::Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else {
        ui.painter().rect_stroke(
            response.rect,
            10.0,
            egui::Stroke::new(1.0, Color32::TRANSPARENT),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text("Start a new scene from this preset")
}
