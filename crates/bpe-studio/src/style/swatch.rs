//! The little painted balls on the Surface chips (lighting part 2a): each Surface drawn in greys
//! the way it takes light, lamp up and to the left.

use eframe::egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2, lerp};

/// The ball's shadowed side and its lit side.
const SWATCH_DARK: Color32 = Color32::from_rgb(0x4a, 0x4a, 0x4a);
const SWATCH_LIGHT: Color32 = Color32::from_rgb(0xc8, 0xc8, 0xc8);
/// A mirror's sky above and ground below.
const SWATCH_SKY: Color32 = Color32::from_rgb(0xd8, 0xd8, 0xd8);
const SWATCH_GROUND: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2c);
/// A highlight.
const SWATCH_SHINE: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
/// Glass: dark inside with a light rim.
const SWATCH_GLASS: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
const SWATCH_RIM: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| lerp(x as f32..=y as f32, t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Light falling off across a ball: `steps` discs, each smaller, lighter and nearer the lamp.
fn shaded(painter: &Painter, centre: Pos2, r: f32, from: Color32, to: Color32, steps: usize) {
    let toward = Vec2::new(-0.35, -0.4) * r;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let at = centre + toward * t;
        painter.circle_filled(at, r * (1.0 - 0.8 * t), mix(from, to, (i + 1) as f32 / steps as f32));
    }
}

/// The upper half of a disc, as a polygon.
fn upper_half(centre: Pos2, r: f32, colour: Color32) -> Shape {
    let points = (0..=16)
        .map(|i| {
            let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
            centre + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect();
    Shape::convex_polygon(points, colour, Stroke::NONE)
}

/// Paints Surface number `kind` (Matte, Plastic, Chrome, Metal, Glass, Toon, in the order of
/// `SurfaceKind`) as a ball of radius `r` at `centre`.
pub fn surface_swatch(painter: &Painter, centre: Pos2, r: f32, kind: usize) {
    let shine = |size: f32| painter.circle_filled(centre + Vec2::new(-0.38, -0.42) * r, r * size, SWATCH_SHINE);
    match kind {
        // Matte: an even fall of light, no highlight.
        0 => shaded(painter, centre, r, SWATCH_DARK, SWATCH_LIGHT, 8),
        // Plastic: the same with a small white highlight.
        1 => {
            shaded(painter, centre, r, SWATCH_DARK, SWATCH_LIGHT, 8);
            shine(0.16);
        }
        // Chrome: a sharp mirror of sky over ground.
        2 => {
            painter.circle_filled(centre, r, SWATCH_GROUND);
            painter.add(upper_half(centre, r, SWATCH_SKY));
            shine(0.12);
        }
        // Metal: the same mirror, blurred.
        3 => {
            painter.circle_filled(centre, r, mix(SWATCH_GROUND, SWATCH_SKY, 0.25));
            painter.add(upper_half(centre + Vec2::new(0.0, 0.08 * r), r * 0.92, mix(SWATCH_GROUND, SWATCH_SKY, 0.55)));
            painter.add(upper_half(centre + Vec2::new(0.0, -0.08 * r), r * 0.8, mix(SWATCH_GROUND, SWATCH_SKY, 0.8)));
            shine(0.2);
        }
        // Glass: dark inside, a light rim and a sharp highlight.
        4 => {
            painter.circle_filled(centre, r, SWATCH_GLASS);
            painter.circle_stroke(centre, r - 0.75, Stroke::new(1.5, SWATCH_RIM));
            shine(0.1);
        }
        // Toon: two flat bands and a hard highlight.
        _ => {
            shaded(painter, centre, r, SWATCH_DARK, mix(SWATCH_DARK, SWATCH_LIGHT, 0.6), 2);
            shine(0.14);
        }
    }
}
