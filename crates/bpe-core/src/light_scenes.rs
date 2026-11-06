//! Lighting scenes (lighting spec, part 6): named snapshots of every light's settings, kept on
//! the scene's Lighting card and shown in turn, on hits or from the timeline.

use serde::{Deserialize, Serialize};

use crate::{Params, ThingId};

/// The Lighting card's setting that holds its scenes.
pub const LIGHT_SCENES_KEY: &str = "scenes";

/// One lighting scene: a name and the settings each light (and rig) had when it was saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LightingScene {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<SceneLight>,
}

/// One thing's saved settings in a lighting scene.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneLight {
    pub thing: ThingId,
    #[serde(default)]
    pub params: Params,
}

impl LightingScene {
    /// The saved settings of `thing`, if the scene holds it.
    pub fn of(&self, thing: ThingId) -> Option<&Params> {
        self.lights.iter().find(|l| l.thing == thing).map(|l| &l.params)
    }

    /// Whether every number in it is finite and no thing is listed twice.
    pub fn is_valid(&self) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        self.lights.iter().all(|l| seen.insert(l.thing) && l.params.0.values().all(finite))
    }
}

/// Whether a saved value is finite all through (a NaN would spread into the picture).
fn finite(v: &crate::Value) -> bool {
    match v {
        crate::Value::Number(x) => x.is_finite(),
        crate::Value::Vec2([x, y]) => x.is_finite() && y.is_finite(),
        crate::Value::Points(ps) => ps.iter().flatten().all(|x| x.is_finite()),
        // Scenes are never saved inside scenes.
        crate::Value::LightScenes(_) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    #[test]
    fn a_scene_finds_its_lights_and_refuses_nan_and_doubles() {
        let mut p = Params::default();
        p.set("strength", Value::Number(2.0));
        let mut s =
            LightingScene { name: "Warm".into(), lights: vec![SceneLight { thing: ThingId(3), params: p.clone() }] };
        assert!(s.is_valid());
        assert_eq!(s.of(ThingId(3)), Some(&p));
        assert_eq!(s.of(ThingId(4)), None);
        s.lights.push(SceneLight { thing: ThingId(3), params: Params::default() });
        assert!(!s.is_valid(), "a thing twice");
        s.lights.pop();
        s.lights[0].params.set("reach", Value::Number(f64::NAN));
        assert!(!s.is_valid(), "NaN");
    }
}
