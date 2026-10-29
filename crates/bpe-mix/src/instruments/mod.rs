//! The instruments a sound module can pick from, all synthesised so the program stays small.

use bpe_core::Choice;

/// A built-in instrument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Choice)]
pub enum Instrument {
    #[default]
    Pluck,
    ElectricPiano,
    GrandPiano,
    Marimba,
    Vibraphone,
    Glass,
    Harp,
    Bells,
    Kalimba,
    Supersaw,
    Pad,
    SynthBass,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Choice;

    #[test]
    fn instrument_names_are_snake_case_and_round_trip() {
        assert_eq!(Instrument::NAMES.len(), 12);
        assert_eq!(Instrument::Pluck.name(), "pluck");
        assert_eq!(Instrument::from_name("electric_piano"), Some(Instrument::ElectricPiano));
        assert_eq!(Instrument::from_name("drum_kit"), None);
    }
}
