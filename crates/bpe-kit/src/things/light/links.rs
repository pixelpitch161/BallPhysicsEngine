//! Lights up (light linking, lighting part 4): a light can light only some things, or every
//! thing but some. Each thing drawn carries a **link number** in its records (and the background
//! the scene's), read back by the light pass from the tags picture; a light names the numbers it
//! lights up or leaves out.

use bpe_core::{Resolved, ThingId};
use bpe_runtime::World;

use super::{LightSettings, LightsUp};

/// The largest link number the renderer reads back exactly (a half-float picture's whole
/// numbers); things past it get 0, which no light names.
pub const MOST_LINK: usize = 2047;

/// `id`'s link number: 1 + its place among the resolved scene's things, 0 past [`MOST_LINK`].
/// The draw and the light hook work it out alike, so they agree without sharing anything.
pub fn number_of(resolved: &Resolved, id: ThingId) -> f32 {
    match resolved.things.keys().position(|&k| k == id) {
        Some(place) if place < MOST_LINK => (place + 1) as f32,
        _ => 0.0,
    }
}

/// The link numbers light `s` names (`me` its own thing): the things its Things matches, and
/// the scene's own number (the background) when Only these lights it or Everything but these
/// leaves it dark. Empty for Everything.
pub fn named(resolved: &Resolved, world: &World, s: &LightSettings, me: ThingId) -> Vec<f32> {
    if s.lights_up == LightsUp::Everything {
        return Vec::new();
    }
    let mut out: Vec<f32> =
        world.matching(&s.link_things, me).into_iter().map(|id| number_of(resolved, id)).filter(|n| *n > 0.0).collect();
    let background = match s.lights_up {
        LightsUp::OnlyThese => s.link_background,
        _ => !s.link_background,
    };
    if background {
        out.push(number_of(resolved, ThingId::ROOT));
    }
    out.sort_by(f32::total_cmp);
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_numbers_follow_the_things_order() {
        let kit = crate::kit();
        let scene = crate::scenes::lit_room(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let ids: Vec<ThingId> = resolved.things.keys().copied().collect();
        for (k, id) in ids.iter().enumerate() {
            assert_eq!(number_of(&resolved, *id), (k + 1) as f32);
        }
        assert_eq!(number_of(&resolved, ThingId(9999)), 0.0);
    }
}
