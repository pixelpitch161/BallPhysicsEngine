//! Colours that go with a colour: the new stops for "add matching colours".

use bpe_core::{Color, Gradient, Stop};

/// A way of choosing colours that go with a base colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harmony {
    Complementary,
    Triad,
    Analogous,
    SplitComplementary,
    Shades,
    Tints,
}

impl Harmony {
    pub const ALL: [Harmony; 6] = [
        Harmony::Complementary,
        Harmony::Triad,
        Harmony::Analogous,
        Harmony::SplitComplementary,
        Harmony::Shades,
        Harmony::Tints,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Harmony::Complementary => "Complementary",
            Harmony::Triad => "Triad",
            Harmony::Analogous => "Analogous",
            Harmony::SplitComplementary => "Split complementary",
            Harmony::Shades => "Shades",
            Harmony::Tints => "Tints",
        }
    }
}

/// The new colours that go with `base` (not `base` itself). Each keeps the base's alpha, so an
/// additive base gives additive matches.
pub fn matching(base: Color, h: Harmony) -> Vec<Color> {
    let turns = |degrees: &[f32]| degrees.iter().map(|d| base.shift_hue(d / 360.0)).collect();
    let shade = |k: f32| {
        let (hue, s, v) = base.to_hsv();
        Color { a: base.a, ..Color::from_hsv(hue, s, v * k) }
    };
    let tint = |t: f32| {
        let mix = |c: u8| (c as f32 + (255.0 - c as f32) * t).round() as u8;
        Color { r: mix(base.r), g: mix(base.g), b: mix(base.b), a: base.a }
    };
    match h {
        Harmony::Complementary => turns(&[180.0]),
        Harmony::Triad => turns(&[120.0, 240.0]),
        Harmony::Analogous => turns(&[-30.0, 30.0]),
        Harmony::SplitComplementary => turns(&[150.0, 210.0]),
        Harmony::Shades => vec![shade(0.75), shade(0.5), shade(0.25)],
        Harmony::Tints => vec![tint(0.25), tint(0.5), tint(0.75)],
    }
}

/// The gradient's stops replaced by its first colour and the colours that match it, evenly
/// spaced, with the same transition. Analogous puts the first colour in the middle.
pub fn add_matching(g: &Gradient, h: Harmony) -> Gradient {
    let base = g.first();
    let mut colours = matching(base, h);
    if h == Harmony::Analogous {
        colours.insert(1, base);
    } else {
        colours.insert(0, base);
    }
    let last = (colours.len() - 1) as f32;
    let stops = colours.into_iter().enumerate().map(|(i, color)| Stop { at: i as f32 / last, color }).collect();
    Gradient { stops, transition: g.transition }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Transition;

    const RED: Color = Color::rgb(255, 0, 0);

    #[test]
    fn complementary_is_opposite_hue() {
        let m = matching(RED, Harmony::Complementary);
        assert_eq!(m.len(), 1);
        let hue = m[0].to_hsv().0 * 360.0;
        assert!((hue - 180.0).abs() <= 1.0, "hue {hue}");
    }

    #[test]
    fn analogous_puts_the_base_in_the_middle() {
        let g = add_matching(&Gradient::solid(RED), Harmony::Analogous);
        let at: Vec<f32> = g.stops.iter().map(|s| s.at).collect();
        assert_eq!(at, vec![0.0, 0.5, 1.0]);
        assert_eq!(g.stops[1].color, RED);
        assert_ne!(g.stops[0].color, RED);
        assert_ne!(g.stops[2].color, RED);
    }

    #[test]
    fn shades_get_darker_and_tints_lighter() {
        let base = Color::rgb(200, 100, 50);
        let value = |c: Color| c.to_hsv().2;
        let shades = matching(base, Harmony::Shades);
        assert_eq!(shades.len(), 3);
        let mut last = value(base);
        for s in shades {
            assert!(value(s) < last);
            last = value(s);
        }
        let sum = |c: Color| c.r as u32 + c.g as u32 + c.b as u32;
        let tints = matching(base, Harmony::Tints);
        assert_eq!(tints.len(), 3);
        let mut last = sum(base);
        for t in tints {
            assert!(sum(t) > last);
            last = sum(t);
        }
    }

    #[test]
    fn matching_keeps_alpha_and_additive() {
        for h in Harmony::ALL {
            for m in matching(Color::additive(200, 80, 40), h) {
                assert!(m.is_additive(), "{h:?} {m:?}");
            }
            for m in matching(Color { r: 200, g: 80, b: 40, a: 128 }, h) {
                assert_eq!(m.a, 128, "{h:?}");
            }
        }
    }

    #[test]
    fn add_matching_keeps_the_transition() {
        let mut g = Gradient::even(&[RED, Color::WHITE]);
        g.transition = Transition::Hue;
        for h in Harmony::ALL {
            let out = add_matching(&g, h);
            assert_eq!(out.transition, Transition::Hue);
            assert_eq!(out.stops[0].at, 0.0);
            assert_eq!(out.stops.last().unwrap().at, 1.0);
        }
        assert_eq!(add_matching(&g, Harmony::Triad).stops.len(), 3);
    }
}
