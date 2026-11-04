//! Group shader: one of the built-in effects, or the user's own effect code, spread across the
//! whole group as one picture. Each ball shows its own piece of it, by where it started in the
//! group, mixed with the ball's colours by a blend and an amount.
//!
//! Its effect settings are the Shader layer's (`effect`, `code`, `speed`, `scale` and each
//! built-in's own under `"<effect id>.<setting>"`); own code's come from its header
//! ([`super::shader::dynamic_defs`]).

use std::sync::OnceLock;

use bpe_core::{Blend, Choice, Depth, Flags, Params, SettingDef, SettingValue, Value, ValueType};

use super::group::Changes;
use super::shader::{self, ShaderLayer};

/// The help a Group shader shows.
pub const HELP: &str = "An effect spread across the whole group as one picture: each ball shows its own piece of it.";

/// How the effect mixes with each ball's colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GroupShaderMix {
    /// The effect's colours instead of the ball's.
    #[default]
    Replace,
    Add,
    Screen,
    Multiply,
    Overlay,
    SoftLight,
    /// The effect's hue and saturation, the ball's lightness.
    Tint,
    /// The effect's hue only.
    Hue,
}

impl GroupShaderMix {
    /// The blend that mixes it.
    pub fn blend(self) -> Blend {
        match self {
            Self::Replace => Blend::Normal,
            Self::Add => Blend::Add,
            Self::Screen => Blend::Screen,
            Self::Multiply => Blend::Multiply,
            Self::Overlay => Blend::Overlay,
            Self::SoftLight => Blend::SoftLight,
            Self::Tint => Blend::Colour,
            Self::Hue => Blend::Hue,
        }
    }
}

/// Where the effect shows, when it changes the whole ball.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Shows {
    /// Inside the balls only.
    #[default]
    Inside,
    /// Inside them and in a soft ring around them.
    Around,
}

fn def(key: &'static str, label: &'static str, help: &'static str, ty: ValueType, default: Value) -> SettingDef {
    SettingDef {
        key,
        label,
        help,
        ty,
        default,
        range: None,
        span: None,
        unit: "",
        group: "Mixing",
        depth: Depth::More,
        flags: Flags::default(),
        when: "",
    }
}

/// Every setting of the Group shader kind, made once: the Shader layer's effect settings (not
/// its space, opacity or blend), then how it mixes with the balls.
pub fn defs() -> &'static [SettingDef] {
    static DEFS: OnceLock<Vec<SettingDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        let mut defs: Vec<SettingDef> = shader::defs()
            .iter()
            .filter(|d| !matches!(d.key, "space" | "opacity" | "blend"))
            .map(|d| match d.key {
                "effect" => SettingDef { help: "Which effect spreads across the group.", ..d.clone() },
                "scale" => SettingDef {
                    help: "How big the effect is drawn: 1 spans the whole group, bigger spreads it wider.",
                    ..d.clone()
                },
                _ => d.clone(),
            })
            .collect();
        let keyed = Flags { keyframe: true, pulse: true, hold: false };
        defs.extend([
            SettingDef {
                range: Some((0.0, 1.0)),
                flags: keyed,
                group: "",
                ..def(
                    "covers",
                    "Covers",
                    "How much of the group it covers, from the From side: 1 is the whole group, 0.5 half of it. Animate it to wipe the effect across the group.",
                    ValueType::Number,
                    Value::Number(1.0),
                )
            },
            SettingDef {
                span: Some((-180.0, 180.0)),
                unit: "°",
                flags: Flags { keyframe: true, pulse: false, hold: false },
                group: "",
                ..def(
                    "from",
                    "From",
                    "The side it covers the group from: 0 from the left, 90 from the top.",
                    ValueType::Number,
                    Value::Number(0.0),
                )
            },
            SettingDef {
                range: Some((0.0, 1.0)),
                group: "",
                flags: Flags { keyframe: true, pulse: false, hold: false },
                ..def(
                    "edge",
                    "Edge",
                    "How soft the edge of the covered part is, as a share of the group.",
                    ValueType::Number,
                    Value::Number(0.2),
                )
            },
            SettingDef {
                depth: Depth::Simple,
                ..def(
                    "mix",
                    "With ball colours",
                    "How the effect mixes with each ball's own colours.",
                    GroupShaderMix::value_type(),
                    GroupShaderMix::Replace.to_value(),
                )
            },
            SettingDef {
                range: Some((0.0, 1.0)),
                flags: keyed,
                depth: Depth::Simple,
                ..def(
                    "amount",
                    "Amount",
                    "0 leaves the balls as they are, 1 mixes in all of it.",
                    ValueType::Number,
                    Value::Number(1.0),
                )
            },
            def(
                "changes",
                "Colours it changes",
                "The whole ball, the Body's colour only, or the layers picked from a list.",
                Changes::value_type(),
                Changes::EveryLayer.to_value(),
            ),
            SettingDef {
                when: "changes=every_layer",
                ..def(
                    "shows",
                    "Where it shows",
                    "Inside the balls only, or also in a soft ring around them.",
                    Shows::value_type(),
                    Shows::Inside.to_value(),
                )
            },
            SettingDef {
                when: "changes=every_layer and shows=around",
                range: Some((0.1, 2.0)),
                unit: "radii",
                flags: Flags { keyframe: true, pulse: false, hold: false },
                ..def(
                    "around",
                    "Ring size",
                    "How far past the ball's edge the ring reaches, in ball radii.",
                    ValueType::Number,
                    Value::Number(0.6),
                )
            },
            SettingDef {
                when: "changes=choose",
                ..def(
                    "chosen",
                    "Layers",
                    "The layers it changes, by number.",
                    ValueType::Text,
                    Value::Text(String::new()),
                )
            },
        ]);
        defs
    })
}

/// A Group shader read from its settings.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupShader {
    /// The effect, as a Shader layer would run it.
    pub shader: ShaderLayer,
    pub mix: GroupShaderMix,
    pub amount: f32,
    pub changes: Changes,
    /// How far past the edge it shows, in radii: `None` inside the balls only (and whenever it
    /// doesn't change the whole ball).
    pub around: Option<f32>,
    /// The layers it changes, by number, for Choose ("3,5").
    pub chosen: String,
    /// How much of the group it covers, 0 to 1, from the side `from` says.
    pub covers: f32,
    /// The side it covers from, in degrees: 0 from the left.
    pub from: f32,
    /// How soft the covered part's edge is, 0 to 1 of the group.
    pub edge: f32,
}

impl GroupShader {
    /// Reads a layer's settings (plain keys, as `ResolvedSlot::layer_params` gives them).
    pub fn from_params(p: &Params) -> GroupShader {
        let number = |k: &str, d: f32| match p.get(k) {
            Some(Value::Number(x)) if x.is_finite() => *x as f32,
            _ => d,
        };
        let changes = p.get("changes").and_then(Changes::from_value).unwrap_or(Changes::EveryLayer);
        let shows = p.get("shows").and_then(Shows::from_value).unwrap_or_default();
        let around =
            (changes == Changes::EveryLayer && shows == Shows::Around).then(|| number("around", 0.6).clamp(0.1, 2.0));
        GroupShader {
            shader: ShaderLayer::from_params(p),
            mix: p.get("mix").and_then(GroupShaderMix::from_value).unwrap_or_default(),
            amount: number("amount", 1.0).clamp(0.0, 1.0),
            changes,
            around,
            chosen: match p.get("chosen") {
                Some(Value::Text(t)) => t.clone(),
                _ => String::new(),
            },
            covers: number("covers", 1.0).clamp(0.0, 1.0),
            from: number("from", 0.0),
            edge: number("edge", 0.2).clamp(0.0, 1.0),
        }
    }

    /// Whether it draws: own code that doesn't read draws nothing.
    pub fn writes(&self) -> bool {
        self.shader.header.is_ok()
    }

    /// The layer ids named in `chosen`; words that aren't numbers are skipped.
    pub fn chosen_ids(&self) -> Vec<u32> {
        self.chosen.split([',', ' ']).filter_map(|w| w.trim().parse().ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_settings_are_the_shaders_effect_ones_then_the_mixing() {
        let keys: Vec<&str> = defs().iter().map(|d| d.key).collect();
        assert_eq!(&keys[..4], ["effect", "code", "speed", "scale"]);
        for gone in ["space", "opacity", "blend"] {
            assert!(!keys.contains(&gone), "{gone}");
        }
        assert!(keys.contains(&"rainbow_flow.bands"), "built-ins' own settings come along");
        assert_eq!(
            &keys[keys.len() - 9..],
            ["covers", "from", "edge", "mix", "amount", "changes", "shows", "around", "chosen"]
        );
    }

    #[test]
    fn around_counts_only_over_the_whole_ball() {
        let mut p = Params::default();
        let read = |p: &Params| GroupShader::from_params(p);
        assert_eq!(read(&p).changes, Changes::EveryLayer, "the whole ball by default");
        assert_eq!(read(&p).around, None);
        p.set("shows", Value::Choice("around".into()));
        assert_eq!(read(&p).around, Some(0.6));
        p.set("around", Value::Number(9.0));
        assert_eq!(read(&p).around, Some(2.0));
        p.set("changes", Value::Choice("body".into()));
        assert_eq!(read(&p).around, None);
        p.set("chosen", Value::Text("3, 5,x".into()));
        assert_eq!(read(&p).chosen_ids(), [3, 5]);
        assert_eq!(GroupShaderMix::Tint.blend(), Blend::Colour);
    }
}

#[cfg(test)]
mod covers_tests {
    use super::*;

    #[test]
    fn it_covers_the_whole_group_unless_told() {
        let mut p = Params::default();
        let g = GroupShader::from_params(&p);
        assert_eq!((g.covers, g.from, g.edge), (1.0, 0.0, 0.2));
        p.set("covers", Value::Number(3.0));
        p.set("from", Value::Number(90.0));
        assert_eq!(GroupShader::from_params(&p).covers, 1.0);
        assert_eq!(GroupShader::from_params(&p).from, 90.0);
    }
}
