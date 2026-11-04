//! A Shape's Look: a stack of layers like the balls' (paint layers come later), whose bottom
//! layer, the Body, is the shape itself. The Body has no settings of its own: picking it shows the
//! shape's outline settings, which are the thing's own (see `things::shape::BODY_HEADINGS`).

use bpe_core::{LayerKind, LayerSection, ModuleKind, SettingDef, SlotId};

/// The card kind.
pub const CARD: &str = "look.shape";
/// The Body layer kind.
pub const BODY: &str = "shape.body";

/// The Look card of shapes.
pub struct ShapeLookKind;

impl ModuleKind for ShapeLookKind {
    fn id(&self) -> &'static str {
        CARD
    }
    fn name(&self) -> &'static str {
        "Look"
    }
    fn help(&self) -> &'static str {
        "How the shape looks: a stack of layers painted bottom to top, on the plain white shape."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == crate::things::shape::KIND
    }
}

/// The shape itself, the bottom layer of its Look.
pub struct ShapeBody;

impl LayerKind for ShapeBody {
    fn id(&self) -> &'static str {
        BODY
    }
    fn name(&self) -> &'static str {
        "Body"
    }
    fn help(&self) -> &'static str {
        "The shape itself. Pick it to change its outline."
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn section(&self) -> LayerSection {
        LayerSection::Drawn
    }
    fn repeats(&self) -> bool {
        false
    }
    fn card(&self) -> &'static str {
        CARD
    }
    fn base(&self) -> bool {
        true
    }
    fn maskable(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
}
