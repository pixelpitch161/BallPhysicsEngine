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

/// How the colours between two stops are worked out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Choice)]
pub enum Mix {
    /// A straight blend in linear light: even light, can go grey or dim in the middle.
    #[default]
    Light,
    /// A straight blend of the colours' numbers, like most paint apps.
    Plain,
    /// A straight blend in OKLab: brightness changes evenly and the middle never goes muddy.
    Even,
    /// Round the colour wheel the short way (OKLCH): through the hues between.
    RoundTheWheel,
    /// Round the colour wheel the long way (OKLCH): through the other hues.
    LongWayRound,
}

/// How one gap between two stops blends from the first colour to the second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Choice)]
pub enum GapBlend {
    /// A straight blend.
    #[default]
    Smooth,
    /// Lingers at each stop and moves quickly between them.
    Eased,
    /// Starts slowly and speeds up.
    EaseIn,
    /// Starts quickly and slows down.
    EaseOut,
    /// A hard edge at the gap's middle: the first colour holds until there, then the second.
    Steps,
}

impl GapBlend {
    fn is_smooth(&self) -> bool {
        *self == Self::Smooth
    }
}

fn half() -> f32 {
    0.5
}

fn is_half(x: &f32) -> bool {
    *x == 0.5
}

/// One colour sitting at a place along a gradient, and how the gap after it blends to the next
/// stop (the last stop's gap is the one back round to the first, used only when the gradient
/// loops).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    /// Where along the gradient, 0 to 1.
    pub at: f32,
    pub color: Color,
    /// How the gap after this stop blends.
    #[serde(default, skip_serializing_if = "GapBlend::is_smooth")]
    pub blend: GapBlend,
    /// Where in the gap after this stop the halfway colour sits, 0 to 1 (for Steps, where the
    /// edge falls).
    #[serde(default = "half", skip_serializing_if = "is_half")]
    pub middle: f32,
}

impl Stop {
    /// A stop whose gap blends smoothly with its halfway colour in the middle.
    pub const fn new(at: f32, color: Color) -> Self {
        Self { at, color, blend: GapBlend::Smooth, middle: 0.5 }
    }
}

/// How the gradients of older files said every gap blends. Read, never written.
#[derive(Clone, Copy, Debug, Deserialize)]
enum SavedTransition {
    Smooth,
    Eased,
    Steps,
    Hue,
}

/// Colours at places along 0 to 1, how each gap between them blends, and how colours mix.
/// Written in scene files as its stops (with `mix` unless Light and `looped` when on), or as a
/// single hex string for a solid colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    /// In order of `at`. Two stops at the same place make a hard edge there.
    pub stops: Vec<Stop>,
    pub mix: Mix,
    /// Loop: the gradient is a circle, the last stop blending back round to the first through
    /// 1 = 0 with the last stop's gap. `sample` wraps its place.
    pub looped: bool,
    /// Draw-time only: the strength of the per-pixel grain the GPU adds where it paints this
    /// gradient, in 1/255 steps. Copied from the setting's `~grain` companion by
    /// [`crate::gradient_extras::apply_moves`] just before drawing; never saved, never edited, and
    /// ignored by [`Gradient::sample`].
    pub grain: f32,
    /// Draw-time only, like `grain`: the gradient's shape companions (its form and their
    /// numbers) as they stand this frame, set by [`crate::gradient_extras::apply_moves`]. Never
    /// saved and ignored by [`Gradient::sample`].
    pub layout: crate::gradient_form::Layout,
}

impl Default for Gradient {
    fn default() -> Self {
        Self::solid(Color::WHITE)
    }
}

impl Serialize for Gradient {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        fn is_light(m: &Mix) -> bool {
            *m == Mix::Light
        }
        fn is_false(b: &bool) -> bool {
            !*b
        }
        #[derive(Serialize)]
        #[serde(rename = "Gradient")]
        struct Saved<'a> {
            stops: &'a [Stop],
            #[serde(skip_serializing_if = "is_light")]
            mix: Mix,
            #[serde(skip_serializing_if = "is_false")]
            looped: bool,
        }
        Saved { stops: &self.stops, mix: self.mix, looped: self.looped }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Gradient {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Solid(Color),
            Stops {
                stops: Vec<Stop>,
                #[serde(default)]
                transition: Option<SavedTransition>,
                #[serde(default)]
                mix: Option<Mix>,
                #[serde(default)]
                looped: bool,
            },
        }
        Ok(match Saved::deserialize(deserializer)? {
            Saved::Solid(c) => Gradient::solid(c),
            Saved::Stops { mut stops, transition, mix, looped } => {
                // An older file's one transition for the whole gradient becomes each gap's blend,
                // or Round the wheel for its Hue (which went round the HSV wheel).
                let blend = match transition {
                    Some(SavedTransition::Eased) => Some(GapBlend::Eased),
                    Some(SavedTransition::Steps) => Some(GapBlend::Steps),
                    _ => None,
                };
                if let Some(blend) = blend {
                    for s in &mut stops {
                        s.blend = blend;
                    }
                }
                let hue = matches!(transition, Some(SavedTransition::Hue));
                let mix = mix.unwrap_or(if hue { Mix::RoundTheWheel } else { Mix::Light });
                Gradient { stops, mix, looped, grain: 0.0, layout: Default::default() }.sorted()
            }
        })
    }
}

/// `t` of the way from `a` to `b`, componentwise.
fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

/// The sRGB number (0 to 1, not rounded) of a linear-light channel, and back.
fn encode(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

fn decode(s: f32) -> f32 {
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

/// Below this OKLCH chroma a colour counts as grey and has no hue of its own.
const GREY_CHROMA: f32 = 1e-4;

/// `u` of the way from `a` to `b` (linear-light RGBA out) mixed the way `mix` says. Alpha always
/// blends straight; 0 and 1 give the end colours exactly.
pub fn mix_colours(a: Color, b: Color, u: f32, mix: Mix) -> [f32; 4] {
    let (la, lb) = (a.to_linear(), b.to_linear());
    if u.is_nan() || u <= 0.0 {
        return la;
    }
    if u >= 1.0 {
        return lb;
    }
    let alpha = la[3] + (lb[3] - la[3]) * u;
    let rgb = |c: [f32; 4]| [c[0], c[1], c[2]];
    let lerp3 = |x: [f32; 3], y: [f32; 3]| std::array::from_fn::<f32, 3, _>(|k| x[k] + (y[k] - x[k]) * u);
    let out = match mix {
        Mix::Light => return lerp4(la, lb, u),
        Mix::Plain => lerp3(rgb(la).map(encode), rgb(lb).map(encode)).map(decode),
        Mix::Even => {
            let lab = lerp3(crate::oklab::linear_to_oklab(rgb(la)), crate::oklab::linear_to_oklab(rgb(lb)));
            crate::oklab::fit_gamut(crate::oklab::oklab_to_lch(lab))
        }
        Mix::RoundTheWheel | Mix::LongWayRound => {
            use crate::oklab::{linear_to_oklab, oklab_to_lch};
            let (mut ca, mut cb) = (oklab_to_lch(linear_to_oklab(rgb(la))), oklab_to_lch(linear_to_oklab(rgb(lb))));
            // A grey end takes the other end's hue, so the blend doesn't swing through others
            // (the long way too: a grey has no hue to go round from).
            let grey = ca[1] < GREY_CHROMA || cb[1] < GREY_CHROMA;
            if ca[1] < GREY_CHROMA {
                ca[2] = cb[2];
            }
            if cb[1] < GREY_CHROMA {
                cb[2] = ca[2];
            }
            let short = (cb[2] - ca[2] + 0.5).rem_euclid(1.0) - 0.5;
            let turn = match mix {
                _ if grey => short,
                Mix::RoundTheWheel => short,
                // Two colours of one hue go a whole turn round the long way.
                // The other way round: a whole turn less (or more) than the short way.
                _ if short >= 0.0 => short - 1.0,
                _ => short + 1.0,
            };
            let lch = [ca[0] + (cb[0] - ca[0]) * u, ca[1] + (cb[1] - ca[1]) * u, (ca[2] + turn * u).rem_euclid(1.0)];
            crate::oklab::fit_gamut(lch)
        }
    };
    [out[0], out[1], out[2], alpha]
}

impl Gradient {
    /// One colour everywhere.
    pub fn solid(c: Color) -> Self {
        Self { stops: vec![Stop::new(0.0, c)], mix: Mix::Light, looped: false, grain: 0.0, layout: Default::default() }
    }

    /// The colours spread evenly from 0 to 1, blending smoothly in linear light. No colours is
    /// solid white.
    pub fn even(colors: &[Color]) -> Self {
        Self::spread(colors, Mix::Light, false)
    }

    /// The colours spread evenly, mixed the way `mix` says: from 0 to 1, or at `i/n` when
    /// `looped` (so the last blends back round to the first). No colours is solid white.
    pub fn spread(colors: &[Color], mix: Mix, looped: bool) -> Self {
        let mut g = match colors {
            [] => Self::solid(Color::WHITE),
            _ => Self {
                stops: colors.iter().map(|&c| Stop::new(0.0, c)).collect(),
                mix,
                looped,
                grain: 0.0,
                layout: Default::default(),
            },
        };
        if colors.len() > 1 {
            g.mix = mix;
            g.looped = looped;
        }
        g.spread_evenly();
        g
    }

    /// The colour of the first stop.
    pub fn first(&self) -> Color {
        self.stops.first().map_or(Color::WHITE, |s| s.color)
    }

    /// The same gradient with its stops in order and inside 0 to 1, each gap's middle inside 0
    /// to 1. No stops is solid white.
    pub fn sorted(mut self) -> Self {
        if self.stops.is_empty() {
            return Self { mix: self.mix, looped: self.looped, ..Self::solid(Color::WHITE) };
        }
        for s in &mut self.stops {
            s.at = if s.at.is_finite() { s.at.clamp(0.0, 1.0) } else { 0.0 };
            s.middle = if s.middle.is_finite() { s.middle.clamp(0.0, 1.0) } else { 0.5 };
        }
        self.stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        self
    }

    /// Spreads the stops evenly, keeping their order: at `i/n` when the gradient loops, else
    /// from 0 to 1. One stop sits at 0.
    pub fn spread_evenly(&mut self) {
        let n = self.stops.len();
        let span = if self.looped { n } else { n.saturating_sub(1) }.max(1) as f32;
        for (i, s) in self.stops.iter_mut().enumerate() {
            s.at = i as f32 / span;
        }
    }

    /// Switches Loop on or off, as the editor's Loop switch does. Switching it on with the last
    /// stop at the very end spreads the stops to `i/n`, leaving room for the blend back round;
    /// switching it off with the stops at `i/n` spreads them back from 0 to 1.
    pub fn set_looped(&mut self, on: bool) {
        if on == self.looped {
            return;
        }
        let n = self.stops.len();
        let at_loop_places =
            n > 1 && self.stops.iter().enumerate().all(|(i, s)| (s.at - i as f32 / n as f32).abs() < 1e-4);
        let last_at_end = self.stops.last().is_some_and(|s| s.at >= 1.0 - 1e-4);
        self.looped = on;
        if n > 1 && ((on && last_at_end) || (!on && at_loop_places)) {
            self.spread_evenly();
        }
    }

    /// The linear-light colour at `t`: clamped to 0 to 1, or wrapped round when the gradient
    /// loops. The stops are taken to be in order, and where two share a place the later one wins
    /// from there on.
    pub fn sample(&self, t: f32) -> [f32; 4] {
        let n = self.stops.len();
        if n <= 1 {
            return self.first().to_linear();
        }
        if self.looped {
            let t = if t.is_finite() { t - t.floor() } else { 0.0 };
            let (first, last) = (&self.stops[0], &self.stops[n - 1]);
            if t < first.at || t >= last.at {
                // The gap back round from the last stop to the first, through 1 = 0. It has room
                // whenever `t` can land in it (with the first at 0 and the last at 1 it can't).
                let width = first.at + 1.0 - last.at;
                let gone = if t >= last.at { t - last.at } else { t + 1.0 - last.at };
                let f = if width > 0.0 { gone / width } else { 0.0 };
                return self.gap(last, first, f);
            }
            return self.inside(t);
        }
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        self.inside(t)
    }

    /// The colour at `t` (0 to 1) between the first and last stops' places, or past them.
    fn inside(&self, t: f32) -> [f32; 4] {
        let Some(i) = self.stops.iter().rposition(|s| s.at <= t) else {
            return self.first().to_linear();
        };
        let Some(next) = self.stops.get(i + 1) else {
            return self.stops[i].color.to_linear();
        };
        let a = &self.stops[i];
        self.gap(a, next, (t - a.at) / (next.at - a.at))
    }

    /// The colour `f` (0 to 1) of the way through the gap after stop `a`, to stop `b`: bent so
    /// the halfway colour sits at `a.middle`, shaped by `a.blend`, then mixed.
    fn gap(&self, a: &Stop, b: &Stop, f: f32) -> [f32; 4] {
        let f = if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 };
        let m = if a.middle.is_finite() { a.middle.clamp(0.001, 0.999) } else { 0.5 };
        if a.blend == GapBlend::Steps {
            return (if f < m { a.color } else { b.color }).to_linear();
        }
        // `f^p` with `m^p = 1/2`; the middle at 0.5 leaves `f` exactly as it is.
        let f = if a.middle == 0.5 { f } else { f.powf(0.5f32.ln() / m.ln()) };
        let u = match a.blend {
            GapBlend::Smooth | GapBlend::Steps => f,
            GapBlend::Eased => f * f * (3.0 - 2.0 * f),
            GapBlend::EaseIn => f * f,
            GapBlend::EaseOut => 1.0 - (1.0 - f) * (1.0 - f),
        };
        mix_colours(a.color, b.color, u, self.mix)
    }

    /// The gradient cut into `n` flat bands, each the colour at its band's middle (a looped
    /// gradient's bands wrap round through its last gap). Two stops a band, each band flat in any
    /// mix; Loop and grain are kept. 0 bands leaves it as it is.
    pub fn banded(&self, n: u32) -> Gradient {
        if n == 0 {
            return self.clone();
        }
        let size = 1.0 / n as f32;
        let mut stops = Vec::with_capacity(2 * n as usize);
        for k in 0..n {
            let color = Color::from_linear(self.sample((k as f32 + 0.5) * size));
            stops.push(Stop::new(k as f32 * size, color));
            stops.push(Stop::new(if k + 1 == n { 1.0 } else { (k + 1) as f32 * size }, color));
        }
        Gradient { stops, mix: Mix::Light, looped: self.looped, grain: self.grain, layout: self.layout.clone() }
    }

    /// The stops for reading one colour at a time (stepping a colour on each hit, a light show's
    /// colour list): every stop, except that a gradient cut into bands ([`Gradient::banded`],
    /// each band two stops of one colour) gives each band's stop once.
    pub fn step_stops(&self) -> std::borrow::Cow<'_, [Stop]> {
        let n = self.stops.len() / 2;
        let size = 1.0 / n.max(1) as f32;
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        let banded = n >= 1
            && self.stops.len() == 2 * n
            && self.stops.chunks(2).enumerate().all(|(k, pair)| {
                pair[0].color == pair[1].color
                    && near(pair[0].at, k as f32 * size)
                    && near(pair[1].at, (k + 1) as f32 * size)
            });
        if banded {
            std::borrow::Cow::Owned(self.stops.iter().step_by(2).copied().collect())
        } else {
            std::borrow::Cow::Borrowed(&self.stops)
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

    /// A gradient whose every gap is Steps.
    fn steps(stops: Vec<Stop>) -> Gradient {
        let stops = stops.into_iter().map(|s| Stop { blend: GapBlend::Steps, ..s }).collect();
        Gradient { stops, ..Gradient::default() }
    }

    fn near4(a: [f32; 4], b: [f32; 4], eps: f32) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= eps)
    }

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
        let g = Gradient { stops: vec![Stop::new(0.0, RED), Stop::new(0.8, BLUE)], ..Gradient::default() };
        assert_eq!(g.sample(0.8), BLUE.to_linear());
        assert_eq!(g.sample(1.0), BLUE.to_linear());
        assert!((g.sample(0.4)[0] - RED.to_linear()[0] * 0.5).abs() < 1e-5);
    }

    #[test]
    fn steps_split_halfway_between_stops() {
        let g = steps(vec![Stop::new(0.0, RED), Stop::new(0.6, BLUE)]);
        assert_eq!(g.sample(0.29), RED.to_linear());
        assert_eq!(g.sample(0.31), BLUE.to_linear());
    }

    #[test]
    fn eased_is_smoothstep_and_round_the_wheel_goes_the_short_way() {
        let mut e = Gradient::even(&[BLACK, WHITE]);
        e.stops[0].blend = GapBlend::Eased;
        assert!((e.sample(0.25)[0] - 0.15625).abs() < 1e-5); // smoothstep(0.25) of linear 0..1
        let h = Gradient::spread(&[Color::rgb(255, 0, 0), Color::rgb(255, 0, 255)], Mix::RoundTheWheel, false);
        let mid = Color::from_linear(h.sample(0.5));
        assert!(mid.r > 200 && mid.b > 100 && mid.g < 70, "red to magenta passes through rose, not green: {mid:?}");
    }

    #[test]
    fn stops_at_the_same_place_make_a_hard_edge() {
        let g = Gradient { stops: vec![Stop::new(0.5, RED), Stop::new(0.5, BLUE)], ..Gradient::default() };
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
        let mut g = steps(vec![Stop::new(1.5, BLUE), Stop::new(-1.0, RED)]);
        g.stops[0].middle = f32::NAN;
        let g = g.sorted();
        assert_eq!((g.stops[0].at, g.stops[0].color, g.stops[1].at), (0.0, RED, 1.0));
        assert_eq!(g.stops[1].middle, 0.5, "a middle that isn't a number goes back to the middle");
        assert_eq!(Gradient { stops: Vec::new(), ..Gradient::default() }.sorted(), Gradient::solid(WHITE));
    }

    #[test]
    fn saves_as_stops_and_reads_a_hex_string_as_solid() {
        let g = Gradient::even(&[RED, BLUE]);
        let text = ron::to_string(&g).unwrap();
        assert_eq!(ron::from_str::<Gradient>(&text).unwrap(), g);
        assert_eq!(ron::from_str::<Gradient>("\"#ff0000\"").unwrap(), Gradient::solid(RED));
    }

    #[test]
    fn stops_without_a_transition_read_as_smooth() {
        let text = r##"(stops: [(at: 0.0, color: "#ff0000"), (at: 1.0, color: "#0000ff")])"##;
        assert_eq!(ron::from_str::<Gradient>(text).unwrap(), Gradient::even(&[RED, BLUE]));
    }

    #[test]
    fn plain_blends_the_numbers_and_even_blends_the_lightness() {
        let plain = Gradient::spread(&[BLACK, WHITE], Mix::Plain, false);
        assert!((encode(plain.sample(0.5)[0]) - 0.5).abs() < 1e-4, "half of sRGB, not of linear light");
        let even = Gradient::spread(&[BLACK, WHITE], Mix::Even, false);
        let mid = even.sample(0.5);
        let lab = crate::oklab::linear_to_oklab([mid[0], mid[1], mid[2]]);
        assert!((lab[0] - 0.5).abs() < 1e-3, "{lab:?}");
        // Light is today's blend, exactly.
        let light = Gradient::even(&[RED, BLUE]);
        assert_eq!(light.sample(0.25), lerp4(RED.to_linear(), BLUE.to_linear(), 0.25));
    }

    #[test]
    fn round_the_wheel_goes_the_short_way_and_the_long_way_the_other() {
        use crate::oklab::{linear_to_oklab, oklab_to_lch};
        let hue = |c: [f32; 4]| oklab_to_lch(linear_to_oklab([c[0], c[1], c[2]]))[2];
        let (hr, hb) = (hue(RED.to_linear()), hue(BLUE.to_linear()));
        let short = Gradient::spread(&[RED, BLUE], Mix::RoundTheWheel, false).sample(0.5);
        let long = Gradient::spread(&[RED, BLUE], Mix::LongWayRound, false).sample(0.5);
        let way = (hb - hr + 0.5).rem_euclid(1.0) - 0.5;
        let want_short = (hr + way * 0.5).rem_euclid(1.0);
        let want_long = (hr + (way + if way >= 0.0 { -1.0 } else { 1.0 }) * 0.5).rem_euclid(1.0);
        let off = |a: f32, b: f32| ((a - b + 0.5).rem_euclid(1.0) - 0.5).abs();
        assert!(off(hue(short), want_short) < 0.01, "{} vs {want_short}", hue(short));
        assert!(off(hue(long), want_long) < 0.01, "{} vs {want_long}", hue(long));
        // Red to blue the short way passes magenta; the long way passes green.
        let (s, l) = (Color::from_linear(short), Color::from_linear(long));
        assert!(s.r > s.g && s.b > s.g, "{s:?}");
        assert!(l.g > l.b && l.g > 30, "{l:?}");
        for c in [short, long] {
            assert!(c.iter().all(|x| (0.0..=1.0).contains(x)));
        }
    }

    #[test]
    fn a_grey_end_takes_the_other_ends_hue() {
        let g = Gradient::spread(&[RED, WHITE], Mix::RoundTheWheel, false);
        let mid = Color::from_linear(g.sample(0.5));
        assert!(mid.r > mid.g && mid.r > mid.b && (mid.g as i32 - mid.b as i32).abs() < 30, "a pink: {mid:?}");
        // The long way round from a grey is the short way: there is no hue to go round from.
        let long = Gradient::spread(&[RED, WHITE], Mix::LongWayRound, false);
        assert_eq!(long.sample(0.3), Gradient { mix: Mix::RoundTheWheel, ..long.clone() }.sample(0.3));
    }

    #[test]
    fn gap_blends_and_middles() {
        let mut g = Gradient::even(&[BLACK, WHITE]);
        g.stops[0].blend = GapBlend::EaseIn;
        assert!((g.sample(0.5)[0] - 0.25).abs() < 1e-5);
        g.stops[0].blend = GapBlend::EaseOut;
        assert!((g.sample(0.5)[0] - 0.75).abs() < 1e-5);
        g.stops[0].blend = GapBlend::Smooth;
        g.stops[0].middle = 0.25;
        assert!((g.sample(0.25)[0] - 0.5).abs() < 1e-5, "the halfway colour sits at the middle");
        assert!(g.sample(0.1)[0] < 0.5 && g.sample(0.4)[0] > 0.5);
        g.stops[0].blend = GapBlend::Steps;
        g.stops[0].middle = 0.3;
        assert_eq!(g.sample(0.29), BLACK.to_linear());
        assert_eq!(g.sample(0.3), WHITE.to_linear());
        // The last stop's gap does nothing when the gradient doesn't loop.
        let mut g = Gradient::even(&[RED, BLUE]);
        g.stops[1].blend = GapBlend::Steps;
        assert_eq!(g.sample(0.5), Gradient::even(&[RED, BLUE]).sample(0.5));
    }

    #[test]
    fn a_looped_gradient_wraps_round_through_its_last_gap() {
        let g = Gradient::spread(&[RED, Color::rgb(0, 255, 0), BLUE], Mix::Light, true);
        assert_eq!(g.stops.iter().map(|s| s.at).collect::<Vec<_>>(), vec![0.0, 1.0 / 3.0, 2.0 / 3.0]);
        assert_eq!(g.sample(1.0), g.sample(0.0));
        assert!(near4(g.sample(1.25), g.sample(0.25), 1e-6));
        assert!(near4(g.sample(-0.25), g.sample(0.75), 1e-6));
        // Halfway back round from blue to red.
        assert!(near4(g.sample(5.0 / 6.0), lerp4(BLUE.to_linear(), RED.to_linear(), 0.5), 1e-5));
        // The wrap gap takes the last stop's blend.
        let mut stepped = g.clone();
        stepped.stops[2].blend = GapBlend::Steps;
        assert_eq!(stepped.sample(0.8), BLUE.to_linear());
        assert_eq!(stepped.sample(0.9), RED.to_linear());
        // With the first stop past 0, the wrap runs across 0.
        let g =
            Gradient { stops: vec![Stop::new(0.2, RED), Stop::new(0.6, BLUE)], looped: true, ..Gradient::default() };
        let mid = lerp4(BLUE.to_linear(), RED.to_linear(), 0.5);
        assert!(near4(g.sample(0.9), mid, 1e-5), "{:?}", g.sample(0.9));
        // 0 is 0.4 into the 0.6 wide gap from 0.6 round to 1.2.
        assert!(near4(g.sample(0.0), lerp4(BLUE.to_linear(), RED.to_linear(), 0.4 / 0.6), 1e-5));
    }

    #[test]
    fn a_banded_gradient_steps_through_each_band_once() {
        let g = Gradient::even(&[RED, BLUE, BLACK]);
        assert_eq!(g.step_stops().len(), 3);
        let b = g.banded(4);
        let stops = b.step_stops();
        assert_eq!(stops.len(), 4);
        assert_eq!(stops[0].color, b.stops[0].color);
        assert_eq!(stops[3].color, b.stops[7].color);
        // A hard edge between two colours is two bands of them, one colour each.
        let edge = Gradient {
            stops: vec![Stop::new(0.0, RED), Stop::new(0.5, RED), Stop::new(0.5, BLUE), Stop::new(1.0, BLUE)],
            ..Gradient::default()
        };
        assert_eq!(edge.step_stops().iter().map(|s| s.color).collect::<Vec<_>>(), [RED, BLUE]);
        // Stops of one colour spaced unlike bands each count.
        let three = Gradient::even(&[RED, RED, RED]);
        assert_eq!(three.step_stops().len(), 3);
    }

    #[test]
    fn bands_are_flat_and_take_their_middles() {
        let g = Gradient::even(&[BLACK, WHITE]).banded(5);
        assert_eq!(g.stops.len(), 10);
        for k in 0..5 {
            let want = Color::from_linear(Gradient::even(&[BLACK, WHITE]).sample((k as f32 + 0.5) / 5.0)).to_linear();
            for f in [0.05, 0.5, 0.95] {
                assert_eq!(g.sample((k as f32 + f) / 5.0), want, "band {k} at {f}");
            }
        }
        assert_eq!(Gradient::even(&[RED, BLUE]).banded(0), Gradient::even(&[RED, BLUE]));
        // A looped gradient's bands go back round: the last band blends blue back towards red.
        let looped = Gradient::spread(&[RED, BLUE], Mix::Light, true).banded(4);
        assert!(looped.looped);
        let last = Color::from_linear(looped.sample(0.9));
        assert!(last.r > 0 && last.b > 0, "{last:?}");
        assert_eq!(looped.sample(0.999), looped.sample(0.9));
    }

    #[test]
    fn loop_switch_respaces_only_when_it_has_to() {
        let mut g = Gradient::even(&[RED, BLUE, WHITE]);
        g.set_looped(true);
        assert!(g.looped);
        assert_eq!(g.stops.iter().map(|s| s.at).collect::<Vec<_>>(), vec![0.0, 1.0 / 3.0, 2.0 / 3.0]);
        g.set_looped(false);
        assert_eq!(g.stops.iter().map(|s| s.at).collect::<Vec<_>>(), vec![0.0, 0.5, 1.0]);
        let mut g = Gradient { stops: vec![Stop::new(0.1, RED), Stop::new(0.7, BLUE)], ..Gradient::default() };
        g.set_looped(true);
        assert_eq!(g.stops.iter().map(|s| s.at).collect::<Vec<_>>(), vec![0.1, 0.7], "room already");
        g.set_looped(false);
        assert_eq!(g.stops.iter().map(|s| s.at).collect::<Vec<_>>(), vec![0.1, 0.7]);
    }

    #[test]
    fn new_fields_save_and_old_transitions_read_as_gap_blends() {
        let mut g = Gradient::spread(&[RED, BLUE, WHITE], Mix::Even, true);
        g.stops[1].blend = GapBlend::EaseOut;
        g.stops[1].middle = 0.3;
        g.grain = 3.0;
        let text = ron::to_string(&g).unwrap();
        assert!(!text.contains("grain") && text.contains("mix:Even") && text.contains("looped:true"), "{text}");
        assert_eq!(ron::from_str::<Gradient>(&text).unwrap(), Gradient { grain: 0.0, layout: Default::default(), ..g });
        let plain = ron::to_string(&Gradient::even(&[RED, BLUE])).unwrap();
        assert!(!plain.contains("mix") && !plain.contains("looped") && !plain.contains("blend"), "{plain}");
        let old = |t: &str| {
            let text =
                format!(r##"(stops: [(at: 0.0, color: "#ff0000"), (at: 1.0, color: "#0000ff")], transition: {t})"##);
            ron::from_str::<Gradient>(&text).unwrap()
        };
        assert_eq!(old("Smooth"), Gradient::even(&[RED, BLUE]));
        assert!(old("Steps").stops.iter().all(|s| s.blend == GapBlend::Steps));
        assert!(old("Eased").stops.iter().all(|s| s.blend == GapBlend::Eased));
        let hue = old("Hue");
        assert_eq!((hue.mix, hue.stops[0].blend), (Mix::RoundTheWheel, GapBlend::Smooth));
    }
}
