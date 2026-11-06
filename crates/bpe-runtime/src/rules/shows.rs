//! What the spectacle actions (rules part 6) leave in the world: camera kicks, the lighting
//! scene a rule shows, and the numbers flash and note events carry.

use bpe_core::ThingId;

/// The pitch a `Note` event carries when the shape plays its own next note, as a hit would.
pub const AS_IF_HIT: u32 = u32::MAX;

/// The hit speed (px/s) a flash of strength 1 is logged as: the looks' full-strength flash.
pub const FULL_FLASH_SPEED: f32 = 600.0;

/// The most camera kicks kept at once; past it the oldest goes.
pub const MOST_KICKS: usize = 64;

/// A camera shake or zoom punch a rule started.
#[derive(Clone, Debug, PartialEq)]
pub struct Kick {
    /// The camera it moves.
    pub thing: ThingId,
    /// The step it started on.
    pub start: u64,
    /// A zoom punch (else a shake).
    pub punch: bool,
    /// Pixels for a shake, a share of the zoom for a punch.
    pub amount: f32,
    /// How long it lasts.
    pub seconds: f32,
    /// A shake's wobbles a second.
    pub rate: f32,
}

impl Kick {
    /// Seconds since it started at `time` (seconds), when it is still going.
    pub fn age(&self, time: f64, rate: u32) -> Option<f64> {
        let age = time - self.start as f64 / rate.max(1) as f64;
        (age >= 0.0 && age < self.seconds as f64).then_some(age)
    }

    /// How much of it is left at `age` seconds: 1 at the start, easing out to 0 at its end.
    pub fn weight(&self, age: f64) -> f32 {
        if self.seconds <= 0.0 {
            return 0.0;
        }
        let left = (1.0 - age as f32 / self.seconds).clamp(0.0, 1.0);
        left * left
    }
}

/// Which lighting scene a rule asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneOp {
    /// Scene `n`, counted from 1.
    Scene(u32),
    /// The one after the scene a rule showed last (the first after none), round again after the last.
    Next,
    /// The one before, round to the last before the first.
    Previous,
    /// Any scene but the one a rule shows now.
    Random,
    /// The lights' own settings.
    LightsOwn,
    /// The Lighting card's own Show again.
    Card,
}

/// The lighting scene a rule shows, from the step it started on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShownScene {
    pub step: u64,
    /// What it crossfades from: a scene's number from 1, 0 for the lights' own, `None` for what
    /// the card showed at that moment.
    pub from: Option<u32>,
    /// What it shows: as `from`, `None` handing back to the card.
    pub to: Option<u32>,
    /// The crossfade's seconds; `None` for the card's Crossfade.
    pub fade: Option<f32>,
}

/// Works out what a rule's `op` shows, from what a rule showed before (`now`), among `count`
/// scenes; `dice` (0 to 1) picks a Random scene. `None` when nothing changes.
pub fn next_scene(now: Option<&ShownScene>, op: SceneOp, count: u32, dice: f32) -> Option<Option<u32>> {
    let current = now.and_then(|s| s.to);
    let at = current.unwrap_or(0);
    let to = match op {
        SceneOp::Scene(n) if n >= 1 && n <= count => Some(n),
        SceneOp::Scene(_) => return None,
        SceneOp::Next | SceneOp::Previous | SceneOp::Random if count == 0 => return None,
        SceneOp::Next => Some(at % count + 1),
        SceneOp::Previous => Some(if at <= 1 { count } else { at - 1 }),
        SceneOp::Random => {
            // Among the scenes but the one showing.
            let others: Vec<u32> = (1..=count).filter(|&n| Some(n) != current).collect();
            let pick = ((dice.clamp(0.0, 1.0) * others.len() as f32) as usize).min(others.len().saturating_sub(1));
            Some(*others.get(pick)?)
        }
        SceneOp::LightsOwn => Some(0),
        SceneOp::Card => None,
    };
    // Asking for what shows already changes nothing (its crossfade isn't started again).
    let same = match now {
        Some(s) => s.to == to,
        None => to.is_none(),
    };
    (!same).then_some(to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(to: Option<u32>) -> ShownScene {
        ShownScene { step: 1, from: None, to, fade: None }
    }

    #[test]
    fn scene_ops_resolve_against_the_last_rule_scene() {
        assert_eq!(next_scene(None, SceneOp::Next, 3, 0.0), Some(Some(1)));
        assert_eq!(next_scene(Some(&shown(Some(3))), SceneOp::Next, 3, 0.0), Some(Some(1)));
        assert_eq!(next_scene(Some(&shown(Some(1))), SceneOp::Previous, 3, 0.0), Some(Some(3)));
        assert_eq!(next_scene(Some(&shown(Some(2))), SceneOp::Previous, 3, 0.0), Some(Some(1)));
        assert_eq!(next_scene(None, SceneOp::Scene(2), 3, 0.0), Some(Some(2)));
        assert_eq!(next_scene(None, SceneOp::Scene(4), 3, 0.0), None, "no fourth scene");
        assert_eq!(next_scene(Some(&shown(Some(2))), SceneOp::Scene(2), 3, 0.0), None, "already showing");
        assert_eq!(next_scene(None, SceneOp::Card, 3, 0.0), None, "the card shows already");
        assert_eq!(next_scene(Some(&shown(Some(2))), SceneOp::Card, 3, 0.0), Some(None));
        assert_eq!(next_scene(None, SceneOp::LightsOwn, 0, 0.0), Some(Some(0)));
        assert_eq!(next_scene(None, SceneOp::Next, 0, 0.0), None);
        for d in [0.0, 0.3, 0.6, 0.99, 1.0] {
            assert_ne!(next_scene(Some(&shown(Some(2))), SceneOp::Random, 3, d), Some(Some(2)));
        }
        assert_eq!(next_scene(Some(&shown(Some(1))), SceneOp::Random, 1, 0.5), None, "only one scene");
    }

    #[test]
    fn a_kick_eases_out_over_its_time() {
        let k = Kick { thing: ThingId(5), start: 240, punch: false, amount: 10.0, seconds: 0.5, rate: 18.0 };
        assert_eq!(k.age(0.99, 240), None, "not yet");
        assert_eq!(k.age(1.0, 240), Some(0.0));
        assert_eq!(k.weight(0.0), 1.0);
        assert!((k.weight(0.25) - 0.25).abs() < 1e-6);
        assert_eq!(k.age(1.5, 240), None, "over");
    }
}
