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
//! the Blend tab's Bands and Grain; part 2 the Shape tab's (offered only on gradients that fill
//! an area, [`crate::Flags::area`]: the form, the place for sites without one of their own, and
//! each form's own numbers); part 3 the Move tab's. A companion's `when` names other companions
//! of the same gradient as `~<name>` (`"~form=spiral"`): read it with [`when_of`].

use std::sync::LazyLock;

use crate::gradient_form::{GradientForm, Layout, Repeat, RippleFrom, defaults};
use crate::{Area, Choice, Depth, Flags, Gradient, Params, Resolved, SettingDef, Value, ValueType};

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
    /// Offered only where the gradient takes its place from its companions
    /// ([`Area::Fills`]): Turn, Centre, Size and Beyond the last colour.
    pub placing: bool,
}

/// The most bands a gradient can be cut into: past the GPU's 1024 samples a row more show
/// nothing new, and each band costs two stops.
pub const MOST_BANDS: f64 = 1024.0;

fn def(key: &'static str, label: &'static str, help: &'static str, ty: ValueType, default: Value) -> SettingDef {
    SettingDef {
        key,
        label,
        help,
        ty,
        default,
        range: None,
        span: None,
        unit: "",
        group: "",
        depth: Depth::More,
        flags: Flags { keyframe: false, pulse: false, hold: false, area: Area::No },
        when: "",
    }
}

fn number(key: &'static str, label: &'static str, help: &'static str, span: (f64, f64)) -> SettingDef {
    let mut d = def(key, label, help, ValueType::Number, Value::Number(0.0));
    d.span = Some(span);
    d.flags.keyframe = true;
    d.flags.pulse = true;
    d
}

/// A Shape tab number: its default, span, hard range, unit and which forms show it.
#[allow(clippy::too_many_arguments)]
fn shape_number(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    default: f32,
    span: (f64, f64),
    range: Option<(f64, f64)>,
    unit: &'static str,
    when: &'static str,
) -> SettingDef {
    let mut d = number(key, label, help, span);
    d.default = Value::Number(default as f64);
    d.range = range;
    d.unit = unit;
    d.when = when;
    d
}

/// The description of `~form` whose default is `form` (every site's default is one of these).
static FORM_DEFS: LazyLock<Vec<SettingDef>> = LazyLock::new(|| {
    GradientForm::ALL
        .iter()
        .map(|f| {
            def(
                "~form",
                "Shape",
                "How the colours are laid out over the area.",
                ValueType::Choice(GradientForm::NAMES),
                Value::Choice(f.name().to_owned()),
            )
        })
        .collect()
});

fn form_def(form: GradientForm) -> &'static SettingDef {
    let at = GradientForm::ALL.iter().position(|f| *f == form).unwrap_or(0);
    &FORM_DEFS[at]
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
    let blend = |name, def| Companion { name, tab: ExtraTab::Blend, def, placing: false };
    let shape = |name, def| Companion { name, tab: ExtraTab::Shape, def, placing: false };
    let placing = |name, def| Companion { name, tab: ExtraTab::Shape, def, placing: true };
    let mut turn = shape_number(
        "~turn",
        "Turn",
        "Which way the shape points, in degrees: 90 runs straight colours from top to bottom.",
        defaults::TURN,
        (-180.0, 180.0),
        None,
        "°",
        "",
    );
    turn.depth = Depth::Simple;
    let mut centre = def(
        "~centre",
        "Centre",
        "Where the shape's middle sits, from the area's middle (1 is its edge).",
        ValueType::Vec2,
        Value::Vec2([0.0, 0.0]),
    );
    centre.flags.keyframe = true;
    let size = shape_number(
        "~size",
        "Size",
        "How big the shape is: 1 fills the area once, less repeats it (with Beyond the last colour).",
        defaults::SIZE,
        (0.1, 4.0),
        Some((1e-3, f64::INFINITY)),
        "",
        "",
    );
    let repeat = def(
        "~repeat",
        "Beyond the last colour",
        "What the colours do past the shape's end.",
        ValueType::Choice(Repeat::NAMES),
        Value::Choice(Repeat::Clamp.name().to_owned()),
    );
    let mut focus = def(
        "~focus",
        "Focus",
        "Where the bright point sits, from the middle (1 is the edge).",
        ValueType::Vec2,
        Value::Vec2(defaults::FOCUS),
    );
    focus.flags.keyframe = true;
    focus.when = "~form=off_centre";
    let mut points = def(
        "~points",
        "Points",
        "Where each colour sits, one point per stop: drag them in the view.",
        ValueType::Points,
        Value::Points(Vec::new()),
    );
    points.when = "~form=free_points";
    let mut from = def(
        "~from",
        "From",
        "Which things the ripples spread out from.",
        ValueType::Choice(RippleFrom::NAMES),
        Value::Choice(RippleFrom::Everything.name().to_owned()),
    );
    from.when = "~form=ripples";
    let n = shape_number;
    vec![
        blend("bands", bands),
        blend("grain", grain),
        shape("form", form_def(GradientForm::Straight).clone()),
        placing("turn", turn),
        placing("centre", centre),
        placing("size", size),
        placing("repeat", repeat),
        shape(
            "sides",
            n(
                "~sides",
                "Sides",
                "How many corners the polygon or star has.",
                defaults::SIDES,
                (3.0, 12.0),
                Some((3.0, 4096.0)),
                "",
                "~form=polygon",
            ),
        ),
        shape(
            "star",
            n(
                "~star",
                "Star depth",
                "How far in the points between the corners go: 0 is a polygon, 1 a sharp star.",
                defaults::STAR,
                (0.0, 1.0),
                None,
                "",
                "~form=polygon",
            ),
        ),
        shape(
            "arms",
            n(
                "~arms",
                "Arms",
                "How many arms the spiral has.",
                defaults::ARMS,
                (1.0, 8.0),
                Some((1.0, f64::INFINITY)),
                "",
                "~form=spiral",
            ),
        ),
        shape(
            "twist",
            n(
                "~twist",
                "Twist",
                "How tightly the spiral winds: turns from the middle to the edge. Below 0 winds the other way.",
                defaults::TWIST,
                (-4.0, 4.0),
                None,
                "",
                "~form=spiral",
            ),
        ),
        shape(
            "mirrors",
            n(
                "~mirrors",
                "Mirrors",
                "How many times the sweep is folded back round the middle.",
                defaults::MIRRORS,
                (1.0, 16.0),
                Some((1.0, f64::INFINITY)),
                "",
                "~form=kaleido",
            ),
        ),
        shape("focus", focus),
        shape(
            "stretch",
            n(
                "~stretch",
                "Stretch",
                "How much wider than tall the round is: 1 is a circle.",
                defaults::STRETCH,
                (0.25, 4.0),
                Some((0.001, f64::INFINITY)),
                "",
                "~form=off_centre",
            ),
        ),
        shape(
            "stretch_turn",
            n(
                "~stretch_turn",
                "Stretch turn",
                "Which way the stretch runs, in degrees.",
                defaults::STRETCH_TURN,
                (-180.0, 180.0),
                None,
                "°",
                "~form=off_centre",
            ),
        ),
        shape(
            "count",
            n(
                "~count",
                "Count",
                "How many waves run across the area.",
                defaults::COUNT,
                (0.0, 12.0),
                None,
                "",
                "~form=waves",
            ),
        ),
        shape(
            "height",
            n(
                "~height",
                "Height",
                "How far the waves bend the colours, as a share of the gradient.",
                defaults::HEIGHT,
                (0.0, 1.0),
                None,
                "",
                "~form=waves",
            ),
        ),
        shape(
            "flow",
            n(
                "~flow",
                "Flow",
                "How fast the waves or ripples move along, in times per second.",
                defaults::FLOW,
                (-2.0, 2.0),
                None,
                "/s",
                "~form=waves|ripples",
            ),
        ),
        shape(
            "detail",
            n(
                "~detail",
                "Detail",
                "How many layers of finer noise are added.",
                defaults::DETAIL,
                (1.0, 8.0),
                Some((1.0, 8.0)),
                "",
                "~form=marble|turbulence",
            ),
        ),
        shape(
            "bend",
            n(
                "~bend",
                "Bend",
                "How far the noise bends the colours, as a share of the gradient.",
                defaults::BEND,
                (0.0, 2.0),
                None,
                "",
                "~form=marble",
            ),
        ),
        shape(
            "seed",
            n(
                "~seed",
                "Seed",
                "Picks a different pattern of noise.",
                defaults::SEED,
                (0.0, 100.0),
                None,
                "",
                "~form=marble|turbulence",
            ),
        ),
        shape(
            "softness",
            n(
                "~softness",
                "Softness",
                "How sharply each colour holds near its point: low blends widely, high keeps cells.",
                defaults::SOFTNESS,
                (0.5, 8.0),
                Some((0.1, 64.0)),
                "",
                "~form=free_points",
            ),
        ),
        shape("points", points),
        shape(
            "spacing",
            n(
                "~spacing",
                "Spacing",
                "How far apart the ripples are, in pixels.",
                defaults::SPACING,
                (4.0, 400.0),
                Some((0.5, f64::INFINITY)),
                "px",
                "~form=ripples",
            ),
        ),
        shape("from", from),
        shape(
            "speed",
            n(
                "~speed",
                "Speed",
                "How fast the turbulence drifts.",
                defaults::SPEED,
                (0.0, 2.0),
                None,
                "",
                "~form=turbulence",
            ),
        ),
    ]
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
    if base.is_empty() {
        return None;
    }
    Some((base, companion(name)?))
}

pub fn is_companion_key(key: &str) -> bool {
    split(key).is_some()
}

/// Whether gradient setting `base` offers companion `c`: every gradient the Blend and Move tabs,
/// a gradient that fills an area the Shape tab (the placing ones only where it has no place of
/// its own).
pub fn offers(base: &SettingDef, c: &Companion) -> bool {
    if base.ty != ValueType::Gradient {
        return false;
    }
    match c.tab {
        ExtraTab::Shape => base.flags.area.is_area() && (!c.placing || base.flags.area.fills()),
        _ => true,
    }
}

/// The description of companion `c` of the setting described by `base`, when it offers it.
/// `~form`'s default is the site's own form.
pub fn def_for(base: Option<&'static SettingDef>, c: &'static Companion) -> Option<&'static SettingDef> {
    let base = base.filter(|d| offers(d, c))?;
    if c.name == "form" {
        return Some(form_def(base.flags.area.default_form()));
    }
    Some(&c.def)
}

/// Companion `c`'s default on gradient `base`.
pub fn default_on(base: &SettingDef, c: &Companion) -> Value {
    if c.name == "form" {
        return Value::Choice(base.flags.area.default_form().name().to_owned());
    }
    c.def.default.clone()
}

/// The companions offered on gradient setting `base`, in order (none on anything else).
pub fn companions_for(base: &SettingDef) -> impl Iterator<Item = &'static Companion> + use<'_> {
    COMPANIONS.iter().filter(move |c| offers(base, c))
}

/// `when` with every term naming a companion (`~form=spiral`) turned into the full key of
/// gradient `base`'s companion (`3/colour~form=spiral`).
pub fn when_of(base: &str, when: &str) -> String {
    when.split(" or ")
        .map(|all| {
            all.split(" and ")
                .map(|term| {
                    let t = term.trim();
                    match t.strip_prefix('!') {
                        Some(rest) if rest.starts_with(SEP) => format!("!{base}{rest}"),
                        _ if t.starts_with(SEP) => format!("{base}{t}"),
                        _ => t.to_owned(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" and ")
        })
        .collect::<Vec<_>>()
        .join(" or ")
}

/// A companion key's default, when it is one (`~form`'s as Straight: the site's own default is
/// [`default_on`]'s).
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

/// Whether some gradient in `resolved` is laid out as Ripples: the frame then builds the
/// distance picture of its things.
pub fn any_ripples(resolved: &Resolved) -> bool {
    let ripples = Value::Choice(GradientForm::Ripples.name().to_owned());
    resolved.things.values().any(|t| {
        std::iter::once(&t.params)
            .chain(t.slots.values().map(|s| &s.params))
            .any(|p| p.iter().any(|(k, v)| *v == ripples && split(k).is_some_and(|(_, c)| c.name == "form")))
    })
}

/// Applies every gradient's companions to the gradient itself, once a frame before anything
/// draws, right after the animation (so keyed, pulsed and followed companions count): Bands cut
/// it into flat bands, Grain and the shape companions ([`Layout`]) are copied onto it for the
/// sites that draw it. A varied gradient's other end (`~to`) gets the same, so each ball's mix of
/// the two keeps them. Pure in its inputs: nothing drawn earlier is read. `time` is for the moves
/// of gradients part 3.
pub fn apply_moves(resolved: &mut Resolved, _time: f64) {
    for t in resolved.things.values_mut() {
        apply_in(&mut t.params);
        for s in t.slots.values_mut() {
            apply_in(&mut s.params);
        }
    }
}

/// What one gradient's stored companions say.
#[derive(Default)]
struct Found {
    bands: f64,
    grain: f64,
    layout: Layout,
    shaped: bool,
}

fn choice<C: Choice>(v: &Value) -> Option<C> {
    if let Value::Choice(n) = v { C::from_name(n) } else { None }
}

/// Puts companion `name`'s value `v` into `f`.
fn take(f: &mut Found, name: &str, v: &Value) {
    let x = number_of(v) as f32;
    let l = &mut f.layout;
    let vec2 = |v: &Value| if let Value::Vec2(p) = v { Some(*p) } else { None };
    match name {
        "bands" => f.bands = number_of(v),
        "grain" => f.grain = number_of(v),
        "form" => l.form = choice(v),
        "turn" => l.turn = Some(x),
        "centre" => l.centre = vec2(v),
        "size" => l.size = Some(x),
        "repeat" => l.repeat = choice(v),
        "sides" => l.sides = x,
        "star" => l.star = x,
        "arms" => l.arms = x,
        "twist" => l.twist = x,
        "mirrors" => l.mirrors = x,
        "focus" => l.focus = vec2(v).unwrap_or(defaults::FOCUS),
        "stretch" => l.stretch = x,
        "stretch_turn" => l.stretch_turn = x,
        "count" => l.count = x,
        "height" => l.height = x,
        "flow" => l.flow = x,
        "detail" => l.detail = x,
        "bend" => l.bend = x,
        "seed" => l.seed = x,
        "softness" => l.softness = x,
        "points" => l.points = if let Value::Points(p) = v { p.clone() } else { Vec::new() },
        "spacing" => l.spacing = x,
        "from" => l.from = choice(v).unwrap_or_default(),
        "speed" => l.speed = x,
        _ => {}
    }
    if !matches!(name, "bands" | "grain") {
        f.shaped = true;
    }
}

/// [`apply_moves`] on one set of params.
fn apply_in(params: &mut Params) {
    let mut found: Vec<(String, Found)> = Vec::new();
    for (k, v) in params.iter() {
        let Some((base, c)) = split(k) else { continue };
        let at = match found.iter().position(|(b, _)| b == base) {
            Some(i) => i,
            None => {
                found.push((base.to_owned(), Found::default()));
                found.len() - 1
            }
        };
        take(&mut found[at].1, c.name, v);
    }
    for (base, f) in found {
        let bands = f.bands.round().clamp(0.0, MOST_BANDS) as u32;
        for key in [base.clone(), crate::vary::to_key(&base)] {
            if let Some(Value::Gradient(g)) = params.get(&key) {
                let mut g: Gradient = if bands > 0 { g.banded(bands) } else { g.clone() };
                g.grain = f.grain as f32;
                if f.shaped {
                    g.layout = f.layout.clone();
                }
                params.set(&key, Value::Gradient(g));
            }
        }
    }
}

/// The layout gradient `base`'s Shape companions give, each read through `get` (its full key;
/// `None` for one not stored): what [`apply_moves`] puts on the gradient, for the editor's
/// previews.
pub fn layout_from(base: &str, get: &dyn Fn(&str) -> Option<Value>) -> Layout {
    let mut f = Found::default();
    for c in COMPANIONS.iter().filter(|c| c.tab == ExtraTab::Shape) {
        if let Some(v) = get(&key(base, c.name)) {
            take(&mut f, c.name, &v);
        }
    }
    f.layout
}

/// The form a gradient at a site whose default is `default` is laid out by this frame.
pub fn form_of(g: &Gradient, default: GradientForm) -> GradientForm {
    g.layout.form_or(default)
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
        assert_eq!(companions_for(gradient).count(), 2, "a gradient that doesn't fill an area: Blend only");
        assert_eq!(companions_for(&bands.def).count(), 0);
    }

    /// A gradient setting at a site of `area`.
    fn site(area: Area) -> &'static SettingDef {
        let bands = companion("bands").unwrap();
        let mut d = SettingDef {
            key: "colour",
            ty: ValueType::Gradient,
            default: Value::Gradient(Gradient::default()),
            ..bands.def.clone()
        };
        d.flags.area = area;
        Box::leak(Box::new(d))
    }

    #[test]
    fn shape_companions_are_offered_on_gradients_that_fill_an_area() {
        let names = |d: &SettingDef| companions_for(d).map(|c| c.name).collect::<Vec<_>>();
        let fills = names(site(Area::Fills(GradientForm::Straight)));
        for n in ["bands", "form", "turn", "centre", "size", "repeat", "sides", "spacing", "points"] {
            assert!(fills.contains(&n), "{n}");
        }
        let placed = names(site(Area::Placed(GradientForm::Straight)));
        assert!(placed.contains(&"form") && placed.contains(&"arms"));
        for n in ["turn", "centre", "size", "repeat"] {
            assert!(!placed.contains(&n), "a placed site has its own {n}");
        }
        assert!(!names(site(Area::No)).contains(&"form"));
        // The site's own default form.
        let stripes = site(Area::Fills(GradientForm::Stripes));
        let form = companion("form").unwrap();
        assert_eq!(def_for(Some(stripes), form).unwrap().default, Value::Choice("stripes".into()));
        assert_eq!(default_on(stripes, form), Value::Choice("stripes".into()));
        assert!(def_for(Some(site(Area::Placed(GradientForm::Straight))), companion("size").unwrap()).is_none());
    }

    #[test]
    fn a_companions_condition_names_its_gradients_companions() {
        assert_eq!(when_of("3/colour", "~form=spiral"), "3/colour~form=spiral");
        assert_eq!(when_of("c", "~form=a or x and !~flow"), "c~form=a or x and !c~flow");
        assert_eq!(when_of("c", ""), "");
        for c in COMPANIONS.iter() {
            for term in c.def.when.split(" or ").flat_map(|a| a.split(" and ")).filter(|t| !t.is_empty()) {
                let name = term.trim_start_matches('!').trim_start_matches(SEP);
                let name = name.split(['=', '!']).next().unwrap();
                assert!(companion(name).is_some(), "{}: {term}", c.name);
            }
            assert!(c.def.help.ends_with('.'), "{} needs a help sentence", c.name);
        }
    }

    #[test]
    fn the_pass_lays_the_gradient_out() {
        let mut p = Params::default();
        p.set("colours", Value::Gradient(two_colours()));
        p.set("colours~form", Value::Choice("spiral".into()));
        p.set("colours~arms", Value::Number(3.0));
        p.set("colours~turn", Value::Number(12.0));
        p.set("plain", Value::Gradient(two_colours()));
        p.set("plain~grain", Value::Number(1.0));
        apply_in(&mut p);
        let Some(Value::Gradient(g)) = p.get("colours") else { panic!() };
        assert_eq!(g.layout.form, Some(GradientForm::Spiral));
        assert_eq!((g.layout.arms, g.layout.turn, g.layout.size), (3.0, Some(12.0), None));
        let Some(Value::Gradient(plain)) = p.get("plain") else { panic!() };
        assert_eq!(plain.layout, Layout::default(), "grain alone leaves the layout");
        let mut r = Resolved::default();
        assert!(!any_ripples(&r));
        let mut t = crate::ResolvedThing {
            id: crate::ThingId::ROOT,
            kind: "scene".into(),
            params: Params::default(),
            slots: Default::default(),
            screen: false,
        };
        t.params.set("bg_colours~form", Value::Choice("ripples".into()));
        r.things.insert(crate::ThingId::ROOT, t);
        assert!(any_ripples(&r));
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
        apply_in(&mut p);
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
}
