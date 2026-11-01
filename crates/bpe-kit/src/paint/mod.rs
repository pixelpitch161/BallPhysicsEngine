//! Paint: one shared set of colour settings that any thing or module can embed. It holds a
//! gradient, what the gradient follows (a place, an order, a speed, hits, time...) and how it
//! repeats and scrolls. A `#[setting(paint, ...)]` field expands to these settings, each key
//! starting with the field's prefix.

use bpe_core::{
    Choice, Depth, Flags, Gradient, GroupOpts, Params, SettingDef, SettingGroup, SettingValue, Value, ValueType,
};
use glam::Vec2;

pub mod eval;
pub mod gpu;
pub mod harmony;
pub mod inside;
pub mod palettes;
pub mod vary;
pub use eval::{Inputs, beats_at, colour_at, hits_on, last_hit, last_hit_at, offset};
pub use gpu::{background_gpu, inside_gpu, paint_gpu};
pub use inside::{InsideFrom, InsidePaint, Pattern};
pub use vary::{apply_vary, seed_of, vary_gpu, vary_offsets};

/// What the colours follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Follow {
    #[default]
    Place,
    Start,
    Order,
    Speed,
    Hits,
    Life,
    Step,
    Beat,
    Time,
    NearestBall,
    Path,
}

/// Whether positions are in the world or on the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Space {
    #[default]
    World,
    Screen,
}

/// What happens past the end of the gradient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Repeat {
    #[default]
    Clamp,
    Repeat,
    Mirror,
}

/// A pattern drawn inside each shape on top of the colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Inside {
    #[default]
    None,
    Stripes,
    Rings,
    Sweep,
}

/// Where a place-based gradient runs from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PlaceShape {
    #[default]
    Direction,
    Centre,
}

/// A gradient and how it is applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    pub colours: Gradient,
    pub follow: Follow,
    pub shape: PlaceShape,
    pub angle: f32,
    pub centre: Vec2,
    pub space: Space,
    pub span: f32,
    pub repeat: Repeat,
    pub scroll: f32,
    pub take_hit_colour: bool,
    pub step_time: f32,
    pub inside: Inside,
    pub inside_angle: f32,
    pub inside_spin: f32,
    pub inside_scale: f32,
    pub inside_scroll: f32,
    /// How far each instance's hue turns from the paint's, in degrees either way.
    pub vary_hue: f32,
    /// How far each instance's saturation moves, either way.
    pub vary_sat: f32,
    /// How far each instance's brightness moves, as a share either way.
    pub vary_bright: f32,
    /// How fast each instance's variation drifts, in changes per second. 0 keeps it fixed.
    pub wobble: f32,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            colours: Gradient::solid(bpe_core::Color::WHITE),
            follow: Follow::Place,
            shape: PlaceShape::Direction,
            angle: 90.0,
            centre: Vec2::ZERO,
            space: Space::World,
            span: 1920.0,
            repeat: Repeat::Clamp,
            scroll: 0.0,
            take_hit_colour: false,
            step_time: 0.15,
            inside: Inside::None,
            inside_angle: 0.0,
            inside_spin: 0.0,
            inside_scale: 1.0,
            inside_scroll: 0.0,
            vary_hue: 0.0,
            vary_sat: 0.0,
            vary_bright: 0.0,
            wobble: 0.0,
        }
    }
}

impl Paint {
    /// One colour everywhere, the rest as a new paint has it.
    pub fn solid(c: bpe_core::Color) -> Self {
        Self { colours: Gradient::solid(c), ..Self::default() }
    }
}

/// Keeps a string for the rest of the program. Called once per outer type, because the
/// derived `defs()` runs once.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// `term` added to every alternative of `when` (which binds `and` tighter than `or`).
fn also(when: &str, term: &str) -> &'static str {
    if when.trim().is_empty() {
        return leak(term.to_owned());
    }
    leak(when.split(" or ").map(|alt| format!("{alt} and {term}")).collect::<Vec<_>>().join(" or "))
}

/// The choices a group may offer: its list, or all of them when it names none.
fn follow_names(o: &GroupOpts) -> &'static [&'static str] {
    if o.follows.is_empty() { Follow::NAMES } else { o.follows }
}

/// What a group follows when nothing is saved: Place, or the first listed name when the list
/// leaves Place out.
fn default_follow(o: &GroupOpts) -> Follow {
    for n in follow_names(o) {
        debug_assert!(Follow::from_name(n).is_some(), "\"{n}\" is not something colours can follow");
    }
    if follow_names(o).contains(&"place") {
        Follow::Place
    } else {
        follow_names(o).iter().find_map(|n| Follow::from_name(n)).unwrap_or_default()
    }
}

/// The paint a group starts with: the outer struct's own default for the field, with a follow
/// off the group's list replaced by the list's default.
fn group_default(o: &GroupOpts, default: &Paint) -> Paint {
    let follow = if follow_names(o).contains(&default.follow.name()) { default.follow } else { default_follow(o) };
    Paint { follow, ..default.clone() }
}

impl SettingGroup for Paint {
    fn group_defs(o: &GroupOpts, default: &Self) -> Vec<SettingDef> {
        let d = group_default(o, default);
        let p = o.prefix;
        let (c, f) = (format!("{p}colours"), format!("{p}follow"));
        let (shape, inside) = (format!("{p}shape"), format!("{p}inside"));
        let on = |extra: &str| also(o.when, &format!("{c}{extra}"));
        let def =
            |key: &str, label: &'static str, help: &'static str, ty: ValueType, default: Value, when: &'static str| {
                SettingDef {
                    key: leak(format!("{p}{key}")),
                    label,
                    help,
                    ty,
                    default,
                    range: None,
                    dice: None,
                    unit: "",
                    group: o.group,
                    depth: Depth::More,
                    flags: Flags::default(),
                    when,
                }
            };
        fn num(mut d: SettingDef, range: (f64, f64), unit: &'static str, keyframe: bool, random: bool) -> SettingDef {
            d.range = Some(range);
            d.unit = unit;
            d.flags.keyframe = keyframe;
            d.flags.random = random;
            d
        }
        fn ty<T: SettingValue>(_: &T) -> ValueType {
            T::value_type()
        }
        fn val<T: SettingValue>(v: &T) -> Value {
            v.to_value()
        }
        let mut v = Vec::new();

        let mut colours = def(
            "colours",
            o.label,
            "The colours, from the first stop to the last.",
            ty(&d.colours),
            val(&d.colours),
            o.when,
        );
        colours.flags.keyframe = true;
        colours.flags.random = true;
        if o.simple {
            colours.depth = Depth::Simple;
        }
        v.push(colours);

        let mut follow = def(
            "follow",
            "Colours follow",
            "What decides where along the colours each thing is painted.",
            ValueType::Choice(follow_names(o)),
            val(&d.follow),
            on(""),
        );
        follow.flags.random = true;
        v.push(follow);

        let near = on(&format!(" and {f}=place|start"));
        v.push(def(
            "shape",
            "Place from",
            "Whether the colours run along a direction or outward from a centre.",
            ty(&d.shape),
            val(&d.shape),
            near,
        ));
        v.push(num(
            def(
                "angle",
                "Direction",
                "The direction the colours run in, in degrees.",
                ty(&d.angle),
                val(&d.angle),
                on(&format!(" and {f}=place|start and {shape}=direction")),
            ),
            (-180.0, 180.0),
            "deg",
            true,
            true,
        ));
        let mut centre = def(
            "centre",
            "Centre",
            "The point the colours spread outward from.",
            ty(&d.centre),
            val(&d.centre),
            on(&format!(" and {f}=place|start and {shape}=centre")),
        );
        centre.flags.keyframe = true;
        v.push(centre);
        if !o.screen_only {
            v.push(def(
                "space",
                "Positions in",
                "Whether positions are measured in the world or on the screen.",
                ty(&d.space),
                val(&d.space),
                on(&format!(" and {f}=place")),
            ));
        }
        let mut span = num(
            def(
                "span",
                "Colours end at",
                "How far the colours stretch before they reach the last stop.",
                ty(&d.span),
                val(&d.span),
                on(&format!(" and {f}=place|start|speed|hits")),
            ),
            (1.0, 20000.0),
            "",
            true,
            true,
        );
        span.dice = Some((1.0, 2000.0));
        v.push(span);
        let mut repeat = def(
            "repeat",
            "Past the end",
            "What the colours do beyond the last stop.",
            ty(&d.repeat),
            val(&d.repeat),
            on(""),
        );
        repeat.flags.random = true;
        v.push(repeat);
        v.push(num(
            def(
                "scroll",
                "Scroll",
                "How fast the colours slide along, in cycles per second.",
                ty(&d.scroll),
                val(&d.scroll),
                on(""),
            ),
            (-4.0, 4.0),
            "cycles/s",
            true,
            true,
        ));
        v.push(def(
            "take_hit_colour",
            "Take hit colour",
            "Whether each step takes the colour of the hit that caused it.",
            ty(&d.take_hit_colour),
            val(&d.take_hit_colour),
            also(o.when, &format!("{f}=step")),
        ));
        v.push(num(
            def(
                "step_time",
                "Step time",
                "How long each step takes to blend into the next, in seconds.",
                ty(&d.step_time),
                val(&d.step_time),
                on(&format!(" and {f}=step")),
            ),
            (0.0, 2.0),
            "s",
            false,
            false,
        ));
        if o.inside {
            let mut pattern = def(
                "inside",
                "Inside pattern",
                "A pattern drawn inside each shape on top of the colours.",
                ty(&d.inside),
                val(&d.inside),
                on(""),
            );
            pattern.flags.random = true;
            v.push(pattern);
            let shown = on(&format!(" and {inside}!=none"));
            let mut more = |key: &str, label: &'static str, help: &'static str, default: f32, range, unit| {
                v.push(num(def(key, label, help, ty(&default), val(&default), shown), range, unit, true, false));
            };
            more(
                "inside_angle",
                "Pattern angle",
                "The angle of the inside pattern, in degrees.",
                d.inside_angle,
                (-180.0, 180.0),
                "deg",
            );
            more(
                "inside_spin",
                "Pattern spin",
                "How fast the inside pattern turns, in degrees per second.",
                d.inside_spin,
                (-720.0, 720.0),
                "deg/s",
            );
            more(
                "inside_scale",
                "Pattern size",
                "How many times the inside pattern's colours go by across the shape.",
                d.inside_scale,
                (0.1, 8.0),
                "",
            );
            more(
                "inside_scroll",
                "Pattern scroll",
                "How fast the inside pattern slides along, in cycles per second.",
                d.inside_scroll,
                (-4.0, 4.0),
                "cycles/s",
            );
        }
        if o.vary {
            // One paint's variation sits under "Variation"; a prefixed paint (an outline, a
            // pattern) gets its own heading so its rows can't be mistaken for the main one's.
            let heading = if p.is_empty() { "Variation" } else { leak(format!("{} variation", o.label)) };
            // Even a one-colour paint can vary, so this ignores the colour count.
            let shown = o.when;
            let mut vary = |key: &str, label: &'static str, help: &'static str, default: f32, range, unit| {
                let mut d = num(def(key, label, help, ty(&default), val(&default), shown), range, unit, true, false);
                d.group = heading;
                v.push(d);
            };
            vary(
                "vary_hue",
                "Vary hue",
                "How far each one's hue turns from the colours, in degrees either way.",
                d.vary_hue,
                (0.0, 180.0),
                "deg",
            );
            vary(
                "vary_sat",
                "Vary saturation",
                "How far each one's colour gets richer or greyer, either way.",
                d.vary_sat,
                (0.0, 1.0),
                "",
            );
            vary(
                "vary_bright",
                "Vary brightness",
                "How far each one gets brighter or darker, as a share either way.",
                d.vary_bright,
                (0.0, 1.0),
                "",
            );
            vary(
                "wobble",
                "Wobble",
                "How fast each one's variation drifts, in changes per second. 0 keeps it fixed.",
                d.wobble,
                (0.0, 5.0),
                "Hz",
            );
        }
        v
    }

    fn group_read(o: &GroupOpts, default: &Self, params: &Params) -> Self {
        let p = o.prefix;
        let get = |key: &str| params.get(&format!("{p}{key}"));
        fn set<T: SettingValue>(to: &mut T, v: Option<&Value>) {
            if let Some(x) = v.and_then(T::from_value) {
                *to = x;
            }
        }
        let mut s = group_default(o, default);
        set(&mut s.colours, get("colours"));
        if let Some(Value::Choice(n)) = get("follow")
            && follow_names(o).contains(&n.as_str())
            && let Some(f) = Follow::from_name(n)
        {
            s.follow = f;
        }
        set(&mut s.shape, get("shape"));
        set(&mut s.angle, get("angle"));
        set(&mut s.centre, get("centre"));
        if !o.screen_only {
            set(&mut s.space, get("space"));
        }
        set(&mut s.span, get("span"));
        set(&mut s.repeat, get("repeat"));
        set(&mut s.scroll, get("scroll"));
        set(&mut s.take_hit_colour, get("take_hit_colour"));
        set(&mut s.step_time, get("step_time"));
        if o.inside {
            set(&mut s.inside, get("inside"));
            set(&mut s.inside_angle, get("inside_angle"));
            set(&mut s.inside_spin, get("inside_spin"));
            set(&mut s.inside_scale, get("inside_scale"));
            set(&mut s.inside_scroll, get("inside_scroll"));
        }
        if o.vary {
            set(&mut s.vary_hue, get("vary_hue"));
            set(&mut s.vary_sat, get("vary_sat"));
            set(&mut s.vary_bright, get("vary_bright"));
            set(&mut s.wobble, get("wobble"));
        }
        s
    }

    fn group_write(&self, o: &GroupOpts, default: &Self, params: &mut Params) {
        let (p, d) = (o.prefix, group_default(o, default));
        let mut put = |key: &str, now: Value, was: Value| {
            if now != was {
                params.set(&format!("{p}{key}"), now);
            }
        };
        put("colours", val(&self.colours), val(&d.colours));
        // Never save a follow the list doesn't offer; it would read back as the default anyway.
        let follow = if follow_names(o).contains(&self.follow.name()) { self.follow } else { d.follow };
        put("follow", val(&follow), val(&d.follow));
        put("shape", val(&self.shape), val(&d.shape));
        put("angle", val(&self.angle), val(&d.angle));
        put("centre", val(&self.centre), val(&d.centre));
        if !o.screen_only {
            put("space", val(&self.space), val(&d.space));
        }
        put("span", val(&self.span), val(&d.span));
        put("repeat", val(&self.repeat), val(&d.repeat));
        put("scroll", val(&self.scroll), val(&d.scroll));
        put("take_hit_colour", val(&self.take_hit_colour), val(&d.take_hit_colour));
        put("step_time", val(&self.step_time), val(&d.step_time));
        if o.inside {
            put("inside", val(&self.inside), val(&d.inside));
            put("inside_angle", val(&self.inside_angle), val(&d.inside_angle));
            put("inside_spin", val(&self.inside_spin), val(&d.inside_spin));
            put("inside_scale", val(&self.inside_scale), val(&d.inside_scale));
            put("inside_scroll", val(&self.inside_scroll), val(&d.inside_scroll));
        }
        if o.vary {
            put("vary_hue", val(&self.vary_hue), val(&d.vary_hue));
            put("vary_sat", val(&self.vary_sat), val(&d.vary_sat));
            put("vary_bright", val(&self.vary_bright), val(&d.vary_bright));
            put("wobble", val(&self.wobble), val(&d.wobble));
        }
    }
}

fn val<T: SettingValue>(v: &T) -> Value {
    v.to_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, Settings};

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    #[derive(Clone, Debug, PartialEq, Default, Settings)]
    struct Two {
        #[setting(paint, label = "Colours", prefix = "", follows = "place|order|speed")]
        fill: Paint,
        #[setting(paint, label = "Outline colours", prefix = "outline_", follows = "place", inside)]
        outline: Paint,
    }

    #[test]
    fn a_paint_field_expands_to_prefixed_settings() {
        let keys: Vec<_> = Two::defs().iter().map(|d| d.key).collect();
        assert!(keys.contains(&"colours") && keys.contains(&"outline_colours") && keys.contains(&"outline_inside"));
        assert!(!keys.contains(&"inside"), "no inside pattern unless asked");
        let f = Two::defs().iter().find(|d| d.key == "follow").unwrap();
        assert_eq!(f.ty, ValueType::Choice(&["place", "order", "speed"]));
        let a = Two::defs().iter().find(|d| d.key == "outline_angle").unwrap();
        assert!(a.when.contains("outline_colours") && a.when.contains("outline_follow=place|start"));
    }

    #[test]
    fn paint_round_trips_through_params() {
        let mut t = Two::default();
        t.outline.colours = Gradient::even(&[RED, BLUE]);
        t.outline.angle = 30.0;
        t.outline.vary_hue = 25.0;
        t.outline.vary_sat = 0.25;
        t.outline.vary_bright = 0.5;
        t.outline.wobble = 1.5;
        t.fill.vary_bright = 0.75;
        let p = t.to_params();
        assert_eq!(p.get("outline_angle"), Some(&Value::Number(30.0)));
        for (key, want) in [
            ("outline_vary_hue", 25.0),
            ("outline_vary_sat", 0.25),
            ("outline_vary_bright", 0.5),
            ("outline_wobble", 1.5),
        ] {
            assert_eq!(p.get(key), Some(&Value::Number(want)), "{key}");
        }
        assert_eq!(p.get("vary_bright"), Some(&Value::Number(0.75)));
        assert!(p.get("angle").is_none(), "defaults are not saved");
        assert_eq!(Two::from_params(&p), t);
    }

    #[test]
    fn variation_settings_are_keyframed_and_grouped() {
        let defs = Two::defs();
        for (key, label, range, group) in [
            ("vary_hue", "Vary hue", (0.0, 180.0), "Variation"),
            ("vary_sat", "Vary saturation", (0.0, 1.0), "Variation"),
            ("vary_bright", "Vary brightness", (0.0, 1.0), "Variation"),
            ("wobble", "Wobble", (0.0, 5.0), "Variation"),
            ("outline_vary_hue", "Vary hue", (0.0, 180.0), "Outline colours variation"),
            ("outline_wobble", "Wobble", (0.0, 5.0), "Outline colours variation"),
        ] {
            let d = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{key} missing"));
            assert_eq!(d.label, label);
            assert_eq!(d.range, Some(range), "{key}");
            assert_eq!(d.group, group, "{key}");
            assert_eq!(d.default, Value::Number(0.0), "{key}");
            assert!(d.flags.keyframe, "{key}");
            assert_eq!(d.depth, Depth::More, "{key}");
        }
    }

    #[test]
    fn a_no_vary_group_has_no_variation_settings() {
        #[derive(Clone, Debug, PartialEq, Default, Settings)]
        struct Still {
            #[setting(paint, label = "Colours", prefix = "bg_", follows = "place|time", no_vary)]
            bg: Paint,
        }
        let keys: Vec<_> = Still::defs().iter().map(|d| d.key).collect();
        assert!(keys.contains(&"bg_colours"));
        for k in ["bg_vary_hue", "bg_vary_sat", "bg_vary_bright", "bg_wobble"] {
            assert!(!keys.contains(&k), "{k} is offered");
        }
        // Nothing is saved or read for them either.
        let mut s = Still::default();
        s.bg.vary_hue = 40.0;
        assert!(s.to_params().get("bg_vary_hue").is_none());
        let mut p = Params::default();
        p.set("bg_wobble", Value::Number(2.0));
        assert_eq!(Still::from_params(&p).bg.wobble, 0.0);
        // One fluid and one picture: the liquid look and the background don't vary.
        let liquid = crate::modules::look_liquid::LiquidLook::defs();
        let scene = crate::things::scene::SceneSettings::defs();
        assert!(liquid.iter().chain(scene).all(|d| !d.key.ends_with("vary_hue") && !d.key.ends_with("wobble")));
        // Every other look's paint does.
        assert!(crate::modules::look::ClassicLook::defs().iter().any(|d| d.key == "vary_hue"));
    }

    #[test]
    fn a_follow_outside_the_list_reads_as_default() {
        let mut p = Params::default();
        p.set("follow", Value::Choice("beat".into()));
        assert_eq!(Two::from_params(&p).fill.follow, Follow::Place);
    }

    #[test]
    fn the_field_s_own_condition_is_added_to_every_setting() {
        #[derive(Clone, Debug, PartialEq, Default, Settings)]
        struct Gated {
            #[setting(label = "Mode")]
            mode: Inside,
            #[setting(paint, label = "Colours", prefix = "p_", when = "mode=stripes or mode=rings", screen_only)]
            p: Paint,
        }
        let defs = Gated::defs();
        assert!(defs.iter().all(|d| d.key != "p_space"), "screen only has no space setting");
        let span = defs.iter().find(|d| d.key == "p_span").unwrap();
        assert_eq!(
            span.when,
            "mode=stripes and p_colours and p_follow=place|start|speed|hits or mode=rings and p_colours and p_follow=place|start|speed|hits"
        );
        let hit = defs.iter().find(|d| d.key == "p_take_hit_colour").unwrap();
        assert!(hit.when.contains("p_follow=step") && !hit.when.contains("p_colours"));
        assert!(defs.iter().find(|d| d.key == "p_colours").unwrap().when == "mode=stripes or mode=rings");
    }

    #[test]
    fn every_paint_setting_has_a_help_sentence() {
        for d in Two::defs() {
            assert!(d.help.ends_with('.'), "{} needs a help sentence", d.key);
        }
    }

    #[test]
    fn every_condition_names_a_setting_that_exists() {
        let defs = Two::defs();
        for d in defs {
            for term in d.when.split(" or ").flat_map(|all| all.split(" and ")).filter(|t| !t.is_empty()) {
                let key = term.split(['=', '!']).find(|k| !k.is_empty()).unwrap();
                assert!(defs.iter().any(|o| o.key == key), "{}: \"{key}\" is not a setting", d.key);
            }
        }
    }

    #[test]
    fn a_follows_list_without_place_defaults_to_its_first_name() {
        let o = GroupOpts {
            prefix: "",
            label: "Colours",
            group: "",
            when: "",
            follows: &["life", "order"],
            inside: false,
            screen_only: false,
            simple: false,
            vary: true,
        };
        // The outer default follows Place, which the list leaves out.
        let outer = Paint::default();
        let defs = Paint::group_defs(&o, &outer);
        let f = defs.iter().find(|d| d.key == "follow").unwrap();
        assert_eq!(f.default, Value::Choice("life".into()));
        let read = Paint::group_read(&o, &outer, &Params::default());
        assert_eq!(read.follow, Follow::Life);
        let mut p = Params::default();
        read.group_write(&o, &outer, &mut p);
        assert!(p.get("follow").is_none(), "the list's own default is not saved");
        let mut order = read;
        order.follow = Follow::Order;
        order.group_write(&o, &outer, &mut p);
        assert_eq!(p.get("follow"), Some(&Value::Choice("order".into())));
    }

    #[test]
    fn the_outer_default_is_the_groups_default() {
        #[derive(Clone, Debug, PartialEq, Settings)]
        struct Navy {
            #[setting(paint, label = "Colours", prefix = "p_", follows = "life|order", inside)]
            p: Paint,
        }
        impl Default for Navy {
            fn default() -> Self {
                let colours = Gradient::solid(Color::rgb(0x20, 0x20, 0x40));
                Self { p: Paint { colours, follow: Follow::Order, inside: Inside::Rings, ..Paint::default() } }
            }
        }
        let d = Navy::default();
        let colours = Navy::defs().iter().find(|s| s.key == "p_colours").unwrap();
        assert_eq!(colours.default, Value::Gradient(d.p.colours.clone()));
        let follow = Navy::defs().iter().find(|s| s.key == "p_follow").unwrap();
        assert_eq!(follow.default, Value::Choice("order".into()), "an outer follow on the list is kept");
        assert_eq!(Navy::from_params(&Params::default()), d);
        assert!(d.to_params().0.is_empty(), "the defaults are not saved");
        let mut white = d.clone();
        white.p.colours = Gradient::solid(Color::WHITE);
        assert_eq!(Navy::from_params(&white.to_params()), white);
    }

    #[test]
    fn a_list_without_place_round_trips_the_outer_default() {
        #[derive(Clone, Debug, PartialEq, Default, Settings)]
        struct Trail {
            #[setting(paint, label = "Colours", prefix = "", follows = "life|order")]
            fill: Paint,
        }
        let t = Trail::default();
        let p = t.to_params();
        assert!(p.get("follow").is_none(), "nothing off the list is written");
        assert_eq!(Trail::from_params(&p).fill.follow, Follow::Life);
        let mut t = t;
        t.fill.follow = Follow::Order;
        assert_eq!(Trail::from_params(&t.to_params()).fill.follow, Follow::Order);
    }
}
