//! The seed explorer: moving previews of random variations of the scene, side by side, so the
//! user can pick the one they like best. Each variation comes from a numbered seed, so the same
//! starting scene, settings and seed always make the same variation.

use crate::inspector::{effects::roll_effect, paint_dice};
use crate::theme;
use bpe_render::{EffectLibrary, FrameData, Renderer};
use bpe_scene::{Dice, Group, Scene};
use bpe_sim::Playback;
use eframe::egui::{self, Color32, RichText, Sense};
use std::time::Instant;

/// Thumbnail size in screen points.
const THUMB: egui::Vec2 = egui::vec2(135.0, 240.0);
/// Card width in screen points, wide enough for the buttons under the thumbnail.
const CARD: f32 = 190.0;
const MOST_COLUMNS: usize = 5;
/// The starting scene plus this many variations.
pub const VARIATIONS: usize = 9;
/// Each thumbnail loops over the first this many seconds of its scene.
const LOOP_SECONDS: f64 = 10.0;
/// Most time one preview spends catching its simulation up in an editor frame.
const THUMB_BUDGET: std::time::Duration = std::time::Duration::from_millis(3);
/// "More like this" keeps changes at most this big, so the new batch stays close to the pick.
const CLOSER: f32 = 0.3;

/// Which parts of the scene the variations change.
#[derive(Clone, Copy, PartialEq)]
struct Vary {
    balls: bool,
    arena: bool,
    bounces: bool,
    look: bool,
    effects: bool,
    sound: bool,
}

impl Vary {
    fn groups(self, scene: &Scene) -> Vec<Group> {
        let mut groups = Vec::new();
        if self.balls {
            groups.extend((0..scene.spawners.len()).map(Group::Balls));
        }
        if self.arena {
            groups.extend([Group::Shape, Group::Movement, Group::World]);
        }
        if self.bounces {
            groups.extend([Group::Bounces, Group::NewBalls]);
        }
        if self.look {
            groups.extend([Group::Background, Group::Trails, Group::Glow, Group::Strings]);
        }
        if self.sound {
            groups.extend([Group::Chords, Group::Notes]);
        }
        groups
    }

    fn any(self) -> bool {
        self.balls || self.arena || self.bounces || self.look || self.effects || self.sound
    }
}

pub struct Explorer {
    pub open: bool,
    /// The scene the explorer was opened on.
    original: Scene,
    /// The scene the variations are made from: the original, or a pick from "More like this".
    base: Scene,
    vary: Vary,
    /// How different the variations are, from 0 (the same) to 1 (anything goes).
    amount: f32,
    /// Seed of the first variation; the others follow on from it.
    seed: u64,
    library: EffectLibrary,
    /// The base first, then the variations.
    cells: Vec<Cell>,
    thumbs: Vec<Thumb>,
    /// The variations need making again because a setting changed.
    stale: bool,
    started: Instant,
}

struct Cell {
    scene: Scene,
    seed: Option<u64>,
}

struct Thumb {
    renderer: Renderer,
    texture: egui::TextureId,
    playback: Playback,
    frame: FrameData,
}

/// What the user chose in the explorer.
pub struct Picked {
    pub scene: Scene,
    pub seed: u64,
}

impl Default for Explorer {
    fn default() -> Self {
        Self {
            open: false,
            original: Scene::default(),
            base: Scene::default(),
            vary: Vary { balls: true, arena: true, bounces: true, look: true, effects: false, sound: false },
            amount: 0.6,
            seed: 1,
            library: EffectLibrary::new(),
            cells: Vec::new(),
            thumbs: Vec::new(),
            stale: true,
            started: Instant::now(),
        }
    }
}

impl Explorer {
    /// Opens the explorer on `scene` with a fresh batch of seeds.
    pub fn open(&mut self, scene: &Scene, library: &EffectLibrary) {
        self.open = true;
        self.original = scene.clone();
        self.base = scene.clone();
        self.library = library.clone();
        self.seed = fresh_seed();
        self.stale = true;
    }

    /// The variation made from `seed` with the current settings.
    fn variation(&self, seed: u64) -> Scene {
        let mut scene = self.base.variation(&self.vary.groups(&self.base), seed, self.amount);
        if self.vary.effects {
            for (index, effect) in scene.effects.iter_mut().enumerate() {
                roll_effect(effect, &self.library, &mut Dice::mixed(seed, 1000 + index as u64), self.amount);
            }
        }
        scene
    }

    fn rebuild(&mut self) {
        let mut cells = vec![Cell { scene: self.base.clone(), seed: None }];
        for i in 0..VARIATIONS as u64 {
            let seed = self.seed.wrapping_add(i);
            cells.push(Cell { scene: self.variation(seed), seed: Some(seed) });
        }
        self.cells = cells;
        for (thumb, cell) in self.thumbs.iter_mut().zip(&self.cells) {
            thumb.playback = Playback::new(&cell.scene);
            thumb.renderer.reset_effects();
        }
        self.stale = false;
        self.started = Instant::now();
    }

    /// Shows the explorer if it is open. Returns the variation the user picked.
    pub fn show(&mut self, ctx: &egui::Context, frame: &eframe::Frame) -> Option<Picked> {
        if !self.open {
            return None;
        }
        if self.stale {
            self.rebuild();
        }
        self.render_thumbnails(ctx, frame);
        let mut picked = None;
        let mut closer = None;
        let modal = egui::Modal::new(egui::Id::unique("seed-explorer")).show(ctx, |ui| {
            // Five previews a row, or fewer in a small window.
            let room = ctx.content_rect().width() - 80.0;
            let columns = ((room / (CARD + 26.0)) as usize).clamp(2, MOST_COLUMNS);
            ui.set_width(columns as f32 * (CARD + 26.0));
            ui.heading("Explore variations");
            ui.label(
                RichText::new(
                    "Each preview is your scene with some settings rolled at random. Click one to use it, \
                     or pick \"More like this\" to explore around it. Ctrl+Z puts your scene back.",
                )
                .color(theme::MUTED),
            );
            ui.add_space(8.0);
            self.controls(ui);
            ui.add_space(10.0);
            let max_height = (ctx.content_rect().height() - 300.0).max(THUMB.y + 90.0);
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
                egui::Grid::new("variations").spacing([10.0, 12.0]).show(ui, |ui| {
                    for (i, cell) in self.cells.iter().enumerate() {
                        let texture = self.thumbs.get(i).map(|t| t.texture);
                        let starting = if self.base == self.original { "Your scene now" } else { "Starting point" };
                        match card(ui, cell, texture, starting) {
                            Some(CardChoice::Use) => {
                                picked = cell.seed.map(|seed| Picked { scene: cell.scene.clone(), seed })
                            }
                            Some(CardChoice::Closer) => closer = Some(i),
                            None => {}
                        }
                        if i % columns == columns - 1 {
                            ui.end_row();
                        }
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if self.base != self.original
                    && ui
                        .button("Back to my scene")
                        .on_hover_text("Make variations of the scene you started with again")
                        .clicked()
                {
                    self.base = self.original.clone();
                    self.stale = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").on_hover_text("Keep your scene as it is").clicked() {
                        self.open = false;
                    }
                });
            });
        });
        if let Some(i) = closer {
            self.base = self.cells[i].scene.clone();
            self.amount = self.amount.min(CLOSER);
            self.seed = fresh_seed();
            self.stale = true;
        }
        if modal.should_close() || picked.is_some() {
            self.close(frame);
        }
        ctx.request_repaint();
        picked
    }

    fn close(&mut self, frame: &eframe::Frame) {
        self.open = false;
        self.cells.clear();
        // Frees the previews' graphics memory until the explorer opens again.
        self.close_thumbs(frame);
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        let before = (self.vary, self.amount, self.seed);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Change").color(theme::MUTED));
            let vary = &mut self.vary;
            ui.checkbox(&mut vary.balls, "Balls")
                .on_hover_text("Where the balls start, how many, their size, speed and colours");
            ui.checkbox(&mut vary.arena, "Arena")
                .on_hover_text("The arena's size, line, spin and shrinking, and gravity");
            ui.checkbox(&mut vary.bounces, "Bounces")
                .on_hover_text("Speeding up, growing, colour changes and new balls on each bounce");
            ui.checkbox(&mut vary.look, "Look").on_hover_text("Background, trails, glow and string art");
            let has_effects = !self.base.effects.is_empty();
            ui.add_enabled(has_effects, egui::Checkbox::new(&mut vary.effects, "Effects"))
                .on_hover_text("The sliders of the effects in the Effects tab")
                .on_disabled_hover_text("Add an effect in the Effects tab to vary it here");
            ui.checkbox(&mut vary.sound, "Sound").on_hover_text(
                "Chords and notes. The previews are silent, so you hear the new sound after you pick one.",
            );
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("How different").color(theme::MUTED));
            ui.spacing_mut().slider_width = 180.0;
            ui.add(egui::Slider::new(&mut self.amount, 0.05..=1.0).show_value(false))
                .on_hover_text("Left keeps the variations close to the starting point; right lets anything change");
            let words = match self.amount {
                a if a < 0.25 => "A little",
                a if a < 0.6 => "Quite a bit",
                a if a < 0.9 => "A lot",
                _ => "Anything goes",
            };
            ui.label(RichText::new(words).color(theme::TEXT));
            ui.add_space(16.0);
            ui.label(RichText::new("Seed").color(theme::MUTED));
            ui.add(egui::DragValue::new(&mut self.seed).range(0..=u32::MAX as u64).speed(1.0)).on_hover_text(
                "The first preview's seed number. The same starting scene, settings and seed always give the same variations.",
            );
            if ui.button("New batch").on_hover_text("Roll nine new variations").clicked() {
                self.seed = self.seed.wrapping_add(VARIATIONS as u64);
            }
        });
        if !self.vary.any() {
            ui.label(RichText::new("Tick at least one thing to change.").color(theme::WARNING));
        }
        if (self.vary, self.amount, self.seed) != before {
            self.stale = true;
        }
    }

    fn render_thumbnails(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(render_state) = frame.wgpu_render_state() else { return };
        let device = &render_state.device;
        let pixels = ctx.pixels_per_point();
        let video = &self.base.video;
        let scale = THUMB.x * pixels / video.width as f32;
        let (width, height) = bpe_export::output_size(&self.base, scale);
        if self.thumbs.first().is_some_and(|t| t.renderer.size() != (width, height)) {
            self.close_thumbs(frame);
        }
        while self.thumbs.len() < self.cells.len() {
            let cell = &self.cells[self.thumbs.len()];
            let mut renderer = Renderer::new(device, width, height);
            renderer.set_effect_library(self.library.clone());
            let texture = render_state.renderer.write().register_native_texture(
                device,
                renderer.output_view(),
                bpe_render::wgpu::FilterMode::Linear,
            );
            self.thumbs.push(Thumb {
                renderer,
                texture,
                playback: Playback::new(&cell.scene),
                frame: FrameData::default(),
            });
        }
        let time = self.started.elapsed().as_secs_f64();
        for (cell, thumb) in self.cells.iter().zip(&mut self.thumbs) {
            let time = time % LOOP_SECONDS.min(cell.scene.video.duration as f64).max(0.5);
            // A heavy variation catches up over a few frames rather than holding up the others.
            if !thumb.playback.seek_within(time, Some(THUMB_BUDGET)) {
                ctx.request_repaint();
            }
            let time = thumb.playback.simulation().time();
            thumb.frame.fill(&cell.scene, &thumb.playback, thumb.renderer.pixel_size(&cell.scene));
            thumb.renderer.render(device, &render_state.queue, &cell.scene, time, &thumb.frame);
        }
    }

    fn close_thumbs(&mut self, frame: &eframe::Frame) {
        if let Some(render_state) = frame.wgpu_render_state() {
            let mut renderer = render_state.renderer.write();
            for thumb in self.thumbs.drain(..) {
                renderer.free_texture(&thumb.texture);
            }
        }
    }
}

/// A seed for a new batch, different every time.
fn fresh_seed() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
    Dice::new(nanos as u64).next_u64() % 100_000
}

enum CardChoice {
    Use,
    Closer,
}

fn card(ui: &mut egui::Ui, cell: &Cell, texture: Option<egui::TextureId>, starting: &str) -> Option<CardChoice> {
    let mut choice = None;
    let inner = egui::Frame::new().fill(theme::CARD).corner_radius(10.0).inner_margin(8.0).show(ui, |ui| {
        ui.set_width(CARD);
        ui.vertical_centered(|ui| {
            let image = match texture {
                Some(texture) => {
                    ui.add(egui::Image::new(egui::load::SizedTexture::new(texture, THUMB)).corner_radius(6.0))
                }
                None => ui.allocate_exact_size(THUMB, Sense::hover()).1,
            };
            let image = image.interact(Sense::click());
            ui.add_space(2.0);
            match cell.seed {
                None => {
                    ui.label(RichText::new(starting).strong().color(theme::TEXT));
                    ui.label(RichText::new("for comparison").small().color(theme::MUTED));
                }
                Some(seed) => {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), Sense::hover());
                        paint_dice(ui.painter(), rect, false);
                        ui.label(RichText::new(format!("Seed {seed}")).strong().color(theme::TEXT));
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        if ui.small_button("Use this").on_hover_text("Use this variation").clicked() {
                            choice = Some(CardChoice::Use);
                        }
                        if ui
                            .small_button("More like this")
                            .on_hover_text("Make new variations close to this one")
                            .clicked()
                        {
                            choice = Some(CardChoice::Closer);
                        }
                    });
                    if image.clicked() {
                        choice = Some(CardChoice::Use);
                    }
                    if image.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        ui.painter().rect_stroke(
                            image.rect,
                            6.0,
                            egui::Stroke::new(2.0, theme::ACCENT),
                            egui::StrokeKind::Outside,
                        );
                    }
                    image.on_hover_text("Click to use this variation");
                }
            }
        });
    });
    if cell.seed.is_none() {
        ui.painter().rect_stroke(
            inner.response.rect,
            10.0,
            egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
            egui::StrokeKind::Inside,
        );
    }
    choice
}
