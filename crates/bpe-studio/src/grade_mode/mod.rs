//! Grade mode (colour grading, `docs/notes/grading.md`): the editor's second face, for the
//! scene's Grade card. Build | Grade in the toolbar (or D) switches. In Grade mode the left
//! column lists the masks (the Whole picture, then the masks) and the Histogram, the right column
//! the selected mask's parts and panels in the order they work, and the preview compares After,
//! Before and Split, with the selected gradient's handles and the mask overlay (O).
//!
//! - `cx.rs`: reading the card and applying edits; `panels.rs`: the panel column;
//! - `masks.rs`: the masks list; `parts.rs`: a mask's parts; `handles.rs`: a gradient's handles
//!   on the preview; `brush.rs`: painting a Brush part on the preview;
//! - `curve.rs`: the Tone Curve's graph; `scopes.rs`: the Histogram;
//! - `colour.rs`: the colour panels' bodies (HSL / Colour, Point Colour, Colour Grading,
//!   Calibration) and their coloured rails; `wheel.rs`: Colour Grading's colour wheel.
//!
//! Everything here but the card itself is the editor's own: never saved, never exported.

use crate::app::Studio;
use crate::style;
use bpe_core::{LayerId, Slot};
use eframe::egui::{self, Ui};
use std::collections::{BTreeMap, BTreeSet};

pub mod brush;
pub mod colour;
pub mod curve;
pub mod cx;
pub mod handles;
pub mod masks;
pub mod panels;
pub mod parts;
pub mod scopes;
#[cfg(test)]
mod tests;
pub mod wheel;

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

/// Which sliders an HSL / Colour panel shows (Lightroom's tabs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HslView {
    #[default]
    Hue,
    Saturation,
    Luminance,
    All,
}

impl HslView {
    pub const ALL: [HslView; 4] = [HslView::Hue, HslView::Saturation, HslView::Luminance, HslView::All];

    pub fn name(self) -> &'static str {
        match self {
            HslView::Hue => "Hue",
            HslView::Saturation => "Saturation",
            HslView::Luminance => "Luminance",
            HslView::All => "All",
        }
    }
}

/// Which wheels a Colour Grading panel shows (Lightroom's views).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WheelView {
    /// Shadows, midtones and highlights side by side.
    #[default]
    ThreeWay,
    /// One region's wheel, big, with its sliders: 0 shadows, 1 midtones, 2 highlights, 3 global.
    One(usize),
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
    /// The sliders each HSL / Colour panel shows.
    pub hsl_view: BTreeMap<LayerId, HslView>,
    /// The wheels each Colour Grading panel shows.
    pub wheel_view: BTreeMap<LayerId, WheelView>,
    /// The pick each Point Colour panel shows (its slot, 0..8).
    pub pick: BTreeMap<LayerId, usize>,
    /// The mask the right column shows (`None` the Whole picture).
    pub mask: Option<LayerId>,
    /// The part of each mask whose handles the preview shows.
    pub part: BTreeMap<LayerId, LayerId>,
    /// The red overlay (O) over where the selected mask covers.
    pub overlay: bool,
    /// A colour is being picked from the picture (the overlay stays off meanwhile).
    pub picking: bool,
    /// The mask whose name is being typed, and what is typed so far.
    pub renaming: Option<(LayerId, String)>,
    /// The mask whose row is being dragged to a new place.
    pub mask_drag: Option<LayerId>,
    /// Where each mask's row was drawn last frame, in order, for dragging.
    pub mask_rows: Vec<(LayerId, egui::Rect)>,
    /// The handle being dragged on the preview.
    pub handle: Option<handles::Grab>,
    /// Brushes A, B and Erase.
    pub brushes: brush::Brushes,
    /// The brush stroke being painted on the preview.
    pub painting: Option<brush::Painting>,
    /// The preview's last grade before Before or Split changed it: where the gradients are (set
    /// while the picture is built, which only reads the studio).
    pub shown: std::cell::RefCell<bpe_gfx::GradeStack>,
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
            hsl_view: BTreeMap::new(),
            wheel_view: BTreeMap::new(),
            pick: BTreeMap::new(),
            mask: None,
            part: BTreeMap::new(),
            overlay: false,
            picking: false,
            renaming: None,
            mask_drag: None,
            mask_rows: Vec::new(),
            handle: None,
            brushes: Default::default(),
            painting: None,
            shown: Default::default(),
            start: None,
        }
    }
}

impl GradeMode {
    /// The mask the right column shows on `card`: the selected one while it is there, else the
    /// Whole picture.
    pub fn mask_of(&self, card: &Slot) -> Option<LayerId> {
        let masks = bpe_kit::modules::grade::masks_of(card);
        self.mask.filter(|m| masks.iter().any(|l| l.id == *m)).or_else(|| bpe_kit::modules::grade::whole_of(card))
    }

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

    /// Whether the overlay is drawn: on, and no colour being picked from the picture.
    pub fn overlay_shown(&self) -> bool {
        self.overlay && !self.picking
    }

    /// The preview's grade as Grade mode shows it: cleared for Before, split for Split.
    pub fn show_on(&self, grade: &mut bpe_gfx::GradeStack) {
        // The stroke being painted shows before it is kept.
        if let Some(p) = &self.painting {
            brush::show_painting(p, grade);
        }
        self.shown.replace(grade.clone());
        // The overlay shows the selected mask (only a mask over part of the picture has one).
        if self.overlay_shown()
            && let Some(mask) = self.mask
            && let Some(m) = grade.masks.iter_mut().find(|m| m.id == mask.0 && m.area.is_some())
        {
            m.overlay = true;
        }
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
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| masks::column(ui, studio));
        });
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
    if let Some(mask) = applied.added {
        studio.grade.mask = Some(mask);
    }
}
