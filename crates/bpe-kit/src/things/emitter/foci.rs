//! What an emitter can sit on: a focus of a shape. A circle's two foci are both its middle.

use bpe_core::{Settings, Thing, ThingId};
use bpe_runtime::{Outline, Pose, World};
use glam::Vec2;

use super::{EmitterSettings, SitsFocus};
use crate::things::shape::{self, ShapeSettings, ShapeStart};

/// The two foci of a wall `outline` as it stands, when it has them: a round ring's are both its
/// middle. Polygons, boxes and moulded outlines have none.
pub fn focus_points(outline: &Outline) -> Option<[Vec2; 2]> {
    match outline {
        Outline::Ring { center, sides: 0, .. } => Some([*center, *center]),
        _ => None,
    }
}

/// Whether `thing` is a shape an emitter can sit on (one with foci).
pub fn can_sit_on(thing: &Thing) -> bool {
    thing.kind == shape::KIND && ShapeSettings::from_params(&thing.params).start == ShapeStart::Circle
}

/// Which of the two foci `focus` names.
pub fn index(focus: SitsFocus) -> usize {
    match focus {
        SitsFocus::Focus1 => 0,
        SitsFocus::Focus2 => 1,
    }
}

/// Where an emitter with settings `s` sits in `world`, when it sits on a focus of a shape that is
/// there and has foci.
pub fn sits_on_pose(world: &World, s: &EmitterSettings) -> Option<Pose> {
    if s.sits_on <= 0 {
        return None;
    }
    let wall = world.walls.get(&ThingId(s.sits_on as u32))?;
    let foci = focus_points(&wall.outline)?;
    Some(Pose { position: foci[index(s.sits_focus)], ..Pose::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circle_has_both_foci_at_its_middle_and_a_polygon_none() {
        let ring = Outline::Ring { center: Vec2::new(3.0, 4.0), radius: 10.0, sides: 0, angle: 0.0, thickness: 1.0 };
        assert_eq!(focus_points(&ring), Some([Vec2::new(3.0, 4.0); 2]));
        let hex = Outline::Ring { center: Vec2::ZERO, radius: 10.0, sides: 6, angle: 0.0, thickness: 1.0 };
        assert_eq!(focus_points(&hex), None);
    }
}
