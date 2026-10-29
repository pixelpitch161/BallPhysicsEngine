//! Reading a thing's Look and turning it into colours.

use bpe_core::{Color, Settings, SlotId};
use bpe_gfx::DrawCx;
use bpe_runtime::Bodies;

use super::{ClassicLook, GradientBy};

/// The thing's classic look, when its Look slot is filled.
pub fn look_of(cx: &DrawCx) -> Option<ClassicLook> {
    cx.resolved.slots.get(&SlotId::Look).map(|s| ClassicLook::from_params(&s.params))
}

/// The linear-light colour of each ball in `bodies`: the gradient sampled by what "Colours
/// follow" says (`row_of(i)` in 0..=1 for rows) when there is one, else the look's one colour;
/// then turned round the colour wheel by "Hue shift per hit" for every hit the ball has taken.
/// The closure owns the look and borrows the bodies.
pub fn colors<'a>(
    look: ClassicLook,
    bodies: &'a Bodies,
    row_of: impl Fn(usize) -> f32 + 'a,
) -> impl Fn(usize) -> [f32; 4] + 'a {
    let plain = look.color.to_linear();
    let count = bodies.len();
    let full = look.gradient_full.max(1e-3);
    move |i| {
        let base = if look.gradient.is_empty() {
            plain
        } else {
            let t = match look.gradient_by {
                GradientBy::Row => row_of(i),
                GradientBy::Order if count > 1 => i as f32 / (count - 1) as f32,
                GradientBy::Order => 0.0,
                GradientBy::Speed => bodies.velocity.get(i).map_or(0.0, |v| v.length() / full),
                GradientBy::Hits => bodies.hits.get(i).map_or(0.0, |h| *h as f32 / full),
            };
            Color::sample_gradient(&look.gradient, t.clamp(0.0, 1.0))
        };
        let hits = bodies.hits.get(i).copied().unwrap_or(0);
        if look.hue_per_hit == 0.0 || hits == 0 {
            base
        } else {
            Color::from_linear(base).shift_hue(look.hue_per_hit * hits as f32 / 360.0).to_linear()
        }
    }
}

/// The glow a wall gets from a look: the old renderer's wall glow at the default ball glow.
pub fn wall_glow(look: &ClassicLook) -> f32 {
    0.015 * look.glow / 0.12
}
