//! A colour generator: a few colours that go together, for the gradient bar's "Random palette".

use bpe_core::Color;
use bpe_core::dice::Rng;

/// `n` colours that go together. Half the time they are spread along a random built-in palette
/// (one colour takes its middle); otherwise hues are stepped around the wheel (close, spread,
/// opposite or evenly), all bright and fairly saturated.
pub fn palette(dice: &mut Rng, n: usize) -> Vec<Color> {
    if dice.unit() < 0.5 {
        let all = crate::paint::palettes::built_in();
        let pick = &all[dice.next_u32() as usize % all.len()];
        return sample_palette(&pick.colours, n);
    }
    procedural_palette(dice, n)
}

/// `n` colours at evenly spaced points along `g` (one colour takes its middle).
pub(crate) fn sample_palette(g: &bpe_core::Gradient, n: usize) -> Vec<Color> {
    (0..n)
        .map(|i| {
            // A looped gradient's end is its start again, so it spreads `i / n` round it.
            let t = match n {
                1 => 0.5,
                _ if g.looped => i as f32 / n as f32,
                _ => i as f32 / (n - 1) as f32,
            };
            Color::from_linear(g.sample(t))
        })
        .collect()
}

fn procedural_palette(dice: &mut Rng, n: usize) -> Vec<Color> {
    let base = dice.unit();
    let sign = if dice.unit() < 0.5 { -1.0 } else { 1.0 };
    let step = match dice.next_u32() % 4 {
        0 => dice.range(0.04, 0.1),
        1 => dice.range(0.1, 0.2),
        2 => 0.5 / (n.max(2) - 1) as f32,
        _ => dice.range(0.6, 0.9) / n.max(1) as f32,
    } * sign;
    let saturation = dice.range(0.55, 0.9);
    (0..n)
        .map(|i| {
            let s = (saturation + dice.range(-0.08, 0.08)).clamp(0.55, 0.9);
            let v = dice.range(0.85, 1.0);
            Color::from_hsv(base + step * i as f32, s, v)
        })
        .collect()
}

/// A seed nobody has used yet, from the clock. Never 0, so "no seed" stays free to mean that.
pub fn fresh_seed() -> u32 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
    let folded = (nanos as u64 ^ (nanos >> 64) as u64) as u32 ^ (nanos >> 32) as u32;
    folded.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_dice_sometimes_returns_a_built_in_palette() {
        let want: Vec<Vec<Color>> =
            crate::paint::palettes::built_in().iter().map(|p| sample_palette(&p.colours, 3)).collect();
        let hits = (0..200).filter(|&seed| want.contains(&palette(&mut Rng::new(seed, 0), 3))).count();
        assert!(hits > 0 && hits < 200, "{hits}");
        assert_eq!(palette(&mut Rng::new(7, 0), 3), palette(&mut Rng::new(7, 0), 3));
        assert!((0..200).all(|seed| palette(&mut Rng::new(seed, 0), 1).len() == 1));
        assert!((0..200).all(|seed| palette(&mut Rng::new(seed, 0), 4).iter().all(|c| !c.is_additive())));
    }

    #[test]
    fn palette_makes_n_distinct_bright_colours() {
        for seed in 0..20 {
            let colours = procedural_palette(&mut Rng::new(seed, 0), 5);
            assert_eq!(colours.len(), 5);
            for c in &colours {
                let (_, s, v) = c.to_hsv();
                assert!((0.52..=0.93).contains(&s), "saturation {s}");
                assert!((0.82..=1.0).contains(&v), "value {v}");
            }
            let distinct: std::collections::BTreeSet<_> = colours.iter().map(|c| (c.r, c.g, c.b)).collect();
            assert!(distinct.len() >= 2, "{colours:?}");
        }
    }

    #[test]
    fn a_fresh_seed_is_never_zero() {
        assert_ne!(fresh_seed(), 0);
    }
}
