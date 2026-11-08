//! The scene's mixer card: the master and the three buses. Each bus is one [`BusSettings`]
//! expanded under `bus1_`, `bus2_` and `bus3_`; its effects are the card's layers (section
//! Effect, `lane` = the bus's index 0..2, see [`super::fx`]).

use super::voice::{SILENT_DB, full_params, prefixed_defs};
use bpe_core::{GroupOpts, ModuleKind, Params, SettingDef, SettingGroup, Settings, SlotId};

/// The mixer card's kind, on the scene settings only.
pub struct MixerKind;

/// The mixer card's id.
pub const MIXER: &str = "sound.mixer";

/// How many buses there are.
pub const BUSES: usize = 3;

/// One bus: its name, whether it plays, its fader, pan, mute and solo, and how much it sends to
/// the other buses (after its fader).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BusSettings {
    /// The bus's name.
    #[setting(label = "Name")]
    pub name: String,
    /// Whether the bus plays. A shape sent to a bus that is off is silent there.
    #[setting(label = "On")]
    pub on: bool,
    /// The bus's level, after its effects.
    #[setting(label = "Fader", range(-60, 6), unit = "dB", keyframe)]
    pub fader: f32,
    /// Left or right.
    #[setting(label = "Pan", range(-100, 100), keyframe)]
    pub pan: f32,
    /// Silences the bus (and its sends).
    #[setting(label = "Mute")]
    pub mute: bool,
    /// While any bus is soloed, only soloed buses play.
    #[setting(label = "Solo")]
    pub solo: bool,
    /// How much goes on to Bus 1, after the fader (all the way down is off).
    #[setting(label = "Send to Bus 1", range(-60, 6), unit = "dB", keyframe)]
    pub send1: f32,
    /// How much goes on to Bus 2.
    #[setting(label = "Send to Bus 2", range(-60, 6), unit = "dB", keyframe)]
    pub send2: f32,
    /// How much goes on to Bus 3.
    #[setting(label = "Send to Bus 3", range(-60, 6), unit = "dB", keyframe)]
    pub send3: f32,
}

impl Default for BusSettings {
    fn default() -> Self {
        BusSettings::numbered(1)
    }
}

impl BusSettings {
    /// Bus `n` (1..=3) as a new scene has it: Bus 1 on, the others off.
    pub fn numbered(n: u8) -> Self {
        Self {
            name: format!("Bus {n}"),
            on: n == 1,
            fader: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            send1: SILENT_DB,
            send2: SILENT_DB,
            send3: SILENT_DB,
        }
    }

    /// The send to bus `to` (1..=3) in dB; [`SILENT_DB`] or below is off.
    pub fn send(&self, to: u8) -> f32 {
        match to {
            1 => self.send1,
            2 => self.send2,
            3 => self.send3,
            _ => SILENT_DB,
        }
    }
}

impl SettingGroup for BusSettings {
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

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MixerSettings {
    /// Turns the whole mix up or down before the limiter.
    #[setting(label = "Input", range(-24, 24), unit = "dB", keyframe, group = "Master")]
    pub master_input: f32,
    /// Turns the finished sound up or down. Turned up, it pushes into the limiter, so the
    /// sound never goes past the Ceiling.
    #[setting(label = "Output", range(-24, 24), unit = "dB", keyframe, group = "Master")]
    pub master_output: f32,
    /// The loudest the sound may get, peaks between samples included.
    #[setting(label = "Ceiling", range(-12, 0), unit = "dBTP", group = "Master")]
    pub master_ceiling: f32,
    /// How quickly the limiter lets go after a peak.
    #[setting(label = "Release", range(1, 1000), unit = "ms", group = "Master")]
    pub master_release: f32,
    /// Fades the sound out over the last this many seconds of the video (0 is off).
    #[setting(label = "End fade", range(0, 30), span(0, 10), unit = "s", group = "Master")]
    pub master_end_fade: f32,
    #[setting(paint, prefix = "bus1_", label = "Bus 1", group = "Bus 1")]
    pub bus1: BusSettings,
    #[setting(paint, prefix = "bus2_", label = "Bus 2", group = "Bus 2")]
    pub bus2: BusSettings,
    #[setting(paint, prefix = "bus3_", label = "Bus 3", group = "Bus 3")]
    pub bus3: BusSettings,
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self {
            master_input: 0.0,
            master_output: 0.0,
            master_ceiling: -1.0,
            master_release: 60.0,
            master_end_fade: 0.0,
            bus1: BusSettings::numbered(1),
            bus2: BusSettings::numbered(2),
            bus3: BusSettings::numbered(3),
        }
    }
}

impl MixerSettings {
    /// Bus `n` (1..=3).
    pub fn bus(&self, n: u8) -> &BusSettings {
        match n {
            2 => &self.bus2,
            3 => &self.bus3,
            _ => &self.bus1,
        }
    }

    /// The card key of bus `n`'s setting `key` (`bus2_fader`).
    pub fn bus_key(n: u8, key: &str) -> String {
        format!("bus{n}_{key}")
    }
}

impl ModuleKind for MixerKind {
    fn id(&self) -> &'static str {
        MIXER
    }
    fn name(&self) -> &'static str {
        "Mixer"
    }
    fn help(&self) -> &'static str {
        "The scene's sound: three buses with their effects, and the master's limiter and levels."
    }
    fn slot(&self) -> SlotId {
        SlotId::Mixer
    }
    fn settings(&self) -> &'static [SettingDef] {
        MixerSettings::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "scene"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Value;

    #[test]
    fn three_buses_bus1_on() {
        let m = MixerSettings::from_params(&Params::default());
        assert!(m.bus1.on && !m.bus2.on && !m.bus3.on);
        assert_eq!((m.bus1.name.as_str(), m.bus2.name.as_str(), m.bus3.name.as_str()), ("Bus 1", "Bus 2", "Bus 3"));
        for n in 1..=3 {
            let b = m.bus(n);
            assert_eq!((b.fader, b.pan, b.mute, b.solo), (0.0, 0.0, false, false));
            assert!((1..=3).all(|to| b.send(to) == SILENT_DB), "sends start off");
        }
        let defs = MixerSettings::defs();
        let def = |k: &str| defs.iter().find(|d| d.key == k).unwrap_or_else(|| panic!("{k}"));
        assert_eq!(def("bus2_on").default, Value::Bool(false));
        assert_eq!(def("bus1_on").default, Value::Bool(true));
        assert_eq!(def("bus3_name").default, Value::Text("Bus 3".into()));
        for k in ["bus1_fader", "bus2_pan", "bus3_send1", "bus1_send2"] {
            assert!(def(k).flags.keyframe, "{k} takes keys");
        }
        assert!(defs.iter().all(|d| !d.key.contains('/')));
        // Saving is sparse and round-trips.
        let mut m = MixerSettings::default();
        assert!(m.to_params().iter().next().is_none());
        m.bus2.on = true;
        m.bus3.fader = -6.0;
        let p = m.to_params();
        assert_eq!(p.get("bus2_on"), Some(&Value::Bool(true)));
        assert_eq!(p.get("bus3_fader"), Some(&Value::Number(-6.0)));
        assert_eq!(MixerSettings::from_params(&p), m);
        assert_eq!(MixerSettings::bus_key(2, "send3"), "bus2_send3");
    }
}
