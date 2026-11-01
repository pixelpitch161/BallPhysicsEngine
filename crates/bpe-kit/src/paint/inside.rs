//! InsidePaint: how colour is laid out inside each ball (a pattern, and either a shade of the
//! ball's own colour or a gradient of its own). Embedded with `#[setting(paint, ...)]`.

use super::{Repeat, also, leak, val};
use bpe_core::{
    Choice, Color, Depth, Flags, Gradient, GroupOpts, Params, SettingDef, SettingGroup, SettingValue, Value, ValueType,
};

/// The pattern drawn inside each ball.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Pattern {
    #[default]
    None,
    Linear,
    Radial,
    Sweep,
    Stripes,
    Rings,
    Dots,
    Checker,
    Halves,
    Slices,
    Swirl,
    Face,
}

/// Where the colours inside come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum InsideFrom {
    #[default]
    ShadeOfOwnColour,
    OwnGradient,
}

/// The colour layout inside each ball.
#[derive(Clone, Debug, PartialEq)]
pub struct InsidePaint {
    pub pattern: Pattern,
    pub from: InsideFrom,
    pub shade: f32,
    pub colours: Gradient,
    pub repeat: Repeat,
    pub angle: f32,
    pub spin: f32,
    pub size: f32,
    pub scroll: f32,
    pub turns: bool,
}

impl Default for InsidePaint {
    fn default() -> Self {
        Self {
            pattern: Pattern::None,
            from: InsideFrom::ShadeOfOwnColour,
            shade: -0.5,
            colours: Gradient::even(&[Color::WHITE, Color::rgb(0, 0, 0)]),
            repeat: Repeat::Clamp,
            angle: 90.0,
            spin: 0.0,
            size: 1.0,
            scroll: 0.0,
            turns: false,
        }
    }
}

impl SettingGroup for InsidePaint {
    fn group_defs(o: &GroupOpts, default: &Self) -> Vec<SettingDef> {
        let p = o.prefix;
        let (pat, from) = (format!("{p}pattern"), format!("{p}from"));
        let shown = also(o.when, &format!("{pat}!=none"));
        let shade_when = also(o.when, &format!("{pat}!=none and {from}=shade_of_own_colour"));
        let own_when = also(o.when, &format!("{pat}!=none and {from}=own_gradient"));
        let def = |key: &str,
                   label: &'static str,
                   help: &'static str,
                   ty: ValueType,
                   default: Value,
                   when: &'static str,
                   range: Option<(f64, f64)>,
                   unit: &'static str,
                   keyframe: bool| {
            let flags = Flags { keyframe, ..Flags::default() };
            SettingDef {
                key: leak(format!("{p}{key}")),
                label,
                help,
                ty,
                default,
                range,
                dice: None,
                unit,
                group: o.group,
                depth: Depth::More,
                flags,
                when,
            }
        };
        fn ty<T: SettingValue>(_: &T) -> ValueType {
            T::value_type()
        }
        let d = default;
        let num = |key, label, help, v: f32, when, range, unit| {
            def(key, label, help, ty(&v), val(&v), when, Some(range), unit, true)
        };
        vec![
            def(
                "pattern",
                "Pattern",
                "The pattern drawn inside each one.",
                ty(&d.pattern),
                val(&d.pattern),
                o.when,
                None,
                "",
                false,
            ),
            def(
                "from",
                "Inside colours",
                "Whether the pattern uses a shade of each one's own colour or a gradient of its own.",
                ty(&d.from),
                val(&d.from),
                shown,
                None,
                "",
                false,
            ),
            num(
                "shade",
                "Shade",
                "How much darker (below 0) or lighter (above 0) the pattern is than each one's colour.",
                d.shade,
                shade_when,
                (-1.0, 1.0),
                "",
            ),
            def(
                "colours",
                "Colours",
                "The pattern's own colours, from the first stop to the last.",
                ty(&d.colours),
                val(&d.colours),
                own_when,
                None,
                "",
                false,
            ),
            def(
                "repeat",
                "Beyond the last colour",
                "What the pattern's colours do beyond the last stop.",
                ty(&d.repeat),
                val(&d.repeat),
                own_when,
                None,
                "",
                false,
            ),
            num("angle", "Angle", "The angle of the pattern, in degrees.", d.angle, shown, (-180.0, 180.0), "deg"),
            num(
                "spin",
                "Spin",
                "How fast the pattern turns, in degrees per second.",
                d.spin,
                shown,
                (-720.0, 720.0),
                "deg/s",
            ),
            num(
                "size",
                "Size",
                "How big the pattern is; larger values repeat it more often.",
                d.size,
                shown,
                (0.1, 24.0),
                "",
            ),
            num(
                "scroll",
                "Scroll",
                "How fast the pattern slides along, in cycles per second.",
                d.scroll,
                shown,
                (-4.0, 4.0),
                "cycles/s",
            ),
            def(
                "turns",
                "Turns with the ball",
                "Whether the pattern turns as the ball does.",
                ty(&d.turns),
                val(&d.turns),
                shown,
                None,
                "",
                false,
            ),
        ]
    }

    fn group_read(o: &GroupOpts, default: &Self, params: &Params) -> Self {
        let p = o.prefix;
        let get = |key: &str| params.get(&format!("{p}{key}"));
        fn set<T: SettingValue>(to: &mut T, v: Option<&Value>) {
            if let Some(x) = v.and_then(T::from_value) {
                *to = x;
            }
        }
        let mut s = default.clone();
        set(&mut s.pattern, get("pattern"));
        set(&mut s.from, get("from"));
        set(&mut s.shade, get("shade"));
        set(&mut s.colours, get("colours"));
        set(&mut s.repeat, get("repeat"));
        set(&mut s.angle, get("angle"));
        set(&mut s.spin, get("spin"));
        set(&mut s.size, get("size"));
        set(&mut s.scroll, get("scroll"));
        set(&mut s.turns, get("turns"));
        s
    }

    fn group_write(&self, o: &GroupOpts, default: &Self, params: &mut Params) {
        let p = o.prefix;
        let mut put = |key: &str, now: Value, was: Value| {
            if now != was {
                params.set(&format!("{p}{key}"), now);
            }
        };
        put("pattern", val(&self.pattern), val(&default.pattern));
        put("from", val(&self.from), val(&default.from));
        put("shade", val(&self.shade), val(&default.shade));
        put("colours", val(&self.colours), val(&default.colours));
        put("repeat", val(&self.repeat), val(&default.repeat));
        put("angle", val(&self.angle), val(&default.angle));
        put("spin", val(&self.spin), val(&default.spin));
        put("size", val(&self.size), val(&default.size));
        put("scroll", val(&self.scroll), val(&default.scroll));
        put("turns", val(&self.turns), val(&default.turns));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Settings;

    #[derive(Clone, Debug, PartialEq, Default, Settings)]
    struct Holder {
        #[setting(paint, label = "Inside each one", prefix = "in_", group = "Inside each one")]
        inside: InsidePaint,
    }

    #[derive(Clone, Debug, PartialEq, Default, Settings)]
    struct Gated {
        #[setting(label = "X", range(0, 2))]
        x: f32,
        #[setting(paint, label = "Inside each one", prefix = "in_", group = "Inside each one", when = "x=1")]
        inside: InsidePaint,
    }

    #[test]
    fn the_fields_own_when_is_anded_into_every_row() {
        let defs = Gated::defs();
        let get = |k: &str| defs.iter().find(|d| d.key == k).unwrap().when;
        assert_eq!(get("in_pattern"), "x=1");
        assert_eq!(get("in_angle"), "x=1 and in_pattern!=none");
        assert_eq!(get("in_shade"), "x=1 and in_pattern!=none and in_from=shade_of_own_colour");
        assert_eq!(get("in_colours"), "x=1 and in_pattern!=none and in_from=own_gradient");
    }

    #[test]
    fn choice_names_read_as_the_required_labels() {
        let pretty = |n: &str| {
            let s = n.replace('_', " ");
            let mut c = s.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        let labels: Vec<_> = InsideFrom::NAMES.iter().map(|n| pretty(n)).collect();
        assert_eq!(labels, ["Shade of own colour", "Own gradient"]);
    }

    #[test]
    fn inside_rows_and_conditions() {
        let defs = Holder::defs();
        let n = "in_pattern!=none";
        #[allow(clippy::type_complexity)]
        let rows: [(&str, &str, &str, Option<(f64, f64)>, bool); 10] = [
            ("in_pattern", "Pattern", "", None, false),
            ("in_from", "Inside colours", n, None, false),
            ("in_shade", "Shade", "in_pattern!=none and in_from=shade_of_own_colour", Some((-1.0, 1.0)), true),
            ("in_colours", "Colours", "in_pattern!=none and in_from=own_gradient", None, false),
            ("in_repeat", "Beyond the last colour", "in_pattern!=none and in_from=own_gradient", None, false),
            ("in_angle", "Angle", n, Some((-180.0, 180.0)), true),
            ("in_spin", "Spin", n, Some((-720.0, 720.0)), true),
            ("in_size", "Size", n, Some((0.1, 24.0)), true),
            ("in_scroll", "Scroll", n, Some((-4.0, 4.0)), true),
            ("in_turns", "Turns with the ball", n, None, false),
        ];
        assert_eq!(defs.len(), rows.len());
        for (key, label, when, range, keyframe) in rows {
            let d = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{key} missing"));
            assert_eq!(d.label, label, "{key}");
            assert_eq!(d.when, when, "{key}");
            assert_eq!(d.range, range, "{key}");
            assert_eq!(d.flags.keyframe, keyframe, "{key}");
            assert_eq!(d.group, "Inside each one", "{key}");
            assert!(d.help.ends_with('.'), "{key}");
        }
        assert_eq!(defs.iter().find(|d| d.key == "in_shade").unwrap().default, Value::Number(-0.5));
        assert_eq!(defs.iter().find(|d| d.key == "in_pattern").unwrap().default, Value::Choice("none".into()));
    }

    #[test]
    fn inside_round_trips_and_saves_only_changes() {
        let mut h = Holder::default();
        assert!(h.to_params().get("in_pattern").is_none(), "defaults are not saved");
        h.inside.pattern = Pattern::Checker;
        h.inside.from = InsideFrom::OwnGradient;
        h.inside.shade = 0.25;
        h.inside.colours = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        h.inside.repeat = Repeat::Mirror;
        h.inside.angle = 30.0;
        h.inside.spin = 45.0;
        h.inside.size = 3.0;
        h.inside.scroll = 0.5;
        h.inside.turns = true;
        let p = h.to_params();
        assert_eq!(p.get("in_pattern"), Some(&Value::Choice("checker".into())));
        assert_eq!(p.get("in_from"), Some(&Value::Choice("own_gradient".into())));
        assert_eq!(Holder::from_params(&p), h);
        let mut only = Holder::default();
        only.inside.size = 2.0;
        let p = only.to_params();
        assert_eq!(p.get("in_size"), Some(&Value::Number(2.0)));
        assert!(p.get("in_angle").is_none() && p.get("in_pattern").is_none());
    }

    #[test]
    fn every_inside_condition_names_a_setting() {
        let defs = Holder::defs();
        for d in defs {
            for term in d.when.split(" or ").flat_map(|all| all.split(" and ")).filter(|t| !t.is_empty()) {
                let key = term.split(['=', '!']).find(|k| !k.is_empty()).unwrap();
                assert!(defs.iter().any(|o| o.key == key), "{}: \"{key}\" is not a setting", d.key);
            }
        }
    }
}
