//! Swarms and big fluids: the GPU layers (Balls tab).

use super::{color_list, color_row, dice_button, grid, label, log_units, more, percent, slider, units};
use crate::theme;
use bpe_scene::{
    Dice, GpuFluid, MAX_FLUID_PARTICLES, MAX_SWARM_POINTS, MIN_GPU_FLUID_PARTICLES, Scene, Swarm, SwarmColor,
    SwarmStart,
};
use eframe::egui::{self, RichText, Ui};

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {
    rolls: u64,
}

impl State {
    fn dice(&mut self) -> Dice {
        fresh_dice(&mut self.rolls)
    }
}

/// A dice with a new seed for each roll, counting rolls in `rolls`.
pub(super) fn fresh_dice(rolls: &mut u64) -> Dice {
    *rolls += 1;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    Dice::new(nanos as u64 ^ rolls.wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

/// Draws the section. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    swarm(ui, &mut scene.gpu.swarm, state);
    fluid(ui, &mut scene.gpu.fluid, state);
}

/// A heading with an on switch and a dice that rolls new settings.
fn switch_heading(ui: &mut Ui, text: &str, help: &str, on: &mut bool) -> bool {
    let mut rolled = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.checkbox(on, RichText::new(text.to_uppercase()).size(12.0).color(theme::MUTED)).on_hover_text(help);
        if *on {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                rolled = dice_button(ui, &format!("Roll random {} settings", text.to_lowercase())).clicked();
            });
        }
    });
    rolled
}

fn swarm(ui: &mut Ui, swarm: &mut Swarm, state: &mut State) {
    let help = "Thousands to a million tiny glowing points that flow, flock and swirl around the balls.";
    if switch_heading(ui, "Swarm", help, &mut swarm.enabled) {
        swarm.roll(&mut state.dice(), 1.0);
    }
    if !swarm.enabled {
        return;
    }
    let d = Swarm::default();
    grid(ui, "swarm", |ui| {
        slider(
            ui,
            "Points",
            "How many points. A million needs a good graphics card.",
            &mut swarm.count,
            d.count,
            1000..=MAX_SWARM_POINTS,
            |s| log_units("")(s).integer(),
        );
        label(ui, "Start", "Where the points are at the start.");
        ui.horizontal(|ui| {
            for (start, name) in
                [(SwarmStart::Fill, "Everywhere"), (SwarmStart::Center, "Middle"), (SwarmStart::Ring, "Edge")]
            {
                if ui.selectable_label(swarm.start == start, name).clicked() {
                    swarm.start = start;
                }
            }
        });
        ui.end_row();
        slider(
            ui,
            "Flow",
            "How strongly the points follow swirling currents.",
            &mut swarm.flow,
            d.flow,
            0.0..=3.0,
            percent,
        );
        slider(
            ui,
            "Flocking",
            "How much points line up with their neighbours, like birds.",
            &mut swarm.flock,
            d.flock,
            0.0..=1.0,
            percent,
        );
        slider(
            ui,
            "Pull to balls",
            "Positive draws points toward balls; negative pushes them away.",
            &mut swarm.attraction,
            d.attraction,
            -3.0..=3.0,
            percent,
        );
        label(ui, "Colour", "How the points are coloured.");
        ui.horizontal(|ui| {
            for (color, name, tip) in [
                (SwarmColor::Speed, "Speed", "From the first colour when slow to the last when fast."),
                (SwarmColor::NearestBall, "Nearest ball", "The colour of the closest ball."),
                (SwarmColor::Gradient, "Mix", "Each point keeps one colour from the list."),
            ] {
                if ui.selectable_label(swarm.color == color, name).on_hover_text(tip).clicked() {
                    swarm.color = color;
                }
            }
        });
        ui.end_row();
        if swarm.color != SwarmColor::NearestBall {
            label(ui, "Colours", "The colours the points use.");
            color_list(ui, &mut swarm.colors, "+");
            ui.end_row();
        }
        slider(
            ui,
            "Brightness",
            "How much light each point adds. Dense areas glow brighter.",
            &mut swarm.brightness,
            d.brightness,
            0.01..=1.0,
            percent,
        );
    });
    more(ui, "swarm-more", |ui| {
        grid(ui, "swarm-more-grid", |ui| {
            slider(
                ui,
                "Point size",
                "Size of each point in pixels.",
                &mut swarm.size,
                d.size,
                0.5..=6.0,
                units(" px", 1),
            );
            slider(
                ui,
                "Top speed",
                "The fastest a point can move.",
                &mut swarm.max_speed,
                d.max_speed,
                20.0..=2000.0,
                log_units(" px/s"),
            );
            slider(ui, "Drag", "How quickly points slow down.", &mut swarm.drag, d.drag, 0.0..=5.0, units("", 2));
            slider(
                ui,
                "Current size",
                "How big the swirls of the flow are.",
                &mut swarm.flow_scale,
                d.flow_scale,
                20.0..=1000.0,
                log_units(" px"),
            );
            slider(
                ui,
                "Current change",
                "How quickly the swirls change over time.",
                &mut swarm.flow_speed,
                d.flow_speed,
                0.0..=1.0,
                units("", 2),
            );
            slider(
                ui,
                "Spacing",
                "How much points keep apart from their neighbours.",
                &mut swarm.separation,
                d.separation,
                0.0..=1.0,
                percent,
            );
            slider(
                ui,
                "Ball reach",
                "How far a ball's pull or push reaches.",
                &mut swarm.reach,
                d.reach,
                10.0..=1000.0,
                log_units(" px"),
            );
            slider(
                ui,
                "Avoid walls",
                "How hard points steer away from walls before touching them.",
                &mut swarm.avoid_walls,
                d.avoid_walls,
                0.0..=3.0,
                percent,
            );
            slider(
                ui,
                "Gravity",
                "Pull on the points. Negative floats them up.",
                &mut swarm.gravity,
                d.gravity,
                -1000.0..=1000.0,
                units(" px/s²", 0),
            );
        });
    });
}

fn fluid(ui: &mut Ui, fluid: &mut GpuFluid, state: &mut State) {
    let help = "Water that fills the arena and splashes around the balls. Balls push it; it does not push back.";
    if switch_heading(ui, "Liquid", help, &mut fluid.enabled) {
        fluid.roll(&mut state.dice(), 1.0);
    }
    if !fluid.enabled {
        return;
    }
    let d = GpuFluid::default();
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
        color_row(ui, "Colour", "The liquid's colour.", &mut fluid.color, d.color);
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
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::inspector::{self, Inspector, Tab};
    use bpe_scene::Scene;
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    /// The settings panel on one tab, for driving with simulated input.
    pub(crate) fn harness(tab: Tab, scene: Scene) -> Harness<'static, (Inspector, Scene)> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(460.0, 3200.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(240)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(
                |ui, (inspector, scene): &mut (Inspector, Scene)| inspector::show(ui, inspector, scene),
                (Inspector { tab, ..Inspector::default() }, scene),
            );
        harness.run();
        harness
    }

    #[test]
    fn switching_the_swarm_on_shows_its_settings() {
        let mut harness = harness(Tab::Balls, Scene::default());
        assert!(harness.query_by_label("Points").is_none());
        harness.get_by_label("SWARM").click();
        harness.run();
        assert!(harness.state().1.gpu.swarm.enabled);
        harness.get_by_label("Points");
        harness.get_by_label("Roll random swarm settings").click();
        harness.run();
        assert_ne!(harness.state().1.gpu.swarm, bpe_scene::Swarm { enabled: true, ..Default::default() });
    }

    #[test]
    fn switching_the_liquid_on_and_resetting_a_setting() {
        let mut scene = Scene::default();
        scene.gpu.fluid.enabled = true;
        scene.gpu.fluid.fill = 0.8;
        let mut harness = harness(Tab::Balls, scene);
        harness.get_by_label("Fill").click();
        harness.step();
        harness.get_by_label("Fill").click();
        harness.run();
        assert_eq!(harness.state().1.gpu.fluid.fill, bpe_scene::GpuFluid::default().fill);
        harness.get_by_label("LIQUID").click();
        harness.run();
        assert!(!harness.state().1.gpu.fluid.enabled);
        assert!(harness.query_by_label("Fill").is_none());
    }
}
