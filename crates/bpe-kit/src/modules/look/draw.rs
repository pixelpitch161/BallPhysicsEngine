//! Reading a thing's Look and turning it into colours.

use bpe_core::{Color, Settings, SlotId};
use bpe_gfx::DrawCx;

use super::ClassicLook;

/// The thing's classic look, when its Look slot is filled.
pub fn look_of(cx: &DrawCx) -> Option<ClassicLook> {
    cx.resolved.slots.get(&SlotId::Look).map(|s| ClassicLook::from_params(&s.params))
}

/// The linear-light colour of each of `count` items: the gradient sampled by `row_of(i)` in
/// 0..=1 when there is one, else the look's one colour.
pub fn colors(look: &ClassicLook, count: usize, row_of: impl Fn(usize) -> f32) -> impl Fn(usize) -> [f32; 4] {
    let plain = look.color.to_linear();
    let gradient: Vec<Color> = look.gradient.clone();
    let _ = count;
    move |i| if gradient.is_empty() { plain } else { Color::sample_gradient(&gradient, row_of(i)) }
}

/// The glow a wall gets from a look: the old renderer's wall glow at the default ball glow.
pub fn wall_glow(look: &ClassicLook) -> f32 {
    0.015 * look.glow / 0.12
}
