//! How a broken thing's Break look shows at a moment after the break (rules part 3, plan
//! `docs/superpowers/plans/rules/part3.md`): drawn only, worked out afresh from the world's
//! [`Remains`] every frame, so scrubbing and export draw the same.

pub mod draw;
pub mod shatter;

#[cfg(test)]
mod tests;

use bpe_core::Settings;
use bpe_gfx::DrawCx;
use bpe_runtime::Remains;

use super::breaks::{BreakLook, LookSettings};

/// How much of a broken thing shows, and how big it is, at a moment of its look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Showing {
    pub alpha: f32,
    pub scale: f32,
}

/// `u` eased in and out, 0 to 1.
fn smooth(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// How far through a time `t` long `age` is, 0 to 1 (a time of 0 is over at once).
fn through(age: f64, t: f64) -> f64 {
    if t.is_finite() && t > 0.0 { (age / t).clamp(0.0, 1.0) } else { 1.0 }
}

/// How `look` shows `age` seconds after the break: `None` once it is over (Vanish never shows).
/// Shatter's is the whole thing's (a light's, and each of its pieces' fade).
pub fn showing(look: &LookSettings, age: f64) -> Option<Showing> {
    if !(age.is_finite() && age >= 0.0) || age >= look.lasts() {
        return None;
    }
    let s = match look.look {
        BreakLook::Vanish => return None,
        BreakLook::FadeOut => Showing { alpha: (1.0 - smooth(through(age, look.fade_time))) as f32, scale: 1.0 },
        BreakLook::Pop => {
            let u = through(age, look.pop_time);
            let swell = if look.swell.is_finite() { look.swell } else { 1.0 };
            Showing { alpha: (1.0 - u * u) as f32, scale: 1.0 + (swell - 1.0) * (1.0 - (1.0 - u) * (1.0 - u)) as f32 }
        }
        BreakLook::Shatter => {
            let after = if look.fade_after.is_finite() { look.fade_after.max(0.0) } else { 0.0 };
            let fade = if age < after { 0.0 } else { smooth(through(age - after, look.fade_over)) };
            Showing { alpha: (1.0 - fade) as f32, scale: 1.0 }
        }
    };
    Some(s)
}

/// What is left of body row `row` (`None`: the whole thing) of the thing `cx` draws, with its look,
/// while the look shows at the moment drawn.
pub fn remains_now<'a>(cx: &DrawCx<'a>, row: Option<u32>) -> Option<(&'a Remains, LookSettings)> {
    let rules = cx.world.rules()?;
    let r = rules.remains_of(cx.thing.id, row).filter(|r| r.shows_at(cx.time))?;
    Some((r, LookSettings::from_params(&r.show.look)))
}

/// How much of the broken thing `cx` draws shows now: `Some` only while it is broken and its look
/// plays out (a light's strength follows it).
pub fn whole_now(cx: &DrawCx) -> Option<Showing> {
    let (r, look) = remains_now(cx, None)?;
    Some(showing(&look, r.age(cx.time)).unwrap_or(Showing { alpha: 0.0, scale: 1.0 }))
}
