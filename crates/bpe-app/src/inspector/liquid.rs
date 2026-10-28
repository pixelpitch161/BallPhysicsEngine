//! Liquid (Balls tab): one section for both kinds of liquid. Small liquid is simulated with the
//! balls on the CPU (`scene.bodies.fluid`); big liquid fills the arena on the graphics card
//! (`scene.gpu.fluid`). The scene keeps both as before, so old scenes load unchanged.

use super::gpu::{fresh_dice, switch_heading};
use super::{
    color_row, grid, label, launch_rows, log_units, more, percent, plain_color_row, position_row, slider, units,
};
use crate::theme;
use bpe_scene::{CPU_FLUID_LIMIT, Fluid, GpuFluid, MAX_FLUID_PARTICLES, MIN_GPU_FLUID_PARTICLES, Scene, Vec2};
use eframe::egui::{RichText, SliderClamping, Ui};

/// Drops a new small liquid starts with.
const SMALL_DROPS: u32 = 2000;

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {
    rolls: u64,
    /// The small liquid's drop count before it was switched off or made big, to bring back.
    small_drops: Option<u32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Size {
    Small,
    Big,
}

/// Draws the section. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    let small_on = scene.bodies.fluid.count > 0;
    let big_on = scene.gpu.fluid.enabled;
    let mut on = small_on || big_on;
    let help = "Water that splashes around the balls. Pick small liquid that balls float in and \
                splash, or big liquid that fills the arena.";
    let rolled = switch_heading(ui, "Liquid", help, &mut on);
    if !on {
        if small_on {
            state.small_drops = Some(scene.bodies.fluid.count);
            scene.bodies.fluid.count = 0;
        }
        scene.gpu.fluid.enabled = false;
        return;
    }
    if !small_on && !big_on {
        // Just switched on: start small, the kind that pushes balls back and makes sounds.
        scene.bodies.fluid.count = state.small_drops.take().unwrap_or(SMALL_DROPS);
    }
    let size = if scene.gpu.fluid.enabled && scene.bodies.fluid.count == 0 { Size::Big } else { Size::Small };
    if rolled {
        let mut dice = fresh_dice(&mut state.rolls);
        match size {
            Size::Small => roll_small(&mut scene.bodies.fluid, &mut dice),
            Size::Big => scene.gpu.fluid.roll(&mut dice, 1.0),
        }
    }

    grid(ui, "liquid-size", |ui| {
        label(ui, "Size", "Small liquid and big liquid work differently. Hover each for details.");
        ui.horizontal(|ui| {
            let small = ui.selectable_label(size == Size::Small, "Small").on_hover_text(
                "Up to 8000 drops. Balls float in it and it pushes them around, its splashes make \
                 sounds, and it plays out the same every time.",
            );
            let big = ui.selectable_label(size == Size::Big, "Big").on_hover_text(
                "Up to 300,000 drops on the graphics card. Fills the arena with smooth, glassy water. \
                 Balls push it aside but it doesn't push back, and it makes no sounds.",
            );
            if small.clicked() && size != Size::Small {
                make_small(scene, state);
            }
            if big.clicked() && size != Size::Big {
                make_big(scene, state);
            }
        });
        ui.end_row();
    });

    let both = scene.bodies.fluid.count > 0 && scene.gpu.fluid.enabled;
    if both {
        // Only older scenes have both. Keep showing both so nothing is hidden.
        ui.label(RichText::new("This scene has both small and big liquid.").color(theme::MUTED));
    }
    if scene.bodies.fluid.count > 0 {
        if both {
            ui.label(RichText::new("Small liquid").strong().color(theme::TEXT));
        }
        small_rows(ui, &mut scene.bodies.fluid);
    }
    if scene.gpu.fluid.enabled {
        if both {
            ui.label(RichText::new("Big liquid").strong().color(theme::TEXT));
        }
        big_rows(ui, &mut scene.gpu.fluid);
    }
}

/// Turns the big liquid into small liquid, keeping its colour and thickness.
fn make_small(scene: &mut Scene, state: &mut State) {
    let big = &mut scene.gpu.fluid;
    big.enabled = false;
    scene.bodies.fluid = Fluid {
        count: state.small_drops.take().unwrap_or(SMALL_DROPS),
        color: big.color,
        thickness: big.viscosity,
        ..scene.bodies.fluid.clone()
    };
}

/// Turns the small liquid into big liquid, keeping its colour and thickness.
fn make_big(scene: &mut Scene, state: &mut State) {
    let small = &mut scene.bodies.fluid;
    if small.count > 0 {
        state.small_drops = Some(small.count.min(CPU_FLUID_LIMIT));
        scene.gpu.fluid.color = small.color;
        scene.gpu.fluid.viscosity = small.thickness;
    }
    small.count = 0;
    scene.gpu.fluid.enabled = true;
}

pub(crate) fn roll_small(fluid: &mut Fluid, dice: &mut bpe_scene::Dice) {
    fluid.thickness = dice.range(0.0, 0.8);
    fluid.radius = dice.range(4.0, 10.0);
    if dice.chance(1.0) {
        fluid.color = bpe_scene::Color::from_hsv(dice.unit(), dice.range(0.5, 0.95), dice.range(0.6, 1.0));
    }
}

fn small_rows(ui: &mut Ui, fluid: &mut Fluid) {
    let d = Fluid::default();
    ui.push_id("small-liquid", |ui| {
        grid(ui, "small-liquid", |ui| {
            slider(
                ui,
                "Drops",
                "How many drops of liquid there are. For more than 8000, pick Big.",
                &mut fluid.count,
                SMALL_DROPS,
                1..=CPU_FLUID_LIMIT,
                // Older scenes can have more; leave them as they are until the slider is moved.
                |s| s.logarithmic(true).clamping(SliderClamping::Edits),
            );
            slider(
                ui,
                "Drop size",
                "Radius of each drop in pixels.",
                &mut fluid.radius,
                d.radius,
                2.0..=30.0,
                units(" px", 1),
            );
            slider(
                ui,
                "Thickness",
                "0% flows like water, 100% oozes like honey.",
                &mut fluid.thickness,
                d.thickness,
                0.0..=1.0,
                percent,
            );
            color_row(ui, "Colour", "The liquid's colour.", &mut fluid.color, d.color);
        });
        if fluid.on_gpu() {
            // Older scenes could ask for more small liquid than the CPU runs, which then didn't show.
            ui.label(
                RichText::new("This is too many drops for small liquid, so it won't show. Lower Drops or pick Big.")
                    .color(theme::MUTED),
            );
        }
        more(ui, "liquid-more", |ui| {
            grid(ui, "liquid-more-grid", |ui| {
                position_row(
                    ui,
                    "Start position",
                    "Where the middle of the liquid starts, from the centre of the video.",
                    &mut fluid.center,
                );
                slider(
                    ui,
                    "Start width",
                    "Width of the block the liquid starts as.",
                    &mut fluid.size.x,
                    d.size.x,
                    20.0..=1080.0,
                    units(" px", 0),
                );
                launch_rows(ui, &mut fluid.velocity, Vec2::ZERO);
            });
        });
    });
}

fn big_rows(ui: &mut Ui, fluid: &mut GpuFluid) {
    let d = GpuFluid::default();
    ui.push_id("big-liquid", |ui| {
        grid(ui, "fluid", |ui| {
            slider(
                ui,
                "Drops",
                "How many drops make the liquid. More looks smoother but needs a better graphics card.",
                &mut fluid.count,
                d.count,
                MIN_GPU_FLUID_PARTICLES..=MAX_FLUID_PARTICLES,
                |s| log_units("")(s).integer(),
            );
            slider(ui, "Fill", "How full the arena is.", &mut fluid.fill, d.fill, 0.02..=0.9, percent);
            slider(
                ui,
                "Pour in over",
                "Seconds the liquid takes to pour in from the top. 0 starts full.",
                &mut fluid.pour_seconds,
                d.pour_seconds,
                0.0..=30.0,
                units(" s", 1),
            );
            plain_color_row(ui, "Colour", "The liquid's colour.", &mut fluid.color, d.color);
            slider(
                ui,
                "Clarity",
                "How clearly you see through the liquid.",
                &mut fluid.clarity,
                d.clarity,
                0.0..=1.0,
                percent,
            );
        });
        more(ui, "fluid-more", |ui| {
            grid(ui, "fluid-more-grid", |ui| {
                slider(
                    ui,
                    "Bending",
                    "How much the liquid bends what is behind it.",
                    &mut fluid.refraction,
                    d.refraction,
                    0.0..=1.0,
                    percent,
                );
                slider(
                    ui,
                    "Shine",
                    "How bright the surface highlights are.",
                    &mut fluid.highlight,
                    d.highlight,
                    0.0..=1.0,
                    percent,
                );
                slider(
                    ui,
                    "Thickness",
                    "Thicker liquid flows slower, like honey.",
                    &mut fluid.viscosity,
                    d.viscosity,
                    0.0..=1.0,
                    percent,
                );
                slider(
                    ui,
                    "Springiness",
                    "How strongly the liquid resists being squashed.",
                    &mut fluid.stiffness,
                    d.stiffness,
                    0.0..=1.0,
                    percent,
                );
                slider(
                    ui,
                    "Ball push",
                    "How strongly balls push the liquid aside.",
                    &mut fluid.ball_push,
                    d.ball_push,
                    0.0..=3.0,
                    percent,
                );
                slider(
                    ui,
                    "Gravity",
                    "How hard the liquid falls. The balls' own gravity is separate.",
                    &mut fluid.gravity,
                    d.gravity,
                    0.0..=3000.0,
                    units(" px/s²", 0),
                );
                slider(
                    ui,
                    "Smoothness",
                    "How smooth the surface looks.",
                    &mut fluid.smoothing,
                    d.smoothing,
                    1.0..=16.0,
                    units(" px", 1),
                );
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use crate::inspector::Tab;
    use crate::inspector::gpu::tests::harness;
    use bpe_scene::{Fluid, GpuFluid, Scene};
    use egui_kittest::kittest::Queryable;

    #[test]
    fn switching_on_starts_small_and_size_swaps_kinds() {
        let mut harness = harness(Tab::Balls, Scene::default());
        assert!(harness.query_by_label("Drops").is_none());
        harness.get_by_label("LIQUID").click();
        harness.run();
        assert_eq!(harness.state().1.bodies.fluid.count, super::SMALL_DROPS);
        assert!(!harness.state().1.gpu.fluid.enabled);
        harness.get_by_label("Drop size");

        harness.get_by_label("Big").click();
        harness.run();
        let scene = &harness.state().1;
        assert!(scene.gpu.fluid.enabled && scene.bodies.fluid.count == 0);
        assert_eq!(scene.gpu.fluid.color, Fluid::default().color);
        harness.get_by_label("Fill");
        assert!(harness.query_by_label("Drop size").is_none());

        harness.get_by_label("Small").click();
        harness.run();
        let scene = &harness.state().1;
        assert!(!scene.gpu.fluid.enabled && scene.bodies.fluid.count == super::SMALL_DROPS);

        harness.get_by_label("LIQUID").click();
        harness.run();
        let scene = &harness.state().1;
        assert!(!scene.gpu.fluid.enabled && scene.bodies.fluid.count == 0);
        assert!(harness.query_by_label("Drops").is_none());
    }

    #[test]
    fn big_liquid_settings_reset_and_switch_off() {
        let mut scene = Scene::default();
        scene.gpu.fluid.enabled = true;
        scene.gpu.fluid.fill = 0.8;
        let mut harness = harness(Tab::Balls, scene);
        harness.get_by_label("Fill").click();
        harness.step();
        harness.get_by_label("Fill").click();
        harness.run();
        assert_eq!(harness.state().1.gpu.fluid.fill, GpuFluid::default().fill);
        harness.get_by_label("LIQUID").click();
        harness.run();
        assert!(!harness.state().1.gpu.fluid.enabled);
        assert!(harness.query_by_label("Fill").is_none());
    }

    #[test]
    fn a_scene_with_both_liquids_shows_both() {
        let mut scene = Scene::default();
        scene.gpu.fluid.enabled = true;
        scene.bodies.fluid.count = 500;
        let harness = harness(Tab::Balls, scene);
        harness.get_by_label("Small liquid");
        harness.get_by_label("Big liquid");
        harness.get_by_label("Fill");
        harness.get_by_label("Drop size");
        // Drawing the panel alone changes nothing.
        assert!(harness.state().1.gpu.fluid.enabled && harness.state().1.bodies.fluid.count == 500);
    }
}
