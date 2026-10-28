//! Each group of balls' own look (Balls tab): shape, fill, outline, glow, trail and motion.

use super::{color_row, dice_button, grid, label, percent, slider, units};
use crate::theme;
use bpe_scene::{BallFill, BallShape, BallStyle, Color, Dice, Look, Spawner, TrailColor};
use eframe::egui::{self, RichText, Ui};

/// Panel state for ball looks that is not part of the scene.
#[derive(Default)]
pub struct State {
    /// Ball groups picked with Ctrl or Shift held. A change to the look of one of them is made to
    /// all of them.
    pub picked: Vec<usize>,
    /// A look copied with "Copy look", ready to paste onto other balls.
    pub copied: Option<BallStyle>,
    pub(crate) rolls: u64,
}

impl State {
    fn dice(&mut self) -> Dice {
        self.rolls += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        Dice::new((nanos as u64) ^ self.rolls.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }

    /// Keeps the picked groups in step with the list: drops ones that are gone, and makes sure the
    /// selected group is among them.
    pub fn sync(&mut self, selected: Option<usize>, count: usize) {
        self.picked.retain(|&i| i < count);
        if let Some(index) = selected
            && !self.picked.contains(&index)
        {
            self.picked = vec![index];
        }
        if selected.is_none() && self.picked.len() == 1 {
            self.picked.clear();
        }
    }

    /// A click on a ball group's card. With Ctrl or Shift held it adds the group to the picked
    /// ones, or takes it out again; a plain click picks only that group.
    pub fn click(&mut self, ui: &Ui, index: usize, selected: &mut Option<usize>) {
        let adding = ui.input(|i| i.modifiers.command || i.modifiers.shift);
        if adding {
            if let Some(at) = self.picked.iter().position(|&i| i == index) {
                if self.picked.len() > 1 {
                    self.picked.remove(at);
                    *selected = self.picked.last().copied();
                }
            } else {
                self.picked.push(index);
                *selected = Some(index);
            }
        } else {
            self.picked = vec![index];
            *selected = Some(index);
        }
    }

    /// After the cards are drawn: makes a change to one picked group's look to the other picked
    /// groups too, setting only what changed so their other settings stay their own.
    pub fn share_changes(&self, before: &[Option<BallStyle>], spawners: &mut [Spawner]) {
        if self.picked.len() < 2 {
            return;
        }
        let edited = self
            .picked
            .iter()
            .copied()
            .find(|&i| spawners.get(i).is_some_and(|s| before.get(i).is_some_and(|b| b.as_ref() != s.style())));
        let Some(edited) = edited else { return };
        let (old, new) = (before[edited].clone(), spawners[edited].style().cloned());
        for &i in self.picked.iter().filter(|&&i| i != edited) {
            let Some(spawner) = spawners.get_mut(i) else { continue };
            match (&old, &new) {
                (Some(old), Some(new)) => match spawner.style_mut() {
                    Some(style) => style.copy_changes(old, new),
                    // A classic group follows along by taking the new look whole.
                    none => *none = Some(new.clone()),
                },
                // Switched to its own look, or back to the classic one.
                _ => *spawner.style_mut() = new.clone(),
            }
        }
    }
}

pub fn shape_name(shape: BallShape) -> &'static str {
    match shape {
        BallShape::Circle => "Circle",
        BallShape::Square => "Square",
        BallShape::Triangle => "Triangle",
        BallShape::Diamond => "Diamond",
        BallShape::Hexagon => "Hexagon",
        BallShape::Star => "Star",
        BallShape::Heart => "Heart",
        BallShape::Donut => "Ring",
    }
}

const SHAPES: [BallShape; 8] = [
    BallShape::Circle,
    BallShape::Square,
    BallShape::Triangle,
    BallShape::Diamond,
    BallShape::Hexagon,
    BallShape::Star,
    BallShape::Heart,
    BallShape::Donut,
];

pub fn fill_name(fill: BallFill) -> &'static str {
    match fill {
        BallFill::Solid => "Solid colour",
        BallFill::Shiny => "Shiny (3D)",
        BallFill::Radial => "Glow from the middle",
        BallFill::Linear => "Top to bottom fade",
        BallFill::Stripes => "Stripes",
        BallFill::Rings => "Rings",
        BallFill::Dots => "Dots",
        BallFill::Checker => "Checkers",
        BallFill::Halves => "Two halves",
        BallFill::Slices => "Beach ball slices",
        BallFill::Swirl => "Swirl",
        BallFill::Face => "Smiley face",
    }
}

const FILLS: [BallFill; 12] = [
    BallFill::Solid,
    BallFill::Shiny,
    BallFill::Radial,
    BallFill::Linear,
    BallFill::Stripes,
    BallFill::Rings,
    BallFill::Dots,
    BallFill::Checker,
    BallFill::Halves,
    BallFill::Slices,
    BallFill::Swirl,
    BallFill::Face,
];

fn uses_second_color(fill: BallFill) -> bool {
    !matches!(fill, BallFill::Solid | BallFill::Shiny)
}

fn has_pattern_size(fill: BallFill) -> bool {
    matches!(
        fill,
        BallFill::Stripes | BallFill::Rings | BallFill::Dots | BallFill::Checker | BallFill::Slices | BallFill::Swirl
    )
}

fn trail_color_name(color: TrailColor) -> &'static str {
    match color {
        TrailColor::Ball => "Ball colour",
        TrailColor::Second => "Second colour",
        TrailColor::Outline => "Outline colour",
    }
}

/// Names a drop-down list for screen readers.
fn named(response: &egui::Response, name: &str) {
    let enabled = response.enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, name));
}

/// A random look, keeping the balls' own colours.
pub fn random_style(dice: &mut Dice) -> BallStyle {
    let shape = if dice.chance(0.6) { BallShape::Circle } else { dice.pick(&SHAPES) };
    let fill = dice.pick(&FILLS);
    let second_color = Color::from_hsv(dice.unit(), dice.range(0.0, 0.8), dice.range(0.2, 1.0));
    BallStyle {
        shape,
        roundness: dice.range(0.0, 0.5),
        fill,
        second_color,
        pattern_size: dice.int(2, 10) as f32,
        turn: dice.range(0.0, 360.0).round(),
        spin: if dice.chance(0.4) { (dice.range(30.0, 240.0) * dice.sign()).round() } else { 0.0 },
        face_motion: shape != BallShape::Circle && dice.chance(0.3),
        outline: if dice.chance(0.5) { dice.range(1.0, 5.0).round() } else { 0.0 },
        outline_color: if dice.chance(0.5) { Color::WHITE } else { Color::from_hsv(dice.unit(), 0.7, 1.0) },
        glow: if dice.chance(0.5) { dice.range(0.1, 0.7) } else { 0.0 },
        glow_size: dice.range(0.4, 1.2),
        opacity: if dice.chance(0.8) { 1.0 } else { dice.range(0.5, 1.0) },
        stretch: if dice.chance(0.4) { dice.range(0.1, 0.7) } else { 0.0 },
        rainbow: if dice.chance(0.2) { dice.range(0.05, 0.5) } else { 0.0 },
        trail_seconds: if dice.chance(0.5) { dice.range(0.1, 0.8) } else { 0.0 },
        trail_width: dice.range(0.2, 1.0),
        trail_opacity: dice.range(0.3, 0.8),
        trail_color: dice.pick(&[TrailColor::Ball, TrailColor::Ball, TrailColor::Second, TrailColor::Outline]),
    }
}

/// The Look part of a ball group's card.
pub fn section(ui: &mut Ui, index: usize, style: &mut Option<BallStyle>, look: &Look, state: &mut State) {
    ui.add_space(6.0);
    let picked = state.picked.len();
    let header = if picked > 1 && state.picked.contains(&index) {
        format!("Look (changes go to all {picked} picked groups)")
    } else {
        "Look".to_string()
    };
    egui::CollapsingHeader::new(RichText::new(header).strong().color(theme::ACCENT_TEXT))
        .id_salt(("ball-look", index))
        .default_open(true)
        .show(ui, |ui| body(ui, style, look, state));
}

fn body(ui: &mut Ui, style: &mut Option<BallStyle>, look: &Look, state: &mut State) {
    ui.horizontal_wrapped(|ui| {
        let presets = BallStyle::presets();
        let shown = match style {
            None => "Classic (follows the Look tab)",
            Some(current) => presets.iter().find(|p| &p.style == current).map_or("Custom look", |p| p.name),
        };
        let preset = egui::ComboBox::from_id_salt("look-preset").selected_text(shown).width(200.0).show_ui(ui, |ui| {
            if ui
                .selectable_label(style.is_none(), "Classic (follows the Look tab)")
                .on_hover_text("Shiny balls whose glow and trail follow the Look tab, like the reference reel.")
                .clicked()
            {
                *style = None;
            }
            for preset in presets {
                if ui.selectable_label(style.as_ref() == Some(&preset.style), preset.name).clicked() {
                    *style = Some(preset.style);
                }
            }
        });
        named(&preset.response, "Ready-made look");
        if dice_button(ui, "Roll a random look for these balls").clicked() {
            *style = Some(random_style(&mut state.dice()));
        }
        if let Some(current) = style.as_ref()
            && ui.small_button("Copy look").on_hover_text("Copy this look to paste onto other balls").clicked()
        {
            state.copied = Some(current.clone());
        }
        if let Some(copied) = state.copied.clone()
            && ui.small_button("Paste look").on_hover_text("Give these balls the look you copied").clicked()
        {
            *style = Some(copied);
        }
    });
    let Some(s) = style.as_mut() else {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("These balls use the classic look.").color(theme::MUTED));
            if ui
                .button("Customise")
                .on_hover_text("Give these balls their own look, starting from how they look now")
                .clicked()
            {
                *style = Some(BallStyle::like_classic(look));
            }
        });
        return;
    };
    let d = BallStyle::default();
    grid(ui, "look-grid", |ui| {
        label(ui, "Shape", "What the balls are drawn as. They still bounce like round balls of their size.");
        let combo = egui::ComboBox::from_id_salt("look-shape").selected_text(shape_name(s.shape)).width(160.0).show_ui(
            ui,
            |ui| {
                for shape in SHAPES {
                    ui.selectable_value(&mut s.shape, shape, shape_name(shape));
                }
            },
        );
        named(&combo.response, "Shape");
        ui.end_row();
        if !matches!(s.shape, BallShape::Circle | BallShape::Donut | BallShape::Heart) {
            slider(
                ui,
                "Round corners",
                "How rounded the shape's corners are.",
                &mut s.roundness,
                d.roundness,
                0.0..=1.0,
                percent,
            );
        }
        label(ui, "Fill", "What fills the shape. Patterns use the ball's colour and the second colour.");
        let combo =
            egui::ComboBox::from_id_salt("look-fill").selected_text(fill_name(s.fill)).width(160.0).show_ui(ui, |ui| {
                for fill in FILLS {
                    ui.selectable_value(&mut s.fill, fill, fill_name(fill));
                }
            });
        named(&combo.response, "Fill");
        ui.end_row();
        if uses_second_color(s.fill) {
            color_row(ui, "Second colour", "The pattern's other colour.", &mut s.second_color, d.second_color);
        }
        if has_pattern_size(s.fill) {
            slider(
                ui,
                "Pattern size",
                "How many stripes, rings, dots or slices fit across a ball.",
                &mut s.pattern_size,
                d.pattern_size,
                1.0..=20.0,
                units("", 1),
            );
        }
        slider(
            ui,
            "Outline",
            "Width of a line around the edge of each ball, in pixels. 0 means no outline.",
            &mut s.outline,
            d.outline,
            0.0..=20.0,
            units(" px", 1),
        );
        if s.outline > 0.0 {
            color_row(ui, "Outline colour", "", &mut s.outline_color, d.outline_color);
        }
        slider(ui, "Glow", "A soft halo of light around each ball.", &mut s.glow, d.glow, 0.0..=2.0, percent);
        if s.glow > 0.0 {
            slider(
                ui,
                "Glow size",
                "How far the halo reaches, compared with the ball's size.",
                &mut s.glow_size,
                d.glow_size,
                0.1..=2.0,
                percent,
            );
        }
        slider(
            ui,
            "Opacity",
            "How solid the balls are. Lower lets you see through them.",
            &mut s.opacity,
            d.opacity,
            0.0..=1.0,
            percent,
        );
        slider(
            ui,
            "Trail",
            "How long a streak each ball leaves behind it, in seconds. 0 means no trail.",
            &mut s.trail_seconds,
            d.trail_seconds,
            0.0..=3.0,
            units(" s", 2),
        );
    });
    egui::CollapsingHeader::new(RichText::new("More look settings").color(theme::ACCENT_TEXT))
        .id_salt("look-more")
        .default_open(false)
        .show(ui, |ui| {
            grid(ui, "look-more-grid", |ui| {
                slider(
                    ui,
                    "Turn",
                    "Starting angle of the shape and pattern.",
                    &mut s.turn,
                    d.turn,
                    0.0..=360.0,
                    units("°", 0),
                );
                slider(
                    ui,
                    "Spin",
                    "How fast the shape and pattern turn, in degrees per second. Negative spins the other way.",
                    &mut s.spin,
                    d.spin,
                    -720.0..=720.0,
                    units("°/s", 0),
                );
                label(ui, "Point where it's going", "Turns the shape so its top points the way the ball is moving.");
                ui.checkbox(&mut s.face_motion, "");
                ui.end_row();
                slider(
                    ui,
                    "Stretch with speed",
                    "Fast balls stretch out along their path, like cartoon motion.",
                    &mut s.stretch,
                    d.stretch,
                    0.0..=1.0,
                    percent,
                );
                slider(
                    ui,
                    "Rainbow",
                    "Cycles the ball's colours around the colour wheel. Full turns per second.",
                    &mut s.rainbow,
                    d.rainbow,
                    -2.0..=2.0,
                    units("/s", 2),
                );
                if s.trail_seconds > 0.0 {
                    slider(
                        ui,
                        "Trail width",
                        "Streak width at the ball, compared with the ball's size.",
                        &mut s.trail_width,
                        d.trail_width,
                        0.05..=2.0,
                        percent,
                    );
                    slider(
                        ui,
                        "Trail brightness",
                        "How bright the streak is at the ball. It fades towards its end.",
                        &mut s.trail_opacity,
                        d.trail_opacity,
                        0.0..=1.0,
                        percent,
                    );
                    label(ui, "Trail colour", "");
                    let combo = egui::ComboBox::from_id_salt("look-trail-color")
                        .selected_text(trail_color_name(s.trail_color))
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for color in [TrailColor::Ball, TrailColor::Second, TrailColor::Outline] {
                                ui.selectable_value(&mut s.trail_color, color, trail_color_name(color));
                            }
                        });
                    named(&combo.response, "Trail colour");
                    ui.end_row();
                }
            });
        });
}
