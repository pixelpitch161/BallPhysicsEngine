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
    /// Draw-time only: how the gradient moves at the moment being drawn (its Move-tab
    /// companions, worked out by [`crate::gradient_extras::apply_moves`] just before drawing).
    /// [`Gradient::sample`] honours it; never saved, never edited, still by default.
    pub moves: Moves,
}

/// How a gradient moves at one moment: set on the value by
/// [`crate::gradient_extras::apply_moves`] from its Move-tab companions, honoured by
/// [`Gradient::sample`] (all but the wave, which the GPU and the CPU colours apply per ball or
/// shape from its last hit). The default is still.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Moves {
    /// How far the colours have slid along (Cycle and Step on the beat), in gradient lengths.
    pub shift: f32,
    /// How far the stops are spread round the middle (Breathe), as a natural log: they sit
    /// `e^breathe` times as far from the middle.
    pub breathe: f32,
    /// How far every colour's hue is turned (Hue spin), in turns round the OKLCH wheel.
    pub hue: f32,
    /// The second gradient the colours blend toward (Morph).
    pub morph: Option<Box<Morph>>,
    /// The bright band a hit sends along the gradient (Hit wave).
    pub wave: Option<HitWave>,
}

impl Moves {
    /// Whether sampling the gradient is as if it had no moves (the wave doesn't change sampling).
    pub fn is_still(&self) -> bool {
        self.shift == 0.0 && self.breathe == 0.0 && self.hue == 0.0 && self.morph.is_none()
    }

    /// Only the wave kept: what is left once the other moves are baked into the stops.
    fn wave_only(&self) -> Moves {
        Moves { wave: self.wave, ..Moves::default() }
    }
}

/// Morph: every point of the gradient blends `amount` of the way toward the same point of `to`,
/// in the gradient's own Mix.
#[derive(Clone, Debug, PartialEq)]
pub struct Morph {
    pub to: Gradient,
    pub amount: f32,
}

/// Hit wave: after a hit, a band `width` wide (as a share of the gradient) runs along the
/// gradient from its start at `speed` gradients a second (from its end when negative), turning
/// the colours toward `colour` by `strength` (past 1 it adds light).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitWave {
    pub strength: f32,
    pub width: f32,
    pub speed: f32,
    pub colour: Color,
}

impl HitWave {
    /// `c` (linear light) at place `t` along the gradient `age` seconds after a hit, with the
    /// wave on it. paint.wgsl's `paint_wave` does the same sums.
    pub fn apply(&self, c: [f32; 4], t: f32, age: f32) -> [f32; 4] {
        let i = self.at(t, age);
        if i <= 0.0 {
            return c;
        }
        let w = self.colour.to_linear();
        let cover = i.min(1.0);
        let extra = (i - 1.0).max(0.0);
        [
            c[0] + (w[0] - c[0]) * cover + w[0] * extra,
            c[1] + (w[1] - c[1]) * cover + w[1] * extra,
            c[2] + (w[2] - c[2]) * cover + w[2] * extra,
            c[3],
        ]
    }

    /// How strongly the wave shows at place `t` `age` seconds after a hit: its strength times a
    /// smooth bump round the front, 0 past `width` from it.
    pub fn at(&self, t: f32, age: f32) -> f32 {
        if !(age >= 0.0 && age.is_finite() && t.is_finite()) || !(self.strength > 0.0 && self.strength.is_finite()) {
            return 0.0;
        }
        let width = if self.width.is_finite() { self.width.max(1e-4) } else { 1e-4 };
        let speed = if self.speed.is_finite() { self.speed } else { 0.0 };
        let front = if speed < 0.0 { 1.0 + speed * age } else { speed * age };
        let d = (t - front) / width;
        if d.abs() >= 1.0 {
            return 0.0;
        }
        let bump = 1.0 - d * d;
        self.strength * bump * bump
    }

    /// How long after a hit the wave is still on the gradient, in seconds (a still wave never
    /// leaves: 10 000 s).
    pub fn lasts(&self) -> f32 {
        let speed = if self.speed.is_finite() { self.speed.abs() } else { 0.0 };
        if speed < 1e-4 {
            return 10_000.0;
        }
        let width = if self.width.is_finite() { self.width.max(0.0) } else { 0.0 };
        ((1.0 + width) / speed).min(10_000.0)
    }
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
                Gradient { stops, mix, looped, grain: 0.0, moves: Moves::default() }.sorted()
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
    mix_linear(a.to_linear(), b.to_linear(), u, mix)
}

/// [`mix_colours`] of two linear-light colours.
pub fn mix_linear(la: [f32; 4], lb: [f32; 4], u: f32, mix: Mix) -> [f32; 4] {
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
        Self { stops: vec![Stop::new(0.0, c)], mix: Mix::Light, looped: false, grain: 0.0, moves: Moves::default() }
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
                moves: Moves::default(),
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
    ///
    /// A moving gradient ([`Gradient::moves`]) is sampled moved: slid along (an unlooped one
    /// wrapping with a cut), spread round the middle, blended toward its Morph gradient and its
    /// hues turned. A still one samples exactly as its stops say.
    pub fn sample(&self, t: f32) -> [f32; 4] {
        if self.moves.is_still() {
            return self.sample_still(t);
        }
        let m = &self.moves;
        let mut v = if t.is_finite() { t } else { 0.0 };
        if m.shift != 0.0 && m.shift.is_finite() {
            v -= m.shift;
            if !self.looped {
                v -= v.floor();
            }
        }
        if m.breathe != 0.0 && m.breathe.is_finite() {
            v = 0.5 + (v - 0.5) * (-m.breathe).exp();
        }
        let mut c = self.sample_still(v);
        if let Some(morph) = &m.morph {
            c = mix_linear(c, morph.to.sample(v), morph.amount, self.mix);
        }
        if m.hue != 0.0 && m.hue.is_finite() {
            c = hue_turned(c, m.hue);
        }
        c
    }

    /// [`Gradient::sample`] as the stops say, without the moves.
    pub fn sample_still(&self, t: f32) -> [f32; 4] {
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
    /// mix; Loop, grain and the wave are kept, the other moves baked into the bands' colours. 0
    /// bands leaves it as it is.
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
        Gradient { stops, mix: Mix::Light, looped: self.looped, grain: self.grain, moves: self.moves.wave_only() }
    }

    /// The stops for reading one colour at a time (stepping a colour on each hit, a light show's
    /// colour list): every stop, except that a gradient cut into bands ([`Gradient::banded`],
    /// each band two stops of one colour) gives each band's stop once.
    ///
    /// A moving gradient gives each stop its colour as moved (sampled at its place).
    pub fn step_stops(&self) -> std::borrow::Cow<'_, [Stop]> {
        let stops = self.step_stops_still();
        if self.moves.is_still() {
            return stops;
        }
        std::borrow::Cow::Owned(
            stops.iter().map(|s| Stop { color: Color::from_linear(self.sample(s.at)), ..*s }).collect(),
        )
    }

    /// [`Gradient::step_stops`] without the moves.
    fn step_stops_still(&self) -> std::borrow::Cow<'_, [Stop]> {
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

    /// The gradient with its stops wandering (Stops wander) at `time` seconds: each drifts on
    /// its own, smooth seeded noise at `speed` changes a second, by up to `amount` (1 at most)
    /// of half the gap to its neighbour on the side it drifts to, so no stop ever passes another.
    /// An end stop of an unlooped gradient drifts only inward, and stops sharing a place (a hard
    /// edge) drift together. The same at the same time.
    pub fn wandered(&self, amount: f32, speed: f32, seed: u32, time: f64) -> Gradient {
        let n = self.stops.len();
        let amount = if amount.is_finite() { amount.clamp(-1.0, 1.0) } else { 0.0 };
        if n < 2 || amount == 0.0 {
            return self.clone();
        }
        let speed = if speed.is_finite() { speed as f64 } else { 0.0 };
        let x = if time.is_finite() { time * speed } else { 0.0 };
        // Stops sharing a place (a hard edge) drift together as one.
        let mut places: Vec<f32> = Vec::new();
        let group: Vec<usize> = self
            .stops
            .iter()
            .map(|s| {
                if places.last() != Some(&s.at) {
                    places.push(s.at);
                }
                places.len() - 1
            })
            .collect();
        let m = places.len();
        let moved: Vec<f32> = (0..m)
            .map(|j| {
                let at = places[j];
                let drift = amount * wander_noise(seed, j, x);
                // The gap to the neighbour on the side it drifts to (round through 1 = 0 when
                // looped).
                let room = if drift > 0.0 {
                    match places.get(j + 1) {
                        Some(next) => next - at,
                        None if self.looped => places[0] + 1.0 - at,
                        None => 0.0,
                    }
                } else if j > 0 {
                    at - places[j - 1]
                } else if self.looped {
                    at + 1.0 - places[m - 1]
                } else {
                    0.0
                };
                at + drift * 0.5 * room.max(0.0)
            })
            .collect();
        let mut out = self.clone();
        for (stop, &j) in out.stops.iter_mut().zip(&group) {
            stop.at = moved[j];
        }
        // A looped gradient's stops may drift through 1 = 0: brought back into 0 to 1 and kept
        // in order round the circle.
        if self.looped {
            for s in &mut out.stops {
                s.at -= s.at.floor();
            }
            out.stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        } else {
            for s in &mut out.stops {
                s.at = s.at.clamp(0.0, 1.0);
            }
        }
        out
    }
}

/// `c` (linear light) with its hue turned `turns` round the OKLCH wheel, lightness and chroma
/// kept (greys stay grey; brought back into sRGB by lowering chroma). Alpha is kept.
pub fn hue_turned(c: [f32; 4], turns: f32) -> [f32; 4] {
    use crate::oklab::{fit_gamut, linear_to_oklab, oklab_to_lch};
    if !turns.is_finite() || turns == 0.0 {
        return c;
    }
    let mut lch = oklab_to_lch(linear_to_oklab([c[0], c[1], c[2]]));
    if lch[1] < GREY_CHROMA {
        return c;
    }
    lch[2] = (lch[2] + turns).rem_euclid(1.0);
    let rgb = fit_gamut(lch);
    [rgb[0], rgb[1], rgb[2], c[3]]
}

/// Smooth seeded noise in -1 to 1 for stop `i` at `x`: a value at each whole `x` from a hash of
/// the seed, the stop and `x`, eased between them. Pure: the same on every machine.
pub fn wander_noise(seed: u32, i: usize, x: f64) -> f32 {
    fn pcg(v: u32) -> u32 {
        let state = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
        (word >> 22) ^ word
    }
    let x = if x.is_finite() { x } else { 0.0 };
    let k = x.floor();
    let f = (x - k) as f32;
    let at = |k: f64| {
        let h = pcg(seed ^ pcg((i as u32).wrapping_mul(0x9e37_79b9) ^ pcg(k as i64 as u32)));
        (h >> 8) as f32 / 16_777_215.0 * 2.0 - 1.0
    };
    let (a, b) = (at(k), at(k + 1.0));
    let e = f * f * (3.0 - 2.0 * f);
    a + (b - a) * e
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
        assert_eq!(ron::from_str::<Gradient>(&text).unwrap(), Gradient { grain: 0.0, ..g });
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

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        (0..4).all(|k| (a[k] - b[k]).abs() < 1e-5)
    }

    fn rgb3() -> Gradient {
        Gradient::spread(&[RED, Color::rgb(0, 255, 0), BLUE], Mix::Even, false)
    }

    fn moved(g: &Gradient, moves: Moves) -> Gradient {
        Gradient { moves, ..g.clone() }
    }

    #[test]
    fn a_still_gradient_samples_as_its_stops() {
        let g = rgb3();
        assert!(g.moves.is_still());
        for i in 0..=40 {
            let t = i as f32 / 40.0;
            assert_eq!(g.sample(t), g.sample_still(t));
        }
        // A wave alone doesn't change sampling.
        let w = HitWave { strength: 1.0, width: 0.1, speed: 1.0, colour: Color::WHITE };
        assert!(Moves { wave: Some(w), ..Moves::default() }.is_still());
    }

    #[test]
    fn a_shift_slides_the_colours_and_cuts_without_loop() {
        let mut looped = rgb3();
        looped.set_looped(true);
        let slid = moved(&looped, Moves { shift: 0.3, ..Moves::default() });
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            assert!(close(slid.sample(t), looped.sample(t - 0.3)), "{t}");
        }
        let open = rgb3();
        let slid = moved(&open, Moves { shift: 0.25, ..Moves::default() });
        assert!(close(slid.sample(0.25), open.sample(0.0)));
        assert!(close(slid.sample(0.75), open.sample(0.5)));
        // The end wraps round to the start with a cut just before the shift.
        assert!(close(slid.sample(0.1), open.sample(0.85)));
        assert!(close(slid.sample(0.2499), open.sample(0.9999)));
    }

    #[test]
    fn breathing_spreads_the_stops_round_the_middle() {
        let g = rgb3();
        let b = 0.4f32;
        let spread = moved(&g, Moves { breathe: b, ..Moves::default() });
        // The stop at 0.25... here the stops are 0, 0.5, 1: the one at 0 shows at 0.5 - 0.5·e^b
        // (clamped), so look at a place inside: 0.4 shows as 0.5 + (0.4 - 0.5)·e^-b.
        let inside = 0.5 + (0.4 - 0.5) * (-b).exp();
        assert!(close(spread.sample(0.4), g.sample(inside)));
        assert!(close(spread.sample(0.5), g.sample(0.5)), "the middle stays");
    }

    #[test]
    fn hue_spin_turns_colours_and_leaves_greys() {
        let red = Gradient::solid(RED);
        let turned = moved(&red, Moves { hue: 0.5, ..Moves::default() });
        let c = Color::from_linear(turned.sample(0.0));
        assert!(c.r < 100 && c.g > 100 && c.b > 100, "red half way round is cyan-ish: {c:?}");
        let grey = Gradient::solid(Color::rgb(128, 128, 128));
        let still = moved(&grey, Moves { hue: 0.3, ..Moves::default() });
        assert_eq!(still.sample(0.0), grey.sample(0.0));
        assert!(
            close(hue_turned(RED.to_linear(), 1.0), RED.to_linear()) || {
                let back = Color::from_linear(hue_turned(RED.to_linear(), 1.0));
                back.r >= 254 && back.g <= 1 && back.b <= 1
            }
        );
    }

    #[test]
    fn morph_blends_every_point_toward_the_other_gradient() {
        let a = rgb3();
        let b = Gradient::spread(&[Color::rgb(255, 255, 0), Color::rgb(0, 0, 0)], Mix::Light, true);
        let at =
            |amount| moved(&a, Moves { morph: Some(Box::new(Morph { to: b.clone(), amount })), ..Moves::default() });
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            assert_eq!(at(0.0).sample(t), a.sample(t));
            assert!(close(at(1.0).sample(t), b.sample(t)));
            assert!(close(at(0.5).sample(t), mix_linear(a.sample(t), b.sample(t), 0.5, a.mix)));
        }
    }

    #[test]
    fn bands_bake_in_the_moves_and_keep_the_wave() {
        let mut g = rgb3();
        let w = HitWave { strength: 1.0, width: 0.1, speed: 1.0, colour: Color::WHITE };
        g.moves = Moves { shift: 0.2, hue: 0.1, wave: Some(w), ..Moves::default() };
        let banded = g.banded(4);
        assert!(banded.moves.is_still());
        assert_eq!(banded.moves.wave, Some(w));
        let c = Color::from_linear(banded.sample(0.125));
        assert_eq!(c, Color::from_linear(g.sample(0.125)));
    }

    #[test]
    fn stops_wander_without_passing_each_other() {
        let mut seed = 1u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as f32 / 16_777_216.0
        };
        for k in 0..50 {
            let n = 2 + (next() * 6.0) as usize;
            let mut ats: Vec<f32> = (0..n).map(|_| next()).collect();
            ats.sort_by(f32::total_cmp);
            let mut g = Gradient {
                stops: ats.iter().enumerate().map(|(i, &at)| Stop::new(at, Color::rgb(i as u8 * 30, 0, 0))).collect(),
                ..Gradient::default()
            };
            g.looped = k % 2 == 0;
            for step in 0..200 {
                let time = step as f64 * 0.37;
                let w = g.wandered(1.0, 1.3, k, time);
                // In the same order as before (round the circle when looped), nothing outside 0 to 1.
                let colours: Vec<u8> = w.stops.iter().map(|s| s.color.r).collect();
                let start = colours.iter().position(|&r| r == 0).unwrap();
                for (i, &r) in colours.iter().cycle().skip(start).take(n).enumerate() {
                    assert_eq!(r, i as u8 * 30, "order kept: {k} {time} {w:?}");
                }
                assert!(w.stops.iter().all(|s| (0.0..=1.0).contains(&s.at)));
                if !g.looped {
                    assert!(w.stops.windows(2).all(|p| p[0].at <= p[1].at));
                }
                assert_eq!(w, g.wandered(1.0, 1.3, k, time), "the same at the same time");
            }
        }
        let edge = Gradient {
            stops: vec![Stop::new(0.0, RED), Stop::new(0.5, RED), Stop::new(0.5, BLUE), Stop::new(1.0, BLUE)],
            ..Gradient::default()
        };
        for step in 0..50 {
            let w = edge.wandered(1.0, 1.0, 5, step as f64 * 0.3);
            assert_eq!(w.stops[1].at, w.stops[2].at, "a hard edge stays hard");
        }
        let g = rgb3();
        assert_ne!(g.wandered(0.5, 1.0, 1, 3.3), g.wandered(0.5, 1.0, 1, 4.1), "it moves");
        assert_ne!(g.wandered(0.5, 1.0, 1, 3.3), g.wandered(0.5, 1.0, 2, 3.3), "the seed counts");
        assert_eq!(g.wandered(0.0, 1.0, 1, 3.3), g);
        // The ends of an unlooped gradient only move inward.
        for step in 0..100 {
            let w = g.wandered(1.0, 2.0, 7, step as f64 * 0.21);
            assert!(w.stops[0].at >= 0.0 && w.stops[2].at <= 1.0);
        }
    }

    #[test]
    fn stepping_reads_moved_colours() {
        let g = moved(&rgb3(), Moves { hue: 0.5, ..Moves::default() });
        let stops = g.step_stops();
        assert_eq!(stops.len(), 3);
        assert_eq!(stops[0].color, Color::from_linear(g.sample(0.0)));
        assert_ne!(stops[0].color, RED);
    }

    #[test]
    fn a_hit_wave_is_a_band_running_from_the_start() {
        let w = HitWave { strength: 1.0, width: 0.2, speed: 2.0, colour: Color::WHITE };
        assert_eq!(w.at(0.5, 0.25), 1.0, "the front is at the place half a gradient in after 0.25 s");
        assert_eq!(w.at(0.75, 0.25), 0.0);
        assert!(w.at(0.6, 0.25) > 0.0 && w.at(0.6, 0.25) < 1.0);
        let black = [0.0, 0.0, 0.0, 1.0];
        assert_eq!(w.apply(black, 0.5, 0.25), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(w.apply(black, 0.9, 0.25), black);
        let back = HitWave { speed: -2.0, ..w };
        assert_eq!(back.at(0.5, 0.25), 1.0);
        assert_eq!(back.at(1.0, 0.0), 1.0);
        let strong = HitWave { strength: 3.0, ..w };
        assert_eq!(strong.apply(black, 0.5, 0.25), [3.0, 3.0, 3.0, 1.0], "past 1 adds light");
        assert!((w.lasts() - 0.6).abs() < 1e-6);
        assert_eq!(HitWave { speed: 0.0, ..w }.lasts(), 10_000.0);
    }
}
