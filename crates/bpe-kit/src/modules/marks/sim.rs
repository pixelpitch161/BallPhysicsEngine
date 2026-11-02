//! The Marks module has nothing to simulate: the world only reads its settings.

use bpe_core::{Params, Settings};
use bpe_runtime::{ModuleSim, ThingState};

use super::MarksBrushes;

pub struct MarksSim;

impl ModuleSim for MarksSim {
    fn kind(&self) -> &'static str {
        "marks.brushes"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(MarksBrushes::from_params(params))
    }
}
