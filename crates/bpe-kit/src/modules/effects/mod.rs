//! The scene's Effects card: a stack of picture effects, bottom first, each a layer. Every
//! built-in effect file is a layer kind (`picture.<file>`), and own code is one more
//! (`picture.own`). Each layer has a Mix (how much shows), a Blend (how its result goes onto what
//! it took) and, over the picture, Takes (the whole picture, only the light, or only the glow),
//! then the effect's own header settings under plain keys. Materials (effects of stage Things)
//! paint the things instead of the picture and have no Takes.

pub mod draw;
pub mod upgrade;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use bpe_core::{
    Blend, Choice, Color, Depth, Flags, LayerKind, LayerSection, ModuleKind, Params, SettingDef, SettingValue, SlotId,
    Value, ValueType,
};
use bpe_gfx::effects::{BUILTINS, Builtin, Header, ParamKind};
use bpe_gfx::{Stage, Takes};

/// The Effects card's kind id.
pub const EFFECTS: &str = "effects";
/// The layer kind of own effect code.
pub const OWN: &str = "picture.own";
/// The setting every effect layer has first: how much of the effect shows.
pub const MIX: &str = "mix";

/// What an effect layer works on (the renderer's [`Takes`], as a setting).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum TakesChoice {
    #[default]
    WholePicture,
    LightOnly,
    GlowOnly,
}

impl TakesChoice {
    pub fn takes(self) -> Takes {
        match self {
            Self::WholePicture => Takes::Whole,
            Self::LightOnly => Takes::Light,
            Self::GlowOnly => Takes::Glow,
        }
    }
}

/// The scene's Effects card.
pub struct EffectsCard;

impl ModuleKind for EffectsCard {
    fn id(&self) -> &'static str {
        EFFECTS
    }
    fn name(&self) -> &'static str {
        "Effects"
    }
    fn help(&self) -> &'static str {
        "Effects over the whole picture, applied bottom first: each takes the picture so far (or only its light \
         or its glow) and changes it."
    }
    fn slot(&self) -> SlotId {
        SlotId::Shader
    }
    fn settings(&self) -> &'static [SettingDef] {
        &[]
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "scene"
    }
}

/// A built-in effect as a layer kind of the Effects card.
#[derive(Clone, Copy)]
pub struct PictureEffect(pub &'static Builtin);

/// Own effect code as a layer kind of the Effects card.
#[derive(Clone, Copy)]
pub struct OwnEffect;

/// Every built-in's layer kind, in [`BUILTINS`] order.
pub fn kinds() -> impl Iterator<Item = PictureEffect> {
    BUILTINS.iter().map(PictureEffect)
}

/// Where a built-in sits in [`BUILTINS`].
fn index_of(builtin: &'static Builtin) -> usize {
    BUILTINS.iter().position(|b| std::ptr::eq(b, builtin)).expect("a built-in effect")
}

/// The layer kind id of a built-in effect: `picture.` and its file name.
pub fn id_of(builtin: &'static Builtin) -> &'static str {
    static IDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    IDS.get_or_init(|| BUILTINS.iter().map(|b| &*Box::leak(format!("picture.{}", b.id).into_boxed_str())).collect())
        [index_of(builtin)]
}

/// The built-in a layer kind id names, if it names one.
pub fn builtin_of(kind: &str) -> Option<&'static Builtin> {
    Builtin::find(kind.strip_prefix("picture.")?)
}

/// Whether a layer kind is one of the Effects card's.
pub fn is_effect(kind: &str) -> bool {
    kind.starts_with("picture.")
}

impl PictureEffect {
    pub fn header(&self) -> &'static Header {
        self.0.header()
    }
}

fn layer_kind_basics() -> (LayerSection, bool, &'static str) {
    (LayerSection::Effect, true, EFFECTS)
}

impl LayerKind for PictureEffect {
    fn id(&self) -> &'static str {
        id_of(self.0)
    }
    fn name(&self) -> &'static str {
        &self.header().name
    }
    fn help(&self) -> &'static str {
        &self.header().about
    }
    fn settings(&self) -> &'static [SettingDef] {
        static SETTINGS: OnceLock<Vec<OnceLock<Vec<SettingDef>>>> = OnceLock::new();
        let all = SETTINGS.get_or_init(|| BUILTINS.iter().map(|_| OnceLock::new()).collect());
        all[index_of(self.0)].get_or_init(|| {
            let header = self.header();
            let mut defs = common_defs(header.stage == Stage::Picture).to_vec();
            defs.extend(param_defs(header, "", "", "Effect", true));
            with_simple_room(defs)
        })
    }
    fn section(&self) -> LayerSection {
        layer_kind_basics().0
    }
    fn repeats(&self) -> bool {
        layer_kind_basics().1
    }
    fn card(&self) -> &'static str {
        layer_kind_basics().2
    }
    fn takes_subs(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
}

impl LayerKind for OwnEffect {
    fn id(&self) -> &'static str {
        OWN
    }
    fn name(&self) -> &'static str {
        "Own code"
    }
    fn help(&self) -> &'static str {
        "Your own effect code, in the same form as an effect file: header lines, then fn effect(uv, p)."
    }
    fn settings(&self) -> &'static [SettingDef] {
        static DEFS: OnceLock<Vec<SettingDef>> = OnceLock::new();
        DEFS.get_or_init(|| {
            let mut defs = common_defs(true).to_vec();
            defs.push(SettingDef {
                help: "The effect's code, in the same form as an effect file: header lines, then fn effect(uv, p).",
                ..def("code", "Code", ValueType::Text, Value::Text(own_start().to_owned()))
            });
            defs
        })
    }
    fn section(&self) -> LayerSection {
        layer_kind_basics().0
    }
    fn repeats(&self) -> bool {
        layer_kind_basics().1
    }
    fn card(&self) -> &'static str {
        layer_kind_basics().2
    }
    fn takes_subs(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
    fn dynamic_defs(&self, get: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
        match get("code") {
            Some(Value::Text(code)) => own_defs(&code),
            _ => &[],
        }
    }
}

/// The code a new own effect starts with: the shortest built-in over the picture, so it works
/// at once.
pub fn own_start() -> &'static str {
    BUILTINS
        .iter()
        .filter(|b| b.header().stage == Stage::Picture)
        .map(|b| b.wgsl)
        .min_by_key(|w| w.len())
        .expect("built-ins exist")
}

/// The settings own `code` adds under plain keys; none when it doesn't parse. Each distinct
/// code's settings are made once and kept.
pub fn own_defs(code: &str) -> &'static [SettingDef] {
    static MADE: OnceLock<Mutex<HashMap<String, &'static [SettingDef]>>> = OnceLock::new();
    // Kept by the header's lines alone: typing in the body never makes new settings, so the
    // settings (which live for the whole run) are only made again when a header line changes.
    let header: String = code.lines().filter(|l| l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    let mut made = MADE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = made.get(&header) {
        return d;
    }
    let defs: &'static [SettingDef] = match bpe_gfx::effects::parse(code) {
        Ok(h) => {
            let room = MOST_SIMPLE - common_defs(true).iter().filter(|d| d.depth == Depth::Simple).count();
            let mut defs = param_defs(&h, "", "", "Effect", true);
            for d in defs.iter_mut().filter(|d| d.depth == Depth::Simple).skip(room) {
                d.depth = Depth::More;
            }
            Box::leak(defs.into_boxed_slice())
        }
        // Not kept: the same header may parse once the body is finished.
        Err(_) => return &[],
    };
    made.insert(header, defs);
    defs
}

/// Why own `code` can't run over the picture, worked out once per distinct code and kept for
/// the most recent few.
pub fn own_problem(code: &str) -> Option<String> {
    static SEEN: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let mut seen = SEEN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = seen.get(code) {
        return p.clone();
    }
    if seen.len() >= 64 {
        seen.clear();
    }
    let p = bpe_gfx::picture_problem(code);
    seen.insert(code.to_owned(), p.clone());
    p
}

fn def(key: &'static str, label: &'static str, ty: ValueType, default: Value) -> SettingDef {
    SettingDef {
        key,
        label,
        help: "",
        ty,
        default,
        range: None,
        span: None,
        unit: "",
        group: "",
        depth: Depth::Simple,
        flags: Flags::default(),
        when: "",
    }
}

/// The most settings a layer shows at the Simple depth.
const MOST_SIMPLE: usize = 6;

/// `defs` with the Simple settings past the first [`MOST_SIMPLE`] shown at More.
fn with_simple_room(mut defs: Vec<SettingDef>) -> Vec<SettingDef> {
    for d in defs.iter_mut().filter(|d| d.depth == Depth::Simple).skip(MOST_SIMPLE) {
        d.depth = Depth::More;
    }
    defs
}

/// Mix, Blend and (over the picture) Takes and Glow above.
fn common_defs(over_picture: bool) -> &'static [SettingDef] {
    static WITH: OnceLock<Vec<SettingDef>> = OnceLock::new();
    static WITHOUT: OnceLock<Vec<SettingDef>> = OnceLock::new();
    let make = |takes: bool| {
        let keyed = Flags { keyframe: true, pulse: true, hold: false };
        let mut defs = vec![
            SettingDef {
                help: "How much of the effect shows: 0 none, 1 all of it. Above 1 pushes it further; below 0 \
                       pulls the other way.",
                span: Some((0.0, 1.0)),
                flags: keyed,
                ..def(MIX, "Mix", ValueType::Number, Value::Number(1.0))
            },
            SettingDef {
                help: "How the effect's result goes onto what it took.",
                depth: Depth::More,
                ..def("blend", "Blend", Blend::value_type(), Blend::Normal.to_value())
            },
        ];
        if takes {
            defs.push(SettingDef {
                help: "What the effect works on: the whole picture, only the light falling on things (needs a \
                       Lighting card), or only the parts brighter than Glow above.",
                ..def("takes", "Takes", TakesChoice::value_type(), TakesChoice::WholePicture.to_value())
            });
            defs.push(SettingDef {
                help: "How bright a part must be to count as glow: the effect works only on what is brighter.",
                span: Some((0.0, 2.0)),
                when: "takes=glow_only",
                depth: Depth::More,
                flags: Flags { keyframe: true, pulse: false, hold: false },
                ..def("glow_above", "Glow above", ValueType::Number, Value::Number(0.9))
            });
        }
        defs
    };
    if over_picture { WITH.get_or_init(|| make(true)) } else { WITHOUT.get_or_init(|| make(false)) }
}

/// One setting per line of an effect's header, each key starting with `prefix` (such as
/// `"lava."` for a ball's Shader layer), shown when `when` holds, under `group`. The header's
/// `[a, b]` is the slider's usual span and `hard [lo, hi]` a real limit. The strings are kept for
/// the program's life, as every setting's words are. Without `simple`, every one shows at More.
pub fn param_defs(
    header: &Header,
    prefix: &str,
    when: &'static str,
    group: &'static str,
    simple: bool,
) -> Vec<SettingDef> {
    let keep = |s: &str| -> &'static str { Box::leak(s.to_owned().into_boxed_str()) };
    let pair = |(a, b): (f32, f32)| (a as f64, b as f64);
    let mut defs = Vec::new();
    for p in &header.params {
        let mut label = p.key.replace('_', " ");
        if let Some(first) = label.get(..1) {
            label = first.to_uppercase() + &label[1..];
        }
        let keyed = Flags { keyframe: true, pulse: true, hold: false };
        let (ty, default, range, span, flags) = match p.kind {
            ParamKind::Number { default, min, max, int: false, hard } => {
                (ValueType::Number, Value::Number(default as f64), hard.map(pair), Some(pair((min, max))), keyed)
            }
            ParamKind::Number { default, min, max, int: true, hard } => {
                (ValueType::Int, Value::Int(default.round() as i64), hard.map(pair), Some(pair((min, max))), keyed)
            }
            ParamKind::Choice { default, names } => {
                (ValueType::Choice(names), Value::Choice(names[default].to_owned()), None, None, Flags::default())
            }
            ParamKind::Toggle { default } => (
                ValueType::Bool,
                Value::Bool(default),
                None,
                None,
                Flags { keyframe: false, pulse: false, hold: false },
            ),
            ParamKind::Color { default } => (
                ValueType::Color,
                Value::Color(Color::from_linear([default[0], default[1], default[2], 1.0])),
                None,
                None,
                Flags { keyframe: true, pulse: false, hold: false },
            ),
        };
        defs.push(SettingDef {
            key: keep(&format!("{prefix}{}", p.key)),
            label: keep(&label),
            help: keep(&p.help),
            ty,
            default,
            range,
            span,
            unit: "",
            group,
            depth: if p.simple && simple { Depth::Simple } else { Depth::More },
            flags,
            when,
        });
    }
    defs
}

/// An effect layer's settings read back (plain keys, as `ResolvedSlot::layer_params` gives them).
pub fn takes_of(p: &Params) -> TakesChoice {
    p.get("takes").and_then(TakesChoice::from_value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_is_an_effect_layer_kind_with_mix_first() {
        let reg = crate::registry();
        let kinds: Vec<PictureEffect> = kinds().collect();
        assert_eq!(kinds.len(), BUILTINS.len());
        for (i, k) in kinds.iter().enumerate() {
            assert_eq!(k.id(), format!("picture.{}", BUILTINS[i].id));
            assert!(reg.layer(k.id()).is_some(), "{} registered", k.id());
            assert_eq!((k.section(), k.card()), (LayerSection::Effect, EFFECTS));
            assert_eq!(builtin_of(k.id()).map(|b| b.id), Some(BUILTINS[i].id));
        }
        let k = kinds.iter().find(|k| k.id() == "picture.kaleidoscope").unwrap();
        assert_eq!(k.name(), "Kaleidoscope");
        assert!(k.help().starts_with("Mirrors the picture"));
        let keys: Vec<&str> = k.settings().iter().map(|d| d.key).collect();
        assert_eq!(&keys[..7], ["mix", "blend", "takes", "glow_above", "slices", "spin", "zoom"]);
        let slices = k.settings().iter().find(|d| d.key == "slices").unwrap();
        assert_eq!((slices.ty, slices.label, &slices.default), (ValueType::Int, "Slices", &Value::Int(6)));
        assert_eq!((slices.span, slices.range), (Some((2.0, 24.0)), Some((1.0, 1e9))));
        let fold = k.settings().iter().find(|d| d.key == "fold").unwrap();
        assert_eq!(fold.default, Value::Choice("slices".into()));
        let lava = kinds.iter().find(|k| k.id() == "picture.lava").unwrap();
        assert!(!lava.settings().iter().any(|d| d.key == "takes"), "a material has no Takes");
        let dark = lava.settings().iter().find(|d| d.key == "dark").unwrap();
        assert_eq!((dark.ty, &dark.default), (ValueType::Color, &Value::Color(Color::rgb(0x14, 0, 0))));
        assert_eq!(reg.module(EFFECTS).map(|m| m.slot()), Some(SlotId::Shader));
        assert!(reg.module(EFFECTS).unwrap().fits("scene") && !reg.module(EFFECTS).unwrap().fits("balls"));
        assert_eq!(bpe_core::check_registry(&reg), Vec::<String>::new());
    }

    #[test]
    fn own_code_adds_its_settings() {
        let own = OwnEffect;
        let code = "// name: Mine\n// simple amount = 0.5 [0, 1] How much\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return sample(uv) * p.amount; }\n";
        let added = own.dynamic_defs(&|k| (k == "code").then(|| Value::Text(code.into())));
        assert_eq!(added.iter().map(|d| d.key).collect::<Vec<_>>(), ["amount"]);
        assert!(own.dynamic_defs(&|k| (k == "code").then(|| Value::Text("nope".into()))).is_empty());
        assert!(own.settings().iter().any(|d| d.key == "code"));
    }
}
