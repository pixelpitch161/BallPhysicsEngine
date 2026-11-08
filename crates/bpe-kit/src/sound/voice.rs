//! A shape's sound: the `sound.voice` card. Two oscillators (one [`OscSettings`] expanded twice,
//! under `a_` and `b_`), how hits play notes (`play_…`), the notes (`notes_…`), the output
//! (`out_…`) and the modulation routes. Its modulators are layers of the card (see
//! [`super::adsr`]).

use super::adsr::{ADSR, AdsrSettings};
use super::mods::{DISTANCE, LFO};
use super::notes::{BeatsOrBars, Change, Plays, Root, Scale, SlotOrder, StrumDir};
use bpe_core::{
    Choice, GroupOpts, Layer, LayerId, ModRoute, ModuleKind, NoteSlot, Params, ROUTES_KEY, Registry, SettingDef,
    SettingGroup, Settings, Slot, SlotId, Target, Value,
};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// The card kind's id.
pub const VOICE: &str = "sound.voice";

/// The quietest a gain knob shows; all the way down is silence.
pub const SILENT_DB: f32 = -60.0;

/// An oscillator's wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Wave {
    Sine,
    #[default]
    Triangle,
    Saw,
    Square,
    Noise,
}

impl Wave {
    pub const ALL: [Wave; 5] = [Wave::Sine, Wave::Triangle, Wave::Saw, Wave::Square, Wave::Noise];

    pub fn label(self) -> &'static str {
        match self {
            Wave::Sine => "Sine",
            Wave::Triangle => "Triangle",
            Wave::Saw => "Saw",
            Wave::Square => "Square",
            Wave::Noise => "Noise",
        }
    }
}

/// The colour of a Noise oscillator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum NoiseColour {
    #[default]
    White,
    Pink,
}

/// How Warp bends the wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum WarpMode {
    #[default]
    Sync,
    BendUp,
    BendDown,
    AsymUp,
    AsymDown,
    Pwm,
    Mirror,
    Flip,
    Quantize,
    Squeeze,
}

impl WarpMode {
    pub const ALL: [WarpMode; 10] = [
        WarpMode::Sync,
        WarpMode::BendUp,
        WarpMode::BendDown,
        WarpMode::AsymUp,
        WarpMode::AsymDown,
        WarpMode::Pwm,
        WarpMode::Mirror,
        WarpMode::Flip,
        WarpMode::Quantize,
        WarpMode::Squeeze,
    ];

    pub fn label(self) -> &'static str {
        match self {
            WarpMode::Sync => "Sync",
            WarpMode::BendUp => "Bend +",
            WarpMode::BendDown => "Bend \u{2212}",
            WarpMode::AsymUp => "Asym +",
            WarpMode::AsymDown => "Asym \u{2212}",
            WarpMode::Pwm => "PWM",
            WarpMode::Mirror => "Mirror",
            WarpMode::Flip => "Flip",
            WarpMode::Quantize => "Quantize",
            WarpMode::Squeeze => "Squeeze",
        }
    }

    /// Its place in [`WarpMode::ALL`].
    pub fn index(self) -> u8 {
        WarpMode::ALL.iter().position(|m| *m == self).unwrap_or(0) as u8
    }
}

/// Where each note starts the wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PhaseMode {
    /// At the Phase knob on every hit.
    #[default]
    Restart,
    /// Wherever it had got to.
    Free,
}

/// The sub oscillator's wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SubWave {
    #[default]
    Off,
    Sine,
    Square,
}

/// How far below the oscillator the sub plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SubOctave {
    #[default]
    One,
    Two,
}

/// Several notes at once, or one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PlayMode {
    #[default]
    Poly,
    Mono,
}

/// How loud a Particles thing's moments play (particles part 7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MomentStrength {
    /// Harder the faster the particle went into what it met (1500 px/s is full).
    #[default]
    Speed,
    /// Louder the bigger the particle (6 px across half is full).
    Size,
    /// Both: fast and big is loudest.
    SpeedAndSize,
    /// Every note as loud.
    TheSame,
}

/// When a shape's notes sound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PlayWhen {
    /// Each accepted hit plays its notes.
    #[default]
    OnHit,
    /// The slot's notes are held from the start, a drone; hits never play them again.
    Always,
    /// The slot's notes are held while a ball is in range of a Distance.
    InRange,
}

/// How long an envelope's note is held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GateMode {
    /// A set length.
    #[default]
    Fixed,
    /// Until the next hit on this shape.
    UntilNextHit,
}

/// The unit a Fixed gate is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GateUnit {
    #[default]
    Ms,
    Beats,
}

/// One oscillator. The card holds it twice, under `a_` and `b_`.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct OscSettings {
    /// Whether this oscillator plays.
    #[setting(label = "On")]
    pub on: bool,
    /// The wave it plays.
    #[setting(label = "Wave")]
    pub wave: Wave,
    /// White noise is even across the pitches; pink falls away towards the highs.
    #[setting(label = "Noise", when = "wave=noise")]
    pub noise: NoiseColour,
    /// How Warp bends the wave.
    #[setting(label = "Warp mode", when = "wave!=noise")]
    pub warp_mode: WarpMode,
    /// How far the wave is bent.
    #[setting(label = "Warp", range(0, 100), unit = "%", keyframe, when = "wave!=noise")]
    pub warp: f32,
    /// The level going into the oscillator's layers (it drives them).
    #[setting(label = "Gain", range(-60, 12), unit = "dB", keyframe)]
    pub gain: f32,
    /// The level coming out of the oscillator's layers.
    #[setting(label = "Level", range(-60, 12), unit = "dB", keyframe)]
    pub level: f32,
    /// Octaves up or down.
    #[setting(label = "Octave", range(-4, 4), keyframe)]
    pub octave: i32,
    /// Semitones up or down.
    #[setting(label = "Semi", range(-24, 24), unit = "st", keyframe)]
    pub semi: i32,
    /// Fine tuning, in hundredths of a semitone.
    #[setting(label = "Cents", range(-100, 100), unit = "ct", keyframe)]
    pub cents: f32,
    /// Where in the wave each note starts.
    #[setting(label = "Phase", range(0, 360), unit = "°", keyframe)]
    pub phase: f32,
    /// Restart the wave at Phase on every note, or let it run free.
    #[setting(label = "Phase mode")]
    pub phase_mode: PhaseMode,
    /// How many copies of the wave play together, slightly detuned.
    #[setting(label = "Unison", range(1, 16), keyframe, when = "wave!=noise")]
    pub unison: i32,
    /// How far apart the unison copies are tuned.
    #[setting(label = "Detune", range(0, 100), unit = "ct", keyframe, when = "wave!=noise")]
    pub detune: f32,
    /// How wide the unison copies spread across left and right.
    #[setting(label = "Spread", range(0, 100), unit = "%", keyframe, when = "wave!=noise")]
    pub spread: f32,
    /// The middle copy against the others.
    #[setting(label = "Blend", range(0, 100), unit = "%", keyframe, when = "wave!=noise")]
    pub blend: f32,
    /// How far each unison copy's start is shuffled.
    #[setting(label = "Random phase", range(0, 100), unit = "%", keyframe, when = "wave!=noise")]
    pub random_phase: f32,
    /// Left or right.
    #[setting(label = "Pan", range(-100, 100), keyframe)]
    pub pan: f32,
    /// A plain wave an octave or two below.
    #[setting(label = "Sub", when = "wave!=noise")]
    pub sub: SubWave,
    /// One or two octaves down.
    #[setting(label = "Sub octave", when = "wave!=noise and sub!=off")]
    pub sub_octave: SubOctave,
    /// How loud the sub is.
    #[setting(label = "Sub level", range(-60, 12), unit = "dB", keyframe, when = "wave!=noise and sub!=off")]
    pub sub_level: f32,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self {
            on: true,
            wave: Wave::Triangle,
            noise: NoiseColour::White,
            warp_mode: WarpMode::Sync,
            warp: 0.0,
            gain: 0.0,
            level: 0.0,
            octave: 0,
            semi: 0,
            cents: 0.0,
            phase: 0.0,
            phase_mode: PhaseMode::Restart,
            unison: 1,
            detune: 20.0,
            spread: 50.0,
            blend: 50.0,
            random_phase: 0.0,
            pan: 0.0,
            sub: SubWave::Off,
            sub_octave: SubOctave::One,
            sub_level: -6.0,
        }
    }
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// `when` with every setting key in it given `prefix`, then joined to `outer` (the embedding
/// field's own condition) with " and ".
fn prefixed_when(when: &str, prefix: &str, outer: &str) -> &'static str {
    let term = |t: &str| -> String {
        let (not, t) = match t.strip_prefix('!') {
            Some(rest) => ("!", rest),
            None => ("", t),
        };
        format!("{not}{prefix}{t}")
    };
    let mine: Vec<String> = when
        .split(" or ")
        .filter(|alt| !alt.is_empty())
        .map(|alt| alt.split(" and ").map(term).collect::<Vec<_>>().join(" and "))
        .collect();
    match (mine.is_empty(), outer.is_empty()) {
        (true, _) => leak(outer.to_owned()),
        (false, true) => leak(mine.join(" or ")),
        (false, false) => leak(mine.iter().map(|alt| format!("{outer} and {alt}")).collect::<Vec<_>>().join(" or ")),
    }
}

/// Every value of `s`, defaults included, by plain key.
pub(crate) fn full_params<T: Settings>(s: &T) -> Params {
    let mut p = Params(T::defs().iter().map(|d| (d.key.to_owned(), d.default.clone())).collect());
    for (k, v) in s.to_params().iter() {
        p.set(k, v.clone());
    }
    p
}

/// A plain struct of settings embedded with a key prefix, its defaults the outer struct's own.
/// The leaked defs are made once per (struct, prefix) for the program's life.
pub(crate) fn prefixed_defs<T: Settings>(o: &GroupOpts, default: &T) -> Vec<SettingDef> {
    type Made = Mutex<BTreeMap<(&'static str, &'static str), Vec<SettingDef>>>;
    static MADE: OnceLock<Made> = OnceLock::new();
    let made = MADE.get_or_init(Default::default);
    let mut made = made.lock().unwrap_or_else(|e| e.into_inner());
    let full = full_params(default);
    made.entry((std::any::type_name::<T>(), o.prefix))
        .or_insert_with(|| {
            T::defs()
                .iter()
                .map(|d| SettingDef {
                    key: leak(format!("{}{}", o.prefix, d.key)),
                    default: full.get(d.key).cloned().unwrap_or_else(|| d.default.clone()),
                    group: o.group,
                    when: prefixed_when(d.when, o.prefix, o.when),
                    ..d.clone()
                })
                .collect()
        })
        .clone()
}

impl SettingGroup for OscSettings {
    fn group_defs(o: &GroupOpts, default: &Self) -> Vec<SettingDef> {
        prefixed_defs(o, default)
    }

    fn group_read(o: &GroupOpts, default: &Self, p: &Params) -> Self {
        let mut own = full_params(default);
        for (k, v) in p.iter() {
            if let Some(plain) = k.strip_prefix(o.prefix) {
                own.set(plain, v.clone());
            }
        }
        Self::from_params(&own)
    }

    fn group_write(&self, o: &GroupOpts, default: &Self, p: &mut Params) {
        let (mine, base) = (full_params(self), full_params(default));
        for (k, v) in mine.iter() {
            if base.get(k) != Some(v) {
                p.set(&format!("{}{k}", o.prefix), v.clone());
            }
        }
    }
}

/// A shape's whole sound.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct VoiceSettings {
    /// Whether the shape makes sound. Off keeps every setting.
    #[setting(label = "Enable sound", group = "Sound")]
    pub enabled: bool,
    #[setting(paint, prefix = "a_", label = "Osc A", group = "Osc A")]
    pub a: OscSettings,
    #[setting(paint, prefix = "b_", label = "Osc B", group = "Osc B")]
    pub b: OscSettings,
    /// Several notes at once (Poly), or one at a time that glides (Mono).
    #[setting(label = "Play", group = "Playing")]
    pub play_mode: PlayMode,
    /// The most notes that sound at once; a new one takes over the oldest.
    #[setting(label = "Voices", range(1, 32), group = "Playing", keyframe)]
    pub play_voices: i32,
    /// How long Mono takes to slide to a new note.
    #[setting(label = "Glide", range(0, 2000), unit = "ms", group = "Playing", keyframe, when = "play_mode=mono")]
    pub play_glide: f32,
    /// Hits softer than this play nothing.
    #[setting(label = "Min hit strength", range(0, 100), unit = "%", group = "Playing", keyframe)]
    pub play_min_strength: f32,
    /// A ball hitting again sooner than this plays nothing.
    #[setting(label = "Retrigger guard", range(0, 500), unit = "ms", group = "Playing", keyframe)]
    pub play_retrigger: f32,
    /// Which balls' hits play notes.
    #[setting(label = "Hits from", group = "Playing")]
    pub play_heard_from: Target,
    /// When the notes sound: on each hit, always (held), or while a ball is in range.
    #[setting(label = "Plays", group = "Playing")]
    pub play_when: PlayWhen,
    /// The Distance whose range holds the notes, by its layer id ("" for the first).
    #[setting(label = "In range of", group = "Playing", when = "play_when=in_range")]
    pub play_zone: String,
    /// A Particles thing's sound: its particles hitting a ball play notes.
    #[setting(label = "Hitting a ball", group = "Playing")]
    pub moments_hit_ball: bool,
    /// Its particles hitting a shape play notes.
    #[setting(label = "Hitting a shape", group = "Playing")]
    pub moments_hit_shape: bool,
    /// Its particles landing on a shape (stopping on it) play notes.
    #[setting(label = "Landing", group = "Playing")]
    pub moments_land: bool,
    /// Each particle born in a burst plays a note.
    #[setting(label = "Being born", group = "Playing")]
    pub moments_born: bool,
    /// Each burst plays a note.
    #[setting(label = "Bursts", group = "Playing")]
    pub moments_burst: bool,
    /// The most notes its particles play a second; the strongest are kept.
    #[setting(label = "Most notes a second", range(1, 2000), unit = "per s", group = "Playing", keyframe)]
    pub moments_most: f32,
    /// What makes a particle's note louder.
    #[setting(label = "Strength from", group = "Playing")]
    pub moments_strength: MomentStrength,
    /// The notes, slot by slot (part 2 plays slot 1).
    #[setting(label = "Notes", group = "Notes")]
    pub notes_slots: Vec<NoteSlot>,
    /// The keyboard's lowest octave.
    #[setting(label = "Lowest octave", range(0, 8), group = "Notes")]
    pub notes_low_octave: i32,
    /// How many octaves the keyboard shows.
    #[setting(label = "Octaves", range(1, 8), group = "Notes")]
    pub notes_octaves: i32,
    /// The shape's key: its root note.
    #[setting(label = "Root", group = "Notes")]
    pub notes_root: Root,
    /// The shape's key: its scale. In-scale keys are marked on the keyboard.
    #[setting(label = "Scale", group = "Notes")]
    pub notes_scale: Scale,
    /// How each hit picks notes from the slot playing.
    #[setting(label = "Plays", group = "Notes")]
    pub notes_plays: Plays,
    /// How long a chord takes to strum, first note to last (0 plays them together).
    #[setting(
        label = "Strum",
        range(0, 200),
        unit = "ms",
        group = "Notes",
        keyframe,
        when = "notes_plays=all_at_once"
    )]
    pub notes_strum: f32,
    /// Which way a strum runs.
    #[setting(label = "Strum from", group = "Notes", when = "notes_plays=all_at_once")]
    pub notes_strum_dir: StrumDir,
    /// How many hits each note (or chord) plays for before the next.
    #[setting(label = "Repeat each", range(1, 16), group = "Notes")]
    pub notes_repeat: i32,
    /// How many octaves above the slot's notes the play order runs on into.
    #[setting(label = "Octave range", range(0, 3), group = "Notes")]
    pub notes_octave_range: i32,
    /// What moves the shape on to its next slot.
    #[setting(label = "Slots change", group = "Notes")]
    pub notes_change: Change,
    /// By tempo: how many beats or bars each slot lasts.
    #[setting(label = "Every", range(1, 64), group = "Notes", when = "notes_change=by_tempo")]
    pub notes_every_beats: i32,
    /// By tempo: counted in beats or in bars.
    #[setting(label = "Every unit", group = "Notes", when = "notes_change=by_tempo")]
    pub notes_every_unit: BeatsOrBars,
    /// By time: how long each slot lasts.
    #[setting(label = "Every", range(0.1, 60), unit = "s", group = "Notes", keyframe, when = "notes_change=by_time")]
    pub notes_every_seconds: f32,
    /// By hits: how many hits on this shape each slot lasts.
    #[setting(label = "Every", range(1, 256), unit = "hits", group = "Notes", when = "notes_change=by_hits")]
    pub notes_every_hits: i32,
    /// The order the slots take turns in.
    #[setting(label = "Order", group = "Notes", when = "notes_change!=only_keyframes")]
    pub notes_order: SlotOrder,
    /// Jumps to a slot where it is keyed, whatever Slots change says (its keys always jump).
    #[setting(label = "Jump to slot", range(1, 16), group = "Notes", keyframe, hold)]
    pub notes_jump: i32,
    /// The shape's loudness.
    #[setting(label = "Volume", range(-60, 6), unit = "dB", group = "Output", keyframe)]
    pub out_volume: f32,
    /// Left or right.
    #[setting(label = "Pan", range(-100, 100), group = "Output", keyframe)]
    pub out_pan: f32,
    /// Pans each note towards where its hit is across the picture.
    #[setting(label = "Follow hit", range(0, 100), unit = "%", group = "Output", keyframe)]
    pub out_follow_hit: f32,
    /// The bus the shape plays into.
    #[setting(label = "Bus", range(1, 3), group = "Output")]
    pub out_bus: i32,
    /// How much goes to Bus 1 besides the main bus (all the way down is off).
    #[setting(label = "Send 1", range(-60, 6), unit = "dB", group = "Output", keyframe)]
    pub out_send1: f32,
    #[setting(label = "Send 2", range(-60, 6), unit = "dB", group = "Output", keyframe)]
    pub out_send2: f32,
    #[setting(label = "Send 3", range(-60, 6), unit = "dB", group = "Output", keyframe)]
    pub out_send3: f32,
    /// Which modulator moves which knob.
    #[setting(label = "Modulation", group = "Modulation")]
    pub routes: Vec<ModRoute>,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            a: OscSettings::default(),
            b: OscSettings { on: false, wave: Wave::Saw, ..OscSettings::default() },
            play_mode: PlayMode::Poly,
            play_voices: 16,
            play_glide: 0.0,
            play_min_strength: 0.0,
            play_retrigger: 30.0,
            play_heard_from: Target::Anything,
            play_when: PlayWhen::OnHit,
            play_zone: String::new(),
            moments_hit_ball: true,
            moments_hit_shape: true,
            moments_land: true,
            moments_born: false,
            moments_burst: false,
            moments_most: 30.0,
            moments_strength: MomentStrength::Speed,
            notes_slots: vec![NoteSlot { notes: vec![60], order_clicked: Vec::new() }],
            notes_low_octave: 4,
            notes_octaves: 3,
            notes_root: Root::C,
            notes_scale: Scale::Major,
            notes_plays: Plays::Random,
            notes_strum: 0.0,
            notes_strum_dir: StrumDir::Up,
            notes_repeat: 1,
            notes_octave_range: 0,
            notes_change: Change::ByTempo,
            notes_every_beats: 1,
            notes_every_unit: BeatsOrBars::Bars,
            notes_every_seconds: 2.0,
            notes_every_hits: 4,
            notes_order: SlotOrder::Forward,
            notes_jump: 1,
            out_volume: 0.0,
            out_pan: 0.0,
            out_follow_hit: 0.0,
            out_bus: 1,
            out_send1: SILENT_DB,
            out_send2: SILENT_DB,
            out_send3: SILENT_DB,
            routes: Vec::new(),
        }
    }
}

/// The card kind.
pub struct VoiceKind;

impl ModuleKind for VoiceKind {
    fn id(&self) -> &'static str {
        VOICE
    }
    fn name(&self) -> &'static str {
        "Sound"
    }
    fn help(&self) -> &'static str {
        "The shape's sound: two oscillators, envelopes and the notes its hits play."
    }
    fn slot(&self) -> SlotId {
        SlotId::Sound
    }
    fn settings(&self) -> &'static [SettingDef] {
        VoiceSettings::defs()
    }
    /// Shapes play hits; Particles things play their particles' moments (particles part 7).
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == crate::things::shape::KIND
            || thing_kind == crate::things::particles::KIND
            || thing_kind == crate::things::text::KIND
    }
}

/// The card "Enable sound" adds: Osc A Triangle, Osc B off, ADSR 1 on Osc A Gain (ADSR 2,
/// Distance 1 and 2 and LFO 1 and 2 off), note C5, Bus 1.
pub fn starter_voice(reg: &Registry) -> Slot {
    debug_assert!(reg.layer(ADSR).is_some(), "the kit has the ADSR layer");
    let layer = |id: u32, kind: &str, name: &str, enabled: bool| Layer {
        id: LayerId(id),
        kind: kind.into(),
        name: name.into(),
        enabled,
        ..Layer::default()
    };
    let mut card = Slot::new(VOICE);
    card.layers = vec![
        layer(0, ADSR, "ADSR 1", true),
        layer(1, ADSR, "ADSR 2", false),
        layer(2, DISTANCE, "Distance 1", false),
        layer(3, DISTANCE, "Distance 2", false),
        layer(4, LFO, "LFO 1", false),
        layer(5, LFO, "LFO 2", false),
    ];
    let route = ModRoute { source: LayerId(0), target: "a_gain".into(), amount: 1.0, bipolar: false, invert: false };
    card.params.set(ROUTES_KEY, Value::Routes(vec![route]));
    let _ = AdsrSettings::default();
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::adsr::AdsrSettings;
    use bpe_core::{LayerSection, ThingId, layered_key};

    fn reg() -> Registry {
        crate::registry()
    }

    #[test]
    fn starter_patch_matches_the_spec() {
        let reg = reg();
        let card = starter_voice(&reg);
        assert_eq!(card.kind, VOICE);
        let v = VoiceSettings::from_params(&card.params);
        assert!(v.enabled);
        assert!(v.a.on && v.a.wave == Wave::Triangle);
        assert!(!v.b.on);
        assert_eq!(v.notes_slots, vec![NoteSlot { notes: vec![60], order_clicked: vec![] }]);
        assert_eq!((v.out_bus, v.play_mode, v.play_voices, v.play_retrigger), (1, PlayMode::Poly, 16, 30.0));
        assert_eq!(v.routes.len(), 1);
        let r = &v.routes[0];
        assert_eq!(
            (r.source, r.target.as_str(), r.amount, r.bipolar, r.invert),
            (LayerId(0), "a_gain", 1.0, false, false)
        );
        let names: Vec<(&str, bool)> = card.layers.iter().take(2).map(|l| (l.name.as_str(), l.enabled)).collect();
        assert_eq!(names, [("ADSR 1", true), ("ADSR 2", false)]);
        assert!(card.layers.iter().take(2).all(|l| l.kind == ADSR));
        let kind = reg.layer(ADSR).unwrap();
        assert_eq!(kind.section(), LayerSection::Modulator);
        let adsr = AdsrSettings::from_params(&Params::default());
        assert_eq!(
            (adsr.delay, adsr.attack, adsr.hold, adsr.decay, adsr.sustain, adsr.release),
            (0.0, 2.0, 0.0, 350.0, 0.0, 200.0)
        );
        assert_eq!((adsr.gate_mode, adsr.gate_unit, adsr.gate_ms), (GateMode::Fixed, GateUnit::Ms, 100.0));
        // No layer param differs from the ADSR's defaults.
        assert!(card.params.iter().all(|(k, _)| k == ROUTES_KEY), "{:?}", card.params);
    }

    #[test]
    fn notes_defaults() {
        let v = VoiceSettings::from_params(&Params::default());
        assert_eq!((v.notes_root, v.notes_scale, v.notes_plays), (Root::C, Scale::Major, Plays::Random));
        assert_eq!((v.notes_strum, v.notes_strum_dir, v.notes_repeat, v.notes_octave_range), (0.0, StrumDir::Up, 1, 0));
        assert_eq!((v.notes_change, v.notes_every_beats, v.notes_every_unit), (Change::ByTempo, 1, BeatsOrBars::Bars));
        assert_eq!((v.notes_every_seconds, v.notes_every_hits, v.notes_order), (2.0, 4, SlotOrder::Forward));
        assert_eq!((v.notes_low_octave, v.notes_octaves), (4, 3), "the keyboard shows C4..B6");
        let defs = VoiceSettings::defs();
        let def = |k: &str| defs.iter().find(|d| d.key == k).unwrap_or_else(|| panic!("{k}"));
        assert_eq!(def("notes_strum").range, Some((0.0, 200.0)));
        assert_eq!(def("notes_octave_range").range, Some((0.0, 3.0)));
        assert_eq!(def("notes_jump").range, Some((1.0, 16.0)));
        assert_eq!(def("notes_scale").default, Value::Choice("major".into()));
        assert_eq!(def("notes_root").default, Value::Choice("c".into()));
        assert_eq!(Scale::ALL.len(), 12);
        assert_eq!(Plays::ALL.len(), 15);
        assert_eq!(Root::Fs.label(), "F#");
    }

    #[test]
    fn jump_keys_are_always_hold() {
        let reg = reg();
        let jump = VoiceSettings::defs().iter().find(|d| d.key == "notes_jump").unwrap();
        assert!(jump.flags.keyframe && jump.flags.hold);
        assert!(VoiceSettings::defs().iter().filter(|d| d.key != "notes_jump").all(|d| !d.flags.hold));
        let mut scene = bpe_core::Scene::empty(&reg);
        let id = scene.add_thing(&reg, "shape", ThingId::ROOT).unwrap();
        scene.add_slot(&reg, id, SlotId::Sound, VOICE).unwrap();
        let a = bpe_core::Address { thing: id, slot: Some(SlotId::Sound), key: "notes_jump".into() };
        scene.set_key(&reg, &a, 1.0, Value::Int(3)).unwrap();
        scene.set_key(&reg, &a, 2.0, Value::Int(5)).unwrap();
        let t = &scene.tracks[&a];
        assert!(t.keys.iter().all(|k| k.ease == bpe_core::Ease::Hold));
        assert_eq!(t.value_at(1.9), Some(Value::Int(3)), "no in-between slot");
        // Other knobs keep the usual ease.
        let g = bpe_core::Address { thing: id, slot: Some(SlotId::Sound), key: "a_gain".into() };
        scene.set_key(&reg, &g, 1.0, Value::Number(-3.0)).unwrap();
        assert_ne!(scene.tracks[&g].keys[0].ease, bpe_core::Ease::Hold);
    }

    #[test]
    fn voice_card_fits_only_shapes_and_particles() {
        let reg = reg();
        let kind = reg.module(VOICE).unwrap();
        assert_eq!(kind.slot(), SlotId::Sound);
        assert!(kind.fits("shape") && kind.fits("particles"));
        for t in ["balls", "camera", "folder", "scene"] {
            assert!(!kind.fits(t), "{t}");
        }
    }

    #[test]
    fn osc_keys_are_prefixed() {
        let defs = VoiceSettings::defs();
        let has = |k: &str| defs.iter().any(|d| d.key == k);
        for k in ["a_gain", "b_gain", "a_wave", "b_sub_level", "a_on", "play_glide", "out_send3", "routes"] {
            assert!(has(k), "{k}");
        }
        assert!(!has("gain"));
        assert!(defs.iter().all(|d| !d.key.contains('/')));
        let b_on = defs.iter().find(|d| d.key == "b_on").unwrap();
        assert_eq!(b_on.default, Value::Bool(false));
        let b_wave = defs.iter().find(|d| d.key == "b_wave").unwrap();
        assert_eq!(b_wave.default, Value::Choice("saw".into()));
        let noise = defs.iter().find(|d| d.key == "a_noise").unwrap();
        assert_eq!(noise.when, "a_wave=noise");
        let sub = defs.iter().find(|d| d.key == "b_sub_level").unwrap();
        assert_eq!(sub.when, "b_wave!=noise and b_sub!=off");
        // Every number knob takes keys.
        for d in defs.iter().filter(|d| matches!(d.default, Value::Number(_))) {
            assert!(d.flags.keyframe, "{}", d.key);
        }
        for d in AdsrSettings::defs().iter().filter(|d| matches!(d.default, Value::Number(_))) {
            assert!(d.flags.keyframe, "{}", d.key);
        }
        // Saving round-trips both oscillators against their own defaults.
        let mut v = VoiceSettings::default();
        v.b.on = true;
        v.a.gain = -3.0;
        let p = v.to_params();
        assert_eq!(p.get("b_on"), Some(&Value::Bool(true)));
        assert_eq!(p.get("a_gain"), Some(&Value::Number(-3.0)));
        assert!(p.get("b_wave").is_none(), "Saw is Osc B's default");
        assert_eq!(VoiceSettings::from_params(&p), v);
    }

    #[test]
    fn shapes_allow_a_sound_card_but_do_not_start_with_one() {
        let reg = reg();
        let shape = reg.thing("shape").unwrap();
        assert!(shape.slots().contains(&SlotId::Sound));
        assert!(!shape.default_slots().contains(&SlotId::Sound));
        let mut scene = bpe_core::Scene::empty(&reg);
        let id = scene.add_thing(&reg, "shape", ThingId::ROOT).unwrap();
        assert!(scene.card(id, SlotId::Sound).is_none());
        scene.add_slot(&reg, id, SlotId::Sound, VOICE).unwrap();
        // A layered ADSR key resolves.
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.things[&id].slots.contains_key(&SlotId::Sound));
        let _ = layered_key(LayerId(0), "attack");
    }
}
