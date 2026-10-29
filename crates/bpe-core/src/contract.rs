//! Checks every registered plugin against the core's rules, so a mistake in a plugin shows up in
//! tests instead of in the editor.

use crate::resolve::defaults;
use crate::{Depth, Registry, Scene, SettingDef, SlotId, Value, ValueType, resolve};
use std::collections::BTreeSet;

/// The most settings a kind may show at Simple depth, so the first view stays short.
pub const MAX_SIMPLE: usize = 6;

/// Every rule a plugin breaks, one line each naming the plugin and the setting. Empty when all pass.
pub fn check_registry(reg: &Registry) -> Vec<String> {
    let mut fails = Vec::new();
    if reg.thing("scene").is_none() {
        fails.push("registry: no \"scene\" thing kind is registered".into());
    }
    for kind in reg.things() {
        check_settings(kind.id(), kind.settings(), &mut fails);
        let slots: BTreeSet<SlotId> = kind.slots().iter().copied().collect();
        for s in kind.default_slots() {
            if !slots.contains(s) {
                fails.push(format!("{}: default slot {} is not one of its slots", kind.id(), s.name()));
            }
        }
    }
    for kind in reg.modules() {
        check_settings(kind.id(), kind.settings(), &mut fails);
        if let Some(graph) = kind.template()
            && bpe_graph::order(&graph).is_err()
        {
            fails.push(format!("{}: the template graph has a cycle", kind.id()));
        }
    }
    if reg.thing("scene").is_some() {
        for p in resolve(&Scene::empty(reg), reg).problems {
            fails.push(format!("scene: an empty scene has a problem: {p:?}"));
        }
    }
    fails
}

fn check_settings(id: &str, defs: &[SettingDef], fails: &mut Vec<String>) {
    let mut fail = |key: &str, what: String| fails.push(format!("{id}: setting \"{key}\": {what}"));
    let mut keys = BTreeSet::new();
    for d in defs {
        if !keys.insert(d.key) {
            fail(d.key, "the key is used twice".into());
        }
        if d.label.trim().is_empty() {
            fail(d.key, "the label is empty".into());
        }
        if !d.ty.accepts(&d.default) {
            fail(d.key, format!("the default {:?} is not a {:?}", d.default, d.ty));
        }
        if let Some((min, max)) = d.range {
            if min >= max {
                fail(d.key, format!("the range {min} to {max} is empty"));
            }
            match d.default {
                Value::Number(x) if x < min || x > max => {
                    fail(d.key, format!("the default {x} is outside the range {min} to {max}"))
                }
                Value::Int(x) if (x as f64) < min || (x as f64) > max => {
                    fail(d.key, format!("the default {x} is outside the range {min} to {max}"))
                }
                Value::Number(_) | Value::Int(_) => {}
                _ => fail(d.key, "only numbers can have a range".into()),
            }
        }
        if (d.flags.keyframe || d.flags.pulse)
            && !matches!(d.ty, ValueType::Number | ValueType::Int | ValueType::Vec2 | ValueType::Color)
        {
            fail(d.key, format!("a {:?} setting can't be keyframed or pulsed", d.ty));
        }
    }
    let simple = defs.iter().filter(|d| d.depth == Depth::Simple).count();
    if simple > MAX_SIMPLE {
        fails.push(format!("{id}: {simple} settings are Simple, at most {MAX_SIMPLE} may be"));
    }
    let all = defaults(defs);
    match ron::to_string(&all).ok().and_then(|text| ron::from_str::<crate::Params>(&text).ok()) {
        Some(back) if back == all => {}
        _ => fails.push(format!("{id}: the default settings don't survive saving and loading")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Settings, ThingKind, testkit};

    #[test]
    fn random_takes_an_optional_dice_range() {
        #[derive(Settings)]
        struct S {
            #[setting(label = "A", range(0.0, 100.0), random(5.0, 20.0))]
            a: f64,
            #[setting(label = "B", range(0.0, 1.0), random)]
            b: f64,
            #[setting(label = "C")]
            c: bool,
        }
        impl Default for S {
            fn default() -> Self {
                Self { a: 10.0, b: 0.5, c: false }
            }
        }
        let defs = S::defs();
        assert_eq!(defs[0].dice, Some((5.0, 20.0)));
        assert_eq!(defs[0].dice_range(), Some((5.0, 20.0)));
        assert_eq!(defs[1].dice, None);
        assert_eq!(defs[1].dice_range(), Some((0.0, 1.0)));
        assert_eq!(defs[2].dice_range(), None);
        assert!(defs[0].flags.random && defs[1].flags.random && !defs[2].flags.random);
    }

    #[test]
    fn testkit_plugins_pass() {
        assert_eq!(check_registry(&testkit::registry()), Vec::<String>::new());
    }

    #[test]
    fn a_default_outside_its_range_is_caught() {
        #[derive(Settings)]
        struct Bad {
            #[setting(label = "Size", range(1.0, 2.0))]
            size: f32,
        }
        impl Default for Bad {
            fn default() -> Self {
                Self { size: 9.0 }
            }
        }
        struct BadKind;
        impl ThingKind for BadKind {
            fn id(&self) -> &'static str {
                "bad"
            }
            fn name(&self) -> &'static str {
                "Bad"
            }
            fn icon(&self) -> &'static str {
                "bad"
            }
            fn help(&self) -> &'static str {
                "A kind with a wrong default."
            }
            fn settings(&self) -> &'static [SettingDef] {
                Bad::defs()
            }
            fn slots(&self) -> &'static [SlotId] {
                &[]
            }
            fn default_slots(&self) -> &'static [SlotId] {
                &[]
            }
        }
        let mut r = testkit::registry();
        r.add_thing(BadKind);
        let fails = check_registry(&r);
        assert_eq!(fails.len(), 1);
        assert!(fails[0].contains("bad") && fails[0].contains("size"));
    }

    #[test]
    fn a_registry_without_scene_defaults_is_caught() {
        let fails = check_registry(&Registry::new());
        assert_eq!(fails.len(), 1);
        assert!(fails[0].contains("scene"));
    }
}
