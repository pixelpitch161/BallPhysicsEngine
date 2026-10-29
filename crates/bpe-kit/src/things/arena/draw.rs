//! Drawing the arena's wall.

use bpe_gfx::{DrawCx, Frame, ShapeInstance, ThingDraw, thing_code};

use crate::modules::look::draw::{look_of, wall_glow};

pub struct ArenaDraw;

impl ThingDraw for ArenaDraw {
    fn kind(&self) -> &'static str {
        "arena"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(wall) = cx.world.walls.get(&cx.thing.id) else { return };
        let Some(look) = look_of(cx) else { return };
        frame.shapes.push(
            ShapeInstance::outline(&wall.outline, look.color.to_linear(), wall_glow(&look))
                .code(thing_code(cx.thing.id)),
        );
    }
}
