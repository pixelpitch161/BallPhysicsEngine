//! Shader effects: full-screen WGSL passes applied to the picture after the scene is drawn.
//!
//! Each effect is one `.wgsl` file defining `fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>`.
//! Comment lines at the top give its name, a one-line description and its sliders:
//!
//! ```text
//! // name: Kaleidoscope
//! // about: Mirrors the picture into slices around the centre.
//! // param slices = 6 [2, 24] int Number of mirrored slices
//! // param spin = 0.03 [-1, 1] Turns per second
//! // param dots = 0 toggle Draw round dots
//! ```
//!
//! The built-in effects are compiled into the program. Custom effects are `.wgsl` files in the
//! folders given to [`EffectLibrary::set_folders`]; a custom file with the same name as a built-in
//! replaces it. [`EffectLibrary::refresh`] reloads files that changed on disk.

pub mod chain;

pub(crate) use chain::EffectChain;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// Most sliders one effect can have.
pub const MAX_PARAMS: usize = 8;
const PRELUDE: &str = include_str!("prelude.wgsl");

const BUILTINS: &[(&str, &str)] = &[
    ("echo", include_str!("builtin/echo.wgsl")),
    ("kaleidoscope", include_str!("builtin/kaleidoscope.wgsl")),
    ("shockwave", include_str!("builtin/shockwave.wgsl")),
    ("color_split", include_str!("builtin/color_split.wgsl")),
    ("color_cycle", include_str!("builtin/color_cycle.wgsl")),
    ("liquid", include_str!("builtin/liquid.wgsl")),
    ("mirror", include_str!("builtin/mirror.wgsl")),
    ("pixels", include_str!("builtin/pixels.wgsl")),
    ("film", include_str!("builtin/film.wgsl")),
    ("glitch", include_str!("builtin/glitch.wgsl")),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    Slider,
    /// A slider that only takes whole numbers.
    Whole,
    /// On (1) or off (0).
    Toggle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    pub kind: ParamKind,
    pub help: String,
}

/// One effect shader, loaded and checked.
#[derive(Debug)]
pub struct EffectInfo {
    /// What scenes refer to it by: the file name without `.wgsl`.
    pub id: String,
    pub title: String,
    pub about: String,
    pub params: Vec<ParamInfo>,
    /// The file it was loaded from, or `None` for a built-in.
    pub path: Option<PathBuf>,
    /// The effect file as written, before the prelude is added.
    pub user_source: String,
    /// The complete shader, or why it could not be built.
    pub compiled: Result<String, String>,
    /// Whether the shader reads its previous frame, which needs an extra texture.
    pub uses_prev: bool,
}

impl EffectInfo {
    /// Parses and checks an effect file. Errors end up in [`EffectInfo::compiled`], never a panic.
    pub fn parse(id: &str, source: &str, path: Option<PathBuf>) -> Self {
        let mut title = None;
        let mut about = String::new();
        let mut params = Vec::new();
        let mut errors = Vec::new();
        for (index, line) in source.lines().enumerate() {
            let Some(comment) = line.trim().strip_prefix("//") else { continue };
            let comment = comment.trim();
            if let Some(rest) = comment.strip_prefix("name:") {
                title = Some(rest.trim().to_string());
            } else if let Some(rest) = comment.strip_prefix("about:") {
                about = rest.trim().to_string();
            } else if let Some(rest) = comment.strip_prefix("param ") {
                match parse_param(rest) {
                    Ok(param) if params.iter().any(|p: &ParamInfo| p.name == param.name) => {
                        errors.push(format!("line {}: `{}` is declared twice", index + 1, param.name))
                    }
                    Ok(param) => params.push(param),
                    Err(error) => errors.push(format!("line {}: {error}", index + 1)),
                }
            }
        }
        if params.len() > MAX_PARAMS {
            errors.push(format!("an effect can have at most {MAX_PARAMS} params, this one has {}", params.len()));
        }
        let compiled = if errors.is_empty() { assemble(source, &params) } else { Err(errors.join("\n")) };
        Self {
            id: id.to_string(),
            title: title.unwrap_or_else(|| id.replace('_', " ")),
            about,
            params,
            path,
            user_source: source.to_string(),
            compiled,
            uses_prev: source.contains("prev("),
        }
    }

    pub fn is_builtin(&self) -> bool {
        self.path.is_none()
    }

    pub fn error(&self) -> Option<&str> {
        self.compiled.as_ref().err().map(String::as_str)
    }

    /// The slider values to send to the shader: the scene's value where it has one, else the default.
    pub fn values(&self, scene_params: &BTreeMap<String, f32>) -> [f32; MAX_PARAMS] {
        let mut values = [0.0; MAX_PARAMS];
        for (slot, param) in values.iter_mut().zip(&self.params) {
            *slot = scene_params.get(&param.name).copied().filter(|v| v.is_finite()).unwrap_or(param.default);
        }
        values
    }
}

/// `name = default [min, max] [int] help` or `name = default toggle help`.
fn parse_param(text: &str) -> Result<ParamInfo, String> {
    let (name, rest) = text.split_once('=').ok_or("expected `param name = default [min, max] help`")?;
    let name = name.trim();
    let valid = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid || name == "_" {
        return Err(format!("`{name}` is not a valid param name (letters, digits and _ only)"));
    }
    let rest = rest.trim_start();
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let default = rest[..end].parse::<f32>().ok().filter(|v| v.is_finite());
    let default = default.ok_or_else(|| format!("`{}` is not a number", &rest[..end]))?;
    let rest = rest[end..].trim_start();
    let (min, max, kind, help) = if let Some(help) = rest.strip_prefix("toggle") {
        (0.0, 1.0, ParamKind::Toggle, help)
    } else {
        let inner = rest.strip_prefix('[').ok_or("expected a range like [0, 1] or the word toggle")?;
        let (range, after) = inner.split_once(']').ok_or("missing `]`")?;
        let (lo, hi) = range.split_once(',').ok_or("expected a range like [0, 1]")?;
        let number = |s: &str| s.trim().parse::<f32>().map_err(|_| format!("`{}` is not a number", s.trim()));
        let (lo, hi) = (number(lo)?, number(hi)?);
        if !lo.is_finite() || !hi.is_finite() || lo >= hi {
            return Err(format!("the range [{lo}, {hi}] is empty"));
        }
        if !(lo..=hi).contains(&default) {
            return Err(format!("the default {default} is outside the range [{lo}, {hi}]"));
        }
        let after = after.trim_start();
        match after.strip_prefix("int") {
            Some(help) if help.is_empty() || help.starts_with(char::is_whitespace) => (lo, hi, ParamKind::Whole, help),
            _ => (lo, hi, ParamKind::Slider, after),
        }
    };
    Ok(ParamInfo { name: name.to_string(), default, min, max, kind, help: help.trim().to_string() })
}

/// Joins the prelude, a `Params` struct for the sliders, the effect and the entry point, then
/// checks the result with naga so mistakes are reported with the line in the effect file.
fn assemble(source: &str, params: &[ParamInfo]) -> Result<String, String> {
    let mut head = String::from(PRELUDE);
    head.push_str("\nstruct Params {\n");
    for param in params {
        head.push_str(&format!("    {}: f32,\n", param.name));
    }
    if params.is_empty() {
        head.push_str("    unused: f32,\n");
    }
    head.push_str("};\n\n");
    let mut tail = String::from("\n@fragment\nfn fs_main(in: FsIn) -> @location(0) vec4<f32> {\n    var p: Params;\n");
    for (i, param) in params.iter().enumerate() {
        tail.push_str(&format!("    p.{} = fx.params[{}][{}];\n", param.name, i / 4, i % 4));
    }
    tail.push_str(
        "    let result = effect(in.uv, p);
    let blended = mix(sample(in.uv), result, fx.amount);
    // Keep feedback from running away and stop a NaN from spreading.
    let safe = select(blended, vec3<f32>(0.0), blended != blended);
    return vec4<f32>(clamp(safe, vec3<f32>(0.0), vec3<f32>(64.0)), 1.0);
}
",
    );
    let full = format!("{head}{source}\n{tail}");
    let first_line = head.lines().count() + 1;
    let user_lines = source.lines().count();
    let locate = |location: Option<naga::SourceLocation>| match location {
        Some(at) if (at.line_number as usize) >= first_line && (at.line_number as usize) < first_line + user_lines => {
            format!("line {}: ", at.line_number as usize - first_line + 1)
        }
        _ => String::new(),
    };
    let module = naga::front::wgsl::parse_str(&full).map_err(|e| {
        let mut message = format!("{}{}", locate(e.location(&full)), e.message());
        if !source.contains("fn effect(") {
            message.push_str("\n(the file must define fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>)");
        }
        message
    })?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| {
            let mut message = format!("{}{}", locate(e.location(&full)), e.as_inner());
            let mut source = std::error::Error::source(e.as_inner());
            while let Some(inner) = source {
                message.push_str(&format!(": {inner}"));
                source = inner.source();
            }
            message
        })?;
    Ok(full)
}

/// All the effects a scene can use: the built-ins plus custom files from the effect folders.
/// Cloning gives another handle to the same library.
#[derive(Clone)]
pub struct EffectLibrary {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    builtins: BTreeMap<String, Arc<EffectInfo>>,
    custom: BTreeMap<String, (Arc<EffectInfo>, Option<SystemTime>)>,
    folders: Vec<PathBuf>,
}

impl std::fmt::Debug for EffectLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectLibrary").field("folders", &self.folders()).finish_non_exhaustive()
    }
}

impl Default for EffectLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectLibrary {
    /// A library holding only the built-in effects.
    pub fn new() -> Self {
        let builtins = BUILTINS
            .iter()
            .map(|(id, source)| (id.to_string(), Arc::new(EffectInfo::parse(id, source, None))))
            .collect();
        Self { inner: Arc::new(RwLock::new(Inner { builtins, custom: BTreeMap::new(), folders: Vec::new() })) }
    }

    /// Sets the folders searched for custom `.wgsl` effects (earlier folders win) and loads them.
    pub fn set_folders(&self, folders: Vec<PathBuf>) {
        let changed = {
            let mut inner = self.inner.write().unwrap();
            let changed = inner.folders != folders;
            inner.folders = folders;
            changed
        };
        if changed {
            self.inner.write().unwrap().custom.clear();
        }
        self.refresh();
    }

    pub fn folders(&self) -> Vec<PathBuf> {
        self.inner.read().unwrap().folders.clone()
    }

    /// Reloads custom effects whose files were added, changed or removed. Returns whether anything changed.
    pub fn refresh(&self) -> bool {
        let folders = self.folders();
        let mut found: BTreeMap<String, (PathBuf, Option<SystemTime>)> = BTreeMap::new();
        for folder in &folders {
            let Ok(entries) = std::fs::read_dir(folder) else { continue };
            let mut files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            files.sort();
            for path in files {
                if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("wgsl")) {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(effect_id) else { continue };
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                found.entry(id).or_insert((path, modified));
            }
        }

        let mut inner = self.inner.write().unwrap();
        let before = inner.custom.len();
        inner.custom.retain(|id, _| found.contains_key(id));
        let mut changed = inner.custom.len() != before;
        for (id, (path, modified)) in found {
            let up_to_date = inner
                .custom
                .get(&id)
                .is_some_and(|(info, time)| info.path.as_deref() == Some(path.as_path()) && *time == modified);
            if up_to_date {
                continue;
            }
            let info = match std::fs::read_to_string(&path) {
                Ok(source) => EffectInfo::parse(&id, &source, Some(path.clone())),
                // Editors sometimes hold the file mid-save; keep the old version and try again next time.
                Err(_) if inner.custom.contains_key(&id) => continue,
                Err(error) => EffectInfo {
                    compiled: Err(format!("could not read {}: {error}", path.display())),
                    ..EffectInfo::parse(&id, "", Some(path.clone()))
                },
            };
            inner.custom.insert(id, (Arc::new(info), modified));
            changed = true;
        }
        changed
    }

    /// Looks up an effect by id. A custom file overrides a built-in of the same name.
    pub fn get(&self, id: &str) -> Option<Arc<EffectInfo>> {
        let inner = self.inner.read().unwrap();
        inner.custom.get(id).map(|(info, _)| info.clone()).or_else(|| inner.builtins.get(id).cloned())
    }

    /// The built-in version of an effect, even when a custom file overrides it.
    pub fn builtin(&self, id: &str) -> Option<Arc<EffectInfo>> {
        self.inner.read().unwrap().builtins.get(id).cloned()
    }

    /// Every available effect: built-ins in their usual order, then custom ones by name.
    pub fn list(&self) -> Vec<Arc<EffectInfo>> {
        let inner = self.inner.read().unwrap();
        let mut list: Vec<_> = BUILTINS
            .iter()
            .map(|(id, _)| {
                inner.custom.get(*id).map(|(info, _)| info.clone()).unwrap_or_else(|| inner.builtins[*id].clone())
            })
            .collect();
        list.extend(
            inner.custom.iter().filter(|(id, _)| !inner.builtins.contains_key(*id)).map(|(_, (info, _))| info.clone()),
        );
        list
    }
}

/// Turns a file name into an effect id: lower case, with spaces and dashes as `_`.
pub fn effect_id(name: &str) -> String {
    name.trim().to_lowercase().replace([' ', '-'], "_")
}

/// The folders to look in for custom effects: `effects` next to the scene file, then next to the program.
pub fn default_folders(scene_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(dir) = scene_dir {
        folders.push(dir.join("effects"));
    }
    if let Some(dir) = std::env::current_exe().ok().as_deref().and_then(Path::parent) {
        let folder = dir.join("effects");
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

/// A starting point for a new custom effect file.
pub const TEMPLATE: &str = "\
// name: My effect
// about: Describe what it does in one line.
// param strength = 0.5 [0, 1] How strong the effect is
// param speed = 1 [0, 5] How fast it moves

// uv is the pixel's position (0,0 top left, 1,1 bottom right). Return its new colour.
// Helpers: sample(uv), prev(uv), to_centered(uv), from_centered(q), rotate(v, angle),
// noise(p), fbm(p), hash21(p), luma(c), hue_shift(c, turns), hsv_to_rgb(h, s, v).
// Live values: fx.time, fx.pulse (jumps on bounces), fx.resolution, and the bounce list
// hit_uv(i), hit_age(i), hit_strength(i), hit_color(i) for i below fx.hit_count.
// Save the file and the preview updates straight away.
fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let q = to_centered(uv);
    let wobble = vec2<f32>(sin(q.y * 20.0 + fx.time * p.speed * 6.0), 0.0) * 0.01 * p.strength;
    return sample(from_centered(q + wobble));
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_compiles() {
        let library = EffectLibrary::new();
        assert_eq!(library.list().len(), BUILTINS.len());
        for info in library.list() {
            assert!(info.compiled.is_ok(), "{}: {}", info.id, info.error().unwrap_or_default());
            assert!(!info.about.is_empty(), "{} has no description", info.id);
            for param in &info.params {
                assert!(param.min <= param.default && param.default <= param.max, "{}.{}", info.id, param.name);
                assert!(!param.help.is_empty(), "{}.{} has no help", info.id, param.name);
            }
        }
    }

    #[test]
    fn template_compiles() {
        let info = EffectInfo::parse("template", TEMPLATE, None);
        assert!(info.compiled.is_ok(), "{:?}", info.compiled.err());
        assert_eq!(info.params.len(), 2);
    }

    #[test]
    fn readme_example_compiles() {
        let readme = include_str!("../../../../README.md");
        let start = readme.find("```wgsl\n").expect("README has a wgsl example") + 8;
        let end = start + readme[start..].find("```").unwrap();
        let info = EffectInfo::parse("wobble", &readme[start..end], None);
        assert!(info.compiled.is_ok(), "{:?}", info.compiled.err());
        assert_eq!(info.params.len(), 3);
    }

    #[test]
    fn params_parse() {
        let p = parse_param("slices = 6 [2, 24] int Number of slices").unwrap();
        assert_eq!((p.name.as_str(), p.default, p.min, p.max, p.kind), ("slices", 6.0, 2.0, 24.0, ParamKind::Whole));
        assert_eq!(p.help, "Number of slices");
        let p = parse_param("dots = 1 toggle Round dots").unwrap();
        assert_eq!((p.kind, p.help.as_str()), (ParamKind::Toggle, "Round dots"));
        let p = parse_param("intensity = 0.5 [0, 1] internal glow").unwrap();
        assert_eq!((p.kind, p.help.as_str()), (ParamKind::Slider, "internal glow"));
        assert!(parse_param("1bad = 0 [0, 1]").is_err());
        assert!(parse_param("x = 0 [1, 0]").is_err());
        assert!(parse_param("x = abc [0, 1]").is_err());
        assert!(parse_param("x = 0").is_err());
        assert!(parse_param("x = 5 [0, 1] too big").is_err());
        assert!(parse_param("x = nan [0, 1]").is_err());
        assert!(parse_param("x = 0 [0, inf]").is_err());
    }

    #[test]
    fn errors_point_at_the_effect_line() {
        let source = "// param a = 1 [0, 2] A\n\nfn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {\n    return sample(uv) * nope;\n}\n";
        let info = EffectInfo::parse("broken", source, None);
        let error = info.error().expect("should fail");
        assert!(error.starts_with("line 4:"), "{error}");
    }

    #[test]
    fn missing_effect_function_is_explained() {
        let info = EffectInfo::parse("empty", "// name: Empty\n", None);
        assert!(info.error().is_some_and(|e| e.contains("fn effect")), "{:?}", info.error());
    }

    #[test]
    fn values_fall_back_to_defaults() {
        let info = EffectInfo::parse("kaleidoscope", BUILTINS[1].1, None);
        let mut params = BTreeMap::new();
        params.insert("spin".to_string(), 0.5);
        params.insert("unknown".to_string(), 9.0);
        params.insert("zoom".to_string(), f32::NAN);
        let values = info.values(&params);
        assert_eq!(&values[..4], &[6.0, 0.5, 1.0, 0.0]);
    }

    #[test]
    fn custom_files_load_reload_and_override() {
        let dir = std::env::temp_dir().join(format!("bpe-effects-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let library = EffectLibrary::new();
        library.set_folders(vec![dir.clone()]);
        assert!(library.get("my_fx").is_none());

        std::fs::write(dir.join("My FX.wgsl"), TEMPLATE).unwrap();
        assert!(library.refresh());
        let first = library.get("my_fx").expect("loaded");
        assert!(first.compiled.is_ok());
        assert!(!library.refresh(), "nothing changed");

        // A changed file is reloaded; make sure the modified time moves on coarse file systems.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("My FX.wgsl"), "fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> { return oops; }")
            .unwrap();
        let file = std::fs::File::options().write(true).open(dir.join("My FX.wgsl")).unwrap();
        file.set_modified(SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
        assert!(library.refresh());
        assert!(library.get("my_fx").unwrap().error().is_some());

        std::fs::write(dir.join("film.wgsl"), TEMPLATE).unwrap();
        library.refresh();
        assert_eq!(library.get("film").unwrap().title, "My effect");
        assert_eq!(library.builtin("film").unwrap().title, "Film");
        assert_eq!(library.list().len(), BUILTINS.len() + 1);

        std::fs::remove_file(dir.join("film.wgsl")).unwrap();
        assert!(library.refresh());
        assert_eq!(library.get("film").unwrap().title, "Film");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
