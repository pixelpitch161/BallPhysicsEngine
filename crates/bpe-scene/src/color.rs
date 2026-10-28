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

    /// Linear-light RGBA, which is what shaders and blending work in.
    pub fn to_linear(self) -> [f32; 4] {
        let channel = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        [channel(self.r), channel(self.g), channel(self.b), self.a as f32 / 255.0]
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
}
