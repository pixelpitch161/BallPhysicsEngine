//! The scene's colour **Grade** card (colour grading, `docs/notes/grading.md`): Lightroom-style
//! panels in a real processing order, edited in the editor's Grade mode.
//!
//! The card's layers are its masks (`LayerSection::Mask`): the Whole picture first (the base
//! layer, always there), later the masks (part 4). A mask's panels (`LayerSection::Panel`) are its
//! sub-layers, in the order they work; a panel kind may be added more than once.
//!
//! - `basic.rs`: the Basic panel; `tone_curve.rs`: the Tone Curve panel;
//! - `hsl.rs`: HSL / Colour; `point_colour.rs`: Point Colour; `colour_grading.rs`: Colour Grading's
//!   wheels; `detail.rs`: Detail (sharpening); `effects.rs`: Effects (vignette, grain, gradient
//!   map); `calibration.rs`: Calibration;
//! - here: the kinds, the starting card, adding and ordering panels, and the stack the scene's
//!   draw puts on the frame ([`stack`]).

use bpe_core::{
    EditError, KindId, Layer, LayerId, LayerKind, LayerSection, ModuleKind, Registry, ResolvedSlot, Scene, SettingDef,
    Settings, Slot, SlotId, ThingId,
};

pub mod basic;
pub mod calibration;
pub mod colour_grading;
pub mod detail;
pub mod effects;
pub mod hsl;
pub mod point_colour;
#[cfg(test)]
mod tests;
pub mod tone_curve;

pub use basic::BasicPanel;
pub use calibration::CalibrationPanel;
pub use colour_grading::ColourGradingPanel;
pub use detail::DetailPanel;
pub use effects::EffectsPanel;
pub use hsl::HslPanel;
pub use point_colour::{PICKS, PointColourPanel, PointPick, pick_prefix};
pub use tone_curve::{CURVE_KEYS, CurveMode, ToneCurvePanel};

/// The card kind.
pub const GRADE: &str = "grade";
/// The Whole picture: the base mask.
pub const WHOLE: &str = "grade.whole";
/// The panel kinds.
pub const BASIC: &str = "grade.basic";
pub const TONE_CURVE: &str = "grade.tone_curve";
pub const HSL: &str = "grade.hsl";
pub const POINT_COLOUR: &str = "grade.point_colour";
pub const COLOUR_GRADING: &str = "grade.colour_grading";
pub const DETAIL: &str = "grade.detail";
pub const EFFECTS: &str = "grade.effects";
pub const CALIBRATION: &str = "grade.calibration";

/// Lightroom Classic's panel order: a mask's panels start in it, and Reset order puts them back
/// in it.
pub const PANEL_ORDER: [&str; 8] = [BASIC, TONE_CURVE, HSL, POINT_COLOUR, COLOUR_GRADING, DETAIL, EFFECTS, CALIBRATION];

/// The Grade card's own settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct GradeCard {
    /// Off, the grade is skipped and every setting kept (Lightroom's switch for the whole edit).
    #[setting(label = "Grade on", group = "Basics")]
    pub on: bool,
}

impl Default for GradeCard {
    fn default() -> Self {
        Self { on: true }
    }
}

pub struct GradeCardKind;

impl ModuleKind for GradeCardKind {
    fn id(&self) -> &'static str {
        GRADE
    }
    fn name(&self) -> &'static str {
        "Grade"
    }
    fn help(&self) -> &'static str {
        "The colour grade over the finished picture: Lightroom's panels, in the order they work."
    }
    fn slot(&self) -> SlotId {
        SlotId::Grade
    }
    fn settings(&self) -> &'static [SettingDef] {
        GradeCard::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "scene"
    }
}

/// The Whole picture mask: the grade's base layer.
pub struct WholeKind;

impl LayerKind for WholeKind {
    fn id(&self) -> &'static str {
        WHOLE
    }
    fn name(&self) -> &'static str {
        "Whole picture"
    }
    fn help(&self) -> &'static str {
        "The grade of the whole picture, before any mask."
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn section(&self) -> LayerSection {
        LayerSection::Mask
    }
    fn repeats(&self) -> bool {
        false
    }
    fn card(&self) -> &'static str {
        GRADE
    }
    fn base(&self) -> bool {
        true
    }
    fn takes_sub(&self, sub: &str) -> bool {
        PANEL_ORDER.contains(&sub)
    }
    fn per_ball(&self) -> bool {
        false
    }
}

/// A grade panel kind.
pub struct PanelKind {
    pub id: &'static str,
    pub name: &'static str,
    pub help: &'static str,
    pub defs: fn() -> &'static [SettingDef],
}

impl LayerKind for PanelKind {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn help(&self) -> &'static str {
        self.help
    }
    fn settings(&self) -> &'static [SettingDef] {
        (self.defs)()
    }
    fn section(&self) -> LayerSection {
        LayerSection::Panel
    }
    fn repeats(&self) -> bool {
        true
    }
    fn card(&self) -> &'static str {
        GRADE
    }
    fn takes_subs(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
}

/// Every panel kind, in Lightroom's order.
pub fn panel_kinds() -> [PanelKind; 8] {
    [
        PanelKind {
            id: BASIC,
            name: "Basic",
            help: "White balance, tone (exposure, contrast, highlights, shadows, whites, blacks) and colour.",
            defs: BasicPanel::defs,
        },
        PanelKind {
            id: TONE_CURVE,
            name: "Tone Curve",
            help: "The parametric curve's four regions and the point curves, for all channels or one.",
            defs: ToneCurvePanel::defs,
        },
        PanelKind {
            id: HSL,
            name: "HSL / Colour",
            help: "Hue, saturation and luminance of each of eight colour bands, from red round to magenta.",
            defs: HslPanel::defs,
        },
        PanelKind {
            id: POINT_COLOUR,
            name: "Point Colour",
            help: "Up to eight colours picked from the picture, each moving the colours near it.",
            defs: PointColourPanel::defs,
        },
        PanelKind {
            id: COLOUR_GRADING,
            name: "Colour Grading",
            help: "Colour wheels tinting the shadows, midtones and highlights, and the whole picture.",
            defs: ColourGradingPanel::defs,
        },
        PanelKind {
            id: DETAIL,
            name: "Detail",
            help: "Sharpening: how strongly, how wide and how fine the sharpened detail is.",
            defs: DetailPanel::defs,
        },
        PanelKind {
            id: EFFECTS,
            name: "Effects",
            help: "The post-crop vignette, film grain that moves with the video, and the gradient map.",
            defs: EffectsPanel::defs,
        },
        PanelKind {
            id: CALIBRATION,
            name: "Calibration",
            help: "The shadows' tint and the red, green and blue primaries' hue and saturation.",
            defs: CalibrationPanel::defs,
        },
    ]
}

/// Adds the card, the Whole picture and the panels to the registry.
pub fn register(r: &mut Registry) {
    r.add_module(GradeCardKind);
    r.add_layer(WholeKind);
    for k in panel_kinds() {
        r.add_layer(k);
    }
}

/// The Grade card a scene starts with when it is first graded: the Whole picture (layer 0) with
/// one panel of each kind in Lightroom's order (layers 1, 2, ...), every setting at its default.
/// [`ensure_grade`] builds exactly this, so the editor can show it before the scene has one.
pub fn starting_card(reg: &Registry) -> Slot {
    let mut scene = Scene::empty(reg);
    ensure_grade(&mut scene, reg);
    scene.card(ThingId::ROOT, SlotId::Grade).cloned().expect("made above")
}

/// Gives the scene its Grade card ([`starting_card`]) when it has none. True when it added it.
pub fn ensure_grade(scene: &mut Scene, reg: &Registry) -> bool {
    if scene.card(ThingId::ROOT, SlotId::Grade).is_some_and(|c| c.kind == GRADE) {
        return false;
    }
    let root = ThingId::ROOT;
    if scene.card(root, SlotId::Grade).is_some() {
        scene.remove_slot(root, SlotId::Grade);
    }
    scene.add_slot(reg, root, SlotId::Grade, GRADE).expect("the kit has the grade card");
    let whole = scene.add_layer(reg, root, SlotId::Grade, WHOLE).expect("the whole picture fits");
    for kind in PANEL_ORDER {
        scene.add_sub_layer(reg, root, SlotId::Grade, whole, kind).expect("a panel fits the whole picture");
    }
    true
}

/// The Whole picture's layer id in `card`.
pub fn whole_of(card: &Slot) -> Option<LayerId> {
    card.layers.iter().find(|l| l.kind == WHOLE).map(|l| l.id)
}

/// The panels of mask `mask` in `card`, in processing order.
pub fn panels_of(card: &Slot, mask: LayerId) -> Vec<&Layer> {
    card.layers.iter().filter(|l| l.parent == Some(mask) && PANEL_ORDER.contains(&l.kind.as_str())).collect()
}

/// Adds a panel of `kind` at the end of mask `mask`'s panels (a second one is named "Tone Curve
/// 2"), giving the scene its card first. Returns the new panel.
pub fn add_panel(scene: &mut Scene, reg: &Registry, mask: LayerId, kind: &str) -> Result<LayerId, EditError> {
    ensure_grade(scene, reg);
    scene.add_sub_layer(reg, ThingId::ROOT, SlotId::Grade, mask, kind)
}

/// Whether panel `panel` may be removed: a mask keeps at least one panel.
pub fn can_remove_panel(card: &Slot, panel: LayerId) -> bool {
    let Some(mask) = card.layer(panel).and_then(|l| l.parent) else { return false };
    panels_of(card, mask).len() > 1
}

/// Puts mask `mask`'s panels back in Lightroom's order, each copy after the first of its kind
/// (copies keep the order they had among themselves). True when the order changed.
pub fn reset_order(scene: &mut Scene, reg: &Registry, mask: LayerId) -> bool {
    let Some(card) = scene.card(ThingId::ROOT, SlotId::Grade) else { return false };
    let now: Vec<(LayerId, KindId)> = panels_of(card, mask).iter().map(|l| (l.id, l.kind.clone())).collect();
    let rank = |kind: &str| PANEL_ORDER.iter().position(|k| *k == kind).unwrap_or(PANEL_ORDER.len());
    let mut sorted = now.clone();
    sorted.sort_by_key(|(_, kind)| rank(kind));
    if sorted == now {
        return false;
    }
    for (place, (id, _)) in sorted.iter().enumerate() {
        let _ = scene.move_sub_layer_in(reg, ThingId::ROOT, SlotId::Grade, *id, place);
    }
    true
}

/// The resolved Grade card as the frame's stack: its shown masks and their shown panels in order,
/// every number at the frame's moment. Empty when the card is off or missing.
pub fn stack(card: Option<&ResolvedSlot>) -> bpe_gfx::GradeStack {
    let Some(card) = card.filter(|c| c.kind == GRADE) else { return bpe_gfx::GradeStack::default() };
    if !GradeCard::from_params(&card.params).on {
        return bpe_gfx::GradeStack::default();
    }
    let masks = card
        .layers
        .iter()
        .filter(|l| l.kind == WHOLE)
        .map(|mask| {
            let panels = card
                .layers
                .iter()
                .filter(|l| l.parent == Some(mask.id))
                .filter_map(|l| {
                    let params = card.layer_params(l.id);
                    match l.kind.as_str() {
                        BASIC => Some(bpe_gfx::GradePanel::Basic(BasicPanel::from_params(&params).gpu())),
                        TONE_CURVE => Some(bpe_gfx::GradePanel::ToneCurve(ToneCurvePanel::from_params(&params).gpu())),
                        HSL => Some(bpe_gfx::GradePanel::Hsl(HslPanel::from_params(&params).gpu())),
                        POINT_COLOUR => {
                            Some(bpe_gfx::GradePanel::PointColour(PointColourPanel::from_params(&params).gpu()))
                        }
                        COLOUR_GRADING => {
                            Some(bpe_gfx::GradePanel::ColourGrading(ColourGradingPanel::from_params(&params).gpu()))
                        }
                        DETAIL => Some(bpe_gfx::GradePanel::Detail(DetailPanel::from_params(&params).gpu())),
                        EFFECTS => Some(bpe_gfx::GradePanel::Effects(EffectsPanel::from_params(&params).gpu())),
                        CALIBRATION => {
                            Some(bpe_gfx::GradePanel::Calibration(CalibrationPanel::from_params(&params).gpu()))
                        }
                        _ => None,
                    }
                })
                .collect();
            bpe_gfx::GradeMask { amount: 1.0, panels }
        })
        .collect();
    bpe_gfx::GradeStack { masks, split: None }
}
