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
