//! Shader effects: one WGSL file each, run over the HDR picture by the effect chain.
//!
//! An effect file defines `fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>` and starts with
//! comment lines that name it and declare its settings:
//!
//! ```text
//! // name: Kaleidoscope
//! // about: Mirrors the picture into slices around the centre.
//! // stage: things
//! // simple slices = 6 [2, 24] int Number of mirrored slices
//! // param spin = 0.03 [-1, 1] Turns per second
//! // param dots = 0 toggle Draw round dots
//! // color tint = #ff8000 The tint
//! ```
//!
//! `simple` settings show at the Simple depth, `param` ones at More. A `color` takes three of
//! the sixteen floats the shader gets. `stage: things` runs the effect before flashes and
//! particles are drawn (materials do); the default runs it over the finished picture. The
//! module kinds in `bpe-kit` are generated from these headers, and stage 4's node graphs will
//! compile to the same file shape.

pub mod chain;

use std::sync::OnceLock;

pub use crate::frame::Stage;

/// How many floats an effect's settings can take together.
pub const MAX_PARAM_FLOATS: usize = 16;

pub const PRELUDE: &str = include_str!("prelude.wgsl");

/// What an effect file's header says.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub name: String,
    pub about: String,
    pub stage: Stage,
    pub params: Vec<ParamDef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDef {
    pub key: String,
    pub help: String,
    pub kind: ParamKind,
    /// Shown at the Simple depth.
    pub simple: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    Number {
        default: f32,
        min: f32,
        max: f32,
        int: bool,
    },
    Toggle {
        default: bool,
    },
    /// Linear light.
    Color {
        default: [f32; 3],
    },
}

impl ParamDef {
    /// How many of the shader's floats this setting takes.
    pub fn floats(&self) -> usize {
        match self.kind {
            ParamKind::Color { .. } => 3,
            _ => 1,
        }
    }
}

impl Header {
    pub fn floats(&self) -> usize {
        self.params.iter().map(ParamDef::floats).sum()
    }
}

/// Names a setting can't have: the module's own Blend, and words WGSL keeps for itself.
const RESERVED: &[&str] = &[
    "blend", "target", "active", "move", "fn", "let", "var", "const", "if", "else", "for", "loop", "return", "struct",
    "true", "false", "break", "continue", "switch", "case", "default", "while", "discard", "override", "enable",
    "type", "alias", "p", "fx", "uv",
];

/// Reads the header. The source must define `fn effect(`.
pub fn parse(wgsl: &str) -> Result<Header, String> {
    let mut header = Header { name: String::new(), about: String::new(), stage: Stage::Picture, params: Vec::new() };
    let mut errors = Vec::new();
    for (index, line) in wgsl.lines().enumerate() {
        let Some(comment) = line.trim().strip_prefix("//") else { continue };
        let comment = comment.trim();
        let (keyword, rest) = match comment.split_once(|c: char| c.is_whitespace() || c == ':') {
            Some((k, r)) => (k, r.trim()),
            None => continue,
        };
        let outcome = match keyword {
            "name" => {
                header.name = rest.to_owned();
                Ok(())
            }
            "about" => {
                header.about = rest.to_owned();
                Ok(())
            }
            "stage" => match rest {
                "things" => {
                    header.stage = Stage::Things;
                    Ok(())
                }
                "picture" => Ok(()),
                other => Err(format!("the stage must be `things` or `picture`, not `{other}`")),
            },
            "param" => parse_param(rest, false).map(|p| header.params.push(p)),
            "simple" => parse_param(rest, true).map(|p| header.params.push(p)),
            "color" => parse_color(rest).map(|p| header.params.push(p)),
            _ => continue,
        };
        if let Err(error) = outcome {
            errors.push(format!("line {}: {error}", index + 1));
        }
    }
    for (i, p) in header.params.iter().enumerate() {
        if header.params[..i].iter().any(|q| q.key == p.key) {
            errors.push(format!("`{}` is declared twice", p.key));
        }
    }
    if header.floats() > MAX_PARAM_FLOATS {
        errors.push(format!(
            "an effect's settings can take at most {MAX_PARAM_FLOATS} numbers (a colour is three), these take {}",
            header.floats()
        ));
    }
    if !wgsl.contains("fn effect(") {
        errors.push("the file must define fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>".to_owned());
    }
    if errors.is_empty() { Ok(header) } else { Err(errors.join("\n")) }
}

fn parse_key(text: &str) -> Result<(&str, &str), String> {
    let (key, rest) = text.split_once('=').ok_or("expected `key = default ...`")?;
    let key = key.trim();
    let valid = key.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return Err(format!("`{key}` is not a valid setting key (lower-case letters, digits and _ only)"));
    }
    if RESERVED.contains(&key) {
        return Err(format!("`{key}` can't be a setting key"));
    }
    Ok((key, rest.trim_start()))
}

/// `key = default [min, max] [int] help` or `key = 0|1 toggle help`.
fn parse_param(text: &str, simple: bool) -> Result<ParamDef, String> {
    let (key, rest) = parse_key(text)?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let default = rest[..end].parse::<f32>().ok().filter(|v| v.is_finite());
    let default = default.ok_or_else(|| format!("`{}` is not a number", &rest[..end]))?;
    let rest = rest[end..].trim_start();
    let (kind, help) = if let Some(help) = rest.strip_prefix("toggle") {
        (ParamKind::Toggle { default: default != 0.0 }, help)
    } else {
        let inner = rest.strip_prefix('[').ok_or("expected a range like [0, 1] or the word toggle")?;
        let (range, after) = inner.split_once(']').ok_or("missing `]`")?;
        let (lo, hi) = range.split_once(',').ok_or("expected a range like [0, 1]")?;
        let number = |s: &str| s.trim().parse::<f32>().map_err(|_| format!("`{}` is not a number", s.trim()));
        let (min, max) = (number(lo)?, number(hi)?);
        if !min.is_finite() || !max.is_finite() || min >= max {
            return Err(format!("the range [{min}, {max}] is empty"));
        }
        if !(min..=max).contains(&default) {
            return Err(format!("the default {default} is outside the range [{min}, {max}]"));
        }
        let after = after.trim_start();
        match after.strip_prefix("int") {
            Some(help) if help.is_empty() || help.starts_with(char::is_whitespace) => {
                (ParamKind::Number { default, min, max, int: true }, help)
            }
            _ => (ParamKind::Number { default, min, max, int: false }, after),
        }
    };
    Ok(ParamDef { key: key.to_owned(), help: help.trim().to_owned(), kind, simple })
}

/// `key = #rrggbb help`.
fn parse_color(text: &str) -> Result<ParamDef, String> {
    let (key, rest) = parse_key(text)?;
    let hex = rest.strip_prefix('#').ok_or("expected a colour like #ff8000")?;
    let (hex, help) = hex.split_at(hex.find(char::is_whitespace).unwrap_or(hex.len()));
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("zz"), 16).ok();
    let (Some(r), Some(g), Some(b)) = (byte(0), byte(2), byte(4)) else {
        return Err(format!("`#{hex}` is not a colour like #ff8000"));
    };
    if hex.len() != 6 {
        return Err(format!("`#{hex}` is not a colour like #ff8000"));
    }
    let linear = bpe_core::Color::rgb(r, g, b).to_linear();
    Ok(ParamDef {
        key: key.to_owned(),
        help: help.trim().to_owned(),
        kind: ParamKind::Color { default: [linear[0], linear[1], linear[2]] },
        simple: false,
    })
}

/// Whether the effect reads its previous frame, which needs a history texture.
pub fn needs_history(wgsl: &str) -> bool {
    wgsl.contains("prev(")
}

/// Every setting's default, packed the way the shader reads them.
pub fn defaults(header: &Header) -> [f32; MAX_PARAM_FLOATS] {
    let mut out = [0.0; MAX_PARAM_FLOATS];
    let mut at = 0;
    for p in &header.params {
        match p.kind {
            ParamKind::Number { default, .. } => out[at] = default,
            ParamKind::Toggle { default } => out[at] = default as u8 as f32,
            ParamKind::Color { default } => out[at..at + 3].copy_from_slice(&default),
        }
        at += p.floats();
    }
    out
}

/// The whole shader: the prelude, a `Params` struct, the effect file and the entry point.
pub fn assemble(wgsl: &str) -> Result<String, String> {
    let header = parse(wgsl)?;
    let mut head = String::from(PRELUDE);
    head.push_str("\nstruct Params {\n");
    for p in &header.params {
        let ty = if matches!(p.kind, ParamKind::Color { .. }) { "vec3<f32>" } else { "f32" };
        head.push_str(&format!("    {}: {ty},\n", p.key));
    }
    if header.params.is_empty() {
        head.push_str("    _unused: f32,\n");
    }
    head.push_str("};\n\n");
    let mut tail = String::from("\nfn unpack() -> Params {\n    var p: Params;\n");
    let mut at = 0;
    for p in &header.params {
        let float = |i: usize| format!("fx.params[{}][{}]", i / 4, i % 4);
        if p.floats() == 3 {
            tail.push_str(&format!(
                "    p.{} = vec3<f32>({}, {}, {});\n",
                p.key,
                float(at),
                float(at + 1),
                float(at + 2)
            ));
        } else {
            tail.push_str(&format!("    p.{} = {};\n", p.key, float(at)));
        }
        at += p.floats();
    }
    tail.push_str(
        "    return p;
}

@fragment
fn fs_main(in: FsIn) -> @location(0) vec4<f32> {
    let result = effect(in.uv, unpack());
    // Effects on things leave every other pixel exactly as it was.
    var base = sample(in.uv);
    if fx.apply_to > 0.5 {
        base = textureLoad(src_tex, vec2<i32>(in.pos.xy), 0).rgb;
    }
    let blended = mix(base, result, fx.blend * target_weight(in.uv));
    // Keep feedback from running away and stop a NaN from spreading.
    let safe = select(blended, vec3<f32>(0.0), blended != blended);
    return vec4<f32>(clamp(safe, vec3<f32>(0.0), vec3<f32>(64.0)), 1.0);
}
",
    );
    Ok(format!("{head}{wgsl}\n{tail}"))
}

/// An effect that comes with the program.
#[derive(Debug)]
pub struct Builtin {
    /// The file name without `.wgsl`.
    pub id: &'static str,
    pub wgsl: &'static str,
    header: OnceLock<Header>,
}

/// The effects that come with the program: ten over the picture, then three materials.
pub static BUILTINS: [Builtin; 13] = [
    Builtin::new("color_cycle", include_str!("builtin/color_cycle.wgsl")),
    Builtin::new("color_split", include_str!("builtin/color_split.wgsl")),
    Builtin::new("echo", include_str!("builtin/echo.wgsl")),
    Builtin::new("film", include_str!("builtin/film.wgsl")),
    Builtin::new("glitch", include_str!("builtin/glitch.wgsl")),
    Builtin::new("kaleidoscope", include_str!("builtin/kaleidoscope.wgsl")),
    Builtin::new("liquid", include_str!("builtin/liquid.wgsl")),
    Builtin::new("mirror", include_str!("builtin/mirror.wgsl")),
    Builtin::new("pixels", include_str!("builtin/pixels.wgsl")),
    Builtin::new("shockwave", include_str!("builtin/shockwave.wgsl")),
    Builtin::new("lava", include_str!("builtin/lava.wgsl")),
    Builtin::new("rainbow_flow", include_str!("builtin/rainbow_flow.wgsl")),
    Builtin::new("hit_glow", include_str!("builtin/hit_glow.wgsl")),
];

impl Builtin {
    const fn new(id: &'static str, wgsl: &'static str) -> Self {
        Self { id, wgsl, header: OnceLock::new() }
    }

    /// The built-in effect with this id.
    pub fn find(id: &str) -> Option<&'static Builtin> {
        BUILTINS.iter().find(|b| b.id == id)
    }

    /// Its header, read once. Built-ins are checked by tests, so a bad one is a bug.
    pub fn header(&self) -> &Header {
        self.header.get_or_init(|| parse(self.wgsl).unwrap_or_else(|e| panic!("built-in effect {}: {e}", self.id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "// name: Test\n// about: A test\n// stage: things\n// simple speed = 0.5 [0, 2] How fast\n// param bands = 6 [2, 24] int Count\n// param dots = 1 toggle Round\n// color tint = #ff8000 The tint\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return vec3<f32>(p.speed * f32(p.bands) * p.dots) * p.tint; }\n";

    fn validate(full: &str) {
        let module = naga::front::wgsl::parse_str(full).unwrap_or_else(|e| panic!("{}", e.emit_to_string(full)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn the_header_gives_the_name_stage_and_every_param() {
        let h = parse(SAMPLE).unwrap();
        assert_eq!((h.name.as_str(), h.about.as_str(), h.stage), ("Test", "A test", Stage::Things));
        let keys: Vec<&str> = h.params.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["speed", "bands", "dots", "tint"]);
        assert!(h.params[0].simple && !h.params[1].simple);
        assert_eq!(h.params[0].help, "How fast");
        assert_eq!(h.params[1].kind, ParamKind::Number { default: 6.0, min: 2.0, max: 24.0, int: true });
        assert_eq!(h.params[2].kind, ParamKind::Toggle { default: true });
        let ParamKind::Color { default } = h.params[3].kind else { panic!() };
        assert!((default[0] - 1.0).abs() < 1e-6 && (default[1] - 0.2158).abs() < 1e-3 && default[2] == 0.0);
        assert_eq!(h.params[3].help, "The tint");
        assert_eq!(h.floats(), 6);
        let d = defaults(&h);
        assert_eq!(&d[..3], &[0.5, 6.0, 1.0]);
        assert_eq!(d[3], 1.0);
        assert_eq!(d[6], 0.0);
    }

    #[test]
    fn bad_headers_are_refused_with_a_reason() {
        assert!(parse("// param blend = 1 [0, 1] x\nfn effect(").unwrap_err().contains("blend"));
        let twice = parse("// param a = 1 [0, 1] x\n// param a = 0.5 [0, 1] y\nfn effect(").unwrap_err();
        assert!(twice.contains("twice"), "{twice}");
        assert!(parse("// param Bad-Key = 1 [0, 1] x\nfn effect(").unwrap_err().contains("Bad-Key"));
        assert!(parse("// param a = 5 [0, 1] x\nfn effect(").unwrap_err().contains("outside"));
        assert!(parse("// color c = #12 x\nfn effect(").unwrap_err().contains("#12"));
        assert!(parse("// stage: later\nfn effect(").unwrap_err().contains("later"));
        assert!(parse("fn main() {}").unwrap_err().contains("fn effect("));
        let many: String = (0..17).map(|i| format!("// param p{i} = 0 [0, 1] x\n")).collect();
        assert!(parse(&format!("{many}fn effect(")).unwrap_err().contains("16"));
        let colours: String = (0..6).map(|i| format!("// color c{i} = #000000 x\n")).collect();
        assert!(parse(&format!("{colours}fn effect(")).unwrap_err().contains("18"));
    }

    #[test]
    fn an_assembled_effect_is_valid_wgsl() {
        let full = assemble(SAMPLE).unwrap();
        assert!(full.contains("struct Params") && full.contains("tint: vec3<f32>") && full.contains("fn fs_main"));
        assert!(full.contains("p.tint = vec3<f32>(fx.params[0][3], fx.params[1][0], fx.params[1][1]);"));
        validate(&full);
        assert!(!needs_history(SAMPLE) && needs_history("fn effect() { let a = prev(uv); }"));
    }

    #[test]
    fn every_builtin_parses_assembles_and_validates() {
        for b in &BUILTINS {
            let h = b.header();
            assert!(!h.name.is_empty() && !h.about.is_empty(), "{}", b.id);
            assert!(h.params.iter().filter(|p| p.simple).count() <= 5, "{}: too many simple params", b.id);
            let full = assemble(b.wgsl).unwrap_or_else(|e| panic!("{}: {e}", b.id));
            validate(&full);
        }
        let things: Vec<&str> = BUILTINS.iter().filter(|b| b.header().stage == Stage::Things).map(|b| b.id).collect();
        assert_eq!(things, ["lava", "rainbow_flow", "hit_glow"]);
        assert_eq!(Builtin::find("echo").map(|b| b.id), Some("echo"));
        assert!(Builtin::find("nope").is_none());
        assert!(needs_history(Builtin::find("echo").unwrap().wgsl));
        assert_eq!(Builtin::find("lava").unwrap().header().floats(), 12);
    }

    #[test]
    fn an_effect_with_no_params_still_assembles() {
        let full = assemble("// name: Plain\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return sample(uv); }")
            .unwrap();
        validate(&full);
        assert!(assemble("fn main() {}").is_err());
    }
}
