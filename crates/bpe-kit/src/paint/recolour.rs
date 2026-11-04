//! The colour sub-layers' maths on the CPU, for what is drawn from the CPU's colours (trails and
//! particles): the same as sub_layers.wgsl's Recolour, Flash on hit and Colour on hit, so a
//! sub-layer changes a trail's colours the way it changes a ball's.

use crate::modules::look::layers::subs::{RecolourMode, SubFlash, SubHue, SubRecolour};

/// Linear light as sRGB-ish, 0 to 1 (`sub_to_srgb`).
fn to_srgb(c: [f32; 3]) -> [f32; 3] {
    c.map(|x| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
    })
}

/// sRGB-ish back to linear light (`sub_to_linear`).
fn to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|x| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    })
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// `c` (linear light) turned `turn` turns round the colour wheel, keeping its saturation and
/// value; light above 1 keeps its extra (`hue_turn`).
pub fn hue_turn(c: [f32; 3], turn: f32) -> [f32; 3] {
    let over = c[0].max(c[1]).max(c[2]).max(1.0);
    let s = to_srgb(c.map(|x| x / over));
    let mx = s[0].max(s[1]).max(s[2]);
    let delta = mx - s[0].min(s[1]).min(s[2]);
    if delta <= 0.0 {
        return c;
    }
    let h = if mx == s[0] {
        ((s[1] - s[2]) / delta + 6.0) % 6.0 / 6.0
    } else if mx == s[1] {
        ((s[2] - s[0]) / delta + 2.0) / 6.0
    } else {
        ((s[0] - s[1]) / delta + 4.0) / 6.0
    };
    let h = (h + turn).rem_euclid(1.0);
    let sat = delta / mx;
    let k = [1.0, 2.0 / 3.0, 1.0 / 3.0];
    let rgb: [f32; 3] = std::array::from_fn(|i| {
        let q = (((h + k[i]).fract()) * 6.0 - 3.0).abs();
        mx * (1.0 + ((q - 1.0).clamp(0.0, 1.0) - 1.0) * sat)
    });
    to_linear(rgb).map(|x| x * over)
}

/// How light `c` (linear light) looks, 0 to 1.
fn lightness(c: [f32; 3]) -> f32 {
    (c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722).clamp(0.0, 1.0).powf(1.0 / 2.2)
}

/// `c` (linear light) through a Recolour sub-layer, mixed in by its opacity.
pub fn recolour(r: &SubRecolour, c: [f32; 3]) -> [f32; 3] {
    let changed = match r.mode {
        RecolourMode::Tint => {
            let t = r.tint.to_linear();
            mix3(c, [t[0], t[1], t[2]], r.amount.clamp(0.0, 1.0))
        }
        RecolourMode::Hue => hue_turn(c, r.turn / 360.0),
        RecolourMode::Gradient => {
            let m = r.colours.sample(lightness(c));
            mix3(c, [m[0], m[1], m[2]], r.amount.clamp(0.0, 1.0))
        }
        RecolourMode::Light => {
            let s = to_srgb(c);
            let x = s.map(|v| (v - 0.5) * r.contrast.max(0.0) + 0.5 + r.brightness);
            let grey = x[0] * 0.2126 + x[1] * 0.7152 + x[2] * 0.0722;
            to_linear(mix3([grey; 3], x, r.saturation.max(0.0)))
        }
    };
    mix3(c, changed, r.opacity.clamp(0.0, 1.0))
}

/// `c` lit by a Flash on hit sub-layer, `age` seconds after a hit of `strength`.
pub fn flash(f: &SubFlash, c: [f32; 3], age: f32, strength: f32) -> [f32; 3] {
    let impact = if f.follow_impact { strength.max(0.0) } else { 1.0 };
    let level = f.strength.max(0.0) * (-3.0 * age.max(0.0) / f.length.max(1e-3)).exp() * impact;
    let k = f.colour.to_linear();
    [c[0] + k[0] * level, c[1] + k[1] * level, c[2] + k[2] * level]
}

/// `c` turned by a Colour on hit sub-layer after `hits` hits.
pub fn hue_on_hit(h: &SubHue, c: [f32; 3], hits: u32) -> [f32; 3] {
    let turn = h.turn_per_hit / 360.0 * hits as f32;
    if turn == 0.0 { c } else { hue_turn(c, turn) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Color;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-3)
    }

    fn lin(c: Color) -> [f32; 3] {
        let l = c.to_linear();
        [l[0], l[1], l[2]]
    }

    #[test]
    fn hue_turns_as_the_colour_wheel_does() {
        let red = lin(Color::rgb(255, 0, 0));
        assert!(close(hue_turn(red, 1.0 / 3.0), lin(Color::rgb(0, 255, 0))));
        assert!(close(hue_turn(red, -1.0 / 3.0), lin(Color::rgb(0, 0, 255))));
        // The same as Color::shift_hue for an ordinary colour.
        let pink = Color::rgb(0xff, 0x5c, 0xa8);
        // (Color rounds to whole steps.)
        let (a, b) = (hue_turn(lin(pink), 0.25), lin(pink.shift_hue(0.25)));
        assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-2), "{a:?} {b:?}");
        // Grey stays grey and light above 1 keeps its extra.
        assert_eq!(hue_turn([0.3; 3], 0.5), [0.3; 3]);
        let bright = hue_turn([2.0, 0.0, 0.0], 1.0 / 3.0);
        assert!(close(bright, [0.0, 2.0, 0.0]), "{bright:?}");
    }

    #[test]
    fn recolour_tints_turns_maps_and_relights() {
        let red = lin(Color::rgb(255, 0, 0));
        let blue = Color::rgb(0, 0, 255);
        let tint = SubRecolour { tint: blue, amount: 1.0, ..SubRecolour::default() };
        assert!(close(recolour(&tint, red), lin(blue)));
        let half = SubRecolour { opacity: 0.5, ..tint.clone() };
        assert!(close(recolour(&half, red), [0.5, 0.0, 0.5]));
        let hue = SubRecolour { mode: RecolourMode::Hue, turn: 120.0, ..SubRecolour::default() };
        assert!(close(recolour(&hue, red), lin(Color::rgb(0, 255, 0))));
        let map = SubRecolour {
            mode: RecolourMode::Gradient,
            colours: bpe_core::Gradient::even(&[Color::rgb(0, 0, 0), Color::rgb(0, 255, 0)]),
            amount: 1.0,
            ..SubRecolour::default()
        };
        assert!(close(recolour(&map, [1.0; 3]), [0.0, 1.0, 0.0]), "white maps to the lightest");
        assert!(close(recolour(&map, [0.0; 3]), [0.0; 3]), "black to the darkest");
        let grey = SubRecolour { mode: RecolourMode::Light, saturation: 0.0, ..SubRecolour::default() };
        let g = recolour(&grey, red);
        assert!((g[0] - g[1]).abs() < 1e-4 && (g[1] - g[2]).abs() < 1e-4 && g[0] > 0.0, "{g:?}");
        let darker = SubRecolour { mode: RecolourMode::Light, brightness: -1.0, ..SubRecolour::default() };
        assert!(close(recolour(&darker, red), [0.0; 3]));
    }

    #[test]
    fn flashes_fade_and_hits_turn() {
        let f = SubFlash { strength: 1.0, length: 0.5, colour: Color::WHITE, follow_impact: true };
        assert!(close(flash(&f, [0.2; 3], 0.0, 1.0), [1.2; 3]));
        assert!(close(flash(&f, [0.2; 3], 0.0, 0.5), [0.7; 3]), "a soft hit flashes less");
        assert!(flash(&f, [0.2; 3], 1.0, 1.0)[0] < 0.25, "gone after a while");
        let h = SubHue { turn_per_hit: 60.0 };
        let red = lin(Color::rgb(255, 0, 0));
        assert_eq!(hue_on_hit(&h, red, 0), red);
        assert!(close(hue_on_hit(&h, red, 2), lin(Color::rgb(0, 255, 0))));
    }
}
