use crate::Choice;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An sRGB colour, written in scene files as a hex string like "#ff8800" or "#ff880080".
///
/// No alpha with some colour ("#ff880000") is an additive colour: it adds its light to whatever
/// is behind it instead of covering it, like the colour picker's "Additive" blending.
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

    /// An additive colour: it adds its light to what is behind it instead of covering it.
    pub const fn additive(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0 }
    }

    /// Whether this colour adds its light instead of covering what is behind it.
    pub const fn is_additive(self) -> bool {
        self.a == 0 && (self.r > 0 || self.g > 0 || self.b > 0)
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
}

/// One colour sitting at a place along a gradient.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    /// Where along the gradient, 0 to 1.
    pub at: f32,
    pub color: Color,
}

/// How a gradient moves from one stop to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Choice)]
pub enum Transition {
    /// A straight blend in linear light.
    #[default]
    Smooth,
    /// A blend that lingers at each stop and moves quickly between them.
    Eased,
    /// Hard bands: each colour holds until halfway to the next stop.
    Steps,
    /// Goes around the colour wheel the short way, so red to magenta passes through rose.
    Hue,
}

/// Colours at places along 0 to 1, and how they blend. Written in scene files as its stops, or
/// as a single hex string for a solid colour.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Gradient {
    /// In order of `at`. Two stops at the same place make a hard edge there.
    pub stops: Vec<Stop>,
    pub transition: Transition,
}

impl Default for Gradient {
    fn default() -> Self {
        Self::solid(Color::WHITE)
    }
}

impl<'de> Deserialize<'de> for Gradient {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Solid(Color),
            Stops { stops: Vec<Stop>, transition: Transition },
        }
        Ok(match Saved::deserialize(deserializer)? {
            Saved::Solid(c) => Gradient::solid(c),
            Saved::Stops { stops, transition } => Gradient { stops, transition }.sorted(),
        })
    }
}

/// `t` of the way from `a` to `b`, componentwise.
fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

impl Gradient {
    /// One colour everywhere.
    pub fn solid(c: Color) -> Self {
        Self { stops: vec![Stop { at: 0.0, color: c }], transition: Transition::Smooth }
    }

    /// The colours spread evenly from 0 to 1, blending smoothly. No colours is solid white.
    pub fn even(colors: &[Color]) -> Self {
        match colors {
            [] => Self::solid(Color::WHITE),
            [only] => Self::solid(*only),
            _ => {
                let last = (colors.len() - 1) as f32;
                let stops = colors.iter().enumerate().map(|(i, &color)| Stop { at: i as f32 / last, color }).collect();
                Self { stops, transition: Transition::Smooth }
            }
        }
    }

    /// The colour of the first stop.
    pub fn first(&self) -> Color {
        self.stops.first().map_or(Color::WHITE, |s| s.color)
    }

    /// The same gradient with its stops in order and inside 0 to 1. No stops is solid white.
    pub fn sorted(mut self) -> Self {
        if self.stops.is_empty() {
            return Self { transition: self.transition, ..Self::solid(Color::WHITE) };
        }
        for s in &mut self.stops {
            s.at = if s.at.is_finite() { s.at.clamp(0.0, 1.0) } else { 0.0 };
        }
        self.stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        self
    }

    /// The linear-light colour at `t` (0 to 1, clamped). The stops are taken to be in order,
    /// and where two share a place the later one wins from there on.
    pub fn sample(&self, t: f32) -> [f32; 4] {
        let t = t.clamp(0.0, 1.0);
        let Some(i) = self.stops.iter().rposition(|s| s.at <= t) else {
            return self.first().to_linear();
        };
        let Some(next) = self.stops.get(i + 1) else {
            return self.stops[i].color.to_linear();
        };
        let a = self.stops[i];
        let f = (t - a.at) / (next.at - a.at);
        match self.transition {
            Transition::Smooth => lerp4(a.color.to_linear(), next.color.to_linear(), f),
            Transition::Eased => lerp4(a.color.to_linear(), next.color.to_linear(), f * f * (3.0 - 2.0 * f)),
            Transition::Steps => (if f < 0.5 { a.color } else { next.color }).to_linear(),
            Transition::Hue => {
                let ((ha, sa, va), (hb, sb, vb)) = (a.color.to_hsv(), next.color.to_hsv());
                let turn = (hb - ha + 0.5).rem_euclid(1.0) - 0.5;
                let mut c = Color::from_hsv(ha + turn * f, sa + (sb - sa) * f, va + (vb - va) * f).to_linear();
                c[3] = a.color.to_linear()[3] + (next.color.to_linear()[3] - a.color.to_linear()[3]) * f;
                c
            }
        }
    }

    /// `n` evenly spaced samples with the first at 0 and the last at 1, ready for a texture.
    pub fn bake(&self, n: usize) -> Vec<[f32; 4]> {
        let last = n.saturating_sub(1).max(1) as f32;
        (0..n).map(|i| self.sample(i as f32 / last)).collect()
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
    fn additive_colours() {
        let glow = Color::additive(255, 136, 0);
        assert!(glow.is_additive());
        assert_eq!(glow.to_hex(), "#ff880000");
        assert_eq!(Color::from_hex("#ff880000"), Some(glow));
        assert!(!Color { r: 0, g: 0, b: 0, a: 0 }.is_additive());
        assert!(!Color::WHITE.is_additive());
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

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);
    const BLACK: Color = Color::rgb(0, 0, 0);
    const WHITE: Color = Color::WHITE;

    #[test]
    fn one_stop_samples_its_colour_everywhere() {
        let c = Color::rgb(200, 30, 90);
        let g = Gradient::solid(c);
        for t in [-1.0, 0.0, 0.37, 1.0, 2.0] {
            assert_eq!(g.sample(t), c.to_linear());
        }
        let add = Color { r: 255, g: 0, b: 0, a: 0 }; // additive keeps alpha 0
        assert_eq!(Gradient::solid(add).sample(0.5)[3], 0.0);
    }

    #[test]
    fn even_spaces_the_stops_and_blends_in_linear_light() {
        let cs = [Color::rgb(255, 0, 0), Color::rgb(0, 255, 0), Color::rgb(0, 0, 255)];
        let g = Gradient::even(&cs);
        let want = [
            (0.0, [1.0, 0.0, 0.0, 1.0]),
            (0.25, [0.5, 0.5, 0.0, 1.0]),
            (0.5, [0.0, 1.0, 0.0, 1.0]),
            (0.9, [0.0, 0.2, 0.8, 1.0]),
            (1.0, [0.0, 0.0, 1.0, 1.0]),
        ];
        for (t, w) in want {
            let got = g.sample(t);
            for k in 0..4 {
                assert!((got[k] - w[k]).abs() < 1e-5, "t {t}: {got:?} vs {w:?}");
            }
        }
    }

    #[test]
    fn stops_move_where_colours_sit() {
        let g = Gradient {
            stops: vec![Stop { at: 0.0, color: RED }, Stop { at: 0.8, color: BLUE }],
            transition: Transition::Smooth,
        };
        assert_eq!(g.sample(0.8), BLUE.to_linear());
        assert_eq!(g.sample(1.0), BLUE.to_linear());
        assert!((g.sample(0.4)[0] - RED.to_linear()[0] * 0.5).abs() < 1e-5);
    }

    #[test]
    fn steps_split_halfway_between_stops() {
        let g = Gradient {
            stops: vec![Stop { at: 0.0, color: RED }, Stop { at: 0.6, color: BLUE }],
            transition: Transition::Steps,
        };
        assert_eq!(g.sample(0.29), RED.to_linear());
        assert_eq!(g.sample(0.31), BLUE.to_linear());
    }

    #[test]
    fn eased_is_smoothstep_and_hue_goes_the_short_way() {
        let e = Gradient { transition: Transition::Eased, ..Gradient::even(&[BLACK, WHITE]) };
        assert!((e.sample(0.25)[0] - 0.15625).abs() < 1e-5); // smoothstep(0.25) of linear 0..1
        let h = Gradient {
            transition: Transition::Hue,
            ..Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(255, 0, 255)])
        };
        let mid = Color::from_linear(h.sample(0.5));
        assert!(mid.r > 200 && mid.b > 100 && mid.g < 10, "red to magenta passes through rose, not green: {mid:?}");
    }

    #[test]
    fn stops_at_the_same_place_make_a_hard_edge() {
        let g = Gradient {
            stops: vec![Stop { at: 0.5, color: RED }, Stop { at: 0.5, color: BLUE }],
            transition: Transition::Smooth,
        };
        assert_eq!(g.sample(0.49), RED.to_linear());
        assert_eq!(g.sample(0.51), BLUE.to_linear());
        assert!(g.sample(0.5).iter().all(|x| x.is_finite()));
    }

    #[test]
    fn bake_has_n_texels_and_both_ends() {
        let g = Gradient::even(&[RED, BLUE]);
        let b = g.bake(256);
        assert_eq!(b.len(), 256);
        assert_eq!(b[0], RED.to_linear());
        assert_eq!(b[255], BLUE.to_linear());
    }

    #[test]
    fn sorted_orders_clamps_and_fills_in_white() {
        let g = Gradient {
            stops: vec![Stop { at: 1.5, color: BLUE }, Stop { at: -1.0, color: RED }],
            transition: Transition::Steps,
        };
        let g = g.sorted();
        assert_eq!((g.stops[0].at, g.stops[0].color, g.stops[1].at), (0.0, RED, 1.0));
        assert_eq!(Gradient { stops: Vec::new(), transition: Transition::Smooth }.sorted(), Gradient::solid(WHITE));
    }

    #[test]
    fn saves_as_stops_and_reads_a_hex_string_as_solid() {
        let g = Gradient::even(&[RED, BLUE]);
        let text = ron::to_string(&g).unwrap();
        assert_eq!(ron::from_str::<Gradient>(&text).unwrap(), g);
        assert_eq!(ron::from_str::<Gradient>("\"#ff0000\"").unwrap(), Gradient::solid(RED));
    }
}
