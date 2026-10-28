//! The Randomize panel: every part of the scene that has a dice, in one list, each with a lock.
//! "Roll everything unlocked" rolls new settings for every part that isn't locked, so the user
//! can freeze the parts they like and keep rolling the rest.
//!
//! Locks are shared with the lock buttons next to the dice in the tabs, through the egui
//! context, so locking a part in either place shows in both.

use crate::inspector::{ball_look, liquid, paint_dice, soft};
use crate::theme;
use bpe_render::EffectLibrary;
use bpe_scene::{Dice, Group, Scene};
use eframe::egui::{self, Color32, RichText, Sense, Ui};
use std::collections::HashSet;

/// A part of the scene that rolls as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Group(Group),
    /// The own look of a ball group that has one.
    BallLook(usize),
    Effect(usize),
    /// Every row of hit particles.
    Particles,
    Swarm,
    Liquid,
    Jelly(usize),
    Rope(usize),
    Cloth(usize),
}

impl Part {
    /// A number unique to the part, so each part rolls its own values from a shared seed and
    /// locking one part doesn't change what the others roll.
    fn salt(self) -> u64 {
        match self {
            Part::Group(group) => group.salt(),
            Part::Particles => 20,
            Part::Swarm => 21,
            Part::Liquid => 22,
            Part::BallLook(i) => 3000 + i as u64,
            Part::Effect(i) => 1000 + i as u64,
            Part::Jelly(i) => 4000 + i as u64,
            Part::Rope(i) => 5000 + i as u64,
            Part::Cloth(i) => 6000 + i as u64,
        }
    }
}

/// One heading of the panel and the parts under it, each with its name.
pub struct Section {
    pub name: &'static str,
    pub parts: Vec<(Part, String)>,
}

/// Every part of `scene` that can roll, grouped like the tabs. Parts that aren't in use (no
/// particles, swarm switched off) are left out.
pub fn sections(scene: &Scene, library: &EffectLibrary) -> Vec<Section> {
    let mut balls = Vec::new();
    for (i, spawner) in scene.spawners.iter().enumerate() {
        balls.push((Part::Group(Group::Balls(i)), format!("Balls {}: place, size, speed and colours", i + 1)));
        if spawner.style().is_some() {
            balls.push((Part::BallLook(i), format!("Balls {}: own look", i + 1)));
        }
    }
    if scene.gpu.swarm.enabled {
        balls.push((Part::Swarm, "Swarm".into()));
    }
    if scene.gpu.fluid.enabled || scene.bodies.fluid.count > 0 {
        balls.push((Part::Liquid, "Liquid".into()));
    }
    let bodies = &scene.bodies;
    balls.extend((0..bodies.jelly.len()).map(|i| (Part::Jelly(i), format!("Jelly balls {}", i + 1))));
    balls.extend((0..bodies.ropes.len()).map(|i| (Part::Rope(i), format!("Rope {}", i + 1))));
    balls.extend((0..bodies.cloths.len()).map(|i| (Part::Cloth(i), format!("Cloth {}", i + 1))));

    let group = |g: Group, name: &str| (Part::Group(g), name.to_string());
    let mut look = vec![
        group(Group::Background, "Background"),
        group(Group::Trails, "Trails"),
        group(Group::Glow, "Glow"),
        group(Group::Strings, "String art"),
    ];
    if !scene.particles.is_empty() {
        look.push((Part::Particles, "Hit particles".into()));
    }
    let effects = scene
        .effects
        .iter()
        .enumerate()
        .filter_map(|(i, effect)| {
            // Effects without sliders have nothing to roll.
            let info = library.get(&effect.shader)?;
            (!info.params.is_empty()).then(|| (Part::Effect(i), info.title.clone()))
        })
        .collect();
    vec![
        Section { name: "Balls", parts: balls },
        Section {
            name: "Arena",
            parts: vec![
                group(Group::Shape, "Arena shape"),
                group(Group::Movement, "Arena movement"),
                group(Group::World, "Gravity and bounciness"),
            ],
        },
        Section {
            name: "Bounces",
            parts: vec![group(Group::Bounces, "Each bounce"), group(Group::NewBalls, "New balls")],
        },
        Section { name: "Look", parts: look },
        Section { name: "Effects", parts: effects },
        Section {
            name: "Sound",
            parts: vec![
                group(Group::Chords, "Chords"),
                group(Group::Notes, "Notes"),
                group(Group::Instrument, "Instrument"),
            ],
        },
        Section { name: "Text", parts: vec![group(Group::Captions, "Captions")] },
    ]
    .into_iter()
    .filter(|s| !s.parts.is_empty())
    .collect()
}

/// Rolls new values for one part. See `bpe_scene::random` for `amount`.
pub fn roll(part: Part, scene: &mut Scene, dice: &mut Dice, amount: f32, library: &EffectLibrary) {
    match part {
        Part::Group(group) => scene.roll(group, dice, amount),
        Part::BallLook(i) => {
            if let Some(spawner) = scene.spawners.get_mut(i) {
                *spawner.style_mut() = Some(ball_look::random_style(dice));
            }
        }
        Part::Effect(i) => {
            if let Some(effect) = scene.effects.get_mut(i) {
                bpe_export::batch::roll_effect(effect, library, dice, amount);
            }
        }
        Part::Particles => scene.particles.iter_mut().for_each(|e| e.roll(dice, amount)),
        Part::Swarm => scene.gpu.swarm.roll(dice, amount),
        // Big liquid when it is on by itself, as in the Liquid section; otherwise small.
        Part::Liquid if scene.gpu.fluid.enabled && scene.bodies.fluid.count == 0 => scene.gpu.fluid.roll(dice, amount),
        Part::Liquid => liquid::roll_small(&mut scene.bodies.fluid, dice),
        Part::Jelly(i) => {
            if let Some(jelly) = scene.bodies.jelly.get_mut(i) {
                soft::roll_jelly(jelly, dice);
            }
        }
        Part::Rope(i) => {
            if let Some(rope) = scene.bodies.ropes.get_mut(i) {
                soft::roll_rope(rope, dice);
            }
        }
        Part::Cloth(i) => {
            if let Some(cloth) = scene.bodies.cloths.get_mut(i) {
                soft::roll_cloth(cloth, dice);
            }
        }
    }
}

/// Order parts roll in: the arena first, so balls rolled after it fit inside the new one.
fn order(part: &Part) -> u8 {
    match part {
        Part::Group(Group::Shape | Group::Movement | Group::World) => 0,
        _ => 1,
    }
}

/// Rolls every part of `scene` that isn't in `locked`, from one seed. Returns how many rolled.
pub fn roll_unlocked(
    scene: &mut Scene,
    locked: &HashSet<Part>,
    seed: u64,
    amount: f32,
    library: &EffectLibrary,
) -> usize {
    let mut parts: Vec<Part> = sections(scene, library)
        .into_iter()
        .flat_map(|s| s.parts.into_iter().map(|(part, _)| part))
        .filter(|part| !locked.contains(part))
        .collect();
    parts.sort_by_key(order);
    for &part in &parts {
        roll(part, scene, &mut Dice::mixed(seed, part.salt()), amount, library);
    }
    parts.len()
}

// ---------- locks, shared through the egui context ----------

fn locks_id() -> egui::Id {
    egui::Id::unique("dice-locks")
}

/// The parts that are locked.
pub fn locks(ctx: &egui::Context) -> HashSet<Part> {
    ctx.data(|d| d.get_temp::<HashSet<Part>>(locks_id())).unwrap_or_default()
}

pub fn is_locked(ctx: &egui::Context, part: Part) -> bool {
    ctx.data(|d| d.get_temp::<HashSet<Part>>(locks_id()).is_some_and(|l| l.contains(&part)))
}

pub fn set_locked(ctx: &egui::Context, part: Part, locked: bool) {
    ctx.data_mut(|d| {
        let locks = d.get_temp_mut_or_default::<HashSet<Part>>(locks_id());
        if locked {
            locks.insert(part);
        } else {
            locks.remove(&part);
        }
    });
}

/// A small padlock that locks or unlocks `part`, drawn closed and bright when locked.
pub fn lock_button(ui: &mut Ui, part: Part) -> egui::Response {
    let locked = is_locked(ui.ctx(), part);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 22.0), Sense::click());
    let response = response.on_hover_text(if locked {
        "Locked: \"Roll everything unlocked\" leaves this alone. Click to unlock."
    } else {
        "Lock this so \"Roll everything unlocked\" in the Randomize panel leaves it alone"
    });
    if response.hovered() {
        ui.painter().rect_filled(rect, 5.0, theme::HOVER);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if locked {
        theme::WARNING
    } else if response.hovered() {
        theme::TEXT
    } else {
        theme::MUTED.gamma_multiply(0.7)
    };
    paint_lock(ui.painter(), rect.center(), locked, color);
    if response.clicked() {
        set_locked(ui.ctx(), part, !locked);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, locked, "Keep when randomizing"));
    response
}

fn paint_lock(painter: &egui::Painter, center: egui::Pos2, locked: bool, color: Color32) {
    let body = egui::Rect::from_center_size(center + egui::vec2(0.0, 2.5), egui::vec2(11.0, 8.0));
    let stroke = egui::Stroke::new(1.4, color);
    if locked {
        painter.rect_filled(body, 2.0, color);
    } else {
        painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Middle);
    }
    // The shackle: an arch over the body, lifted and open on the right when unlocked.
    let lift = if locked { 0.0 } else { 2.5 };
    let (left, right) = (body.left() + 2.5, body.right() - 2.5);
    let top = body.top() - 5.0 - lift;
    let mut points = vec![egui::pos2(left, body.top())];
    for k in 0..=8 {
        let a = std::f32::consts::PI * (1.0 - k as f32 / 8.0);
        let r = (right - left) * 0.5;
        points.push(egui::pos2(left + r + r * a.cos(), top + 2.0 - 2.0 * a.sin()));
    }
    points.push(egui::pos2(right, if locked { body.top() } else { top + 3.0 }));
    painter.add(egui::Shape::line(points, stroke));
}

// ---------- the panel ----------

pub struct DicePanel {
    pub open: bool,
    /// How different each roll is, from 0 (a nudge) to 1 (brand new).
    amount: f32,
    rolls: u64,
}

impl Default for DicePanel {
    fn default() -> Self {
        Self { open: false, amount: 1.0, rolls: 0 }
    }
}

impl DicePanel {
    fn next_seed(&mut self) -> u64 {
        self.rolls += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        (nanos as u64) ^ self.rolls.wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }

    /// Shows the panel if it is open. Returns a status line when something rolled.
    pub fn show(&mut self, ctx: &egui::Context, scene: &mut Scene, library: &EffectLibrary) -> Option<String> {
        if !self.open {
            return None;
        }
        let mut rolled = None;
        let mut open = self.open;
        egui::Window::new("Randomize")
            .open(&mut open)
            .default_width(330.0)
            .default_pos(ctx.content_rect().right_top() + egui::vec2(-360.0, 60.0))
            .resizable(true)
            .show(ctx, |ui| rolled = self.contents(ui, scene, library));
        self.open = open;
        rolled
    }

    fn contents(&mut self, ui: &mut Ui, scene: &mut Scene, library: &EffectLibrary) -> Option<String> {
        let ctx = ui.ctx().clone();
        let sections = sections(scene, library);
        let all: Vec<Part> = sections.iter().flat_map(|s| s.parts.iter().map(|(p, _)| *p)).collect();
        let locked = locks(&ctx);
        let free = all.iter().filter(|p| !locked.contains(p)).count();
        let mut rolled = None;

        ui.label(
            RichText::new("Lock the parts you like, then roll the rest. Each roll is one undo step.")
                .color(theme::MUTED),
        );
        ui.add_space(6.0);
        let text = if free == all.len() {
            "Roll everything".to_string()
        } else {
            format!("Roll {free} unlocked of {}", all.len())
        };
        let button = egui::Button::new(RichText::new(format!("          {text}")).strong().color(Color32::WHITE))
            .fill(theme::ACCENT)
            .min_size(egui::vec2(ui.available_width(), 34.0));
        let response =
            ui.add_enabled(free > 0, button).on_hover_text("New random settings for every part that isn't locked");
        paint_dice(
            ui.painter(),
            egui::Rect::from_center_size(response.rect.left_center() + egui::vec2(20.0, 0.0), egui::vec2(22.0, 22.0)),
            false,
        );
        if response.clicked() {
            let count = roll_unlocked(scene, &locked, self.next_seed(), self.amount, library);
            rolled = Some(format!("Rolled {count} unlocked parts."));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("How different").on_hover_text(
                "Small numbers nudge the current settings; 100% rolls brand new ones. Ball looks always roll new.",
            );
            ui.add(
                egui::Slider::new(&mut self.amount, 0.05..=1.0)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                    .custom_parser(|s| s.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0)),
            );
        });
        ui.horizontal(|ui| {
            if ui.small_button("Lock all").clicked() {
                all.iter().for_each(|&p| set_locked(&ctx, p, true));
            }
            if ui.small_button("Unlock all").clicked() {
                all.iter().for_each(|&p| set_locked(&ctx, p, false));
            }
            ui.label(RichText::new(format!("{} locked", all.len() - free)).small().color(theme::MUTED));
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(ctx.content_rect().height() * 0.6)
            .min_scrolled_height(ctx.content_rect().height() * 0.45)
            .show(ui, |ui| {
                for section in &sections {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(section.name.to_uppercase()).size(12.0).color(theme::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let every = section.parts.iter().all(|(p, _)| locked.contains(p));
                            let label = if every { "Unlock" } else { "Lock" };
                            if ui
                                .small_button(label)
                                .on_hover_text(format!("{label} everything under {}", section.name))
                                .clicked()
                            {
                                section.parts.iter().for_each(|&(p, _)| set_locked(&ctx, p, !every));
                            }
                        });
                    });
                    for (part, name) in &section.parts {
                        ui.horizontal(|ui| {
                            lock_button(ui, *part);
                            let color = if locked.contains(part) { theme::MUTED } else { theme::TEXT };
                            ui.label(RichText::new(name).color(color));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if crate::inspector::dice_button(ui, &format!("Roll only {}", name.to_lowercase()))
                                    .clicked()
                                {
                                    roll(*part, scene, &mut Dice::new(self.next_seed()), self.amount, library);
                                    rolled = Some(format!("Rolled new settings for {}.", name.to_lowercase()));
                                }
                            });
                        });
                    }
                }
            });
        rolled.map(|r| format!("{r} Ctrl+Z puts them back."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene::presets().into_iter().find(|p| p.name == "Ball looks").map_or_else(Scene::default, |p| p.scene)
    }

    #[test]
    fn locked_parts_keep_their_settings() {
        let library = EffectLibrary::new();
        let base = scene();
        let locked: HashSet<Part> =
            [Part::Group(Group::Background), Part::Group(Group::Chords), Part::Group(Group::Bounces)].into();
        let mut rolled = base.clone();
        let count = roll_unlocked(&mut rolled, &locked, 42, 1.0, &library);
        assert!(count > 3);
        assert_ne!(rolled, base);
        assert_eq!(rolled.look.background_top, base.look.background_top);
        assert_eq!(rolled.look.background_bottom, base.look.background_bottom);
        assert_eq!(rolled.sound.progression, base.sound.progression);
        // "New balls" isn't locked and lives in the same settings, so check only the bounce ones.
        let bounce = |s: &Scene| (s.rules.speed_gain, s.rules.max_speed, s.rules.grow, s.rules.max_radius);
        assert_eq!(bounce(&rolled), bounce(&base));
        assert_eq!(rolled.rules.recolor, base.rules.recolor);
        assert_ne!(rolled.look.trail_seconds, base.look.trail_seconds);
    }

    #[test]
    fn same_seed_same_roll_and_locks_dont_shift_other_parts() {
        let library = EffectLibrary::new();
        let base = scene();
        let (mut a, mut b, mut c) = (base.clone(), base.clone(), base.clone());
        roll_unlocked(&mut a, &HashSet::new(), 7, 1.0, &library);
        roll_unlocked(&mut b, &HashSet::new(), 7, 1.0, &library);
        assert_eq!(a, b);
        roll_unlocked(&mut c, &[Part::Group(Group::Glow)].into(), 7, 1.0, &library);
        assert_eq!(c.look.trail_seconds, a.look.trail_seconds, "locking glow leaves the trail roll the same");
        assert_eq!(c.look.ball_glow, base.look.ball_glow);
    }

    #[test]
    fn everything_locked_rolls_nothing() {
        let library = EffectLibrary::new();
        let base = scene();
        let all: HashSet<Part> =
            sections(&base, &library).into_iter().flat_map(|s| s.parts.into_iter().map(|(p, _)| p)).collect();
        let mut rolled = base.clone();
        assert_eq!(roll_unlocked(&mut rolled, &all, 3, 1.0, &library), 0);
        assert_eq!(rolled, base);
    }

    #[test]
    fn ball_looks_only_listed_for_groups_with_their_own_look() {
        let library = EffectLibrary::new();
        let mut scene = scene();
        scene.spawners.truncate(1);
        *scene.spawners[0].style_mut() = None;
        let parts: Vec<Part> =
            sections(&scene, &library).into_iter().flat_map(|s| s.parts.into_iter().map(|(p, _)| p)).collect();
        assert!(!parts.contains(&Part::BallLook(0)));
        let mut rolled = scene.clone();
        roll_unlocked(&mut rolled, &HashSet::new(), 5, 1.0, &library);
        assert!(rolled.spawners[0].style().is_none(), "classic balls stay classic");
    }

    #[test]
    fn locks_are_shared_through_the_context() {
        let ctx = egui::Context::default();
        let part = Part::Effect(2);
        assert!(!is_locked(&ctx, part));
        set_locked(&ctx, part, true);
        assert!(is_locked(&ctx, part));
        assert_eq!(locks(&ctx), [part].into());
        set_locked(&ctx, part, false);
        assert!(locks(&ctx).is_empty());
    }
}
