//! The modulators besides the ADSR: Distance (how near the balls are), LFO (a wave of its own)
//! and Hit (a number taken from each hit). Each is a layer of a shape's sound card, in
//! `LayerSection::Modulator`, with its settings under layered keys. Up to [`MAX_OF_EACH`] of
//! each kind.

use super::voice::VOICE;
use bpe_core::{
    EditError, LayerId, LayerKind, LayerSection, ModRoute, ROUTES_KEY, Registry, Scene, SettingDef, Settings, Slot,
    SlotId, Target, ThingId, Value, layered_key,
};

/// The layer kinds' ids.
pub const DISTANCE: &str = "mod.distance";
pub const LFO: &str = "mod.lfo";
pub const HIT: &str = "mod.hit";
/// Every modulator kind, in the order the + menu offers them.
pub const MOD_KINDS: [&str; 4] = [super::adsr::ADSR, DISTANCE, LFO, HIT];
/// The most modulators of one kind a sound has.
pub const MAX_OF_EACH: usize = 8;
/// Where a Distance's Far starts when it is turned on, in scene pixels.
pub const FAR_ON: f64 = 200.0;

/// Where a Distance measures from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bpe_core::Choice)]
pub enum Measure {
    /// The shape's middle.
    #[default]
    Centre,
    /// The nearest point of the shape's outline, inside or out.
    Edge,
}

/// Which ball (or balls) a Distance follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bpe_core::Choice)]
pub enum Follows {
    /// The nearest ball.
    #[default]
    Nearest,
    /// The ball whose hit played the note (one value per note).
    EachNotesBall,
    /// The balls' average distance.
    Average,
    /// How many balls are within Far, full at Full at.
    HowMany,
}

/// An LFO's wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bpe_core::Choice)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    /// A new random value each cycle, held (sample and hold).
    Random,
    /// Random values each cycle, glided between.
    SmoothRandom,
}

impl LfoShape {
    pub const ALL: [LfoShape; 6] =
        [LfoShape::Sine, LfoShape::Triangle, LfoShape::Saw, LfoShape::Square, LfoShape::Random, LfoShape::SmoothRandom];

    pub fn label(self) -> &'static str {
        match self {
            LfoShape::Sine => "Sine",
            LfoShape::Triangle => "Triangle",
            LfoShape::Saw => "Saw",
            LfoShape::Square => "Square",
            LfoShape::Random => "Random",
            LfoShape::SmoothRandom => "Smooth random",
        }
    }
}

/// A tempo-synced LFO's cycle: a note length.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bpe_core::Choice)]
pub enum NoteLength {
    N32,
    N16,
    N8,
    #[default]
    N4,
    N2,
    Bar1,
    Bar2,
    Bar4,
    Bar8,
}

impl NoteLength {
    pub const ALL: [NoteLength; 9] = [
        NoteLength::N32,
        NoteLength::N16,
        NoteLength::N8,
        NoteLength::N4,
        NoteLength::N2,
        NoteLength::Bar1,
        NoteLength::Bar2,
        NoteLength::Bar4,
        NoteLength::Bar8,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NoteLength::N32 => "1/32",
            NoteLength::N16 => "1/16",
            NoteLength::N8 => "1/8",
            NoteLength::N4 => "1/4",
            NoteLength::N2 => "1/2",
            NoteLength::Bar1 => "1 bar",
            NoteLength::Bar2 => "2 bars",
            NoteLength::Bar4 => "4 bars",
            NoteLength::Bar8 => "8 bars",
        }
    }

    /// Its length in whole notes (a bar of 4/4 is 1).
    pub fn wholes(self) -> f64 {
        match self {
            NoteLength::N32 => 1.0 / 32.0,
            NoteLength::N16 => 1.0 / 16.0,
            NoteLength::N8 => 1.0 / 8.0,
            NoteLength::N4 => 0.25,
            NoteLength::N2 => 0.5,
            NoteLength::Bar1 => 1.0,
            NoteLength::Bar2 => 2.0,
            NoteLength::Bar4 => 4.0,
            NoteLength::Bar8 => 8.0,
        }
    }

    /// Whether it counts in bars (of the scene's time signature) rather than notes.
    pub fn is_bars(self) -> bool {
        matches!(self, NoteLength::Bar1 | NoteLength::Bar2 | NoteLength::Bar4 | NoteLength::Bar8)
    }
}

/// Straight, dotted (half as long again) or triplet (two thirds).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bpe_core::Choice)]
pub enum NoteFeel {
    #[default]
    Straight,
    Dotted,
    Triplet,
}

impl NoteFeel {
    pub fn factor(self) -> f64 {
        match self {
            NoteFeel::Straight => 1.0,
            NoteFeel::Dotted => 1.5,
            NoteFeel::Triplet => 2.0 / 3.0,
        }
    }
}

/// When an LFO's wave starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bpe_core::Choice)]
pub enum LfoStart {
    /// From Phase on every hit on the shape (one wave for the shape).
    Restart,
    /// Running from the start of the clip, whatever happens.
    #[default]
    Free,
    /// One wave per note, from the note's start.
    PerNote,
}

/// What a Hit modulator takes from each hit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bpe_core::Choice)]
pub enum HitPick {
    /// How hard the ball hit.
    #[default]
    Strength,
    /// Where on the edge, 0..1 clockwise from the first corner.
    Place,
    /// Which side was hit.
    Side,
    /// The ball's number in its group.
    BallNumber,
    /// A random number per hit.
    Random,
}

impl HitPick {
    pub const ALL: [HitPick; 5] =
        [HitPick::Strength, HitPick::Place, HitPick::Side, HitPick::BallNumber, HitPick::Random];

    pub fn label(self) -> &'static str {
        match self {
            HitPick::Strength => "Strength",
            HitPick::Place => "Place",
            HitPick::Side => "Side",
            HitPick::BallNumber => "Ball number",
            HitPick::Random => "Random",
        }
    }
}

/// How near the balls are: 1 at Near or closer, 0 at Far or beyond.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DistanceSettings {
    /// From the shape's middle or its nearest edge.
    #[setting(label = "Measure")]
    pub measure: Measure,
    /// Closer than this is full.
    #[setting(label = "Near", range(0, 4000), unit = "px", keyframe)]
    pub near: f32,
    /// Further than this is nothing.
    #[setting(label = "Far", range(0, 4000), unit = "px", keyframe)]
    pub far: f32,
    /// Bends the way from Far to Near (0 is a smooth S).
    #[setting(label = "Curve", range(-100, 100), keyframe)]
    pub curve: f32,
    /// How long it takes to follow a change.
    #[setting(label = "Smooth", range(0, 500), unit = "ms", keyframe)]
    pub smooth: f32,
    /// Full when far, nothing when near.
    #[setting(label = "Invert")]
    pub invert: bool,
    /// The nearest ball, each note's own ball, the average, or how many are in range.
    #[setting(label = "Follows")]
    pub follows: Follows,
    /// How many balls in range count as full.
    #[setting(label = "Full at", range(1, 64), keyframe, when = "follows=how_many")]
    pub full_at: i32,
    /// Which balls count.
    #[setting(label = "Heard from")]
    pub heard_from: Target,
}

impl Default for DistanceSettings {
    fn default() -> Self {
        Self {
            measure: Measure::Centre,
            near: 0.0,
            far: FAR_ON as f32,
            curve: 0.0,
            smooth: 20.0,
            invert: false,
            follows: Follows::Nearest,
            full_at: 4,
            heard_from: Target::Anything,
        }
    }
}

/// A wave of its own.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LfoSettings {
    #[setting(label = "Shape")]
    pub shape: LfoShape,
    /// Set in Hz, or in note lengths at the scene's tempo.
    #[setting(label = "Tempo sync")]
    pub sync: bool,
    #[setting(label = "Rate", range(0.01, 40), unit = "Hz", keyframe, when = "!sync")]
    pub rate_hz: f32,
    #[setting(label = "Rate", when = "sync")]
    pub rate_note: NoteLength,
    #[setting(label = "Feel", when = "sync")]
    pub rate_feel: NoteFeel,
    /// Where in the wave it starts.
    #[setting(label = "Phase", range(0, 360), unit = "deg", keyframe)]
    pub phase: f32,
    /// How long it takes to grow to full, after Delay.
    #[setting(label = "Fade in", range(0, 10), unit = "s", keyframe)]
    pub fade_in: f32,
    /// A wait before it starts moving.
    #[setting(label = "Delay", range(0, 10), unit = "s", keyframe)]
    pub delay: f32,
    /// Rounds off its corners.
    #[setting(label = "Smooth", range(0, 100), unit = "%", keyframe)]
    pub smooth: f32,
    /// From each hit, running free, or one per note.
    #[setting(label = "Start")]
    pub start: LfoStart,
}

impl Default for LfoSettings {
    fn default() -> Self {
        Self {
            shape: LfoShape::Sine,
            sync: false,
            rate_hz: 2.0,
            rate_note: NoteLength::N4,
            rate_feel: NoteFeel::Straight,
            phase: 0.0,
            fade_in: 0.0,
            delay: 0.0,
            smooth: 0.0,
            start: LfoStart::Free,
        }
    }
}

/// A number from each hit, held for the note it plays.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HitSettings {
    #[setting(label = "Takes")]
    pub pick: HitPick,
    /// The hit's number that counts as nothing.
    #[setting(label = "Low", range(0, 1), keyframe)]
    pub low: f32,
    /// The hit's number that counts as full.
    #[setting(label = "High", range(0, 1), keyframe)]
    pub high: f32,
    /// Bends the way from Low to High (0 is straight).
    #[setting(label = "Curve", range(-100, 100), keyframe)]
    pub curve: f32,
    /// How long it takes to move to a new hit's number (Mono).
    #[setting(label = "Smooth", range(0, 500), unit = "ms", keyframe)]
    pub smooth: f32,
}

impl Default for HitSettings {
    fn default() -> Self {
        Self { pick: HitPick::Strength, low: 0.0, high: 1.0, curve: 0.0, smooth: 0.0 }
    }
}

macro_rules! mod_kind {
    ($kind:ident, $id:expr, $name:expr, $help:expr, $settings:ty) => {
        /// The layer kind.
        pub struct $kind;

        impl LayerKind for $kind {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> &'static str {
                $name
            }
            fn help(&self) -> &'static str {
                $help
            }
            fn settings(&self) -> &'static [SettingDef] {
                <$settings>::defs()
            }
            fn section(&self) -> LayerSection {
                LayerSection::Modulator
            }
            fn repeats(&self) -> bool {
                true
            }
            fn card(&self) -> &'static str {
                VOICE
            }
            fn maskable(&self) -> bool {
                false
            }
            fn per_ball(&self) -> bool {
                false
            }
        }
    };
}

mod_kind!(DistanceKind, DISTANCE, "Distance", "Moves knobs as balls come near the shape.", DistanceSettings);
mod_kind!(LfoKind, LFO, "LFO", "A wave of its own that moves knobs up and down.", LfoSettings);
mod_kind!(HitKind, HIT, "Hit", "A number taken from each hit, held for its note.", HitSettings);

/// The name a new modulator of `kind` shows: "ADSR", "Distance", "LFO" or "Hit".
pub fn kind_name(kind: &str) -> &'static str {
    match kind {
        DISTANCE => "Distance",
        LFO => "LFO",
        HIT => "Hit",
        _ => "ADSR",
    }
}

/// How many modulators of `kind` the card has.
pub fn count_of(card: &Slot, kind: &str) -> usize {
    card.layers.iter().filter(|l| l.kind == kind).count()
}

/// The first free "<Kind> <n>" name in the card.
fn numbered_name(card: &Slot, kind: &str) -> String {
    let base = kind_name(kind);
    (1u32..).map(|n| format!("{base} {n}")).find(|n| !card.layers.iter().any(|l| &l.name == n)).expect("a free number")
}

/// Adds a modulator of `kind` after the others, named "<Kind> <n>" and switched on (a Distance
/// is turned on with [`turn_on_distance`]). Refused at [`MAX_OF_EACH`] of that kind.
pub fn add_modulator(scene: &mut Scene, reg: &Registry, thing: ThingId, kind: &str) -> Result<LayerId, EditError> {
    let card = scene.card(thing, SlotId::Sound).ok_or(EditError::NoSlot(SlotId::Sound))?;
    if !MOD_KINDS.contains(&kind) {
        return Err(EditError::UnknownKind(kind.into()));
    }
    if count_of(card, kind) >= MAX_OF_EACH {
        return Err(EditError::NotAccepted);
    }
    let name = numbered_name(card, kind);
    let id = scene.add_layer_in(reg, thing, SlotId::Sound, kind, None)?;
    scene.rename_layer(reg, thing, SlotId::Sound, id, &name)?;
    if kind == DISTANCE {
        let card = scene.card_mut(thing, SlotId::Sound).expect("checked above");
        turn_on_distance(card, id);
    }
    Ok(id)
}

/// Turns the Distance `layer` on: Far 200 px, and routed to Osc A Gain and Osc B Gain at full
/// amount (each a VCA, so the shape fades from 0 dB near to silence far). Routes it already has
/// are left as they are.
pub fn turn_on_distance(card: &mut Slot, layer: LayerId) {
    if let Some(l) = card.layers.iter_mut().find(|l| l.id == layer) {
        l.enabled = true;
    }
    card.params.set(&layered_key(layer, "far"), Value::Number(FAR_ON));
    let mut routes = match card.params.get(ROUTES_KEY) {
        Some(Value::Routes(r)) => r.clone(),
        _ => Vec::new(),
    };
    for target in ["a_gain", "b_gain"] {
        if !routes.iter().any(|r| r.source == layer && r.target == target) {
            routes.push(ModRoute { source: layer, target: target.into(), amount: 1.0, bipolar: false, invert: false });
        }
    }
    card.params.set(ROUTES_KEY, Value::Routes(routes));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::{ADSR, starter_voice};
    use bpe_core::{Address, PulseSource, SoundSource};

    fn shape_with_sound() -> (Scene, Registry, ThingId) {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let id = scene.add_thing(&reg, crate::things::shape::KIND, ThingId::ROOT).unwrap();
        scene.things.get_mut(&id).unwrap().slots.insert(SlotId::Sound, starter_voice(&reg));
        (scene, reg, id)
    }

    fn routes(scene: &Scene, id: ThingId) -> Vec<ModRoute> {
        match scene.card(id, SlotId::Sound).unwrap().params.get(ROUTES_KEY) {
            Some(Value::Routes(r)) => r.clone(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn starter_has_six_tiles_only_adsr1_on() {
        let reg = crate::registry();
        let card = starter_voice(&reg);
        let tiles: Vec<(&str, &str, bool)> =
            card.layers.iter().map(|l| (l.name.as_str(), l.kind.as_str(), l.enabled)).collect();
        assert_eq!(
            tiles,
            [
                ("ADSR 1", ADSR, true),
                ("ADSR 2", ADSR, false),
                ("Distance 1", DISTANCE, false),
                ("Distance 2", DISTANCE, false),
                ("LFO 1", LFO, false),
                ("LFO 2", LFO, false),
            ]
        );
        for l in &card.layers {
            assert_eq!(reg.layer(&l.kind).unwrap().section(), LayerSection::Modulator, "{}", l.name);
        }
    }

    #[test]
    fn turning_on_distance_routes_both_gains() {
        let (mut scene, _reg, id) = shape_with_sound();
        let card = scene.card_mut(id, SlotId::Sound).unwrap();
        let d = card.layers.iter().find(|l| l.kind == DISTANCE).unwrap().id;
        turn_on_distance(card, d);
        let card = scene.card(id, SlotId::Sound).unwrap();
        assert!(card.layer(d).unwrap().enabled);
        assert_eq!(card.params.get(&layered_key(d, "far")), Some(&Value::Number(200.0)));
        let mine: Vec<(String, f32)> =
            routes(&scene, id).into_iter().filter(|r| r.source == d).map(|r| (r.target, r.amount)).collect();
        assert_eq!(mine, [("a_gain".to_owned(), 1.0), ("b_gain".to_owned(), 1.0)]);
        // Again: no doubled routes.
        turn_on_distance(scene.card_mut(id, SlotId::Sound).unwrap(), d);
        assert_eq!(routes(&scene, id).iter().filter(|r| r.source == d).count(), 2);
    }

    #[test]
    fn at_most_eight_of_each() {
        let (mut scene, reg, id) = shape_with_sound();
        for n in 3..=8 {
            let l = add_modulator(&mut scene, &reg, id, LFO).unwrap();
            assert_eq!(scene.card(id, SlotId::Sound).unwrap().layer(l).unwrap().name, format!("LFO {n}"));
        }
        assert_eq!(add_modulator(&mut scene, &reg, id, LFO), Err(EditError::NotAccepted));
        // Other kinds still have room; a Hit starts at 1.
        let h = add_modulator(&mut scene, &reg, id, HIT).unwrap();
        let card = scene.card(id, SlotId::Sound).unwrap();
        assert_eq!(card.layer(h).unwrap().name, "Hit 1");
        assert!(card.layer(h).unwrap().enabled);
        assert_eq!(count_of(card, LFO), 8);
        // A new Distance is turned on and routed.
        let d = add_modulator(&mut scene, &reg, id, DISTANCE).unwrap();
        assert_eq!(routes(&scene, id).iter().filter(|r| r.source == d).count(), 2);
        // Modulators stay together, after the starter ones and before nothing else.
        let card = scene.card(id, SlotId::Sound).unwrap();
        assert!(card.layers.iter().all(|l| MOD_KINDS.contains(&l.kind.as_str())));
    }

    #[test]
    fn deleting_a_modulator_removes_its_routes_and_follow_pulses_in_one_edit() {
        let (mut scene, reg, id) = shape_with_sound();
        let lfo = add_modulator(&mut scene, &reg, id, LFO).unwrap();
        let other = add_modulator(&mut scene, &reg, id, LFO).unwrap();
        // The LFO moves Osc A cents and the other LFO's rate; the other one moves the cutoff of nothing.
        let mut r = routes(&scene, id);
        r.push(ModRoute { source: lfo, target: "a_cents".into(), amount: 0.5, bipolar: false, invert: false });
        r.push(ModRoute {
            source: other,
            target: layered_key(lfo, "rate_hz"),
            amount: 0.2,
            bipolar: false,
            invert: false,
        });
        r.push(ModRoute { source: other, target: "a_cents".into(), amount: 0.1, bipolar: false, invert: false });
        scene.card_mut(id, SlotId::Sound).unwrap().params.set(ROUTES_KEY, Value::Routes(r));
        // A ball's Look follows the LFO, another setting follows the other LFO.
        let ball = scene.add_thing(&reg, "balls", ThingId::ROOT).unwrap();
        let follows = |l| PulseSource::Sound { shape: id, source: SoundSource::Modulator(l) };
        let a = Address { thing: ball, slot: None, key: "radius".into() };
        let b = Address { thing: ball, slot: None, key: "speed".into() };
        scene.pulses.insert(a.clone(), bpe_core::Pulse { source: follows(lfo), ..Default::default() });
        scene.pulses.insert(b.clone(), bpe_core::Pulse { source: follows(other), ..Default::default() });

        scene.remove_layer(&reg, id, SlotId::Sound, lfo).unwrap();
        let left = routes(&scene, id);
        assert!(left.iter().all(|r| r.source != lfo), "routes from it go");
        assert!(left.iter().all(|r| !r.target.starts_with(&format!("{lfo}/"))), "routes to its knobs go");
        assert!(left.iter().any(|r| r.source == other && r.target == "a_cents"), "others stay");
        assert!(!scene.pulses.contains_key(&a), "a setting following it stops");
        assert!(scene.pulses.contains_key(&b), "one following another modulator stays");
    }
}
