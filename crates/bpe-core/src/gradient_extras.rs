//! Companion settings of a gradient: what lays its colours out, moves them or cuts them into
//! bands, kept beside the gradient's own key so each is an ordinary number that keyframes, hit
//! pulses, Follow sound and Follow light can drive.
//!
//! A gradient setting with key `k` (a layer's keeps its prefix: `"3/colours"`) may have
//! `"<k>~<name>"` beside it in the same params, in the style of the vary keys (`"<k>~to"`).
//! Every gradient setting gets them without its struct naming them: [`crate::Scene::def_at`] and
//! [`crate::layer_def`] answer their descriptions. A companion that isn't stored reads as its
//! default, and every default leaves the gradient exactly as it is.
//!
//! Each companion names the tab of the editor's gradient row it shows on. Gradients part 1 has
//! the Blend tab's Bands and Grain; part 2 adds the Shape tab's, part 3 the Move tab's: the eight
//! moves (Cycle, Step on the beat, Breathe, Hue spin, Stops wander, Hit wave, Morph and Shape
//! spin), each companion's `group` naming the move's fold in the tab. [`apply_moves`] puts them
//! on the gradients once a frame, pure in the time and the scene's tempo.

use std::sync::LazyLock;

use crate::{
    Color, Depth, Flags, Gradient, HitWave, Mix, Morph, Moves, Params, Resolved, SettingDef, ThingId, Value, ValueType,
};

/// What separates a gradient's key from a companion's name.
pub const SEP: char = '~';

/// The tab of the editor's gradient row a companion shows on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtraTab {
    Blend,
    Shape,
    Move,
}

impl ExtraTab {
    pub const ALL: [ExtraTab; 3] = [ExtraTab::Blend, ExtraTab::Shape, ExtraTab::Move];

    pub fn label(self) -> &'static str {
        match self {
            Self::Blend => "Blend",
            Self::Shape => "Shape",
            Self::Move => "Move",
        }
    }
}

/// One companion setting: its name after the `~`, its tab, and its description (whose `key` is
/// `"~<name>"`; the full key is the gradient's key with it on the end).
#[derive(Clone, Debug, PartialEq)]
pub struct Companion {
    pub name: &'static str,
    pub tab: ExtraTab,
    pub def: SettingDef,
}

/// The most bands a gradient can be cut into: past the GPU's 1024 samples a row more show
/// nothing new, and each band costs two stops.
pub const MOST_BANDS: f64 = 1024.0;

fn number(key: &'static str, label: &'static str, help: &'static str, span: (f64, f64)) -> SettingDef {
    SettingDef {
        key,
        label,
        help,
        ty: ValueType::Number,
        default: Value::Number(0.0),
        range: None,
        span: Some(span),
        unit: "",
        group: "",
        depth: Depth::More,
        flags: Flags { keyframe: true, pulse: true, hold: false },
        when: "",
    }
}

/// A Move-tab number companion in fold `group`, with its default, span, hard lower end (when
/// it has one) and unit.
#[allow(clippy::too_many_arguments)]
fn mover(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    group: &'static str,
    default: f64,
    span: (f64, f64),
    least: Option<f64>,
    unit: &'static str,
) -> SettingDef {
    let mut d = number(key, label, help, span);
    d.default = Value::Number(default);
    d.group = group;
    d.unit = unit;
    d.range = least.map(|lo| (lo, f64::MAX));
    d
}

/// The colour Morph blends toward until one is picked: cyan to magenta, mixed Even.
pub fn morph_default() -> Gradient {
    Gradient::spread(&[Color::rgb(0x00, 0xe5, 0xff), Color::rgb(0xff, 0x2b, 0xd6)], Mix::Even, false)
}

/// The Move tab's folds, in order.
pub const CYCLE: &str = "Cycle";
pub const STEP: &str = "Step on the beat";
pub const BREATHE: &str = "Breathe";
pub const HUE: &str = "Hue spin";
pub const WANDER: &str = "Stops wander";
pub const WAVE: &str = "Hit wave";
pub const MORPH: &str = "Morph";
pub const SHAPE_MOVES: &str = "Shape moves";

/// The Move tab's companions, in order.
fn moves() -> Vec<Companion> {
    let m = |name, def| Companion { name, tab: ExtraTab::Move, def };
    let mut wave_colour =
        number("~wave_colour", "Hit wave colour", "The colour of the band a hit sends along the gradient.", (0.0, 1.0));
    wave_colour.ty = ValueType::Color;
    wave_colour.default = Value::Color(Color::WHITE);
    wave_colour.span = None;
    wave_colour.group = WAVE;
    wave_colour.flags.pulse = false;
    let mut morph_to =
        number("~morph_to", "Morph to", "The gradient the colours blend toward as Morph amount goes up.", (0.0, 1.0));
    morph_to.ty = ValueType::Gradient;
    morph_to.default = Value::Gradient(morph_default());
    morph_to.span = None;
    morph_to.group = MORPH;
    morph_to.flags.pulse = false;
    vec![
        m(
            "cycle",
            mover(
                "~cycle",
                "Cycle speed",
                "Slides the colours along the gradient, this many times a second (back the other way below 0). With Loop on it never cuts.",
                CYCLE,
                0.0,
                (-2.0, 2.0),
                None,
                "/s",
            ),
        ),
        m(
            "cycle_shift",
            mover(
                "~cycle_shift",
                "Cycle shift",
                "How far along the colours are slid before Cycle speed moves them (1 is the whole gradient).",
                CYCLE,
                0.0,
                (0.0, 1.0),
                None,
                "",
            ),
        ),
        m(
            "step_every",
            mover(
                "~step_every",
                "Step every",
                "Jumps the colours one stop along every this many beats of the scene's tempo (the beats in a bar for a bar). 0 is off.",
                STEP,
                0.0,
                (0.0, 8.0),
                Some(0.0),
                "beats",
            ),
        ),
        m(
            "step_glide",
            mover(
                "~step_glide",
                "Step glide",
                "How long each step takes to slide into place. 0 jumps.",
                STEP,
                0.15,
                (0.0, 1.0),
                Some(0.0),
                "s",
            ),
        ),
        m(
            "breathe",
            mover(
                "~breathe",
                "Breathe amount",
                "Squeezes the colours toward the middle and spreads them out again, over and over. 0 is still.",
                BREATHE,
                0.0,
                (0.0, 1.0),
                None,
                "",
            ),
        ),
        m(
            "breathe_speed",
            mover(
                "~breathe_speed",
                "Breathe speed",
                "Breaths a second (when Breathe every is 0).",
                BREATHE,
                0.5,
                (0.0, 4.0),
                None,
                "/s",
            ),
        ),
        m(
            "breathe_every",
            mover(
                "~breathe_every",
                "Breathe every",
                "One breath every this many beats of the scene's tempo. 0 uses Breathe speed instead.",
                BREATHE,
                0.0,
                (0.0, 8.0),
                Some(0.0),
                "beats",
            ),
        ),
        m(
            "hue_speed",
            mover(
                "~hue_speed",
                "Hue spin speed",
                "Turns every colour round the colour wheel together, this many turns a second.",
                HUE,
                0.0,
                (-1.0, 1.0),
                None,
                "turns/s",
            ),
        ),
        m(
            "hue_turn",
            mover(
                "~hue_turn",
                "Hue turn",
                "How far every colour is turned round the colour wheel before Hue spin speed turns them.",
                HUE,
                0.0,
                (-180.0, 180.0),
                None,
                "°",
            ),
        ),
        m(
            "wander",
            mover(
                "~wander",
                "Wander amount",
                "Lets each stop drift on its own, up to this share of the way to its neighbour (1 at most: stops never pass each other). 0 is still.",
                WANDER,
                0.0,
                (0.0, 1.0),
                None,
                "",
            ),
        ),
        m(
            "wander_speed",
            mover(
                "~wander_speed",
                "Wander speed",
                "How quickly the stops drift, in changes a second.",
                WANDER,
                0.5,
                (0.0, 4.0),
                None,
                "/s",
            ),
        ),
        m(
            "wander_seed",
            mover(
                "~wander_seed",
                "Wander seed",
                "Picks another way for the stops to drift.",
                WANDER,
                0.0,
                (0.0, 100.0),
                None,
                "",
            ),
        ),
        m(
            "wave",
            mover(
                "~wave",
                "Hit wave strength",
                "Each hit sends a bright band along the gradient from its start, on balls and shapes (their own hits). 0 is off; past 1 it adds light.",
                WAVE,
                0.0,
                (0.0, 2.0),
                None,
                "",
            ),
        ),
        m(
            "wave_width",
            mover(
                "~wave_width",
                "Hit wave width",
                "How wide the band is, as a share of the gradient.",
                WAVE,
                0.15,
                (0.01, 1.0),
                Some(0.0),
                "",
            ),
        ),
        m(
            "wave_speed",
            mover(
                "~wave_speed",
                "Hit wave speed",
                "How fast the band runs along the gradient, in gradients a second (from the end below 0).",
                WAVE,
                2.0,
                (-8.0, 8.0),
                None,
                "/s",
            ),
        ),
        m("wave_colour", wave_colour),
        m(
            "morph",
            mover(
                "~morph",
                "Morph amount",
                "Blends every colour toward the same place of the Morph to gradient: 0 none, 1 all the way.",
                MORPH,
                0.0,
                (0.0, 1.0),
                None,
                "",
            ),
        ),
        m("morph_to", morph_to),
        m(
            "spin",
            mover(
                "~spin",
                "Shape spin",
                "Turns the gradient's shape (its Turn) this many degrees a second.",
                SHAPE_MOVES,
                0.0,
                (-180.0, 180.0),
                None,
                "°/s",
            ),
        ),
    ]
}

/// Every companion, in the order the editor lists them in their tabs.
pub static COMPANIONS: LazyLock<Vec<Companion>> = LazyLock::new(|| {
    let mut bands = number(
        "~bands",
        "Bands",
        "Cuts the colours into this many flat bands, each the colour at its middle. 0 is off.",
        (0.0, 24.0),
    );
    bands.range = Some((0.0, MOST_BANDS));
    let grain = number(
        "~grain",
        "Grain",
        "Fine still noise where the colours are painted, hiding stair-steps in slow dark gradients. 0 is off.",
        (0.0, 8.0),
    );
    let mut all = vec![
        Companion { name: "bands", tab: ExtraTab::Blend, def: bands },
        Companion { name: "grain", tab: ExtraTab::Blend, def: grain },
    ];
    all.extend(moves());
    all
});

/// The companion named `name`.
pub fn companion(name: &str) -> Option<&'static Companion> {
    COMPANIONS.iter().find(|c| c.name == name)
}

/// Companion `name` of the gradient at `base`: `"colours~bands"`.
pub fn key(base: &str, name: &str) -> String {
    format!("{base}{SEP}{name}")
}

/// The gradient key a companion key belongs to and which companion it is: `"3/colours~grain"` is
/// `("3/colours", grain)`. `None` for any other key.
pub fn split(key: &str) -> Option<(&str, &'static Companion)> {
    let (base, name) = key.rsplit_once(SEP)?;
    // A companion (Morph's gradient) has no companions of its own.
    if base.is_empty() || is_companion_key(base) {
        return None;
    }
    Some((base, companion(name)?))
}

pub fn is_companion_key(key: &str) -> bool {
    split(key).is_some()
}

/// The description of companion `c` of the setting described by `base`, when that is a
/// gradient.
pub fn def_for(base: Option<&'static SettingDef>, c: &'static Companion) -> Option<&'static SettingDef> {
    base.filter(|d| d.ty == ValueType::Gradient).map(|_| &c.def)
}

/// The companions offered on gradient setting `base`, in order (none on anything else).
pub fn companions_for(base: &SettingDef) -> impl Iterator<Item = &'static Companion> + use<> {
    let gradient = base.ty == ValueType::Gradient;
    COMPANIONS.iter().filter(move |_| gradient)
}

/// A companion key's default, when it is one.
pub fn default_of(key: &str) -> Option<Value> {
    split(key).map(|(_, c)| c.def.default.clone())
}

/// A number a companion holds (0 for anything else).
fn number_of(v: &Value) -> f64 {
    match v {
        Value::Number(x) if x.is_finite() => *x,
        Value::Int(x) => *x as f64,
        _ => 0.0,
    }
}

/// Whether any companion is stored anywhere in `resolved`: the frame builder copies the
/// resolved scene to apply them only then.
pub fn any_in(resolved: &Resolved) -> bool {
    resolved.things.values().any(|t| {
        std::iter::once(&t.params)
            .chain(t.slots.values().map(|s| &s.params))
            .any(|p| p.iter().any(|(k, _)| is_companion_key(k)))
    })
}

/// The scene's beat, for the moves that keep time: one beat's length in seconds at the scene's
/// tempo and beat note, as the timeline's grid and `bpe-kit`'s `beat_at` count them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beat {
    pub seconds: f64,
}

impl Beat {
    /// The beat of the scene `resolved` holds, read from its root's Tempo and Beat note (120 BPM
    /// in quarter notes where they are missing or make no sense).
    pub fn of(resolved: &Resolved) -> Beat {
        let root = resolved.things.get(&ThingId::ROOT).map(|t| &t.params);
        let tempo = match root.and_then(|p| p.get("tempo")) {
            // As the scene settings read it: a 32-bit number.
            Some(Value::Number(x)) => *x as f32 as f64,
            Some(Value::Int(x)) => *x as f64,
            _ => 120.0,
        };
        let tempo = if tempo > 0.0 && tempo.is_finite() { tempo } else { 120.0 };
        let note = match root.and_then(|p| p.get("time_bottom")) {
            Some(Value::Choice(n)) => n.parse::<f64>().ok().filter(|n| *n > 0.0),
            _ => None,
        }
        .unwrap_or(4.0);
        Beat { seconds: 60.0 / tempo * 4.0 / note }
    }
}

impl Default for Beat {
    fn default() -> Self {
        Beat { seconds: 0.5 }
    }
}

/// Applies every gradient's companions to the gradient itself, once a frame before anything
/// draws, right after the animation (so keyed, pulsed and followed companions count): the moves
/// at `time` (Stops wander moves the stops; Cycle, Step on the beat, Breathe, Hue spin, Morph and
/// Hit wave go on [`Gradient::moves`]; Shape spin turns the shape's `~turn`), then Bands cut it
/// into flat bands, then Grain is copied onto it for the GPU. A one-colour gradient takes its
/// moved colour as its one stop. A varied gradient's other end (`~to`) gets the same, so each
/// ball's mix of the two keeps them. Pure in its inputs: the params, the time and the scene's
/// tempo; nothing drawn earlier is read.
pub fn apply_moves(resolved: &mut Resolved, time: f64) {
    let beat = Beat::of(resolved);
    let time = if time.is_finite() { time } else { 0.0 };
    for t in resolved.things.values_mut() {
        apply_in(&mut t.params, time, beat);
        for s in t.slots.values_mut() {
            apply_in(&mut s.params, time, beat);
        }
    }
}

/// The companions stored for one gradient, the rest reading as their defaults.
struct Found {
    base: String,
    stored: Vec<(&'static str, Value)>,
}

impl Found {
    fn value(&self, name: &str) -> Value {
        match self.stored.iter().find(|(n, _)| *n == name) {
            Some((_, v)) => v.clone(),
            None => companion(name).map_or(Value::Number(0.0), |c| c.def.default.clone()),
        }
    }

    fn number(&self, name: &str) -> f64 {
        number_of(&self.value(name))
    }
}

/// [`apply_moves`] on one set of params.
fn apply_in(params: &mut Params, time: f64, beat: Beat) {
    let mut found: Vec<Found> = Vec::new();
    for (k, v) in params.iter() {
        let Some((base, c)) = split(k) else { continue };
        let at = match found.iter().position(|f| f.base == base) {
            Some(i) => i,
            None => {
                found.push(Found { base: base.to_owned(), stored: Vec::new() });
                found.len() - 1
            }
        };
        found[at].stored.push((c.name, v.clone()));
    }
    for f in found {
        // Shape spin: the shape's own turn, turned on by the time (a whole turn changes nothing).
        let spin = f.number("spin");
        if spin != 0.0 {
            let turn_key = key(&f.base, "turn");
            let was = params.get(&turn_key).map_or(0.0, number_of);
            params.set(&turn_key, Value::Number(was + (spin * time).rem_euclid(360.0)));
        }
        let moves = MovesAt::of(&f, time, beat);
        for key in [f.base.clone(), crate::vary::to_key(&f.base)] {
            if let Some(Value::Gradient(g)) = params.get(&key) {
                let g = moves.apply(g);
                params.set(&key, Value::Gradient(g));
            }
        }
    }
}

/// One gradient's companions worked out at one moment.
struct MovesAt {
    time: f64,
    wander: (f32, f32, u32),
    cycle: f64,
    step: Option<f64>,
    breathe: f32,
    hue: f32,
    morph: Option<(Gradient, f32)>,
    wave: Option<HitWave>,
    bands: u32,
    grain: f32,
}

impl MovesAt {
    fn of(f: &Found, time: f64, beat: Beat) -> MovesAt {
        let fin = |x: f64| if x.is_finite() { x } else { 0.0 };
        // Cycle: slid speed × time on from the shift, kept within one length for precision.
        let cycle = fin(f.number("cycle_shift") + f.number("cycle") * time).rem_euclid(1.0);
        // Step on the beat: whole steps so far (the first on the first step's beat), the latest
        // gliding in.
        let every = f.number("step_every");
        let step = (every > 0.0).then(|| {
            let period = (every * beat.seconds).max(1e-6);
            let k = (time / period).floor();
            if k < 1.0 {
                return 0.0;
            }
            // A glide longer than a step would start the next step before this one lands.
            let glide = f.number("step_glide").min(period);
            let g = if glide > 0.0 { ((time - k * period) / glide).clamp(0.0, 1.0) } else { 1.0 };
            k - 1.0 + g * g * (3.0 - 2.0 * g)
        });
        let amount = f.number("breathe");
        let breathe = if amount != 0.0 {
            let every = f.number("breathe_every");
            let phase =
                if every > 0.0 { time / (every * beat.seconds).max(1e-6) } else { time * f.number("breathe_speed") };
            fin(amount * (std::f64::consts::TAU * phase.rem_euclid(1.0)).sin()) as f32
        } else {
            0.0
        };
        let hue = fin(f.number("hue_turn") / 360.0 + f.number("hue_speed") * time).rem_euclid(1.0) as f32;
        let amount = f.number("morph") as f32;
        let morph = (amount != 0.0).then(|| {
            let to = match f.value("morph_to") {
                Value::Gradient(g) => g,
                _ => morph_default(),
            };
            (to, amount)
        });
        let strength = f.number("wave") as f32;
        // Below 0 shows nothing (as 0), on the CPU and the GPU alike.
        let wave = (strength > 0.0).then(|| HitWave {
            strength,
            width: f.number("wave_width") as f32,
            speed: f.number("wave_speed") as f32,
            colour: match f.value("wave_colour") {
                Value::Color(c) => c,
                _ => Color::WHITE,
            },
        });
        let seed = f.number("wander_seed").round().clamp(0.0, u32::MAX as f64) as u32;
        MovesAt {
            time,
            wander: (f.number("wander") as f32, f.number("wander_speed") as f32, seed),
            cycle,
            step,
            breathe,
            hue,
            morph,
            wave,
            bands: f.number("bands").round().clamp(0.0, MOST_BANDS) as u32,
            grain: f.number("grain") as f32,
        }
    }

    /// `g` with these moves, bands and grain on it.
    fn apply(&self, g: &Gradient) -> Gradient {
        let (amount, speed, seed) = self.wander;
        let mut g = if amount != 0.0 { g.wandered(amount, speed, seed, self.time) } else { g.clone() };
        let n = g.stops.len();
        let spacing = if g.looped { n } else { n.saturating_sub(1) }.max(1) as f64;
        let shift = (self.cycle + self.step.unwrap_or(0.0) / spacing).rem_euclid(1.0);
        g.moves = Moves {
            shift: shift as f32,
            breathe: self.breathe,
            hue: self.hue,
            morph: self.morph.as_ref().map(|(to, amount)| Box::new(Morph { to: to.clone(), amount: *amount })),
            wave: self.wave,
        };
        // One colour: its moved colour, so readers of the first colour see it too.
        if n <= 1 && !g.moves.is_still() {
            let c = Color::from_linear(g.sample(0.0));
            if let Some(s) = g.stops.first_mut() {
                s.color = c;
            }
            g.moves = Moves { wave: self.wave, ..Moves::default() };
        }
        if self.bands > 0 {
            g = g.banded(self.bands);
        }
        g.grain = self.grain;
        g
    }
}

/// The longest any gradient in `params` (moved by [`apply_moves`]) shows its Hit wave after a
/// hit, in seconds: how far back a ball's last hit is looked for. 0 when none has a wave.
pub fn wave_memory(params: &Params) -> f32 {
    params
        .iter()
        .filter_map(|(_, v)| match v {
            Value::Gradient(g) => g.moves.wave.map(|w| w.lasts()),
            _ => None,
        })
        .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Address, Color, Scene, SlotId, ThingId, testkit};

    #[test]
    fn keys_split_into_the_gradient_and_its_companion() {
        assert_eq!(key("colours", "bands"), "colours~bands");
        let (base, c) = split("3/colours~grain").unwrap();
        assert_eq!((base, c.name, c.tab), ("3/colours", "grain", ExtraTab::Blend));
        assert!(split("colours~to").is_none());
        assert!(split("~bands").is_none());
        assert!(split("colours").is_none());
        assert_eq!(default_of("colours~bands"), Some(Value::Number(0.0)));
    }

    #[test]
    fn only_gradients_have_companions() {
        let bands = companion("bands").unwrap();
        let gradient = SettingDef {
            key: "colours",
            ty: ValueType::Gradient,
            default: Value::Gradient(Gradient::default()),
            ..bands.def.clone()
        };
        let gradient: &'static SettingDef = Box::leak(Box::new(gradient));
        assert_eq!(def_for(Some(gradient), bands).map(|d| d.label), Some("Bands"));
        assert!(def_for(Some(&bands.def), bands).is_none());
        assert_eq!(companions_for(gradient).count(), COMPANIONS.len());
        assert_eq!(companions_for(&bands.def).count(), 0);
    }

    fn two_colours() -> Gradient {
        Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)])
    }

    #[test]
    fn the_pass_bands_gradients_and_carries_grain() {
        let mut p = Params::default();
        p.set("colours", Value::Gradient(two_colours()));
        p.set("colours~to", Value::Gradient(two_colours()));
        p.set("colours~bands", Value::Number(3.6));
        p.set("colours~grain", Value::Number(2.0));
        p.set("other", Value::Gradient(two_colours()));
        apply_in(&mut p, 0.0, Beat::default());
        let Some(Value::Gradient(g)) = p.get("colours") else { panic!() };
        assert_eq!(g.stops.len(), 8, "3.6 rounds to 4 bands");
        assert_eq!(g.grain, 2.0);
        assert_eq!(p.get("colours~to"), p.get("colours"), "the other end gets the same");
        assert_eq!(p.get("other"), Some(&Value::Gradient(two_colours())), "a gradient without companions is left");
    }

    #[test]
    fn companions_are_settings_of_cards_and_layers() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        let layer = s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        let gradient = crate::layered_key(layer, "colours");
        let companion = Address::on(b, SlotId::Look, &key(&gradient, "bands"));
        assert_eq!(s.def_at(&r, &companion).map(|d| d.label), Some("Bands"));
        s.set_at(&r, &companion, Value::Number(5.0)).unwrap();
        assert_eq!(s.value_at(&r, &companion), Some(Value::Number(5.0)));
        let resolved = crate::resolve(&s, &r);
        assert!(any_in(&resolved));
        assert_eq!(resolved.things[&b].slots[&SlotId::Look].params.get(&companion.key), Some(&Value::Number(5.0)));
        let loaded = crate::load(&crate::save(&s), &r).unwrap();
        assert_eq!(loaded.things[&b].slots[&SlotId::Look].params.get(&companion.key), Some(&Value::Number(5.0)));
        assert_eq!(s.vary_use(&r, &companion), None, "companions don't vary across balls");
        // Keyed from its unstored default, it animates.
        s.set_at(&r, &companion, Value::Number(0.0)).unwrap();
        assert!(s.things[&b].slots[&SlotId::Look].params.get(&companion.key).is_none(), "the default isn't stored");
        s.set_key(&r, &companion, 0.0, Value::Number(0.0)).unwrap();
        s.set_key(&r, &companion, 2.0, Value::Number(4.0)).unwrap();
        let mut resolved = crate::resolve(&s, &r);
        crate::Animation::new(&s, &resolved).apply(&mut resolved, 2.0, &crate::Inputs::NONE);
        apply_moves(&mut resolved, 2.0);
        let Some(Value::Gradient(g)) = resolved.things[&b].slots[&SlotId::Look].params.get(&gradient) else { panic!() };
        assert_eq!(g.stops.len(), 8, "four bands at 2 s");
        // A number setting has none.
        let width = Address::on(b, SlotId::Look, &key(&crate::layered_key(layer, "width"), "bands"));
        assert!(s.def_at(&r, &width).is_none());
    }

    /// `colours` with these companions, through the pass at `time` at 120 BPM.
    fn moved_at(companions: &[(&str, Value)], g: Gradient, time: f64) -> Gradient {
        let mut p = Params::default();
        p.set("colours", Value::Gradient(g));
        for (name, v) in companions {
            p.set(&key("colours", name), v.clone());
        }
        apply_in(&mut p, time, Beat::default());
        match p.get("colours") {
            Some(Value::Gradient(g)) => g.clone(),
            _ => panic!(),
        }
    }

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    fn three() -> Gradient {
        Gradient::spread(&[Color::rgb(255, 0, 0), Color::rgb(0, 255, 0), Color::rgb(0, 0, 255)], Mix::Even, true)
    }

    #[test]
    fn every_move_at_its_default_leaves_the_gradient() {
        let all: Vec<(&str, Value)> =
            COMPANIONS.iter().filter(|c| c.tab == ExtraTab::Move).map(|c| (c.name, c.def.default.clone())).collect();
        assert!(all.len() >= 19);
        for time in [0.0, 1.3, 77.0] {
            assert_eq!(moved_at(&all, three(), time), three());
        }
        assert!(COMPANIONS.iter().filter(|c| c.tab == ExtraTab::Move).all(|c| !c.def.group.is_empty()));
    }

    #[test]
    fn cycle_slides_by_speed_times_time_from_the_shift() {
        let g = moved_at(&[("cycle", n(0.5)), ("cycle_shift", n(0.1))], three(), 3.0);
        assert!((g.moves.shift - 0.6).abs() < 1e-6, "{:?}", g.moves);
        let back = moved_at(&[("cycle", n(-0.25))], three(), 1.0);
        assert!((back.moves.shift - 0.75).abs() < 1e-6);
        // Long clips keep their precision.
        let late = moved_at(&[("cycle", n(0.3))], three(), 10_000.5);
        assert!((late.moves.shift - (0.3f64 * 10_000.5).rem_euclid(1.0) as f32).abs() < 1e-5);
    }

    #[test]
    fn steps_land_on_the_beat_and_glide() {
        // 120 BPM: a beat is half a second. Three looped stops: a step is a third.
        let at = |time| moved_at(&[("step_every", n(1.0)), ("step_glide", n(0.1))], three(), time).moves.shift;
        assert_eq!(at(0.0), 0.0);
        assert_eq!(at(0.49), 0.0);
        assert!((at(0.55) - 0.5 / 3.0).abs() < 1e-5, "half way through the first glide: {}", at(0.55));
        assert!((at(0.7) - 1.0 / 3.0).abs() < 1e-5);
        assert!((at(1.2) - 2.0 / 3.0).abs() < 1e-5);
        // A glide longer than a step lands as the next step starts: no jump at the beat.
        let long = |time| moved_at(&[("step_every", n(1.0)), ("step_glide", n(3.0))], three(), time).moves.shift;
        assert!((long(0.999) - long(1.0)).abs() < 1e-3, "{} {}", long(0.999), long(1.0));
        let jump = moved_at(&[("step_every", n(2.0)), ("step_glide", n(0.0))], three(), 1.0);
        assert!((jump.moves.shift - 1.0 / 3.0).abs() < 1e-5, "every two beats, no glide");
    }

    #[test]
    fn breathing_follows_its_speed_or_the_beat() {
        let at = |time, every| {
            moved_at(&[("breathe", n(0.5)), ("breathe_speed", n(1.0)), ("breathe_every", n(every))], three(), time)
                .moves
                .breathe
        };
        assert!((at(0.25, 0.0) - 0.5).abs() < 1e-5, "a quarter breath in at 1 a second");
        assert!(at(0.0, 0.0).abs() < 1e-6);
        // One breath every 2 beats (1 s at 120 BPM).
        assert!((at(0.25, 2.0) - 0.5).abs() < 1e-5);
        assert!((at(0.75, 2.0) + 0.5).abs() < 1e-5);
    }

    #[test]
    fn hue_morph_and_wave_go_on_the_value() {
        let g = moved_at(&[("hue_speed", n(0.25)), ("hue_turn", n(90.0))], three(), 2.0);
        assert!((g.moves.hue - 0.75).abs() < 1e-6);
        let g = moved_at(&[("morph", n(0.4))], three(), 0.0);
        let m = g.moves.morph.as_ref().unwrap();
        assert_eq!((m.amount, &m.to), (0.4, &morph_default()));
        let to = Gradient::solid(Color::rgb(1, 2, 3));
        let g = moved_at(&[("morph", n(1.0)), ("morph_to", Value::Gradient(to.clone()))], three(), 0.0);
        assert_eq!(g.moves.morph.as_ref().unwrap().to, to);
        let g = moved_at(&[("wave", n(1.5)), ("wave_colour", Value::Color(Color::rgb(9, 9, 9)))], three(), 0.0);
        let w = g.moves.wave.unwrap();
        assert_eq!((w.strength, w.width, w.speed, w.colour), (1.5, 0.15, 2.0, Color::rgb(9, 9, 9)));
        let mut p = Params::default();
        p.set("colours", Value::Gradient(g));
        assert!((wave_memory(&p) - 1.15 / 2.0).abs() < 1e-6);
        assert_eq!(wave_memory(&Params::default()), 0.0);
        let below = moved_at(&[("wave", n(-1.0))], three(), 0.0);
        assert!(below.moves.wave.is_none(), "a strength below 0 shows nothing");
    }

    #[test]
    fn wander_moves_the_stops_and_spin_turns_the_shape() {
        let g = moved_at(&[("wander", n(1.0)), ("wander_speed", n(1.0)), ("wander_seed", n(3.0))], three(), 2.4);
        assert_ne!(g.stops, three().stops);
        assert_eq!(g, moved_at(&[("wander", n(1.0)), ("wander_speed", n(1.0)), ("wander_seed", n(3.0))], three(), 2.4));
        let mut p = Params::default();
        p.set("colours", Value::Gradient(three()));
        p.set("colours~spin", n(90.0));
        p.set("colours~turn", n(10.0));
        apply_in(&mut p, 1.5, Beat::default());
        assert_eq!(p.get("colours~turn"), Some(&n(145.0)));
    }

    #[test]
    fn a_one_colour_gradient_takes_its_moved_colour() {
        let red = Gradient::solid(Color::rgb(255, 0, 0));
        let g = moved_at(&[("hue_turn", n(180.0))], red.clone(), 0.0);
        assert_eq!(g.stops.len(), 1);
        assert!(g.moves.is_still());
        assert_eq!(
            g.first(),
            Color::from_linear(Gradient { moves: Moves { hue: 0.5, ..Moves::default() }, ..red }.sample(0.0))
        );
    }

    #[test]
    fn the_pass_is_pure_in_time() {
        let companions = [
            ("cycle", n(0.37)),
            ("hue_speed", n(0.1)),
            ("wander", n(0.8)),
            ("breathe", n(0.3)),
            ("step_every", n(1.0)),
            ("bands", n(6.0)),
        ];
        let at3 = moved_at(&companions, three(), 3.0);
        let _ = moved_at(&companions, three(), 1.0);
        assert_eq!(moved_at(&companions, three(), 3.0), at3, "scrubbing back and forth");
        assert_ne!(moved_at(&companions, three(), 1.0), at3);
        assert!(at3.moves.is_still(), "bands bake the moves in");
    }

    #[test]
    fn colour_and_gradient_companions_save_and_load() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        let layer = s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        let gradient = crate::layered_key(layer, "colours");
        let to = Address::on(b, SlotId::Look, &key(&gradient, "morph_to"));
        let colour = Address::on(b, SlotId::Look, &key(&gradient, "wave_colour"));
        assert_eq!(s.def_at(&r, &to).map(|d| d.ty), Some(ValueType::Gradient));
        s.set_at(&r, &to, Value::Gradient(three())).unwrap();
        s.set_at(&r, &colour, Value::Color(Color::rgb(1, 2, 3))).unwrap();
        let loaded = crate::load(&crate::save(&s), &r).unwrap();
        let params = &loaded.things[&b].slots[&SlotId::Look].params;
        assert_eq!(params.get(&to.key), Some(&Value::Gradient(three())));
        assert_eq!(params.get(&colour.key), Some(&Value::Color(Color::rgb(1, 2, 3))));
        let deeper = Address::on(b, SlotId::Look, &key(&to.key, "bands"));
        assert!(s.def_at(&r, &deeper).is_none());
    }

    #[test]
    fn a_companion_gradient_has_no_companions() {
        assert!(split("colours~morph_to").is_some());
        assert!(split("colours~morph_to~bands").is_none());
        assert_eq!(default_of("colours~morph_to"), Some(Value::Gradient(morph_default())));
    }

    #[test]
    fn the_beat_comes_from_the_scene_s_tempo() {
        let r = testkit::registry();
        let s = Scene::empty(&r);
        let mut resolved = crate::resolve(&s, &r);
        assert_eq!(Beat::of(&resolved).seconds, 0.5, "120 BPM in quarters when nothing says");
        let root = &mut resolved.things.get_mut(&ThingId::ROOT).unwrap().params;
        root.set("tempo", n(90.0));
        root.set("time_bottom", Value::Choice("8".into()));
        assert!((Beat::of(&resolved).seconds - 60.0 / 90.0 * 0.5).abs() < 1e-12);
    }

    /// Follow sound as a level rising by 0.1 a second.
    struct Rising;

    impl crate::SoundLevels for Rising {
        fn level(&self, _: ThingId, _: &crate::SoundSource, time: f64) -> f32 {
            (0.1 * time) as f32
        }
    }

    #[test]
    fn cycle_speed_can_follow_sound() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        let layer = s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        let gradient = crate::layered_key(layer, "colours");
        s.set_at(&r, &Address::on(b, SlotId::Look, &gradient), Value::Gradient(three())).unwrap();
        let speed = Address::on(b, SlotId::Look, &key(&gradient, "cycle"));
        let source = crate::PulseSource::Sound { shape: ThingId(9), source: crate::SoundSource::Level };
        s.set_pulse(&r, &speed, source, n(1.0), 0.25).unwrap();
        let shift_at = |time: f64| {
            let mut resolved = crate::resolve(&s, &r);
            let inputs = crate::Inputs { sound: Some(&Rising), ..crate::Inputs::NONE };
            crate::Animation::new(&s, &resolved).apply(&mut resolved, time, &inputs);
            apply_moves(&mut resolved, time);
            match resolved.things[&b].slots[&SlotId::Look].params.get(&gradient) {
                Some(Value::Gradient(g)) => g.moves.shift,
                _ => panic!(),
            }
        };
        assert_eq!(shift_at(0.0), 0.0);
        // At 2 s the sound is at 0.2: Cycle speed 0.2, so 0.4 along.
        assert!((shift_at(2.0) - 0.4).abs() < 1e-4, "{}", shift_at(2.0));
    }
}
