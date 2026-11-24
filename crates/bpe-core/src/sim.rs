//! What a scene simulates: bouncing balls, or water running through a course. Each kind of scene
//! has its own things, cards, editor and physics; the two never share a scene.

use serde::{Deserialize, Serialize};

/// The kind of scene. A scene file that doesn't say is a Balls scene, so every file from before
/// Water scenes existed opens as it was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SimKind {
    #[default]
    Balls,
    Water,
}

impl SimKind {
    pub const ALL: [SimKind; 2] = [SimKind::Balls, SimKind::Water];

    /// The name shown in the editor ("a Water scene").
    pub fn name(self) -> &'static str {
        match self {
            SimKind::Balls => "Balls",
            SimKind::Water => "Water",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_is_balls_unless_it_says_water() {
        assert_eq!(SimKind::default(), SimKind::Balls);
        assert_eq!(SimKind::ALL.map(SimKind::name), ["Balls", "Water"]);
    }
}
