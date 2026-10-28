//! Hit particles in the Look tab: one row per kind of event, each with a particle style.

use super::{color_list, color_row, dice_button, grid, label, more, slider, units};
use crate::theme;
use bpe_scene::{
    Blend, Color, Dice, Emitter, EventMatch, ParticleColor, ParticlePreset, Scene, particle_hash,
};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Ui, Vec2};

/// The event kinds that get a row, in order.
const ROWS: [(EventMatch, &str); 9] = [
    (EventMatch::AnyHit, "On every hit of any kind: walls, pegs and other balls."),
    (EventMatch::Wall, "When a ball hits the arena wall."),
    (EventMatch::Obstacle, "When a ball hits a peg or a line."),
    (EventMatch::Escape, "When a ball escapes through a ring's gap."),
    (EventMatch::Ball, "When two balls bump into each other. Turn on \"Balls bump into each other\" in Bounces."),
    (EventMatch::Soft, "When a jelly ball, rope or cloth touches something."),
    (EventMatch::Spawn, "When a new ball appears."),
    (EventMatch::Beat, "On every beat of the song."),
    (EventMatch::Action, "When a rule's \"Burst particles\" block fires. Rules can also name a style directly."),
];

/// Panel state for this section that is not part of the scene.
#[derive(Default)]
pub struct State {
    /// The emitter whose settings are open, by its place in `Scene::particles`.
    open: Option<usize>,
    rolls: u64,
}

impl State {
    fn dice(&mut self) -> Dice {
        self.rolls += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        Dice::new(nanos as u64 ^ self.rolls.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }
}

/// Draws the section. Edits go straight into `scene`; the app keeps them as undo steps.
pub fn section(ui: &mut Ui, scene: &mut Scene, state: &mut State) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("HIT PARTICLES").size(12.0).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if dice_button(ui, "Roll random particles for every row that has some").clicked() {
                let mut dice = state.dice();
                for emitter in &mut scene.particles {
                    emitter.roll(&mut dice, 1.0);
                }
            }
        });
    });
    ui.label(
        RichText::new("Sparks, smoke or confetti that burst out where things happen. Click a row's name for its settings.")
            .small()
            .color(theme::MUTED),
    );
    ui.add_space(4.0);

    let mut remove = None;
    for (kind, help) in ROWS {
        let found: Vec<usize> =
            scene.particles.iter().enumerate().filter(|(_, e)| e.on.kind == kind).map(|(i, _)| i).collect();
        if found.is_empty() {
            event_row(ui, scene, state, kind, help, None, &mut remove);
        }
        for index in found {
            event_row(ui, scene, state, kind, help, Some(index), &mut remove);
            if state.open == Some(index) {
                settings(ui, &mut scene.particles[index], index);
            }
        }
    }
    if let Some(index) = remove {
        scene.particles.remove(index);
        state.open = None;
    }
}

/// One row: the event's name, its particle style and a dice. `index` is its emitter, if any.
fn event_row(
    ui: &mut Ui,
    scene: &mut Scene,
    state: &mut State,
    kind: EventMatch,
    help: &str,
    index: Option<usize>,
    remove: &mut Option<usize>,
) {
    let id = ui.scope_id().with(("particle row", kind, index));
    ui.horizontal(|ui| {
        let open = index.is_some() && state.open == index;
        let name = match index.map(|i| &scene.particles[i]) {
            Some(e) if kind == EventMatch::Action && !e.on.action.is_empty() => format!("Rule: {}", e.on.action),
            _ => kind.name().to_string(),
        };
        let text = RichText::new(name).color(theme::TEXT);
        let response = ui
            .add_sized([118.0, 22.0], egui::Button::selectable(open, text).frame_when_inactive(false))
            .on_hover_text(format!("{help} Right-click to add another style."));
        if response.clicked() && index.is_some() {
            state.open = if open { None } else { index };
        }
        response.context_menu(|ui| {
            if ui.button("Add another style").clicked() {
                scene.particles.push(ParticlePreset::Sparks.emitter(kind));
                state.open = Some(scene.particles.len() - 1);
                ui.close();
            }
            if let Some(i) = index
                && ui.button("Remove").clicked()
            {
                *remove = Some(i);
                ui.close();
            }
        });
        let current = index.map(|i| scene.particles[i].preset);
        let enabled = index.is_none_or(|i| scene.particles[i].enabled);
        let shown = if enabled { current } else { None };
        let what = format!("Particle style for {}", kind.name().to_lowercase());
        egui::ComboBox::from_id_salt(id)
            .width(120.0)
            .selected_text(match shown {
                Some(preset) => preset.name(),
                None if current.is_some() => "Off",
                None => "None",
            })
            .show_ui(ui, |ui| {
                if ui.selectable_label(shown.is_none(), "None").clicked()
                    && let Some(i) = index
                {
                    *remove = Some(i);
                }
                for preset in ParticlePreset::ALL {
                    ui.horizontal(|ui| {
                        preset_icon(ui, preset, 18.0);
                        if ui.selectable_label(shown == Some(preset), preset.name()).clicked() {
                            match index {
                                Some(i) => {
                                    let keep = scene.particles[i].on.clone();
                                    scene.particles[i] = Emitter { on: keep, ..preset.emitter(kind) };
                                }
                                None => {
                                    scene.particles.push(preset.emitter(kind));
                                    state.open = Some(scene.particles.len() - 1);
                                }
                            }
                        }
                    });
                }
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &what));
        if let Some(preset) = shown {
            preset_icon(ui, preset, 20.0);
        }
        if let Some(i) = index {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if dice_button(ui, "Roll random settings for these particles").clicked() {
                    let mut dice = state.dice();
                    scene.particles[i].roll(&mut dice, 1.0);
                }
                let e = &mut scene.particles[i];
                ui.checkbox(&mut e.enabled, "On").on_hover_text("Show these particles");
            });
        }
    });
}

fn settings(ui: &mut Ui, emitter: &mut Emitter, index: usize) {
    let d = emitter.preset.emitter(emitter.on.kind);
    egui::Frame::new()
        .fill(theme::CARD)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            grid(ui, &format!("particles-{index}"), |ui| {
                slider(ui, "Amount", "Particles per event, at full impact.", &mut emitter.count, d.count, 1..=400, |s| s);
                slider(ui, "Speed", "How fast particles fly out.", &mut emitter.speed, d.speed, 0.0..=2000.0, units(" px/s", 0));
                slider(ui, "Size", "Particle size in pixels.", &mut emitter.size, d.size, 0.5..=120.0, units(" px", 1));
                slider(ui, "Lasts", "How long each particle lasts.", &mut emitter.life, d.life, 0.05..=6.0, units(" s", 2));
                label(ui, "Colour", "Where the particles get their colour.");
                ui.horizontal(|ui| {
                    let mode = color_mode(&emitter.color);
                    for (k, name, help) in [
                        (0, "Ball", "The colour of the ball that caused it."),
                        (1, "Wall", "The colour of what was hit."),
                        (2, "One", "One colour of your choice."),
                        (3, "Mix", "Each particle picks from a list of colours."),
                    ] {
                        if ui.selectable_label(mode == k, name).on_hover_text(help).clicked() && mode != k {
                            emitter.color = match k {
                                0 => ParticleColor::FromBall,
                                1 => ParticleColor::Wall,
                                2 => ParticleColor::Fixed(Color::rgb(0xff, 0xf2, 0xc0)),
                                _ => ParticleColor::Gradient(vec![
                                    Color::rgb(0xff, 0x3e, 0x5e),
                                    Color::rgb(0xff, 0xd8, 0x2e),
                                    Color::rgb(0x2e, 0xb8, 0xff),
                                ]),
                            };
                        }
                    }
                });
                ui.end_row();
                match &mut emitter.color {
                    ParticleColor::Fixed(c) => color_row(ui, "Pick", "The particles' colour.", c, Color::rgb(0xff, 0xf2, 0xc0)),
                    ParticleColor::Gradient(colors) => {
                        label(ui, "Colours", "Each particle picks a colour along these.");
                        color_list(ui, colors, "+");
                        ui.end_row();
                    }
                    _ => {}
                }
                label(ui, "Bounce off walls", "Particles bounce off walls and pegs instead of flying through.");
                ui.checkbox(&mut emitter.bounce, "Bounce");
                ui.end_row();
            });
            more(ui, &format!("particles-more-{index}"), |ui| {
                grid(ui, &format!("particles-more-grid-{index}"), |ui| {
                    slider(ui, "Spread", "How widely particles fan out. 360° sends them every way.", &mut emitter.spread, d.spread, 0.0..=360.0, units("°", 0));
                    slider(ui, "Gravity", "Pull on the particles. Negative floats them up, like smoke.", &mut emitter.gravity, d.gravity, -1000.0..=2000.0, units(" px/s²", 0));
                    slider(ui, "Air drag", "How quickly particles slow down.", &mut emitter.drag, d.drag, 0.0..=10.0, units("", 2));
                    label(ui, "Glow", "Glowing particles add light; the others cover what is behind them, like paper or smoke.");
                    let mut glow = emitter.blend == Blend::Add;
                    if ui.checkbox(&mut glow, "Add light").changed() {
                        emitter.blend = if glow { Blend::Add } else { Blend::Alpha };
                    }
                    ui.end_row();
                    label(ui, "Follow impact", "Gentle hits burst fewer, slower particles than hard ones.");
                    ui.checkbox(&mut emitter.follow_impact, "Harder hits, bigger bursts");
                    ui.end_row();
                    if emitter.on.kind == EventMatch::Action {
                        label(ui, "Rule burst name", "Only bursts with this name, as set in the rule. Empty answers every burst.");
                        ui.text_edit_singleline(&mut emitter.on.action);
                        ui.end_row();
                    }
                    label(ui, "Only ball", "Only events of this ball, by its number. Empty is every ball.");
                    let mut text = emitter.on.ball.map(|b| b.to_string()).unwrap_or_default();
                    if ui.add(egui::TextEdit::singleline(&mut text).desired_width(60.0)).changed() {
                        emitter.on.ball = text.trim().parse().ok();
                    }
                    ui.end_row();
                });
            });
        });
    ui.add_space(4.0);
}

fn color_mode(color: &ParticleColor) -> u8 {
    match color {
        ParticleColor::FromBall => 0,
        ParticleColor::Wall => 1,
        ParticleColor::Fixed(_) => 2,
        ParticleColor::Gradient(_) => 3,
    }
}

/// A tiny looping animation of a particle style, so the list shows what each looks like.
pub fn preset_icon(ui: &mut Ui, preset: ParticlePreset, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter_at(rect);
    let time = ui.input(|i| i.time) as f32;
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    let c = rect.center();
    let r = size * 0.5;
    let t = (time * 0.8).fract();
    let colors = [
        Color32::from_rgb(0xff, 0x3e, 0x5e),
        Color32::from_rgb(0xff, 0xd8, 0x2e),
        Color32::from_rgb(0x5c, 0xe6, 0x5c),
        Color32::from_rgb(0x2e, 0xb8, 0xff),
    ];
    for k in 0..6u32 {
        let angle = particle_hash(k, 1) * std::f32::consts::TAU;
        let dir = Vec2::angled(angle);
        let reach = r * (0.25 + 0.75 * t);
        let p: Pos2 = c + dir * reach;
        let fade = ((1.0 - t) * 255.0) as u8;
        match preset {
            ParticlePreset::Sparks => {
                painter.line_segment([c + dir * reach * 0.5, p], egui::Stroke::new(1.2, Color32::from_rgba_unmultiplied(255, 220, 140, fade)));
            }
            ParticlePreset::Smoke if k < 3 => {
                painter.circle_filled(c + dir * reach * 0.4, r * (0.3 + 0.4 * t), Color32::from_rgba_unmultiplied(190, 195, 210, fade / 3));
            }
            ParticlePreset::Confetti => {
                let flutter = (time * 6.0 + k as f32).cos().abs().max(0.2);
                let rect = egui::Rect::from_center_size(p + Vec2::new(0.0, t * r * 0.4), Vec2::new(2.6 * flutter, 1.8));
                painter.rect_filled(rect, 0.0, colors[k as usize % 4]);
            }
            ParticlePreset::Stars if k < 4 => {
                let s = 2.0 + (time * 9.0 + k as f32).sin();
                let star = Color32::from_rgba_unmultiplied(255, 242, 192, fade);
                painter.line_segment([p - Vec2::X * s, p + Vec2::X * s], egui::Stroke::new(1.0, star));
                painter.line_segment([p - Vec2::Y * s, p + Vec2::Y * s], egui::Stroke::new(1.0, star));
            }
            ParticlePreset::Shards => {
                let side = dir.rot90() * 1.2;
                let tip = p + dir * 2.5;
                painter.add(egui::Shape::convex_polygon(vec![p - side, tip, p + side], colors[(k as usize + 1) % 4], egui::Stroke::NONE));
            }
            ParticlePreset::Rings if k == 0 => {
                painter.circle_stroke(c, reach, egui::Stroke::new(1.2, Color32::from_rgba_unmultiplied(160, 200, 255, fade)));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::inspector::{Tab, gpu::tests::harness};
    use bpe_scene::{EventMatch, ParticlePreset, Scene};
    use egui_kittest::kittest::Queryable;

    #[test]
    fn every_event_kind_gets_a_row() {
        let harness = harness(Tab::Look, Scene::default());
        for (kind, _) in super::ROWS {
            harness.get_by_label(kind.name());
        }
    }

    #[test]
    fn a_row_opens_its_settings_and_they_reset() {
        let mut scene = Scene::default();
        let mut emitter = ParticlePreset::Sparks.emitter(EventMatch::Wall);
        emitter.count = 99;
        scene.particles.push(emitter);
        let mut harness = harness(Tab::Look, scene);
        assert!(harness.query_by_label("Amount").is_none());
        harness.get_by_label(EventMatch::Wall.name()).click();
        harness.run();
        harness.get_by_label("Amount").click();
        harness.step();
        harness.get_by_label("Amount").click();
        harness.run();
        assert_eq!(harness.state().1.particles[0].count, ParticlePreset::Sparks.emitter(EventMatch::Wall).count);
        harness.get_by_label("Roll random settings for these particles").click();
        harness.run();
        assert_ne!(harness.state().1.particles[0], ParticlePreset::Sparks.emitter(EventMatch::Wall));
    }
}
