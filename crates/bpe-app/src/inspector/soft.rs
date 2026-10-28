//! Soft bodies: jelly balls, ropes and cloth (Balls tab), and how balls bump into each
//! other (Bounces tab).

use super::{
    color_list, color_row, dice_button, grid, heading, label, launch_rows, more, percent, position_row, slider, units,
};
use crate::theme;
use bpe_scene::{
    BallContact, Cloth, ClothPin, Color, Container, Dice, GOOD_CONTACT_QUALITY, Jelly, Pocket, Rope, Scene, Vec2,
    palette,
};
use eframe::egui::{self, RichText, Ui};

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {
    /// Counts dice rolls, so each roll comes out different.
    rolls: u64,
}

impl State {
    fn dice(&mut self) -> Dice {
        self.rolls += 1;
        Dice::new(0x5eed_50f7 ^ self.rolls.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }
}

/// One soft thing to remove or copy after the cards are drawn.
enum Change {
    Remove(Kind, usize),
    Duplicate(Kind, usize),
    Roll(Kind, usize),
}

#[derive(Clone, Copy)]
enum Kind {
    Jelly,
    Rope,
    Cloth,
}

/// A card with a title, Remove, Duplicate and dice buttons, and its rows.
fn card(
    ui: &mut Ui,
    id: (&str, usize),
    title: String,
    kind: Kind,
    change: &mut Option<Change>,
    add: impl FnOnce(&mut Ui),
) {
    ui.push_id(id, |ui| {
        egui::Frame::new()
            .fill(theme::CARD)
            .stroke(egui::Stroke::new(1.0, theme::LINE))
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).strong().color(theme::TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").on_hover_text("Delete this").clicked() {
                            *change = Some(Change::Remove(kind, id.1));
                        }
                        if ui.small_button("Duplicate").on_hover_text("Add a copy of this").clicked() {
                            *change = Some(Change::Duplicate(kind, id.1));
                        }
                        if dice_button(ui, "Roll random settings for this").clicked() {
                            *change = Some(Change::Roll(kind, id.1));
                        }
                    });
                });
                ui.add_space(4.0);
                add(ui);
            });
        ui.add_space(6.0);
    });
}

/// Jelly balls, ropes and cloth in the Balls tab. Liquid has its own section (`liquid`).
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    ui.add_space(10.0);
    heading(ui, "Soft things");
    let bodies = &mut scene.bodies;
    // Liquid also lives in `bodies`, but it has its own section and ignores the solver passes.
    let none = bodies.jelly.is_empty() && bodies.ropes.is_empty() && bodies.cloths.is_empty();
    if none {
        ui.label(
            RichText::new("Squishy jelly balls, ropes and cloth that balls can bump into. Add some below.")
                .color(theme::MUTED),
        );
    }
    let mut change = None;
    for (index, jelly) in bodies.jelly.iter_mut().enumerate() {
        let title = format!("Jelly balls · {} × {}", jelly.columns, jelly.rows);
        card(ui, ("jelly", index), title, Kind::Jelly, &mut change, |ui| jelly_rows(ui, jelly));
    }
    for (index, rope) in bodies.ropes.iter_mut().enumerate() {
        card(ui, ("rope", index), "Rope".into(), Kind::Rope, &mut change, |ui| rope_rows(ui, rope));
    }
    for (index, cloth) in bodies.cloths.iter_mut().enumerate() {
        card(ui, ("cloth", index), "Cloth".into(), Kind::Cloth, &mut change, |ui| cloth_rows(ui, cloth));
    }
    match change {
        Some(Change::Remove(kind, i)) => match kind {
            Kind::Jelly => drop(bodies.jelly.remove(i)),
            Kind::Rope => drop(bodies.ropes.remove(i)),
            Kind::Cloth => drop(bodies.cloths.remove(i)),
        },
        Some(Change::Duplicate(kind, i)) => {
            // The copy goes a little to the side, so it doesn't sit exactly on the original.
            let shift = Vec2::new(60.0, 60.0);
            match kind {
                Kind::Jelly => {
                    let copy = Jelly { center: bodies.jelly[i].center + shift, ..bodies.jelly[i].clone() };
                    bodies.jelly.insert(i + 1, copy);
                }
                Kind::Rope => {
                    let r = &bodies.ropes[i];
                    let copy = Rope { from: r.from + shift, to: r.to + shift, ..r.clone() };
                    bodies.ropes.insert(i + 1, copy);
                }
                Kind::Cloth => {
                    let copy = Cloth { center: bodies.cloths[i].center + shift, ..bodies.cloths[i].clone() };
                    bodies.cloths.insert(i + 1, copy);
                }
            }
        }
        Some(Change::Roll(kind, i)) => {
            let mut dice = state.dice();
            match kind {
                Kind::Jelly => roll_jelly(&mut bodies.jelly[i], &mut dice),
                Kind::Rope => roll_rope(&mut bodies.ropes[i], &mut dice),
                Kind::Cloth => roll_cloth(&mut bodies.cloths[i], &mut dice),
            }
        }
        None => {}
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Jelly balls").on_hover_text("A block of squishy balls that wobble when they land").clicked() {
            bodies.jelly.push(Jelly { columns: 2, rows: 2, ..Jelly::default() });
        }
        if ui.button("+ Rope").on_hover_text("A rope held at both ends that balls bounce on").clicked() {
            bodies.ropes.push(Rope::default());
        }
        if ui.button("+ Cloth").on_hover_text("A sheet hanging from its top edge that balls push around").clicked() {
            bodies.cloths.push(Cloth::default());
        }
    });
    if !none {
        more(ui, "soft-more", |ui| {
            grid(ui, "soft-more-grid", |ui| {
                slider(
                    ui,
                    "Solver passes",
                    "How many times each step soft things are pulled back into shape. More is stiffer and steadier but slower.",
                    &mut bodies.iterations,
                    8,
                    1..=32,
                    |s| s,
                );
            });
        });
    }
}

fn jelly_rows(ui: &mut Ui, jelly: &mut Jelly) {
    let defaults = Jelly::default();
    grid(ui, "jelly", |ui| {
        slider(
            ui,
            "Balls across",
            "How many columns of jelly balls the block has.",
            &mut jelly.columns,
            1,
            1..=12,
            |s| s,
        );
        slider(ui, "Balls down", "How many rows of jelly balls the block has.", &mut jelly.rows, 1, 1..=12, |s| s);
        slider(
            ui,
            "Ball size",
            "Radius of each jelly ball in pixels.",
            &mut jelly.radius,
            defaults.radius,
            10.0..=300.0,
            units(" px", 0),
        );
        slider(
            ui,
            "Squishiness",
            "How much the balls squash when they land. 0% is firm rubber, 100% is wobbly jelly.",
            &mut jelly.squishiness,
            defaults.squishiness,
            0.0..=1.0,
            percent,
        );
        launch_rows(ui, &mut jelly.velocity, Vec2::ZERO);
        label(ui, "Colours", "Colours from the first jelly ball to the last, blended evenly.");
        color_list(ui, &mut jelly.gradient, "+");
        ui.end_row();
    });
    more(ui, "jelly-more", |ui| {
        grid(ui, "jelly-more-grid", |ui| {
            slider(
                ui,
                "Gap",
                "Distance from one jelly ball's centre to the next.",
                &mut jelly.spacing,
                defaults.spacing,
                10.0..=600.0,
                units(" px", 0),
            );
            slider(
                ui,
                "Points around",
                "How many points make up each ball's skin. More is rounder but slower.",
                &mut jelly.points,
                defaults.points,
                8..=64,
                |s| s,
            );
            position_row(
                ui,
                "Start position",
                "Where the middle of the block starts, from the centre of the video.",
                &mut jelly.center,
            );
        });
    });
}

fn rope_rows(ui: &mut Ui, rope: &mut Rope) {
    let defaults = Rope::default();
    grid(ui, "rope", |ui| {
        position_row(ui, "Start", "Where the rope's first end is, from the centre of the video.", &mut rope.from);
        position_row(ui, "End", "Where the rope's other end is, from the centre of the video.", &mut rope.to);
        slider(
            ui,
            "Slack",
            "How much longer the rope is than the gap between its ends. More sags lower.",
            &mut rope.slack,
            defaults.slack,
            0.5..=3.0,
            percent,
        );
        slider(
            ui,
            "Stiffness",
            "100% doesn't stretch at all; lower is a stretchy bungee cord.",
            &mut rope.stiffness,
            defaults.stiffness,
            0.0..=1.0,
            percent,
        );
        slider(
            ui,
            "Thickness",
            "How thick the rope is drawn and how far balls stay off it.",
            &mut rope.thickness,
            defaults.thickness,
            1.0..=60.0,
            units(" px", 1),
        );
        color_row(ui, "Colour", "", &mut rope.color, defaults.color);
        label(ui, "Held at", "Which ends are fixed in place. A loose end swings freely.");
        ui.horizontal(|ui| {
            ui.checkbox(&mut rope.pin_start, "Start");
            ui.checkbox(&mut rope.pin_end, "End");
        });
        ui.end_row();
    });
    more(ui, "rope-more", |ui| {
        grid(ui, "rope-more-grid", |ui| {
            slider(
                ui,
                "Points",
                "How many points the rope is made of. More bends smoothly but is slower.",
                &mut rope.points,
                defaults.points,
                2..=400,
                |s| s,
            );
        });
    });
}

fn cloth_rows(ui: &mut Ui, cloth: &mut Cloth) {
    let defaults = Cloth::default();
    grid(ui, "cloth", |ui| {
        slider(
            ui,
            "Width",
            "Width of the cloth in pixels.",
            &mut cloth.size.x,
            defaults.size.x,
            20.0..=1080.0,
            units(" px", 0),
        );
        slider(
            ui,
            "Height",
            "Height of the cloth in pixels.",
            &mut cloth.size.y,
            defaults.size.y,
            20.0..=1900.0,
            units(" px", 0),
        );
        slider(
            ui,
            "Stiffness",
            "How firmly the cloth keeps its shape. Lower drapes and folds more.",
            &mut cloth.stiffness,
            defaults.stiffness,
            0.0..=1.0,
            percent,
        );
        label(ui, "Held by", "Which points of the cloth are fixed in place.");
        ui.horizontal(|ui| {
            for (pin, name, tip) in [
                (ClothPin::TopRow, "Top edge", "The whole top edge is fixed, like a curtain."),
                (ClothPin::TopCorners, "Top corners", "Only the two top corners are fixed, so it sags in the middle."),
                (ClothPin::None, "Nothing", "Nothing holds it, so it falls."),
            ] {
                ui.selectable_value(&mut cloth.pin, pin, name).on_hover_text(tip);
            }
        });
        ui.end_row();
        color_row(ui, "Colour", "", &mut cloth.color, defaults.color);
    });
    more(ui, "cloth-more", |ui| {
        grid(ui, "cloth-more-grid", |ui| {
            slider(
                ui,
                "Points across",
                "How many points the cloth has across. More is smoother but slower.",
                &mut cloth.columns,
                defaults.columns,
                2..=120,
                |s| s,
            );
            slider(
                ui,
                "Points down",
                "How many points the cloth has down. More is smoother but slower.",
                &mut cloth.rows,
                defaults.rows,
                2..=120,
                |s| s,
            );
            position_row(
                ui,
                "Position",
                "Where the middle of the cloth starts, from the centre of the video.",
                &mut cloth.center,
            );
        });
    });
}

fn roll_jelly(jelly: &mut Jelly, dice: &mut Dice) {
    jelly.columns = dice.int(1, 4);
    jelly.rows = dice.int(1, 4);
    jelly.radius = dice.range(30.0, 110.0);
    jelly.spacing = jelly.radius * dice.range(2.1, 2.6);
    jelly.squishiness = dice.range(0.2, 0.95);
    jelly.velocity = Vec2::new(dice.range(-300.0, 300.0), dice.range(-200.0, 100.0));
    jelly.gradient = (0..dice.int(1, 3)).map(|_| bright(dice)).collect();
}

fn roll_rope(rope: &mut Rope, dice: &mut Dice) {
    let y = dice.range(-500.0, 400.0);
    let half = dice.range(150.0, 420.0);
    rope.from = Vec2::new(-half, y + dice.range(-150.0, 150.0));
    rope.to = Vec2::new(half, y + dice.range(-150.0, 150.0));
    rope.slack = dice.range(1.0, 1.5);
    rope.stiffness = if dice.chance(0.6) { 1.0 } else { dice.range(0.2, 0.9) };
    rope.thickness = dice.range(4.0, 16.0);
    rope.color = bright(dice);
}

fn roll_cloth(cloth: &mut Cloth, dice: &mut Dice) {
    cloth.size = Vec2::new(dice.range(300.0, 800.0), dice.range(250.0, 700.0));
    cloth.stiffness = dice.range(0.3, 1.0);
    cloth.pin = dice.pick(&[ClothPin::TopRow, ClothPin::TopCorners, ClothPin::TopCorners]);
    cloth.color = bright(dice);
}

fn bright(dice: &mut Dice) -> Color {
    palette(dice, 1)[0]
}

/// The ball-to-ball settings in the Bounces tab: whether balls bump into each other, and how.
pub fn bounces_section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    ui.add_space(10.0);
    let physics = &mut scene.physics;
    ui.horizontal(|ui| {
        ui.label(RichText::new("BALLS BUMP INTO EACH OTHER").size(12.0).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if dice_button(ui, "Roll random settings for balls bumping into each other").clicked() {
                let mut dice = state.dice();
                physics.ball_collisions = true;
                let contact = &mut physics.contact;
                contact.quality = dice.pick(&[4, 8, 8, 12, 16]);
                contact.friction = dice.range(0.0, 0.6);
                contact.spin = dice.chance(0.5);
            }
        });
    });
    let was_on = physics.ball_collisions;
    ui.checkbox(&mut physics.ball_collisions, "Balls bump into each other")
        .on_hover_text("When off, balls pass through one another and only bounce off walls and pegs.");
    // Turning it on picks the steady solver, so piles settle; older scenes keep the classic one.
    if physics.ball_collisions && !was_on && physics.contact.quality == 0 {
        physics.contact.quality = GOOD_CONTACT_QUALITY;
    }
    if !physics.ball_collisions {
        return;
    }
    let contact = &mut physics.contact;
    let defaults = BallContact { quality: GOOD_CONTACT_QUALITY, ..BallContact::default() };
    grid(ui, "contact", |ui| {
        slider(
            ui,
            "Stacking quality",
            "How carefully touching balls are worked out. Higher makes piles and stacks stand still; 0 is the quick classic push-apart older scenes use.",
            &mut contact.quality,
            defaults.quality,
            0..=32,
            |s| s.custom_formatter(|v, _| if v == 0.0 { "Classic".into() } else { format!("{v:.0}") }),
        );
        if contact.quality > 0 {
            slider(
                ui,
                "Friction",
                "How much balls grip each other as they rub. 0 is like ice.",
                &mut contact.friction,
                defaults.friction,
                0.0..=1.0,
                percent,
            );
            let name = label(
                ui,
                "Spin",
                "Balls spin when they rub against each other, shown by a glint turning on each ball.",
            );
            ui.checkbox(&mut contact.spin, "").labelled_by(name.id);
            ui.end_row();
        }
    });
    if contact.quality == 0 {
        return;
    }
    let mut springy = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Feel like:").color(theme::MUTED));
        if ui.button("Billiards").on_hover_text("Hard, slippery balls that spin").clicked() {
            (contact.quality, contact.friction, contact.spin) = (8, 0.08, true);
        }
        if ui.button("Sand pile").on_hover_text("Grippy balls that heap up and settle").clicked() {
            (contact.quality, contact.friction, contact.spin) = (12, 0.5, false);
        }
        if ui.button("Newton's cradle").on_hover_text("Perfect clacks that pass the push along a row").clicked() {
            (contact.quality, contact.friction, contact.spin) = (16, 0.0, false);
            springy = true;
        }
    });
    if springy {
        scene.physics.restitution = 1.0;
    }
    let contact = &mut scene.physics.contact;
    more(ui, "contact-more", |ui| {
        grid(ui, "contact-more-grid", |ui| {
            if contact.spin {
                slider(
                    ui,
                    "Spin fades",
                    "How quickly spinning balls stop spinning, per second.",
                    &mut contact.spin_drag,
                    defaults.spin_drag,
                    0.0..=5.0,
                    percent,
                );
            }
            let name = label(
                ui,
                "Resting balls sleep",
                "Balls that have come to rest in a pile stop moving until something hits them, which keeps big piles perfectly still and quick.",
            );
            ui.checkbox(&mut contact.sleep, "").labelled_by(name.id);
            ui.end_row();
            label(
                ui,
                "Pockets",
                "Holes that swallow balls, like a billiard table's. Each one swallowed makes an escape.",
            );
            ui.horizontal(|ui| {
                ui.label(format!("{}", contact.pockets.len()));
                if let Container::Rect { center, size, thickness, .. } = &scene.container
                    && ui
                        .small_button("Billiard pockets")
                        .on_hover_text("Put six pockets in the corners and middles of the box")
                        .clicked()
                {
                    let half = *size * 0.5 - Vec2::splat(*thickness * 0.5 + 2.0);
                    contact.pockets = [(-1.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (1.0, 1.0)]
                        .map(|(x, y)| Pocket {
                            position: *center + half * Vec2::new(x, y),
                            radius: if y == 0.0 { 46.0 } else { 56.0 },
                        })
                        .to_vec();
                }
                if !contact.pockets.is_empty() && ui.small_button("Remove").clicked() {
                    contact.pockets.clear();
                }
            });
            ui.end_row();
        });
    });
}

#[cfg(test)]
mod tests {
    use crate::inspector::{self, Inspector, Tab};
    use bpe_scene::{Container, GOOD_CONTACT_QUALITY, Rope, Scene, Shape, Vec2};
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        inspector: Inspector,
        scene: Scene,
    }

    fn harness(tab: Tab, scene: Scene) -> Harness<'static, State> {
        let state = State { inspector: Inspector { tab, ..Inspector::default() }, scene };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(460.0, 3000.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(240)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(|ui, state: &mut State| inspector::show(ui, &mut state.inspector, &mut state.scene), state);
        harness.run();
        harness
    }

    fn click(harness: &mut Harness<'_, State>, label: &str) {
        harness.get_by_label(label).click();
        harness.run();
    }

    #[test]
    fn turning_on_bumping_picks_the_steady_solver_and_presets_set_the_feel() {
        let mut harness = harness(Tab::Bounces, Scene::default());
        assert!(!harness.state().scene.physics.ball_collisions);
        click(&mut harness, "Balls bump into each other");
        let contact = &harness.state().scene.physics.contact;
        assert!(harness.state().scene.physics.ball_collisions);
        assert_eq!(contact.quality, GOOD_CONTACT_QUALITY);
        click(&mut harness, "Billiards");
        let contact = &harness.state().scene.physics.contact;
        assert!(contact.spin && contact.friction < 0.1);
        click(&mut harness, "Newton's cradle");
        assert_eq!(harness.state().scene.physics.restitution, 1.0);
        assert!(!harness.state().scene.physics.contact.spin);
    }

    #[test]
    fn old_scenes_keep_the_classic_solver_until_changed() {
        let mut scene = Scene::default();
        scene.physics.ball_collisions = true;
        let harness = harness(Tab::Bounces, scene);
        assert_eq!(harness.state().scene.physics.contact.quality, 0);
        harness.get_by_label("Stacking quality");
    }

    #[test]
    fn soft_things_are_added_copied_and_removed() {
        let mut harness = harness(Tab::Balls, Scene::default());
        click(&mut harness, "+ Jelly balls");
        click(&mut harness, "+ Rope");
        click(&mut harness, "+ Cloth");
        let bodies = &harness.state().scene.bodies;
        assert_eq!((bodies.jelly.len(), bodies.ropes.len(), bodies.cloths.len()), (1, 1, 1));
        harness.get_by_label("Squishiness");
        harness.get_by_label("Slack");
        // The rope's card: duplicate it, then remove the copy.
        // After the ball cards and the jelly's card.
        let rope = harness.state().scene.spawners.len() + 1;
        let duplicates: Vec<_> = harness.get_all_by_label("Duplicate").collect();
        duplicates[rope].click();
        harness.run();
        let ropes = &harness.state().scene.bodies.ropes;
        assert_eq!(ropes.len(), 2);
        assert_eq!(ropes[1].from, Rope::default().from + Vec2::new(60.0, 60.0));
        let jelly = harness.state().scene.spawners.len();
        let removes: Vec<_> = harness.get_all_by_label("Remove").collect();
        removes[jelly].click();
        harness.run();
        assert!(harness.state().scene.bodies.jelly.is_empty());
    }

    #[test]
    fn a_custom_arena_offers_drawing_importing_and_tracing() {
        let scene = Scene {
            shapes: vec![Shape::heart()],
            container: Container::Custom {
                shape: "Heart".into(),
                center: Vec2::ZERO,
                radius: 480.0,
                rotation: 0.0,
                thickness: 3.0,
                color: bpe_scene::Color::WHITE,
            },
            ..Scene::default()
        };
        let mut harness = harness(Tab::Arena, scene);
        harness.get_by_label("Import SVG…");
        harness.get_by_label("Trace picture…");
        harness.get_by_label("Outline");
        click(&mut harness, "Draw…");
        assert!(crate::draw_shape::active(&harness.ctx));
    }
}
