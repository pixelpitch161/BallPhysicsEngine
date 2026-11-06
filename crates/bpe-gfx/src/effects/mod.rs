//! Shader effects: one WGSL file each, run over the HDR picture by the effect chain.
//!
//! An effect file defines `fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>` and starts with
//! comment lines that name it and declare its settings:
//!
//! ```text
//! // name: Kaleidoscope
//! // about: Mirrors the picture into slices around the centre.
//! // stage: things
//! // simple slices = 6 [2, 24] hard [1, 1e9] int Number of mirrored slices
//! // param spin = 0.03 [-1, 1] Turns per second
//! // param dots = 0 toggle Draw round dots
//! // choice fold = slices [slices, rings, grid] How the picture folds
//! // color tint = #ff8000 The tint
//! // memory: fade keep
//! ```
//!
//! `simple` settings show at the Simple depth, `param` ones at More. A number's `[a, b]` is the
//! slider's usual span: typed or dragged values go past it. `hard [a, b]` is a real limit, for
//! values that break the maths (each says why in its effect). A `choice` is packed as the index
//! of the picked name. A `color` takes three of the sixteen floats the shader gets. `memory`
//! says how long an effect that reads its earlier frames remembers: a number of seconds, or
//! `fade <key>`, a setting that keeps that share of the picture every 1/60 s. `stage: things` runs the effect before flashes and
//! particles are drawn (materials do); the default runs it over the finished picture. The
//! module kinds in `bpe-kit` are generated from these headers, and stage 4's node graphs will
//! compile to the same file shape.

pub mod chain;

use std::sync::OnceLock;

pub use crate::frame::Stage;

/// How many floats an effect's settings can take together.
pub const MAX_PARAM_FLOATS: usize = 16;

pub const PRELUDE: &str = include_str!("prelude.wgsl");

/// The longest an effect's memory can be, in seconds: what the editor replays after a seek.
pub const MAX_MEMORY: f32 = 10.0;
/// How long an effect that reads its earlier frames remembers when its header doesn't say.
pub const DEFAULT_MEMORY: f32 = 2.0;

/// What an effect file's header says.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub name: String,
    pub about: String,
    pub stage: Stage,
    pub params: Vec<ParamDef>,
    pub memory: Memory,
}

/// How long an effect remembers its earlier frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Memory {
    /// Its header says nothing: [`DEFAULT_MEMORY`] when it reads `prev(` or `kept(`, else none.
    #[default]
    Unsaid,
    Seconds(f32),
    /// The setting named keeps this share of the picture every 1/60 s.
    Fade(String),
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
        /// The slider's usual span.
        min: f32,
        max: f32,
        int: bool,
        /// A real limit, for values that break the maths.
        hard: Option<(f32, f32)>,
    },
    /// One of `names`, packed as its index.
    Choice {
        default: usize,
        names: &'static [&'static str],
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

/// Names a setting can't have: the module's own Blend (and an own effect's Code), words WGSL keeps for itself, and the
/// effect's own functions.
const RESERVED: &[&str] = &[
    "blend",
    "code",
    "mix",
    "takes",
    "glow_above",
    "target",
    "active",
    "move",
    "fn",
    "let",
    "var",
    "const",
    "if",
    "else",
    "for",
    "loop",
    "return",
    "struct",
    "true",
    "false",
    "break",
    "continue",
    "switch",
    "case",
    "default",
    "while",
    "discard",
    "override",
    "enable",
    "type",
    "alias",
    "p",
    "fx",
    "uv",
    "effect",
    "unpack",
];

/// The leading comment block of an effect: every `//` line before the first line of code.
/// Comments inside the code are not header lines.
pub fn header_lines(wgsl: &str) -> impl Iterator<Item = (usize, &str)> {
    wgsl.lines()
        .enumerate()
        .map(|(i, l)| (i, l.trim()))
        .take_while(|(_, l)| l.is_empty() || l.starts_with("//"))
        .filter_map(|(i, l)| l.strip_prefix("//").map(|c| (i, c)))
}

/// Reads the header. The source must define `fn effect(`.
pub fn parse(wgsl: &str) -> Result<Header, String> {
    let mut header = Header {
        name: String::new(),
        about: String::new(),
        stage: Stage::Picture,
        params: Vec::new(),
        memory: Memory::Unsaid,
    };
    let mut errors = Vec::new();
    for (index, comment) in header_lines(wgsl) {
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
            "choice" => parse_choice(rest).map(|p| header.params.push(p)),
            "memory" => parse_memory(rest).map(|m| header.memory = m),
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
    if let Memory::Fade(key) = &header.memory
        && !header.params.iter().any(|p| &p.key == key && matches!(p.kind, ParamKind::Number { .. }))
    {
        errors.push(format!("memory: fade `{key}` names no number setting"));
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

/// `key = default [a, b] [hard [lo, hi]] [int] help` (`[a, b]` the usual span) or
/// `key = 0|1 toggle help`.
fn parse_param(text: &str, simple: bool) -> Result<ParamDef, String> {
    let (key, rest) = parse_key(text)?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let default = rest[..end].parse::<f32>().ok().filter(|v| v.is_finite());
    let default = default.ok_or_else(|| format!("`{}` is not a number", &rest[..end]))?;
    let rest = rest[end..].trim_start();
    let (kind, help) = if let Some(help) = rest.strip_prefix("toggle") {
        (ParamKind::Toggle { default: default != 0.0 }, help)
    } else {
        let ((min, max), mut after) = parse_pair(rest)?;
        let (mut hard, mut int) = (None, false);
        loop {
            after = after.trim_start();
            if let Some(more) = word(after, "hard") {
                let (pair, left) = parse_pair(more.trim_start())?;
                hard = Some(pair);
                after = left;
            } else if let Some(more) = word(after, "int") {
                int = true;
                after = more;
            } else {
                break;
            }
        }
        if let Some((lo, hi)) = hard
            && !(lo..=hi).contains(&default)
        {
            return Err(format!("the default {default} is outside the hard limit [{lo}, {hi}]"));
        }
        (ParamKind::Number { default, min, max, int, hard }, after)
    };
    Ok(ParamDef { key: key.to_owned(), help: help.trim().to_owned(), kind, simple })
}

/// `text` after the whole word `w` at its start, if it starts with it.
fn word<'a>(text: &'a str, w: &str) -> Option<&'a str> {
    text.strip_prefix(w).filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// `[a, b]` at the start of `text` (a below b, both finite), and what follows it.
fn parse_pair(text: &str) -> Result<((f32, f32), &str), String> {
    let inner = text.strip_prefix('[').ok_or("expected a span like [0, 1] or the word toggle")?;
    let (range, after) = inner.split_once(']').ok_or("missing `]`")?;
    let (lo, hi) = range.split_once(',').ok_or("expected a span like [0, 1]")?;
    let number = |s: &str| s.trim().parse::<f32>().map_err(|_| format!("`{}` is not a number", s.trim()));
    let (min, max) = (number(lo)?, number(hi)?);
    if !min.is_finite() || !max.is_finite() || min >= max {
        return Err(format!("the span [{min}, {max}] is empty"));
    }
    Ok(((min, max), after))
}

/// `key = default [a, b, c] help`: a choice of names, `default` one of them.
fn parse_choice(text: &str) -> Result<ParamDef, String> {
    let (key, rest) = parse_key(text)?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let default = &rest[..end];
    let rest = rest[end..].trim_start();
    let inner = rest.strip_prefix('[').ok_or("expected the names like [one, two]")?;
    let (list, help) = inner.split_once(']').ok_or("missing `]`")?;
    let names: Vec<&str> = list.split(',').map(str::trim).collect();
    let valid = |n: &str| !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if names.len() < 2 || !names.iter().all(|n| valid(n)) {
        return Err("a choice needs two or more names (lower-case letters, digits and _)".to_owned());
    }
    let Some(at) = names.iter().position(|n| *n == default) else {
        return Err(format!("the default `{default}` isn't one of the names"));
    };
    let names: &'static [&'static str] = interned_names(&names);
    Ok(ParamDef {
        key: key.to_owned(),
        help: help.trim().to_owned(),
        kind: ParamKind::Choice { default: at, names },
        simple: false,
    })
}

/// `names` kept for the program's life, each distinct list once (headers are read again and
/// again as own code is typed).
fn interned_names(names: &[&str]) -> &'static [&'static str] {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static MADE: OnceLock<Mutex<HashMap<Vec<String>, &'static [&'static str]>>> = OnceLock::new();
    let key: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    let mut made = MADE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = made.get(&key) {
        return found;
    }
    let kept: Vec<&'static str> = names.iter().map(|n| &*Box::leak((*n).to_owned().into_boxed_str())).collect();
    let kept: &'static [&'static str] = Box::leak(kept.into_boxed_slice());
    made.insert(key, kept);
    kept
}

/// `<seconds>` or `fade <key>`.
fn parse_memory(text: &str) -> Result<Memory, String> {
    if let Some(key) = word(text, "fade") {
        return Ok(Memory::Fade(key.trim().to_owned()));
    }
    match text.trim().parse::<f32>() {
        Ok(s) if s.is_finite() && s >= 0.0 => Ok(Memory::Seconds(s.min(MAX_MEMORY))),
        _ => Err(format!("memory must be a number of seconds or `fade <setting>`, not `{text}`")),
    }
}

/// How many seconds an effect with `header`, its settings packed as `params`, remembers: 0 for
/// one that never reads its earlier frames, at most [`MAX_MEMORY`].
pub fn memory_seconds(header: &Header, wgsl: &str, params: &[f32; MAX_PARAM_FLOATS]) -> f32 {
    match &header.memory {
        Memory::Seconds(s) => s.clamp(0.0, MAX_MEMORY),
        Memory::Unsaid if needs_history(wgsl) => DEFAULT_MEMORY,
        Memory::Unsaid => 0.0,
        Memory::Fade(key) => {
            let mut at = 0;
            let mut keep = None;
            for p in &header.params {
                if &p.key == key {
                    keep = Some(params[at]);
                }
                at += p.floats();
            }
            let keep = keep.unwrap_or(0.0);
            if keep.is_nan() || keep <= 0.0 {
                return 0.0;
            }
            if keep >= 1.0 {
                return MAX_MEMORY;
            }
            // Frames (of 1/60 s) until what is kept is under 1/4096 of what it was: in linear
            // light, so that what is left shows as less than one step of the 8-bit picture.
            let frames = (1.0f32 / 4096.0).ln() / keep.ln();
            (frames / 60.0).clamp(0.0, MAX_MEMORY)
        }
    }
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

/// Whether the effect reads its previous frame (or what it kept), which needs a history texture.
pub fn needs_history(wgsl: &str) -> bool {
    wgsl.contains("prev(") || wgsl.contains("kept(")
}

/// Whether an effect reads the object picture (which thing is where) itself. Effects on one
/// thing always need it, for their target.
pub fn needs_objects(wgsl: &str) -> bool {
    ["object_at(", "object_code(", "object_seed("].iter().any(|f| wgsl.contains(f))
}

/// Every setting's default, packed the way the shader reads them.
pub fn defaults(header: &Header) -> [f32; MAX_PARAM_FLOATS] {
    let mut out = [0.0; MAX_PARAM_FLOATS];
    let mut at = 0;
    for p in &header.params {
        match p.kind {
            ParamKind::Number { default, .. } => out[at] = default,
            ParamKind::Toggle { default } => out[at] = default as u8 as f32,
            ParamKind::Choice { default, .. } => out[at] = default as f32,
            ParamKind::Color { default } => out[at..at + 3].copy_from_slice(&default),
        }
        at += p.floats();
    }
    out
}

/// The blend modes' maths for an effect's Blend, every name starting `bpe_` so an effect's own
/// names can't clash with them.
fn blend_maths() -> &'static str {
    static MADE: OnceLock<String> = OnceLock::new();
    MADE.get_or_init(|| {
        let mut text = include_str!("../shaders/blend_maths.wgsl").to_owned();
        for name in top_level_names(&text) {
            let bodies = scan(&text).struct_bodies;
            text = rename_word(&text, &name, &format!("bpe_{name}"), &bodies);
        }
        text
    })
}

/// The whole shader: the prelude, a `Params` struct, the effect file and the entry point. An
/// effect that reads its earlier frames writes two pictures: the picture, and its memory (what
/// `prev` and `kept` read next frame).
pub fn assemble(wgsl: &str) -> Result<String, String> {
    let header = parse(wgsl)?;
    let memory = needs_history(wgsl);
    let mut head = String::from(PRELUDE);
    head.push('\n');
    head.push_str(blend_maths());
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
    tail.push_str("    return p;\n}\n");
    tail.push_str(if memory {
        "
struct FsOut {
    @location(0) picture: vec4<f32>,
    @location(1) memory: vec4<f32>,
};

@fragment
fn fs_main(in: FsIn) -> FsOut {
    let both = bpe_run(in);
    // What the next frame's prev() and kept() read: the picture itself, or what the effect
    // took, and what it kept (on the main picture).
    var memory = both[0];
    if fx.takes.x > 0.5 {
        memory = both[1];
    }
    if fx.in_layer < 0.5 && bpe_keep >= 0.0 {
        memory.a = bpe_keep;
    }
    return FsOut(both[0], memory);
}
"
    } else {
        "
@fragment
fn fs_main(in: FsIn) -> @location(0) vec4<f32> {
    return bpe_run(in)[0];
}
"
    });
    tail.push_str(
        "
// The effect at this pixel: the picture out, and what the effect made of what it took.
fn bpe_run(in: FsIn) -> array<vec4<f32>, 2> {
    // Effects on things leave every other pixel exactly as it was.
    let here = textureLoad(src_tex, vec2<i32>(in.pos.xy), 0);
    var base = sample(in.uv);
    if fx.apply_to > 0.5 {
        base = bpe_straight(here);
    }
    var result = effect(in.uv, unpack());
    // A NaN leaves the pixel as it was instead of spreading, and feedback can't run away.
    result = select(result, base, result != result);
    let code = u32(fx.takes.z);
    if code != 0u {
        result = bpe_blended(code, base, result);
    }
    var blended = mix(base, result, fx.blend * target_weight(in.uv));
    blended = select(blended, base, blended != blended);
    let made = vec4<f32>(clamp(blended, vec3<f32>(-64.0), vec3<f32>(64.0)), 1.0);
    // An effect on the light or the glow puts what it made back into the picture.
    if fx.takes.x > 1.5 {
        let p = here.rgb;
        let glow = max(p - vec3<f32>(fx.takes.y), vec3<f32>(0.0));
        let out = p - glow + max(blended, vec3<f32>(0.0));
        return array<vec4<f32>, 2>(vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(64.0)), here.a), made);
    }
    if fx.takes.x > 0.5 {
        let ip = vec2<i32>(in.pos.xy);
        let p = here.rgb;
        let unlit = textureLoad(unlit_tex, ip, 0).rgb;
        let share = bpe_share(unlit, textureLoad(lit_tex, ip, 0).rgb);
        let light = bpe_light_of(p, unlit, textureLoad(lit_tex, ip, 0).rgb);
        let out = p + (blended - light) * share;
        return array<vec4<f32>, 2>(vec4<f32>(clamp(out, vec3<f32>(0.0), vec3<f32>(64.0)), here.a), made);
    }
    // A blend layer keeps how much it covers, and the effect's colour covers that much too.
    var cover = 1.0;
    if fx.in_layer > 0.5 {
        cover = here.a;
        blended = blended * bpe_cover(here);
    }
    let out = vec4<f32>(clamp(blended, vec3<f32>(0.0), vec3<f32>(64.0)), cover);
    return array<vec4<f32>, 2>(out, out);
}
",
    );
    Ok(format!("{head}{wgsl}\n{tail}"))
}

pub const IN_BALL: &str = include_str!("in_ball.wgsl");
/// What effects call for a Shader layer inside a shape; follows [`IN_BALL`].
pub const IN_SHAPE: &str = include_str!("in_shape.wgsl");

/// Names the in-ball prelude gives effects, which `prefixed` leaves as they are.
const PRELUDE_NAMES: &[&str] = &[
    "sample",
    "prev",
    "aspect",
    "to_centered",
    "from_centered",
    "fold_uv",
    "hash21",
    "noise",
    "fbm",
    "luma",
    "hue_shift",
    "hsv_to_rgb",
    "hit_uv",
    "hit_age",
    "hit_strength",
    "hit_color",
    "bass",
    "mid",
    "treble",
    "object_at",
    "object_code",
    "object_seed",
    "target_weight",
    "fx",
    "Fx",
    "PI",
    "TAU",
    "MAX_HITS",
    "rotate",
    "keep",
    "kept",
    "ball_count",
    "ball_uv",
    "ball_radius",
    "ball_seed",
    "ball_color",
    "ball_hit",
    "nearest_ball",
    "beat_phase",
    "bar_phase",
    "kick",
];

/// `wgsl` with its comments (`//` to the end of the line, `/* */`, which nest) turned to spaces,
/// every other byte where it was.
fn without_comments(wgsl: &str) -> String {
    let b = wgsl.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < b.len() {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    out[i..i + 2].fill(b' ');
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    out[i..i + 2].fill(b' ');
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if b[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    // Only ASCII bytes were replaced, and only whole comments, so this stays UTF-8.
    String::from_utf8(out).unwrap_or_else(|_| wgsl.to_owned())
}

/// What [`scan`] finds in an effect source: the names it declares at the top level, and the
/// byte ranges of its struct bodies (inside the braces).
struct Scanned {
    names: Vec<String>,
    struct_bodies: Vec<std::ops::Range<usize>>,
}

/// Reads `wgsl`'s top level: each declaration (`fn`, `const`, `struct`, `var`, `alias`,
/// `override`) outside every brace, however it is indented or whatever attributes come before
/// it, and the bodies of its structs.
fn scan(wgsl: &str) -> Scanned {
    let code = without_comments(wgsl);
    let b = code.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Scanned { names: Vec::new(), struct_bodies: Vec::new() };
    let mut depth = 0usize;
    // The keyword whose name comes next, and the struct whose body is next.
    let mut naming = false;
    let mut in_struct = false;
    let mut body_start = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'{' {
            if depth == 0 && in_struct {
                body_start = Some(i + 1);
                in_struct = false;
            }
            depth += 1;
        } else if c == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0
                && let Some(start) = body_start.take()
            {
                out.struct_bodies.push(start..i);
            }
        } else if c == b'<' && naming {
            // `var<private> name`: the address space comes before the name.
            while i < b.len() && b[i] != b'>' {
                i += 1;
            }
        } else if c.is_ascii_digit() {
            // A number, suffix and all (`1e-3`, `2u`).
            while i + 1 < b.len() && (word(b[i + 1]) || b[i + 1] == b'.') {
                i += 1;
            }
        } else if word(c) {
            let start = i;
            while i + 1 < b.len() && word(b[i + 1]) {
                i += 1;
            }
            let w = &code[start..=i];
            let attribute = start > 0 && b[start - 1] == b'@';
            if depth == 0 && !attribute {
                if naming {
                    if !out.names.iter().any(|n| n == w) {
                        out.names.push(w.to_owned());
                    }
                    naming = false;
                } else if matches!(w, "fn" | "const" | "struct" | "var" | "alias" | "override") {
                    naming = true;
                    in_struct = w == "struct";
                }
            }
        }
        i += 1;
    }
    out
}

/// The names an effect source declares at the top level: its functions, constants, structs,
/// variables and aliases, outside comments and every brace.
fn top_level_names(wgsl: &str) -> Vec<String> {
    scan(wgsl).names
}

/// `text` with every whole-word use of `from` as `to`, but not a member: right after a `.` (a
/// field read) or a field declared in one of `struct_bodies` (a name followed by `:`), nor an
/// attribute's name (right after `@`).
fn rename_word(text: &str, from: &str, to: &str, struct_bodies: &[std::ops::Range<usize>]) -> String {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(at) = text[i..].find(from) {
        let start = i + at;
        let end = start + from.len();
        let before = if start > 0 { Some(bytes[start - 1]) } else { None };
        let after = bytes.get(end).copied();
        let field = struct_bodies.iter().any(|r| r.contains(&start))
            && text[end..].trim_start().starts_with(':')
            && !text[end..].trim_start().starts_with("::");
        out.push_str(&text[i..start]);
        if before.is_some_and(|b| word(b) || b == b'.' || b == b'@') || after.is_some_and(word) || field {
            out.push_str(from);
        } else {
            out.push_str(to);
        }
        i = end;
    }
    out.push_str(&text[i..]);
    out
}

/// An effect as it runs inside a ball, as copy `n` of the effects in one program: its `Params`
/// struct, its code, and `unpack(l)` reading its settings from a layer record's `extra`, every
/// name it declares at the top level starting `fx{n}_` (so `effect` is `fx{n}_effect`), the
/// prelude's names left as they are.
pub fn prefixed(wgsl: &str, n: usize) -> Result<String, String> {
    let header = parse(wgsl)?;
    let mut text = String::from("struct Params {\n");
    for p in &header.params {
        let ty = if matches!(p.kind, ParamKind::Color { .. }) { "vec3<f32>" } else { "f32" };
        text.push_str(&format!("    {}: {ty},\n", p.key));
    }
    if header.params.is_empty() {
        text.push_str("    _unused: f32,\n");
    }
    text.push_str("};\n\n");
    text.push_str(wgsl);
    text.push_str("\nfn unpack(l: LayerGpu) -> Params {\n    var p: Params;\n");
    let mut at = 0;
    for p in &header.params {
        let float = |i: usize| format!("l.extra[{}][{}]", i / 4, i % 4);
        if p.floats() == 3 {
            text.push_str(&format!(
                "    p.{} = vec3<f32>({}, {}, {});\n",
                p.key,
                float(at),
                float(at + 1),
                float(at + 2)
            ));
        } else {
            text.push_str(&format!("    p.{} = {};\n", p.key, float(at)));
        }
        at += p.floats();
    }
    text.push_str("    return p;\n}\n");
    for name in top_level_names(&text) {
        if PRELUDE_NAMES.contains(&name.as_str()) {
            return Err(format!("`{name}` is a name effects already have; call it something else"));
        }
        // Each rename moves what follows it: the struct bodies are found again each time.
        let bodies = scan(&text).struct_bodies;
        text = rename_word(&text, &name, &format!("fx{n}_{name}"), &bodies);
    }
    Ok(text)
}

/// The effect code of one in-ball program: each of `sources` once, prefixed by its place in the
/// list, then `run_effect(slot, uv, l)`, which runs effect `slot` on layer record `l`.
pub fn in_ball_effects(sources: &[&str]) -> Result<String, String> {
    let mut out = String::from(IN_BALL);
    out.push_str(IN_SHAPE);
    let mut run =
        String::from("\nfn run_effect(slot: u32, uv: vec2<f32>, l: LayerGpu) -> vec3<f32> {\n    switch slot {\n");
    for (n, source) in sources.iter().enumerate() {
        out.push('\n');
        out.push_str(&prefixed(source, n).map_err(|e| format!("{}: {e}", name_or(source, n)))?);
        run.push_str(&format!(
            "        case {n}u: {{\n            return fx{n}_effect(uv, fx{n}_unpack(l));\n        }}\n"
        ));
    }
    run.push_str("        default: {\n            return sample(uv);\n        }\n    }\n}\n");
    out.push_str(&run);
    Ok(out)
}

/// What to call an effect in an error: its header's name, or its place.
fn name_or(source: &str, n: usize) -> String {
    parse(source).ok().map(|h| h.name).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("effect {}", n + 1))
}

/// An effect that comes with the program.
#[derive(Debug)]
pub struct Builtin {
    /// The file name without `.wgsl`.
    pub id: &'static str,
    pub wgsl: &'static str,
    header: OnceLock<Header>,
}

/// The effects that come with the program: 23 over the picture (the first ten from before the
/// lighting, then the psychedelic ones), then three materials.
pub static BUILTINS: [Builtin; 26] = [
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
    Builtin::new("hue_storm", include_str!("builtin/hue_storm.wgsl")),
    Builtin::new("liquid_light", include_str!("builtin/liquid_light.wgsl")),
    Builtin::new("light_ripples", include_str!("builtin/light_ripples.wgsl")),
    Builtin::new("ball_worlds", include_str!("builtin/ball_worlds.wgsl")),
    Builtin::new("polar_tunnel", include_str!("builtin/polar_tunnel.wgsl")),
    Builtin::new("breathing", include_str!("builtin/breathing.wgsl")),
    Builtin::new("thermal", include_str!("builtin/thermal.wgsl")),
    Builtin::new("op_art", include_str!("builtin/op_art.wgsl")),
    Builtin::new("fractal", include_str!("builtin/fractal.wgsl")),
    Builtin::new("rainbow_echoes", include_str!("builtin/rainbow_echoes.wgsl")),
    Builtin::new("melting", include_str!("builtin/melting.wgsl")),
    Builtin::new("solarize", include_str!("builtin/solarize.wgsl")),
    Builtin::new("gravity_lens", include_str!("builtin/gravity_lens.wgsl")),
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
        assert_eq!(h.params[1].kind, ParamKind::Number { default: 6.0, min: 2.0, max: 24.0, int: true, hard: None });
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
    fn comments_in_the_code_are_not_header_lines() {
        let wgsl = "// name: Rim\n// simple width = 1 [0, 2] How wide\n\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {\n    // color the rim, then\n    // name it\n    // param x\n    return sample(uv) * p.width;\n}\n";
        let header = parse(wgsl).unwrap();
        assert_eq!(header.name, "Rim");
        assert_eq!(header.params.len(), 1);
        validate(&assemble(wgsl).unwrap());
        assert!(!needs_objects(wgsl));
        assert!(needs_objects("fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return object_at(uv).rgb; }"));
    }

    #[test]
    fn bad_headers_are_refused_with_a_reason() {
        assert!(parse("// param blend = 1 [0, 1] x\nfn effect(").unwrap_err().contains("blend"));
        let twice = parse("// param a = 1 [0, 1] x\n// param a = 0.5 [0, 1] y\nfn effect(").unwrap_err();
        assert!(twice.contains("twice"), "{twice}");
        assert!(parse("// param Bad-Key = 1 [0, 1] x\nfn effect(").unwrap_err().contains("Bad-Key"));
        assert!(parse("// param a = 5 [0, 1] hard [0, 2] x\nfn effect(").unwrap_err().contains("outside"));
        // Past the usual span is fine when there is no hard limit.
        assert!(parse("// param a = 5 [0, 1] x\nfn effect(").is_ok());
        assert!(parse("// choice c = d [a, b] x\nfn effect(").unwrap_err().contains("`d`"));
        assert!(parse("// choice c = a [a] x\nfn effect(").unwrap_err().contains("two or more"));
        assert!(parse("// memory: soon\nfn effect(").unwrap_err().contains("soon"));
        assert!(parse("// memory: fade nope\nfn effect(").is_err());
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
        assert_eq!(BUILTINS.len() - things.len(), 23);
        assert_eq!(Builtin::find("echo").map(|b| b.id), Some("echo"));
        assert!(Builtin::find("nope").is_none());
        assert!(needs_history(Builtin::find("echo").unwrap().wgsl));
        assert_eq!(Builtin::find("lava").unwrap().header().floats(), 12);
    }

    #[test]
    fn prefixed_renames_declarations_and_their_uses_but_not_prelude_names_or_longer_words() {
        let src = "// name: Mine\n// simple effect_size = 0.5 [0, 1] Size\nconst spread: f32 = 2.0;\nfn helper(x: f32) -> f32 { return x * spread; }\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { let helper_x = 1.0; return sample(uv) * helper(p.effect_size) * helper_x; }\n";
        let out = prefixed(src, 3).unwrap();
        for want in [
            "fn fx3_effect(",
            "fn fx3_helper(",
            "const fx3_spread",
            "struct fx3_Params",
            "fn fx3_unpack(l: LayerGpu) -> fx3_Params",
            "fx3_helper(p.effect_size)",
            "x * fx3_spread",
            "sample(uv)",
            "let helper_x",
            "p.effect_size = l.extra[0][0];",
        ] {
            assert!(out.contains(want), "{want} in\n{out}");
        }
        assert!(!out.contains("fx3_sample") && !out.contains("fx3_effect_size"), "{out}");
        let clash = "fn sample(uv: vec2<f32>) -> vec3<f32> { return vec3<f32>(0.0); }\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return sample(uv); }\n";
        assert!(prefixed(clash, 0).unwrap_err().contains("sample"));
    }

    #[test]
    fn prefixed_leaves_struct_fields_and_member_reads_alone_and_finds_every_top_level_name() {
        // A setting named like a constant, and a struct of its own with a field named like one.
        let same = "// name: Same\n// simple spread = 0.5 [0, 1] Spread\nconst spread: f32 = 2.0;\nstruct Pair { spread: f32, other: Pair2 }\nstruct Pair2 { x: f32 }\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { let q = Pair(p.spread * spread, Pair2(1.0)); return sample(uv) * q.spread * q.other.x; }\n";
        validate(&in_ball_module(&in_ball_effects(&[same]).unwrap()));
        let out = prefixed(same, 2).unwrap();
        for want in ["const fx2_spread", "p.spread * fx2_spread", "struct fx2_Pair { spread: f32, other: fx2_Pair2 }"] {
            assert!(out.contains(want), "{want} in\n{out}");
        }
        // Declarations behind an attribute or indented are top-level too: two effects with the
        // same helpers run side by side.
        let a = "// name: A\n@must_use fn helper(x: f32) -> f32 { return x; }\n  const lift: f32 = 0.5;\n  var<private> seen: f32;\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { seen = 1.0; return sample(uv) * helper(lift) * seen; }\n";
        let b = "// name: B\n@must_use\nfn helper(x: f32) -> f32 { return 2.0 * x; }\n  const lift: f32 = 0.25;\n  var<private> seen: f32;\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { seen = 2.0; return sample(uv) * helper(lift) * seen; }\n";
        validate(&in_ball_module(&in_ball_effects(&[a, b]).unwrap()));
        let names = top_level_names(a);
        assert_eq!(names, ["helper", "lift", "seen", "effect"]);
        // Locals inside a function are not top-level.
        assert!(!top_level_names("fn f() {\n    var x = 1.0;\nconst y = 2.0;\n}\n").contains(&"y".to_owned()));
        for key in ["effect", "unpack"] {
            let src = format!(
                "// name: K\n// simple {key} = 0.5 [0, 1] K\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {{ return sample(uv); }}\n"
            );
            assert!(parse(&src).unwrap_err().contains(key), "{key} is refused as a setting key");
        }
    }

    #[test]
    fn hard_limits_choices_and_memory_are_read() {
        let src = "// name: T\n// simple n = 3 [1, 6] hard [0, 16] int Count\n// choice look = grid [slices, rings, grid] Fold\n// param keep = 0.9 [0, 1] Keep\n// memory: fade keep\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return prev(uv) * p.keep * p.n * p.look; }\n";
        let h = parse(src).unwrap();
        assert_eq!(
            h.params[0].kind,
            ParamKind::Number { default: 3.0, min: 1.0, max: 6.0, int: true, hard: Some((0.0, 16.0)) }
        );
        assert_eq!(h.params[1].kind, ParamKind::Choice { default: 2, names: &["slices", "rings", "grid"] });
        assert_eq!(h.memory, Memory::Fade("keep".to_owned()));
        let mut packed = defaults(&h);
        assert_eq!(&packed[..3], &[3.0, 2.0, 0.9]);
        // 0.9 a frame falls under 1/4096 after about 79 frames.
        let s = memory_seconds(&h, src, &packed);
        assert!((s - 78.95 / 60.0).abs() < 0.01, "{s}");
        packed[2] = 1.0;
        assert_eq!(memory_seconds(&h, src, &packed), MAX_MEMORY);
        packed[2] = 0.0;
        assert_eq!(memory_seconds(&h, src, &packed), 0.0);
        let unsaid = "// name: U\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return prev(uv); }\n";
        assert_eq!(memory_seconds(&parse(unsaid).unwrap(), unsaid, &packed), DEFAULT_MEMORY);
        let none = "// name: N\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return sample(uv); }\n";
        assert_eq!(memory_seconds(&parse(none).unwrap(), none, &packed), 0.0);
        let said = "// name: S\n// memory: 30\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return prev(uv); }\n";
        assert_eq!(memory_seconds(&parse(said).unwrap(), said, &packed), MAX_MEMORY);
        validate(&assemble(src).unwrap());
    }

    #[test]
    fn an_effect_that_keeps_values_writes_a_second_picture() {
        let thermal = Builtin::find("thermal").unwrap().wgsl;
        assert!(needs_history(thermal));
        let full = assemble(thermal).unwrap();
        assert!(full.contains("@location(1)"), "a memory picture");
        validate(&full);
        let plain = assemble(Builtin::find("film").unwrap().wgsl).unwrap();
        assert!(!plain.contains("@location(1)"));
    }

    /// The scene shader with these in-ball effects.
    fn in_ball_module(effects: &str) -> String {
        crate::scene_source(Some(effects))
    }

    #[test]
    fn every_builtin_validates_inside_a_ball_alone_and_three_at_once() {
        for b in &BUILTINS {
            let code = in_ball_effects(&[b.wgsl]).unwrap_or_else(|e| panic!("{}: {e}", b.id));
            validate(&in_ball_module(&code));
        }
        let three: Vec<&str> =
            ["lava", "kaleidoscope", "glitch"].iter().map(|id| Builtin::find(id).unwrap().wgsl).collect();
        validate(&in_ball_module(&in_ball_effects(&three).unwrap()));
        let twice = Builtin::find("lava").unwrap().wgsl;
        validate(&in_ball_module(&in_ball_effects(&[twice, twice]).unwrap()));
    }

    #[test]
    fn an_effect_with_no_params_still_assembles() {
        let full = assemble("// name: Plain\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return sample(uv); }")
            .unwrap();
        validate(&full);
        assert!(assemble("fn main() {}").is_err());
    }
}
