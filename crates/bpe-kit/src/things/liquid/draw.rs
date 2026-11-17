//! How a Liquid thing draws: its drops, straight from the card's buffers (the host's body), run
//! together into a surface (the particles' liquid pass), each drop in its own colour as mixed.
//! Every Liquid thing draws only its own drops, with its own Look.

use bpe_core::SlotId;
use bpe_gfx::{DrawCx, Frame, Liquid, LiquidStyle, ThingDraw};

use super::KIND;
use super::cards::{ColourFrom, LOOK, LookSettings};
use super::state::{host_of, state_of};

pub struct LiquidDraw;

/// Each drop's reach, in cells. One cell (two drops apart) showed poured drops' random packing as
/// lumps and pits all over the surface; one and a half smooths it out.
const REACH: f32 = 1.5;
/// The field inside the liquid: four drops a cell, each `(1 − d²)³` over its reach.
const INSIDE: f32 = std::f32::consts::PI * REACH * REACH;
/// The pretend light's Plastic.
const PLASTIC: f32 = 1.0;

/// The surface pass's numbers for `look`, the drops of liquid `index` on the grid at `origin` with
/// cells `cell` wide.
pub fn style_of(look: &LookSettings, index: usize, origin: glam::Vec2, cell: f32) -> LiquidStyle {
    let one = look.colour.to_linear();
    LiquidStyle {
        a: [look.level * INSIDE, look.softness, look.roundness, PLASTIC],
        b: [look.light_angle.to_radians(), look.shine, 0.0, 0.0],
        c: [0.0, look.opacity, 0.0, 0.0],
        sky_top: [0.9, 0.95, 1.0, 0.0],
        sky_bottom: [0.2, 0.22, 0.25, 0.0],
        d: [0.0; 4],
        e: [origin.x, origin.y, cell, cell * REACH],
        f: [look.swirl_detail, 1.0, index as f32, if look.colour_from == ColourFrom::OneColour { 1.0 } else { 0.0 }],
        g: [one[0], one[1], one[2], 0.0],
    }
}

impl ThingDraw for LiquidDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(index) = state_of(cx.world, cx.thing.id).and_then(|s| s.index) else { return };
        let Some(host) = host_of(cx.world).and_then(|h| h.host.as_ref()) else { return };
        let Some(body) = host.body.as_ref() else { return };
        let look = LookSettings::read(cx.resolved.slots.get(&SlotId::Look).filter(|c| c.kind == LOOK));
        let style = style_of(&look, index, host.layout.origin, host.layout.cell);
        frame.liquids.push(Liquid { blobs: Vec::new(), style, front: look.in_front, drops: Some(body.source()) });
    }
}
