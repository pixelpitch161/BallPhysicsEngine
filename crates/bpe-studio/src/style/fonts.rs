//! IBM Plex Sans and Mono, built into the exe (SIL Open Font License, see assets/fonts/OFL.txt).

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId};
use std::sync::Arc;

const SANS: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf");
const SANS_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf");
const SANS_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf");
const MONO: &[u8] = include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf");

/// The family of the semibold weight, for section headings and the primary button.
pub const SEMIBOLD: &str = "semibold";
/// The family of the medium weight, for the selected tree row.
pub const MEDIUM: &str = "medium";

/// Plex Sans for everything, Plex Mono for numbers; egui's own fonts stay behind them for glyphs
/// Plex lacks.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(base_definitions());
}

/// The editor's own four families over egui's defaults.
fn base_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in
        [("plex-sans", SANS), ("plex-medium", SANS_MEDIUM), ("plex-semibold", SANS_SEMIBOLD), ("plex-mono", MONO)]
    {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |first: &str| std::iter::once(first.to_owned()).chain(fallbacks.iter().cloned()).collect::<Vec<_>>();
    fonts.families.insert(FontFamily::Proportional, with("plex-sans"));
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), with("plex-semibold"));
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), with("plex-medium"));
    let mono_fallbacks = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    fonts
        .families
        .insert(FontFamily::Monospace, std::iter::once("plex-mono".to_owned()).chain(mono_fallbacks).collect());
    fonts
}

/// The egui family a Text font shows in (the font picker draws each name in its own font).
pub fn text_family(name: &str) -> FontFamily {
    FontFamily::Name(format!("text:{name}").into())
}

/// Whether `family` can be drawn with this frame: [`ensure_text_fonts`] installs a font for the
/// next frame, and until then its family isn't there.
pub fn family_ready(ctx: &egui::Context, family: &FontFamily) -> bool {
    ctx.fonts(|f| f.definitions().families.contains_key(family))
}

/// Installs every built-in Text font found on disk and every font the scene keeps as egui
/// families ([`text_family`]), beside the editor's own four: once, and again whenever the scene's
/// fonts change. A file that is missing or can't be read is left out.
pub fn ensure_text_fonts(ctx: &egui::Context, scene: &bpe_core::Scene) {
    use bpe_kit::text::fonts::{BUILT_IN, Found, find};
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (name, font) in &scene.fonts {
        (name, font.bytes.as_ptr() as usize, font.bytes.len()).hash(&mut h);
    }
    let key = h.finish();
    let id = egui::Id::unique("text fonts installed");
    if ctx.data(|d| d.get_temp::<u64>(id)) == Some(key) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(id, key));
    let mut fonts = base_definitions();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let none = Default::default();
    let built_in = BUILT_IN.iter().map(|b| (b.name.to_string(), find(&none, b.name, 400.0)));
    let kept = scene.fonts.keys().map(|name| (name.clone(), find(&scene.fonts, name, 400.0)));
    // The scene's come second, so a loaded font sharing a built-in's name shows as itself.
    for (name, found) in built_in.chain(kept) {
        let Found::Font(font) = found else { continue };
        let data = format!("text:{name}");
        fonts.font_data.insert(data.clone(), Arc::new(FontData::from_owned(font.bytes.to_vec())));
        let family = std::iter::once(data).chain(fallbacks.iter().cloned()).collect();
        fonts.families.insert(text_family(&name), family);
    }
    ctx.set_fonts(fonts);
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
