use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An sRGB colour, written in scene files as a hex string like "#ff8800" or "#ff880080".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => None,
        }
    }

    /// A colour from hue (0 to 1 around the colour wheel), saturation and value (each 0 to 1).
    pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> Self {
        let h = hue.rem_euclid(1.0) * 6.0;
        let (s, v) = (saturation.clamp(0.0, 1.0), value.clamp(0.0, 1.0));
        let f = h - h.floor();
        let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
        let (r, g, b) = match h as u32 {
            0 => (v, t, p),
            1 => (q, v, p),
            2 => (p, v, t),
            3 => (p, q, v),
            4 => (t, p, v),
            _ => (v, p, q),
        };
        let byte = |c: f32| (c * 255.0).round() as u8;
        Self::rgb(byte(r), byte(g), byte(b))
    }

    /// Hue, saturation and value, each 0 to 1.
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let (r, g, b) = (self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0);
        let max = r.max(g).max(b);
        let delta = max - r.min(g).min(b);
        let hue = if delta == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.0) / 6.0
        } else if max == g {
            ((b - r) / delta + 2.0) / 6.0
        } else {
            ((r - g) / delta + 4.0) / 6.0
        };
        (hue, if max == 0.0 { 0.0 } else { delta / max }, max)
    }

    /// The same colour turned around the colour wheel by `turn` (1 is a full turn), keeping its alpha.
    pub fn shift_hue(self, turn: f32) -> Self {
        let (h, s, v) = self.to_hsv();
        Self { a: self.a, ..Self::from_hsv(h + turn, s, v) }
    }

    /// Linear-light RGBA, which is what shaders and blending work in.
    pub fn to_linear(self) -> [f32; 4] {
        let channel = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        [channel(self.r), channel(self.g), channel(self.b), self.a as f32 / 255.0]
    }

    /// The sRGB colour for linear-light RGBA, rounded to the nearest step.
    pub fn from_linear(c: [f32; 4]) -> Self {
        let channel = |c: f32| {
            let c = c.clamp(0.0, 1.0);
            let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0).round() as u8
        };
        Self { r: channel(c[0]), g: channel(c[1]), b: channel(c[2]), a: (c[3].clamp(0.0, 1.0) * 255.0).round() as u8 }
    }

    /// Blends from `a` to `b` in linear light. `t` runs 0 to 1.
    pub fn mix(a: Color, b: Color, t: f32) -> Color {
        if t <= 0.0 {
            return a;
        }
        if t >= 1.0 {
            return b;
        }
        let (a, b) = (a.to_linear(), b.to_linear());
        Self::from_linear(std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t))
    }

    /// Picks a colour along evenly spaced stops, blending in linear light. `t` runs 0 to 1.
    pub fn sample_gradient(stops: &[Color], t: f32) -> [f32; 4] {
        match stops {
            [] => Self::WHITE.to_linear(),
            [only] => only.to_linear(),
            _ => {
                let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
                let i = (x.floor() as usize).min(stops.len() - 2);
                let f = x - i as f32;
                let (a, b) = (stops[i].to_linear(), stops[i + 1].to_linear());
                std::array::from_fn(|k| a[k] + (b[k] - a[k]) * f)
            }
        }
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Color::from_hex(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid colour {text:?}, expected \"#rrggbb\"")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        for text in ["#3e9bff", "#ff880080"] {
            assert_eq!(Color::from_hex(text).unwrap().to_hex(), text);
        }
        assert!(Color::from_hex("#12345").is_none());
    }

    #[test]
    fn hsv_round_trip() {
        for color in [Color::rgb(0x3e, 0x9b, 0xff), Color::rgb(255, 0, 0), Color::rgb(10, 200, 90), Color::WHITE] {
            let (h, s, v) = color.to_hsv();
            assert_eq!(Color::from_hsv(h, s, v), color);
        }
        assert_eq!(Color::rgb(255, 0, 0).shift_hue(1.0 / 3.0), Color::rgb(0, 255, 0));
    }

    #[test]
    fn linear_round_trip() {
        for c in [Color::rgb(0, 0, 0), Color::rgb(1, 128, 254), Color { r: 3, g: 77, b: 200, a: 90 }] {
            assert_eq!(Color::from_linear(c.to_linear()), c);
        }
    }
}
