//! The preset gallery: every built-in starting point with a small moving preview, after the
//! scenes the user saved as their own presets.

use crate::theme;
use bpe_render::{FrameData, Renderer};
use bpe_scene::{Preset, SavedPreset, Scene};
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
    /// The user's saved scenes first, then the built-in presets.
    presets: Vec<Entry>,
    /// How many of `presets` are the user's own.
    mine: usize,
    /// The name typed for saving the current scene as a preset.
    save_name: String,
    /// What the last save or delete did.
    message: Option<String>,
    /// A preview for each preset, made the first time its card scrolls into view.
    thumbs: Vec<Option<Thumb>>,
    /// Which cards were on screen last frame. Only those are simulated and drawn.
    visible: Vec<bool>,
    /// The card to start from next frame, so a long list shares the budget fairly.
    next: usize,
    last_drawn: Option<Instant>,
    opened: Instant,
}

/// One card in the gallery.
struct Entry {
    name: String,
    description: String,
    scene: Scene,
    /// The user's own saved preset, which can be deleted.
    saved: Option<SavedPreset>,
}

impl From<Preset> for Entry {
    fn from(preset: Preset) -> Self {
        Self { name: preset.name.into(), description: preset.description.into(), scene: preset.scene, saved: None }
    }
}

impl From<SavedPreset> for Entry {
    fn from(saved: SavedPreset) -> Self {
        let bpe_scene::PresetData::Scene(scene) = &saved.data else { unreachable!("only scenes are listed") };
        Self {
            name: saved.name.clone(),
            description: "Saved by you".into(),
            scene: (**scene).clone(),
            saved: Some(saved),
        }
    }
}

/// A gallery card and its preview, if made yet.
type Thumbed = (Entry, Option<Thumb>);

struct Thumb {
    renderer: Renderer,
    texture: egui::TextureId,
    playback: Playback,
    frame: FrameData,
}

impl Gallery {
    pub fn new(open: bool) -> Self {
        let mut gallery = Self {
            open,
            presets: Vec::new(),
            mine: 0,
            save_name: String::new(),
            message: None,
            thumbs: Vec::new(),
            visible: Vec::new(),
            next: 0,
            last_drawn: None,
            opened: Instant::now(),
        };
        gallery.reload();
        gallery
    }

    /// Reads the user's saved scenes again. Built-in previews already made are kept.
    fn reload(&mut self) {
        let mine: Vec<Entry> = crate::my_presets::scenes().into_iter().map(Entry::from).collect();
        let unchanged = mine.len() == self.mine && mine.iter().zip(&self.presets).all(|(a, b)| a.saved == b.saved);
        if unchanged && !self.presets.is_empty() {
            return;
        }
        let built_in: Vec<Thumbed> = if self.presets.is_empty() {
            Scene::presets().into_iter().map(|p| (Entry::from(p), None)).collect()
        } else {
            let thumbs = std::mem::take(&mut self.thumbs);
            std::mem::take(&mut self.presets).into_iter().zip(thumbs).skip(self.mine).collect()
        };
        self.mine = mine.len();
        let (presets, thumbs) = mine.into_iter().map(|e| (e, None)).chain(built_in).unzip();
        self.presets = presets;
        self.thumbs = thumbs;
        self.visible = vec![false; self.presets.len()];
        self.next = 0;
    }

    pub fn open(&mut self) {
        self.open = true;
        self.opened = Instant::now();
        self.last_drawn = None;
        self.message = None;
        self.reload();
    }

    /// Shows the gallery if it is open. Returns the preset the user picked. `current` and
    /// `base_dir` (its file's folder) are for saving the current scene as a preset.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        current: &Scene,
        base_dir: Option<&std::path::Path>,
    ) -> Option<Scene> {
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
            let mut delete = None;
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
                for (section, range) in [("MY SCENES", 0..self.mine), ("BUILT IN", self.mine..self.presets.len())] {
                    if self.mine == 0 || range.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(section).size(12.0).color(theme::MUTED));
                    ui.add_space(4.0);
                    self.cards(ui, section, range, &mut picked, &mut delete);
                    ui.add_space(10.0);
                }
                if self.mine == 0 {
                    self.cards(ui, "presets", 0..self.presets.len(), &mut picked, &mut delete);
                }
            });
            if let Some(i) = delete {
                let entry = &self.presets[i];
                self.message = Some(match entry.saved.as_ref().map(crate::my_presets::delete) {
                    Some(Ok(())) => format!("Deleted \"{}\".", entry.name),
                    Some(Err(error)) => format!("Couldn't delete it: {error}"),
                    None => String::new(),
                });
                self.reload();
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label("Save my current scene as");
                ui.add(egui::TextEdit::singleline(&mut self.save_name).desired_width(170.0).hint_text(&current.name));
                let name = if self.save_name.trim().is_empty() { current.name.trim() } else { self.save_name.trim() };
                let name = if name.is_empty() { "My scene" } else { name }.to_string();
                let replace = self.presets[..self.mine]
                    .iter()
                    .any(|e| bpe_scene::saved_file_name(&e.name) == bpe_scene::saved_file_name(&name));
                let label = if replace { "Replace" } else { "Save" };
                if ui.button(label).on_hover_text("Keep this scene in My scenes, to start new scenes from it").clicked()
                {
                    self.message = Some(match crate::my_presets::save_scene(&name, current, base_dir) {
                        Ok(()) => format!("Saved \"{name}\" to My scenes."),
                        Err(error) => format!("Couldn't save it: {error}"),
                    });
                    self.save_name.clear();
                    self.reload();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Keep my current scene").clicked() {
                        self.open = false;
                    }
                });
            });
            if let Some(message) = &self.message {
                ui.label(RichText::new(message).small().color(theme::MUTED));
            }
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

    /// Cards for presets in `range`, three to a row.
    fn cards(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        range: std::ops::Range<usize>,
        picked: &mut Option<Scene>,
        delete: &mut Option<usize>,
    ) {
        egui::Grid::new(id).spacing([14.0, 14.0]).show(ui, |ui| {
            for (k, i) in range.enumerate() {
                let preset = &self.presets[i];
                let texture = self.thumbs[i].as_ref().map(|t| t.texture);
                let response = card(ui, preset, texture);
                self.visible[i] = ui.is_rect_visible(response.rect);
                if response.clicked() {
                    *picked = Some(preset.scene.clone());
                }
                if preset.saved.is_some() {
                    response.context_menu(|ui| {
                        if ui.button("Delete this preset").clicked() {
                            *delete = Some(i);
                            ui.close();
                        }
                    });
                }
                if k % 3 == 2 {
                    ui.end_row();
                }
            }
        });
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

fn card(ui: &mut egui::Ui, preset: &Entry, texture: Option<egui::TextureId>) -> egui::Response {
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
                ui.label(RichText::new(&preset.name).strong().color(theme::TEXT));
                ui.add(egui::Label::new(RichText::new(&preset.description).small().color(theme::MUTED)).wrap());
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
    let hover = if preset.saved.is_some() {
        "Start a new scene from this preset. Right-click to delete it."
    } else {
        "Start a new scene from this preset"
    };
    response.on_hover_text(hover)
}
