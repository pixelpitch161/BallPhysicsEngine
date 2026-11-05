//! What an emitter can sit on: a focus of a shape. An ellipse has two; a circle's two foci are
//! both its middle.

use bpe_core::{Settings, Thing, ThingId};
use bpe_runtime::{Pose, World};

use super::{EmitterSettings, SitsFocus};
use crate::things::shape::{self, ShapeSettings, ShapeStart};

/// Whether `thing` is a shape an emitter can sit on (a circle or an ellipse).
pub fn can_sit_on(thing: &Thing) -> bool {
    thing.kind == shape::KIND
        && matches!(ShapeSettings::from_params(&thing.params).start, ShapeStart::Circle | ShapeStart::Ellipse)
}

/// Whether `thing`'s two foci are apart (an ellipse that isn't round) rather than both its middle.
pub fn two_foci(thing: &Thing) -> bool {
    let s = ShapeSettings::from_params(&thing.params);
    s.start == ShapeStart::Ellipse && s.width != s.height
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
    let foci = shape::foci(world, ThingId(s.sits_on as u32))?;
    Some(Pose { position: foci[index(s.sits_focus)], ..Pose::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::SHAPE;
    use crate::{kit, new_scene};

    #[test]
    fn circles_and_ellipses_can_be_sat_on_and_polygons_cannot() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let shape = scene.things.get_mut(&SHAPE).unwrap();
        shape.params = ShapeSettings::circle(300.0, 4.0).to_params();
        assert!(can_sit_on(shape) && !two_foci(shape));
        shape.params = ShapeSettings::ellipse(600.0, 900.0, 4.0).to_params();
        assert!(can_sit_on(shape) && two_foci(shape));
        shape.params = ShapeSettings::ellipse(600.0, 600.0, 4.0).to_params();
        assert!(can_sit_on(shape) && !two_foci(shape), "a round ellipse's foci are its middle");
        shape.params = ShapeSettings::polygon(6, 300.0, 4.0).to_params();
        assert!(!can_sit_on(shape));
    }
}
