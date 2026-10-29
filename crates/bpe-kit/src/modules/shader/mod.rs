//! Shader modules: one module kind per built-in effect file, generated from its header. On
//! Scene defaults a shader is a picture effect; on a thing it is a material painted over that
//! thing alone.

pub mod draw;

use std::sync::OnceLock;

use bpe_core::{Color, Depth, Flags, ModuleKind, SettingDef, SlotId, Value, ValueType};
use bpe_gfx::effects::{BUILTINS, Builtin, Header, ParamKind};

/// The key of the setting every shader module has first: how much of the effect shows.
pub const BLEND: &str = "blend";

/// The module kind ids, in [`BUILTINS`] order: `shader.` and the file name.
const IDS: [&str; 13] = [
    "shader.color_cycle",
    "shader.color_split",
    "shader.echo",
    "shader.film",
    "shader.glitch",
    "shader.kaleidoscope",
    "shader.liquid",
    "shader.mirror",
    "shader.pixels",
    "shader.shockwave",
    "shader.lava",
    "shader.rainbow_flow",
    "shader.hit_glow",
];

static SETTINGS: [OnceLock<Vec<SettingDef>>; 13] = [const { OnceLock::new() }; 13];

/// A built-in effect as a module kind.
#[derive(Clone, Copy)]
pub struct EffectKind(pub &'static Builtin);

/// Every built-in effect's module kind.
pub fn kinds() -> impl Iterator<Item = EffectKind> {
    BUILTINS.iter().map(EffectKind)
}

/// Where a built-in sits in [`BUILTINS`].
fn index_of(builtin: &'static Builtin) -> usize {
    BUILTINS.iter().position(|b| std::ptr::eq(b, builtin)).expect("a built-in effect")
}

/// The module kind id of a built-in effect.
pub fn id_of(builtin: &'static Builtin) -> &'static str {
    IDS[index_of(builtin)]
}

impl EffectKind {
    pub fn header(&self) -> &'static Header {
        self.0.header()
    }
}

impl ModuleKind for EffectKind {
    fn id(&self) -> &'static str {
        id_of(self.0)
    }
    fn name(&self) -> &'static str {
        &self.header().name
    }
    fn help(&self) -> &'static str {
        &self.header().about
    }
    fn slot(&self) -> SlotId {
        SlotId::Shader
    }
    fn settings(&self) -> &'static [SettingDef] {
        SETTINGS[index_of(self.0)].get_or_init(|| settings_of(self.header()))
    }
}

/// `blend`, then one setting per header line. The header's strings are kept for the program's
/// life, as every setting's words are.
fn settings_of(header: &Header) -> Vec<SettingDef> {
    let keep = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    let mut defs = vec![SettingDef {
        key: BLEND,
        label: "Blend",
        help: "How much of the effect shows.",
        ty: ValueType::Number,
        default: Value::Number(1.0),
        range: Some((0.0, 1.0)),
        dice: None,
        unit: "",
        group: "",
        depth: Depth::Simple,
        flags: Flags { keyframe: true, random: true, pulse: true, structural: false },
    }];
    for p in &header.params {
        let mut label = p.key.replace('_', " ");
        if let Some(first) = label.get(..1) {
            label = first.to_uppercase() + &label[1..];
        }
        let (ty, default, range, flags) = match p.kind {
            ParamKind::Number { default, min, max, int: false } => (
                ValueType::Number,
                Value::Number(default as f64),
                Some((min as f64, max as f64)),
                Flags { keyframe: true, random: true, pulse: true, structural: false },
            ),
            ParamKind::Number { default, min, max, int: true } => (
                ValueType::Int,
                Value::Int(default.round() as i64),
                Some((min as f64, max as f64)),
                Flags { keyframe: true, random: true, pulse: true, structural: false },
            ),
            ParamKind::Toggle { default } => (
                ValueType::Bool,
                Value::Bool(default),
                None,
                Flags { keyframe: false, random: true, pulse: false, structural: false },
            ),
            ParamKind::Color { default } => (
                ValueType::Color,
                Value::Color(Color::from_linear([default[0], default[1], default[2], 1.0])),
                None,
                Flags { keyframe: true, random: true, pulse: false, structural: false },
            ),
        };
        defs.push(SettingDef {
            key: keep(&p.key),
            label: keep(&label),
            help: keep(&p.help),
            ty,
            default,
            range,
            dice: None,
            unit: "",
            group: "",
            depth: if p.simple { Depth::Simple } else { Depth::More },
            flags,
        });
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_is_a_shader_kind_with_blend_first() {
        let kinds: Vec<EffectKind> = kinds().collect();
        assert_eq!(kinds.len(), 13);
        for (i, k) in kinds.iter().enumerate() {
            assert_eq!(k.id(), format!("shader.{}", BUILTINS[i].id));
        }
        let k = kinds.iter().find(|k| k.id() == "shader.kaleidoscope").unwrap();
        assert_eq!((k.name(), k.slot()), ("Kaleidoscope", SlotId::Shader));
        assert!(k.help().starts_with("Mirrors the picture"));
        let keys: Vec<&str> = k.settings().iter().map(|d| d.key).collect();
        assert_eq!(keys, ["blend", "slices", "spin", "zoom", "angle", "pulse"]);
        assert_eq!((k.settings()[1].ty, k.settings()[1].label), (ValueType::Int, "Slices"));
        assert_eq!(k.settings()[1].default, Value::Int(6));
        assert_eq!(k.settings()[1].range, Some((2.0, 24.0)));
        assert_eq!(k.settings()[4].depth, Depth::More);
        let mirror = kinds.iter().find(|k| k.id() == "shader.mirror").unwrap();
        assert_eq!((mirror.settings()[1].ty, mirror.settings()[1].label), (ValueType::Bool, "Left right"));
        let lava = kinds.iter().find(|k| k.id() == "shader.lava").unwrap();
        let dark = lava.settings().iter().find(|d| d.key == "dark").unwrap();
        assert_eq!((dark.ty, &dark.default), (ValueType::Color, &Value::Color(Color::rgb(0x14, 0, 0))));
        assert_eq!(bpe_core::check_registry(&crate::registry()), Vec::<String>::new());
    }
}
