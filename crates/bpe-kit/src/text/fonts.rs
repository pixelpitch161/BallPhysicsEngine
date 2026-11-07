//! The fonts a Text can be set in: the 52 built in (files in a `fonts` folder beside the exe), the
//! ones a scene keeps inside itself (Load font), and IBM Plex Sans SemiBold, built into the program,
//! for when a font can't be found.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use bpe_core::SceneFont;

/// The headings the font picker lists the built-in fonts under, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontGroup {
    BigAndBold,
    RoundAndFriendly,
    Clean,
    Elegant,
    TechAndRetro,
    HandDrawn,
    Wild,
}

impl FontGroup {
    pub const ALL: [FontGroup; 7] = [
        FontGroup::BigAndBold,
        FontGroup::RoundAndFriendly,
        FontGroup::Clean,
        FontGroup::Elegant,
        FontGroup::TechAndRetro,
        FontGroup::HandDrawn,
        FontGroup::Wild,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FontGroup::BigAndBold => "Big and bold",
            FontGroup::RoundAndFriendly => "Round and friendly",
            FontGroup::Clean => "Clean",
            FontGroup::Elegant => "Elegant",
            FontGroup::TechAndRetro => "Tech and retro",
            FontGroup::HandDrawn => "Hand drawn",
            FontGroup::Wild => "Wild",
        }
    }
}

/// A built-in font: its name, its group, its file (weight → file name; several for a family that
/// comes as one file a weight) and the weights it has (a variable font's axis, or the span of its
/// files), `None` for a font with one weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltIn {
    pub name: &'static str,
    pub group: FontGroup,
    pub files: &'static [(u16, &'static str)],
    pub weights: Option<(u16, u16)>,
}

/// The font a new Text is set in.
pub const DEFAULT_FONT: &str = "Anton";

/// The name of the font built into the program, used when another can't be found.
pub const FALLBACK_NAME: &str = "IBM Plex Sans";

macro_rules! single {
    ($name:literal, $group:ident, $file:literal) => {
        BuiltIn { name: $name, group: FontGroup::$group, files: &[(400, $file)], weights: None }
    };
}

macro_rules! variable {
    ($name:literal, $group:ident, $file:literal, $lo:literal, $hi:literal) => {
        BuiltIn { name: $name, group: FontGroup::$group, files: &[(400, $file)], weights: Some(($lo, $hi)) }
    };
}

/// Every built-in font, group by group in the picker's order.
pub const BUILT_IN: [BuiltIn; 52] = [
    // Big and bold
    single!("Anton", BigAndBold, "Anton.ttf"),
    single!("Bebas Neue", BigAndBold, "BebasNeue.ttf"),
    variable!("Oswald", BigAndBold, "Oswald.ttf", 200, 700),
    single!("Archivo Black", BigAndBold, "ArchivoBlack.ttf"),
    single!("Bowlby One", BigAndBold, "BowlbyOne.ttf"),
    single!("Russo One", BigAndBold, "RussoOne.ttf"),
    single!("Black Ops One", BigAndBold, "BlackOpsOne.ttf"),
    single!("Bungee", BigAndBold, "Bungee.ttf"),
    single!("Sigmar", BigAndBold, "Sigmar.ttf"),
    single!("Rubik Mono One", BigAndBold, "RubikMonoOne.ttf"),
    // Round and friendly
    single!("Varela Round", RoundAndFriendly, "VarelaRound.ttf"),
    variable!("Fredoka", RoundAndFriendly, "Fredoka.ttf", 300, 700),
    variable!("Baloo 2", RoundAndFriendly, "Baloo2.ttf", 400, 800),
    variable!("Nunito", RoundAndFriendly, "Nunito.ttf", 200, 1000),
    single!("Lilita One", RoundAndFriendly, "LilitaOne.ttf"),
    single!("Titan One", RoundAndFriendly, "TitanOne.ttf"),
    single!("Chewy", RoundAndFriendly, "Chewy.ttf"),
    single!("Luckiest Guy", RoundAndFriendly, "LuckiestGuy.ttf"),
    // Clean
    variable!("IBM Plex Sans", Clean, "IBMPlexSans.ttf", 100, 700),
    variable!("Inter", Clean, "Inter.ttf", 100, 900),
    variable!("Montserrat", Clean, "Montserrat.ttf", 100, 900),
    BuiltIn {
        name: "Poppins",
        group: FontGroup::Clean,
        files: &[
            (400, "Poppins-400.ttf"),
            (600, "Poppins-600.ttf"),
            (700, "Poppins-700.ttf"),
            (900, "Poppins-900.ttf"),
        ],
        weights: Some((400, 900)),
    },
    variable!("Outfit", Clean, "Outfit.ttf", 100, 900),
    variable!("Space Grotesk", Clean, "SpaceGrotesk.ttf", 300, 700),
    // Elegant
    variable!("Playfair Display", Elegant, "PlayfairDisplay.ttf", 400, 900),
    single!("DM Serif Display", Elegant, "DMSerifDisplay.ttf"),
    single!("Abril Fatface", Elegant, "AbrilFatface.ttf"),
    variable!("Cinzel", Elegant, "Cinzel.ttf", 400, 900),
    single!("Shrikhand", Elegant, "Shrikhand.ttf"),
    // Tech and retro
    single!("Space Mono", TechAndRetro, "SpaceMono.ttf"),
    variable!("JetBrains Mono", TechAndRetro, "JetBrainsMono.ttf", 100, 800),
    variable!("Orbitron", TechAndRetro, "Orbitron.ttf", 400, 900),
    single!("Audiowide", TechAndRetro, "Audiowide.ttf"),
    single!("Press Start 2P", TechAndRetro, "PressStart2P.ttf"),
    single!("VT323", TechAndRetro, "VT323.ttf"),
    single!("Silkscreen", TechAndRetro, "Silkscreen.ttf"),
    single!("Righteous", TechAndRetro, "Righteous.ttf"),
    // Hand drawn
    single!("Permanent Marker", HandDrawn, "PermanentMarker.ttf"),
    variable!("Caveat", HandDrawn, "Caveat.ttf", 400, 700),
    single!("Pacifico", HandDrawn, "Pacifico.ttf"),
    single!("Lobster", HandDrawn, "Lobster.ttf"),
    single!("Satisfy", HandDrawn, "Satisfy.ttf"),
    single!("Bangers", HandDrawn, "Bangers.ttf"),
    // Wild
    single!("Monoton", Wild, "Monoton.ttf"),
    single!("Bungee Shade", Wild, "BungeeShade.ttf"),
    single!("Bungee Outline", Wild, "BungeeOutline.ttf"),
    single!("Rubik Glitch", Wild, "RubikGlitch.ttf"),
    single!("Rubik Wet Paint", Wild, "RubikWetPaint.ttf"),
    single!("Rubik Puddles", Wild, "RubikPuddles.ttf"),
    single!("Nosifer", Wild, "Nosifer.ttf"),
    single!("Creepster", Wild, "Creepster.ttf"),
    single!("Fascinate", Wild, "Fascinate.ttf"),
];

/// The built-in font called `name`, if there is one.
pub fn built_in(name: &str) -> Option<&'static BuiltIn> {
    BUILT_IN.iter().find(|b| b.name == name)
}

impl BuiltIn {
    /// The file of the weight nearest `weight` (a tie goes to the heavier).
    pub fn file_for(&self, weight: f32) -> &'static str {
        let mut best = self.files[0];
        for &f in self.files {
            let (d, b) = ((f.0 as f32 - weight).abs(), (best.0 as f32 - weight).abs());
            if d < b || (d == b && f.0 > best.0) {
                best = f;
            }
        }
        best.1
    }
}

/// A font ready to lay text out in: its name, its file's bytes, a hash of them (what layouts are
/// cached by) and its weight axis when it is a variable font.
#[derive(Debug)]
pub struct FontData {
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub id: u64,
    pub weight_axis: Option<(f32, f32)>,
}

impl FontData {
    /// Reads `bytes` as a font called `name`; `None` when they aren't a font this can use.
    pub fn new(name: &str, bytes: Arc<[u8]>) -> Option<FontData> {
        if !is_font(&bytes) {
            return None;
        }
        let weight_axis = weight_axis_of(&bytes);
        Some(FontData { name: name.to_string(), id: fnv(&bytes), bytes, weight_axis })
    }
}

/// What looking a font up found: the font, or the fallback (and the font that was wanted).
#[derive(Clone, Debug)]
pub enum Found {
    Font(Arc<FontData>),
    /// The font wasn't there: `missing_file` when it is a built-in whose file isn't on this PC
    /// (no fonts folder), else no font has that name (a removed loaded font, a typo).
    Fallback {
        wanted: String,
        font: Arc<FontData>,
        missing_file: bool,
    },
}

impl Found {
    pub fn font(&self) -> &Arc<FontData> {
        match self {
            Found::Font(f) | Found::Fallback { font: f, .. } => f,
        }
    }

    /// What to tell the owner about it, in plain words; `None` when the font was found.
    pub fn problem(&self) -> Option<String> {
        match self {
            Found::Font(_) => None,
            Found::Fallback { missing_file: true, .. } => {
                Some("Font file not found: put the fonts folder beside ball-studio.exe".to_string())
            }
            Found::Fallback { wanted, .. } => {
                Some(format!("Font \"{wanted}\" not found; showing {FALLBACK_NAME} instead"))
            }
        }
    }
}

/// The repository's fonts folder (development and tests).
pub fn repo_fonts_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/text"))
}

/// Where the built-in fonts' files are: the first of the `BPE_FONTS` environment variable, the
/// `fonts` folder beside the running program and the repository's `assets/fonts/text` that holds
/// `Anton.ttf`. Looked for once.
pub fn fonts_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let env = std::env::var_os("BPE_FONTS").map(PathBuf::from);
        let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("fonts")));
        [env, exe, Some(repo_fonts_dir())].into_iter().flatten().find(|d| d.join("Anton.ttf").is_file())
    })
    .clone()
}

/// IBM Plex Sans SemiBold, built into the program.
pub fn fallback() -> Arc<FontData> {
    static FALLBACK: OnceLock<Arc<FontData>> = OnceLock::new();
    FALLBACK
        .get_or_init(|| {
            let bytes: Arc<[u8]> = crate::things::balls::shapes::FONT.into();
            Arc::new(FontData::new(FALLBACK_NAME, bytes).expect("the built-in font reads"))
        })
        .clone()
}

/// The font `name` at `weight`: a font the scene keeps (first, so a loaded font may share a
/// built-in's name), else a built-in from `dir`, else the fallback.
pub fn find_in(dir: Option<&Path>, scene_fonts: &BTreeMap<String, SceneFont>, name: &str, weight: f32) -> Found {
    if let Some(f) = scene_fonts.get(name)
        && let Some(font) = scene_font(name, &f.bytes)
    {
        return Found::Font(font);
    }
    let fall = |missing_file| Found::Fallback { wanted: name.to_string(), font: fallback(), missing_file };
    let Some(b) = built_in(name) else { return fall(false) };
    let Some(dir) = dir else { return fall(true) };
    match file_font(b.name, &dir.join(b.file_for(weight))) {
        Some(font) => Found::Font(font),
        None => fall(true),
    }
}

/// [`find_in`] the fonts folder [`fonts_dir`] finds.
pub fn find(scene_fonts: &BTreeMap<String, SceneFont>, name: &str, weight: f32) -> Found {
    find_in(fonts_dir().as_deref(), scene_fonts, name, weight)
}

/// What the Font row says about the font `name` without reading it: `None` when it is one the
/// scene keeps (`in_scene`) or a built-in whose file is in the fonts folder, else why the text
/// shows in the fallback.
pub fn problem_of(in_scene: bool, name: &str) -> Option<String> {
    if in_scene {
        return None;
    }
    let missing_file = match built_in(name) {
        Some(b) => !fonts_dir().is_some_and(|d| d.join(b.file_for(400.0)).is_file()),
        None => false,
    };
    if built_in(name).is_some() && !missing_file {
        return None;
    }
    Found::Fallback { wanted: name.to_string(), font: fallback(), missing_file }.problem()
}

/// A built-in font's file, read once for the process.
fn file_font(name: &str, path: &Path) -> Option<Arc<FontData>> {
    static FILES: OnceLock<Mutex<HashMap<PathBuf, Option<Arc<FontData>>>>> = OnceLock::new();
    let mut files = FILES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    files
        .entry(path.to_path_buf())
        .or_insert_with(|| {
            let bytes: Arc<[u8]> = std::fs::read(path).ok()?.into();
            FontData::new(name, bytes).map(Arc::new)
        })
        .clone()
}

/// A scene's font, read once while the scene holds those bytes (the cache keeps the bytes, so
/// their address names them).
fn scene_font(name: &str, bytes: &Arc<[u8]>) -> Option<Arc<FontData>> {
    type Key = (usize, usize, String);
    static SCENE: OnceLock<Mutex<HashMap<Key, (Arc<[u8]>, Option<Arc<FontData>>)>>> = OnceLock::new();
    let mut fonts = SCENE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if fonts.len() >= 64 {
        fonts.clear();
    }
    let key = (bytes.as_ptr() as usize, bytes.len(), name.to_string());
    fonts.entry(key).or_insert_with(|| (bytes.clone(), FontData::new(name, bytes.clone()).map(Arc::new))).1.clone()
}

/// Whether `bytes` are a font file (TrueType or OpenType) the text engine can read.
pub fn is_font(bytes: &[u8]) -> bool {
    use skrifa::MetadataProvider;
    let Ok(font) = skrifa::FontRef::new(bytes) else { return false };
    rustybuzz::Face::from_slice(bytes, 0).is_some() && font.outline_glyphs().iter().next().is_some()
}

/// The family name a font file gives itself ("Anton"), its typographic one first.
pub fn family_name(bytes: &[u8]) -> Option<String> {
    use skrifa::MetadataProvider;
    use skrifa::string::StringId;
    let font = skrifa::FontRef::new(bytes).ok()?;
    [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME].into_iter().find_map(|id| {
        let name: String = font.localized_strings(id).english_or_first()?.chars().collect();
        let name = name.trim().to_string();
        (!name.is_empty()).then_some(name)
    })
}

/// A variable font's weight axis, lowest to highest.
fn weight_axis_of(bytes: &[u8]) -> Option<(f32, f32)> {
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::new(bytes).ok()?;
    font.axes().iter().find(|a| a.tag() == skrifa::Tag::new(b"wght")).map(|a| (a.min_value(), a.max_value()))
}

/// FNV-1a of `bytes`.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        repo_fonts_dir()
    }

    fn none() -> BTreeMap<String, SceneFont> {
        BTreeMap::new()
    }

    #[test]
    fn every_built_in_font_is_on_disk_and_loads() {
        use skrifa::MetadataProvider;
        use skrifa::instance::{LocationRef, Size};
        let dir = repo();
        assert_eq!(BUILT_IN.len(), 52);
        let mut names: Vec<&str> = BUILT_IN.iter().map(|b| b.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 52, "names are unique");
        for g in FontGroup::ALL {
            assert!(BUILT_IN.iter().any(|b| b.group == g), "{}", g.name());
        }
        // The groups run in the picker's order, as the spec's table has them.
        let order: Vec<usize> =
            BUILT_IN.iter().map(|b| FontGroup::ALL.iter().position(|g| *g == b.group).unwrap()).collect();
        assert!(order.windows(2).all(|w| w[0] <= w[1]), "{order:?}");
        let counts: Vec<usize> =
            FontGroup::ALL.iter().map(|g| BUILT_IN.iter().filter(|b| b.group == *g).count()).collect();
        assert_eq!(counts, [10, 8, 6, 5, 8, 6, 9]);
        for b in &BUILT_IN {
            for &(w, file) in b.files {
                let found = find_in(Some(&dir), &none(), b.name, w as f32);
                let Found::Font(font) = &found else { panic!("{} {file}: {found:?}", b.name) };
                assert_eq!(font.name, b.name);
                let f = skrifa::FontRef::new(&font.bytes).unwrap();
                let m = f.metrics(Size::unscaled(), LocationRef::default());
                assert!(m.units_per_em > 0, "{file}");
            }
        }
    }

    #[test]
    fn variable_fonts_report_their_weight_axis() {
        let dir = repo();
        let inter = find_in(Some(&dir), &none(), "Inter", 400.0);
        assert_eq!(inter.font().weight_axis, Some((100.0, 900.0)));
        assert_eq!(find_in(Some(&dir), &none(), "Anton", 400.0).font().weight_axis, None);
    }

    #[test]
    fn poppins_picks_the_nearest_weight_file() {
        let dir = repo();
        let file = |name: &str| std::fs::read(dir.join(name)).unwrap();
        let at = |w: f32| find_in(Some(&dir), &none(), "Poppins", w).font().bytes.to_vec();
        assert_eq!(at(300.0), file("Poppins-400.ttf"));
        assert_eq!(at(650.0), file("Poppins-700.ttf"), "a tie goes to the heavier");
        assert_eq!(at(620.0), file("Poppins-600.ttf"));
        assert_eq!(at(1000.0), file("Poppins-900.ttf"));
    }

    #[test]
    fn a_missing_folder_falls_back_and_says_so() {
        let found = find_in(None, &none(), "Anton", 400.0);
        assert!(matches!(found, Found::Fallback { .. }));
        assert!(found.problem().unwrap().contains("fonts folder"), "{:?}", found.problem());
        assert_eq!(found.font().name, FALLBACK_NAME);
        let unknown = find_in(Some(&repo()), &none(), "Nope", 400.0);
        assert!(matches!(unknown, Found::Fallback { .. }));
        assert!(unknown.problem().unwrap().contains("not found"), "{:?}", unknown.problem());
        // A folder without the file.
        let empty = std::env::temp_dir();
        assert!(find_in(Some(&empty), &none(), "Bebas Neue", 400.0).problem().unwrap().contains("fonts folder"));
    }

    #[test]
    fn scene_fonts_win_and_are_found_by_name() {
        let bytes: Arc<[u8]> = std::fs::read(repo().join("Anton.ttf")).unwrap().into();
        let mut fonts = none();
        fonts.insert("Mine".to_string(), SceneFont::new(bytes.clone()));
        let found = find_in(None, &fonts, "Mine", 400.0);
        let Found::Font(font) = found else { panic!("{found:?}") };
        assert_eq!(font.name, "Mine");
        assert_eq!(*font.bytes, *bytes);
        // Something that isn't a font, kept in a scene by hand, falls back.
        fonts.insert("Junk".to_string(), SceneFont::new(b"junk".to_vec().into()));
        assert!(matches!(find_in(None, &fonts, "Junk", 400.0), Found::Fallback { .. }));
    }

    #[test]
    fn family_name_and_is_font() {
        let anton = std::fs::read(repo().join("Anton.ttf")).unwrap();
        assert_eq!(family_name(&anton).as_deref(), Some("Anton"));
        assert_eq!(family_name(&std::fs::read(repo().join("Inter.ttf")).unwrap()).as_deref(), Some("Inter"));
        assert!(is_font(&anton));
        assert!(!is_font(b"hello"));
        assert!(!is_font(&[]));
        assert!(!is_font(&anton[..100]), "a cut-off file");
    }

    #[test]
    fn the_font_row_problem() {
        assert_eq!(problem_of(false, "Anton"), None, "the repository's folder has it");
        assert_eq!(problem_of(true, "Nope"), None, "a font the scene keeps");
        assert_eq!(
            problem_of(false, "Nope").as_deref(),
            Some("Font \"Nope\" not found; showing IBM Plex Sans instead")
        );
    }
}
