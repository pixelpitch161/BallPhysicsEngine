//! The preset gallery: every built-in starting point with a small moving preview.

use crate::theme;
use bpe_render::{FrameData, Renderer};
use bpe_scene::{Preset, Scene};
use bpe_sim::Playback;
use eframe::egui::{self, Color32, RichText, Sense};
use std::time::Instant;

/// Thumbnail size in screen points.
const THUMB: egui::Vec2 = egui::vec2(135.0, 240.0);
/// Each thumbnail loops over the first this many seconds of its scene.
const LOOP_SECONDS: f64 = 10.0;

pub struct Gallery {
    pub open: bool,
    presets: Vec<Preset>,
    thumbs: Vec<Thumb>,
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
        Self { open, presets: Scene::presets(), thumbs: Vec::new(), opened: Instant::now() }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.opened = Instant::now();
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
            egui::Grid::new("presets").spacing([14.0, 14.0]).show(ui, |ui| {
                for (i, preset) in self.presets.iter().enumerate() {
                    if card(ui, preset, self.thumbs.get(i).map(|t| t.texture)).clicked() {
                        picked = Some(preset.scene.clone());
                    }
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
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
        ctx.request_repaint();
        picked
    }

    fn render_thumbnails(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else { return };
        let device = &render_state.device;
        let pixels = ctx.pixels_per_point();
        if self.thumbs.is_empty() {
            for preset in &self.presets {
                let scale = THUMB.x * pixels / preset.scene.video.width as f32;
                let (width, height) = bpe_export::output_size(&preset.scene, scale);
                let renderer = Renderer::new(device, width, height);
                let texture = render_state.renderer.write().register_native_texture(
                    device,
                    renderer.output_view(),
                    bpe_render::wgpu::FilterMode::Linear,
                );
                self.thumbs.push(Thumb {
                    renderer,
                    texture,
                    playback: Playback::new(&preset.scene),
                    frame: FrameData::default(),
                });
            }
        }
        let time = self.opened.elapsed().as_secs_f64();
        for (preset, thumb) in self.presets.iter().zip(&mut self.thumbs) {
            let time = time % LOOP_SECONDS.min(preset.scene.video.duration as f64);
            thumb.playback.seek(time);
            thumb.frame.fill(&preset.scene, &thumb.playback, thumb.renderer.pixel_size(&preset.scene));
            thumb.renderer.render(device, &render_state.queue, &preset.scene, time, &thumb.frame);
        }
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
