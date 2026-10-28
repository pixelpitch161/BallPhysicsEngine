//! Music settings: the song a video plays over, a MIDI melody for the bounces, beat sync, and
//! which instrument plays for which kind of event.

use serde::{Deserialize, Serialize};

/// What makes the sound of a note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Instrument {
    /// A soft synthesised pluck. `decay` is seconds to fade by 60 dB, `brightness` 0 to 1.
    Pluck { decay: f32, brightness: f32 },
    /// A WAV file played at the pitch of `root_note` and pitched up or down from there.
    Sample { path: String, root_note: u8 },
    /// One of the built-in synthesised instruments, by name, such as "Marimba" or "Drum kit".
    Library { name: String },
    /// A folder of WAV files named by the note they play, such as `C4.wav` or `F#3.wav`. Each
    /// note plays the nearest file, pitched up or down.
    SampleFolder { path: String },
    /// A SoundFont (`.sf2`) file and which of its instruments to play. Drum kits are usually
    /// bank 128.
    SoundFont { path: String, bank: u32, preset: u32 },
}

impl Instrument {
    pub fn library(name: &str) -> Self {
        Instrument::Library { name: name.into() }
    }
}

/// The song a video plays over, a melody for the bounces, and how the bounces follow the beat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Music {
    /// An mp3, wav, ogg or flac file mixed under the bounce sounds. Relative paths start from
    /// the scene file's folder.
    pub song: Option<String>,
    /// Seconds into the song where the video starts.
    pub offset: f32,
    /// Song volume in dB, compared with the bounce sounds. 0 is about as loud.
    pub gain: f32,
    /// How far the song dips while bounces play, from 0 (not at all) to 1 (a lot).
    pub duck_amount: f32,
    /// Beats per minute. With no song this lays out the beat; with a song, 0 uses the tempo
    /// found in it and anything else overrides it.
    pub bpm: f32,
    pub beat_sync: BeatSync,
    /// Each bounce plays the next note of this melody instead of a note from the chords.
    pub melody: Option<Melody>,
}

impl Default for Music {
    fn default() -> Self {
        Self {
            song: None,
            offset: 0.0,
            gain: -3.0,
            duck_amount: 0.3,
            bpm: 0.0,
            beat_sync: BeatSync::default(),
            melody: None,
        }
    }
}

/// How the bounces are made to land on the beat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BeatSync {
    pub mode: SyncMode,
    /// Lock steers this many balls, the first ones the scene makes.
    pub lead_balls: u32,
    /// The most Lock may change a ball's speed at one bounce, as a fraction (0.15 is 15%).
    pub max_change: f32,
    /// Lock may aim for half or quarter beats when the next whole beat is out of reach: 1, 2 or 4.
    pub subdivisions: u32,
    /// Seconds a hit may miss a beat by and still count as on the beat.
    pub tolerance: f32,
    /// How much Warp may speed up or slow down the video between beats, as a fraction.
    pub warp_strength: f32,
}

impl Default for BeatSync {
    fn default() -> Self {
        Self {
            mode: SyncMode::Off,
            lead_balls: 1,
            max_change: 0.15,
            subdivisions: 2,
            tolerance: 0.04,
            warp_strength: 0.35,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Balls move freely.
    #[default]
    Off,
    /// After each bounce, the lead balls' speed is adjusted a little so the next bounce lands on a beat.
    Lock,
    /// Gravity, speed and arena size are tuned once so as many bounces as possible land on beats.
    Nudge,
    /// The video's clock is gently stretched between bounces so they land on beats.
    Warp,
}

/// A melody from a MIDI file, one note (or chord) per bounce.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Melody {
    /// A `.mid` file, or `builtin:` and the name of a melody that comes with the program.
    pub path: String,
    /// Which track of the file to play. None picks the busiest track that isn't drums.
    pub track: Option<u32>,
    /// Semitones to shift every note up (or down, if negative).
    pub transpose: i32,
    pub advance_on: Advance,
    /// Harder hits play louder notes. Off plays each note as loud as the file says.
    pub velocity_from_impact: bool,
}

impl Default for Melody {
    fn default() -> Self {
        Self {
            path: "builtin:ode-to-joy".into(),
            track: None,
            transpose: 0,
            advance_on: Advance::AllHits,
            velocity_from_impact: true,
        }
    }
}

/// Which hits move the melody on to its next note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Advance {
    /// Every hit that makes a sound plays the next note.
    #[default]
    AllHits,
    /// Only hits on the arena's wall play the next note; other hits replay the current one.
    WallHits,
    /// Every ball plays the melody from the start on its own.
    PerBall,
}

/// One row of the voices table: which events play on which instrument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceMap {
    pub on: EventFilter,
    pub instrument: Instrument,
    /// Volume of this voice in dB.
    pub gain: f32,
    pub pitch: PitchMode,
    pub enabled: bool,
}

impl Default for VoiceMap {
    fn default() -> Self {
        Self {
            on: EventFilter::AllHits,
            instrument: Instrument::library("Marimba"),
            gain: 0.0,
            pitch: PitchMode::Tune,
            enabled: true,
        }
    }
}

/// Which events a voice plays for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventFilter {
    /// Every hit of a ball on a wall, peg, other ball or soft body, and escapes.
    #[default]
    AllHits,
    /// The arena's wall or rings.
    Walls,
    /// Pegs, lines and shapes inside the arena.
    Obstacles,
    /// Balls bumping into each other.
    BallHits,
    /// Jelly balls, ropes and cloth touching something.
    SoftHits,
    /// A ball escaping through a ring's gap.
    Escapes,
    /// A new ball appearing.
    Spawns,
    /// Every beat of the song or tempo.
    Beats,
    /// The first beat of every bar.
    Downbeats,
    /// A rule's "play a note" action.
    RuleNotes,
}

impl EventFilter {
    pub const ALL: [EventFilter; 10] = [
        EventFilter::AllHits,
        EventFilter::Walls,
        EventFilter::Obstacles,
        EventFilter::BallHits,
        EventFilter::SoftHits,
        EventFilter::Escapes,
        EventFilter::Spawns,
        EventFilter::Beats,
        EventFilter::Downbeats,
        EventFilter::RuleNotes,
    ];

    pub fn name(self) -> &'static str {
        match self {
            EventFilter::AllHits => "Every hit",
            EventFilter::Walls => "Wall hits",
            EventFilter::Obstacles => "Peg hits",
            EventFilter::BallHits => "Ball meets ball",
            EventFilter::SoftHits => "Jelly hits",
            EventFilter::Escapes => "Escapes",
            EventFilter::Spawns => "New balls",
            EventFilter::Beats => "Every beat",
            EventFilter::Downbeats => "Every bar",
            EventFilter::RuleNotes => "Rule notes",
        }
    }
}

/// Which note a voice plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PitchMode {
    /// The next note of the melody, or a note of the current chord when there is no melody.
    #[default]
    Tune,
    /// Always this MIDI note. For the drum kit, the note picks the drum (36 kick, 38 snare, 49 crash).
    Fixed(u8),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_settings_round_trip_and_default_when_missing() {
        let music = Music {
            song: Some("song.mp3".into()),
            melody: Some(Melody { track: Some(2), transpose: -5, ..Melody::default() }),
            beat_sync: BeatSync { mode: SyncMode::Lock, ..BeatSync::default() },
            ..Music::default()
        };
        let text = ron::to_string(&music).unwrap();
        assert_eq!(ron::from_str::<Music>(&text).unwrap(), music);
        assert_eq!(ron::from_str::<Music>("()").unwrap(), Music::default());
        let voice: VoiceMap =
            ron::from_str("(on: Escapes, instrument: Library(name: \"Drum kit\"), pitch: Fixed(49))").unwrap();
        assert_eq!(voice.on, EventFilter::Escapes);
        assert_eq!(voice.pitch, PitchMode::Fixed(49));
        assert!(voice.enabled);
    }
}
