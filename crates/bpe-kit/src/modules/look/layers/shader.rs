//! Shader: one of the built-in effects, or the user's own effect code, painted inside the ball in
//! its place in the stack. The effect sees the layers under it as its picture.
//!
//! The kind's settings are the common ones, then every built-in's own settings under keys
//! `"<effect id>.<setting>"` shown only for that effect. Own code's settings come from its header,
//! under `"own.<setting>"` ([`LayerKind::dynamic_defs`]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use bpe_core::{Blend, Depth, Flags, Params, SettingDef, SettingValue, Value, ValueType};
use bpe_gfx::effects::{BUILTINS, Builtin, Header, MAX_PARAM_FLOATS};

use super::shading::Space;
use crate::modules::effects::draw::pack_params;
use crate::modules::effects::param_defs;

/// What the Effect setting offers: every built-in by id but the lens effects (which need the whole
/// picture), then own code.
pub const EFFECTS: [&str; 28] = [
    "color_cycle",
    "color_split",
    "echo",
    "film",
    "glitch",
    "kaleidoscope",
    "liquid",
    "mirror",
    "pixels",
    "shockwave",
    "hue_storm",
    "liquid_light",
    "light_ripples",
    "ball_worlds",
    "polar_tunnel",
    "breathing",
    "thermal",
    "op_art",
    "fractal",
    "rainbow_echoes",
    "melting",
    "solarize",
    "gravity_lens",
    "lava",
    "rainbow_flow",
    "hit_glow",
    "fringes",
    OWN,
];

/// The Effect setting's name for the user's own code.
pub const OWN: &str = "own";

/// The help every Shader layer shows: what its effect can and can't see.
pub const HELP: &str = "An effect painted inside the ball. It sees the layers under it, leaving out other Shader \
                        layers; an effect that reads earlier frames sees this one again.";

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

/// Every setting of the Shader layer kind, made once.
pub fn defs() -> &'static [SettingDef] {
    static DEFS: OnceLock<Vec<SettingDef>> = OnceLock::new();
    DEFS.get_or_init(|| {
        let keyed = Flags { keyframe: true, pulse: true, hold: false, area: bpe_core::Area::No };
        let mut defs = vec![
            SettingDef {
                help: "Which effect paints the layer: one that comes with Ball Studio, or your own code.",
                ..def("effect", "Effect", ValueType::Choice(&EFFECTS), Value::Choice("lava".into()))
            },
            SettingDef {
                help: "The effect's code, in the same form as an effect file: header lines, then fn effect(uv, p).",
                when: "effect=own",
                ..def("code", "Code", ValueType::Text, Value::Text(own_start().to_owned()))
            },
            SettingDef {
                help: "How fast the effect's time runs: 0 stands still.",
                range: Some((0.0, 4.0)),
                flags: keyed,
                ..def("speed", "Speed", ValueType::Number, Value::Number(1.0))
            },
            SettingDef {
                help: "How big the effect is drawn: bigger spreads it wider over the ball.",
                range: Some((0.1, 8.0)),
                flags: Flags { keyframe: true, pulse: false, hold: false, area: bpe_core::Area::No },
                ..def("scale", "Scale", ValueType::Number, Value::Number(1.0))
            },
            SettingDef {
                help: "Whether the effect turns with the ball or stays upright on the screen.",
                ..def("space", "Stays with", Space::value_type(), Space::Ball.to_value())
            },
            SettingDef {
                help: "0 is invisible, 1 is solid.",
                range: Some((0.0, 1.0)),
                group: "Mixing",
                depth: Depth::More,
                flags: keyed,
                ..def("opacity", "Opacity", ValueType::Number, Value::Number(1.0))
            },
            SettingDef {
                help: "How it mixes with the layers under it in the same ball.",
                group: "Mixing",
                depth: Depth::More,
                ..def("blend", "Blend", Blend::value_type(), Blend::Normal.to_value())
            },
        ];
        for b in BUILTINS.iter().filter(|b| !b.header().lens.any()) {
            let when: &'static str = Box::leak(format!("effect={}", b.id).into_boxed_str());
            defs.extend(param_defs(b.header(), &format!("{}.", b.id), when, "Effect", false));
        }
        defs
    })
}

/// The code a new Own code effect starts with: the shortest built-in, so it works at once.
pub fn own_start() -> &'static str {
    BUILTINS.iter().map(|b| b.wgsl).min_by_key(|w| w.len()).expect("built-ins exist")
}

/// The settings own `code` adds, under `"own.<setting>"`; none when it doesn't parse. Each
/// distinct code's settings are made once and kept.
pub fn own_defs(code: &str) -> &'static [SettingDef] {
    static MADE: OnceLock<Mutex<HashMap<String, &'static [SettingDef]>>> = OnceLock::new();
    let mut made = MADE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(d) = made.get(code) {
        return d;
    }
    let defs: &'static [SettingDef] = match bpe_gfx::effects::parse(code) {
        Ok(h) => Box::leak(param_defs(&h, "own.", "effect=own", "Effect", false).into_boxed_slice()),
        Err(_) => &[],
    };
    made.insert(code.to_owned(), defs);
    defs
}

/// The settings a layer's own values add: its own code's, when Effect is Own code.
pub fn dynamic_defs(get: &dyn Fn(&str) -> Option<Value>) -> &'static [SettingDef] {
    match (get("effect"), get("code")) {
        (Some(Value::Choice(e)), Some(Value::Text(code))) if e == OWN => own_defs(&code),
        _ => &[],
    }
}

/// Why own `code` can't run inside a ball (its header, then its WGSL), worked out once per
/// distinct code and kept for the most recent few.
pub fn own_problem(code: &str) -> Option<String> {
    static SEEN: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let mut seen = SEEN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = seen.get(code) {
        return p.clone();
    }
    if seen.len() >= 64 {
        seen.clear();
    }
    let p = bpe_gfx::in_ball_problem(code);
    seen.insert(code.to_owned(), p.clone());
    p
}

/// A Shader layer read from its settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderLayer {
    /// The effect's id, or [`OWN`].
    pub effect: String,
    /// The effect's source: a built-in's file or the layer's own code.
    pub source: std::borrow::Cow<'static, str>,
    /// Its header, or why the code doesn't read.
    pub header: Result<Header, String>,
    /// The effect's settings, packed the way the shader reads them.
    pub params: [f32; MAX_PARAM_FLOATS],
    pub speed: f32,
    pub scale: f32,
    pub space: Space,
    pub opacity: f32,
    pub blend: Blend,
}

impl ShaderLayer {
    /// Reads a layer's settings (plain keys, as `ResolvedSlot::layer_params` gives them).
    pub fn from_params(p: &Params) -> ShaderLayer {
        let number = |k: &str, d: f32| match p.get(k) {
            Some(Value::Number(x)) if x.is_finite() => *x as f32,
            _ => d,
        };
        let effect = match p.get("effect") {
            Some(Value::Choice(e)) if EFFECTS.contains(&e.as_str()) => e.clone(),
            _ => "lava".to_owned(),
        };
        let (source, header) = if effect == OWN {
            let code = match p.get("code") {
                Some(Value::Text(t)) => t.clone(),
                _ => own_start().to_owned(),
            };
            // Code that can't run in a ball draws nothing, leaving the stack's other shader layers
            // to build and draw.
            let header = match own_problem(&code) {
                Some(why) => Err(why),
                None => bpe_gfx::effects::parse(&code),
            };
            (std::borrow::Cow::Owned(code), header)
        } else {
            let b = Builtin::find(&effect).expect("listed");
            (std::borrow::Cow::Borrowed(b.wgsl), Ok(b.header().clone()))
        };
        let prefix = format!("{effect}.");
        let own: Params =
            Params(p.iter().filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_owned(), v.clone()))).collect());
        let params = header.as_ref().map(|h| pack_params(h, &own)).unwrap_or([0.0; MAX_PARAM_FLOATS]);
        ShaderLayer {
            effect,
            source,
            header,
            params,
            speed: number("speed", 1.0),
            scale: number("scale", 1.0).max(0.1),
            space: p.get("space").and_then(Space::from_value).unwrap_or_default(),
            opacity: number("opacity", 1.0),
            blend: p.get("blend").and_then(Blend::from_value).unwrap_or_default(),
        }
    }

    /// What the editor calls its effect: the built-in's name, or "Own code".
    pub fn effect_name(&self) -> String {
        if self.effect == OWN {
            return "Own code".to_owned();
        }
        Builtin::find(&self.effect).map_or_else(|| self.effect.clone(), |b| b.header().name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtins_settings_are_listed_under_its_id_and_shown_only_for_it() {
        let defs = defs();
        for b in &BUILTINS {
            // A lens effect needs the whole picture: it isn't offered inside a ball.
            assert_eq!(EFFECTS.contains(&b.id), !b.header().lens.any(), "{}", b.id);
            if b.header().lens.any() {
                continue;
            }
            for p in &b.header().params {
                let key = format!("{}.{}", b.id, p.key);
                let d = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{key}"));
                assert_eq!(d.when, format!("effect={}", b.id));
            }
        }
        assert!(defs.iter().any(|d| d.key == "lava.heat" || d.key.starts_with("lava.")));
        let keys: Vec<&str> = defs.iter().take(7).map(|d| d.key).collect();
        assert_eq!(keys, ["effect", "code", "speed", "scale", "space", "opacity", "blend"]);
    }

    #[test]
    fn own_code_adds_its_settings_and_reads_with_its_header() {
        let code = "// name: Mine\n// simple warmth = 0.25 [0, 1] How warm\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return sample(uv) * p.warmth; }\n";
        let get = |k: &str| match k {
            "effect" => Some(Value::Choice(OWN.into())),
            "code" => Some(Value::Text(code.into())),
            _ => None,
        };
        let added = dynamic_defs(&get);
        assert_eq!(added.iter().map(|d| d.key).collect::<Vec<_>>(), ["own.warmth"]);
        assert!(std::ptr::eq(added, dynamic_defs(&get)), "made once");
        let mut p = Params::default();
        p.set("effect", Value::Choice(OWN.into()));
        p.set("code", Value::Text(code.into()));
        p.set("own.warmth", Value::Number(0.75));
        let layer = ShaderLayer::from_params(&p);
        assert_eq!(layer.params[0], 0.75);
        assert_eq!(layer.effect_name(), "Own code");
        p.set("code", Value::Text("fn main() {}".into()));
        assert!(ShaderLayer::from_params(&p).header.is_err());
        let not_own = |k: &str| if k == "effect" { Some(Value::Choice("lava".into())) } else { None };
        assert!(dynamic_defs(&not_own).is_empty());
    }
}
