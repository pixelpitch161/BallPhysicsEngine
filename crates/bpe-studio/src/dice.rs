//! The dice: a painted die button, and the Randomize window that rolls the whole scene or the
//! selected things from a seed, by an amount, from the scene as it was before the last roll.

use crate::app::Studio;
use crate::theme;
use bpe_core::Scene;
use bpe_kit::random::{Scope, fresh_seed};
use eframe::egui::{self, Color32, Response, RichText, Sense, Stroke, Ui, Vec2};

/// A die: a rounded square with three dots down the diagonal.
pub fn paint_die(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let rect = rect.shrink(1.0);
    painter.rect_stroke(rect, 3.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
    let step = rect.width() / 4.0;
    for i in [-1.0, 0.0, 1.0] {
        painter.circle_filled(rect.center() + Vec2::splat(step * i), rect.width() / 11.0, color);
    }
}

/// A small button showing a die, named `label` for screen readers and tests.
pub fn dice_button(ui: &mut Ui, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let name = label.to_owned();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    let color = if response.hovered() { theme::ACCENT_TEXT } else { theme::MUTED };
    paint_die(ui.painter(), rect, color);
    response.on_hover_text(label)
}

/// The Randomize window's state. A roll starts from `base` while the scene still equals the
/// last roll's result, so "Roll" tries another seed on the same scene instead of piling up.
pub struct RandomizeWindow {
    pub open: bool,
    /// Roll the whole scene, else the selected things.
    pub whole_scene: bool,
    /// How far from the current settings to go, 0 to 1.
    pub amount: f32,
    /// The seed the last roll used, and the one "Same seed again" repeats.
    pub seed: u32,
    /// The scene before the last roll, and what that roll made of it.
    pub last: Option<(Scene, Scene)>,
}

impl Default for RandomizeWindow {
    fn default() -> Self {
        Self { open: false, whole_scene: true, amount: 1.0, seed: 0, last: None }
    }
}

/// What the window asked for this frame.
enum Request {
    Roll { seed: u32 },
    Unlock,
}

/// Rolls `scope` with `seed` from the base scene: the scene before the last roll when the scene
/// is still that roll's result, else the scene as it is now.
pub fn roll_from_base(studio: &mut Studio, scope: Scope, seed: u32, amount: f32) -> usize {
    let base = match studio.panels.randomize.last.take() {
        Some((base, result)) if result == studio.scene => base,
        _ => studio.scene.clone(),
    };
    // The roll starts from the base, but Undo goes back to the scene as it was just now.
    let before = std::mem::replace(&mut studio.scene, base.clone());
    let changed = studio.roll_recording(before, scope, seed, amount);
    studio.panels.randomize.seed = seed;
    studio.panels.randomize.last = Some((base, studio.scene.clone()));
    changed
}

/// Draws the Randomize window when it is open and applies what was asked.
pub fn show(ctx: &egui::Context, studio: &mut Studio) {
    if !studio.panels.randomize.open {
        return;
    }
    let locks = studio.scene.locks.len();
    let has_selection = studio.selection.primary.is_some();
    let mut request = None;
    let mut open = true;
    {
        let win = &mut studio.panels.randomize;
        egui::Window::new("Randomize").open(&mut open).resizable(false).default_width(320.0).show(ctx, |ui| {
            ui.label(RichText::new("What to roll").strong());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut win.whole_scene, true, "Whole scene")
                    .on_hover_text("Every thing, every module in the Library, and every change on top");
                ui.add_enabled_ui(has_selection, |ui| {
                    ui.selectable_value(&mut win.whole_scene, false, "Selected things")
                        .on_hover_text("The selected things' own settings and the cards they link")
                        .on_disabled_hover_text("Select something in the tree first");
                });
            });
            ui.add_space(6.0);
            let amount = ui.label(RichText::new("How much").strong());
            ui.add(egui::Slider::new(&mut win.amount, 0.0..=1.0).show_value(false)).labelled_by(amount.id).on_hover_text(
                "All the way is something new; a little keeps things close to what they are",
            );
            ui.horizontal(|ui| {
                ui.label(RichText::new("A little").small().color(theme::MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("All new").small().color(theme::MUTED));
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let seed = ui.label(RichText::new("Seed").strong());
                ui.add(egui::DragValue::new(&mut win.seed).speed(1.0)).labelled_by(seed.id).on_hover_text(
                    "The number the dice start from: the same seed on the same scene gives the same result",
                );
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let roll = egui::Button::new(RichText::new("Roll").strong().color(Color32::WHITE)).fill(theme::ACCENT);
                if ui.add(roll).on_hover_text("Roll with a fresh seed").clicked() {
                    request = Some(Request::Roll { seed: fresh_seed() });
                }
                if ui.button("Same seed again").on_hover_text("Roll this seed again from the scene before the last roll").clicked() {
                    request = Some(Request::Roll { seed: win.seed.max(1) });
                }
            });
            ui.add_space(8.0);
            let unlock = match locks {
                0 => "Nothing is locked".to_owned(),
                1 => "Unlock everything (1 locked)".to_owned(),
                n => format!("Unlock everything ({n} locked)"),
            };
            if ui.add_enabled(locks > 0, egui::Button::new(unlock)).on_hover_text("Let the dice pick every setting again").clicked() {
                request = Some(Request::Unlock);
            }
            ui.label(
                RichText::new("Right-click a setting's name to lock it; the dice then leave it alone. Every roll is one Undo step.")
                    .small()
                    .color(theme::MUTED),
            );
        });
    }
    studio.panels.randomize.open = open;
    match request {
        Some(Request::Roll { seed }) => {
            let scope = if studio.panels.randomize.whole_scene || !has_selection {
                Scope::Scene
            } else {
                Scope::Things(studio.selection.all(&studio.scene))
            };
            let amount = studio.panels.randomize.amount;
            roll_from_base(studio, scope, seed, amount);
        }
        Some(Request::Unlock) => {
            let before = studio.scene.clone();
            studio.scene.locks.clear();
            studio.after_separate_edit(before);
            studio.recorded = true;
            studio.status = "Unlocked every setting.".to_owned();
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Studio;
    use bpe_core::{Address, Owner, ThingId};
    use egui_kittest::{Harness, kittest::Queryable};

    fn harness() -> Harness<'static, Studio> {
        let mut studio = Studio::headless();
        studio.panels.randomize.open = true;
        let mut h = Harness::builder()
            .with_size(egui::vec2(800.0, 600.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(|ui, s: &mut Studio| show(ui.ctx(), s), studio);
        h.run();
        h
    }

    #[test]
    fn the_randomize_window_rolls_the_scene_and_the_same_seed_repeats() {
        let mut h = harness();
        let size = Address { owner: Owner::Thing(ThingId(2)), slot: None, key: "radius".into() };
        h.state_mut().scene.set_locked(size.clone(), true);
        h.run();
        let original = h.state().scene.clone();
        assert!(h.query_by_label_contains("1 locked").is_some());

        h.get_by_label("Roll").click();
        h.run();
        let first = h.state().scene.clone();
        assert_ne!(first, original);
        assert!(h.state().status.contains("seed"), "{}", h.state().status);
        let reg = &h.state().kit.registry;
        assert_eq!(first.value_at(reg, &size), original.value_at(reg, &size), "locked");
        let seed = h.state().panels.randomize.seed;

        h.get_by_label("Same seed again").click();
        h.run();
        assert_eq!(h.state().scene, first, "the same seed from the same base gives the same scene");

        h.get_by_label("Roll").click();
        h.run();
        let second = h.state().scene.clone();
        assert_ne!(second, first);
        assert_ne!(h.state().panels.randomize.seed, seed);

        let s = h.state_mut();
        assert!(s.undo());
        assert_eq!(s.scene, first);
        assert!(s.undo());
        assert_eq!(s.scene, original, "every roll is one undo step");
        h.run();
        h.get_by_label_contains("Unlock everything").click();
        h.run();
        assert!(h.state().scene.locks.is_empty());
    }

    #[test]
    fn selected_things_roll_only_themselves() {
        let mut h = harness();
        h.state_mut().selection.set(ThingId(2));
        h.run();
        let original = h.state().scene.clone();
        h.get_by_label("Selected things").click();
        h.run();
        h.get_by_label("Roll").click();
        h.run();
        let s = h.state();
        assert_ne!(s.scene.things[&ThingId(2)].params, original.things[&ThingId(2)].params);
        assert_eq!(s.scene.things[&ThingId::ROOT].params, original.things[&ThingId::ROOT].params);
        assert_eq!(s.scene.things[&ThingId(1)].params, original.things[&ThingId(1)].params, "the arena stays");
    }

    #[test]
    fn a_rolled_card_is_one_undo_step() {
        let mut studio = Studio::headless();
        let original = studio.scene.clone();
        let changed = studio.roll_recording(original.clone(), Scope::Card { thing: ThingId(2), slot: None }, 9, 1.0);
        assert!(changed > 0);
        assert_ne!(studio.scene, original);
        assert!(studio.undo());
        assert_eq!(studio.scene, original);
    }
}
