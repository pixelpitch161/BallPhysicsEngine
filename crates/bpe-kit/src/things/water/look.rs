//! How a Water scene looks, read from its cards (Water mode part 6): [`plain_look`] for the plain
//! drawing, and the look drawing's inputs. Left out of the take's print (the kit's `build.rs`), so a
//! change here never makes a final again.

use bpe_core::{Color, Resolved, Scene, SlotId, ThingId};
use bpe_water::draw::{PlainLiquid, PlainLook};

use super::draft::{Keyed, MOST_LIQUIDS};
use super::{LIQUID, PIECE, WALL, course_of};
use crate::modules::water::{BackCard, BackShows, MaterialCard, WaterLook};

/// What the plain drawing is told by the cards at `time`: each Liquid's Colour, Deep colour and
/// Clarity, the Course's Rock colour, each Piece's and Wall's Material Colour (in the order they are
/// laid over the rock), and the Back (darker rock, a colour, a gradient's middle; a Picture shows as
/// darker rock until pictures are drawn).
pub fn plain_look(scene: &Scene, resolved: &Resolved, time: f64) -> PlainLook {
    let srgb = |c: Color| [c.r, c.g, c.b].map(|v| f32::from(v) / 255.0);
    let rock = course_of(scene)
        .and_then(|id| Keyed::of(scene, resolved, id, SlotId::Material))
        .map_or_else(|| MaterialCard::default().colour, |k| k.card::<MaterialCard>(time).colour);
    let rock = srgb(rock);
    let back: BackCard = Keyed::of(scene, resolved, ThingId::ROOT, SlotId::Back).unwrap_or_default().card(time);
    let darker = rock.map(|c| c * (1.0 - back.darker.clamp(0.0, 1.0)));
    let back = match back.shows {
        BackShows::DarkerRock | BackShows::Picture => darker,
        BackShows::Colour => srgb(back.colour),
        BackShows::Gradient => srgb(Color::from_linear(back.gradient.sample(0.5))),
    };
    let liquids = scene
        .order
        .iter()
        .filter(|id| scene.things[*id].kind == LIQUID && resolved.things.contains_key(*id))
        .take(MOST_LIQUIDS)
        .map(|id| {
            let look: WaterLook = Keyed::of(scene, resolved, *id, SlotId::Look).unwrap_or_default().card(time);
            PlainLiquid { colour: srgb(look.colour), deep: srgb(look.deep), clarity: look.clarity }
        })
        .collect();
    let solids = scene
        .order
        .iter()
        .filter(|id| matches!(scene.things[*id].kind.as_str(), PIECE | WALL) && resolved.things.contains_key(*id))
        .take(bpe_water::draw::MOST_COLOURED)
        .map(|id| {
            let m: MaterialCard = Keyed::of(scene, resolved, *id, SlotId::Material).unwrap_or_default().card(time);
            srgb(m.colour)
        })
        .collect();
    PlainLook { back, rock, liquids, solids }
}
