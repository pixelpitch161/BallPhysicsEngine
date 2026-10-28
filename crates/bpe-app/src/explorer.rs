//! The seed explorer: moving previews of random variations of the scene, side by side, so the
//! user can pick the one they like best. Each variation comes from a numbered seed, so the same
//! starting scene, settings and seed always make the same variation. Each preview gets a score
//! from simulating it, and thumbs up or down teach the scorer what the user likes.

use crate::inspector::paint_dice;
use crate::theme;
use bpe_export::batch::{self, Vary};
use bpe_export::score::{self, Score, ScoreInputs, Weights};
use bpe_render::{EffectLibrary, FrameData, Renderer};
use bpe_scene::{Dice, Scene};
use bpe_sim::Playback;
use eframe::egui::{self, Color32, RichText, Sense};
use std::sync::{Arc, Mutex};
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
    /// Scores of the cells, filled in by a background thread after each rebuild.
    scores: Arc<Mutex<Scores>>,
    /// How the scorer weighs each part, learned from thumbs up and down.
    weights: Weights,
    /// Thumbs given in this batch, by seed, so each can only be given once.
    thumbs_given: std::collections::HashMap<u64, bool>,
    /// Set when the user asks for a batch of videos like the current variations.
    pub make_batch: Option<BatchRequest>,
}

/// Everything the batch factory needs to carry on from the explorer.
pub struct BatchRequest {
    pub base: Scene,
    pub vary: Vary,
    pub amount: f32,
    pub seed: u64,
}

#[derive(Default)]
struct Scores {
    /// Which rebuild these scores are for; a newer rebuild makes older results stale.
    generation: u64,
    scores: Vec<Option<Score>>,
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
            vary: Vary { balls: true, arena: true, bounces: true, look: true, ..Vary::default() },
            amount: 0.6,
            seed: 1,
            library: EffectLibrary::new(),
            cells: Vec::new(),
            thumbs: Vec::new(),
            stale: true,
            started: Instant::now(),
            scores: Arc::default(),
            weights: Weights::default(),
            thumbs_given: Default::default(),
            make_batch: None,
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
        self.weights = batch::load_weights();
    }

    /// The variation made from `seed` with the current settings: the same scene a batch with
    /// these settings makes for that seed.
    fn variation(&self, seed: u64) -> Scene {
        batch::variation(&self.base, &self.vary, seed, self.amount, &self.library, &[]).0
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
        self.thumbs_given.clear();
        self.score_in_background();
    }

    /// Simulates and scores the cells on a few threads, so the badges fill in while the previews
    /// play. A newer rebuild makes the threads stop after the cell they are on.
    fn score_in_background(&self) {
        let generation = {
            let mut scores = self.scores.lock().unwrap();
            scores.generation += 1;
            scores.scores = vec![None; self.cells.len()];
            scores.generation
        };
        let scenes: Arc<Vec<Scene>> = Arc::new(self.cells.iter().map(|c| c.scene.clone()).collect());
        let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, scenes.len().max(1));
        for _ in 0..threads {
            let (scenes, next, shared, weights) = (scenes.clone(), next.clone(), self.scores.clone(), self.weights);
            std::thread::spawn(move || {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(scene) = scenes.get(index) else { return };
                    if shared.lock().unwrap().generation != generation {
                        return;
                    }
                    let stats = score::simulate(scene);
                    let score = weights.score(score::parts(scene, &stats, &ScoreInputs::default()));
                    let mut scores = shared.lock().unwrap();
                    if scores.generation != generation {
                        return;
                    }
                    scores.scores[index] = Some(score);
                }
            });
        }
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
        let mut thumb = None;
        // Totals use the latest weights, so a Like or Nope changes every badge straight away.
        let scores: Vec<Option<Score>> =
            self.scores.lock().unwrap().scores.iter().map(|s| s.map(|s| self.weights.score(s.parts))).collect();
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
                        let score = scores.get(i).copied().flatten();
                        let given = cell.seed.and_then(|seed| self.thumbs_given.get(&seed).copied());
                        match card(ui, cell, texture, starting, score, given) {
                            Some(CardChoice::Use) => {
                                picked = cell.seed.map(|seed| Picked { scene: cell.scene.clone(), seed })
                            }
                            Some(CardChoice::Closer) => closer = Some(i),
                            Some(CardChoice::Thumb(liked)) => thumb = Some((i, liked)),
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
                    let enabled = self.vary.any();
                    if ui
                        .add_enabled(enabled, egui::Button::new("Make a batch of these…"))
                        .on_hover_text(
                            "Render many videos with these settings, overnight if you like, and keep the best \
                             ones. Opens the batch factory.",
                        )
                        .clicked()
                    {
                        self.make_batch = Some(BatchRequest {
                            base: self.base.clone(),
                            vary: self.vary,
                            amount: self.amount,
                            seed: self.seed,
                        });
                    }
                    if let Some(best) = best_seed(&self.cells, &scores) {
                        ui.label(RichText::new(format!("Top score: seed {best}")).color(theme::MUTED)).on_hover_text(
                            "The badge on each preview is how satisfying the scorer thinks it is, from 0 to 100",
                        );
                    }
                });
            });
        });
        if let Some((i, liked)) = thumb
            && let (Some(seed), Some(score)) = (self.cells[i].seed, scores.get(i).copied().flatten())
        {
            crate::batch::learn(&mut self.weights, &score.parts, liked);
            self.thumbs_given.insert(seed, liked);
        }
        if let Some(i) = closer {
            self.base = self.cells[i].scene.clone();
            self.amount = self.amount.min(CLOSER);
            self.seed = fresh_seed();
            self.stale = true;
        }
        if modal.should_close() || picked.is_some() || self.make_batch.is_some() {
            self.close(frame);
        }
        ctx.request_repaint();
        picked
    }

    fn close(&mut self, frame: &eframe::Frame) {
        self.open = false;
        self.cells.clear();
        // Stops the scoring threads after the cell they are on.
        self.scores.lock().unwrap().generation += 1;
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
            ui.checkbox(&mut vary.instrument, "Instrument")
                .on_hover_text("How the instrument sounds: how long its notes ring and how bright they are");
            let has_captions = !self.base.captions.is_empty();
            ui.checkbox(&mut vary.captions, "Captions").on_hover_text(if has_captions {
                "The captions' words, colours and animations"
            } else {
                "Adds a hook caption such as \"Can it escape?\" to some variations"
            });
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

/// The seed of the best-scoring variation, once every variation has a score.
fn best_seed(cells: &[Cell], scores: &[Option<Score>]) -> Option<u64> {
    if cells.len() < 2 || scores.len() != cells.len() || scores.iter().any(Option::is_none) {
        return None;
    }
    cells
        .iter()
        .zip(scores)
        .filter_map(|(cell, score)| Some((cell.seed?, score.as_ref()?.total)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(seed, _)| seed)
}

enum CardChoice {
    Use,
    Closer,
    /// Thumbs up (true) or down.
    Thumb(bool),
}

/// Paints a score badge, 0 to 100, in the top right corner of `rect`. Hovering it lists the parts.
pub fn score_badge(ui: &egui::Ui, rect: egui::Rect, score: Option<Score>, id: egui::Id) {
    let size = egui::vec2(38.0, 22.0);
    let badge = egui::Rect::from_min_size(rect.right_top() + egui::vec2(-size.x - 6.0, 6.0), size);
    let painter = ui.painter();
    let (text, fill) = match score {
        Some(score) => {
            let value = (score.total * 100.0).round();
            let fill = if value >= 70.0 {
                Color32::from_rgb(40, 150, 90)
            } else if value >= 45.0 {
                Color32::from_rgb(170, 130, 40)
            } else {
                Color32::from_rgb(160, 60, 60)
            };
            (format!("{value:.0}"), fill)
        }
        None => ("···".to_string(), Color32::from_black_alpha(160)),
    };
    painter.rect_filled(badge, 11.0, fill);
    painter.text(badge.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(13.0), Color32::WHITE);
    let response = ui.interact(badge, id, Sense::hover());
    response.on_hover_ui(|ui| match score {
        Some(score) => {
            ui.label(RichText::new(format!("Score {:.0} of 100", score.total * 100.0)).strong());
            for (name, value, meaning) in score.parts.list() {
                if let Some(value) = value {
                    ui.label(format!("{name}: {:.0}  ({meaning})", value * 100.0));
                }
            }
            ui.label(RichText::new("Like and Nope teach the scorer what you like.").small().color(theme::MUTED));
        }
        None => _ = ui.label("Scoring…"),
    });
}

fn card(
    ui: &mut egui::Ui,
    cell: &Cell,
    texture: Option<egui::TextureId>,
    starting: &str,
    score: Option<Score>,
    given: Option<bool>,
) -> Option<CardChoice> {
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
            score_badge(ui, image.rect, score, image.id.with("score"));
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
                    // Like and Nope sit on the preview's bottom corners once it has a score.
                    if score.is_some() {
                        let corner = image.rect.left_bottom() + egui::vec2(6.0, -26.0);
                        let up = egui::Rect::from_min_size(corner, egui::vec2(46.0, 20.0));
                        let down = up.translate(egui::vec2(image.rect.width() - 12.0 - 46.0, 0.0));
                        let enabled = given.is_none();
                        let like = egui::Button::new(RichText::new("Like").small())
                            .wrap_mode(egui::TextWrapMode::Extend)
                            .selected(given == Some(true));
                        if ui.put(up, like).on_hover_text("Score ones like this higher").clicked() && enabled {
                            choice = Some(CardChoice::Thumb(true));
                        }
                        let nope = egui::Button::new(RichText::new("Nope").small())
                            .wrap_mode(egui::TextWrapMode::Extend)
                            .selected(given == Some(false));
                        if ui.put(down, nope).on_hover_text("Score ones like this lower").clicked() && enabled {
                            choice = Some(CardChoice::Thumb(false));
                        }
                    }
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
