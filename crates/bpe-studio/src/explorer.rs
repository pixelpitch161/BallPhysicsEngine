//! The seed explorer: the scene beside nine variations of it, each drawn small and scored in
//! the background, with "Use this", "More like this" and thumbs up or down to teach the scorer.

use crate::app::Studio;
use crate::settings;
use crate::theme;
use crate::thumbs::{self, Thumb, Thumbnailer};
use bpe_core::{Resolved, Scene, Settings, ThingId};
use bpe_gfx::{Frame, build_frame_at};
use bpe_kit::random::{Roll, START_AMOUNT, Scope, fresh_seed, roll};
use bpe_kit::score::{Parts, Sampled, Score, learn, sample, score};
use bpe_kit::{Kit, SceneSettings, resolve_scene};
use bpe_runtime::Playback;
use eframe::egui::{self, Color32, RichText, Ui};
use eframe::egui_wgpu::RenderState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// The original and nine variations.
pub const CELLS: usize = 10;
/// Every thumbnail plays this much of its scene, then starts over.
pub const LOOP: f64 = 10.0;
/// "More like this" rolls at most this far from the chosen variation.
pub const NARROW: f32 = 0.3;
/// How long each thumbnail's playback may work per frame before it is drawn where it got to.
const STEP_BUDGET: Duration = Duration::from_millis(3);
const THUMB_HEIGHT: f32 = 160.0;
const CELL_WIDTH: f32 = 170.0;
const PER_ROW: usize = 5;

/// One picture in the explorer: the original or a variation of it.
pub struct Cell {
    /// The seed that made it; 0 for the original.
    pub seed: u32,
    pub scene: Scene,
    resolved: Resolved,
    /// Its score under the current weights, once the background run has sampled it.
    pub score: Option<Score>,
    sampled: Option<Sampled>,
    /// A thumbs up (`Some(true)`) or down from the user.
    pub rated: Option<bool>,
    /// Its own run, stepped a little each frame for the moving thumbnail.
    playback: Option<Playback>,
}

impl Cell {
    fn new(seed: u32, scene: Scene, kit: &Kit) -> Self {
        let resolved = resolve_scene(&scene, &kit.registry);
        Cell { seed, scene, resolved, score: None, sampled: None, rated: None, playback: None }
    }

    fn name(&self, index: usize) -> String {
        if index == 0 { "Original".to_owned() } else { format!("Variation {index}") }
    }

    fn duration(&self) -> f64 {
        self.resolved
            .things
            .get(&ThingId::ROOT)
            .map(|r| SceneSettings::from_params(&r.params).duration.max(0.0) as f64)
            .unwrap_or(LOOP)
    }
}

/// What the explorer asked the editor for this frame.
pub enum Outcome {
    Nothing,
    /// "Make a batch of these…": render many seeds around `base`, each `amount` away.
    Batch {
        base: Box<Scene>,
        amount: f32,
    },
}

/// Clicks inside the window, applied after it is drawn.
enum Request {
    Reroll,
    Use(usize),
    MoreLike(usize),
    Rate(usize, bool),
    Batch,
}

pub struct Explorer {
    pub open: bool,
    /// The scene the variations are rolled from.
    pub base: Scene,
    /// The seed of the current set; variation `i` uses `seed * 16 + i`.
    pub seed: u32,
    /// How far the variations go from the base, 0 to 1.
    pub amount: f32,
    pub cells: Vec<Cell>,
    /// What the scorer values, taught by thumbs up and down.
    pub weights: Parts,
    /// Save the weights to the settings folder when they change.
    pub persist: bool,
    /// Counts the sets rolled, so a slow run from an earlier set can't score the current one.
    generation: u64,
    scores: Option<mpsc::Receiver<(u64, usize, Sampled)>>,
    /// Tells the current set's scoring thread to give up.
    stop: Arc<AtomicBool>,
    /// When the thumbnails' loop started.
    started: Instant,
    thumbnailer: Option<Thumbnailer>,
    /// One texture per cell, kept for the editor's lifetime.
    thumbs: Vec<Thumb>,
    frame: Frame,
}

impl Default for Explorer {
    fn default() -> Self {
        Self {
            open: false,
            base: Scene::default(),
            seed: 0,
            amount: START_AMOUNT,
            cells: Vec::new(),
            weights: Parts::WEIGHTS,
            persist: false,
            generation: 0,
            scores: None,
            stop: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            thumbnailer: None,
            thumbs: Vec::new(),
            frame: Frame::default(),
        }
    }
}

impl Explorer {
    /// Opens the window on `scene` with nine fresh variations.
    pub fn open_from(&mut self, scene: &Scene, kit: &Kit) {
        self.base = scene.clone();
        self.amount = START_AMOUNT;
        self.reroll_with(fresh_seed(), kit);
        self.open = true;
    }

    /// Rolls the nine variations of the base from `seed` and starts scoring all ten.
    pub fn reroll_with(&mut self, seed: u32, kit: &Kit) {
        self.seed = seed;
        self.started = Instant::now();
        let mut cells = vec![Cell::new(0, self.base.clone(), kit)];
        for i in 1..CELLS as u32 {
            let mut scene = self.base.clone();
            let seed = seed.wrapping_mul(16).wrapping_add(i).max(1);
            roll(&mut scene, &kit.registry, &Scope::Scene, Roll { seed, amount: self.amount });
            cells.push(Cell::new(seed, scene, kit));
        }
        self.cells = cells;
        self.start_scoring(kit);
    }

    /// Makes `index` the new original and rolls closer variations of it.
    pub fn more_like(&mut self, index: usize, kit: &Kit) {
        self.base = self.cells[index].scene.clone();
        self.amount = self.amount.min(NARROW);
        self.reroll_with(fresh_seed(), kit);
    }

    /// A thumbs up or down on `index`: the weights learn from its parts and every score is
    /// worked out again under them.
    pub fn rate(&mut self, index: usize, like: bool) {
        let Some(parts) = self.cells[index].score.map(|s| s.parts) else {
            return;
        };
        learn(&mut self.weights, &parts, like);
        self.cells[index].rated = Some(like);
        self.rescore();
        if self.persist {
            settings::save_weights(&self.weights);
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.stop.store(true, Ordering::Relaxed);
        self.scores = None;
        for cell in &mut self.cells {
            cell.playback = None;
        }
    }

    /// Whether any cell is still being scored.
    pub fn scoring(&self) -> bool {
        self.scores.is_some() && self.cells.iter().any(|c| c.score.is_none())
    }

    fn rescore(&mut self) {
        for cell in &mut self.cells {
            if let Some(sampled) = &cell.sampled {
                cell.score = Some(score(sampled, &self.weights));
            }
        }
    }

    /// One thread samples every cell of this set in turn; an older set's thread is told to stop.
    fn start_scoring(&mut self, kit: &Kit) {
        self.stop.store(true, Ordering::Relaxed);
        self.stop = Arc::new(AtomicBool::new(false));
        self.generation += 1;
        let generation = self.generation;
        let stop = self.stop.clone();
        let sims = kit.sims.clone();
        let jobs: Vec<(usize, Scene, Resolved)> =
            self.cells.iter().enumerate().map(|(i, c)| (i, c.scene.clone(), c.resolved.clone())).collect();
        let (tx, rx) = mpsc::channel();
        self.scores = Some(rx);
        let spawned = std::thread::Builder::new().name("explorer scoring".into()).spawn(move || {
            for (i, scene, resolved) in jobs {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let Some(sampled) = sample(&scene, &resolved, &sims, &stop) else {
                    return;
                };
                if tx.send((generation, i, sampled)).is_err() {
                    return;
                }
            }
        });
        if spawned.is_err() {
            self.scores = None;
        }
    }

    /// Takes the scores that landed since last frame.
    fn receive(&mut self) {
        let Some(rx) = &self.scores else {
            return;
        };
        let mut landed = Vec::new();
        while let Ok(item) = rx.try_recv() {
            landed.push(item);
        }
        for (generation, index, sampled) in landed {
            if generation == self.generation
                && let Some(cell) = self.cells.get_mut(index)
            {
                cell.score = Some(score(&sampled, &self.weights));
                cell.sampled = Some(sampled);
            }
        }
    }

    /// Steps cell `index`'s run towards the loop time and draws it into its thumbnail.
    fn draw_thumb(&mut self, index: usize, state: &RenderState, kit: &Kit) {
        let thumbnailer = self.thumbnailer.get_or_insert_with(|| Thumbnailer::new(&state.device));
        while self.thumbs.len() <= index {
            self.thumbs.push(Thumb::new(state));
        }
        let cell = &mut self.cells[index];
        let span = LOOP.min(cell.duration()).max(1.0 / 60.0);
        let time = self.started.elapsed().as_secs_f64() % span;
        let playback =
            cell.playback.get_or_insert_with(|| Playback::new(&cell.scene, &cell.resolved, kit.sims.clone()));
        playback.seek_within(time, Some(STEP_BUDGET));
        let shown = playback.time();
        build_frame_at(
            &cell.scene,
            &cell.resolved,
            playback.world(),
            playback.log(),
            shown,
            shown,
            &kit.draws,
            &mut self.frame,
        );
        thumbnailer.draw(&state.device, &state.queue, &self.frame, self.thumbs[index].texture());
    }
}

/// Draws the explorer when it is open and applies what was clicked: "Use this" becomes one undo
/// step in the editor, "Make a batch of these…" comes back as [`Outcome::Batch`].
pub fn show(ctx: &egui::Context, studio: &mut Studio, state: Option<&RenderState>) -> Outcome {
    if !studio.panels.explorer.open {
        return Outcome::Nothing;
    }
    let kit = studio.kit.clone();
    let mut request = None;
    let mut open = true;
    {
        let ex = &mut studio.panels.explorer;
        ex.receive();
        if ex.scoring() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if state.is_some() {
            ctx.request_repaint();
        }
        egui::Window::new("Explore")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_pos(egui::pos2(12.0, theme::BELOW_TOOLBAR))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Roll again").on_hover_text("Nine new variations of the original").clicked() {
                        request = Some(Request::Reroll);
                    }
                    ui.separator();
                    let amount = ui.label(RichText::new("How much").strong());
                    ui.add(egui::Slider::new(&mut ex.amount, 0.0..=1.0).show_value(false))
                        .labelled_by(amount.id)
                        .on_hover_text("How far the next roll goes from the original; shapes and arrangements only change near the right end");
                    ui.separator();
                    if ui
                        .button("Make a batch of these…")
                        .on_hover_text("Render many seeds like these and keep the best")
                        .clicked()
                    {
                        request = Some(Request::Batch);
                    }
                });
                ui.label(
                    RichText::new(
                        "Thumbs up or down teach the score what you like. Every roll starts from the original.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
                ui.add_space(6.0);
                for row in 0..CELLS.div_ceil(PER_ROW) {
                    ui.horizontal_top(|ui| {
                        for index in row * PER_ROW..((row + 1) * PER_ROW).min(ex.cells.len()) {
                            if let Some(state) = state {
                                ex.draw_thumb(index, state, &kit);
                            }
                            cell_ui(ui, ex, index, state.is_some(), &mut request);
                        }
                    });
                }
            });
    }
    if !open {
        studio.panels.explorer.close();
    }
    match request {
        Some(Request::Reroll) => studio.panels.explorer.reroll_with(fresh_seed(), &kit),
        Some(Request::MoreLike(index)) => studio.panels.explorer.more_like(index, &kit),
        Some(Request::Rate(index, like)) => studio.panels.explorer.rate(index, like),
        Some(Request::Use(index)) => {
            let chosen = studio.panels.explorer.cells[index].scene.clone();
            let name = studio.panels.explorer.cells[index].name(index);
            let before = std::mem::replace(&mut studio.scene, chosen);
            studio.after_separate_edit(before);
            studio.recorded = true;
            studio.status = if index == 0 { "Kept the original.".to_owned() } else { format!("Took {name}.") };
            studio.panels.explorer.close();
        }
        Some(Request::Batch) => {
            let ex = &studio.panels.explorer;
            return Outcome::Batch { base: Box::new(ex.base.clone()), amount: ex.amount };
        }
        None => {}
    }
    Outcome::Nothing
}

fn cell_ui(ui: &mut Ui, ex: &Explorer, index: usize, has_picture: bool, request: &mut Option<Request>) {
    let cell = &ex.cells[index];
    ui.vertical(|ui| {
        ui.set_width(CELL_WIDTH);
        let size = thumbs::shown_size(THUMB_HEIGHT);
        let text = if has_picture { "…" } else { "No picture" };
        thumbs::show(ui, ex.thumbs.get(index), size, text);
        ui.horizontal(|ui| {
            ui.label(RichText::new(cell.name(index)).strong());
            if cell.seed != 0 {
                ui.label(RichText::new(format!("seed {}", cell.seed)).small().color(theme::MUTED));
            }
        });
        match cell.score {
            Some(score) => {
                let badge = RichText::new(format!("Score {:.0}", score.total * 100.0)).color(score_colour(score.total));
                ui.label(badge).on_hover_ui(|ui| {
                    for (name, value, meaning) in score.parts.list() {
                        ui.label(format!("{name}: {:.0}  {meaning}", value * 100.0));
                    }
                });
            }
            None => _ = ui.label(RichText::new("Scoring…").color(theme::MUTED)),
        }
        ui.horizontal(|ui| {
            if ui.button("Use this").on_hover_text("Make this the scene (one Undo step)").clicked() {
                *request = Some(Request::Use(index));
            }
            if ui.button("More like this").on_hover_text("Make this the original and roll closer variations").clicked()
            {
                *request = Some(Request::MoreLike(index));
            }
        });
        ui.horizontal(|ui| {
            let scored = cell.score.is_some();
            ui.add_enabled_ui(scored, |ui| {
                if ui
                    .selectable_label(cell.rated == Some(true), "Like")
                    .on_hover_text("Teach the score: more like this")
                    .clicked()
                {
                    *request = Some(Request::Rate(index, true));
                }
                if ui
                    .selectable_label(cell.rated == Some(false), "Nope")
                    .on_hover_text("Teach the score: less like this")
                    .clicked()
                {
                    *request = Some(Request::Rate(index, false));
                }
            });
        });
    });
}

fn score_colour(total: f32) -> Color32 {
    if total >= 0.6 {
        theme::ACCENT_TEXT
    } else if total >= 0.3 {
        Color32::from_rgb(230, 190, 90)
    } else {
        theme::MUTED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Studio;
    use egui_kittest::{Harness, kittest::Queryable};

    fn harness() -> Harness<'static, Studio> {
        let mut studio = Studio::headless();
        let scene = studio.scene.clone();
        let kit = studio.kit.clone();
        studio.panels.explorer.open_from(&scene, &kit);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1100.0, 800.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(30)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(|ui, s: &mut Studio| _ = show(ui.ctx(), s, None), studio);
        h.run();
        h
    }

    #[test]
    fn the_explorer_shows_the_original_and_nine_variations() {
        let mut h = harness();
        assert!(h.query_by_label("Original").is_some());
        for i in 1..=9 {
            assert!(h.query_by_label(&format!("Variation {i}")).is_some(), "Variation {i}");
        }
        let ex = &h.state().panels.explorer;
        assert_eq!(ex.cells.len(), 10);
        assert_eq!(ex.cells[0].scene, ex.base);
        for cell in &ex.cells[1..] {
            assert_ne!(cell.scene, ex.base);
        }
        let seed = ex.seed;
        let scenes: Vec<Scene> = ex.cells.iter().map(|c| c.scene.clone()).collect();
        let kit = h.state().kit.clone();
        h.state_mut().panels.explorer.reroll_with(seed, &kit);
        h.run();
        let again: Vec<Scene> = h.state().panels.explorer.cells.iter().map(|c| c.scene.clone()).collect();
        assert_eq!(again, scenes, "the same seed gives the same variations");
        h.get_by_label("Roll again").click();
        h.run();
        let fresh: Vec<Scene> = h.state().panels.explorer.cells.iter().map(|c| c.scene.clone()).collect();
        assert_ne!(fresh[1..], scenes[1..]);
        assert_eq!(fresh[0], scenes[0], "the original stays");
    }

    #[test]
    fn use_this_hands_back_that_variation_as_one_undo_step() {
        let mut h = harness();
        let original = h.state().scene.clone();
        let chosen = h.state().panels.explorer.cells[1].scene.clone();
        h.get_all_by_label("Use this").nth(1).expect("the first variation's button").click();
        h.run();
        let s = h.state_mut();
        assert_eq!(s.scene, chosen);
        assert!(!s.panels.explorer.open, "the explorer closes once a variation is taken");
        assert!(s.undo());
        assert_eq!(s.scene, original);
    }

    #[test]
    fn the_explorer_opens_below_the_toolbar() {
        let h = harness();
        let title = h.get_by_label("Explore").rect();
        assert!(title.top() >= crate::theme::BELOW_TOOLBAR, "the window covers the toolbar: {title:?}");
    }

    #[test]
    fn more_like_this_narrows_the_amount_and_rebases() {
        let mut h = harness();
        assert_eq!(h.state().panels.explorer.amount, 0.5, "the explorer starts half way");
        let liked = h.state().panels.explorer.cells[2].scene.clone();
        h.get_all_by_label("More like this").nth(2).expect("the second variation's button").click();
        h.run();
        let ex = &h.state().panels.explorer;
        assert_eq!(ex.amount, 0.3);
        assert_eq!(ex.base, liked);
        assert_eq!(ex.cells[0].scene, liked, "it becomes the original");
        assert!(ex.open);
    }

    #[test]
    fn scores_arrive_in_the_background_and_thumbs_teach_the_weights() {
        let mut h = harness();
        let mut scored = false;
        for _ in 0..400 {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(25));
            if h.state().panels.explorer.cells.iter().all(|c| c.score.is_some()) {
                scored = true;
                break;
            }
        }
        assert!(scored, "every cell gets a score within ten seconds");
        assert_eq!(h.query_all_by_label_contains("Score ").count(), 10, "a score badge on every cell");
        let before = h.state().panels.explorer.weights;
        h.get_all_by_label("Like").nth(1).expect("the first variation's thumbs up").click();
        h.run();
        let ex = &h.state().panels.explorer;
        assert_ne!(ex.weights, before, "a thumbs up moves the weights");
        assert_eq!(ex.cells[1].rated, Some(true));
        assert!(ex.cells.iter().all(|c| c.score.is_some()), "scores are re-totalled, not thrown away");
    }
}
