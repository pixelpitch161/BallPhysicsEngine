//! Grade mode (colour grading, `docs/notes/grading.md`): the editor's second face, for the
//! scene's Grade card. Build | Grade in the toolbar (or D) switches. In Grade mode the left
//! column lists the masks (part 1: the Whole picture) and the Histogram, the right column the
//! mask's panels in the order they work, and the preview compares After, Before and Split.
//!
//! - `cx.rs`: reading the card and applying edits; `panels.rs`: the panel column;
//! - `curve.rs`: the Tone Curve's graph; `scopes.rs`: the Histogram.
//!
//! Everything here but the card itself is the editor's own: never saved, never exported.

use crate::app::Studio;
use crate::style;
use bpe_core::{LayerId, Slot};
use eframe::egui::{self, RichText, Ui};
use std::collections::{BTreeMap, BTreeSet};

pub mod curve;
pub mod cx;
pub mod panels;
pub mod scopes;
#[cfg(test)]
mod tests;

/// The editor's two faces: building the scene, or grading its picture. Not saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorMode {
    #[default]
    Build,
    Grade,
}

/// What the preview shows in Grade mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compare {
    /// The graded picture.
    #[default]
    After,
    /// The picture without the grade.
    Before,
    /// Without the grade left of a line, graded right of it.
    Split,
}

/// Which of a Tone Curve's point curves is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    #[default]
    Rgb,
    Red,
    Green,
    Blue,
}

impl Channel {
    pub const ALL: [Channel; 4] = [Channel::Rgb, Channel::Red, Channel::Green, Channel::Blue];

    /// The panel's setting holding its points.
    pub fn key(self) -> &'static str {
        match self {
            Channel::Rgb => "rgb",
            Channel::Red => "red",
            Channel::Green => "green",
            Channel::Blue => "blue",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Channel::Rgb => "RGB",
            Channel::Red => "Red",
            Channel::Green => "Green",
            Channel::Blue => "Blue",
        }
    }
}

/// Grade mode's own state.
pub struct GradeMode {
    pub compare: Compare,
    /// Where the Split line is, across the picture (0..1).
    pub split: f32,
    /// The Split line is being dragged.
    pub split_drag: bool,
    /// Panels folded shut.
    pub folded: BTreeSet<LayerId>,
    /// The point curve each Tone Curve shows.
    pub channel: BTreeMap<LayerId, Channel>,
    /// The panel whose header is being dragged to a new place.
    pub dragging: Option<LayerId>,
    /// Where each panel's header was drawn last frame, in order, for dragging.
    pub headers: Vec<(LayerId, egui::Rect)>,
    /// The curve point being dragged: panel, channel, point.
    pub curve_drag: Option<(LayerId, Channel, usize)>,
    /// The Histogram's latest counts.
    pub bins: Option<Box<bpe_gfx::grade::HistogramBins>>,
    /// The picture changed since the last count was asked for: count again when that one is in.
    pub recount: bool,
    /// The card the column shows before the scene has one.
    start: Option<Slot>,
}

impl Default for GradeMode {
    fn default() -> Self {
        Self {
            compare: Compare::After,
            split: 0.5,
            split_drag: false,
            folded: BTreeSet::new(),
            channel: BTreeMap::new(),
            dragging: None,
            headers: Vec::new(),
            curve_drag: None,
            bins: None,
            recount: false,
            start: None,
        }
    }
}

impl GradeMode {
    /// The starting card (`bpe_kit::modules::grade::starting_card`), made once.
    pub fn start(&mut self, reg: &bpe_core::Registry) -> Slot {
        self.start.get_or_insert_with(|| bpe_kit::modules::grade::starting_card(reg)).clone()
    }

    /// The Before view on or off (`\`): from Before back to After.
    pub fn toggle_before(&mut self) {
        self.compare = if self.compare == Compare::Before { Compare::After } else { Compare::Before };
    }

    /// The Split view on or off (Y).
    pub fn toggle_split(&mut self) {
        self.compare = if self.compare == Compare::Split { Compare::After } else { Compare::Split };
    }

    /// The preview's grade as Grade mode shows it: cleared for Before, split for Split.
    pub fn show_on(&self, grade: &mut bpe_gfx::GradeStack) {
        match self.compare {
            Compare::After => {}
            Compare::Before => *grade = Default::default(),
            Compare::Split => {
                if !grade.is_identity() {
                    grade.split = Some(self.split.clamp(0.0, 1.0));
                }
            }
        }
    }
}

/// The Build | Grade switch, for the toolbar.
pub fn mode_switch(ui: &mut Ui, mode: &mut EditorMode) {
    let frame = egui::Frame::new().stroke(egui::Stroke::new(1.0, style::LINE)).corner_radius(style::RADIUS);
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (m, name, help) in [
            (EditorMode::Build, "Build", "Build the scene: its things and their settings (D switches)"),
            (EditorMode::Grade, "Grade", "Grade the finished picture's colour, like Lightroom (D switches)"),
        ] {
            let on = *mode == m;
            let button = style::toggle_button(name, on).frame_when_inactive(on);
            if ui.add(button).on_hover_text(help).clicked() {
                *mode = m;
            }
        }
    });
}

/// The left column in Grade mode: the masks, then the Histogram.
pub fn left(ui: &mut Ui, studio: &mut Studio) {
    egui::Panel::left("grade masks")
        .default_size(260.0)
        .min_size(200.0)
        .max_size(420.0)
        .resizable(true)
        .frame(crate::app::side_frame().inner_margin(egui::Margin::symmetric(10, 10)))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            masks(ui, studio);
        });
}

/// The masks list and the Histogram.
pub fn masks(ui: &mut Ui, studio: &mut Studio) {
    ui.label(RichText::new("Masks").font(style::semibold(13.0)));
    ui.add_space(4.0);
    let start = studio.grade.start(&studio.kit.registry);
    let card = cx::card_or_start(&studio.scene, &start);
    let gx = cx::GradeCx { scene: &studio.scene, kit: &studio.kit, card, time: studio.sim_time() };
    let on = gx.on();
    let panels = bpe_kit::modules::grade::whole_of(card)
        .map(|w| bpe_kit::modules::grade::panels_of(card, w).iter().filter(|p| gx.changed(p.id)).count())
        .unwrap_or(0);
    let mut edits = Vec::new();
    let row = egui::Frame::new()
        .fill(style::SELECTED)
        .corner_radius(style::RADIUS)
        .inner_margin(egui::Margin::symmetric(8, 5));
    row.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let ink = if on { style::TEXT } else { style::FAINT };
            let name = ui.label(RichText::new("Whole picture").color(ink));
            name.on_hover_text("The grade of the whole picture. Masks for parts of it come with a later update.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, "Grade on"));
                let ink = if response.hovered() { style::TEXT } else { style::MUTED };
                crate::tree::paint_eye(ui.painter(), rect, on, ink);
                let response = response.on_hover_text(if on {
                    "The grade is on. Click to see the picture without it (every setting is kept)."
                } else {
                    "The grade is off: the video has no grade. Click to turn it on."
                });
                if response.clicked() {
                    edits.push(cx::GradeEdit::Set("on".into(), bpe_core::Value::Bool(!on)));
                }
                let count = if panels == 1 { "1 panel changed".to_owned() } else { format!("{panels} panels changed") };
                ui.label(RichText::new(count).small().color(style::MUTED));
            });
        });
    });
    ui.add_space(14.0);
    ui.label(RichText::new("Histogram").font(style::semibold(13.0)));
    ui.add_space(4.0);
    scopes::histogram(ui, studio.grade.bins.as_deref());
    ui.add_space(4.0);
    ui.label(
        RichText::new("The finished picture's red, green, blue and brightness, darks on the left.")
            .small()
            .color(style::FAINT),
    );
    apply(studio, edits);
}

/// The right column in Grade mode: the Whole picture's panels.
pub fn right(ui: &mut Ui, studio: &mut Studio) {
    egui::Panel::right("grade panels")
        .default_size(style::INSPECTOR_W)
        .min_size(300.0)
        .max_size(640.0)
        .resizable(true)
        .frame(crate::app::side_frame())
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| panels::column(ui, studio));
        });
}

/// Applies Grade mode's edits to the studio's scene. A keyed edit pauses the playing, as in the
/// inspector.
pub fn apply(studio: &mut Studio, edits: Vec<cx::GradeEdit>) {
    let time = studio.sim_time();
    let applied = cx::apply(&mut studio.scene, &studio.kit, time, edits);
    if applied.keyed {
        studio.panels.inspector.keyed_edit = true;
    }
}
