//! The built-in scenes (spec `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md`): from simple
//! starters to full spectacles, opened from the editor's Scenes window as a new project the person owns.
//!
//! Each scene is code: its [`SceneInfo`] and a `build` function that makes it from a blank project with
//! today's starting cards, so a change to the file format can never break one. Nothing marks an opened
//! scene as built-in; it is an ordinary scene.

pub(crate) mod build;
mod middle;
mod spectacles;
mod starters;

use bpe_core::{Registry, Scene, Settings, ThingId};

/// How far along a scene is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    /// A clean template with one simple sound.
    Starter,
    /// Two or three systems together, fuller sound.
    InBetween,
    /// A showpiece ready to post, with full sound design.
    Spectacle,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Starter, Level::InBetween, Level::Spectacle];

    pub fn label(self) -> &'static str {
        match self {
            Level::Starter => "Starter",
            Level::InBetween => "In-between",
            Level::Spectacle => "Spectacle",
        }
    }

    /// The name of the filter that shows every scene of this level.
    pub fn plural(self) -> &'static str {
        match self {
            Level::Starter => "Starters",
            Level::InBetween => "In-between",
            Level::Spectacle => "Spectacles",
        }
    }
}

/// What kind of video a scene makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    BounceAndDrop,
    Escape,
    Music,
    BattlesAndRaces,
    Hypnotic,
    LightAndGlass,
    LiquidSmokeSparks,
    Words,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::BounceAndDrop,
        Kind::Escape,
        Kind::Music,
        Kind::BattlesAndRaces,
        Kind::Hypnotic,
        Kind::LightAndGlass,
        Kind::LiquidSmokeSparks,
        Kind::Words,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::BounceAndDrop => "Bounce and drop",
            Kind::Escape => "Escape",
            Kind::Music => "Music",
            Kind::BattlesAndRaces => "Battles and races",
            Kind::Hypnotic => "Hypnotic",
            Kind::LightAndGlass => "Light and glass",
            Kind::LiquidSmokeSparks => "Liquid, smoke and sparks",
            Kind::Words => "Words",
        }
    }
}

/// How much sound a scene has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SoundLevel {
    None,
    /// One clean sound that is easy to swap (the starters).
    Simple,
    /// Full sound design.
    Designed,
}

impl SoundLevel {
    pub fn label(self) -> &'static str {
        match self {
            SoundLevel::None => "no sound",
            SoundLevel::Simple => "one simple sound",
            SoundLevel::Designed => "sound designed",
        }
    }
}

/// A moment a scene builds to that has to land on time (an escape on the drop), checked by the tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Payoff {
    pub what: PayoffEvent,
    pub at_seconds: f64,
    pub within_seconds: f64,
}

/// What a [`Payoff`] waits for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PayoffEvent {
    /// The first firing of the rule with this name.
    RuleFires(&'static str),
    /// The thing with this name breaking.
    Breaks(&'static str),
    /// The rules' counter with this name reaching an amount.
    Counter(&'static str, f64),
}

/// What the Scenes window says about a scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneInfo {
    /// Kebab case, never changed once shipped.
    pub id: &'static str,
    pub name: &'static str,
    pub level: Level,
    /// One or two.
    pub kinds: &'static [Kind],
    /// One or two sentences on what happens.
    pub about: &'static str,
    /// The systems it uses, in the person's words.
    pub uses: &'static [&'static str],
    /// One sentence: the good things to change first.
    pub change_first: &'static str,
    pub sound: SoundLevel,
    /// The video's length.
    pub seconds: f64,
    /// The moment its still picture is taken.
    pub poster_seconds: f64,
    /// How much of it the preview plays, looping.
    pub preview_seconds: f64,
    /// Two colours (0xRRGGBB) a tile shows before its picture is ready.
    pub swatch: [u32; 2],
    pub payoff: Option<Payoff>,
}

impl SceneInfo {
    /// Whether every word in `words` (ignoring case) is in the name, a kind or the description.
    pub fn matches(&self, words: &str) -> bool {
        let hay = format!(
            "{} {} {}",
            self.name,
            self.kinds.iter().map(|k| k.label()).collect::<Vec<_>>().join(" "),
            self.about
        )
        .to_lowercase();
        words.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
    }

    /// Its kinds as the window shows them: "Escape · Music".
    pub fn kinds_label(&self) -> String {
        self.kinds.iter().map(|k| k.label()).collect::<Vec<_>>().join(" · ")
    }
}

/// One built-in scene.
pub struct Entry {
    pub info: SceneInfo,
    pub build: fn(&Registry) -> Scene,
}

impl Entry {
    /// The scene, made fresh: an ordinary scene that owns every setting.
    pub fn scene(&self, reg: &Registry) -> Scene {
        let mut scene = (self.build)(reg);
        scene.name = self.info.name.into();
        debug_assert!(
            scene.things.get(&ThingId::ROOT).is_some_and(|root| {
                let s = crate::SceneSettings::from_params(&root.params);
                (s.duration as f64 - self.info.seconds).abs() < 1e-3
            }),
            "{}: the scene's length is its info's",
            self.info.id
        );
        scene
    }
}

/// Every built-in scene, in the order the Scenes window lists them.
static ENTRIES: &[Entry] = &[
    starters::first_bounce::ENTRY,
    starters::ball_in_a_circle::ENTRY,
    starters::grow_every_bounce::ENTRY,
    starters::shrinking_box::ENTRY,
    starters::every_bounce_makes_two::ENTRY,
    starters::rainbow_drop::ENTRY,
    starters::fountain::ENTRY,
    starters::string_art::ENTRY,
    starters::spinning_triangle::ENTRY,
    starters::neon_tube::ENTRY,
    starters::lit_room::ENTRY,
    starters::breakable_bricks::ENTRY,
    starters::bounce_off_words::ENTRY,
    starters::sparks_on_hit::ENTRY,
    middle::will_it_escape::ENTRY,
    middle::escape_layers::ENTRY,
    middle::melody_per_bounce::ENTRY,
    middle::polyrhythm_rings::ENTRY,
    middle::all_meet_at_once::ENTRY,
    middle::hypnotic_petals::ENTRY,
    middle::red_vs_blue::ENTRY,
    middle::colour_wars::ENTRY,
    middle::marble_race::ENTRY,
    middle::merge_and_grow::ENTRY,
    middle::stained_glass::ENTRY,
    middle::prism_rainbow::ENTRY,
    middle::club_night::ENTRY,
    middle::splash_glass::ENTRY,
    spectacles::the_simulation_is_the_song::ENTRY,
    spectacles::escape_on_the_drop::ENTRY,
    spectacles::one_to_a_million::ENTRY,
    spectacles::diamond_fire::ENTRY,
    spectacles::mirror_maze::ENTRY,
    spectacles::fire_tornado::ENTRY,
    spectacles::word_from_chaos::ENTRY,
    spectacles::liquid_escape::ENTRY,
    spectacles::letter_smash::ENTRY,
];

/// Every built-in scene, in the order the Scenes window lists them.
pub fn all() -> &'static [Entry] {
    ENTRIES
}

/// The built-in scene with this id.
pub fn find(id: &str) -> Option<&'static Entry> {
    all().iter().find(|e| e.info.id == id)
}
