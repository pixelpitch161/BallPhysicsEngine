//! All tempo maths in one place, so tempo changes can come later without touching its users.

use bpe_kit::SceneSettings;

/// One fixed tempo and time signature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoMap {
    bpm: f64,
    top: u32,
    bottom: u32,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self { bpm: 120.0, top: 4, bottom: 4 }
    }
}

impl TempoMap {
    pub fn new(bpm: f64, top: u32, bottom: u32) -> Self {
        Self { bpm: bpm.clamp(20.0, 300.0), top: top.clamp(1, 16), bottom: bottom.clamp(1, 64) }
    }

    pub fn from_settings(s: &SceneSettings) -> Self {
        Self::new(s.tempo as f64, s.time_top, s.time_bottom.beats())
    }

    pub fn bpm(&self) -> f64 {
        self.bpm
    }

    /// Beats in a bar (the top number).
    pub fn top(&self) -> u32 {
        self.top
    }

    /// Which note is a beat (the bottom number).
    pub fn bottom(&self) -> u32 {
        self.bottom
    }

    /// Seconds in one beat: the tempo counts quarter notes, scaled by the bottom number.
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm * 4.0 / self.bottom as f64
    }

    /// Seconds in one bar.
    pub fn bar_seconds(&self) -> f64 {
        self.beat_seconds() * self.top as f64
    }

    /// Beats since the start at `time` seconds (0 at the start, fractional between beats).
    pub fn beat_at(&self, time: f64) -> f64 {
        time / self.beat_seconds()
    }

    /// Bars since the start at `time` seconds.
    pub fn bar_at(&self, time: f64) -> f64 {
        time / self.bar_seconds()
    }

    /// How long `beats` beats last, in seconds.
    pub fn seconds_of_beats(&self, beats: f64) -> f64 {
        beats * self.beat_seconds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_map_maths() {
        let t = TempoMap::new(120.0, 4, 4);
        assert_eq!((t.beat_seconds(), t.bar_seconds()), (0.5, 2.0));
        assert_eq!(t.beat_at(1.25), 2.5);
        assert_eq!(t.bar_at(3.0), 1.5);
        assert_eq!(t.seconds_of_beats(3.0), 1.5);
        let t = TempoMap::new(120.0, 6, 8);
        assert_eq!((t.beat_seconds(), t.bar_seconds()), (0.25, 1.5));
        assert_eq!(TempoMap::from_settings(&SceneSettings::default()), TempoMap::default());
    }

    #[test]
    fn light_shows_keep_the_same_beat_as_the_sound() {
        use bpe_kit::things::scene::TimeBottom;
        for (tempo, top, bottom) in
            [(120.0, 4, TimeBottom::Four), (87.5, 7, TimeBottom::Eight), (500.0, 3, TimeBottom::Two)]
        {
            let s = SceneSettings { tempo, time_top: top, time_bottom: bottom, ..SceneSettings::default() };
            let t = TempoMap::from_settings(&s);
            let (beat, in_bar) = bpe_kit::things::rig::pattern::beat_seconds(&s);
            assert_eq!((beat, in_bar), (t.beat_seconds(), t.top()), "{tempo} {top}");
        }
    }
}
