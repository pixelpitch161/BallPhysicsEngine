//! Named gradients that ship with the editor.

use bpe_core::{Color, Gradient, Transition};
use serde::{Deserialize, Serialize};

/// A named set of colours.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub colours: Gradient,
}

fn hex(codes: &[&str]) -> Vec<Color> {
    codes.iter().map(|c| Color::from_hex(c).expect("built-in palette colour")).collect()
}

/// The palettes that ship with the editor, in menu order.
pub fn built_in() -> Vec<Palette> {
    let table: [(&str, &[&str]); 20] = [
        ("Sunset", &["#2b1055", "#7597de", "#f4a259", "#f25c54", "#ffd6a5"]),
        ("Neon", &["#ff00e5", "#00f0ff", "#39ff14", "#fff200"]),
        ("Pastel", &["#ffb3ba", "#ffdfba", "#ffffba", "#baffc9", "#bae1ff"]),
        ("Ocean", &["#03045e", "#0077b6", "#00b4d8", "#90e0ef", "#caf0f8"]),
        ("Forest", &["#1b4332", "#2d6a4f", "#40916c", "#74c69d", "#b7e4c7"]),
        ("Candy", &["#ff6b9d", "#c44dff", "#4dc9ff", "#ffe14d"]),
        ("Fire", &["#1a0000", "#8b0000", "#ff4500", "#ffa500", "#ffee88"]),
        ("Ice", &["#e0fbfc", "#c2dfe3", "#9db4c0", "#5c6b73"]),
        ("Mono", &["#000000", "#bcbcbc", "#ffffff"]),
        ("Retro", &["#264653", "#2a9d8f", "#e9c46a", "#f4a261", "#e76f51"]),
        ("Vapor", &["#ff71ce", "#b967ff", "#01cdfe", "#05ffa1", "#fffb96"]),
        ("Earth", &["#3d2b1f", "#6f4e37", "#a47148", "#c9ad7f", "#7d8f69"]),
        ("Aurora", &["#0b132b", "#1c7c54", "#3ddc97", "#7b2cbf", "#c77dff"]),
        ("Berry", &["#2d0a31", "#6a0572", "#ab83a1", "#d7263d", "#ff6f91"]),
        ("Citrus", &["#fff176", "#ffb300", "#ff7043", "#9ccc65"]),
        ("Desert", &["#edc9af", "#e2a76f", "#c1440e", "#7a3b2e", "#2f4858"]),
        ("Lagoon", &["#006466", "#0b8a8f", "#17c3b2", "#a2f3e0"]),
        ("Midnight", &["#000814", "#001d3d", "#003566", "#4361ee", "#a9b8ff"]),
        ("Rainbow", &["#ff0000", "#ffff00", "#00ff00", "#00ffff", "#0000ff", "#ff00ff"]),
        ("Gold", &["#5c3d00", "#b8860b", "#ffd700", "#fff2a8"]),
    ];
    table
        .iter()
        .map(|(name, codes)| {
            let mut colours = Gradient::even(&hex(codes));
            if *name == "Rainbow" {
                colours.transition = Transition::Hue;
            }
            Palette { name: name.to_string(), colours }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_twenty_named_palettes() {
        let names: Vec<String> = built_in().into_iter().map(|p| p.name).collect();
        let want = [
            "Sunset", "Neon", "Pastel", "Ocean", "Forest", "Candy", "Fire", "Ice", "Mono", "Retro", "Vapor", "Earth",
            "Aurora", "Berry", "Citrus", "Desert", "Lagoon", "Midnight", "Rainbow", "Gold",
        ];
        assert_eq!(names, want);
        for p in built_in() {
            let n = p.colours.stops.len();
            assert!((3..=6).contains(&n), "{} has {n}", p.name);
            assert_eq!(p.colours.clone().sorted(), p.colours);
            let want = if p.name == "Rainbow" { Transition::Hue } else { Transition::Smooth };
            assert_eq!(p.colours.transition, want);
            assert!(p.colours.stops.iter().all(|s| !s.color.is_additive()));
        }
    }
}
