//! Captions: words drawn over the video, such as a "Can it escape?" hook, a ball counter or a
//! timer.
//!
//! A caption's text is a template. Words in braces such as `{balls}` are filled in with live
//! values on every frame (see [`TOKENS`]). Each caption shows between two times, animates in and
//! out, and can pulse when balls hit something or on the song's beat. Everything here is plain
//! data and arithmetic, so the preview and the exported video show exactly the same captions.

use crate::{Color, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Live values a caption's text can show, as (token, what it shows).
pub const TOKENS: &[(&str, &str)] = &[
    ("{balls}", "How many balls there are now"),
    ("{hits}", "How many times balls have hit a wall or peg so far"),
    ("{escapes}", "How many rings balls have escaped so far"),
    ("{rings}", "How many rings the arena has"),
    ("{rings_left}", "How many rings are still unbroken"),
    ("{time}", "Time since the start, like 0:12"),
    ("{countdown}", "Time left until the end, like 0:11"),
    ("{counter:score}", "A counter that rules add to. Change \"score\" to the counter's name."),
    ("{song}", "The song's name, if the scene plays one"),
    (
        "{seed}",
        "The variation's seed, for series like \"Day 12\". The explorer and batches write the number in; before that it shows the Sound tab's seed",
    ),
];

/// Ready-made captions: (text, style).
pub const TEMPLATES: &[(&str, CaptionStyle)] = &[
    ("Can it escape?", CaptionStyle::Hook),
    ("{balls} balls", CaptionStyle::Counter),
    ("Ring {escapes}/{rings}", CaptionStyle::Counter),
    ("{time}", CaptionStyle::Timer),
    ("{countdown}", CaptionStyle::Timer),
    ("Day {seed}", CaptionStyle::Hook),
    ("Wait for it...", CaptionStyle::Subtitle),
    ("Every bounce adds a ball", CaptionStyle::Subtitle),
    ("{hits} bounces", CaptionStyle::Counter),
    ("How many rings will break?", CaptionStyle::Hook),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Caption {
    /// A name to tell captions apart in the editor and for rules to refer to. Empty uses the text.
    pub name: String,
    pub enabled: bool,
    /// The words, with live values in braces such as `{balls}`. A new line starts a new line.
    pub text: String,
    /// The preset the caption was made from. Changing it moves and restyles the caption.
    pub style: CaptionStyle,
    pub font: Font,
    /// Height of capital letters, in pixels of the video.
    pub size: f32,
    pub color: Color,
    /// Width of the line around the letters, in pixels. 0 for none.
    pub outline: f32,
    pub outline_color: Color,
    /// Brightness of the soft light around the letters, 0 to 1.
    pub glow: f32,
    /// Where the caption sits: the middle of its first line, in pixels from the centre of the
    /// frame (y points down).
    pub position: Vec2,
    pub align: Align,
    /// Seconds from the start when the caption appears.
    pub show_from: f32,
    /// Seconds from the start when it disappears. 0 keeps it until the end.
    pub show_to: f32,
    pub anim_in: Anim,
    pub anim_out: Anim,
    /// Seconds each animation takes.
    pub anim_seconds: f32,
    pub react: React,
    /// Draw the caption before the shader effects, so they bend and colour it too. Off keeps it
    /// sharp and readable on top.
    pub under_effects: bool,
}

/// Ready-made looks that set a caption's place, size and font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptionStyle {
    /// Big words near the top that ask a question.
    #[default]
    Hook,
    /// Huge words in the middle of the frame.
    BigCentre,
    /// A number with a word, below the arena.
    Counter,
    /// A clock in a corner.
    Timer,
    /// A short line near the bottom.
    Subtitle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Font {
    /// A tall, bold font for hooks.
    #[default]
    Display,
    /// A soft, rounded font.
    Rounded,
    /// Every digit the same width, so counters and timers don't wobble.
    Mono,
    /// A .ttf or .otf file. A relative path is looked up next to the scene file.
    File(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Align {
    Left,
    #[default]
    Centre,
    Right,
}

/// How a caption appears or disappears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Anim {
    /// Just there.
    None,
    #[default]
    Fade,
    /// Grows from nothing, overshoots a little and settles.
    Pop,
    /// Slides up into place.
    Slide,
    /// Letters appear one at a time.
    Typewriter,
    /// Drops in from above and bounces.
    Bounce,
    /// Flickers and jumps sideways.
    Glitch,
}

/// What makes a caption pulse while it is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum React {
    #[default]
    None,
    /// Grows a little each time a ball hits something.
    PulseOnHit,
    /// Grows a little on each beat of the song.
    PulseOnBeat,
}

/// Everything a caption needs to know about the moment it is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextValues {
    /// Seconds from the start.
    pub time: f32,
    /// Length of the video in seconds.
    pub duration: f32,
    pub balls: u32,
    pub hits: u32,
    pub escapes: u32,
    pub rings: u32,
    /// Counters that rules add to, by name.
    pub counters: BTreeMap<String, i64>,
    /// File name of the song, without its extension.
    pub song: String,
    pub seed: u64,
    /// 0 to 1, jumps on each hit and fades quickly.
    pub hit_pulse: f32,
    /// 0 to 1, jumps on each beat and fades quickly.
    pub beat_pulse: f32,
    /// Caption text set by rules, by caption name, replacing the caption's own text.
    pub texts: BTreeMap<String, String>,
    /// Captions shown by rules, by name, with the time they were shown.
    pub shown: BTreeMap<String, f32>,
}

/// How a caption looks at one moment, before it is laid out in letters.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptionState {
    /// The text with live values filled in.
    pub text: String,
    /// How many characters of `text` are visible (fewer while typing in or out).
    pub visible: usize,
    /// 0 to 1.
    pub alpha: f32,
    /// Size multiplier from animations and pulses.
    pub scale: f32,
    /// Pixels the caption is moved from its place.
    pub offset: Vec2,
    /// 0 to 1: how strongly letters jump sideways and split into colours.
    pub glitch: f32,
    /// A number that changes a few times a second while glitching, for picking random jumps.
    pub glitch_seed: u32,
}

#[allow(clippy::derivable_impls)] // Not derivable: the default caption has text and a style.
impl Default for Caption {
    fn default() -> Self {
        Self { name: String::new(), ..Self::new("Can it escape?", CaptionStyle::Hook) }
    }
}

impl Caption {
    /// A caption with `text`, laid out and styled by `style`.
    pub fn new(text: &str, style: CaptionStyle) -> Self {
        let mut caption = Self {
            name: String::new(),
            enabled: true,
            text: text.into(),
            style,
            font: Font::Display,
            size: 96.0,
            color: Color::WHITE,
            outline: 6.0,
            outline_color: Color::rgb(0x0a, 0x0a, 0x14),
            glow: 0.25,
            position: Vec2::ZERO,
            align: Align::Centre,
            show_from: 0.0,
            show_to: 0.0,
            anim_in: Anim::Pop,
            anim_out: Anim::Fade,
            anim_seconds: 0.4,
            react: React::None,
            under_effects: false,
        };
        caption.apply_style(style);
        caption.name = caption.default_name();
        caption
    }

    /// A name made from the text, such as "Can it escape?" or "balls counter".
    pub fn default_name(&self) -> String {
        let words: String = self.text.chars().take(24).collect();
        if words.trim().is_empty() { "Caption".into() } else { words.trim().to_string() }
    }

    /// Moves and restyles the caption to match `style`, keeping its text, colours and times.
    pub fn apply_style(&mut self, style: CaptionStyle) {
        self.style = style;
        let (position, size, font, anim_in, react) = match style {
            CaptionStyle::Hook => (Vec2::new(0.0, -700.0), 96.0, Font::Display, Anim::Pop, React::None),
            CaptionStyle::BigCentre => (Vec2::new(0.0, 0.0), 180.0, Font::Display, Anim::Pop, React::PulseOnHit),
            CaptionStyle::Counter => (Vec2::new(0.0, 650.0), 84.0, Font::Rounded, Anim::Slide, React::PulseOnHit),
            CaptionStyle::Timer => (Vec2::new(0.0, -820.0), 64.0, Font::Mono, Anim::Fade, React::None),
            CaptionStyle::Subtitle => (Vec2::new(0.0, 780.0), 56.0, Font::Rounded, Anim::Typewriter, React::None),
        };
        self.position = position;
        self.size = size;
        self.font = font;
        self.anim_in = anim_in;
        self.react = react;
        self.align = Align::Centre;
        self.outline = (size * 0.06).round();
    }

    /// Moves the caption up or down, as little as it can, so it doesn't sit on top of one of
    /// `others` that is on screen at the same time. It stays at least its own size inside
    /// `half_height` of the middle; if there is no clear spot it stays where it is.
    pub fn keep_clear_of(&mut self, others: &[Caption], half_height: f32) {
        let open = |c: &Caption| if c.show_to > c.show_from { c.show_to } else { f32::MAX };
        let together: Vec<&Caption> = others
            .iter()
            .filter(|o| o.enabled && o.show_from < open(self) && self.show_from < open(o))
            .filter(|o| (o.position.x - self.position.x).abs() < 300.0 || o.align != self.align)
            .collect();
        let clear = |y: f32| together.iter().all(|o| (o.position.y - y).abs() >= (o.size + self.size) * 0.6);
        let step = self.size * 0.6;
        let limit = (half_height - self.size).max(0.0);
        let start = self.position.y;
        for i in 0..40 {
            let offset = (i / 2 + i % 2) as f32 * step * if i % 2 == 1 { 1.0 } else { -1.0 };
            let y = start + offset;
            if y.abs() <= limit && clear(y) {
                self.position.y = y;
                return;
            }
        }
    }

    /// The seconds the caption is shown, as (from, to).
    pub fn span(&self, duration: f32) -> (f32, f32) {
        let from = self.show_from.max(0.0);
        let to = if self.show_to > from { self.show_to } else { duration.max(from) };
        (from, to)
    }

    /// How the caption looks at `values.time`, or `None` while it is hidden.
    pub fn state_at(&self, values: &TextValues) -> Option<CaptionState> {
        if !self.enabled {
            return None;
        }
        let t = values.time;
        let (mut from, to) = self.span(values.duration);
        // A rule can show the caption from the moment it fires.
        if let Some(&shown) = values.shown.get(&self.name) {
            from = from.max(shown).min(to);
            if t < shown {
                return None;
            }
        }
        if t < from || t > to {
            return None;
        }
        let text = fill_caption(values.texts.get(&self.name).unwrap_or(&self.text), values);
        let length = text.chars().count();
        let seconds = self.anim_seconds.max(0.01);
        let entering = ((t - from) / seconds).clamp(0.0, 1.0);
        // A caption kept until the end doesn't animate out.
        let leaving = if self.show_to > from { ((to - t) / seconds).clamp(0.0, 1.0) } else { 1.0 };
        let mut state = CaptionState {
            text,
            visible: length,
            alpha: 1.0,
            scale: 1.0,
            offset: Vec2::ZERO,
            glitch: 0.0,
            glitch_seed: (t * 24.0) as u32,
        };
        self.animate(&mut state, self.anim_in, entering, length, 1.0);
        self.animate(&mut state, self.anim_out, leaving, length, -1.0);
        let pulse = match self.react {
            React::None => 0.0,
            React::PulseOnHit => values.hit_pulse,
            React::PulseOnBeat => values.beat_pulse,
        };
        state.scale *= 1.0 + 0.18 * pulse.clamp(0.0, 1.0);
        (state.alpha > 0.0 && state.scale > 0.0).then_some(state)
    }

    /// Applies one animation at `progress` (0 hidden, 1 fully shown). `direction` is 1 coming
    /// in and -1 going out, so slides and bounces leave the way that looks natural.
    fn animate(&self, state: &mut CaptionState, anim: Anim, progress: f32, length: usize, direction: f32) {
        if progress >= 1.0 {
            return;
        }
        let p = progress;
        let smooth = p * p * (3.0 - 2.0 * p);
        match anim {
            Anim::None => {}
            Anim::Fade => state.alpha *= smooth,
            Anim::Pop => {
                // Back-out easing: overshoots to about 110% and settles.
                let s = 1.70158 * 1.2;
                let q = p - 1.0;
                state.scale *= (1.0 + (s + 1.0) * q * q * q + s * q * q).max(0.0);
                state.alpha *= (p * 4.0).min(1.0);
            }
            Anim::Slide => {
                state.offset.y += (1.0 - smooth) * self.size * 1.2 * direction;
                state.alpha *= smooth;
            }
            Anim::Typewriter => {
                state.visible = state.visible.min((p * length as f32).floor() as usize);
            }
            Anim::Bounce => {
                // Drops from above and bounces twice, each smaller.
                let fall = 1.0 - p;
                let bounce = (p * std::f32::consts::PI * 2.5).cos().abs() * fall * fall;
                state.offset.y -= bounce * self.size * 2.5 * direction;
                state.alpha *= (p * 6.0).min(1.0);
            }
            Anim::Glitch => {
                state.glitch = state.glitch.max(1.0 - p);
                // Flickers on and off, more often shown as it settles.
                let flicker = hash(state.glitch_seed.wrapping_add(17)) < p * 1.2;
                if !flicker {
                    state.alpha *= 0.15;
                }
            }
        }
    }
}

/// The text with every `{token}` replaced by its live value. Unknown tokens are left as typed.
pub fn fill_caption(template: &str, values: &TextValues) -> String {
    let mut out = String::with_capacity(template.len() + 8);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let token = &after[..close];
        match value(token, values) {
            Some(text) => out.push_str(&text),
            None => {
                out.push('{');
                out.push_str(token);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

fn value(token: &str, values: &TextValues) -> Option<String> {
    let token = token.trim();
    Some(match token {
        "balls" => values.balls.to_string(),
        "hits" => values.hits.to_string(),
        "escapes" => values.escapes.to_string(),
        "rings" => values.rings.to_string(),
        "rings_left" => values.rings.saturating_sub(values.escapes).to_string(),
        "time" => clock(values.time),
        "countdown" => clock((values.duration - values.time).max(0.0).ceil()),
        "song" => values.song.clone(),
        "seed" => values.seed.to_string(),
        _ => {
            let name = token.strip_prefix("counter:")?.trim();
            values.counters.get(name).copied().unwrap_or(0).to_string()
        }
    })
}

/// Whole seconds as m:ss.
fn clock(seconds: f32) -> String {
    let whole = seconds.max(0.0).floor() as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// A repeatable number from 0 up to 1.
fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_caption_moves_off_one_already_there() {
        let ring = Caption::new("Ring {escapes}/{rings}", CaptionStyle::Counter);
        let mut hits = Caption::new("{hits} bounces", CaptionStyle::Counter);
        hits.keep_clear_of(std::slice::from_ref(&ring), 960.0);
        assert!((hits.position.y - ring.position.y).abs() >= (hits.size + ring.size) * 0.6);
        assert!(hits.position.y.abs() <= 960.0 - hits.size);
        // A caption shown at another time can stay put.
        let mut later = Caption::new("{hits} bounces", CaptionStyle::Counter);
        later.show_from = 5.0;
        let mut early = ring.clone();
        early.show_to = 4.0;
        later.keep_clear_of(&[early], 960.0);
        assert_eq!(later.position, ring.position);
    }

    fn values(time: f32) -> TextValues {
        TextValues { time, duration: 30.0, balls: 12, hits: 340, escapes: 3, rings: 10, seed: 42, ..Default::default() }
    }

    #[test]
    fn tokens_fill_in() {
        let v = values(72.5);
        assert_eq!(fill_caption("{balls} balls", &v), "12 balls");
        assert_eq!(fill_caption("Ring {escapes}/{rings}, {rings_left} left", &v), "Ring 3/10, 7 left");
        assert_eq!(fill_caption("{time}", &v), "1:12");
        assert_eq!(fill_caption("Day {seed}", &v), "Day 42");
        assert_eq!(fill_caption("{ hits }", &v), "340");
    }

    #[test]
    fn countdown_never_goes_below_zero() {
        assert_eq!(fill_caption("{countdown}", &values(0.0)), "0:30");
        assert_eq!(fill_caption("{countdown}", &values(29.2)), "0:01");
        assert_eq!(fill_caption("{countdown}", &values(40.0)), "0:00");
    }

    #[test]
    fn unknown_and_broken_tokens_stay_as_typed() {
        let v = values(0.0);
        assert_eq!(fill_caption("{nope} and {balls", &v), "{nope} and {balls");
        assert_eq!(fill_caption("", &v), "");
        assert_eq!(fill_caption("{}", &v), "{}");
        assert_eq!(fill_caption("}{balls}{", &v), "}12{");
    }

    #[test]
    fn counters_default_to_zero() {
        let mut v = values(0.0);
        assert_eq!(fill_caption("{counter:score}", &v), "0");
        v.counters.insert("score".into(), 7);
        assert_eq!(fill_caption("Score {counter: score }", &v), "Score 7");
    }

    #[test]
    fn unicode_text_fills_safely() {
        let v = values(0.0);
        assert_eq!(fill_caption("⚽ {balls} ⚽ {", &v), "⚽ 12 ⚽ {");
    }

    #[test]
    fn shown_only_between_its_times() {
        let caption = Caption { show_from: 2.0, show_to: 5.0, ..Caption::new("Hi", CaptionStyle::Hook) };
        assert!(caption.state_at(&values(1.9)).is_none());
        assert!(caption.state_at(&values(3.0)).is_some());
        assert!(caption.state_at(&values(5.1)).is_none());
        let disabled = Caption { enabled: false, ..caption };
        assert!(disabled.state_at(&values(3.0)).is_none());
    }

    #[test]
    fn kept_until_the_end_when_show_to_is_zero() {
        let caption = Caption::new("Hi", CaptionStyle::Hook);
        let state = caption.state_at(&values(29.99)).unwrap();
        assert_eq!(state.alpha, 1.0);
    }

    #[test]
    fn animations_settle_to_the_plain_caption() {
        for anim in [Anim::None, Anim::Fade, Anim::Pop, Anim::Slide, Anim::Typewriter, Anim::Bounce, Anim::Glitch] {
            let caption = Caption {
                anim_in: anim,
                anim_out: anim,
                show_to: 10.0,
                ..Caption::new("{balls}", CaptionStyle::Counter)
            };
            let state = caption.state_at(&values(5.0)).unwrap();
            assert_eq!(state.text, "12");
            assert_eq!(state.visible, 2, "{anim:?}");
            assert_eq!((state.alpha, state.offset, state.glitch), (1.0, Vec2::ZERO, 0.0), "{anim:?}");
            assert!((state.scale - 1.0).abs() < 1e-6, "{anim:?}");
            // Partway in, every animation shows something different from the settled caption.
            if anim != Anim::None {
                let early = caption.state_at(&values(0.1));
                assert!(early.is_none_or(|e| e != state), "{anim:?}");
            }
        }
    }

    #[test]
    fn typewriter_reveals_letters_in_order() {
        let caption =
            Caption { anim_in: Anim::Typewriter, anim_seconds: 1.0, ..Caption::new("abcd", CaptionStyle::Subtitle) };
        let visible = |t| caption.state_at(&values(t)).unwrap().visible;
        assert_eq!(visible(0.0), 0);
        assert_eq!(visible(0.5), 2);
        assert_eq!(visible(2.0), 4);
    }

    #[test]
    fn pop_overshoots_then_settles() {
        let caption = Caption { anim_in: Anim::Pop, anim_seconds: 1.0, ..Caption::default() };
        let scales: Vec<f32> =
            (0..=10).map(|i| caption.state_at(&values(i as f32 * 0.1)).map_or(0.0, |s| s.scale)).collect();
        assert!(scales.iter().any(|&s| s > 1.05));
        assert!((scales[10] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pulses_grow_the_caption() {
        let caption = Caption { react: React::PulseOnHit, anim_in: Anim::None, ..Caption::default() };
        let mut v = values(5.0);
        v.hit_pulse = 1.0;
        assert!(caption.state_at(&v).unwrap().scale > 1.1);
        v.hit_pulse = 0.0;
        v.beat_pulse = 1.0;
        assert_eq!(caption.state_at(&v).unwrap().scale, 1.0);
    }

    #[test]
    fn rules_can_show_and_rewrite_captions() {
        let caption = Caption { name: "boom".into(), anim_in: Anim::None, ..Caption::new("Hi", CaptionStyle::Hook) };
        let mut v = values(3.0);
        v.shown.insert("boom".into(), 4.0);
        assert!(caption.state_at(&v).is_none());
        v.time = 4.5;
        v.texts.insert("boom".into(), "{balls} now".into());
        assert_eq!(caption.state_at(&v).unwrap().text, "12 now");
    }

    #[test]
    fn zero_length_and_backwards_spans_are_safe() {
        let caption = Caption { show_from: 5.0, show_to: 5.0, anim_seconds: 0.0, ..Caption::default() };
        assert_eq!(caption.span(30.0), (5.0, 30.0));
        let caption = Caption { show_from: 40.0, ..Caption::default() };
        assert_eq!(caption.span(30.0), (40.0, 40.0));
        assert!(caption.state_at(&values(30.0)).is_none());
    }

    #[test]
    fn every_template_and_style_round_trips() {
        for &(text, style) in TEMPLATES {
            let caption = Caption::new(text, style);
            let ron = ron::to_string(&caption).unwrap();
            assert_eq!(ron::from_str::<Caption>(&ron).unwrap(), caption);
        }
        let sparse: Caption = ron::from_str("(text: \"Hi\")").unwrap();
        assert_eq!(sparse.text, "Hi");
        assert!(sparse.enabled);
    }
}
