//! The Start window: a blank scene, the starter scenes with pictures, a scene file, or one of
//! the files opened lately.

use crate::theme;
use crate::thumbs::{self, Thumb, Thumbnailer};
use bpe_core::{Scene, Settings, ThingId};
use bpe_kit::{Kit, SceneSettings, resolve_scene};
use eframe::egui::{self, Color32, RichText, Sense, Ui};
use eframe::egui_wgpu::RenderState;
use std::path::PathBuf;
use std::sync::Arc;

const THUMB_HEIGHT: f32 = 200.0;
const PER_ROW: usize = 5;
/// The latest moment of a starter its picture shows.
const LATEST_STILL: f64 = 4.0;

/// What was chosen in the window.
pub enum Choice {
    Nothing,
    Blank,
    Scene(Box<Scene>),
    Open,
    Recent(PathBuf),
}

#[derive(Default)]
pub struct StartWindow {
    pub open: bool,
    /// The scene files to offer, newest first.
    pub recent: Vec<PathBuf>,
    starters: Vec<(String, Scene)>,
    thumbnailer: Option<Thumbnailer>,
    thumbs: Vec<Thumb>,
}

impl StartWindow {
    pub fn open_with(&mut self, recent: Vec<PathBuf>) {
        self.recent = recent;
        self.open = true;
    }

    /// Draws the next starter's picture, one a frame so the window shows at once.
    fn draw_next(&mut self, state: &RenderState, kit: &Kit) -> bool {
        let index = self.thumbs.len();
        let Some((_, scene)) = self.starters.get(index) else {
            return false;
        };
        let thumbnailer = self.thumbnailer.get_or_insert_with(|| Thumbnailer::new(&state.device));
        let thumb = Thumb::new(state);
        let time = (duration(scene, kit) / 3.0).min(LATEST_STILL);
        thumbnailer.draw_still(&state.device, &state.queue, scene, kit, time, thumb.texture());
        self.thumbs.push(thumb);
        true
    }
}

fn duration(scene: &Scene, kit: &Kit) -> f64 {
    let resolved = resolve_scene(scene, &kit.registry);
    resolved.things.get(&ThingId::ROOT).map_or(3.0, |r| SceneSettings::from_params(&r.params).duration.max(0.0) as f64)
}

/// Draws the Start window when it is open. Choosing anything closes it.
pub fn show(ctx: &egui::Context, win: &mut StartWindow, kit: &Arc<Kit>, state: Option<&RenderState>) -> Choice {
    if !win.open {
        return Choice::Nothing;
    }
    if win.starters.is_empty() {
        win.starters = bpe_kit::scenes::starters(&kit.registry);
    }
    if let Some(state) = state
        && win.draw_next(state, kit)
    {
        ctx.request_repaint();
    }
    let mut choice = Choice::Nothing;
    let mut open = true;
    egui::Window::new("Start")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(
                RichText::new("Start from a blank scene or one of these, and make it your own.").color(theme::MUTED),
            );
            ui.add_space(8.0);
            let cards: Vec<Option<usize>> = std::iter::once(None).chain((0..win.starters.len()).map(Some)).collect();
            for row in cards.chunks(PER_ROW) {
                ui.horizontal_top(|ui| {
                    for card in row {
                        match card {
                            None => blank_card(ui, &mut choice),
                            Some(i) => starter_card(ui, win, *i, state.is_some(), &mut choice),
                        }
                    }
                });
                ui.add_space(6.0);
            }
            ui.separator();
            ui.horizontal_top(|ui| {
                if ui.button("Open a scene file…").on_hover_text("A scene saved with this editor").clicked() {
                    choice = Choice::Open;
                }
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Recent").strong());
                    if win.recent.is_empty() {
                        ui.label(RichText::new("Scenes you open or save show here.").small().color(theme::MUTED));
                    }
                    for path in &win.recent {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if ui.link(name).on_hover_text(path.display().to_string()).clicked() {
                            choice = Choice::Recent(path.clone());
                        }
                    }
                });
            });
        });
    if !open || !matches!(choice, Choice::Nothing) {
        win.open = false;
    }
    choice
}

fn card_size() -> egui::Vec2 {
    thumbs::shown_size(THUMB_HEIGHT)
}

fn blank_card(ui: &mut Ui, choice: &mut Choice) {
    ui.vertical(|ui| {
        ui.set_width(card_size().x);
        let (rect, response) = ui.allocate_exact_size(card_size(), Sense::click());
        let hovered = response.hovered();
        ui.painter().rect_filled(rect, 6.0, theme::BACKGROUND);
        let stroke = egui::Stroke::new(1.5, if hovered { theme::ACCENT_TEXT } else { theme::LINE });
        ui.painter().rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Inside);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "+",
            egui::FontId::proportional(42.0),
            stroke.color,
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Blank scene"));
        if response.on_hover_text("A circle and one ball").clicked() {
            *choice = Choice::Blank;
        }
        // Painted, not a label, so the card is the one thing called "Blank scene".
        let (caption, _) = ui.allocate_exact_size(egui::vec2(card_size().x, 20.0), Sense::hover());
        let font = egui::TextStyle::Body.resolve(ui.style());
        ui.painter().text(caption.left_center(), egui::Align2::LEFT_CENTER, "Blank scene", font, theme::TEXT);
    });
}

fn starter_card(ui: &mut Ui, win: &StartWindow, index: usize, has_picture: bool, choice: &mut Choice) {
    let (name, scene) = &win.starters[index];
    ui.vertical(|ui| {
        ui.set_width(card_size().x);
        let text = if has_picture { "…" } else { "" };
        let picture = thumbs::show(ui, win.thumbs.get(index), card_size(), text).interact(Sense::click());
        let take = |choice: &mut Choice| *choice = Choice::Scene(Box::new(scene.clone()));
        let corner = picture.rect.right_bottom() - egui::vec2(6.0, 6.0);
        if picture.on_hover_text(format!("Start from {name}")).clicked() {
            take(choice);
        }
        // "Use" sits on the picture's corner, so the name below has the card's whole width.
        let spot = egui::Rect::from_min_max(corner - egui::vec2(44.0, 22.0), corner);
        let button = egui::Button::new(RichText::new("Use").color(Color32::WHITE))
            .fill(theme::ACCENT)
            .wrap_mode(egui::TextWrapMode::Extend);
        if ui.put(spot, button).on_hover_text(format!("Start from {name}")).clicked() {
            take(choice);
        }
        ui.add(egui::Label::new(RichText::new(name).strong()).truncate());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Pending, Studio};
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        window: StartWindow,
        kit: Arc<Kit>,
        chosen: Vec<Choice>,
    }

    fn harness(recent: Vec<PathBuf>) -> Harness<'static, State> {
        let kit = Arc::new(bpe_kit::kit());
        let mut window = StartWindow::default();
        window.open_with(recent);
        let state = State { window, kit, chosen: Vec::new() };
        let mut h = Harness::builder()
            .with_size(egui::vec2(1100.0, 900.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(30)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    let kit = s.kit.clone();
                    match show(ui.ctx(), &mut s.window, &kit, None) {
                        Choice::Nothing => {}
                        choice => s.chosen.push(choice),
                    }
                },
                state,
            );
        h.run();
        h
    }

    #[test]
    fn a_long_starter_name_never_runs_under_its_use_button() {
        let h = harness(Vec::new());
        let buttons: Vec<egui::Rect> = h.get_all_by_label("Use").map(|b| b.rect()).collect();
        assert_eq!(buttons.len(), bpe_kit::scenes::STARTERS.len());
        for (name, button) in bpe_kit::scenes::STARTERS.iter().zip(&buttons) {
            let label = h.get_by_label(name).rect();
            assert!(!label.intersects(*button), "{name} runs under its Use button: {label:?} {button:?}");
        }
    }

    #[test]
    fn the_start_window_offers_blank_starters_and_recent_files() {
        let recent = PathBuf::from("/somewhere/My reel.ron");
        let mut h = harness(vec![recent.clone()]);
        assert!(h.query_by_label("Blank scene").is_some());
        for name in bpe_kit::scenes::STARTERS {
            assert!(h.query_by_label(name).is_some(), "{name}");
        }
        assert!(h.query_by_label("Open a scene file…").is_some());
        assert!(h.query_by_label("My reel.ron").is_some(), "the recent file by its name");

        h.get_all_by_label("Use").nth(2).expect("the third starter's button").click();
        h.run();
        match h.state().chosen.last() {
            Some(Choice::Scene(scene)) => assert_eq!(scene.name, bpe_kit::scenes::STARTERS[2]),
            _ => panic!("a starter was chosen"),
        }
        assert!(!h.state().window.open, "choosing closes the window");

        h.state_mut().window.open_with(vec![recent.clone()]);
        h.run();
        h.get_by_label("Blank scene").click();
        h.run();
        assert!(matches!(h.state().chosen.last(), Some(Choice::Blank)));

        h.state_mut().window.open_with(vec![recent.clone()]);
        h.run();
        h.get_by_label("My reel.ron").click();
        h.run();
        assert!(matches!(h.state().chosen.last(), Some(Choice::Recent(p)) if *p == recent));
    }

    #[test]
    fn choosing_a_starter_asks_to_save_changes_then_starts_a_new_scene() {
        let mut studio = Studio::headless();
        let ctx = egui::Context::default();
        let starter = bpe_kit::scenes::starters(&studio.kit.registry).swap_remove(3).1;

        // Nothing unsaved: the starter replaces the scene at once, as a new unsaved document.
        studio.choose_start(Choice::Scene(Box::new(starter.clone())), &ctx);
        assert_eq!(studio.scene, starter);
        assert_eq!(studio.path, None, "Save asks for a file name, so no file is overwritten");
        assert!(!studio.history.can_undo(), "a fresh start, like New");

        // Unsaved changes: the save-changes question comes first.
        studio.scene.name = "Edited".into();
        let other = bpe_kit::scenes::starters(&studio.kit.registry).swap_remove(0).1;
        studio.choose_start(Choice::Scene(Box::new(other.clone())), &ctx);
        assert!(matches!(studio.confirm, Some(Pending::Starter(_))));
        assert_eq!(studio.scene.name, "Edited", "nothing is thrown away before the answer");
    }
}
