//! Rig: a folder for lights that plays a light show on them (lighting spec, part 6). Every Light
//! thing under it, nested rigs included, is one of its fixtures; its pattern (Chase, Strobe,
//! Sweep, Alternate colours, Random) steps on the beats, the bars, the hits or the seconds and
//! sets each fixture's brightness, colour, turn and sway at the moment drawn. Nothing is kept
//! between frames and nothing physical reads it.

pub mod fixtures;
pub mod pattern;

use bpe_core::{Choice, Color, Gradient, KindId, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

/// The thing kind's id.
pub const RIG: &str = "rig";

pub struct RigKind;

/// What the rig does to its lights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Pattern {
    /// The lights as they are (coloured from the list, when the colours come from it).
    Still,
    /// One light after another, round and round.
    #[default]
    Chase,
    /// Every light flashes together at each step.
    Strobe,
    /// The lights swing to and fro.
    Sweep,
    /// The lights take the colours of the list, moving along one light each step.
    AlternateColours,
    /// Lights flick on at random each step.
    Random,
}

/// What moves the pattern on a step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum StepsOn {
    /// The beats of the scene's tempo.
    #[default]
    Beats,
    /// The bars of the scene's tempo.
    Bars,
    /// The hits of the things it hears.
    Hits,
    /// A step every so many seconds.
    Seconds,
}

/// The order the pattern goes through the lights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Order {
    /// First to last, as they are in the list of things.
    #[default]
    Forward,
    /// Last to first.
    Backward,
    /// First to last and back again.
    BackAndForth,
    /// From the middle lights out to both ends.
    FromTheMiddleOut,
    /// From both ends in to the middle.
    ToTheMiddle,
    /// A mixed-up order that stays the same (change the Seed for another).
    Shuffled,
}

/// What the pattern counts as one light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Across {
    /// Each Light thing; a light riding every ball lights all its lamps alike.
    #[default]
    Lights,
    /// Every lamp: a light riding every ball counts each ball's lamp, so a chase runs ball to ball.
    EveryLamp,
}

/// Where the lights' colours come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum RigColours {
    /// Each light keeps its own colour.
    #[default]
    TheLights,
    /// The rig's list of colours, handed out by the pattern.
    TheList,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct RigSettings {
    /// What the rig does to its lights.
    #[setting(label = "Pattern", simple, group = "Pattern")]
    pub pattern: Pattern,
    /// What moves the pattern on a step.
    #[setting(label = "Steps on", simple, group = "Pattern")]
    pub steps_on: StepsOn,
    /// How many beats a step lasts: 0.5 steps on every half beat.
    #[setting(
        label = "Every",
        when = "steps_on=beats",
        range(0.001, 1e9),
        span(0.25, 4),
        unit = "beats",
        keyframe,
        group = "Pattern"
    )]
    pub every_beats: f32,
    /// How many bars a step lasts.
    #[setting(
        label = "Every",
        when = "steps_on=bars",
        range(0.001, 1e9),
        span(0.25, 4),
        unit = "bars",
        keyframe,
        group = "Pattern"
    )]
    pub every_bars: f32,
    /// How many seconds a step lasts.
    #[setting(
        label = "Every",
        when = "steps_on=seconds",
        range(0.001, 1e9),
        span(0.05, 4),
        unit = "s",
        keyframe,
        group = "Pattern"
    )]
    pub every_seconds: f32,
    /// How many hits make a step.
    #[setting(
        label = "Every",
        when = "steps_on=hits",
        range(1, 1e9),
        span(1, 16),
        unit = "hits",
        keyframe,
        group = "Pattern"
    )]
    pub every_hits: u32,
    /// Whose hits move the pattern on.
    #[setting(label = "Hits from", when = "steps_on=hits", group = "Pattern")]
    pub heard_from: Target,
    /// Hits closer together than this count once.
    #[setting(
        label = "Least gap",
        when = "steps_on=hits",
        range(0, 1e9),
        span(0, 1),
        unit = "s",
        keyframe,
        group = "Pattern"
    )]
    pub least_gap: f32,
    /// How long a step lasts after its hit, for flashes and tails.
    #[setting(
        label = "Step lasts",
        when = "steps_on=hits",
        range(0.001, 1e9),
        span(0.05, 2),
        unit = "s",
        keyframe,
        group = "Pattern"
    )]
    pub step_lasts: f32,
    /// The order the pattern goes through the lights.
    #[setting(label = "Order", when = "pattern!=strobe", group = "Pattern")]
    pub order: Order,
    /// What the pattern counts as one light.
    #[setting(label = "Steps across", group = "Pattern")]
    pub across: Across,
    /// How many lights a chase lights at once.
    #[setting(label = "Lit at once", when = "pattern=chase", range(0, 1e9), span(1, 8), keyframe, group = "Pattern")]
    pub lit_at_once: f32,
    /// How many steps a light takes to fade out after it goes off: 0 snaps off.
    #[setting(
        label = "Tail",
        when = "pattern=chase or pattern=strobe or pattern=random",
        range(0, 1e9),
        span(0, 4),
        unit = "steps",
        keyframe,
        group = "Pattern"
    )]
    pub tail: f32,
    /// How much of a step a strobe flash lasts.
    #[setting(label = "Flash length", when = "pattern=strobe", span(0.02, 1), unit = "×", keyframe, group = "Pattern")]
    pub flash_length: f32,
    /// How many of the lights are on in a step.
    #[setting(label = "Lit share", when = "pattern=random", span(0, 1), keyframe, group = "Pattern")]
    pub lit_share: f32,
    /// How far a sweep turns the lights, end to end.
    #[setting(label = "Sweep angle", when = "pattern=sweep", span(0, 180), unit = "°", keyframe, group = "Pattern")]
    pub sweep_angle: f32,
    /// How far a sweep moves the lights side to side, end to end.
    #[setting(label = "Sway", when = "pattern=sweep", span(0, 600), unit = "px", keyframe, group = "Pattern")]
    pub sway: f32,
    /// How far behind its neighbour each light swings: 0 all together, 0.5 every other one opposite.
    #[setting(label = "Offset", when = "pattern=sweep", span(0, 1), keyframe, group = "Pattern")]
    pub offset: f32,
    /// Shuffles the Shuffled order and the Random pattern.
    #[setting(label = "Seed", when = "pattern=random or order=shuffled", span(0, 100), group = "Pattern")]
    pub seed: u32,
    /// Where the lights' colours come from.
    #[setting(label = "Colours from", group = "Colours")]
    pub colours_from: RigColours,
    /// The rig's colours, handed out in order.
    #[setting(label = "Colours", when = "colours_from=the_list", keyframe, group = "Colours")]
    pub colours: Gradient,
    /// How bright the whole rig is.
    #[setting(label = "Brightness", span(0, 2), unit = "×", simple, keyframe, pulse, group = "Colours")]
    pub brightness: f32,
    /// How bright a light stays while the pattern has it off.
    #[setting(label = "Off level", span(0, 1), keyframe, group = "Colours")]
    pub off_level: f32,
}

impl Default for RigSettings {
    fn default() -> Self {
        Self {
            pattern: Pattern::Chase,
            steps_on: StepsOn::Beats,
            every_beats: 1.0,
            every_bars: 1.0,
            every_seconds: 0.5,
            every_hits: 1,
            heard_from: Target::Kind(KindId::from("balls")),
            least_gap: 0.08,
            step_lasts: 0.3,
            order: Order::Forward,
            across: Across::Lights,
            lit_at_once: 1.0,
            tail: 1.0,
            flash_length: 0.25,
            lit_share: 0.5,
            sweep_angle: 60.0,
            sway: 0.0,
            offset: 0.25,
            seed: 1,
            colours_from: RigColours::TheLights,
            colours: Gradient::even(&[
                Color::rgb(0xff, 0x4d, 0x6d),
                Color::rgb(0xff, 0xb1, 0x3b),
                Color::rgb(0x3b, 0xd1, 0xff),
                Color::rgb(0xa8, 0x5c, 0xff),
            ]),
            brightness: 1.0,
            off_level: 0.1,
        }
    }
}

/// Where a new rig's four spots hang, across the top of the picture.
pub const STARTER_SPOTS: [f32; 4] = [-360.0, -120.0, 120.0, 360.0];
/// How high up a new rig's spots hang.
pub const STARTER_Y: f32 = -760.0;

/// Adds a rig under `parent` with four Spot lights in a row across the top, pointing down, so its
/// pattern shows at once. Adding it gives the scene its Lighting card when it has none.
pub fn add_rig(
    scene: &mut bpe_core::Scene,
    reg: &bpe_core::Registry,
    parent: bpe_core::ThingId,
) -> Result<bpe_core::ThingId, bpe_core::EditError> {
    use crate::things::light::{LightSettings, LightType, add_light};
    let rig = crate::add_thing(scene, reg, RIG, parent)?;
    let name = scene.free_name("Rig");
    if let Some(t) = scene.things.get_mut(&rig) {
        t.name = name;
    }
    for x in STARTER_SPOTS {
        let id = add_light(scene, reg, LightType::Spot, rig)?;
        let rides = fixtures::rides_anything(scene, id);
        if let Some(t) = scene.things.get_mut(&id) {
            let mut s = LightSettings::from_params(&t.params);
            // Riding a thing, the spots sit right on what they ride.
            if !rides {
                s.position = Vec2::new(x, STARTER_Y);
            }
            s.cone = 40.0;
            s.reach = 1800.0;
            s.direction = 90.0;
            t.params = s.to_params();
        }
    }
    Ok(rig)
}

impl ThingKind for RigKind {
    fn id(&self) -> &'static str {
        RIG
    }
    fn name(&self) -> &'static str {
        "Rig"
    }
    fn icon(&self) -> &'static str {
        "rig"
    }
    fn help(&self) -> &'static str {
        "A rig of lights: plays a light show on the lights inside it (chase, strobe, sweep, colours) on the beat \
         or on hits."
    }
    fn settings(&self) -> &'static [SettingDef] {
        RigSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[]
    }
    fn family(&self) -> &'static str {
        "Lights"
    }
    fn can_stick_to_screen(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_settings_have_hard_limits_only_where_the_maths_breaks() {
        for d in RigSettings::defs() {
            if let Some((lo, hi)) = d.range {
                assert!(
                    matches!(
                        d.key,
                        "every_beats"
                            | "every_bars"
                            | "every_seconds"
                            | "every_hits"
                            | "least_gap"
                            | "step_lasts"
                            | "lit_at_once"
                            | "tail"
                    ),
                    "{} has a hard range {lo}..{hi}",
                    d.key
                );
            }
        }
    }
}
